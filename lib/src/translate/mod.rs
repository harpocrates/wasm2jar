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

use crate::jvm::UnqualifiedName;
use crate::wasm::{StackType, TableType};
use wasmparser::{ElementItem, ElementKind, FuncType, InitExpr, ResizableLimits};

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
#[derive(Debug)]
pub struct Table {
    /// Where is the table defined?
    pub origin: MemberOrigin,

    /// Name of the method in the class (if exported, this matches the export name)
    pub field_name: UnqualifiedName,

    /// Table type
    pub table_type: TableType,

    /// Table limits
    pub limits: ResizableLimits,
}

pub struct Global<'a> {
    /// Where is the table defined?
    pub origin: MemberOrigin,

    /// Name of the method in the class (if exported, this matches the export name)
    pub field_name: UnqualifiedName,

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
    pub items: Vec<ElementItem>,
}

/// WASM functions are represented as methods
pub struct Function {
    /// Where is the function defined?
    pub origin: MemberOrigin,

    /// Name of the method in the class
    pub method_name: UnqualifiedName,

    /// Function type
    pub func_type: FuncType,
}
