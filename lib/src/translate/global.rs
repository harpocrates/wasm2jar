use super::{ExportName, ImportName};
use crate::jvm::class_graph::{AccessMode, FieldId};
use crate::jvm::code::{CodeBuilder, CodeBuilderExts, Instruction};
use crate::jvm::Error;
use crate::runtime::WasmRuntime;
use crate::wasm::StackType;
use wasmparser::ConstExpr;

/// Translated global variable
///
/// Every global gets translated into a field stored on the main WASM module class, though the
/// particular representation depends on factors such as whether the global is imported/exported
/// and whether it is mutable or not.
pub struct Global<'a, 'g> {
    /// Field in the class which stores the global
    ///
    /// Left as `None` for globals before we know whether they are exported or not (since until we
    /// know the `repr`, we don't know their type).
    pub field: Option<FieldId<'g>>,

    /// What is on the field?
    pub repr: GlobalRepr,

    /// Global type
    pub global_type: StackType,

    /// Is the global mutable?
    pub mutable: bool,

    /// Initial value
    pub initial: Option<ConstExpr<'a>>,

    /// If the global is imported, this contains the name under which it is imported.
    pub import: Option<ImportName<'a>>,

    /// If the global is exported, this holds the export information
    pub export: Vec<ExportName<'a>>,
}

/// Representation of a global in a module
#[derive(Eq, PartialEq)]
pub enum GlobalRepr {
    /// Global uses a `org.wasm2jar.Global` field, with a boxed value inside
    BoxedExternal,

    /// Global uses an internal unboxed field
    UnboxedInternal,
}

impl<'a, 'g> Global<'a, 'g> {
    /// Write a global from the top of the operand stack
    ///
    /// Assumes the top of the stack is the new global value followed by the WASM module class
    pub fn write(
        &self,
        runtime: &WasmRuntime<'g>,
        code: &mut CodeBuilder<'g>,
    ) -> Result<(), Error> {
        match self.repr {
            GlobalRepr::UnboxedInternal => {
                code.access_field(self.field.unwrap(), AccessMode::Write)?;
            }
            GlobalRepr::BoxedExternal => {
                match self.global_type {
                    StackType::I32 => code.invoke(code.java.members.lang.integer.value_of)?,
                    StackType::I64 => code.invoke(code.java.members.lang.long.value_of)?,
                    StackType::F32 => code.invoke(code.java.members.lang.float.value_of)?,
                    StackType::F64 => code.invoke(code.java.members.lang.double.value_of)?,
                    StackType::FuncRef | StackType::ExternRef => (),
                }
                code.push_instruction(Instruction::Swap)?;
                code.access_field(self.field.unwrap(), AccessMode::Read)?;
                code.push_instruction(Instruction::Swap)?;
                code.access_field(runtime.members.global.value, AccessMode::Write)?;
            }
        }
        Ok(())
    }

    /// Read a global onto the stack
    ///
    /// Assumes the top of the stack is the WASM module class
    pub fn read(&self, runtime: &WasmRuntime<'g>, code: &mut CodeBuilder<'g>) -> Result<(), Error> {
        code.access_field(self.field.unwrap(), AccessMode::Read)?;
        match self.repr {
            GlobalRepr::UnboxedInternal => (),
            GlobalRepr::BoxedExternal => {
                code.access_field(runtime.members.global.value, AccessMode::Read)?;
                match self.global_type {
                    StackType::I32 => {
                        code.checkcast(code.java.classes.lang.integer)?;
                        code.invoke(code.java.members.lang.number.int_value)?;
                    }
                    StackType::I64 => {
                        code.checkcast(code.java.classes.lang.long)?;
                        code.invoke(code.java.members.lang.number.long_value)?;
                    }
                    StackType::F32 => {
                        code.checkcast(code.java.classes.lang.float)?;
                        code.invoke(code.java.members.lang.number.float_value)?;
                    }
                    StackType::F64 => {
                        code.checkcast(code.java.classes.lang.double)?;
                        code.invoke(code.java.members.lang.number.double_value)?;
                    }
                    StackType::FuncRef => {
                        code.checkcast(code.java.classes.lang.invoke.method_handle)?;
                    }
                    StackType::ExternRef => (),
                }
            }
        }
        Ok(())
    }
}
