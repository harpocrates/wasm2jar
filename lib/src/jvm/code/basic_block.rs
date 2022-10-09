use crate::jvm::class_file::{ConstantPoolOverflow, ConstantsPool, ConstantsWriter};
use crate::jvm::class_graph::BootstrapMethodId;
use crate::jvm::code::{BranchInstruction, SerializableInstruction, SynLabel, VerifierInstruction};
use crate::jvm::descriptors::RenderDescriptor;
use crate::jvm::names::Name;
use crate::jvm::verifier::VerifierFrame;
use crate::util::{Offset, OffsetVec, Width};
use std::collections::HashMap;

pub type SerializableBasicBlock<'g> = BasicBlock<
    VerifierFrame<'g>,
    SerializableInstruction,
    BranchInstruction<SynLabel, SynLabel, SynLabel>,
>;

pub type VerifierBasicBlock<'g> = BasicBlock<
    VerifierFrame<'g>,
    VerifierInstruction<'g>,
    BranchInstruction<SynLabel, SynLabel, SynLabel>,
>;

/// A JVM method code body is made up of a linear sequence of basic blocks.
///
/// We also store some extra information that ultimately allows us to compute things like: the
/// maximum height of the locals, the maximum height of the stack, and the stack map frames.
#[derive(Debug, Eq, PartialEq, Clone)]
pub struct BasicBlock<Frame, Insn, BrInsn> {
    /// Frame at the start of the block
    pub frame: Frame,

    /// Straight-line instructions in the block
    pub instructions: OffsetVec<Insn>,

    /// Branch instruction to close the block
    pub branch_end: BrInsn,
}

impl<Frame, Insn: Width, BrInsn: Width> Width for BasicBlock<Frame, Insn, BrInsn> {
    fn width(&self) -> usize {
        self.instructions.offset_len().0 + self.branch_end.width()
    }
}

impl<Frame, Insn: Width, BrInsn: Width> BasicBlock<Frame, Insn, BrInsn> {
    /// Given an expected order of blocks, compute the offset of every basic block with respect to
    /// that start of the method.
    pub fn compute_block_offsets(
        block_layout_order: &[SynLabel],
        blocks: &HashMap<SynLabel, BasicBlock<Frame, Insn, BrInsn>>,
    ) -> HashMap<SynLabel, Offset> {
        let mut block_offsets: HashMap<SynLabel, Offset> = HashMap::new();
        let mut offset = Offset(0);
        for block_lbl in block_layout_order {
            block_offsets.insert(*block_lbl, offset);
            offset.0 += blocks[block_lbl].width();
        }
        block_offsets
    }
}

impl<'g, Frame, Lbl> BasicBlock<Frame, VerifierInstruction<'g>, BranchInstruction<Lbl, Lbl, Lbl>> {
    /// Serialize the instructions inside a block
    ///
    /// This is the point at which instructions referencing the constant pool get fully resolved
    /// into offsets to actual constants. Consequently, this is also the first time that the
    /// actual width of the basic block is understood.
    pub fn serialize_instructions(
        self,
        constants: &mut ConstantsPool<'g>,
        bootstrap_methods: &mut HashMap<BootstrapMethodId<'g>, u16>,
        offset_from_start: Offset,
    ) -> Result<
        BasicBlock<Frame, SerializableInstruction, BranchInstruction<Lbl, Lbl, Lbl>>,
        ConstantPoolOverflow,
    > {
        let constants = &std::cell::RefCell::new(constants);

        // Serialize the instructions
        let instructions = self
            .instructions
            .iter()
            .map(
                |(_, _, insn)| -> Result<SerializableInstruction, ConstantPoolOverflow> {
                    insn.map(
                        |class| class.constant_index(&mut constants.borrow_mut()),
                        |constant| constant.constant_index(&mut constants.borrow_mut()),
                        |field| field.constant_index(&mut constants.borrow_mut()),
                        |method| method.constant_index(&mut constants.borrow_mut()),
                        |indy_method| -> Result<_, ConstantPoolOverflow> {
                            let next_bootstrap_index = bootstrap_methods.len() as u16;
                            let bootstrap_method = *bootstrap_methods
                                .entry(indy_method.bootstrap)
                                .or_insert(next_bootstrap_index);
                            let method_utf8 =
                                constants.borrow_mut().get_utf8(indy_method.name.as_str())?;
                            let desc_utf8 = constants
                                .borrow_mut()
                                .get_utf8(&indy_method.descriptor.render())?;
                            let name_and_type_idx = constants
                                .borrow_mut()
                                .get_name_and_type(method_utf8, desc_utf8)?;
                            constants
                                .borrow_mut()
                                .get_invoke_dynamic(bootstrap_method as u16, name_and_type_idx)
                        },
                    )
                },
            )
            .collect::<Result<OffsetVec<SerializableInstruction>, ConstantPoolOverflow>>()?;

        // Ensure the branch instruction has the right padding
        let mut branch_end = self.branch_end;
        let branch_off = offset_from_start.0 + instructions.offset_len().0 + 1;
        let padding = match (branch_off % 4) as u8 {
            0 => 0,
            x => 4 - x,
        };
        branch_end.set_padding(padding);

        Ok(BasicBlock {
            frame: self.frame,
            instructions,
            branch_end,
        })
    }
}

#[cfg(test)]
mod serialize_test {

    use super::*;
    use crate::jvm::code::Instruction::*;

    #[derive(Debug, Eq, PartialEq)]
    struct NoFrame;
    type SimpleBasicBlock<'g> =
        BasicBlock<NoFrame, VerifierInstruction<'g>, BranchInstruction<usize, usize, usize>>;

    fn serialize_basic_block<'g>(
        block: SimpleBasicBlock<'g>,
    ) -> BasicBlock<NoFrame, SerializableInstruction, BranchInstruction<usize, usize, usize>> {
        let mut constants = ConstantsPool::new();
        let mut bootstrap_methods = HashMap::new();
        block
            .serialize_instructions(&mut constants, &mut bootstrap_methods, Offset(0))
            .unwrap()
    }

    #[test]
    fn empty_block() {
        let empty_block = BasicBlock {
            frame: NoFrame,
            instructions: OffsetVec::new(),
            branch_end: BranchInstruction::FallThrough(0),
        };
        let serialized_empty_block = BasicBlock {
            frame: NoFrame,
            instructions: OffsetVec::new(),
            branch_end: BranchInstruction::FallThrough(0),
        };

        assert_eq!(serialize_basic_block(empty_block), serialized_empty_block);
    }

    #[test]
    fn padding_lookup_branch_end() {
        fn make_block<const N: usize, Insn: Width>(
            padding: u8,
            instructions: [Insn; N],
        ) -> BasicBlock<NoFrame, Insn, BranchInstruction<usize, usize, usize>> {
            BasicBlock {
                frame: NoFrame,
                instructions: OffsetVec::from(instructions),
                branch_end: BranchInstruction::TableSwitch {
                    padding,
                    default: 1,
                    low: 0,
                    targets: vec![2, 3, 4],
                },
            }
        }

        /* The expected padding for the following really is 2->1->0->3 and not 3->2->1->0. Why?
         * Because recall the padding occurs _after_ the single byte that specifies a `TableSwitch`
         * is coming (and its purpose is to align the offset of `default` to a 4-byte multiple)
         */
        assert_eq!(
            serialize_basic_block(make_block(0, [IConst0])),
            make_block(2, [IConst0]),
        );
        assert_eq!(
            serialize_basic_block(make_block(0, [IConst0, IConst1])),
            make_block(1, [IConst0, IConst1]),
        );
        assert_eq!(
            serialize_basic_block(make_block(0, [IConst0, IConst1, IAdd])),
            make_block(0, [IConst0, IConst1, IAdd]),
        );
        assert_eq!(
            serialize_basic_block(make_block(0, [IConst0, IConst1, IConst2, IAdd])),
            make_block(3, [IConst0, IConst1, IConst2, IAdd]),
        );
    }
}
