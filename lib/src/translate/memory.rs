use super::{ExportName, ImportName};
use crate::jvm::class_graph::{AccessMode, FieldId};
use crate::jvm::code::{CodeBuilder, CodeBuilderExts, Instruction};
use crate::jvm::{BaseType, Error, FieldType};
use crate::runtime::WasmRuntime;
use crate::util::Width;
use wasmparser::{MemArg, MemoryType};

#[derive(Debug)]
pub struct Memory<'a, 'g> {
    /// Field in the class which stores the memory
    ///
    /// Left as `None` for memories before we know whether they are exported or not (since until we
    /// know the `repr`, we don't know their type).
    pub field: Option<FieldId<'g>>,

    /// What is on the field?
    pub repr: MemoryRepr,

    /// Memory type
    pub memory_type: MemoryType,

    /// If the memory is imported, this contains the name under which it is imported.
    pub import: Option<ImportName<'a>>,

    /// If the memory is exported, this holds the export information
    pub export: Vec<ExportName<'a>>,
}

/// Representation of a memory in a module
#[derive(Debug)]
pub enum MemoryRepr {
    /// Memory uses a `org.wasm2jar.Memory` field
    External,

    /// Memory uses an internal field
    Internal,
}

impl<'a, 'g> Memory<'a, 'g> {
    pub fn is_resizable(&self) -> bool {
        !self
            .memory_type
            .maximum
            .filter(|e| e == &self.memory_type.initial)
            .is_some()
    }

    /// Load a value from memory onto the stack
    ///
    /// Assumes the top of the stack is the offset into the memory
    pub fn load(
        &self,
        runtime: &WasmRuntime<'g>,
        code: &mut CodeBuilder<'g>,
        this_off: u16,
        memarg: MemArg,
        ty: BaseType,
    ) -> Result<(), Error> {
        // Adjust the offset
        if memarg.offset != 0 {
            code.const_int(memarg.offset as i32)?; // TODO: overflow
            code.push_instruction(Instruction::IAdd)?;
        }

        // Load the memory
        code.push_instruction(Instruction::ALoad(this_off))?;
        code.access_field(self.field.unwrap(), AccessMode::Read)?;
        match self.repr {
            MemoryRepr::External => {
                code.access_field(runtime.members.memory.bytes, AccessMode::Read)?;
            }
            MemoryRepr::Internal => (),
        }

        // Re-order the stack and call the get function
        code.push_instruction(Instruction::Swap)?;
        let get_func = match ty {
            BaseType::Byte => code.java.members.nio.byte_buffer.get_byte,
            BaseType::Short => code.java.members.nio.byte_buffer.get_short,
            BaseType::Int => code.java.members.nio.byte_buffer.get_int,
            BaseType::Float => code.java.members.nio.byte_buffer.get_float,
            BaseType::Long => code.java.members.nio.byte_buffer.get_long,
            BaseType::Double => code.java.members.nio.byte_buffer.get_double,
            t => panic!("Cannot get {:?}", t),
        };
        code.invoke(get_func)?;

        Ok(())
    }

    /// Load a value from the stack to memory
    ///
    /// Assumes the top of the stack is the value followed by the offset into the memory
    pub fn store(
        &self,
        runtime: &WasmRuntime<'g>,
        code: &mut CodeBuilder<'g>,
        this_off: u16,
        temp_off: u16,
        memarg: MemArg,
        ty: BaseType,
    ) -> Result<(), Error> {
        if ty.width() == 1 && memarg.offset == 0 {
            // Load the memory
            code.push_instruction(Instruction::ALoad(this_off))?;
            code.access_field(self.field.unwrap(), AccessMode::Read)?;
            match self.repr {
                MemoryRepr::External => {
                    code.access_field(runtime.members.memory.bytes, AccessMode::Read)?;
                }
                MemoryRepr::Internal => (),
            }

            // Re-order the stack
            code.push_instruction(Instruction::DupX2)?;
            code.push_instruction(Instruction::Pop)?;
        } else {
            // Stash the value being stored
            code.set_local(temp_off, &FieldType::Base(ty))?;

            // Adjust the offset
            if memarg.offset != 0 {
                code.const_int(memarg.offset as i32)?; // TODO: overflow
                code.push_instruction(Instruction::IAdd)?;
            }

            // Load the memory
            code.push_instruction(Instruction::ALoad(this_off))?;
            code.access_field(self.field.unwrap(), AccessMode::Read)?;
            match self.repr {
                MemoryRepr::External => {
                    code.access_field(runtime.members.memory.bytes, AccessMode::Read)?;
                }
                MemoryRepr::Internal => (),
            }

            // Re-order the stack
            code.push_instruction(Instruction::Swap)?;
            code.get_local(temp_off, &FieldType::Base(ty))?;
            code.kill_top_local(temp_off, None)?;
        }

        // Call the store function
        let put_func = match ty {
            BaseType::Byte => code.java.members.nio.byte_buffer.put_byte,
            BaseType::Short => code.java.members.nio.byte_buffer.put_short,
            BaseType::Int => code.java.members.nio.byte_buffer.put_int,
            BaseType::Float => code.java.members.nio.byte_buffer.put_float,
            BaseType::Long => code.java.members.nio.byte_buffer.put_long,
            BaseType::Double => code.java.members.nio.byte_buffer.put_double,
            t => panic!("Cannot store {:?}", t),
        };
        code.invoke(put_func)?;
        code.push_instruction(Instruction::Pop)?;

        Ok(())
    }
}
