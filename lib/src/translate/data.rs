use crate::jvm::class_graph::{AccessMode, ClassGraph, FieldId, JavaLibrary, MethodId};
use crate::jvm::code::{
    BranchInstruction, CodeBuilder, CodeBuilderExts, EqComparison, Instruction,
};
use crate::jvm::model::Method;
use crate::jvm::{BaseType, Error, FieldType};
use wasmparser::DataKind;

/// Translated data segment
///
/// Every data segment is turned into a static private method on the main WASM module. That method
/// takes as argument the WASM module and returns the byte array associated with the constant data.
pub struct Data<'a, 'g> {
    /// Kind of data segment (active vs. passive)
    pub kind: Option<DataKind<'a>>,

    /// Data bytes
    pub bytes: Option<&'a [u8]>,

    /// Static method in the class which returns the data bytes
    pub method: MethodId<'g>,

    /// Field in WASM class containing the data, if initialized
    pub field: FieldId<'g>,
}

impl<'a, 'g> Data<'a, 'g> {
    /// Generate the static method associated with the data segment
    pub fn generate_method(
        &self,
        class_graph: &'g ClassGraph<'g>,
        java: &'g JavaLibrary<'g>,
    ) -> Result<Method<'g>, Error> {
        let bytes = self.bytes.unwrap();
        let mut code = CodeBuilder::new(class_graph, java, self.method);
        let generate = code.fresh_label();
        let this_off = 0;

        // Check if the cached data field is non-null (and return it if so)
        code.push_instruction(Instruction::ALoad(this_off))?;
        code.access_field(self.field, AccessMode::Read)?;
        code.dup()?;
        code.push_branch_instruction(BranchInstruction::IfNull(EqComparison::EQ, generate, ()))?;
        code.return_(Some(FieldType::array(FieldType::byte())))?;
        code.place_label(generate)?;
        code.pop()?;

        // Prepare the byte array to return
        code.const_int(bytes.len() as i32)?;
        code.push_instruction(Instruction::NewArray(BaseType::Byte))?;
        code.invoke(java.members.nio.byte_buffer.wrap)?;

        // Copy in all of the data into the buffer
        for chunk in bytes.chunks(u16::MAX as usize) {
            code.const_string(chunk.iter().map(|&c| c as char).collect::<String>())?;
            code.const_string("ISO-8859-1")?;
            code.invoke(java.members.lang.string.get_bytes)?;
            code.invoke(java.members.nio.byte_buffer.put_bytearray_relative)?;
        }

        // Return the buffer
        code.invoke(code.java.members.nio.byte_buffer.array)?;
        code.dup()?;
        code.push_instruction(Instruction::ALoad(this_off))?;
        code.push_instruction(Instruction::Swap)?;
        code.access_field(self.field, AccessMode::Write)?;
        code.return_(Some(FieldType::array(FieldType::byte())))?;

        Ok(Method {
            id: self.method,
            code_impl: Some(code.result()?),
            exceptions: vec![],
            generic_signature: None,
        })
    }

    /// Generate code corresponding to dropping the data
    pub fn drop_data(&self, code: &mut CodeBuilder<'g>, this_off: u16) -> Result<(), Error> {
        code.push_instruction(Instruction::ALoad(this_off))?;
        code.const_int(0)?;
        code.push_instruction(Instruction::NewArray(BaseType::Byte))?;
        code.access_field(self.field, AccessMode::Write)?;

        Ok(())
    }
}
