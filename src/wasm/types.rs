use crate::jvm::{FieldType, RefType, Width};
use wasmparser::{Type, WasmFuncType};

/// Subset of WASM types that we know how to put on the WASM stack
#[derive(Copy, Clone)]
pub enum StackType {
    I32,
    I64,
    F32,
    F64,
    FuncRef,
    ExternRef,
}

impl StackType {
    /// Convert a stack type into the corresponding JVM type
    pub const fn field_type(&self) -> FieldType {
        match self {
            StackType::I32 => FieldType::INT,
            StackType::I64 => FieldType::LONG,
            StackType::F32 => FieldType::FLOAT,
            StackType::F64 => FieldType::DOUBLE,
            StackType::FuncRef => FieldType::Ref(RefType::METHOD_HANDLE_CLASS),
            StackType::ExternRef => FieldType::Ref(RefType::OBJECT_CLASS),
        }
    }

    /// Mapping from general types into stack types
    pub const fn from_general(wasm_type: &Type) -> Option<StackType> {
        Some(match wasm_type {
            Type::I32 => StackType::I32,
            Type::I64 => StackType::I64,
            Type::F32 => StackType::F32,
            Type::F64 => StackType::F64,
            Type::FuncRef => StackType::FuncRef,
            Type::ExternRef => StackType::ExternRef,
            _ => return None,
        })
    }
}

/// Mapping from general types into reference types
pub const fn ref_type_from_general(wasm_type: &Type) -> Option<RefType> {
    Some(match wasm_type {
        Type::FuncRef => RefType::METHOD_HANDLE_CLASS,
        Type::ExternRef => RefType::OBJECT_CLASS,
        _ => return None,
    })
}

impl Width for StackType {
    fn width(&self) -> usize {
        match self {
            StackType::I32 | StackType::F32 | StackType::FuncRef | StackType::ExternRef => 1,
            StackType::I64 | StackType::F64 => 2,
        }
    }
}

/// WASM type of a function or block
pub struct FunctionType {
    pub inputs: Vec<StackType>,
    pub outputs: Vec<StackType>,
}

impl FunctionType {
    /// Mapping from general types into stack types
    pub fn from_general<F: WasmFuncType>(wasm_type: &F) -> Option<FunctionType> {
        let mut inputs = vec![];
        for input in wasm_type.inputs() {
            match StackType::from_general(&input) {
                Some(input_type) => inputs.push(input_type),
                None => return None,
            }
        }

        let mut outputs = vec![];
        for output in wasm_type.outputs() {
            match StackType::from_general(&output) {
                Some(output_type) => outputs.push(output_type),
                None => return None,
            }
        }

        Some(FunctionType { inputs, outputs })
    }
}
