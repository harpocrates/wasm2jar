use super::{
    BranchInstruction, Constant, ConstantIndex, Frame, Instruction, Offset, RefType, SynLabel,
    VerificationType,
};

pub enum Error {
    ConstantPoolOverflow {
        constant: Constant,
        offset: usize,
    },
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
        instruction: Instruction,
        kind: VerifierErrorKind,
    },
    VerifierBranchingError {
        instruction: BranchInstruction<SynLabel, SynLabel, ()>,
        kind: VerifierErrorKind,
    },

    /// A label needs to have incompatible frames
    IncompatibleFrames(
        SynLabel,
        Frame<RefType, (RefType, Offset)>,
        Frame<RefType, (RefType, Offset)>,
    ),
}

pub enum VerifierErrorKind {
    EmptyStack,
    InvalidWidth(usize),
    NotArrayType,
    InvalidIndex,
    InvalidType,
    MissingConstant(ConstantIndex),
    NotLoadableConstant(Constant),
    IncompatibleTypes(
        VerificationType<RefType, (RefType, Offset)>,
        VerificationType<RefType, (RefType, Offset)>,
    ),
    BadDescriptor(String),
}
