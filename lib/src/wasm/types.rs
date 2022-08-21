use crate::jvm::{ClassId, FieldType, JavaClasses, MethodDescriptor, RefType};
use crate::util::Width;
use wasmparser::{ValType, WasmFuncType};

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
    pub const fn field_type<'g>(self, java: &JavaClasses<'g>) -> FieldType<ClassId<'g>> {
        match self {
            StackType::I32 => FieldType::int(),
            StackType::I64 => FieldType::long(),
            StackType::F32 => FieldType::float(),
            StackType::F64 => FieldType::double(),
            StackType::FuncRef => FieldType::object(java.lang.invoke.method_handle),
            StackType::ExternRef => FieldType::object(java.lang.object),
        }
    }

    /// Mapping from general types into stack types
    pub const fn from_general(wasm_type: ValType) -> Result<StackType, BadType> {
        Ok(match wasm_type {
            ValType::I32 => StackType::I32,
            ValType::I64 => StackType::I64,
            ValType::F32 => StackType::F32,
            ValType::F64 => StackType::F64,
            ValType::FuncRef => StackType::FuncRef,
            ValType::ExternRef => StackType::ExternRef,
            _ => return Err(BadType::UnsupportedType(wasm_type)),
        })
    }
}

/// Mapping from general types into reference types
pub const fn ref_type_from_general<'g>(
    wasm_type: ValType,
    java: &JavaClasses<'g>,
) -> Result<RefType<ClassId<'g>>, BadType> {
    Ok(match wasm_type {
        ValType::FuncRef => RefType::Object(java.lang.invoke.method_handle),
        ValType::ExternRef => RefType::Object(java.lang.object),
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
#[derive(Clone, Debug)]
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
    pub fn method_descriptor<'g>(
        &self,
        java: &JavaClasses<'g>,
    ) -> MethodDescriptor<ClassId<'g>> {
        let return_type = match self.outputs.as_slice() {
            [] => None,
            [output_ty] => Some(output_ty.field_type(java)),
            _ => Some(FieldType::array(FieldType::object(java.lang.object))),
        };
        let parameters = self
            .inputs
            .iter()
            .map(|input| input.field_type(java))
            .collect();
        MethodDescriptor {
            parameters,
            return_type,
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
    /// Convert a stack type into the corresponding JVM reference type
    pub const fn ref_type<'g>(self, java: &JavaClasses<'g>) -> RefType<ClassId<'g>> {
        match self {
            TableType::FuncRef => RefType::Object(java.lang.invoke.method_handle),
            TableType::ExternRef => RefType::Object(java.lang.object),
        }
    }

    /// Convert a stack type into the corresponding JVM type
    pub const fn field_type<'g>(self, java: &JavaClasses<'g>) -> FieldType<ClassId<'g>> {
        FieldType::Ref(self.ref_type(java))
    }

    /// Mapping from general types into table types
    pub const fn from_general(wasm_type: ValType) -> Result<TableType, BadType> {
        Ok(match wasm_type {
            ValType::FuncRef => TableType::FuncRef,
            ValType::ExternRef => TableType::ExternRef,
            _ => return Err(BadType::UnsupportedTableType(wasm_type)),
        })
    }
}

/// Ways in which types can go wrong
#[derive(Debug)]
pub enum BadType {
    UnsupportedType(ValType),
    UnsupportedReferenceType(ValType),
    UnsupportedTableType(ValType),
    MissingTypeIdx(u32),
    MissingFuncIdx(u32),
}
