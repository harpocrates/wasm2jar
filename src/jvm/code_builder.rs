use super::*;

use std::cell::{RefCell, RefMut};
use std::collections::{HashMap, HashSet};
use std::convert::TryFrom;
use std::fmt;
use std::rc::Rc;

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
pub struct CodeBuilder {
    /// This method signature
    descriptor: MethodDescriptor,

    /// Basic blocks in the code
    blocks: HashMap<SynLabel, BasicBlock<SynLabel, SynLabel, SynLabel>>,

    /// Order of basic blocks in the code (elements are unique and exactly match keys of `blocks`)
    block_order: Vec<SynLabel>,

    /// Labels which have been referenced in blocks so far, but not placed yet (keys do not overlap
    /// with keys of `block`)
    unplaced_labels: HashMap<SynLabel, Frame<RefType, (RefType, Offset)>>,

    /// Offset of the end of the last block in `blocks`
    blocks_end_offset: Offset,

    /// Block currently under construction (label is not in `blocks` _or_ `unplaced_labels`)
    current_block: Option<CurrentBlock>,

    /// Maximum size of locals seen so far
    max_locals: Offset,

    /// Maximum size of stack seen so far
    max_stack: Offset,

    /// Next label
    next_label: SynLabel,

    /// Class graph
    class_graph: Rc<RefCell<ClassGraph>>,

    /// Constants pool
    constants_pool: Rc<RefCell<ConstantsPool>>,

    /// Enclosing type
    this_type: RefType,
}

impl CodeBuilder {
    /// Create a builder for a new method
    pub fn new(
        descriptor: MethodDescriptor,
        is_instance_method: bool,
        is_constructor: bool,
        class_graph: Rc<RefCell<ClassGraph>>,
        constants_pool: Rc<RefCell<ConstantsPool>>,
        this_type: RefType,
    ) -> CodeBuilder {
        // The initial local variables are just the parameters (including maybe "this")
        let mut locals = OffsetVec::new();
        if is_constructor {
            locals.push(VerificationType::UninitializedThis);
        } else if is_instance_method {
            locals.push(VerificationType::Object(this_type.clone()));
        }
        for arg_type in &descriptor.parameters {
            locals.push(VerificationType::from(arg_type.clone()));
        }

        let max_locals = locals.offset_len();
        let entry_frame = Frame {
            locals,
            stack: OffsetVec::new(),
        };

        CodeBuilder {
            descriptor,
            blocks: HashMap::new(),
            block_order: vec![SynLabel::START],
            unplaced_labels: HashMap::new(),
            blocks_end_offset: Offset(0),
            current_block: Some(CurrentBlock::new(SynLabel::START, entry_frame)),
            max_stack: Offset(0),
            max_locals,
            next_label: SynLabel::START.next(),
            class_graph,
            constants_pool,
            this_type,
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
        let max_locals: u16 = match u16::try_from(self.max_locals.0) {
            Ok(max_locals) => max_locals,
            Err(_) => return Err(Error::MethodCodeMaxStackOverflow(self.max_locals)),
        };
        let max_stack: u16 = match u16::try_from(self.max_stack.0) {
            Ok(max_stack) => max_stack,
            Err(_) => return Err(Error::MethodCodeMaxStackOverflow(self.max_stack)),
        };

        // Extract a mapping of label to offset and labels used (our next iteration is destructive)
        let mut jump_targets: HashSet<SynLabel> = HashSet::new();
        let mut label_offsets: HashMap<SynLabel, Offset> = HashMap::new();
        for (block_label, basic_block) in &self.blocks {
            label_offsets.insert(*block_label, basic_block.offset_from_start);
            if let Some(jump_target) = basic_block.branch_end.jump_target() {
                jump_targets.insert(jump_target.merge());
            }
        }
        let jump_targets = jump_targets;
        let label_offsets = label_offsets;

        // Loop through the blocks in placement order to accumulate code and frames
        let mut code_array: BytecodeArray = BytecodeArray(vec![]);
        let implicit_frame: Frame<ClassConstantIndex, u16> = self.blocks[&SynLabel::START]
            .frame
            .into_serializable(&mut self.constants_pool.borrow_mut(), Offset(0))?;
        let mut frames: Vec<(Offset, Frame<ClassConstantIndex, u16>)> = vec![];
        let mut fallthrough_label: Option<SynLabel> = None;
        for block_label in &self.block_order {
            if let Some(fallthrough_label) = fallthrough_label.take() {
                assert_eq!(
                    fallthrough_label, *block_label,
                    "fallthrough does not match next block"
                );
            }
            let basic_block = self.blocks.remove(block_label).expect("missing block");

            // Guard against empty blocks (they will cause pain when we get to stack map tables)
            if basic_block.width() == 0 {
                return Err(Error::EmptyBlock(basic_block))?;
            }

            // If this block is ever jumped to, construct a stack map frame for it
            if jump_targets.contains(&block_label) {
                frames.push((
                    basic_block.offset_from_start,
                    basic_block.frame.into_serializable(
                        &mut self.constants_pool.borrow_mut(),
                        basic_block.offset_from_start,
                    )?,
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
            attributes.push(
                self.constants_pool
                    .borrow_mut()
                    .get_attribute(stack_map_table)?,
            );
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
    pub fn lookup_frame(&self, label: SynLabel) -> Option<&Frame<RefType, (RefType, Offset)>> {
        // The block is already placed
        if let Some(basic_block) = self.blocks.get(&label) {
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
        expected: &Frame<RefType, (RefType, Offset)>,
        extra_block: Option<(SynLabel, &Frame<RefType, (RefType, Offset)>)>,
    ) -> Result<(), Error> {
        // Annoying edge case
        match extra_block {
            Some((extra_block_label, found)) if extra_block_label == label => {
                if found != expected {
                    return Err(Error::IncompatibleFrames(
                        label,
                        found.clone(),
                        expected.clone(),
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
                    found.clone(),
                    expected.clone(),
                ))
            } else {
                Ok(())
            }
        } else {
            let _ = self.unplaced_labels.insert(label, expected.clone());
            Ok(())
        }
    }
}

impl BytecodeBuilder for CodeBuilder {
    type Lbl = SynLabel;

    fn fresh_label(&mut self) -> SynLabel {
        let to_return = self.next_label;
        self.next_label = self.next_label.next();
        to_return
    }

    fn push_instruction(&mut self, insn: Instruction) -> Result<(), Error> {
        if let Some(current_block) = self.current_block.as_mut() {
            current_block
                .latest_frame
                .interpret_instruction(
                    &insn,
                    current_block.instructions.offset_len(),
                    &self.class_graph.borrow(),
                    &self.constants_pool.borrow(),
                    &self.this_type,
                )
                .map_err(|kind| Error::VerifierError {
                    instruction: insn.clone(),
                    kind,
                })?;
            current_block
                .latest_frame
                .update_maximums(&mut self.max_locals, &mut self.max_stack);
            current_block.extend_block(insn)?;
        }
        Ok(())
    }

    fn push_branch_instruction(
        &mut self,
        insn: BranchInstruction<SynLabel, SynLabel, ()>,
    ) -> Result<(), Error> {
        if let Some(mut current_block) = self.current_block.take() {
            current_block
                .latest_frame
                .interpret_branch_instruction(
                    &insn,
                    &self.class_graph.borrow(),
                    &self.descriptor.return_type,
                )
                .map_err(|kind| Error::VerifierBranchingError {
                    instruction: insn.clone(),
                    kind,
                })?;
            current_block
                .latest_frame
                .update_maximums(&mut self.max_locals, &mut self.max_stack);

            // Check that the jump target (if there is one) has a compatible frame
            if let Some(jump_label) = insn.jump_target().map(|jump_target| jump_target.merge()) {
                self.assert_frame_for_label(
                    jump_label,
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
            self.block_order
                .extend(next_curr_block_opt.iter().map(|b| b.label));
            self.current_block = next_curr_block_opt;
            if let Some(_) = self.blocks.insert(block_label, basic_block) {
                return Err(Error::DuplicateLabel(block_label));
            }
        }
        Ok(())
    }

    fn place_label(&mut self, label: SynLabel) -> Result<(), Error> {
        if let Some(mut current_block) = self.current_block.take() {
            let fall_through_insn = BranchInstruction::FallThrough(label);
            current_block
                .latest_frame
                .interpret_branch_instruction(
                    &fall_through_insn,
                    &self.class_graph.borrow(),
                    &self.descriptor.return_type,
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
            self.block_order
                .extend(next_curr_block_opt.iter().map(|b| b.label));
            self.current_block = next_curr_block_opt;
            if let Some(_) = self.blocks.insert(block_label, basic_block) {
                return Err(Error::DuplicateLabel(block_label));
            }
        } else {
            // Find the frame
            let frame = self
                .unplaced_labels
                .remove(&label)
                .ok_or(Error::PlacingLabelBeforeReference(label))?;

            self.current_block = Some(CurrentBlock::new(label, frame));
        }

        Ok(())
    }

    fn constants(&self) -> RefMut<ConstantsPool> {
        self.constants_pool.borrow_mut()
    }
}

/// Just like `BasicBlock`, but not closed off yet
struct CurrentBlock {
    pub label: SynLabel,

    /// State of the frame at the start of `instructions`
    pub entry_frame: Frame<RefType, (RefType, Offset)>,

    /// Tracks the state of the frame at the end of `instructions`
    pub latest_frame: Frame<RefType, (RefType, Offset)>,

    /// Accumulated instructions
    pub instructions: OffsetVec<Instruction>,
}

impl CurrentBlock {
    /// New block starting with a given frame
    pub fn new(label: SynLabel, entry_frame: Frame<RefType, (RefType, Offset)>) -> CurrentBlock {
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
            BasicBlock<SynLabel, SynLabel, SynLabel>,
            Option<CurrentBlock>,
        ),
        Error,
    > {
        let fallthrough_target: Option<SynLabel> = branch_end.fallthrough_target();

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

    pub fn extend_block(&mut self, insn: Instruction) -> Result<(), Error> {
        self.instructions.push(insn);

        Ok(())
    }
}

/// A JVM method code body is made up of a linear sequence of basic blocks.
///
/// We also store some extra information that ultimately allows us to compute things like: the
/// maximum height of the locals, the maximum height of the stack, and the stack map frames.
#[derive(Debug)]
pub struct BasicBlock<Lbl, LblWide, LblFall> {
    /// Offset of the start of the basic block from the start of the method
    pub offset_from_start: Offset,

    /// Frame at the start of the block
    pub frame: Frame<RefType, (RefType, Offset)>,

    /// Straight-line instructions in the block
    pub instructions: OffsetVec<Instruction>,

    /// Branch instruction to close the block
    pub branch_end: BranchInstruction<Lbl, LblWide, LblFall>,
}

impl<Lbl, LblWide, LblFall> Width for BasicBlock<Lbl, LblWide, LblFall> {
    fn width(&self) -> usize {
        self.instructions.offset_len().0 + self.branch_end.width()
    }
}

/// Opaque label
#[derive(Copy, Clone, Hash, Eq, PartialEq)]
pub struct SynLabel(usize);

impl SynLabel {
    /// Label for the first block in the method
    pub const START: SynLabel = SynLabel(0);

    /// Get the next fresh label
    pub fn next(&self) -> SynLabel {
        SynLabel(self.0 + 1)
    }
}

impl fmt::Debug for SynLabel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_fmt(format_args!("l{}", self.0))
    }
}
