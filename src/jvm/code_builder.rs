use super::*;

use std::collections::HashMap;
use std::fmt;

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
    unplaced_labels: HashMap<SynLabel, Frame>,

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
    class_graph: ClassGraph,

    /// Constants pool
    constants_pool: ConstantsPool,

    /// Enclosing type
    this_type: RefType,
}

impl CodeBuilder {
    /// Generate a fresh label
    pub fn fresh_label(&mut self) -> SynLabel {
        let to_return = self.next_label;
        self.next_label = self.next_label.next();
        to_return
    }

    /// Push a new instruction to the current block
    pub fn push_instruction(&mut self, insn: Instruction) -> Result<(), Error> {
        if let Some(current_block) = self.current_block.as_mut() {
            current_block
                .latest_frame
                .interpret_instruction(
                    &insn,
                    current_block.instructions.offset_len(),
                    &self.class_graph,
                    &self.constants_pool,
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

    /// Push a new branch instruction to end the current block and possibly implicitly start a new
    /// current block
    pub fn push_branch_instruction(
        &mut self,
        insn: BranchInstruction<SynLabel, SynLabel, ()>,
    ) -> Result<(), Error> {
        if let Some(mut current_block) = self.current_block.take() {
            current_block
                .latest_frame
                .interpret_branch_instruction(
                    &insn,
                    &self.class_graph,
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
            self.blocks_end_offset.0 += basic_block.instructions.offset_len().0;
            self.block_order
                .extend(next_curr_block_opt.iter().map(|b| b.label));
            self.current_block = next_curr_block_opt;
            if let Some(_) = self.blocks.insert(block_label, basic_block) {
                return Err(Error::DuplicateLabel(block_label));
            }
        }
        Ok(())
    }

    /// Query the expected frame for a label that has already been referred to and possibly even
    /// jumped to
    pub fn lookup_frame(&self, label: SynLabel) -> Option<&Frame> {
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
                .interpret_branch_instruction(
                    &fall_through_insn,
                    &self.class_graph,
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
            self.blocks_end_offset.0 += basic_block.instructions.offset_len().0;
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
        expected: &Frame,
        extra_block: Option<(SynLabel, &Frame)>,
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

/// Just like `BasicBlock`, but not closed off yet
struct CurrentBlock {
    pub label: SynLabel,

    /// State of the frame at the start of `instructions`
    pub entry_frame: Frame,

    /// Tracks the state of the frame at the end of `instructions`
    pub latest_frame: Frame,

    /// Accumulated instructions
    pub instructions: OffsetVec<Instruction>,
}

impl CurrentBlock {
    /// New block starting with a given frame
    pub fn new(label: SynLabel, entry_frame: Frame) -> CurrentBlock {
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
pub struct BasicBlock<Lbl, LblWide, LblFall> {
    /// Offset of the start of the basic block from the start of the method
    pub offset_from_start: Offset,

    /// Frame at the start of the block
    pub frame: Frame,

    /// Straight-line instructions in the block
    pub instructions: OffsetVec<Instruction>,

    /// Branch instruction to close the block
    pub branch_end: BranchInstruction<Lbl, LblWide, LblFall>,
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
