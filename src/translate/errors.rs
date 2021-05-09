use crate::jvm;
use wasmparser::{BinaryReaderError, Type};

#[derive(Debug)]
pub enum Error {
    BytecodeGen(jvm::Error),
    WasmParser(BinaryReaderError),
    UnsupportedStackType(Type),
    LocalsOverflow,
}
