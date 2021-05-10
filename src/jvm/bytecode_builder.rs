use super::{BranchInstruction, ConstantsPool, Error, Frame, Instruction, Offset, RefType};
use std::cell::RefMut;
use std::fmt::Debug;

/// Abstract bytecode building trait
pub trait BytecodeBuilder<E: Debug = Error> {
    /// Block labels
    type Lbl: Copy + Eq + PartialEq;

    /// Generate a fresh label
    fn fresh_label(&mut self) -> Self::Lbl;

    /// Start a new block with the given label, ending the current block (if there is one) with a
    /// fallthrough. This can fail if:
    ///
    ///   * the label was already placed
    ///   * the label was already jumped to from elsewhere, and the frames don't match
    ///   * the label was not ever been jumped to and there is no fallthrough (so we have no way of
    ///     inferring the expected frame)
    ///
    fn place_label(&mut self, label: Self::Lbl) -> Result<(), E>;

    /// Push a new instruction to the current block
    fn push_instruction(&mut self, insn: Instruction) -> Result<(), E>;

    /// Push a new branch instruction to close the current block and possibly open a new one
    fn push_branch_instruction(
        &mut self,
        insn: BranchInstruction<Self::Lbl, Self::Lbl, ()>,
    ) -> Result<(), E>;

    /// Get the constant pool
    fn constants(&self) -> RefMut<ConstantsPool>;

    /// Get the current frame
    fn current_frame(&self) -> Option<&Frame<RefType, (RefType, Offset)>>;
}
