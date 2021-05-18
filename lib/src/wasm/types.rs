use crate::jvm::{FieldType, MethodDescriptor, RefType, Width};
use wasmparser::{Type, WasmFuncType};

/// Subset of WASM types that we know how to put on the WASM stack
#[derive(Debug, Copy, Clone)]
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
    pub const fn field_type(self) -> FieldType {
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
    pub const fn from_general(wasm_type: Type) -> Result<StackType, BadType> {
        Ok(match wasm_type {
            Type::I32 => StackType::I32,
            Type::I64 => StackType::I64,
            Type::F32 => StackType::F32,
            Type::F64 => StackType::F64,
            Type::FuncRef => StackType::FuncRef,
            Type::ExternRef => StackType::ExternRef,
            _ => return Err(BadType::UnsupportedType(wasm_type)),
        })
    }
}

/// Mapping from general types into reference types
pub const fn ref_type_from_general(wasm_type: Type) -> Result<RefType, BadType> {
    Ok(match wasm_type {
        Type::FuncRef => RefType::METHOD_HANDLE_CLASS,
        Type::ExternRef => RefType::OBJECT_CLASS,
        _ => return Err(BadType::UnsupportedReferenceType(wasm_type)),
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
#[derive(Debug)]
pub struct FunctionType {
    pub inputs: Vec<StackType>,
    pub outputs: Vec<StackType>,
}

impl FunctionType {
    /// Mapping from general types into stack types
    pub fn from_general<F: WasmFuncType>(wasm_type: &F) -> Result<FunctionType, BadType> {
        let inputs = wasm_type
            .inputs()
            .map(StackType::from_general)
            .collect::<Result<Vec<StackType>, BadType>>()?;

        let outputs = wasm_type
            .outputs()
            .map(StackType::from_general)
            .collect::<Result<Vec<StackType>, BadType>>()?;

        Ok(FunctionType { inputs, outputs })
    }

    /// Into a method descriptor
    pub fn method_descriptor(&self) -> MethodDescriptor {
        if self.outputs.len() > 1 {
            todo!()
        } else {
            let parameters = self.inputs.iter().map(|input| input.field_type()).collect();
            let return_type = self.outputs.iter().next().map(|output| output.field_type());
            MethodDescriptor {
                parameters,
                return_type,
            }
        }
    }
}

/// WASM type for a table
#[derive(Copy, Clone, Debug)]
pub enum TableType {
    FuncRef,
    ExternRef,
}

impl TableType {
    /// Convert a stack type into the corresponding JVM reference typ
    pub const fn ref_type(self) -> RefType {
        match self {
            TableType::FuncRef => RefType::METHOD_HANDLE_CLASS,
            TableType::ExternRef => RefType::OBJECT_CLASS,
        }
    }

    /// Convert a stack type into the corresponding JVM type
    pub const fn field_type(self) -> FieldType {
        FieldType::Ref(self.ref_type())
    }

    /// Mapping from general types into table types
    pub const fn from_general(wasm_type: Type) -> Result<TableType, BadType> {
        Ok(match wasm_type {
            Type::FuncRef => TableType::FuncRef,
            Type::ExternRef => TableType::ExternRef,
            _ => return Err(BadType::UnsupportedTableType(wasm_type)),
        })
    }
}

/// Ways in which types can go wrong
#[derive(Debug)]
pub enum BadType {
    UnsupportedType(Type),
    UnsupportedReferenceType(Type),
    UnsupportedTableType(Type),
    MissingTypeIdx(u32),
    MissingFuncIdx(u32),
}