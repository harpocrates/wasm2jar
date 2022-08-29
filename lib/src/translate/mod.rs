mod errors;
mod function;
mod module;
mod renamer;
mod settings;
mod utility;

pub use errors::*;
pub use function::*;
pub use module::*;
pub use renamer::*;
pub use settings::*;
pub use utility::*;

use crate::jvm::class_graph::{AccessMode, FieldId, MethodId};
use crate::jvm::code::{CodeBuilderExts, CodeBuilder};
use crate::jvm::UnqualifiedName;
use crate::wasm::{FunctionType, StackType, TableType};
use wasmparser::{ElementItem, ElementKind, InitExpr, MemoryType};

/// Visibility of different importable/exportable entities in the WASM module
#[derive(Debug)]
pub struct MemberOrigin {
    /// If imported, which module is it imported from?
    imported: Option<Option<String>>,

    /// Is it exported?
    exported: bool,
}

impl MemberOrigin {
    /// A member is internal if it is not imported or exported
    pub fn is_internal(&self) -> bool {
        !self.exported && self.imported.is_none()
    }
}

/// WASM table
///
/// Internal tables are represented as fields on the module that have array types. Since tables
/// types are constrained, we have only two cases to handle:
///
///   * Function reference tables have type `[Ljava/lang/invoke/MethodHandle;`
///   * External reference tables have type `[Ljava/lang/Object;`
///
/// External (imported or exported) tables require an extra layer of indirected. They are more
/// complicated because they can be altered (even resized) from the outside or be aliased with
/// different names (an imported table can be re-exported under a different name).
pub struct Table<'g> {
    /// Where is the table defined?
    pub origin: MemberOrigin,

    /// Name of the method in the class (if exported, this matches the export name)
    pub field_name: UnqualifiedName,

    /// Field in the class which stores the table
    pub field: Option<FieldId<'g>>,

    /// Table type
    pub table_type: TableType,

    /// Table initial size
    pub initial: u32,

    /// Table maximum size
    pub maximum: Option<u32>,
}

pub struct Memory<'g> {
    /// Where is the memory defined?
    pub origin: MemberOrigin,

    /// Name of the field in the class (if exported, this matches the export name)
    pub field_name: UnqualifiedName,

    /// Field in the class which stores the memory
    pub field: Option<FieldId<'g>>,

    /// Memory type
    pub memory_type: MemoryType,
}

pub struct Global<'a, 'g> {
    /// Where is the table defined?
    pub origin: MemberOrigin,

    /// Field in the class which stores the global
    pub field: FieldId<'g>,

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
        code.access_field(self.field, AccessMode::Write)?;
        Ok(())
    }

    /// Read a global onto the stack
    ///
    /// Assumes the top of the stack is the WASM module class
    pub fn read(&self, code: &mut CodeBuilder<'g>) -> Result<(), Error> {
        code.access_field(self.field, AccessMode::Read)?;
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

pub struct Element<'a> {
    /// Type of element section
    pub kind: ElementKind<'a>,

    /// Element type
    pub element_type: TableType,

    /// Entries in the element
    pub items: Vec<ElementItem<'a>>,
}

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
    pub export: Option<(ExportName<'a>, bool)>,
}

pub struct ImportName<'a> {
    /// Name of the module from which the entity is imported
    pub module: &'a str,

    /// Name of the entity within the imported module
    pub name: &'a str,
}

pub struct ExportName<'a> {
    /// Name off the exported entity
    pub name: &'a str,
}
