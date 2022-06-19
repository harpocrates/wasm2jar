use super::*;
use wasmparser::types::Types;
use wasmparser::{BlockType, FuncType, ValidatorResources, WasmModuleResources};

pub trait WasmModuleResourcesExt {
    fn function_at_idx(&self, func_idx: u32) -> Option<&FuncType>;

    fn function_type_at_idx(&self, type_idx: u32) -> Option<&FuncType>;

    /// Query a block type
    fn block_type(&self, typ: BlockType) -> Result<FunctionType, BadType> {
        match typ {
            BlockType::Empty => Ok(FunctionType {
                inputs: vec![],
                outputs: vec![],
            }),
            BlockType::Type(typ) => Ok(FunctionType {
                inputs: vec![],
                outputs: vec![StackType::from_general(typ)?],
            }),
            BlockType::FuncType(type_idx) => {
                let func = self
                    .function_type_at_idx(type_idx)
                    .ok_or(BadType::MissingTypeIdx(type_idx))?;
                FunctionType::from_general(func)
            }
        }
    }

    /// Query a function type from a function index
    fn function_idx_type(&self, func_idx: u32) -> Result<FunctionType, BadType> {
        let func = self
            .function_at_idx(func_idx)
            .ok_or(BadType::MissingFuncIdx(func_idx))?;
        FunctionType::from_general(func)
    }
}

impl WasmModuleResourcesExt for ValidatorResources {
    fn function_at_idx(&self, func_idx: u32) -> Option<&FuncType> {
        self.type_of_function(func_idx)
    }

    fn function_type_at_idx(&self, type_idx: u32) -> Option<&FuncType> {
        self.func_type_at(type_idx)
    }
}

impl WasmModuleResourcesExt for Types {
    fn function_at_idx(&self, func_idx: u32) -> Option<&FuncType> {
        self.function_at(func_idx)
    }

    fn function_type_at_idx(&self, type_idx: u32) -> Option<&FuncType> {
        self.func_type_at(type_idx)
    }
}
