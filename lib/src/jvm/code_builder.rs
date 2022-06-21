use super::{
    BranchInstruction, ClassGraph, ConstantsPool, Error, Frame, Instruction, Offset, RefType,
};
use std::fmt::Debug;

/// Abstract code building trait
pub trait CodeBuilder<E: Debug = Error> {
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

    /// Like `place_label`, but specifies an explicit frame. This rules out the failure mode of
    /// `place_label` for when there is no way of inferring the expected frame.
    ///
    /// TODO: switch `frame` to take a `Cow` (we often have the frame owned)
    fn place_label_with_frame(
        &mut self,
        label: Self::Lbl,
        frame: &Frame<RefType, (RefType, Offset)>,
    ) -> Result<(), E>;

    /// Push a new instruction to the current block
    fn push_instruction(&mut self, insn: Instruction) -> Result<(), E>;

    /// Push a new branch instruction to close the current block and possibly open a new one
    fn push_branch_instruction(
        &mut self,
        insn: BranchInstruction<Self::Lbl, Self::Lbl, ()>,
    ) -> Result<(), E>;

    /// Get the constant pool
    fn constants(&self) -> &ConstantsPool;

    /// Get the class graph
    fn class_graph(&self) -> &ClassGraph;

    /// Get the current frame
    fn current_frame(&self) -> Option<&Frame<RefType, (RefType, Offset)>>;
}
