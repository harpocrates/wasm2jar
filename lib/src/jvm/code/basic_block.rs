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

impl<'g, Frame>
    BasicBlock<Frame, VerifierInstruction<'g>, BranchInstruction<SynLabel, SynLabel, SynLabel>>
{
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
        BasicBlock<Frame, SerializableInstruction, BranchInstruction<SynLabel, SynLabel, SynLabel>>,
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
                            Ok(constants
                                .borrow_mut()
                                .get_invoke_dynamic(bootstrap_method as u16, name_and_type_idx)?)
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
