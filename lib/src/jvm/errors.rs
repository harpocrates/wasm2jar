use super::{BinaryName, RefType};
use crate::jvm::class_file::{BytecodeIndex, ClassConstantIndex, ConstantPoolOverflow};
use crate::jvm::code::{BranchInstruction, SynLabel};
use crate::jvm::verifier::Frame;
use crate::util::Offset;

#[derive(Debug)]
pub enum Error {
    ConstantPoolOverflow(ConstantPoolOverflow),
    IoError(std::io::Error),
    MethodCodeMaxStackOverflow(Offset),
    MethodCodeMaxLocalsOverflow(Offset),
    MethodCodeOverflow(Offset),

    MethodCodeNotFinished {
        pending_block: Option<SynLabel>,
        unplaced_labels: Vec<SynLabel>,
    },

    /// Two blocks claim to have the same label (indicates a bug)
    DuplicateLabel(SynLabel),

    /// A label is placed before it has ever been referred to
    ///
    /// This is fixable by making sure you place the block _after_ some jump to it.
    PlacingLabelBeforeReference(SynLabel),

    /// Error trying to verify
    VerifierError {
        instruction: String, // VerifierInstruction<'g>,
        kind: VerifierErrorKind,
    },
    VerifierBranchingError {
        instruction: BranchInstruction<SynLabel, SynLabel, ()>,
        kind: VerifierErrorKind,
    },

    /// A label needs to have incompatible frames
    IncompatibleFrames(
        SynLabel,
        Frame<RefType<BinaryName>, (RefType<BinaryName>, Offset)>,
        Frame<RefType<BinaryName>, (RefType<BinaryName>, Offset)>,
    ),

    /// A particular offset has two conflicting frames
    ConflictingFrames(
        Offset,
        Frame<ClassConstantIndex, BytecodeIndex>,
        Frame<ClassConstantIndex, BytecodeIndex>,
    ),
}

impl From<ConstantPoolOverflow> for Error {
    fn from(overflow: ConstantPoolOverflow) -> Error {
        Error::ConstantPoolOverflow(overflow)
    }
}

#[derive(Debug)]
pub enum VerifierErrorKind {
    EmptyStack,
    InvalidWidth(usize),
    NotArrayType,
    InvalidIndex,
    InvalidType,
    BadDescriptor(String),
}
