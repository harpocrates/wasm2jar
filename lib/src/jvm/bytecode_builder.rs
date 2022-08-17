use super::*;

use std::collections::{HashMap, HashSet};
use std::convert::TryFrom;
use crate::util::{OffsetVec, Width, Offset};
use crate::jvm::class_file::{Code, StackMapTable, BytecodeArray, Serialize};
use crate::jvm::model::{SynLabel, BasicBlock};
use crate::jvm::verifier::*;
use elsa::FrozenVec;

/*

Deciding when you need `goto_w`
===============================

Solution: use an iterative approach. Start by not using `goto` at all, then after all code is
emitted:

  i.   scan the method from top to bottom for jumps which overflow the `i16` offset and need `goto_w`
  ii.  replace those with `goto_w` (or in some cases emit extra jumps)
  iii. repeat until all jumps are within bounds

Notes:

  - this is iterative because sometimes (albeit rarely) code introduced around one `goto_w` can
    cause another offset to fall over the `i16` threshold
  - backwards jumps can be turned into `goto_w` in the construction phase (since the offset is
    known)

*/

/// This provides a very slightly simplified interface for building up method bodies. It does
/// internal bookeeping to track frames, labels, reachability, etc.
///
/// ### Constructing verification frames
///
/// Normally, figuring out what the right frame types are is a fixpoint iterative process, since
/// blocks jumping to the same frame need to have their output frames merged and then that
/// information must be propagated further backwards through the CFG. We avoid this and instead
/// compute our final frames right from the start. The tradeoff here is that instead of merging
/// frames, we require the frames be completely identical. In practice, this doesn't matter to us
/// because we don't leverage subtyping much in our codegen (and this simplfication just means we
/// might reject code which isn't incorrect - not that we accept incorrect code).
///
/// ### Tracking reachability
///
/// Control flow translation is one of the more involved bits of WASM (which uses structured
/// constructs like loops, ifs, blocks, breaks, etc.) and the JVM verifier doesn't detect and
/// ignore dead bytecode; it still wants stackmaps for it. Other libraries take the approach of
/// doing a followup pass over the bytecode to either remove the dead code or replace it with a
/// `nop`* `athrow` sequence, but that would lead to lots of wasted bytes for us. Instead, we
/// enforce that labels cannot be placed unless they are reachable (either with a fall-through from
/// above, or there has already been a jump to the label). This is also important for the sake of
/// always being able to find the initial frame of the block.
pub struct BytecodeBuilder<'a, 'g> {
    /// Method code under construction
    code: crate::jvm::model::Code<'g>,

    /// Labels which have been referenced in blocks so far, but not placed yet (keys do not overlap
    /// with keys of `block`)
    unplaced_labels: HashMap<SynLabel, VerifierFrame<'g>>,

    /// Offset of the end of the last block in `code.blocks`
    blocks_end_offset: Offset,

    /// Block currently under construction (label is not in `blocks` _or_ `unplaced_labels`)
    current_block: Option<CurrentBlock<'g>>,

    /// Next label
    next_label: SynLabel,

    /// Class graph
    pub class_graph: &'g ClassGraph<'g>,

    /// Java library references
    pub java: &'g JavaLibrary<'g>,

    /// Constants pool
    pub constants_pool: &'a ConstantsPool,

    /// Bootstrap methods
    pub bootstrap_methods: &'a FrozenVec<Box<BootstrapMethodData<'g>>>,

    /// Reference to method data in the class graph
    pub method: &'g MethodData<'g>,
}

impl<'a, 'g> BytecodeBuilder<'a, 'g> {
    /// Create a builder for a new method
    pub fn new(
        class_graph: &'g ClassGraph<'g>,
        java: &'g JavaLibrary<'g>,
        constants_pool: &'a ConstantsPool,
        bootstrap_methods: &'a FrozenVec<Box<BootstrapMethodData<'g>>>,
        method: &'g MethodData<'g>,
    ) -> Self {
        // The initial local variables are just the parameters (including maybe "this")
        let mut locals = OffsetVec::new();
        if method.name == UnqualifiedName::INIT {
            locals.push(VerificationType::UninitializedThis);
        } else if !method.is_static() {
            locals.push(VerificationType::Object(RefType::Object(method.class)));
        }
        for arg_type in &method.descriptor.parameters {
            locals.push(VerificationType::from(*arg_type));
        }

        let max_locals = locals.offset_len();
        let entry_frame = Frame {
            locals,
            stack: OffsetVec::new(),
        };

        let code = crate::jvm::model::Code {
            max_locals,
            max_stack: Offset(0),
            blocks: HashMap::new(),
            block_order: vec![],
        };

        BytecodeBuilder {
            code,
            unplaced_labels: HashMap::new(),
            blocks_end_offset: Offset(0),
            current_block: Some(CurrentBlock::new(SynLabel::START, entry_frame)),
            next_label: SynLabel::START.next(),
            class_graph,
            java,
            constants_pool,
            bootstrap_methods,
            method,
        }
    }

    /// Turn the builder into the final code attribute (with a stack map table attribute on it)
    pub fn result(mut self) -> Result<Code, Error> {
        // Weed out some error cases early
        if self.current_block.is_some() || !self.unplaced_labels.is_empty() {
            return Err(Error::MethodCodeNotFinished {
                pending_block: self
                    .current_block
                    .as_ref()
                    .map(|current_block| current_block.label),
                unplaced_labels: self.unplaced_labels.keys().cloned().collect(),
            });
        }

        if let Err(_) = u16::try_from(self.blocks_end_offset.0) {
            return Err(Error::MethodCodeOverflow(self.blocks_end_offset));
        }

        // Convert max locals and stack
        let max_locals: u16 = match u16::try_from(self.code.max_locals.0) {
            Ok(max_locals) => max_locals,
            Err(_) => return Err(Error::MethodCodeMaxStackOverflow(self.code.max_locals)),
        };
        let max_stack: u16 = match u16::try_from(self.code.max_stack.0) {
            Ok(max_stack) => max_stack,
            Err(_) => return Err(Error::MethodCodeMaxStackOverflow(self.code.max_stack)),
        };

        // Extract a mapping of label to offset and labels used (our next iteration is destructive)
        let mut jump_targets: HashSet<SynLabel> = HashSet::new();
        let mut label_offsets: HashMap<SynLabel, Offset> = HashMap::new();
        for (block_label, basic_block) in &self.code.blocks {
            label_offsets.insert(*block_label, basic_block.offset_from_start);
            for jump_target in basic_block.branch_end.jump_targets().targets() {
                jump_targets.insert(*jump_target);
            }
        }
        let jump_targets = jump_targets;
        let label_offsets = label_offsets;

        // Loop through the blocks in placement order to accumulate code and frames
        let mut code_array: BytecodeArray = BytecodeArray(vec![]);
        let implicit_frame: Frame<ClassConstantIndex, u16> = self.code.blocks[&SynLabel::START]
            .frame
            .into_serializable(&self.constants_pool, Offset(0))?;
        let mut frames: Vec<(Offset, Frame<ClassConstantIndex, u16>)> = vec![];
        let mut fallthrough_label: Option<SynLabel> = None;

        for block_label in &self.code.block_order {
            if let Some(fallthrough_label) = fallthrough_label.take() {
                assert_eq!(
                    fallthrough_label, *block_label,
                    "fallthrough does not match next block"
                );
            }
            let basic_block = self.code.blocks.remove(block_label).expect("missing block");

            // If this block is ever jumped to, construct a stack map frame for it
            if jump_targets.contains(&block_label) {
                frames.push((
                    basic_block.offset_from_start,
                    basic_block
                        .frame
                        .into_serializable(&self.constants_pool, basic_block.offset_from_start)?,
                ));
            }

            // Serialize the instructions in the block to the bytecode array
            for (_, _, insn) in basic_block.instructions.iter() {
                insn.serialize(&mut code_array.0).map_err(Error::IoError)?;
            }
            let branch_end_offset: i64 =
                (basic_block.offset_from_start.0 + basic_block.instructions.offset_len().0) as i64;
            let end_insn = basic_block.branch_end.map_labels(
                |lbl: &SynLabel| {
                    i16::try_from(label_offsets[lbl].0 as i64 - branch_end_offset)
                        .expect("regular jump overflow")
                },
                |lbl: &SynLabel| {
                    i32::try_from(label_offsets[lbl].0 as i64 - branch_end_offset)
                        .expect("wide jump overflow")
                },
                |_| (),
            );
            end_insn
                .serialize(&mut code_array.0)
                .map_err(Error::IoError)?;

            fallthrough_label = basic_block.branch_end.fallthrough_target();
        }
        assert_eq!(
            fallthrough_label, None,
            "method cannot end with fallthrough label"
        );

        // Build up stack map frames
        let mut previous_frame = implicit_frame;
        let mut previous_offset = Offset(0);
        let mut stack_map_frames = vec![];
        for (offset, frame) in frames {
            let offset_delta = if stack_map_frames.is_empty() {
                offset.0 - previous_offset.0
            } else if offset == previous_offset {
                if frame != previous_frame {
                    return Err(Error::ConflictingFrames(offset, frame, previous_frame));
                } else {
                    continue;
                }
            } else {
                offset.0 - previous_offset.0 - 1
            };
            stack_map_frames.push(frame.stack_map_frame(offset_delta as u16, &previous_frame));

            previous_frame = frame;
            previous_offset = offset;
        }

        let mut attributes = vec![];

        // Add `StackMapTable` attribute only if there are frames
        if !stack_map_frames.is_empty() {
            let stack_map_table = StackMapTable(stack_map_frames);
            attributes.push(self.constants_pool.get_attribute(stack_map_table)?);
        }

        Ok(Code {
            max_stack,
            max_locals,
            code_array,
            exception_table: vec![],
            attributes,
        })
    }

    /// Query the expected frame for a label that has already been referred to and possibly even
    /// jumped to
    pub fn lookup_frame(&self, label: SynLabel) -> Option<&VerifierFrame<'g>> {
        // The block is already placed
        if let Some(basic_block) = self.code.blocks.get(&label) {
            return Some(&basic_block.frame);
        }

        // The block is only referred to
        if let Some(frame) = self.unplaced_labels.get(&label) {
            return Some(&frame);
        }

        // The block is the one we are currently processing
        if let Some(current_block) = self.current_block.as_ref().filter(|b| b.label == label) {
            return Some(&current_block.entry_frame);
        }

        None
    }

    /// Check that the label has a certain frame. If the frame is already being tracked, we can
    /// assert that the frames match. Otherwise, we start tracking the frame (so the next time it
    /// is placed or used, we'll be able to compare frames).
    ///
    /// ### Annoying edge case
    ///
    /// Sometimes we call `assert_frame_for_label` after we've taken the current block out, but
    /// before we've added it back into the general blocks map. This is problematic because it
    /// means that we won't find the current block's frame anywhere (this matters when closing the
    /// current block with a jump back to the start of the block). The work around is to specify
    /// that block in `extra_block`: we'll check that first and skip the other check/update if the
    /// extra block's label matches the assertion label.
    fn assert_frame_for_label(
        &mut self,
        label: SynLabel,
        expected: &VerifierFrame<'g>,
        extra_block: Option<(SynLabel, &VerifierFrame<'g>)>,
    ) -> Result<(), Error> {
        // Annoying edge case
        match extra_block {
            Some((extra_block_label, found)) if extra_block_label == label => {
                if found != expected {
                    return Err(Error::IncompatibleFrames(
                        label,
                        found.into_printable(),
                        expected.into_printable(),
                    ));
                } else {
                    return Ok(());
                }
            }
            _ => (),
        }

        if let Some(found) = self.lookup_frame(label) {
            if found != expected {
                Err(Error::IncompatibleFrames(
                    label,
                    found.into_printable(),
                    expected.into_printable(),
                ))
            } else {
                Ok(())
            }
        } else {
            let _ = self.unplaced_labels.insert(label, expected.clone());
            Ok(())
        }
    }

    /// Generate a fresh label
    pub fn fresh_label(&mut self) -> SynLabel {
        let to_return = self.next_label;
        self.next_label = self.next_label.next();
        to_return
    }

    /// Push a new instruction to the current block
    pub fn push_instruction(&mut self, insn: VerifierInstruction<'g>) -> Result<(), Error> {
        if let Some(current_block) = self.current_block.as_mut() {
            current_block
                .latest_frame
                .verify_instruction(
                    &insn,
                    current_block.instructions.offset_len(),
                    &self.java.classes,
                    &RefType::Object(self.method.class),
                )
                .map_err(|kind| Error::VerifierError {
                    instruction: format!("{:?}", insn),
                    kind,
                })?;
            current_block
                .latest_frame
                .update_maximums(&mut self.code.max_locals, &mut self.code.max_stack);

            // Resolve field, method, etc. references into constant pool indices
            let constants = self.constants_pool;
            let bootstrap_methods = self.bootstrap_methods;
            let insn = insn.map(
                |class| -> Result<ClassConstantIndex, Error> {
                    Ok(class.constant_index(constants)?)
                },
                |constant| -> Result<ConstantIndex, Error> {
                    Ok(constant.constant_index(constants)?)
                },
                |field| -> Result<FieldRefConstantIndex, Error> {
                    Ok(field.constant_index(constants)?)
                },
                |method| -> Result<MethodRefConstantIndex, Error> {
                    Ok(method.constant_index(constants)?)
                },
                |indy_method| -> Result<InvokeDynamicConstantIndex, Error> {
                    let bootstrap_method = bootstrap_methods
                        .iter()
                        .position(|bm| bm == indy_method.bootstrap)
                        .unwrap_or_else(|| {
                            bootstrap_methods.push(Box::new(indy_method.bootstrap.clone()));
                            bootstrap_methods.len() - 1
                        });
                    let method_utf8 = constants.get_utf8(indy_method.name.as_str())?;
                    let desc_utf8 = constants.get_utf8(&indy_method.descriptor.render())?;
                    let name_and_type_idx = constants.get_name_and_type(method_utf8, desc_utf8)?;
                    Ok(constants.get_invoke_dynamic(bootstrap_method as u16, name_and_type_idx)?)
                },
            )?;

            current_block.extend_block(insn)?;
        }
        Ok(())
    }

    /// Push a new branch instruction to close the current block and possibly open a new one
    pub fn push_branch_instruction(
        &mut self,
        insn: BranchInstruction<SynLabel, SynLabel, ()>,
    ) -> Result<(), Error> {
        if let Some(mut current_block) = self.current_block.take() {
            current_block
                .latest_frame
                .verify_branch_instruction(&insn, &self.method.descriptor.return_type)
                .map_err(|kind| Error::VerifierBranchingError {
                    instruction: insn.clone(),
                    kind,
                })?;
            current_block
                .latest_frame
                .update_maximums(&mut self.code.max_locals, &mut self.code.max_stack);

            // Check that the jump target (if there is one) has a compatible frame
            for jump_label in insn.jump_targets().targets() {
                self.assert_frame_for_label(
                    *jump_label,
                    &current_block.latest_frame,
                    Some((current_block.label, &current_block.entry_frame)),
                )?;
            }

            // Turn the current block into a regular block, possibly open the next current block
            let (block_label, basic_block, next_curr_block_opt) = current_block.close_block(
                self.blocks_end_offset,
                insn.map_labels(|lbl| *lbl, |lbl| *lbl, |()| self.fresh_label()),
            )?;

            // Update all the local state in the builder
            self.blocks_end_offset.0 += basic_block.width();
            self.code.block_order.push(block_label);
            self.current_block = next_curr_block_opt;
            if let Some(_) = self.code.blocks.insert(block_label, basic_block) {
                return Err(Error::DuplicateLabel(block_label));
            }
        }
        Ok(())
    }

    /// Start a new block with the given label, ending the current block (if there is one) with a
    /// fallthrough. This can fail if:
    ///
    ///   * the label was already placed
    ///   * the label was already jumped to from elsewhere, and the frames don't match
    ///   * the label was not ever been jumped to and there is no fallthrough (so we have no way of
    ///     inferring the expected frame)
    ///
    pub fn place_label(&mut self, label: SynLabel) -> Result<(), Error> {
        if let Some(mut current_block) = self.current_block.take() {
            let fall_through_insn = BranchInstruction::FallThrough(label);
            current_block
                .latest_frame
                .verify_branch_instruction(
                    &fall_through_insn,
                    &self.method.descriptor.return_type,
                )
                .map_err(|kind| Error::VerifierBranchingError {
                    instruction: fall_through_insn.map_labels(|lbl| *lbl, |lbl| *lbl, |_| ()),
                    kind,
                })?;

            // Check that the jump target (if there is one) has a compatible frame
            self.assert_frame_for_label(
                label,
                &current_block.latest_frame,
                Some((current_block.label, &current_block.entry_frame)),
            )?;

            // Turn the current block into a regular block, possibly open the next current block
            let (block_label, basic_block, next_curr_block_opt) =
                current_block.close_block(self.blocks_end_offset, fall_through_insn)?;

            // Update all the local state in the builder
            let _ = self.unplaced_labels.remove(&label);
            self.blocks_end_offset.0 += basic_block.width();
            self.code.block_order.push(block_label);
            self.current_block = next_curr_block_opt;
            if let Some(_) = self.code.blocks.insert(block_label, basic_block) {
                return Err(Error::DuplicateLabel(block_label));
            }
        } else {
            // Find the frame
            let frame: VerifierFrame<'g> = self
                .unplaced_labels
                .remove(&label)
                .ok_or(Error::PlacingLabelBeforeReference(label))?;

            self.current_block = Some(CurrentBlock::new(label, frame));
        }

        Ok(())
    }

    /// Like `place_label`, but specifies an explicit frame. This rules out the failure mode of
    /// `place_label` for when there is no way of inferring the expected frame.
    ///
    /// TODO: switch `frame` to take a `Cow` (we often have the frame owned)
    pub fn place_label_with_frame(
        &mut self,
        label: SynLabel,
        frame: &VerifierFrame<'g>,
    ) -> Result<(), Error> {
        self.assert_frame_for_label(label, frame, None)?;
        self.place_label(label)
    }

    /// Get the current frame
    pub fn current_frame(&self) -> Option<&VerifierFrame<'g>> {
        self.current_block
            .as_ref()
            .map(|current_block| &current_block.latest_frame)
    }

    /// Generalize the type of the top value on the stack.
    ///
    /// If the top-most local is not a reference type or the specied type is not more general, this
    /// will result in a verifier error. Otherwise, it will set the top of the stack to the
    /// specified more general type.
    pub fn generalize_top_stack_type(
        &mut self,
        general_type: RefType<&'g ClassData<'g>>,
    ) -> Result<(), Error> {
        if let Some(current_block) = self.current_block.as_mut() {
            current_block
                .latest_frame
                .generalize_top_stack_type(general_type)
                .map_err(|kind| Error::VerifierError {
                    instruction: format!("Hinting at more general type: {:?}", general_type),
                    kind,
                })
        } else {
            Ok(())
        }
    }

    /// Kill a local variable
    pub fn kill_top_local(
        &mut self,
        offset: u16,
        local_type_assertion: Option<FieldType<&'g ClassData<'g>>>,
    ) -> Result<(), Error> {
        if let Some(current_block) = self.current_block.as_mut() {
            current_block
                .latest_frame
                .kill_top_local(offset, local_type_assertion)
                .map_err(|kind| Error::VerifierError {
                    instruction: format!("Kill local (at offset {}): {:?}", offset, local_type_assertion),
                    kind,
                })
        } else {
            Ok(())
        }
    }

}

/// Just like `BasicBlock`, but not closed off yet
struct CurrentBlock<'g> {
    pub label: SynLabel,

    /// State of the frame at the start of `instructions`
    pub entry_frame: VerifierFrame<'g>,

    /// Tracks the state of the frame at the end of `instructions`
    pub latest_frame: VerifierFrame<'g>,

    /// Accumulated instructions
    pub instructions: OffsetVec<SerializableInstruction>,
}

impl<'g> CurrentBlock<'g> {
    /// New block starting with a given frame
    pub fn new(label: SynLabel, entry_frame: VerifierFrame<'g>) -> CurrentBlock<'g> {
        CurrentBlock {
            label,
            latest_frame: entry_frame.clone(),
            entry_frame,
            instructions: OffsetVec::new(),
        }
    }

    /// Seal the current block into a basic block
    pub fn close_block(
        self,
        offset_from_start: Offset,
        branch_end: BranchInstruction<SynLabel, SynLabel, SynLabel>,
    ) -> Result<
        (
            SynLabel,
            BasicBlock<'g>,
            Option<CurrentBlock<'g>>,
        ),
        Error,
    > {
        let fallthrough_target: Option<SynLabel> = branch_end.fallthrough_target();

        // Adjust padding
        let branch_end = match branch_end {
            BranchInstruction::TableSwitch {
                default,
                low,
                targets,
                ..
            } => {
                let off = offset_from_start.0 + self.instructions.offset_len().0 + 1;
                let padding = match (off % 4) as u8 {
                    0 => 0,
                    x => 4 - x,
                };
                BranchInstruction::TableSwitch {
                    padding,
                    default,
                    low,
                    targets,
                }
            }
            BranchInstruction::LookupSwitch {
                default, targets, ..
            } => {
                let off = offset_from_start.0 + self.instructions.offset_len().0 + 1;
                let padding = match (off % 4) as u8 {
                    0 => 0,
                    x => 4 - x,
                };
                BranchInstruction::LookupSwitch {
                    padding,
                    default,
                    targets,
                }
            }

            other => other,
        };

        let basic_block = BasicBlock {
            offset_from_start,
            frame: self.entry_frame,
            instructions: self.instructions,
            branch_end,
        };

        // Construct a next current block only if there is a fall-through
        let next_block = if let Some(label) = fallthrough_target {
            let current_block = CurrentBlock {
                label,
                entry_frame: self.latest_frame.clone(),
                latest_frame: self.latest_frame,
                instructions: OffsetVec::new(),
            };
            Some(current_block)
        } else {
            None
        };

        Ok((self.label, basic_block, next_block))
    }

    pub fn extend_block(&mut self, insn: SerializableInstruction) -> Result<(), Error> {
        self.instructions.push(insn);

        Ok(())
    }
}

