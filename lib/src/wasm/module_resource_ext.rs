use super::*;
use wasmparser::{Type, TypeOrFuncType, WasmModuleResources};

pub trait WasmModuleResourcesExt: WasmModuleResources {
    /// Query a block type
    fn block_type(&self, typ: TypeOrFuncType) -> Result<FunctionType, BadType> {
        if let TypeOrFuncType::Type(Type::EmptyBlockType) = typ {
            Ok(FunctionType {
                inputs: vec![],
                outputs: vec![],
            })
        } else {
            self.function_type(typ)
        }
    }

    /// Query a function type
    fn function_type(&self, typ: TypeOrFuncType) -> Result<FunctionType, BadType> {
        match typ {
            TypeOrFuncType::Type(typ) => Ok(FunctionType {
                inputs: vec![],
                outputs: vec![StackType::from_general(typ)?],
            }),
            TypeOrFuncType::FuncType(type_idx) => {
                let func = self
                    .func_type_at(type_idx)
                    .ok_or(BadType::MissingTypeIdx(type_idx))?;
                FunctionType::from_general(func)
            }
        }
    }

    /// Query a function type from a function index
    fn function_idx_type(&self, func_idx: u32) -> Result<FunctionType, BadType> {
        let func = self
            .type_of_function(func_idx)
            .ok_or(BadType::MissingFuncIdx(func_idx))?;
        FunctionType::from_general(func)
    }
}

impl<A: WasmModuleResources> WasmModuleResourcesExt for A {}
