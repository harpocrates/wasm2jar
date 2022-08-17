use crate::jvm::{MethodData, BranchInstruction, SerializableInstruction, ClassData};
use crate::util::{Offset, OffsetVec, Width};
use crate::jvm::verifier::VerifierFrame;
use std::collections::HashMap;
use std::fmt;

/// In-memory representation of a method
pub struct Method<'g> {
    /// The current method
    pub method: &'g MethodData<'g>,

    /// Method code implementation
    pub code_impl: Option<Code<'g>>,

    /// Which exceptions can this method throw?
    ///
    /// Note: this does not need to include `RuntimeException`, `Error`, or subclasses
    pub exceptions: Vec<&'g ClassData<'g>>,

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
    pub blocks: HashMap<SynLabel, BasicBlock<'g>>,

    /// Order of basic blocks in the code (elements are unique and exactly match keys of `blocks`)
    pub block_order: Vec<SynLabel>,

}

/// A JVM method code body is made up of a linear sequence of basic blocks.
///
/// We also store some extra information that ultimately allows us to compute things like: the
/// maximum height of the locals, the maximum height of the stack, and the stack map frames.
#[derive(Debug)]
pub struct BasicBlock<'g> {
    /// Offset of the start of the basic block from the start of the method
    pub offset_from_start: Offset,

    /// Frame at the start of the block
    pub frame: VerifierFrame<'g>,

    /// Straight-line instructions in the block
    pub instructions: OffsetVec<SerializableInstruction>,

    /// Branch instruction to close the block
    pub branch_end: BranchInstruction<SynLabel, SynLabel, SynLabel>,
}

impl<'g> Width for BasicBlock<'g> {
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
