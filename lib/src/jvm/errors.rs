use super::{
    BranchInstruction, ClassConstantIndex, Constant, ConstantIndex, Frame, Instruction, Offset,
    RefType, SynLabel, VerificationType,
};

#[derive(Debug)]
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
        instruction: String, //  Instruction<'a>
        kind: VerifierErrorKind,
    },
    VerifierBranchingError {
        instruction: BranchInstruction<SynLabel, SynLabel, ()>,
        kind: VerifierErrorKind,
    },

    /// A label needs to have incompatible frames
    IncompatibleFrames(
        SynLabel,
        Frame<String, String>,
        Frame<String, String>,
    ),

    /// A particular offset has two conflicting frames
    ConflictingFrames(
        Offset,
        Frame<ClassConstantIndex, u16>,
        Frame<ClassConstantIndex, u16>,
    ),

    MissingClass(String),
    MissingMember(String),
    AmbiguousMethod(String, String),
}

#[derive(Debug)]
pub enum VerifierErrorKind {
    EmptyStack,
    InvalidWidth(usize),
    NotArrayType,
    InvalidIndex,
    InvalidType,
    MissingConstant(ConstantIndex),
    NotLoadableConstant(Constant),
    IncompatibleTypes(
        VerificationType<String, (String, Offset)>,
        VerificationType<String, (String, Offset)>,
    ),
    BadDescriptor(String),
}
