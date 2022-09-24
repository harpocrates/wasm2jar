//! Fix wide jumps by rewriting them into `goto_w`
//!
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

use crate::jvm::code::{BasicBlock, BranchInstruction, JumpTargets, LabelGenerator};
use crate::util::{Interval, Offset, OffsetVec, RefId, SegmentTree, Width};
use std::cell::Cell;
use std::collections::HashMap;
use std::collections::HashSet;
use std::hash::Hash;
use std::ops::{RangeBounds, RangeInclusive};

/// Range of relative jump offsets supported by `goto` and `if*` branch instructions
pub const SIGNED_16BIT_JUMP_RANGE: RangeInclusive<isize> =
    RangeInclusive::new(i16::MIN as isize, i16::MAX as isize);

/// Given blocks in the specified order, detect which jumps are oversized and rewrite them.
///
/// This process might end up introducing new blocks, which is why both the block order and blocks
/// themselves must be mutable references. The `small_jump_range` parameter should always be
/// `SIGNED_16BIT_JUMP_RANGE` - it is a parameter only for unit testing purposes.
pub fn widen_oversized_jumps<Frame: Clone, Insn: Default + Width, Lbl: Copy + Eq + Hash>(
    block_order: &mut Vec<Lbl>,
    blocks: &mut HashMap<Lbl, BasicBlock<Frame, Insn, BranchInstruction<Lbl, Lbl, Lbl>>>,
    label_generator: &mut impl LabelGenerator<Lbl>,
    small_jump_range: &impl RangeBounds<isize>,
) {
    // Mapping from `Lbl` into a `usize` representing the start of the block along with the
    // initial offset of the start of the block
    //
    // NOTE: we could rewrite oversized `goto` backjumps right here (and this would have a small
    // added benefit of not introducing an extra two `nop` instructions).
    let mut label_index_and_offset: HashMap<Lbl, (usize, Offset)> = HashMap::new();
    let mut current_offset: usize = 0;
    for (idx, lbl) in block_order.iter().enumerate() {
        label_index_and_offset.insert(*lbl, (idx, Offset(current_offset)));
        current_offset += blocks[lbl].width();
    }

    // These are all of the jumps which we may end up rewriting - ie. jumps using 16-bit signed
    // relative offsets
    let mut rewritable_jumps: Vec<JumpInterval<Lbl>> = blocks
        .iter()
        .filter_map(|(block_lbl, block)| match block.branch_end.jump_targets() {
            JumpTargets::Regular(to_block_lbl) => {
                let (mut from_index, mut from_offset) = label_index_and_offset[block_lbl];
                from_index += 1;
                from_offset.0 += block.instructions.offset_len().0;
                let (to_index, to_offset) = label_index_and_offset[&to_block_lbl];
                let jump_distance = to_offset.0 as isize - from_offset.0 as isize;
                let (jump_range, is_forward) = if from_index <= to_index {
                    (RangeInclusive::new(from_index, to_index), true)
                } else {
                    (RangeInclusive::new(to_index, from_index), false)
                };
                Some(JumpInterval {
                    jump_from_block: *block_lbl,
                    jump_range,
                    is_goto: matches!(block.branch_end, BranchInstruction::Goto(_)),
                    is_forward,
                    jump_distance: Cell::new(jump_distance),
                })
            }
            _ => None,
        })
        .collect();

    // Sort for stability of algorithm output (for unit test repeatibility)
    rewritable_jumps
        .sort_unstable_by_key(|jump| (*jump.jump_range.start(), *jump.jump_range.end()));

    // Compute the starter set of oversized jumps (if there are none, bail out now)
    let mut oversized_jumps: Vec<RefId<JumpInterval<Lbl>>> = rewritable_jumps
        .iter()
        .filter(|jump| jump.is_oversized(small_jump_range))
        .map(RefId)
        .collect();
    if oversized_jumps.is_empty() {
        return;
    }
    let mut known_oversized_jumps = oversized_jumps
        .iter()
        .map(|jump| jump.jump_from_block)
        .collect::<HashSet<Lbl>>();

    // Construct a segment tree using all of the jumps
    let jump_tree = SegmentTree::new(rewritable_jumps.iter().collect());
    let mut widen_goto: HashSet<Lbl> = HashSet::new();
    let mut widen_branch: HashMap<Lbl, (Lbl, Lbl)> = HashMap::new();
    while let Some(oversized_jump) = oversized_jumps.pop() {
        // Record the fact this jump will have to be rewritten
        if oversized_jump.is_goto {
            widen_goto.insert(oversized_jump.jump_from_block);
        } else {
            widen_branch.insert(
                oversized_jump.jump_from_block,
                (label_generator.fresh_label(), label_generator.fresh_label()),
            );
        }

        // Update the new jump distances of intervals crossing
        for interval in jump_tree.intervals_containing(&oversized_jump.jump_start_index()) {
            // Interval is already going to be rewritten - nothing else to do here
            if widen_goto.contains(&interval.jump_from_block)
                || widen_branch.contains_key(&interval.jump_from_block)
                || known_oversized_jumps.contains(&interval.jump_from_block)
            {
                continue;
            }

            // If the new interval length is too big, add it back to the set of oversized jumps
            let bytes_added_by_rewrite = if interval.is_goto { 4 } else { 8 };
            if interval.lengthen_jump(bytes_added_by_rewrite, small_jump_range) {
                known_oversized_jumps.insert(interval.jump_from_block);
                oversized_jumps.push(RefId(interval));
            }
        }
    }

    // Now that we know all of the jumps to widen, splice in the new blocks
    let mut new_block_order: Vec<Lbl> = block_order
        .iter()
        .flat_map(|lbl| -> Vec<Lbl> {
            match widen_branch.get(lbl) {
                None => vec![*lbl],
                Some((extra1, extra2)) => vec![*lbl, *extra1, *extra2],
            }
        })
        .collect();

    // Update goto blocks (this is where we perform the rewrite `goto` -> `nop nop goto_w`)
    for goto_block_label in widen_goto {
        let goto_terminated_block = blocks.get_mut(&goto_block_label).unwrap();
        goto_terminated_block.instructions.push(Insn::default());
        goto_terminated_block.instructions.push(Insn::default());
        let goto_label = match &goto_terminated_block.branch_end {
            BranchInstruction::Goto(lbl) => lbl,
            _other => unreachable!("Goto block does not end in goto"),
        };
        goto_terminated_block.branch_end = BranchInstruction::GotoW(*goto_label);
    }

    // Update branch blocks (this is where we perform the rewrite `if*` -> `ifn* goto goto_w`)
    for (branch_block_label, (extra_block1, extra_block2)) in widen_branch {
        let branch_terminated_block = blocks.get_mut(&branch_block_label).unwrap();
        let (new_branch_end, next_lbl, far_lbl) = match &branch_terminated_block.branch_end {
            BranchInstruction::If(comp, far_lbl, next_lbl) => (
                BranchInstruction::If(!*comp, extra_block2, extra_block1),
                *next_lbl,
                *far_lbl,
            ),
            BranchInstruction::IfICmp(comp, far_lbl, next_lbl) => (
                BranchInstruction::IfICmp(!*comp, extra_block2, extra_block1),
                *next_lbl,
                *far_lbl,
            ),
            BranchInstruction::IfACmp(comp, far_lbl, next_lbl) => (
                BranchInstruction::IfACmp(!*comp, extra_block2, extra_block1),
                *next_lbl,
                *far_lbl,
            ),
            BranchInstruction::IfNull(comp, far_lbl, next_lbl) => (
                BranchInstruction::IfNull(!*comp, extra_block2, extra_block1),
                *next_lbl,
                *far_lbl,
            ),
            _other => unreachable!("Branch block does not end in branch"),
        };
        branch_terminated_block.branch_end = new_branch_end;

        blocks.insert(
            extra_block1,
            BasicBlock {
                instructions: OffsetVec::new(),
                frame: blocks[&next_lbl].frame.clone(),
                branch_end: BranchInstruction::Goto(next_lbl),
            },
        );
        blocks.insert(
            extra_block2,
            BasicBlock {
                instructions: OffsetVec::new(),
                frame: blocks[&far_lbl].frame.clone(),
                branch_end: BranchInstruction::GotoW(far_lbl),
            },
        );
    }

    std::mem::swap(&mut new_block_order, block_order);
}

#[derive(Debug)]
struct JumpInterval<Lbl> {
    /// Jump starts at the end of this block
    jump_from_block: Lbl,

    /// Representation of the interval jumped in index-space
    jump_range: RangeInclusive<usize>,

    /// Is this a `goto` (vs. an `if*`)?
    is_goto: bool,

    /// Is this a forward jump (vs. a backward one)?
    is_forward: bool,

    /// Distance being jumped
    jump_distance: Cell<isize>,
}

impl<Lbl> JumpInterval<Lbl> {
    /// Is this jump too big to fit in a 16-bit signed offset
    fn is_oversized(&self, small_jump_range: &impl RangeBounds<isize>) -> bool {
        !small_jump_range.contains(&self.jump_distance.get())
    }

    /// Increase the jump distance and return whether the new distance is oversized
    fn lengthen_jump(&self, by: isize, small_jump_range: &impl RangeBounds<isize>) -> bool {
        let old_dist = self.jump_distance.get();
        let new_dist = if old_dist < 0 {
            old_dist - by
        } else {
            old_dist + by
        };
        self.jump_distance.set(new_dist);
        !small_jump_range.contains(&new_dist)
    }

    /// Where in the interval space did the jump start?
    ///
    /// Note: this is _not_ just the start of the jump range. Whether the jump starts or ends at
    /// the start or end of the jump range depends on whether it is a forward or backwards jump/
    fn jump_start_index(&self) -> usize {
        if self.is_forward {
            *self.jump_range.start()
        } else {
            *self.jump_range.end()
        }
    }
}

impl<Lbl> Interval for JumpInterval<Lbl> {
    type Endpoint = usize;

    fn from(&self) -> usize {
        *self.jump_range.start()
    }

    fn until(&self) -> usize {
        *self.jump_range.end()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::jvm::code::{
        EqComparison, Instruction, OrdComparison, SerializableInstruction, SynLabel,
        SynLabelGenerator,
    };

    type Block =
        BasicBlock<(), SerializableInstruction, BranchInstruction<SynLabel, SynLabel, SynLabel>>;

    /// Make a dummy basic block that is the specified number of bytes long
    fn dummy_block(
        len: usize,
        branch_end: BranchInstruction<SynLabel, SynLabel, SynLabel>,
    ) -> Block {
        let first_nop = if len % 2 == 0 {
            None
        } else {
            Some(Instruction::Nop)
        };
        BasicBlock {
            instructions: first_nop
                .into_iter()
                .chain((0..len / 2).flat_map(|_| [Instruction::IConst2, Instruction::Pop]))
                .collect(),
            frame: (),
            branch_end,
        }
    }

    /// Basic block containing _only_ a branch instruction
    fn empty_block(branch_end: BranchInstruction<SynLabel, SynLabel, SynLabel>) -> Block {
        dummy_block(0, branch_end)
    }

    /// Make basic blocks map
    fn make_basic_block(blocks: &[(SynLabel, &Block)]) -> HashMap<SynLabel, Block> {
        blocks
            .iter()
            .cloned()
            .map(|(lbl, block)| (lbl, block.clone()))
            .collect()
    }

    fn assert_jump_rewrite(
        block_order: &mut Vec<SynLabel>,
        blocks: &mut HashMap<SynLabel, Block>,
        label_generator: &mut SynLabelGenerator,
        expected_block_order: &[SynLabel],
        expected_blocks: &HashMap<SynLabel, Block>,
    ) {
        let old_offsets = BasicBlock::compute_block_offsets(block_order, blocks);

        // Perform the rewrites
        widen_oversized_jumps(
            block_order,
            blocks,
            label_generator,
            &SIGNED_16BIT_JUMP_RANGE,
        );
        let new_offsets = BasicBlock::compute_block_offsets(block_order, blocks);

        // Check that the output layout order is the one we expect
        assert_eq!(
            block_order, expected_block_order,
            "Unexpected output block layout order"
        );

        // Check that the output blocks are the ones we expect
        let blocks_keys: HashSet<SynLabel> = blocks.keys().copied().collect();
        let expected_blocks_keys: HashSet<SynLabel> = expected_blocks.keys().copied().collect();
        assert_eq!(
            blocks_keys
                .difference(&expected_blocks_keys)
                .collect::<HashSet<_>>(),
            HashSet::new(),
            "Output blocks have more than the expected labels"
        );
        assert_eq!(
            expected_blocks_keys
                .difference(&blocks_keys)
                .collect::<HashSet<_>>(),
            HashSet::new(),
            "Output blocks are missing some expected labels"
        );
        for key in blocks_keys {
            let block = &blocks[&key];
            assert_eq!(
                block, &expected_blocks[&key],
                "Unexpected output block for label {:?}",
                key
            );

            // Check that jumps are all the right sizes now
            match block.branch_end.jump_targets() {
                JumpTargets::None | JumpTargets::WideMany(_) => (),
                JumpTargets::Regular(target) => {
                    let from_offset = new_offsets[&key].0 + block.instructions.offset_len().0;
                    let to_offset = new_offsets[&target].0;
                    let jump_distance = to_offset as isize - from_offset as isize;
                    assert!(
                        i16::try_from(jump_distance).is_ok(),
                        "regular jump from {:?} should have been widened (jump offset {})",
                        key,
                        jump_distance
                    );
                }
                JumpTargets::Wide(target) => {
                    let from_offset = new_offsets[&key].0 + block.instructions.offset_len().0;
                    let to_offset = new_offsets[&target].0;
                    let jump_distance = to_offset as isize - from_offset as isize;
                    assert!(
                        i16::try_from(jump_distance).is_err(),
                        "regular jump from {:?} should not have been widened (jump offset {})",
                        key,
                        jump_distance
                    );
                }
            }
        }

        // Check that the new offsets for any blocks differ from old offsets of the same blocks by
        // exact multiples of 4.
        for (old_block_lbl, old_offset) in old_offsets {
            let new_offset = new_offsets[&old_block_lbl];
            assert_eq!(
                new_offset.0.abs_diff(old_offset.0) % 4,
                0,
                "Block with label {:?} used to have offset {} but now has offset {}",
                old_block_lbl,
                old_offset.0,
                new_offset.0,
            );
        }
    }

    // Single block without a jump should not be rewritten at all
    #[test]
    fn no_jumps() {
        let label_generator = &mut SynLabelGenerator::new(SynLabel::START);

        let l0 = label_generator.fresh_label();
        let block_order = &mut vec![l0];

        let b0 = &dummy_block(13, BranchInstruction::Return);
        let blocks = &mut make_basic_block(&[(l0, b0)]);

        assert_jump_rewrite(
            block_order,
            blocks,
            label_generator,
            &[l0],
            &make_basic_block(&[(l0, b0)]),
        );
    }

    // Several small jumps which should not be rewritten
    #[test]
    fn non_oversized_jumps() {
        let label_generator = &mut SynLabelGenerator::new(SynLabel::START);

        let l0 = label_generator.fresh_label();
        let l1 = label_generator.fresh_label();
        let l2 = label_generator.fresh_label();
        let block_order = &mut vec![l0, l1, l2];

        let b0 = &dummy_block(2, BranchInstruction::If(OrdComparison::LT, l2, l1));
        let b1 = &dummy_block(2, BranchInstruction::Return);
        let b2 = &dummy_block(2, BranchInstruction::Goto(l1));
        let blocks = &mut make_basic_block(&[(l0, b0), (l1, b1), (l2, b2)]);

        assert_jump_rewrite(
            block_order,
            blocks,
            label_generator,
            &[l0, l1, l2],
            &make_basic_block(&[(l0, b0), (l1, b1), (l2, b2)]),
        );
    }

    // Long backward `goto` jump that should be rewritten to a `goto_w`
    #[test]
    fn oversized_back_goto() {
        let label_generator = &mut SynLabelGenerator::new(SynLabel::START);

        let l0 = label_generator.fresh_label();
        let l1 = label_generator.fresh_label();
        let l2 = label_generator.fresh_label();
        let block_order = &mut vec![l0, l1, l2];

        let b0 = &dummy_block(2, BranchInstruction::Goto(l2));
        let b1 = &dummy_block(2, BranchInstruction::Return);
        let b2 = &dummy_block(34000, BranchInstruction::Goto(l1));
        let blocks = &mut make_basic_block(&[(l0, b0), (l1, b1), (l2, b2)]);

        let mut new_b2 = b2.clone();
        new_b2.instructions.push(Instruction::Nop);
        new_b2.instructions.push(Instruction::Nop);
        new_b2.branch_end = BranchInstruction::GotoW(l1);

        assert_jump_rewrite(
            block_order,
            blocks,
            label_generator,
            &[l0, l1, l2],
            &make_basic_block(&[(l0, b0), (l1, b1), (l2, &new_b2)]),
        );
    }

    // Long backward `ifeq` jump that should be rewritten to a `ifne` + `goto_w`
    #[test]
    fn oversized_back_ifeq() {
        let label_generator = &mut SynLabelGenerator::new(SynLabel::START);

        let l0 = label_generator.fresh_label();
        let l1 = label_generator.fresh_label();
        let l2 = label_generator.fresh_label();
        let l3 = label_generator.fresh_label();
        let block_order = &mut vec![l0, l1, l2, l3];

        let b0 = &dummy_block(2, BranchInstruction::Goto(l2));
        let b1 = &dummy_block(2, BranchInstruction::Return);
        let b2 = &dummy_block(34000, BranchInstruction::If(OrdComparison::EQ, l1, l3));
        let b3 = &dummy_block(2, BranchInstruction::Return);
        let blocks = &mut make_basic_block(&[(l0, b0), (l1, b1), (l2, b2), (l3, b3)]);

        let label_generator_copy = &mut label_generator.clone();
        let l4 = label_generator_copy.fresh_label();
        let l5 = label_generator_copy.fresh_label();

        let new_b2 = &mut b2.clone();
        new_b2.branch_end = BranchInstruction::If(OrdComparison::NE, l5, l4);
        let b4 = &empty_block(BranchInstruction::Goto(l3));
        let b5 = &empty_block(BranchInstruction::GotoW(l1));

        assert_jump_rewrite(
            block_order,
            blocks,
            label_generator,
            &[l0, l1, l2, l4, l5, l3],
            &make_basic_block(&[
                (l0, b0),
                (l1, b1),
                (l2, new_b2),
                (l3, b3),
                (l4, b4),
                (l5, b5),
            ]),
        );
    }

    // Long forward `goto` jump that should be rewritten to a `goto_w`
    #[test]
    fn oversized_forward_goto() {
        let label_generator = &mut SynLabelGenerator::new(SynLabel::START);

        let l0 = label_generator.fresh_label();
        let l1 = label_generator.fresh_label();
        let l2 = label_generator.fresh_label();
        let l3 = label_generator.fresh_label();
        let block_order = &mut vec![l0, l1, l2, l3];

        let b0 = &dummy_block(2, BranchInstruction::If(OrdComparison::EQ, l2, l1));
        let b1 = &dummy_block(2, BranchInstruction::Goto(l3));
        let b2 = &dummy_block(34000, BranchInstruction::Return);
        let b3 = &dummy_block(2, BranchInstruction::Return);
        let blocks = &mut make_basic_block(&[(l0, b0), (l1, b1), (l2, b2), (l3, b3)]);

        let new_b1 = &mut b1.clone();
        new_b1.instructions.push(Instruction::Nop);
        new_b1.instructions.push(Instruction::Nop);
        new_b1.branch_end = BranchInstruction::GotoW(l3);

        assert_jump_rewrite(
            block_order,
            blocks,
            label_generator,
            &[l0, l1, l2, l3],
            &make_basic_block(&[(l0, b0), (l1, new_b1), (l2, b2), (l3, b3)]),
        );
    }

    // Long forward `ifeq` jump that should be rewritten to a `ifne` + `goto_w`
    #[test]
    fn oversized_forward_ifeq() {
        let label_generator = &mut SynLabelGenerator::new(SynLabel::START);

        let l0 = label_generator.fresh_label();
        let l1 = label_generator.fresh_label();
        let l2 = label_generator.fresh_label();
        let block_order = &mut vec![l0, l1, l2];

        let b0 = &dummy_block(2, BranchInstruction::If(OrdComparison::EQ, l2, l1));
        let b1 = &dummy_block(34000, BranchInstruction::Return);
        let b2 = &dummy_block(2, BranchInstruction::Return);
        let blocks = &mut make_basic_block(&[(l0, b0), (l1, b1), (l2, b2)]);

        let label_generator_copy = &mut label_generator.clone();
        let l3 = label_generator_copy.fresh_label();
        let l4 = label_generator_copy.fresh_label();

        let new_b0 = &mut b0.clone();
        new_b0.branch_end = BranchInstruction::If(OrdComparison::NE, l4, l3);
        let b3 = &empty_block(BranchInstruction::Goto(l1));
        let b4 = &empty_block(BranchInstruction::GotoW(l2));

        assert_jump_rewrite(
            block_order,
            blocks,
            label_generator,
            &[l0, l3, l4, l1, l2],
            &make_basic_block(&[(l0, new_b0), (l1, b1), (l2, b2), (l3, b3), (l4, b4)]),
        );
    }

    // more complicated situation with lots of overlapping jump intervals, and a variety of them
    // being rewritten and not rewritten
    #[test]
    fn complex_chain_of_rewrites() {
        let label_generator = &mut SynLabelGenerator::new(SynLabel::START);

        let l0 = label_generator.fresh_label();
        let l1 = label_generator.fresh_label();
        let l2 = label_generator.fresh_label();
        let l3 = label_generator.fresh_label();
        let l4 = label_generator.fresh_label();
        let l5 = label_generator.fresh_label();
        let l6 = label_generator.fresh_label();
        let l7 = label_generator.fresh_label();
        let l8 = label_generator.fresh_label();
        let l9 = label_generator.fresh_label();
        let block_order = &mut vec![l0, l1, l2, l3, l4, l5, l6, l7, l8, l9];

        let b0 = &dummy_block(2, BranchInstruction::IfICmp(OrdComparison::GT, l3, l1));
        let b1 = &dummy_block(4000, BranchInstruction::FallThrough(l2));
        let b2 = &dummy_block(
            i16::MAX as usize - b1.width() - 5,
            BranchInstruction::FallThrough(l3),
        );
        let b3 = &dummy_block(2000, BranchInstruction::IfACmp(EqComparison::EQ, l7, l5));
        let b4 = &dummy_block(
            i16::MAX as usize + 6 - b3.width() - 4000,
            BranchInstruction::Goto(l2),
        );
        let b5 = &dummy_block(
            i16::MAX as usize - 2 - b3.width() - b4.width(),
            BranchInstruction::IfICmp(OrdComparison::GT, l3, l6),
        );
        let b6 = &dummy_block(
            2000 - 6,
            BranchInstruction::IfICmp(OrdComparison::GT, l9, l7),
        );
        let b7 = &dummy_block(
            i16::MAX as usize + 1 - b4.width() - b5.width() - b6.width(),
            BranchInstruction::FallThrough(l8),
        );
        let b8 = &dummy_block(20, BranchInstruction::FallThrough(l9));
        let b9 = &empty_block(BranchInstruction::Return);
        let blocks = &mut make_basic_block(&[
            (l0, b0),
            (l1, b1),
            (l2, b2),
            (l3, b3),
            (l4, b4),
            (l5, b5),
            (l6, b6),
            (l7, b7),
            (l8, b8),
            (l9, b9),
        ]);

        // This is the intuition. Barely undersized jumps are those that if they get widened (eg.
        // from a rewrite somewhere between the start/end), they'll become oversized.
        assert_eq!(
            b0.branch_end.width() + b1.width() + b2.width(),
            i16::MAX as usize - 2,
            "forward jump l0->l3 is barely undersized",
        );
        assert_eq!(
            b2.width() + b3.width() + b4.width() - b4.branch_end.width(),
            i16::MAX as usize + 24768,
            "backward jump l4->l2 is oversized",
        );
        assert_eq!(
            b3.width() + b4.width() + b5.width() - b5.branch_end.width(),
            i16::MAX as usize - 2,
            "backward jump l5->l3 is barely undersized",
        );
        assert_eq!(
            b3.branch_end.width() + b4.width() + b5.width() + b6.width(),
            i16::MAX as usize - 2,
            "forward jump l3->l7 is barely undersized",
        );
        assert_eq!(
            b7.width() + b8.width(),
            26,
            "forward jump l6->l9 is quite small"
        );

        let label_generator_copy = &mut label_generator.clone();
        let l10 = label_generator_copy.fresh_label();
        let l11 = label_generator_copy.fresh_label();
        let l12 = label_generator_copy.fresh_label();
        let l13 = label_generator_copy.fresh_label();

        let new_b3 = &mut b3.clone();
        new_b3.branch_end = BranchInstruction::IfACmp(EqComparison::NE, l11, l10);
        let b10 = &empty_block(BranchInstruction::Goto(l5));
        let b11 = &empty_block(BranchInstruction::GotoW(l7));

        let new_b4 = &mut b4.clone();
        new_b4.instructions.push(Instruction::Nop);
        new_b4.instructions.push(Instruction::Nop);
        new_b4.branch_end = BranchInstruction::GotoW(l2);

        let new_b5 = &mut b5.clone();
        new_b5.branch_end = BranchInstruction::IfICmp(OrdComparison::LE, l13, l12);
        let b12 = &empty_block(BranchInstruction::Goto(l6));
        let b13 = &empty_block(BranchInstruction::GotoW(l3));

        assert_jump_rewrite(
            block_order,
            blocks,
            label_generator,
            &[l0, l1, l2, l3, l10, l11, l4, l5, l12, l13, l6, l7, l8, l9],
            &make_basic_block(&[
                (l0, b0),
                (l1, b1),
                (l2, b2),
                (l3, new_b3),
                (l4, new_b4),
                (l5, new_b5),
                (l6, b6),
                (l7, b7),
                (l8, b8),
                (l9, b9),
                (l10, b10),
                (l11, b11),
                (l12, b12),
                (l13, b13),
            ]),
        );
    }
}
