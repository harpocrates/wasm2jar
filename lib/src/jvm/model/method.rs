use crate::jvm::constants_writer::ConstantsWriter;
use crate::jvm::descriptors::RenderDescriptor;
use crate::jvm::names::Name;
use crate::jvm::verifier::VerifierFrame;
use crate::jvm::{
    BootstrapMethodId, ClassId, ConstantPoolOverflow, ConstantsPool, Instruction, JumpTargets,
    MethodId, VerifierInstruction,
};
use crate::jvm::{BranchInstruction, SerializableInstruction};
use crate::util::{Offset, OffsetVec, RefId, Width};
use std::cell::Cell;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;
use std::rc::Rc;

/// In-memory representation of a method
pub struct Method<'g> {
    /// The current method
    pub method: MethodId<'g>,

    /// Method code implementation
    pub code_impl: Option<Code<'g>>,

    /// Which exceptions can this method throw?
    ///
    /// Note: this does not need to include `RuntimeException`, `Error`, or subclasses
    pub exceptions: Vec<ClassId<'g>>,

    /// Generic method signature
    ///
    /// [Format](https://docs.oracle.com/javase/specs/jvms/se11/html/jvms-4.html#jvms-4.7.9.1)
    pub generic_signature: Option<String>,
}

/// Method code.
pub struct Code<'g> {
    /// Maximum size of locals through the method
    pub max_locals: Offset,

    /// Maximum size of stack through the method
    pub max_stack: Offset,

    /// Basic blocks in the code
    pub blocks: HashMap<SynLabel, SerializableBasicBlock<'g>>,

    /// Order of basic blocks in the code (elements are unique and exactly match keys of `blocks`)
    pub block_order: Vec<SynLabel>,
}

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
    /// Offset of the start of the basic block from the start of the method
    pub offset_from_start: Offset,

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
        constants: &ConstantsPool,
        bootstrap_methods: &mut HashMap<BootstrapMethodId<'g>, u16>,
        offset_from_start: Offset,
    ) -> Result<
        BasicBlock<Frame, SerializableInstruction, BranchInstruction<SynLabel, SynLabel, ()>>,
        ConstantPoolOverflow,
    > {
        // Serialize the instructions
        let instructions =
            self.instructions
                .iter()
                .map(
                    |(_, _, insn)| -> Result<SerializableInstruction, ConstantPoolOverflow> {
                        insn.map(
                            |class| class.constant_index(constants),
                            |constant| constant.constant_index(constants),
                            |field| field.constant_index(constants),
                            |method| method.constant_index(constants),
                            |indy_method| -> Result<_, ConstantPoolOverflow> {
                                let next_bootstrap_index = bootstrap_methods.len() as u16;
                                let bootstrap_method = *bootstrap_methods
                                    .entry(indy_method.bootstrap)
                                    .or_insert(next_bootstrap_index);
                                let method_utf8 = constants.get_utf8(indy_method.name.as_str())?;
                                let desc_utf8 =
                                    constants.get_utf8(&indy_method.descriptor.render())?;
                                let name_and_type_idx =
                                    constants.get_name_and_type(method_utf8, desc_utf8)?;
                                Ok(constants
                                    .get_invoke_dynamic(bootstrap_method, name_and_type_idx)?)
                            },
                        )
                    },
                )
                .collect::<Result<OffsetVec<SerializableInstruction>, ConstantPoolOverflow>>()?;

        // Ensure the branch instruction has the right padding
        let mut branch_end = self
            .branch_end
            .map_labels(|lbl| *lbl, |lbl| *lbl, |_lbl| ());
        let branch_off = offset_from_start.0 + instructions.offset_len().0;
        let padding = match (branch_off % 4) as u8 {
            0 => 0,
            x => 4 - x,
        };
        branch_end.set_padding(padding);

        Ok(BasicBlock {
            offset_from_start,
            frame: self.frame,
            instructions,
            branch_end,
        })
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

#[derive(Clone)]
pub struct SynLabelGenerator(SynLabel);

impl SynLabelGenerator {
    pub fn new(start: SynLabel) -> SynLabelGenerator {
        SynLabelGenerator(start)
    }

    pub fn fresh_label(&mut self) -> SynLabel {
        let to_return = self.0.clone();
        self.0 = self.0.next();
        to_return
    }
}

impl fmt::Debug for SynLabel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_fmt(format_args!("l{}", self.0))
    }
}
