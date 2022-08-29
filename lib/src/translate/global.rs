use crate::jvm::class_graph::{AccessMode, FieldId};
use crate::jvm::code::{CodeBuilderExts, CodeBuilder};
use crate::jvm::{Error, UnqualifiedName};
use crate::wasm::StackType;
use wasmparser::InitExpr;
use super::MemberOrigin;

pub struct Global<'a, 'g> {
    /// Where is the table defined?
    pub origin: MemberOrigin,

    /// Field in the class which stores the global
    ///
    /// Left as `None` for globals before we know whether they are exported or not (since until we
    /// know the `repr`, we don't know their type).
    pub field: Option<FieldId<'g>>,

    /// Field name
    pub field_name: UnqualifiedName,

    /// What is on the field?
    pub repr: GlobalRepr,

    /// Global type
    pub global_type: StackType,

    /// Is the global mutable?
    pub mutable: bool,

    /// Initial value
    pub initial: Option<InitExpr<'a>>,
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
    pub fn write(&self, code: &mut CodeBuilder<'g>) -> Result<(), Error> {
        match self.repr {
            GlobalRepr::UnboxedInternal => (),
            GlobalRepr::BoxedExternal =>
                match self.global_type {
                    StackType::I32 => code.invoke(code.java.members.lang.integer.value_of)?,
                    StackType::I64 => code.invoke(code.java.members.lang.long.value_of)?,
                    StackType::F32 => code.invoke(code.java.members.lang.float.value_of)?,
                    StackType::F64 => code.invoke(code.java.members.lang.double.value_of)?,
                    StackType::FuncRef | StackType::ExternRef => (),
                },
        }
        code.access_field(self.field.unwrap(), AccessMode::Write)?;
        Ok(())
    }

    /// Read a global onto the stack
    ///
    /// Assumes the top of the stack is the WASM module class
    pub fn read(&self, code: &mut CodeBuilder<'g>) -> Result<(), Error> {
        code.access_field(self.field.unwrap(), AccessMode::Read)?;
        match self.repr {
            GlobalRepr::UnboxedInternal => (),
            GlobalRepr::BoxedExternal =>
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
                },
        }
        Ok(())
    }
}


