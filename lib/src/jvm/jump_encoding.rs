//! This module is responsible for fixing jumps that require bigger relative offsets than the jump
//! instruction supports. The general idea is to switch to using `goto_w` for all jumps that don't
//! fit in the signed 16-bit offset that most other jump instructions have.
//!
//! ### Termination
//!
//! This is a tricky problem because the `goto_w` rewrites are themselves longer than the initial
//! jump instruction, so the rewrites risk causing other jumps to become oversized and also need to
//! be rewritten. Thankfully, we know the process will finish because the number of rewritable
//! 16-bit jump instructions only ever decreases:
//!
//!   - once a jump is rewritten, it can be discarded from consideration (`goto_w` is definitely
//!     enough)
//!
//!   - any extra 16-bit jump instruction introduced in a rewrite are always jumping small _fixed_
//!     distances so they never need to be rewritten
//!
//! ### Rewriting
//!
//! There are two categories of 16-bit jumps that need to be rewritten: `goto` and `if*`. Depending
//! on the case, we employ a different rewrite. Rewrites must always insert a segment that is a
//! multiple of four bytes wide, so that any `lookupswitch`/`tableswitch` padding is still correct.
//! For `goto`, this requires some `nop` padding:
//!
//!
//! ```text,ignore,no_run
//!                           nop
//!                           nop
//!     goto L2               goto_w L2
//! L1: ...         =>    L1: ...
//!     ...                   ...
//! L2: ...               L2: ...
//! ```
//!
//! For other instructions, the rewrite happens to already introduce exactly 8 bytes.
//!
//! ```text,ignore,no_run
//!                           ifnot* L4
//!                       L3: goto L1
//!     if* L2            L4: goto_w L2
//! L1: ...         =>    L1: ...
//!     ...                   ...
//! L2: ...               L2: ...
//! ```
//!

use crate::jvm::{BranchInstruction, SerializableInstruction};
use crate::jvm::model::{BasicBlock, SynLabelGenerator, SynLabel};
use crate::util::{RefId, Offset, OffsetVec, Width, SegmentTree, Interval};
use std::collections::HashMap;
use std::cell::Cell;
use crate::jvm::{Instruction, JumpTargets};
use std::collections::HashSet;
use std::ops::Range;

/// Range of relative jump offsets supported by `goto` and `if*` branch instructions
pub const SIGNED_16BIT_JUMP_RANGE: Range<isize> = Range {
    start: i16::MIN as isize,
    end: i16::MAX as isize + 1,
};

/// Given blocks in the specified order, detect which jumps are oversized and rewrite them.
///
/// This process might end up introducing new blocks, which is why both the block order and blocks
/// themselves must be mutable references. The `small_jump_range` parameter should always be
/// `SIGNED_16BIT_JUMP_RANGE` - it is a parameter only for unit testing purposes.
pub fn widen_oversized_jumps<Frame: Clone>(
    block_order: &mut Vec<SynLabel>,
    blocks: &mut HashMap<SynLabel, BasicBlock<Frame, SerializableInstruction, BranchInstruction<SynLabel, SynLabel, SynLabel>>>,
    label_generator: &mut SynLabelGenerator,
    small_jump_range: Range<isize>,
) {

    // Mapping from `SynLabel` into a `usize` representing the start of the block along with the
    // initial offset of the start of the block
    let mut label_index_and_offset: HashMap<SynLabel, (usize, Offset)> = HashMap::new();
    let mut current_offset: usize = 0;
    for (idx, lbl) in block_order.iter().enumerate() {
        label_index_and_offset.insert(*lbl, (idx, Offset(current_offset)));
        current_offset += blocks[lbl].width();
    }

    // These are all of the jumps which we may end up rewriting - ie. jumps using 16-bit signed
    // relative offsets
    let rewritable_jumps: Vec<JumpInterval> = blocks
        .iter()
        .filter_map(|(block_lbl, block)| {
            match block.branch_end.jump_targets() {
                JumpTargets::Regular(to_block_lbl) => {
                    let (mut from_index, mut from_offset) = label_index_and_offset[&block_lbl];
                    from_index += 1;
                    from_offset.0 += block.instructions.offset_len().0;
                    let (to_index, to_offset) = label_index_and_offset[&to_block_lbl];
                    let jump_distance = to_offset.0 as isize - from_offset.0 as isize;
                    Some(JumpInterval {
                        from: *block_lbl,
                        to: to_block_lbl,
                        from_index,
                        to_index,
                        is_goto: matches!(block.branch_end, BranchInstruction::Goto(_)),
                        jump_distance: Cell::new(jump_distance),
                    })
                },
                _ => None,
            }
        })
        .collect();
    
    // Construct a segment tree using all of the jumps
    let mut oversized_jumps: HashSet<RefId<JumpInterval>> = rewritable_jumps
        .iter()
        .filter(|jump| jump.is_oversized(&small_jump_range))
        .map(RefId)
        .collect();
    let jump_tree = SegmentTree::new(rewritable_jumps.iter().collect());
    let mut widen_goto: HashSet<SynLabel> = HashSet::new();
    let mut widen_branch: HashMap<SynLabel, (SynLabel, SynLabel)> = HashMap::new();
    while let Some(oversized_jump) = oversized_jumps.iter().nth(0).copied() {
        oversized_jumps.remove(&oversized_jump);

        // Record the fact this jump will have to be rewritten
        if oversized_jump.is_goto {
            widen_goto.insert(oversized_jump.from);
        } else {
            widen_branch.insert(oversized_jump.from, (label_generator.fresh_label(), label_generator.fresh_label()));
        }

        // Update the new jump distances of intervals crossing
        for interval in jump_tree.intervals_containing(&oversized_jump.from_index) {

            // Interval is already going to be rewritten - nothing else to do here
            if widen_goto.contains(&interval.from) || widen_branch.contains_key(&interval.from) {
                continue
            }

            // If the new interval length is too big, add it back to the set of oversized jumps
            let bytes_added_by_rewrite = if interval.is_goto { 4 } else { 8 };
            if interval.lengthen_jump(bytes_added_by_rewrite, &small_jump_range) {
                oversized_jumps.insert(RefId(interval));
            }
        }
    }
   
    // Now that we know all of the jumps to widen, splice in the new blocks
    let mut new_block_order: Vec<SynLabel> = block_order
        .iter()
        .flat_map(|lbl| -> Vec<SynLabel> {
            match widen_branch.get(lbl) {
                None => vec![*lbl],
                Some((extra1, extra2)) => vec![*lbl, *extra1, *extra2],
            }
        })
        .collect();

    // Update goto blocks
    for goto_block_label in widen_goto {
        let goto_terminated_block = blocks.get_mut(&goto_block_label).unwrap();
        goto_terminated_block.instructions.push(Instruction::Nop);
        goto_terminated_block.instructions.push(Instruction::Nop);
        let goto_label = match &goto_terminated_block.branch_end {
            BranchInstruction::Goto(lbl) => lbl,
            _other => unreachable!("Goto block does not end in goto"),
        };
        goto_terminated_block.branch_end = BranchInstruction::GotoW(*goto_label);
    }

    // Update branch blocks
    for (branch_block_label, (extra_block1, extra_block2)) in widen_branch {
        let branch_terminated_block = blocks.get_mut(&branch_block_label).unwrap();
        let (new_branch_end, next_lbl, far_lbl) = match &branch_terminated_block.branch_end {
            BranchInstruction::If(comp, far_lbl, next_lbl) =>
                (BranchInstruction::If(!*comp, extra_block2, extra_block1), *next_lbl, *far_lbl),
            BranchInstruction::IfICmp(comp, far_lbl, next_lbl) =>
                (BranchInstruction::IfICmp(!*comp, extra_block2, extra_block1), *next_lbl, *far_lbl),
            BranchInstruction::IfACmp(comp, far_lbl, next_lbl) =>
                (BranchInstruction::IfACmp(!*comp, extra_block2, extra_block1), *next_lbl, *far_lbl),
            BranchInstruction::IfNull(comp, far_lbl, next_lbl) =>
                (BranchInstruction::IfNull(!*comp, extra_block2, extra_block1), *next_lbl, *far_lbl),
            _other => unreachable!("Branch block does not end in branch"),
        };
        branch_terminated_block.branch_end = new_branch_end;

        blocks.insert(
            extra_block1,
            BasicBlock {
                offset_from_start: Offset(0),
                instructions: OffsetVec::new(),
                frame: blocks[&next_lbl].frame.clone(),
                branch_end: BranchInstruction::Goto(next_lbl),
            }
        );
        blocks.insert(
            extra_block2,
            BasicBlock {
                offset_from_start: Offset(0),
                instructions: OffsetVec::new(),
                frame: blocks[&far_lbl].frame.clone(),
                branch_end: BranchInstruction::GotoW(far_lbl),
            }
        );
    }

    std::mem::swap(&mut new_block_order, block_order);
}

struct JumpInterval {

    /// Jump starts at the end of this block
    from: SynLabel,

    /// Jump ends at the start of this block
    to: SynLabel,

    /// Representation of `from` in an index space
    from_index: usize,

    /// Representation of `to` in an index space
    to_index: usize,

    /// Is this a `goto` (vs. an `if*`)?
    is_goto: bool,

    /// Distance being jumped
    jump_distance: Cell<isize>,
}

impl JumpInterval {

    /// Is this jump too big to fit in a 16-bit signed offset
    fn is_oversized(&self, small_jump_range: &Range<isize>) -> bool {
        !small_jump_range.contains(&self.jump_distance.get())
    }

    /// Increase the jump distance and return whether the new distance is oversized
    fn lengthen_jump(&self, by: isize, small_jump_range: &Range<isize>) -> bool {
        let old_dist = self.jump_distance.get();
        let new_dist = if old_dist < 0 { old_dist - by } else { old_dist + by };
        self.jump_distance.set(new_dist);
        !small_jump_range.contains(&new_dist)
    }

    /// Is this a forward or backward jump?
    fn is_forward_jump(&self) -> bool {
        self.from_index <= self.to_index
    }
}

impl Interval for JumpInterval {
    type Endpoint = usize;

    fn from(&self) -> usize {
        self.from_index
    }

    fn until(&self) -> usize {
        self.to_index
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::jvm::OrdComparison;

    // Dummy offset - not used
    const offset_from_start: Offset = Offset(0);

    // Single block without a jump should not be rewritten at all
    #[test]
    fn single_block() {
        let mut label_generator = SynLabelGenerator::new(SynLabel::START);

        let label1 = label_generator.fresh_label();
        let block_order = vec![label1];

        let mut blocks = HashMap::new();        
        blocks.insert(
            label1,
            BasicBlock {
                offset_from_start,
                instructions: vec![
                    Instruction::IConst1,
                    Instruction::IConst2,
                    Instruction::IAdd,
                ].into_iter().collect(),
                branch_end: BranchInstruction::IReturn,
                frame: ()
            },
        );

        let mut blocks_copy = blocks.clone();
        let mut block_order_copy = block_order.clone();
        widen_oversized_jumps(
            &mut block_order_copy,
            &mut blocks_copy,
            &mut label_generator,
            SIGNED_16BIT_JUMP_RANGE,
        );

        assert_eq!(block_order, block_order_copy);
        assert_eq!(blocks_copy, blocks);
    }

    // Several block with a short jump that should not be rewritten
    #[test]
    fn short_jump() {
        let mut label_generator = SynLabelGenerator::new(SynLabel::START);

        let label1 = label_generator.fresh_label();
        let label2 = label_generator.fresh_label();
        let label3 = label_generator.fresh_label();
        let block_order = vec![label1, label2, label3];

        let mut blocks = HashMap::new();        
        blocks.insert(
            label1,
            BasicBlock {
                offset_from_start,
                instructions: vec![
                    Instruction::IConst1,
                ].into_iter().collect(),
                branch_end: BranchInstruction::Goto(label3),
                frame: ()
            },
        );
        blocks.insert(
            label2,
            BasicBlock {
                offset_from_start,
                instructions: vec![
                    Instruction::IConst1,
                    Instruction::IAdd,
                ].into_iter().collect(),
                branch_end: BranchInstruction::IReturn,
                frame: ()
            },
        );
        blocks.insert(
            label3,
            BasicBlock {
                offset_from_start,
                instructions: vec![
                    Instruction::IConst1,
                    Instruction::IAdd,
                ].into_iter().collect(),
                branch_end: BranchInstruction::Goto(label2),
                frame: ()
            },
        );


        let mut blocks_copy = blocks.clone();
        let mut block_order_copy = block_order.clone();
        widen_oversized_jumps(
            &mut block_order_copy,
            &mut blocks_copy,
            &mut label_generator,
            SIGNED_16BIT_JUMP_RANGE,
        );

        assert_eq!(block_order, block_order_copy);
        assert_eq!(blocks_copy, blocks);
    }

    // Several blocks with a long jump that should be rewritten to a `goto_w`
    #[test]
    fn rewrite_goto_to_wide_goto() {
        let mut label_generator = SynLabelGenerator::new(SynLabel::START);

        let label1 = label_generator.fresh_label();
        let label2 = label_generator.fresh_label();
        let label3 = label_generator.fresh_label();
        let block_order = vec![label1, label2, label3];

        let mut blocks = HashMap::new();        
        blocks.insert(
            label1,
            BasicBlock {
                offset_from_start,
                instructions: vec![
                    Instruction::IConst1,
                ].into_iter().collect(),
                branch_end: BranchInstruction::Goto(label3),
                frame: ()
            },
        );
        blocks.insert(
            label2,
            BasicBlock {
                offset_from_start,
                instructions: vec![
                    Instruction::IConst1,
                    Instruction::IAdd,
                ].into_iter().collect(),
                branch_end: BranchInstruction::IReturn,
                frame: ()
            },
        );
        blocks.insert(
            label3,
            BasicBlock {
                offset_from_start,
                instructions: (0..17000)
                    .flat_map(|_| vec![Instruction::IConst1, Instruction::IAdd])
                    .collect(),
                branch_end: BranchInstruction::Goto(label2),
                frame: ()
            },
        );

        let mut blocks_copy = blocks.clone();
        let mut block_order_copy = block_order.clone();
        widen_oversized_jumps(
            &mut block_order_copy,
            &mut blocks_copy,
            &mut label_generator,
            SIGNED_16BIT_JUMP_RANGE,
        );

        assert_eq!(block_order, block_order_copy);
        assert_eq!(blocks_copy[&label1], blocks[&label1]);
        assert_eq!(blocks_copy[&label2], blocks[&label2]);
        assert!(matches!(blocks_copy[&label3].branch_end, BranchInstruction::GotoW(_)));
        assert_eq!(blocks_copy.len(), 3);
    }

    // Several blocks with a long branch that should be rewritten to a `goto_w` + small jumps
    #[test]
    fn rewrite_ifeq_to_wide_goto() {
        let mut label_generator = SynLabelGenerator::new(SynLabel::START);

        let label1 = label_generator.fresh_label();
        let label2 = label_generator.fresh_label();
        let label3 = label_generator.fresh_label();
        let label4 = label_generator.fresh_label();
        let block_order = vec![label1, label2, label3, label4];

        let mut blocks = HashMap::new();        
        blocks.insert(
            label1,
            BasicBlock {
                offset_from_start,
                instructions: vec![
                    Instruction::IConst1,
                ].into_iter().collect(),
                branch_end: BranchInstruction::Goto(label3),
                frame: (),
            },
        );
        blocks.insert(
            label2,
            BasicBlock {
                offset_from_start,
                instructions: vec![
                    Instruction::IConst1,
                    Instruction::IAdd,
                ].into_iter().collect(),
                branch_end: BranchInstruction::IReturn,
                frame: (),
            },
        );
        blocks.insert(
            label3,
            BasicBlock {
                offset_from_start,
                instructions: (0..17000)
                    .flat_map(|_| vec![Instruction::IConst1, Instruction::IAdd])
                    .collect(),
                branch_end: BranchInstruction::If(OrdComparison::EQ, label2, label4),
                frame: (),
            },
        );
        blocks.insert(
            label4,
            BasicBlock {
                offset_from_start,
                instructions: OffsetVec::new(),
                branch_end: BranchInstruction::IReturn,
                frame: (),
            },
        );

        let mut blocks_copy = blocks.clone();
        let mut block_order_copy = block_order.clone();
        let mut label_generator_copy = label_generator.clone();
        widen_oversized_jumps(
            &mut block_order_copy,
            &mut blocks_copy,
            &mut label_generator_copy,
            SIGNED_16BIT_JUMP_RANGE,
        );

        let label5 = label_generator.fresh_label();
        let label6 = label_generator.fresh_label();
        assert_eq!(block_order_copy, vec![label1, label2, label3, label5, label6, label4]);
        assert_eq!(blocks_copy[&label1], blocks[&label1]);
        assert_eq!(blocks_copy[&label2], blocks[&label2]);
        assert_eq!(blocks_copy[&label3], BasicBlock { branch_end: BranchInstruction::If(OrdComparison::NE, label6, label5), ..blocks[&label3].clone() });
        assert_eq!(blocks_copy[&label5], BasicBlock { offset_from_start, instructions: OffsetVec::new(), branch_end: BranchInstruction::Goto(label4), frame: () });
        assert_eq!(blocks_copy[&label6], BasicBlock { offset_from_start, instructions: OffsetVec::new(), branch_end: BranchInstruction::GotoW(label2), frame: () });
        assert_eq!(blocks_copy[&label4], blocks[&label4]);
        assert_eq!(blocks_copy.len(), 6); // 4 initial block + 2 synthetic blocks
    }

}

