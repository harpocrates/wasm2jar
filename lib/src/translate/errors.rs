use crate::jvm;
use crate::wasm;

#[derive(Debug)]
pub enum Error {
    BytecodeGen(jvm::Error),
    WasmParser(wasmparser::BinaryReaderError),
    UnsupportedType(wasm::BadType),
    LocalsOverflow,
}

impl From<jvm::Error> for Error {
    fn from(err: jvm::Error) -> Error {
        Error::BytecodeGen(err)
    }
}

impl From<wasm::BadType> for Error {
    fn from(err: wasm::BadType) -> Error {
        Error::UnsupportedType(err)
    }
}

impl From<wasmparser::BinaryReaderError> for Error {
    fn from(err: wasmparser::BinaryReaderError) -> Error {
        Error::WasmParser(err)
    }
}
