mod data;
mod element;
mod errors;
mod function;
mod global;
mod memory;
mod module;
mod renamer;
mod settings;
mod table;
mod utility;

pub use data::*;
pub use element::*;
pub use errors::*;
pub use function::*;
pub use global::*;
pub use memory::*;
pub use module::*;
pub use renamer::*;
pub use settings::*;
pub use table::*;
pub use utility::*;

use crate::jvm::class_graph::{ConstantData, FieldId, MethodId};
use crate::jvm::code::{CodeBuilder, CodeBuilderExts, Instruction};
use crate::runtime::WasmRuntime;
use crate::wasm::{ref_type_from_general, FunctionType};
use wasmparser::{ConstExpr, Operator};

/// WASM functions are represented as methods
pub struct Function<'a, 'g> {
    /// Function type
    pub func_type: FunctionType,

    /// Method in the class containing the implementation
    ///
    /// Note: the method will have an "adapted" signature, meaning there is always one final
    /// argument that is the module itself. In addition, it should always be a static method.
    pub method: MethodId<'g>,

    /// If the function contains a `return_call` or `return_call_indirect`, this is the method that
    /// should be used when the function is itself used in a tail call.
    ///
    /// It has the same signature as `method` except that the return value will either be the
    /// (boxed) return value of `method` or a thunk to evaluate.
    pub tailcall_method: Option<MethodId<'g>>,

    /// If the function is imported, this contains the name under which it is imported along with
    /// the field (on the main WASM object) holding the method handle.
    pub import: Option<(ImportName<'a>, FieldId<'g>)>,

    /// If the function is exported, this holds the export information
    ///
    /// The boolean indicates whether we should _also_ generate a public (non-static) method on the
    /// WASM module object. This doesn't fit in a generalized export framework, but it is very
    /// convenient for functions.
    pub export: Vec<(ExportName<'a>, bool)>,
}

#[derive(Debug)]
pub struct ImportName<'a> {
    /// Name of the module from which the entity is imported
    pub module: &'a str,

    /// Name of the entity within the imported module
    pub name: &'a str,
}

#[derive(Debug)]
pub struct ExportName<'a> {
    /// Name off the exported entity
    pub name: &'a str,
}

/// Tranlsate a constant expression
fn translate_const_expr<'a, 'g>(
    functions: &[Function<'a, 'g>],
    globals: &[Global<'a, 'g>],
    runtime: &WasmRuntime<'g>,
    wasm_module_variable_off: u16,
    code: &mut CodeBuilder<'g>,
    init_expr: &ConstExpr,
) -> Result<(), Error> {
    for operator in init_expr.get_operators_reader().into_iter() {
        match operator? {
            Operator::I32Const { value } => code.const_int(value)?,
            Operator::I64Const { value } => code.const_long(value)?,
            Operator::F32Const { value } => code.const_float(f32::from_bits(value.bits()))?,
            Operator::F64Const { value } => code.const_double(f64::from_bits(value.bits()))?,
            Operator::RefNull { ty } => {
                let ref_type = ref_type_from_general(ty, &code.java.classes)?;
                code.const_null(ref_type)?;
            }
            Operator::RefFunc { function_index } => {
                let method = functions[function_index as usize].method;
                let method_handle = ConstantData::MethodHandle(method);
                code.push_instruction(Instruction::Ldc(method_handle))?;
            }
            Operator::End => (),
            Operator::GlobalGet { global_index } => {
                code.push_instruction(Instruction::ALoad(wasm_module_variable_off))?;
                globals[global_index as usize].read(runtime, code)?;
            }
            other => todo!(
                "figure out which other expressions and valid, then rule out the rest {:?}",
                other
            ),
        }
    }

    Ok(())
}
