use crate::jvm::class_graph::{AccessMode, ClassGraph, FieldId, JavaLibrary, MethodId};
use crate::jvm::code::{
    BranchInstruction, CodeBuilder, CodeBuilderExts, EqComparison, Instruction,
};
use crate::jvm::model::Method;
use crate::jvm::FieldType;
use crate::runtime::WasmRuntime;
use crate::translate::{ConstantData, Error, Function, Global};
use crate::wasm::TableType;
use wasmparser::{ElementItem, ElementKind};

pub struct Element<'a, 'g> {
    /// Type of element section
    pub kind: ElementKind<'a>,

    /// Element type
    pub element_type: TableType,

    /// Entries in the element
    pub items: Vec<ElementItem<'a>>,

    /// Function used to get the element
    ///
    /// This takes the module as an argument since elements can refer to imported globals.
    pub method: MethodId<'g>,

    /// Field in WASM class containing the data, if initialized
    pub field: FieldId<'g>,
}

impl<'a, 'g> Element<'a, 'g> {
    /// Generate the static method associated with the data segment
    pub fn generate_method(
        &self,
        class_graph: &'g ClassGraph<'g>,
        java: &'g JavaLibrary<'g>,
        runtime: &WasmRuntime<'g>,
        functions: &[Function<'a, 'g>],
        globals: &[Global<'a, 'g>],
    ) -> Result<Method<'g>, Error> {
        let mut code = CodeBuilder::new(class_graph, java, self.method);
        let this_off = 0;
        let offset_var = 1;
        let generate = code.fresh_label();

        // Check if the cached data field is non-null (and return it if so)
        code.push_instruction(Instruction::ALoad(this_off))?;
        code.access_field(self.field, AccessMode::Read)?;
        code.dup()?;
        code.push_branch_instruction(BranchInstruction::IfNull(EqComparison::EQ, generate, ()))?;
        code.return_(Some(FieldType::array(
            self.element_type.field_type(&java.classes),
        )))?;
        code.place_label(generate)?;
        code.pop()?;

        // Prepare the array to return
        code.const_int(self.items.len() as i32)?;
        code.push_instruction(Instruction::ANewArray(
            self.element_type.ref_type(&java.classes),
        ))?;

        // Index variable
        code.const_int(0)?;
        code.push_instruction(Instruction::IStore(offset_var))?;

        // Copy in all of the items into the element array
        for item in &self.items {
            code.push_instruction(Instruction::Dup)?;
            code.push_instruction(Instruction::ILoad(offset_var))?;
            match item {
                ElementItem::Func(func_idx) => {
                    let method = functions[*func_idx as usize].method;
                    let method_handle = ConstantData::MethodHandle(method);
                    code.push_instruction(Instruction::Ldc(method_handle))?;
                }
                ElementItem::Expr(elem_expr) => {
                    super::translate_const_expr(
                        functions, globals, runtime, this_off, &mut code, &elem_expr,
                    )?;
                }
            }
            code.push_instruction(Instruction::AAStore)?;
            code.push_instruction(Instruction::IInc(offset_var, 1))?;
        }

        // Return the array
        code.dup()?;
        code.push_instruction(Instruction::ALoad(this_off))?;
        code.push_instruction(Instruction::Swap)?;
        code.access_field(self.field, AccessMode::Write)?;
        code.return_(Some(FieldType::array(
            self.element_type.field_type(&java.classes),
        )))?;

        Ok(Method {
            id: self.method,
            code_impl: Some(code.result()?),
            exceptions: vec![],
            generic_signature: None,
        })
    }

    /// Generate code corresponding to dropping the element
    pub fn drop_element(&self, code: &mut CodeBuilder<'g>, this_off: u16) -> Result<(), Error> {
        code.push_instruction(Instruction::ALoad(this_off))?;
        code.const_int(0)?;
        code.push_instruction(Instruction::ANewArray(
            self.element_type.ref_type(&code.java.classes),
        ))?;
        code.access_field(self.field, AccessMode::Write)?;

        Ok(())
    }
}
