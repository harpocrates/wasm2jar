mod code_builder_exts;
mod errors;
mod function;
mod module;
mod renamer;
mod settings;
mod utility;

pub use code_builder_exts::*;
pub use errors::*;
pub use function::*;
pub use module::*;
pub use renamer::*;
pub use settings::*;
pub use utility::*;

use crate::jvm::{FieldData, MethodData, UnqualifiedName};
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
    pub field: Option<&'g FieldData<'g>>,

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
    pub field: Option<&'g FieldData<'g>>,

    /// Memory type
    pub memory_type: MemoryType,
}

pub struct Global<'a, 'g> {
    /// Where is the table defined?
    pub origin: MemberOrigin,

    /// Name of the field in the class (if exported, this matches the export name)
    pub field_name: UnqualifiedName,

    /// Field in the class which stores the global
    pub field: Option<&'g FieldData<'g>>,

    /// Global type
    pub global_type: StackType,

    /// Is the global mutable?
    pub mutable: bool,

    /// Initial value
    pub initial: Option<InitExpr<'a>>,
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
    pub method: &'g MethodData<'g>,

    /// If the function contains a `return_call` or `return_call_indirect`, this is the method that
    /// should be used when the function is itself used in a tail call.
    ///
    /// It has the same signature as `method` except that the return value will either be the
    /// (boxed) return value of `method` or a thunk to evaluate.
    pub tailcall_method: Option<&'g MethodData<'g>>,

    /// If the function is imported, this contains the name under which it is imported along with
    /// the field (no the main WASM object) holding the method handle
    pub import: Option<(ImportName<'a>, &'g FieldData<'g>)>,

    /// If the function is exported, this holds the export information
    ///
    /// The boolean indicates whether we should _also_ generate a public (non-static) method on the
    /// WASM module object. This doesn't fit in a generalized export framework, but it is very
    /// convenient for functions.
    pub export: Option<(ExportName<'a>, bool)>
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

