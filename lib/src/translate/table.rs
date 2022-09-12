use super::{Element, ExportName, ImportName};
use crate::jvm::class_graph::{AccessMode, ClassId, FieldId, JavaClasses};
use crate::jvm::code::{CodeBuilder, CodeBuilderExts, Instruction};
use crate::jvm::{Error, RefType};
use crate::runtime::WasmRuntime;
use wasmparser::TableType;
use wasmparser::ValType;

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
pub struct Table<'a, 'g> {
    /// Field in the class which stores the table
    ///
    /// Left as `None` for tables before we know whether they are exported or not (since until we
    /// know the `repr`, we don't know their type).
    pub field: Option<FieldId<'g>>,

    /// What is on the field?
    pub repr: TableRepr,

    /// Table type
    pub table_type: TableType,

    /// If the memory is imported, this contains the name under which it is imported.
    pub import: Option<ImportName<'a>>,

    /// If the memory is exported, this holds the export information
    pub export: Vec<ExportName<'a>>,
}

#[derive(Copy, Clone)]
pub enum TableRepr {
    /// Table uses a `org.wasm2jar.*Table`
    External,

    /// Table uses an internal field
    Internal,
}

impl<'a, 'g> Table<'a, 'g> {
    /// Type of the element
    pub fn element_type(&self, java: &JavaClasses<'g>) -> RefType<ClassId<'g>> {
        match self.table_type.element_type {
            ValType::FuncRef => RefType::Object(java.lang.invoke.method_handle),
            ValType::ExternRef => RefType::Object(java.lang.object),
            _ => panic!(),
        }
    }

    /// Load the table array onto the stack
    ///
    /// Assumes the stack starts with having the main WASM module object on it
    pub fn load_array(
        &self,
        runtime: &WasmRuntime<'g>,
        code: &mut CodeBuilder<'g>,
    ) -> Result<(), Error> {
        code.access_field(self.field.unwrap(), AccessMode::Read)?;
        if let TableRepr::External = self.repr {
            let array_field = match self.table_type.element_type {
                wasmparser::ValType::FuncRef => runtime.members.function_table.table,
                wasmparser::ValType::ExternRef => runtime.members.reference_table.table,
                _ => panic!(),
            };
            code.access_field(array_field, AccessMode::Read)?;
        }

        Ok(())
    }

    /// Initialize table from an element
    ///
    /// Assumes the top of the stack is the number of elements to intiialize, followed by an offset
    /// into the element from which to start copying, followed by an offset in the table where to
    /// start writing.
    pub fn init(
        &self,
        runtime: &WasmRuntime<'g>,
        code: &mut CodeBuilder<'g>,
        this_off: u16,
        len_off: u16,
        src_off: u16,
        dst_off: u16,
        element: &Element<'a, 'g>,
    ) -> Result<(), Error> {
        // `System.arraycopy(wasm_elem(), src, table, dst, len)`
        code.push_instruction(Instruction::ALoad(this_off))?;
        code.invoke(element.method)?;
        code.push_instruction(Instruction::ILoad(src_off))?;
        code.push_instruction(Instruction::ALoad(this_off))?;
        self.load_array(runtime, code)?;
        code.push_instruction(Instruction::ILoad(dst_off))?;
        code.push_instruction(Instruction::ILoad(len_off))?;
        code.invoke(code.java.members.lang.system.arraycopy)?;

        Ok(())
    }
}
