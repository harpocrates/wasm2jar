use crate::jvm;
use wasmparser::{BinaryReaderError, Type, TypeOrFuncType};

#[derive(Debug)]
pub enum Error {
    BytecodeGen(jvm::Error),
    WasmParser(BinaryReaderError),
    UnsupportedStackType(Type),
    UnsupportedReferenceType(Type),
    UnsupportedFunctionType(TypeOrFuncType),
    LocalsOverflow,
}

impl From<jvm::Error> for Error {
    fn from(err: jvm::Error) -> Error {
        Error::BytecodeGen(err)
    }
}

impl From<BinaryReaderError> for Error {
    fn from(err: BinaryReaderError) -> Error {
        Error::WasmParser(err)
    }
}
