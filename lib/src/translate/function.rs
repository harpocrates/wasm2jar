use super::{
    BootstrapUtilities, Data, Element, Error, Function, Global, Memory, Settings, Table,
    UtilityClass, UtilityMethod,
};
use crate::jvm::class_graph::ClassId;
use crate::jvm::code::{
    BranchCond, BranchInstruction, CodeBuilder, CodeBuilderExts, EqComparison, Instruction,
    OrdComparison, SynLabel,
};
use crate::jvm::{BaseType, FieldType, MethodDescriptor, RefType, UnqualifiedName};
use crate::runtime::WasmRuntime;
use crate::util::{OffsetVec, Width};
use crate::wasm::{
    ref_type_from_general, ControlFrame, FunctionType, StackType, WasmModuleResourcesExt,
};
use std::collections::HashMap;
use std::convert::TryFrom;
use std::iter::FromIterator;
use std::ops::Not;
use wasmparser::{
    BlockType, BrTable, FuncValidator, FunctionBody, MemArg, Operator, ValType, ValidatorResources,
};

/// Context for translating a WASM function into a JVM one
pub struct FunctionTranslator<'a, 'b, 'g> {
    /// WASM type of the function being translated
    function_typ: &'b FunctionType,

    /// Translation settings
    settings: &'b Settings,

    /// Utilities
    utilities: &'b mut UtilityClass<'g>,

    /// Bootstrap utilities (unlike `utilities`, these get cleared across parts)
    bootstrap_utilities: &'b mut BootstrapUtilities<'g>,

    /// Code builder
    jvm_code: &'b mut CodeBuilder<'g>,

    runtime: &'b WasmRuntime<'g>,

    /// Main module class
    class: ClassId<'g>,

    /// Functions
    wasm_functions: &'b [Function<'a, 'g>],

    /// Tables
    wasm_tables: &'b [Table<'a, 'g>],

    /// Memories
    wasm_memories: &'b [Memory<'a, 'g>],

    /// Globals
    wasm_globals: &'b [Global<'a, 'g>],

    /// Datas
    wasm_datas: &'b [Data<'a, 'g>],

    /// Elements
    wasm_elements: &'b [Element<'a, 'g>],

    /// Local variables
    jvm_locals: LocalsLayout<'g>,

    /// Validator for the WASM function
    pub wasm_validator: &'b mut FuncValidator<ValidatorResources>,

    /// Previous height of the WASM stack
    wasm_prev_operand_stack_height: u32,

    /// WASM function being translated
    wasm_function: FunctionBody<'a>,

    /// Stack of WASM structured control flow frames
    wasm_frames: Vec<ControlFrame<SynLabel>>,

    /// Count of WASM control frames which are unreachable
    wasm_unreachable_frame_count: usize,
}

impl<'a, 'b, 'g> FunctionTranslator<'a, 'b, 'g> {
    pub fn new(
        function_typ: &'b FunctionType,
        settings: &'b Settings,
        utilities: &'b mut UtilityClass<'g>,
        bootstrap_utilities: &'b mut BootstrapUtilities<'g>,
        jvm_code: &'b mut CodeBuilder<'g>,
        class: ClassId<'g>,
        runtime: &'b WasmRuntime<'g>,
        wasm_functions: &'b [Function<'a, 'g>],
        wasm_tables: &'b [Table<'a, 'g>],
        wasm_memories: &'b [Memory<'a, 'g>],
        wasm_globals: &'b [Global<'a, 'g>],
        wasm_datas: &'b [Data<'a, 'g>],
        wasm_elements: &'b [Element<'a, 'g>],
        wasm_function: FunctionBody<'a>,
        wasm_validator: &'b mut FuncValidator<ValidatorResources>,
    ) -> Result<Self, Error> {
        let jvm_locals = LocalsLayout::new(
            function_typ
                .inputs
                .iter()
                .map(|wasm_ty| wasm_ty.field_type(&jvm_code.java.classes)),
            RefType::Object(class),
        );

        Ok(FunctionTranslator {
            function_typ,
            settings,
            utilities,
            bootstrap_utilities,
            jvm_code,
            jvm_locals,
            class,
            runtime,
            wasm_functions,
            wasm_tables,
            wasm_memories,
            wasm_globals,
            wasm_datas,
            wasm_elements,
            wasm_validator,
            wasm_prev_operand_stack_height: 0,
            wasm_function,
            wasm_frames: vec![],
            wasm_unreachable_frame_count: 0,
        })
    }

    /// Translate a function
    pub fn translate(&mut self) -> Result<(), Error> {
        self.visit_locals()?;
        self.visit_operators()?;
        Ok(())
    }

    /// Visit all locals
    ///
    /// This also handles zero-initializing the locals (as is required by WASM)
    fn visit_locals(&mut self) -> Result<(), Error> {
        let mut reader = self.wasm_function.get_binary_reader();
        self.wasm_validator.read_locals(&mut reader)?;

        let first_local_idx = self.function_typ.inputs.len() as u32;
        for local_idx in first_local_idx..self.wasm_validator.len_locals() {
            let local_type = self.wasm_validator.get_local_type(local_idx).unwrap();

            // WASM locals are zero initialized
            let local_type = StackType::from_general(local_type)?;
            let field_type = local_type.field_type(&self.jvm_code.java.classes);
            let idx = self.jvm_locals.push_local(field_type)?;
            self.jvm_code.zero_local(idx, field_type)?;
        }

        Ok(())
    }

    /// Visit all operators
    fn visit_operators(&mut self) -> Result<(), Error> {
        let op_reader = self.wasm_function.get_operators_reader()?;
        let mut op_iter = op_reader.into_iter_with_offsets();
        let mut last_offset = 0;

        /* When we call `visit_operator`, we need to pass in an operator which we know will get
         * consumed and an option of an operator that may be consumed. We keep a mutable option
         * for the "next" operator, on which `visit_operator` calls `take` if it needs it.
         */
        let mut next_operator: Option<(Operator, usize)> = None;
        loop {
            let this_operator = if let Some(operator) = next_operator.take() {
                operator
            } else if let Some(op_offset) = op_iter.next() {
                let operator_offset = op_offset?;
                last_offset = operator_offset.1;
                operator_offset
            } else {
                break;
            };
            next_operator = match op_iter.next() {
                None => None,
                Some(Ok(next_op)) => {
                    last_offset = next_op.1;
                    Some(next_op)
                }
                Some(Err(err)) => return Err(Error::WasmParser(err)),
            };

            self.visit_operator(this_operator, &mut next_operator)?;
        }

        // If control flow falls through to the end, insert an implicit return
        if self.jvm_code.current_frame().is_some() {
            self.visit_return()?;
        }

        self.wasm_validator.finish(last_offset + 1)?;
        Ok(())
    }

    /// Visit and interpret an operator
    ///
    /// The second operator argument is a lookahead which, if it is going to be consumed, should
    /// be taken (so the caller knows it has been consumed). Having a lookahead argument enables
    /// one important class of optimizations: combining condition operators immediately followed
    /// by operators that act on the condition.
    fn visit_operator(
        &mut self,
        operator_offset: (Operator, usize),
        next_operator_offset: &mut Option<(Operator, usize)>,
    ) -> Result<(), Error> {
        use crate::jvm::code::CompareMode::*;
        use crate::jvm::code::Instruction::*;
        use crate::jvm::code::ShiftType::*;

        let (operator, offset) = operator_offset;
        let next_op = next_operator_offset;
        self.wasm_validator.op(offset, &operator)?;

        // Detect if the current frame is unreachable and handle things differently
        if self.jvm_code.current_frame().is_none() {
            match operator {
                // Increment the unreachable frame count and skip the operator
                Operator::Block { .. } | Operator::Loop { .. } | Operator::If { .. } => {
                    self.wasm_unreachable_frame_count += 1;
                    return Ok(());
                }

                // Process the operator as normal (don't do an early return)
                Operator::End | Operator::Else if 0 == self.wasm_unreachable_frame_count => (),

                // Decrement the unreachable frame count and skip the operator
                Operator::End => {
                    self.wasm_unreachable_frame_count -= 1;
                    return Ok(());
                }

                // Skip the operator
                _ => return Ok(()),
            }
        }

        match operator {
            // Control Instructions
            Operator::Unreachable => {
                self.utilities
                    .invoke_utility(UtilityMethod::Unreachable, self.jvm_code)?;
                self.jvm_code
                    .push_branch_instruction(BranchInstruction::AThrow)?;
            }
            Operator::Nop => self.jvm_code.push_instruction(Instruction::Nop)?,
            Operator::Block { blockty } => self.visit_block(blockty)?,
            Operator::Loop { blockty } => self.visit_loop(blockty)?,
            Operator::If { blockty } => {
                self.visit_if(blockty, BranchCond::If(OrdComparison::NE))?
            }
            Operator::Else => self.visit_else()?,
            Operator::End => self.visit_end()?,
            Operator::Br { relative_depth } => self.visit_branch(relative_depth)?,
            Operator::BrIf { relative_depth } => {
                self.visit_branch_if(relative_depth, BranchCond::If(OrdComparison::NE))?
            }
            Operator::BrTable { targets } => self.visit_branch_table(targets)?,
            Operator::Return => self.visit_return()?,
            Operator::Call { function_index } => self.visit_call(function_index)?,
            Operator::CallIndirect {
                type_index,
                table_index,
                table_byte: _,
            } => self.visit_call_indirect(BlockType::FuncType(type_index), table_index)?,

            // Parametric Instructions
            Operator::Drop => self.jvm_code.pop()?,
            Operator::Select => self.visit_select(None, BranchCond::If(OrdComparison::NE))?,
            Operator::TypedSelect { ty } => {
                self.visit_select(Some(ty), BranchCond::If(OrdComparison::NE))?
            }

            // Variable Instructions
            Operator::LocalGet { local_index } => {
                let (off, field_type) = self.jvm_locals.lookup_local(local_index)?;
                self.jvm_code.get_local(off, &field_type)?;
            }
            Operator::LocalSet { local_index } => {
                let (off, field_type) = self.jvm_locals.lookup_local(local_index)?;
                self.jvm_code.set_local(off, &field_type)?;
            }
            Operator::LocalTee { local_index } => {
                let (off, field_type) = self.jvm_locals.lookup_local(local_index)?;
                self.jvm_code.dup()?;
                self.jvm_code.set_local(off, &field_type)?;
            }
            Operator::GlobalGet { global_index } => self.visit_global_get(global_index)?,
            Operator::GlobalSet { global_index } => self.visit_global_set(global_index)?,

            // Table instructions
            Operator::TableGet { table } => self.visit_table_get(table)?,
            Operator::TableSet { table } => self.visit_table_set(table)?,
            Operator::TableInit { elem_index, table } => {
                self.visit_table_init(elem_index, table)?
            }
            Operator::TableCopy {
                src_table,
                dst_table,
            } => self.visit_table_copy(src_table, dst_table)?,
            Operator::TableGrow { table } => self.visit_table_grow(table)?,
            Operator::TableSize { table } => self.visit_table_size(table)?,
            Operator::TableFill { table } => self.visit_table_fill(table)?,

            // Memory Instructions
            Operator::I32Load { memarg } => self.visit_memory_load(memarg, BaseType::Int)?,
            Operator::I64Load { memarg } => self.visit_memory_load(memarg, BaseType::Long)?,
            Operator::F32Load { memarg } => self.visit_memory_load(memarg, BaseType::Float)?,
            Operator::F64Load { memarg } => self.visit_memory_load(memarg, BaseType::Double)?,
            Operator::I32Load8S { memarg } => self.visit_memory_load(memarg, BaseType::Byte)?,
            Operator::I32Load8U { memarg } => {
                self.visit_memory_load(memarg, BaseType::Byte)?;
                self.jvm_code.const_int(0xFF)?;
                self.jvm_code.push_instruction(Instruction::IAnd)?;
            }
            Operator::I32Load16S { memarg } => {
                self.visit_memory_load(memarg, BaseType::Short)?;
            }
            Operator::I32Load16U { memarg } => {
                self.visit_memory_load(memarg, BaseType::Short)?;
                self.jvm_code.const_int(0xFFFF)?;
                self.jvm_code.push_instruction(Instruction::IAnd)?;
            }
            Operator::I64Load8S { memarg } => {
                self.visit_memory_load(memarg, BaseType::Byte)?;
                self.jvm_code.push_instruction(Instruction::I2L)?;
            }
            Operator::I64Load8U { memarg } => {
                self.visit_memory_load(memarg, BaseType::Byte)?;
                self.jvm_code.const_int(0xFF)?;
                self.jvm_code.push_instruction(Instruction::IAnd)?;
                self.jvm_code.push_instruction(Instruction::I2L)?;
            }
            Operator::I64Load16S { memarg } => {
                self.visit_memory_load(memarg, BaseType::Short)?;
                self.jvm_code.push_instruction(Instruction::I2L)?;
            }
            Operator::I64Load16U { memarg } => {
                self.visit_memory_load(memarg, BaseType::Short)?;
                self.jvm_code.const_int(0xFFFF)?;
                self.jvm_code.push_instruction(Instruction::IAnd)?;
                self.jvm_code.push_instruction(Instruction::I2L)?;
            }
            Operator::I64Load32S { memarg } => {
                self.visit_memory_load(memarg, BaseType::Int)?;
                self.jvm_code.push_instruction(Instruction::I2L)?;
            }
            Operator::I64Load32U { memarg } => {
                self.visit_memory_load(memarg, BaseType::Int)?;
                self.jvm_code.push_instruction(Instruction::I2L)?;
                self.jvm_code.const_long(0xFFFFFFFF)?;
                self.jvm_code.push_instruction(Instruction::LAnd)?;
            }
            Operator::I32Store { memarg } => self.visit_memory_store(memarg, BaseType::Int)?,
            Operator::I64Store { memarg } => self.visit_memory_store(memarg, BaseType::Long)?,
            Operator::F32Store { memarg } => self.visit_memory_store(memarg, BaseType::Float)?,
            Operator::F64Store { memarg } => self.visit_memory_store(memarg, BaseType::Double)?,
            Operator::I32Store8 { memarg } => self.visit_memory_store(memarg, BaseType::Byte)?,
            Operator::I32Store16 { memarg } => self.visit_memory_store(memarg, BaseType::Short)?,
            Operator::I64Store8 { memarg } => {
                self.jvm_code.const_long(0xFF)?;
                self.jvm_code.push_instruction(Instruction::LAnd)?;
                self.jvm_code.push_instruction(Instruction::L2I)?;
                self.visit_memory_store(memarg, BaseType::Byte)?;
            }
            Operator::I64Store16 { memarg } => {
                self.jvm_code.const_long(0xFFFF)?;
                self.jvm_code.push_instruction(Instruction::LAnd)?;
                self.jvm_code.push_instruction(Instruction::L2I)?;
                self.visit_memory_store(memarg, BaseType::Short)?;
            }
            Operator::I64Store32 { memarg } => {
                self.jvm_code.const_long(0xFFFFFFFF)?;
                self.jvm_code.push_instruction(Instruction::LAnd)?;
                self.jvm_code.push_instruction(Instruction::L2I)?;
                self.visit_memory_store(memarg, BaseType::Int)?;
            }
            Operator::MemorySize { mem, .. } => self.visit_memory_size(mem)?, // TODO: what is `mem_byte` for?
            Operator::MemoryGrow { mem, .. } => self.visit_memory_grow(mem)?,
            Operator::MemoryInit { data_index, mem } => self.visit_memory_init(mem, data_index)?,
            Operator::MemoryCopy { dst_mem, src_mem } => {
                self.visit_memory_copy(src_mem, dst_mem)?
            }
            Operator::MemoryFill { mem } => self.visit_memory_fill(mem)?,
            Operator::DataDrop { data_index } => self.visit_data_drop(data_index)?,
            Operator::ElemDrop { elem_index } => self.visit_element_drop(elem_index)?,

            // Numeric Instructions
            Operator::I32Const { value } => self.jvm_code.const_int(value)?,
            Operator::I64Const { value } => self.jvm_code.const_long(value)?,
            Operator::F32Const { value } => {
                self.jvm_code.const_float(f32::from_bits(value.bits()))?
            }
            Operator::F64Const { value } => {
                self.jvm_code.const_double(f64::from_bits(value.bits()))?
            }

            Operator::I32Eqz => self.visit_cond(BranchCond::If(OrdComparison::EQ), next_op)?,
            Operator::I32Eq => self.visit_cond(BranchCond::IfICmp(OrdComparison::EQ), next_op)?,
            Operator::I32Ne => self.visit_cond(BranchCond::IfICmp(OrdComparison::NE), next_op)?,
            Operator::I32LtS => self.visit_cond(BranchCond::IfICmp(OrdComparison::LT), next_op)?,
            Operator::I32LtU => {
                self.jvm_code
                    .invoke(self.jvm_code.java.members.lang.integer.compare_unsigned)?;
                self.visit_cond(BranchCond::If(OrdComparison::LT), next_op)?;
            }
            Operator::I32GtS => self.visit_cond(BranchCond::IfICmp(OrdComparison::GT), next_op)?,
            Operator::I32GtU => {
                self.jvm_code
                    .invoke(self.jvm_code.java.members.lang.integer.compare_unsigned)?;
                self.visit_cond(BranchCond::If(OrdComparison::GT), next_op)?;
            }
            Operator::I32LeS => self.visit_cond(BranchCond::IfICmp(OrdComparison::LE), next_op)?,
            Operator::I32LeU => {
                self.jvm_code
                    .invoke(self.jvm_code.java.members.lang.integer.compare_unsigned)?;
                self.visit_cond(BranchCond::If(OrdComparison::LE), next_op)?;
            }
            Operator::I32GeS => self.visit_cond(BranchCond::IfICmp(OrdComparison::GE), next_op)?,
            Operator::I32GeU => {
                self.jvm_code
                    .invoke(self.jvm_code.java.members.lang.integer.compare_unsigned)?;
                self.visit_cond(BranchCond::If(OrdComparison::GE), next_op)?;
            }

            Operator::F32Eq => {
                self.jvm_code.push_instruction(FCmp(G))?; // either G or L works here
                self.visit_cond(BranchCond::If(OrdComparison::EQ), next_op)?;
            }
            Operator::F32Ne => {
                self.jvm_code.push_instruction(FCmp(G))?; // either G or L works here
                self.visit_cond(BranchCond::If(OrdComparison::NE), next_op)?;
            }
            Operator::F32Lt => {
                self.jvm_code.push_instruction(FCmp(G))?;
                self.visit_cond(BranchCond::If(OrdComparison::LT), next_op)?;
            }
            Operator::F32Gt => {
                self.jvm_code.push_instruction(FCmp(L))?;
                self.visit_cond(BranchCond::If(OrdComparison::GT), next_op)?;
            }
            Operator::F32Le => {
                self.jvm_code.push_instruction(FCmp(G))?;
                self.visit_cond(BranchCond::If(OrdComparison::LE), next_op)?;
            }
            Operator::F32Ge => {
                self.jvm_code.push_instruction(FCmp(L))?;
                self.visit_cond(BranchCond::If(OrdComparison::GE), next_op)?;
            }

            Operator::F64Eq => {
                self.jvm_code.push_instruction(DCmp(G))?; // either G or L works here
                self.visit_cond(BranchCond::If(OrdComparison::EQ), next_op)?;
            }
            Operator::F64Ne => {
                self.jvm_code.push_instruction(DCmp(G))?; // either G or L works here
                self.visit_cond(BranchCond::If(OrdComparison::NE), next_op)?;
            }
            Operator::F64Lt => {
                self.jvm_code.push_instruction(DCmp(G))?;
                self.visit_cond(BranchCond::If(OrdComparison::LT), next_op)?;
            }
            Operator::F64Gt => {
                self.jvm_code.push_instruction(DCmp(L))?;
                self.visit_cond(BranchCond::If(OrdComparison::GT), next_op)?;
            }
            Operator::F64Le => {
                self.jvm_code.push_instruction(DCmp(G))?;
                self.visit_cond(BranchCond::If(OrdComparison::LE), next_op)?;
            }
            Operator::F64Ge => {
                self.jvm_code.push_instruction(DCmp(L))?;
                self.visit_cond(BranchCond::If(OrdComparison::GE), next_op)?;
            }

            Operator::I64Eqz => {
                self.jvm_code.push_instruction(LConst0)?;
                self.jvm_code.push_instruction(LCmp)?;
                self.visit_cond(BranchCond::If(OrdComparison::EQ), next_op)?;
            }
            Operator::I64Eq => {
                self.jvm_code.push_instruction(LCmp)?;
                self.visit_cond(BranchCond::If(OrdComparison::EQ), next_op)?;
            }
            Operator::I64Ne => {
                self.jvm_code.push_instruction(LCmp)?;
                self.visit_cond(BranchCond::If(OrdComparison::NE), next_op)?;
            }
            Operator::I64LtS => {
                self.jvm_code.push_instruction(LCmp)?;
                self.visit_cond(BranchCond::If(OrdComparison::LT), next_op)?;
            }
            Operator::I64LtU => {
                self.jvm_code
                    .invoke(self.jvm_code.java.members.lang.long.compare_unsigned)?;
                self.visit_cond(BranchCond::If(OrdComparison::LT), next_op)?;
            }
            Operator::I64GtS => {
                self.jvm_code.push_instruction(LCmp)?;
                self.visit_cond(BranchCond::If(OrdComparison::GT), next_op)?;
            }
            Operator::I64GtU => {
                self.jvm_code
                    .invoke(self.jvm_code.java.members.lang.long.compare_unsigned)?;
                self.visit_cond(BranchCond::If(OrdComparison::GT), next_op)?;
            }
            Operator::I64LeS => {
                self.jvm_code.push_instruction(LCmp)?;
                self.visit_cond(BranchCond::If(OrdComparison::LE), next_op)?;
            }
            Operator::I64LeU => {
                self.jvm_code
                    .invoke(self.jvm_code.java.members.lang.long.compare_unsigned)?;
                self.visit_cond(BranchCond::If(OrdComparison::LE), next_op)?;
            }
            Operator::I64GeS => {
                self.jvm_code.push_instruction(LCmp)?;
                self.visit_cond(BranchCond::If(OrdComparison::GE), next_op)?;
            }
            Operator::I64GeU => {
                self.jvm_code
                    .invoke(self.jvm_code.java.members.lang.long.compare_unsigned)?;
                self.visit_cond(BranchCond::If(OrdComparison::GE), next_op)?;
            }

            Operator::I32Clz => self.jvm_code.invoke(
                self.jvm_code
                    .java
                    .members
                    .lang
                    .integer
                    .number_of_leading_zeros,
            )?,
            Operator::I32Ctz => self.jvm_code.invoke(
                self.jvm_code
                    .java
                    .members
                    .lang
                    .integer
                    .number_of_trailing_zeros,
            )?,
            Operator::I32Popcnt => self
                .jvm_code
                .invoke(self.jvm_code.java.members.lang.integer.bit_count)?,
            Operator::I32Add => self.jvm_code.push_instruction(IAdd)?,
            Operator::I32Sub => self.jvm_code.push_instruction(ISub)?,
            Operator::I32Mul => self.jvm_code.push_instruction(IMul)?,
            Operator::I32DivS => {
                if self.settings.trap_integer_division_overflow {
                    self.utilities
                        .invoke_utility(UtilityMethod::I32DivS, self.jvm_code)?;
                } else {
                    self.jvm_code.push_instruction(IDiv)?;
                }
            }
            Operator::I32DivU => self
                .jvm_code
                .invoke(self.jvm_code.java.members.lang.integer.divide_unsigned)?,
            Operator::I32RemS => self.jvm_code.push_instruction(IRem)?,
            Operator::I32RemU => self
                .jvm_code
                .invoke(self.jvm_code.java.members.lang.integer.remainder_unsigned)?,
            Operator::I32And => self.jvm_code.push_instruction(IAnd)?,
            Operator::I32Or => self.jvm_code.push_instruction(IOr)?,
            Operator::I32Xor => self.jvm_code.push_instruction(IXor)?,
            Operator::I32Shl => self.jvm_code.push_instruction(ISh(Left))?,
            Operator::I32ShrS => self.jvm_code.push_instruction(ISh(ArithmeticRight))?,
            Operator::I32ShrU => self.jvm_code.push_instruction(ISh(LogicalRight))?,
            Operator::I32Rotl => self
                .jvm_code
                .invoke(self.jvm_code.java.members.lang.integer.rotate_left)?,
            Operator::I32Rotr => self
                .jvm_code
                .invoke(self.jvm_code.java.members.lang.integer.rotate_right)?,

            Operator::I64Clz => {
                self.jvm_code
                    .invoke(self.jvm_code.java.members.lang.long.number_of_leading_zeros)?;
                self.jvm_code.push_instruction(I2L)?;
            }
            Operator::I64Ctz => {
                self.jvm_code.invoke(
                    self.jvm_code
                        .java
                        .members
                        .lang
                        .long
                        .number_of_trailing_zeros,
                )?;
                self.jvm_code.push_instruction(I2L)?;
            }
            Operator::I64Popcnt => {
                self.jvm_code
                    .invoke(self.jvm_code.java.members.lang.long.bit_count)?;
                self.jvm_code.push_instruction(I2L)?;
            }
            Operator::I64Add => self.jvm_code.push_instruction(LAdd)?,
            Operator::I64Sub => self.jvm_code.push_instruction(LSub)?,
            Operator::I64Mul => self.jvm_code.push_instruction(LMul)?,
            Operator::I64DivS => {
                if self.settings.trap_integer_division_overflow {
                    self.utilities
                        .invoke_utility(UtilityMethod::I64DivS, self.jvm_code)?;
                } else {
                    self.jvm_code.push_instruction(LDiv)?;
                }
            }
            Operator::I64RemS => self.jvm_code.push_instruction(LRem)?,
            Operator::I64DivU => self
                .jvm_code
                .invoke(self.jvm_code.java.members.lang.long.divide_unsigned)?,
            Operator::I64RemU => self
                .jvm_code
                .invoke(self.jvm_code.java.members.lang.long.remainder_unsigned)?,
            Operator::I64And => self.jvm_code.push_instruction(LAnd)?,
            Operator::I64Or => self.jvm_code.push_instruction(LOr)?,
            Operator::I64Xor => self.jvm_code.push_instruction(LXor)?,
            Operator::I64Shl => {
                self.jvm_code.push_instruction(L2I)?;
                self.jvm_code.push_instruction(LSh(Left))?;
            }
            Operator::I64ShrS => {
                self.jvm_code.push_instruction(L2I)?;
                self.jvm_code.push_instruction(LSh(ArithmeticRight))?;
            }
            Operator::I64ShrU => {
                self.jvm_code.push_instruction(L2I)?;
                self.jvm_code.push_instruction(LSh(LogicalRight))?;
            }
            Operator::I64Rotl => {
                self.jvm_code.push_instruction(L2I)?;
                self.jvm_code
                    .invoke(self.jvm_code.java.members.lang.long.rotate_left)?;
            }
            Operator::I64Rotr => {
                self.jvm_code.push_instruction(L2I)?;
                self.jvm_code
                    .invoke(self.jvm_code.java.members.lang.long.rotate_right)?;
            }

            Operator::F32Abs => {
                if self.settings.bitwise_floating_abs {
                    self.utilities
                        .invoke_utility(UtilityMethod::F32Abs, self.jvm_code)?;
                } else {
                    self.jvm_code
                        .invoke(self.jvm_code.java.members.lang.math.abs_float)?;
                }
            }
            Operator::F32Neg => self.jvm_code.push_instruction(FNeg)?,
            Operator::F32Ceil => {
                self.jvm_code.push_instruction(F2D)?;
                self.jvm_code
                    .invoke(self.jvm_code.java.members.lang.math.ceil)?;
                self.jvm_code.push_instruction(D2F)?;
            }
            Operator::F32Floor => {
                self.jvm_code.push_instruction(F2D)?;
                self.jvm_code
                    .invoke(self.jvm_code.java.members.lang.math.floor)?;
                self.jvm_code.push_instruction(D2F)?;
            }
            Operator::F32Trunc => {
                self.utilities
                    .invoke_utility(UtilityMethod::F32Trunc, self.jvm_code)?;
            }
            Operator::F32Nearest => {
                self.jvm_code.push_instruction(F2D)?;
                self.jvm_code
                    .invoke(self.jvm_code.java.members.lang.math.rint)?;
                self.jvm_code.push_instruction(D2F)?;
            }
            Operator::F32Sqrt => {
                self.jvm_code.push_instruction(F2D)?;
                self.jvm_code
                    .invoke(self.jvm_code.java.members.lang.math.sqrt)?;
                self.jvm_code.push_instruction(D2F)?;
            }
            Operator::F32Add => self.jvm_code.push_instruction(FAdd)?,
            Operator::F32Sub => self.jvm_code.push_instruction(FSub)?,
            Operator::F32Mul => self.jvm_code.push_instruction(FMul)?,
            Operator::F32Div => self.jvm_code.push_instruction(FDiv)?,
            Operator::F32Min => self
                .jvm_code
                .invoke(self.jvm_code.java.members.lang.float.min)?,
            Operator::F32Max => self
                .jvm_code
                .invoke(self.jvm_code.java.members.lang.float.max)?,
            Operator::F32Copysign => self
                .jvm_code
                .invoke(self.jvm_code.java.members.lang.math.copy_sign_float)?,
            Operator::F64Abs => {
                if self.settings.bitwise_floating_abs {
                    self.utilities
                        .invoke_utility(UtilityMethod::F64Abs, self.jvm_code)?;
                } else {
                    self.jvm_code
                        .invoke(self.jvm_code.java.members.lang.math.abs_double)?;
                }
            }
            Operator::F64Neg => self.jvm_code.push_instruction(DNeg)?,
            Operator::F64Ceil => self
                .jvm_code
                .invoke(self.jvm_code.java.members.lang.math.ceil)?,
            Operator::F64Floor => self
                .jvm_code
                .invoke(self.jvm_code.java.members.lang.math.floor)?,
            Operator::F64Trunc => {
                self.utilities
                    .invoke_utility(UtilityMethod::F64Trunc, self.jvm_code)?;
            }
            Operator::F64Nearest => self
                .jvm_code
                .invoke(self.jvm_code.java.members.lang.math.rint)?,
            Operator::F64Sqrt => self
                .jvm_code
                .invoke(self.jvm_code.java.members.lang.math.sqrt)?,
            Operator::F64Add => self.jvm_code.push_instruction(DAdd)?,
            Operator::F64Sub => self.jvm_code.push_instruction(DSub)?,
            Operator::F64Mul => self.jvm_code.push_instruction(DMul)?,
            Operator::F64Div => self.jvm_code.push_instruction(DDiv)?,
            Operator::F64Min => self
                .jvm_code
                .invoke(self.jvm_code.java.members.lang.double.min)?,
            Operator::F64Max => self
                .jvm_code
                .invoke(self.jvm_code.java.members.lang.double.max)?,
            Operator::F64Copysign => {
                self.jvm_code
                    .invoke(self.jvm_code.java.members.lang.math.copy_sign_double)?;
            }

            Operator::I32WrapI64 => self.jvm_code.push_instruction(L2I)?,
            Operator::I32TruncF32S => {
                self.utilities
                    .invoke_utility(UtilityMethod::I32TruncF32S, self.jvm_code)?;
            }
            Operator::I32TruncF32U => {
                self.utilities
                    .invoke_utility(UtilityMethod::I32TruncF32U, self.jvm_code)?;
            }
            Operator::I32TruncF64S => {
                self.utilities
                    .invoke_utility(UtilityMethod::I32TruncF64S, self.jvm_code)?;
            }
            Operator::I32TruncF64U => {
                self.utilities
                    .invoke_utility(UtilityMethod::I32TruncF64U, self.jvm_code)?;
            }
            Operator::I64ExtendI32S => self.jvm_code.push_instruction(I2L)?,
            Operator::I64ExtendI32U => {
                self.utilities
                    .invoke_utility(UtilityMethod::I64ExtendI32U, self.jvm_code)?;
            }
            Operator::I64TruncF32S => {
                self.utilities
                    .invoke_utility(UtilityMethod::I64TruncF32S, self.jvm_code)?;
            }
            Operator::I64TruncF32U => {
                self.utilities
                    .invoke_utility(UtilityMethod::I64TruncF32U, self.jvm_code)?;
            }
            Operator::I64TruncF64S => {
                self.utilities
                    .invoke_utility(UtilityMethod::I64TruncF64S, self.jvm_code)?;
            }
            Operator::I64TruncF64U => {
                self.utilities
                    .invoke_utility(UtilityMethod::I64TruncF64U, self.jvm_code)?;
            }
            Operator::F32ConvertI32S => self.jvm_code.push_instruction(I2F)?,
            Operator::F32ConvertI32U => {
                self.utilities
                    .invoke_utility(UtilityMethod::F32ConvertI32U, self.jvm_code)?;
            }
            Operator::F32ConvertI64S => self.jvm_code.push_instruction(L2F)?,
            Operator::F32ConvertI64U => {
                self.utilities
                    .invoke_utility(UtilityMethod::F32ConvertI64U, self.jvm_code)?;
            }
            Operator::F32DemoteF64 => self.jvm_code.push_instruction(D2F)?,
            Operator::F64ConvertI32S => self.jvm_code.push_instruction(I2D)?,
            Operator::F64ConvertI32U => {
                self.utilities
                    .invoke_utility(UtilityMethod::F64ConvertI32U, self.jvm_code)?;
            }
            Operator::F64ConvertI64S => self.jvm_code.push_instruction(L2D)?,
            Operator::F64ConvertI64U => {
                self.utilities
                    .invoke_utility(UtilityMethod::F64ConvertI64U, self.jvm_code)?;
            }
            Operator::F64PromoteF32 => self.jvm_code.push_instruction(F2D)?,

            Operator::I32ReinterpretF32 => self
                .jvm_code
                .invoke(self.jvm_code.java.members.lang.float.float_to_raw_int_bits)?,
            Operator::I64ReinterpretF64 => self.jvm_code.invoke(
                self.jvm_code
                    .java
                    .members
                    .lang
                    .double
                    .double_to_raw_long_bits,
            )?,
            Operator::F32ReinterpretI32 => self
                .jvm_code
                .invoke(self.jvm_code.java.members.lang.float.int_bits_to_float)?,
            Operator::F64ReinterpretI64 => self
                .jvm_code
                .invoke(self.jvm_code.java.members.lang.double.long_bits_to_double)?,

            Operator::I32Extend8S => self.jvm_code.push_instruction(I2B)?,
            Operator::I32Extend16S => self.jvm_code.push_instruction(I2S)?,
            Operator::I64Extend8S => {
                self.jvm_code.push_instruction(L2I)?;
                self.jvm_code.push_instruction(I2B)?;
                self.jvm_code.push_instruction(I2L)?;
            }
            Operator::I64Extend16S => {
                self.jvm_code.push_instruction(L2I)?;
                self.jvm_code.push_instruction(I2S)?;
                self.jvm_code.push_instruction(I2L)?;
            }
            Operator::I64Extend32S => {
                self.jvm_code.push_instruction(L2I)?;
                self.jvm_code.push_instruction(I2L)?;
            }

            Operator::I32TruncSatF32S => self.jvm_code.push_instruction(F2I)?,
            Operator::I32TruncSatF32U => self
                .utilities
                .invoke_utility(UtilityMethod::I32TruncSatF32U, self.jvm_code)?,
            Operator::I32TruncSatF64S => self.jvm_code.push_instruction(D2I)?,
            Operator::I32TruncSatF64U => self
                .utilities
                .invoke_utility(UtilityMethod::I32TruncSatF64U, self.jvm_code)?,
            Operator::I64TruncSatF32S => self.jvm_code.push_instruction(F2L)?,
            Operator::I64TruncSatF32U => self
                .utilities
                .invoke_utility(UtilityMethod::I64TruncSatF32U, self.jvm_code)?,
            Operator::I64TruncSatF64S => self.jvm_code.push_instruction(D2L)?,
            Operator::I64TruncSatF64U => self
                .utilities
                .invoke_utility(UtilityMethod::I64TruncSatF64U, self.jvm_code)?,

            // Reference Instructions
            Operator::RefNull { ty } => {
                let ref_type = ref_type_from_general(ty, &self.jvm_code.java.classes)?;
                self.jvm_code.const_null(ref_type)?;
            }
            Operator::RefIsNull => {
                self.visit_cond(BranchCond::IfNull(EqComparison::EQ), next_op)?
            }
            Operator::RefFunc { function_index } => {
                let function = &self.wasm_functions[function_index as usize];
                let this_off = self.jvm_locals.lookup_this()?.0;

                self.jvm_code.const_methodhandle(function.method)?;
                self.jvm_code
                    .const_int(function.func_type.inputs.len() as i32)?;
                self.jvm_code.const_int(1)?;
                self.jvm_code
                    .new_ref_array(RefType::Object(self.jvm_code.java.classes.lang.object))?;
                self.jvm_code.dup()?;
                self.jvm_code.const_int(0)?;
                self.jvm_code
                    .push_instruction(Instruction::ALoad(this_off))?;
                self.jvm_code.push_instruction(Instruction::AAStore)?;
                self.jvm_code.invoke(
                    self.jvm_code
                        .java
                        .members
                        .lang
                        .invoke
                        .method_handles
                        .insert_arguments,
                )?;
            }

            _ => todo!(),
        }

        self.wasm_prev_operand_stack_height = self.wasm_validator.operand_stack_height();
        Ok(())
    }

    /// Visit a condition, and optionally also a followup operator if that operator can benefit
    /// from being merged with the condition. If the followup operator gets used, it will be taken
    /// out of the mutable option.
    fn visit_cond(
        &mut self,
        condition: BranchCond,
        next_operator_offset: &mut Option<(Operator, usize)>,
    ) -> Result<(), Error> {
        match next_operator_offset.take() {
            Some((Operator::If { blockty }, offset)) => {
                self.wasm_validator.op(offset, &Operator::If { blockty })?;
                self.visit_if(blockty, condition)?;
            }
            Some((Operator::BrIf { relative_depth }, offset)) => {
                self.wasm_prev_operand_stack_height = self.wasm_validator.operand_stack_height();
                self.wasm_validator
                    .op(offset, &Operator::BrIf { relative_depth })?;
                self.visit_branch_if(relative_depth, condition)?;
            }
            Some((Operator::TypedSelect { ty }, offset)) => {
                self.wasm_validator
                    .op(offset, &Operator::TypedSelect { ty })?;
                self.visit_select(Some(ty), condition)?
            }

            other => {
                self.jvm_code.condition(&condition)?;
                *next_operator_offset = other;
            }
        }

        Ok(())
    }

    /// Visit the start of an `if` block
    fn visit_if(&mut self, ty: BlockType, condition: BranchCond) -> Result<(), Error> {
        let ty = self.wasm_validator.resources().block_type(ty)?;
        let else_block = self.jvm_code.fresh_label();
        let end_block = self.jvm_code.fresh_label();

        self.jvm_code
            .push_branch_instruction(condition.not().into_instruction(else_block, ()))?;

        #[cfg(debug_assertions)]
        self.assert_top_stack(&ty.inputs);

        let base_stack_height = self.wasm_validator.operand_stack_height() - ty.inputs.len() as u32;
        self.wasm_frames.push(ControlFrame::If {
            else_block,
            end_block,
            return_values: ty.outputs,
            base_stack_height,
        });

        Ok(())
    }

    /// Visit an `else` block
    fn visit_else(&mut self) -> Result<(), Error> {
        let else_frame = match self.wasm_frames.pop() {
            Some(ControlFrame::If {
                else_block,
                end_block,
                return_values,
                base_stack_height,
            }) => {
                self.jvm_code
                    .push_branch_instruction(BranchInstruction::Goto(end_block))?;
                self.jvm_code.place_label(else_block)?;
                ControlFrame::Else {
                    end_block,
                    return_values,
                    base_stack_height,
                }
            }
            _ => panic!("expected `if` control frame before `else`"),
        };
        self.wasm_frames.push(else_frame);

        Ok(())
    }

    /// Visit a `block` block
    fn visit_block(&mut self, ty: BlockType) -> Result<(), Error> {
        let ty = self.wasm_validator.resources().block_type(ty)?;
        let end_block = self.jvm_code.fresh_label();

        #[cfg(debug_assertions)]
        self.assert_top_stack(&ty.inputs);

        let base_stack_height = self.wasm_validator.operand_stack_height() - ty.inputs.len() as u32;
        self.wasm_frames.push(ControlFrame::Block {
            end_block,
            return_values: ty.outputs,
            base_stack_height,
        });

        Ok(())
    }

    /// Visit a `loop` block
    fn visit_loop(&mut self, ty: BlockType) -> Result<(), Error> {
        let ty = self.wasm_validator.resources().block_type(ty)?;
        let start_loop = self.jvm_code.fresh_label();
        let after_block = self.jvm_code.fresh_label();

        #[cfg(debug_assertions)]
        self.assert_top_stack(&ty.inputs);

        let base_stack_height = self.wasm_validator.operand_stack_height() - ty.inputs.len() as u32;
        self.wasm_frames.push(ControlFrame::Loop {
            start_loop,
            after_block,
            input_values: ty.inputs,
            return_values: ty.outputs,
            base_stack_height,
        });
        self.jvm_code.place_label(start_loop)?;

        Ok(())
    }

    /// Visit the end of a block
    ///
    /// Note: unlike `br`/`br_if`, reaching the end of a block naturally means that the stack
    /// should be precisely in the state of:
    ///
    ///   * the top values correspond the the block's return values
    ///   * the height of the stack under those return values matches the height of the stack when
    ///     the block was entered (and also under the argument values)
    ///
    fn visit_end(&mut self) -> Result<(), Error> {
        let control_frame = if let Some(frame) = self.wasm_frames.pop() {
            frame
        } else {
            // all functions end with one final `End`
            // TODO: review this
            return Ok(());
        };

        // In the case of a single-arm `if`, we must place the else label
        if let ControlFrame::If { else_block, .. } = &control_frame {
            self.jvm_code.place_label(*else_block)?;
        }

        use crate::jvm::Error::PlacingLabelBeforeReference;

        /* At the end of all control flow blocks, we attempt to just fallthrough to the end label.
         * However, this can fail in one important case: if the label has never been referenced
         * before and there is no currently active block. As it happens, this is exactly the case
         * that represents an unreachable end in WASM (since there are no other future jumps that
         * can jump to the end of a prior block).
         *
         * In exactly that one case, we can recover and just continue: the label won't have been
         * placed anywhere, and there won't be an active current block.
         */
        match self.jvm_code.place_label(control_frame.end_label()) {
            Err(PlacingLabelBeforeReference(_)) => (),
            Err(err) => return Err(err.into()),
            Ok(()) => {
                #[cfg(debug_assertions)]
                self.assert_top_stack(control_frame.return_values());

                debug_assert_eq!(
                    control_frame.base_stack_height() + control_frame.return_values().len() as u32,
                    self.wasm_validator.operand_stack_height(),
                    "Stack does not have the expected height",
                );
            }
        }

        Ok(())
    }

    /// Inspect the current state of the operand stack and control frames to figure out what a
    /// branch to this relative depth entails. Return the:
    ///
    ///   - number of operand stack pops that will be needed
    ///   - the return values
    ///   - the label to jump to or `None` if the branch is really a return
    ///
    /// Most of the time, just calling `visit_branch` is enough. However, sometimes, we can
    /// optimize some branches differently (eg. if we are in a `br_table` and there is no stack to
    /// unwind, we'd prefer the `lookuptable` jump straight to the right label).
    fn prepare_for_branch(&self, relative_depth: u32) -> (u32, Vec<StackType>, Option<SynLabel>) {
        let relative_depth = relative_depth as usize;

        // Detect the case where the branch is really a return
        if self.wasm_frames.len() == relative_depth {
            return (0, self.function_typ.outputs.clone(), None);
        }

        let target_frame = self
            .wasm_frames
            .iter()
            .nth_back(relative_depth)
            .expect("No frame found for branch");
        let branch_values = target_frame.branch_values().to_vec();
        let target_label = target_frame.branch_label();

        // A `br` may involve unwinding the stack to the proper height
        let required_pops = self.wasm_prev_operand_stack_height
            - branch_values.len() as u32
            - target_frame.base_stack_height();

        (required_pops, branch_values, Some(target_label))
    }

    /// If `prepare_for_branch` has already been called, feed its outputs here (instead of using
    /// `visit_branch`).
    fn visit_prepared_branch(
        &mut self,
        required_pops: u32,
        branch_values: Vec<StackType>,
        target_label: SynLabel,
    ) -> Result<(), Error> {
        #[cfg(debug_assertions)]
        self.assert_top_stack(&branch_values);

        if required_pops > 0 {
            // Stash branch values (so we can unwind the stack under them)
            for branch_value in branch_values.iter().rev() {
                let field_type = branch_value.field_type(&self.jvm_code.java.classes);
                let local_idx = self.jvm_locals.push_local(field_type)?;
                self.jvm_code.set_local(local_idx, &field_type)?;
            }

            // Unwind the stack as many times as needed
            // TODO: optimize unwinding two width 1 types with `pop2`
            for _ in 0..required_pops {
                self.jvm_code.pop()?;
            }

            // Unstash branch values
            for _ in 0..branch_values.len() {
                let (local_idx, field_type) = self.jvm_locals.pop_local()?;
                self.jvm_code.get_local(local_idx, &field_type)?;
                self.jvm_code.kill_top_local(local_idx, Some(field_type))?;
            }
        }

        self.jvm_code
            .push_branch_instruction(BranchInstruction::Goto(target_label))?;

        Ok(())
    }

    /// Visit a `br` to an outer block
    fn visit_branch(&mut self, relative_depth: u32) -> Result<(), Error> {
        let (req_pops, branch_values, target_lbl_opt) = self.prepare_for_branch(relative_depth);
        match target_lbl_opt {
            Some(target_lbl) => self.visit_prepared_branch(req_pops, branch_values, target_lbl),
            None => self.visit_return(),
        }
    }

    /// Visit a `br_if` to an outer block
    fn visit_branch_if(&mut self, relative_depth: u32, condition: BranchCond) -> Result<(), Error> {
        let skip_branch = self.jvm_code.fresh_label();

        self.wasm_prev_operand_stack_height -= 1;
        self.jvm_code
            .push_branch_instruction(condition.not().into_instruction(skip_branch, ()))?;
        self.visit_branch(relative_depth)?;
        self.jvm_code.place_label(skip_branch)?;

        Ok(())
    }

    /// Visit a `br_table` to outer blocks
    fn visit_branch_table(&mut self, table: BrTable) -> Result<(), Error> {
        self.wasm_prev_operand_stack_height -= 1;

        // If there are no cases apart from the default, we cannot use `tableswitch`
        if table.is_empty() {
            self.jvm_code.push_instruction(Instruction::Pop)?;
            self.visit_branch(table.default())?;
            return Ok(());
        }

        // Labels to go to for each entry in the branch table. The last label is the default.
        let mut table_switch_targets = vec![];

        /* Labels + blocks that will have to go after the `tableswitch`. Whenever a `br_table` has
         * a target which first needs some stack unwinding, we must jump to an intermediate block
         * to unwind the stack, and then branch out.
         *
         * These blocks can sometimes have a lot of duplication (eg. when multiple cases are
         * breaking out to the same target). For this reason, we emit only one intermediate block
         * per relative branch target.
         */
        let mut pending_branch_blocks = HashMap::new();

        for target in table.targets().chain(std::iter::once(Ok(table.default()))) {
            let relative_depth = target?;
            let (req_pops, ret_values, target_lbl_opt) = self.prepare_for_branch(relative_depth);

            // If there is no stack to unwind, go straight to the final target label
            match target_lbl_opt {
                Some(target_lbl) if req_pops == 0 => table_switch_targets.push(target_lbl),
                _ => {
                    let entry = pending_branch_blocks
                        .entry(relative_depth)
                        .or_insert_with(|| {
                            let block_lbl = self.jvm_code.fresh_label();
                            (block_lbl, req_pops, ret_values, target_lbl_opt)
                        });
                    table_switch_targets.push(entry.0);
                }
            }
        }

        let default = table_switch_targets.pop().expect("no default target found");
        self.jvm_code
            .push_branch_instruction(BranchInstruction::TableSwitch {
                padding: 0,
                default,
                low: 0,
                targets: table_switch_targets,
            })?;

        // Now, place any extra blocks we may have accumulated
        for (_, (block_lbl, req_pops, ret_values, target_lbl_opt)) in pending_branch_blocks {
            self.jvm_code.place_label(block_lbl)?;
            match target_lbl_opt {
                Some(target_lbl) => self.visit_prepared_branch(req_pops, ret_values, target_lbl)?,
                None => self.visit_return()?,
            }
        }

        Ok(())
    }

    /// Visit a `select`
    fn visit_select(&mut self, ty: Option<ValType>, condition: BranchCond) -> Result<(), Error> {
        let ty = match ty {
            None => None,
            Some(ty) => Some(StackType::from_general(ty)?),
        };

        // The hint only matters for reference types
        let ref_ty_hint = ty.and_then(|st| match st.field_type(&self.jvm_code.java.classes) {
            FieldType::Ref(hint_ref) => Some(hint_ref),
            _ => None,
        });

        let else_block = self.jvm_code.fresh_label();
        let end_block = self.jvm_code.fresh_label();

        self.jvm_code
            .push_branch_instruction(condition.not().into_instruction(else_block, ()))?;

        // Are we selecting between two wide values? (if not, it is two regular values)
        let select_is_wide = self
            .jvm_code
            .current_frame()
            .expect("no current frame")
            .stack
            .iter()
            .last()
            .map_or(false, |(_, _, t)| t.width() == 2);

        // Keep the bottom value
        if select_is_wide {
            self.jvm_code.push_instruction(Instruction::Pop2)?;
        } else {
            self.jvm_code.push_instruction(Instruction::Pop)?;
        }
        if let Some(ref_ty) = ref_ty_hint {
            self.jvm_code.generalize_top_stack_type(ref_ty)?;
        }
        self.jvm_code
            .push_branch_instruction(BranchInstruction::Goto(end_block))?;

        // Keep the top value
        self.jvm_code.place_label(else_block)?;
        if select_is_wide {
            self.jvm_code.push_instruction(Instruction::Dup2X2)?;
            self.jvm_code.push_instruction(Instruction::Pop2)?;
            self.jvm_code.push_instruction(Instruction::Pop2)?;
        } else {
            self.jvm_code.push_instruction(Instruction::DupX1)?;
            self.jvm_code.push_instruction(Instruction::Pop2)?;
        }
        if let Some(ref_ty) = ref_ty_hint {
            self.jvm_code.generalize_top_stack_type(ref_ty)?;
        }

        self.jvm_code.place_label(end_block)?;

        Ok(())
    }

    /// Visit a return
    fn visit_return(&mut self) -> Result<(), Error> {
        if self.function_typ.outputs.len() > 1 {
            // TODO: this clone is spurious, but to satiate the borrow checker which doesn't
            // know that `pack_stack_into_array` doesn't touch `self.function_typ.outputs`
            // Consider pulling `pack_stack_into_array` out of this class to avoid this.
            self.pack_stack_into_array(&self.function_typ.outputs.clone())?
        }

        self.jvm_code.return_(
            self.function_typ
                .method_descriptor(&self.jvm_code.java.classes)
                .return_type,
        )?;
        Ok(())
    }

    /// Visit a call
    fn visit_call(&mut self, function_index: u32) -> Result<(), Error> {
        let function = &self.wasm_functions[function_index as usize];

        // Load the module reference onto the stack (it is always the last argument)
        let (off, field_type) = self.jvm_locals.lookup_this()?;
        self.jvm_code.get_local(off, &field_type)?;

        // Call the corresponding method and unpack the outputs if need be
        self.jvm_code.invoke(function.method)?;
        if function.func_type.outputs.len() > 1 {
            self.unpack_stack_from_array(&function.func_type.outputs)?;
        }

        Ok(())
    }

    /// Visit a `call_indirect`
    fn visit_call_indirect(&mut self, typ: BlockType, table_idx: u32) -> Result<(), Error> {
        let func_typ = self.wasm_validator.resources().block_type(typ)?;
        let table = &self.wasm_tables[table_idx as usize];

        // Compute the method descriptor we'll actually be calling
        let mut desc = func_typ.method_descriptor(&self.jvm_code.java.classes);
        desc.parameters.push(FieldType::int());
        desc.parameters.push(FieldType::object(self.class));

        let this_off = self.jvm_locals.lookup_this()?.0;
        let bootstrap_method = self.bootstrap_utilities.get_table_bootstrap(
            table_idx,
            table,
            self.jvm_code.class_graph,
            self.utilities,
            self.jvm_code.java,
            self.runtime,
        )?;

        self.jvm_code
            .push_instruction(Instruction::ALoad(this_off))?;
        self.jvm_code
            .invoke_dynamic(bootstrap_method, UnqualifiedName::CALLINDIRECT, desc)?;
        if func_typ.outputs.len() > 1 {
            self.unpack_stack_from_array(&func_typ.outputs)?;
        }

        Ok(())
    }

    /// Pack the top stack elements into an array
    ///
    /// This is used when returning out of functions that return multiple values.
    /// TODO: this could be made into a powerful `invokedynamic` packer (since the `MethodType` is
    /// enough to figure out what to do).
    fn pack_stack_into_array(&mut self, expected: &[StackType]) -> Result<(), Error> {
        let object = FieldType::object(self.jvm_code.java.classes.lang.object);

        // Initialize the variable containing the array for packing values
        let arr_offset = self.jvm_locals.push_local(FieldType::array(object))?;
        self.jvm_code.const_int(expected.len() as i32)?;
        self.jvm_code
            .push_instruction(Instruction::ANewArray(RefType::Object(
                self.jvm_code.java.classes.lang.object,
            )))?;
        self.jvm_code
            .set_local(arr_offset, &FieldType::array(object))?;

        // Initialize the variable containing the index
        let idx_offset = self.jvm_locals.push_local(FieldType::int())?;
        self.jvm_code.const_int(expected.len() as i32 - 1)?;
        self.jvm_code.set_local(idx_offset, &FieldType::int())?;

        // Initialize the a temporary variable for stashing boxed values
        let tmp_offset = self.jvm_locals.push_local(object)?;
        self.jvm_code.zero_local(tmp_offset, object)?;

        for stack_value in expected.iter().rev() {
            // Turn the top value into an object and stack it in the temp variable
            match stack_value {
                StackType::I32 => self
                    .jvm_code
                    .invoke(self.jvm_code.java.members.lang.integer.value_of)?,
                StackType::I64 => self
                    .jvm_code
                    .invoke(self.jvm_code.java.members.lang.long.value_of)?,
                StackType::F32 => self
                    .jvm_code
                    .invoke(self.jvm_code.java.members.lang.float.value_of)?,
                StackType::F64 => self
                    .jvm_code
                    .invoke(self.jvm_code.java.members.lang.double.value_of)?,
                StackType::FuncRef | StackType::ExternRef => (), // already reference types
            }
            self.jvm_code
                .push_instruction(Instruction::AStore(tmp_offset))?;

            // Put the top of the stack in the array
            self.jvm_code
                .push_instruction(Instruction::ALoad(arr_offset))?;
            self.jvm_code
                .push_instruction(Instruction::ILoad(idx_offset))?;
            self.jvm_code
                .push_instruction(Instruction::ALoad(tmp_offset))?;
            self.jvm_code.push_instruction(Instruction::AAStore)?;

            // Update the index
            self.jvm_code
                .push_instruction(Instruction::IInc(idx_offset, -1))?;
        }

        // Put the array back on the stack
        self.jvm_code
            .push_instruction(Instruction::ALoad(arr_offset))?;

        // Kill the locals
        let _ = self.jvm_locals.pop_local()?;
        let _ = self.jvm_locals.pop_local()?;
        let _ = self.jvm_locals.pop_local()?;
        self.jvm_code.kill_top_local(tmp_offset, None)?;
        self.jvm_code
            .kill_top_local(idx_offset, Some(FieldType::int()))?;
        self.jvm_code.kill_top_local(arr_offset, None)?;

        Ok(())
    }

    /// Unpack the top stack elements from an array
    ///
    /// This is used when calling functions that return multiple values.
    fn unpack_stack_from_array(&mut self, expected: &[StackType]) -> Result<(), Error> {
        let object = FieldType::object(self.jvm_code.java.classes.lang.object);

        // Initialize the variable containing the array for packing values
        let arr_offset = self.jvm_locals.push_local(FieldType::array(object))?;
        self.jvm_code
            .push_instruction(Instruction::AStore(arr_offset))?;

        // Initialize the variable containing the index
        let idx_offset = self.jvm_locals.push_local(FieldType::int())?;
        self.jvm_code.push_instruction(Instruction::IConst0)?;
        self.jvm_code
            .push_instruction(Instruction::IStore(idx_offset))?;

        for stack_value in expected.iter() {
            // Put onto the top of stack the next element in the array
            self.jvm_code
                .push_instruction(Instruction::ALoad(arr_offset))?;
            self.jvm_code
                .push_instruction(Instruction::ILoad(idx_offset))?;
            self.jvm_code.push_instruction(Instruction::AALoad)?;

            // Unbox the top of the stack
            match stack_value {
                StackType::I32 => {
                    let integer_cls = RefType::Object(self.jvm_code.java.classes.lang.integer);
                    self.jvm_code
                        .push_instruction(Instruction::CheckCast(integer_cls))?;
                    self.jvm_code
                        .invoke(self.jvm_code.java.members.lang.number.int_value)?;
                }
                StackType::I64 => {
                    let long_cls = RefType::Object(self.jvm_code.java.classes.lang.long);
                    self.jvm_code
                        .push_instruction(Instruction::CheckCast(long_cls))?;
                    self.jvm_code
                        .invoke(self.jvm_code.java.members.lang.number.long_value)?;
                }
                StackType::F32 => {
                    let float_cls = RefType::Object(self.jvm_code.java.classes.lang.float);
                    self.jvm_code
                        .push_instruction(Instruction::CheckCast(float_cls))?;
                    self.jvm_code
                        .invoke(self.jvm_code.java.members.lang.number.float_value)?;
                }
                StackType::F64 => {
                    let double_cls = RefType::Object(self.jvm_code.java.classes.lang.double);
                    self.jvm_code
                        .push_instruction(Instruction::CheckCast(double_cls))?;
                    self.jvm_code
                        .invoke(self.jvm_code.java.members.lang.number.double_value)?;
                }
                StackType::FuncRef => {
                    let handle_cls =
                        RefType::Object(self.jvm_code.java.classes.lang.invoke.method_handle);
                    self.jvm_code
                        .push_instruction(Instruction::CheckCast(handle_cls))?;
                }
                StackType::ExternRef => (), // already supposed to be `java/lang/Object`
            }

            // Update the index
            self.jvm_code
                .push_instruction(Instruction::IInc(idx_offset, 1))?;
        }

        // Kill the locals
        let _ = self.jvm_locals.pop_local()?;
        let _ = self.jvm_locals.pop_local()?;
        self.jvm_code
            .kill_top_local(idx_offset, Some(FieldType::int()))?;
        self.jvm_code.kill_top_local(arr_offset, None)?;

        Ok(())
    }

    /// Debugging check that the top of the JVM stack matches the set of expected input types (eg.
    /// for a block).
    pub fn assert_top_stack(&self, expected: &[StackType]) {
        let current_frame = &self
            .jvm_code
            .current_frame()
            .expect("No current frame")
            .stack;
        assert!(
            current_frame.len() >= expected.len(),
            "Not enough elements on the stack"
        );

        let found_verification_types = current_frame.iter().rev().map(|(_, _, t)| t);
        let expected_verification_types = expected.iter().rev();
        let types_match = found_verification_types
            .zip(expected_verification_types)
            .all(|(ty1, ty2)| *ty1 == ty2.field_type(&self.jvm_code.java.classes).into());

        assert!(types_match, "Stack does not match expected input types");
    }

    /// Visit a global get operator
    fn visit_global_get(&mut self, global_index: u32) -> Result<(), Error> {
        let global = &self.wasm_globals[global_index as usize];

        // Read from the field
        let this_off = self.jvm_locals.lookup_this()?.0;
        self.jvm_code
            .push_instruction(Instruction::ALoad(this_off))?;
        global.read(self.runtime, self.jvm_code)?;

        Ok(())
    }

    /// Visit a global set operator
    fn visit_global_set(&mut self, global_index: u32) -> Result<(), Error> {
        let global = &self.wasm_globals[global_index as usize];
        let global_field_type = global.global_type.field_type(&self.jvm_code.java.classes);

        // Stash the value being set in a local
        let temp_index = self.jvm_locals.push_local(global_field_type)?;
        self.jvm_code.set_local(temp_index, &global_field_type)?;

        // Write to the field
        let this_off = self.jvm_locals.lookup_this()?.0;
        self.jvm_code
            .push_instruction(Instruction::ALoad(this_off))?;
        self.jvm_code.get_local(temp_index, &global_field_type)?;
        global.write(self.runtime, self.jvm_code)?;

        // Clear the local
        self.jvm_code
            .kill_top_local(temp_index, Some(global_field_type))?;
        self.jvm_locals.pop_local()?;

        Ok(())
    }

    /// Visit a table get operator
    fn visit_table_get(&mut self, table_idx: u32) -> Result<(), Error> {
        let table = &self.wasm_tables[table_idx as usize];

        let desc = MethodDescriptor {
            parameters: vec![FieldType::int()],
            return_type: Some(FieldType::Ref(
                table.element_type(&self.jvm_code.java.classes),
            )),
        };
        self.visit_table_operator(table_idx, UnqualifiedName::TABLEGET, desc)?;

        Ok(())
    }

    /// Visit a table set operator
    fn visit_table_set(&mut self, table_idx: u32) -> Result<(), Error> {
        let table = &self.wasm_tables[table_idx as usize];

        let desc = MethodDescriptor {
            parameters: vec![
                FieldType::int(),
                FieldType::Ref(table.element_type(&self.jvm_code.java.classes)),
            ],
            return_type: None,
        };
        self.visit_table_operator(table_idx, UnqualifiedName::TABLESET, desc)?;

        Ok(())
    }

    /// Visit a table intialization operator
    fn visit_table_init(&mut self, element: u32, table: u32) -> Result<(), Error> {
        let element = &self.wasm_elements[element as usize];
        let table = &self.wasm_tables[table as usize];
        let this_off = self.jvm_locals.lookup_this()?.0;

        // Put the length, source index, and destination index in variables
        let len_off = self.jvm_locals.push_local(FieldType::int())?;
        let src_off = self.jvm_locals.push_local(FieldType::int())?;
        let dst_off = self.jvm_locals.push_local(FieldType::int())?;
        self.jvm_code
            .push_instruction(Instruction::IStore(len_off))?;
        self.jvm_code
            .push_instruction(Instruction::IStore(src_off))?;
        self.jvm_code
            .push_instruction(Instruction::IStore(dst_off))?;

        table.init(
            self.runtime,
            self.jvm_code,
            this_off,
            len_off,
            src_off,
            dst_off,
            element,
        )?;

        // Clear the locals
        self.jvm_locals.pop_local()?;
        self.jvm_locals.pop_local()?;
        self.jvm_locals.pop_local()?;
        self.jvm_code
            .kill_top_local(dst_off, Some(FieldType::int()))?;
        self.jvm_code
            .kill_top_local(src_off, Some(FieldType::int()))?;
        self.jvm_code
            .kill_top_local(len_off, Some(FieldType::int()))?;

        Ok(())
    }

    /// Visit a table copy operator
    fn visit_table_copy(&mut self, src_table: u32, dst_table: u32) -> Result<(), Error> {
        let src_table = &self.wasm_tables[src_table as usize];
        let dst_table = &self.wasm_tables[dst_table as usize];
        let this_off = self.jvm_locals.lookup_this()?.0;

        // Number of entries to copy
        let len_idx = self.jvm_locals.push_local(FieldType::int())?;
        self.jvm_code
            .push_instruction(Instruction::IStore(len_idx))?;

        // Copy from this offset
        let src_off_idx = self.jvm_locals.push_local(FieldType::int())?;
        self.jvm_code
            .push_instruction(Instruction::IStore(src_off_idx))?;

        // Copy to this offset
        let dst_off_idx = self.jvm_locals.push_local(FieldType::int())?;
        self.jvm_code
            .push_instruction(Instruction::IStore(dst_off_idx))?;

        // System.arraycopy()
        self.jvm_code
            .push_instruction(Instruction::ALoad(this_off))?;
        src_table.load_array(self.runtime, self.jvm_code)?;
        self.jvm_code
            .push_instruction(Instruction::ILoad(src_off_idx))?;
        self.jvm_code
            .push_instruction(Instruction::ALoad(this_off))?;
        dst_table.load_array(self.runtime, self.jvm_code)?;
        self.jvm_code
            .push_instruction(Instruction::ILoad(dst_off_idx))?;
        self.jvm_code
            .push_instruction(Instruction::ILoad(len_idx))?;
        self.jvm_code
            .invoke(self.jvm_code.java.members.lang.system.arraycopy)?;

        // Clean up temporary locals
        self.jvm_locals.pop_local()?;
        self.jvm_locals.pop_local()?;
        self.jvm_locals.pop_local()?;
        self.jvm_code
            .kill_top_local(dst_off_idx, Some(FieldType::int()))?;
        self.jvm_code
            .kill_top_local(src_off_idx, Some(FieldType::int()))?;
        self.jvm_code
            .kill_top_local(len_idx, Some(FieldType::int()))?;

        Ok(())
    }

    /// Visit a table grow operator
    fn visit_table_grow(&mut self, table_idx: u32) -> Result<(), Error> {
        let table = &self.wasm_tables[table_idx as usize];

        let desc = MethodDescriptor {
            parameters: vec![
                FieldType::Ref(table.element_type(&self.jvm_code.java.classes)),
                FieldType::int(),
            ],
            return_type: Some(FieldType::int()),
        };
        self.visit_table_operator(table_idx, UnqualifiedName::TABLEGROW, desc)?;

        Ok(())
    }

    /// Visit a table size operator
    fn visit_table_size(&mut self, table_idx: u32) -> Result<(), Error> {
        let desc = MethodDescriptor {
            parameters: vec![],
            return_type: Some(FieldType::int()),
        };
        self.visit_table_operator(table_idx, UnqualifiedName::TABLESIZE, desc)?;

        Ok(())
    }

    /// Visit a table fill operator
    fn visit_table_fill(&mut self, table_idx: u32) -> Result<(), Error> {
        let table = &self.wasm_tables[table_idx as usize];

        let desc = MethodDescriptor {
            parameters: vec![
                FieldType::int(),
                FieldType::Ref(table.element_type(&self.jvm_code.java.classes)),
                FieldType::int(),
            ],
            return_type: None,
        };
        self.visit_table_operator(table_idx, UnqualifiedName::TABLEFILL, desc)?;

        Ok(())
    }

    /// Visit a table operator that is handled by the table bootstrap method and issue the
    /// corresponding `invokedynamic` instruction
    fn visit_table_operator(
        &mut self,
        table_idx: u32,
        method_name: UnqualifiedName,
        mut method_type: MethodDescriptor<ClassId<'g>>,
    ) -> Result<(), Error> {
        let table = &self.wasm_tables[table_idx as usize];

        // Compute the method descriptor we'll actually be calling
        method_type.parameters.push(FieldType::object(self.class));

        let this_off = self.jvm_locals.lookup_this()?.0;
        let bootstrap_method = self.bootstrap_utilities.get_table_bootstrap(
            table_idx,
            table,
            self.jvm_code.class_graph,
            self.utilities,
            self.jvm_code.java,
            self.runtime,
        )?;

        self.jvm_code
            .push_instruction(Instruction::ALoad(this_off))?;
        self.jvm_code
            .invoke_dynamic(bootstrap_method, method_name, method_type)?;

        Ok(())
    }

    fn visit_memory_load(&mut self, memarg: MemArg, ty: BaseType) -> Result<(), Error> {
        let memory = &self.wasm_memories[memarg.memory as usize];
        let this_off = self.jvm_locals.lookup_this()?.0;
        memory.load(self.runtime, self.jvm_code, this_off, memarg, ty)?;

        Ok(())
    }

    fn visit_memory_store(&mut self, memarg: MemArg, ty: BaseType) -> Result<(), Error> {
        let memory = &self.wasm_memories[memarg.memory as usize];
        let this_off = self.jvm_locals.lookup_this()?.0;

        // TODO: this is unused if the type has width 1
        let temp_off = self.jvm_locals.push_local(FieldType::Base(ty))?;
        memory.store(self.runtime, self.jvm_code, this_off, temp_off, memarg, ty)?;
        self.jvm_locals.pop_local()?;

        Ok(())
    }

    fn visit_memory_init(&mut self, mem: u32, segment: u32) -> Result<(), Error> {
        let memory = &self.wasm_memories[mem as usize];
        let data = &self.wasm_datas[segment as usize];
        let this_off = self.jvm_locals.lookup_this()?.0;

        // Put the length, source index, and destination index in variables
        let len_off = self.jvm_locals.push_local(FieldType::int())?;
        let src_off = self.jvm_locals.push_local(FieldType::int())?;
        let dst_off = self.jvm_locals.push_local(FieldType::int())?;
        self.jvm_code
            .push_instruction(Instruction::IStore(len_off))?;
        self.jvm_code
            .push_instruction(Instruction::IStore(src_off))?;
        self.jvm_code
            .push_instruction(Instruction::IStore(dst_off))?;

        memory.init(
            self.runtime,
            self.jvm_code,
            this_off,
            len_off,
            src_off,
            dst_off,
            data,
        )?;

        // Clear the locals
        self.jvm_locals.pop_local()?;
        self.jvm_locals.pop_local()?;
        self.jvm_locals.pop_local()?;

        Ok(())
    }

    fn visit_data_drop(&mut self, data: u32) -> Result<(), Error> {
        let data = &self.wasm_datas[data as usize];
        let this_off = self.jvm_locals.lookup_this()?.0;

        data.drop_data(self.jvm_code, this_off)?;

        Ok(())
    }

    fn visit_element_drop(&mut self, element: u32) -> Result<(), Error> {
        let element = &self.wasm_elements[element as usize];
        let this_off = self.jvm_locals.lookup_this()?.0;

        element.drop_element(self.jvm_code, this_off)?;

        Ok(())
    }

    fn visit_memory_copy(&mut self, src_memory: u32, dst_memory: u32) -> Result<(), Error> {
        let src_memory = &self.wasm_memories[src_memory as usize];
        let dst_memory = &self.wasm_memories[dst_memory as usize];
        let this_off = self.jvm_locals.lookup_this()?.0;

        // Number of entries to copy
        let len_idx = self.jvm_locals.push_local(FieldType::int())?;
        self.jvm_code
            .push_instruction(Instruction::IStore(len_idx))?;

        // Copy from this offset
        let src_off_idx = self.jvm_locals.push_local(FieldType::int())?;
        self.jvm_code
            .push_instruction(Instruction::IStore(src_off_idx))?;

        // Copy to this offset
        let dst_off_idx = self.jvm_locals.push_local(FieldType::int())?;
        self.jvm_code
            .push_instruction(Instruction::IStore(dst_off_idx))?;

        // System.arraycopy()
        self.jvm_code
            .push_instruction(Instruction::ALoad(this_off))?;
        src_memory.load_bytebuffer(self.runtime, self.jvm_code)?;
        self.jvm_code
            .push_instruction(Instruction::ILoad(dst_off_idx))?;
        self.jvm_code
            .push_instruction(Instruction::ALoad(this_off))?;
        dst_memory.load_bytebuffer(self.runtime, self.jvm_code)?;
        self.jvm_code
            .push_instruction(Instruction::ILoad(src_off_idx))?;
        self.jvm_code
            .push_instruction(Instruction::ILoad(len_idx))?;
        self.jvm_code
            .invoke(self.jvm_code.java.members.nio.byte_buffer.put_bytebuffer)?;
        self.jvm_code.push_instruction(Instruction::Pop)?;

        // Clean up temporary locals
        let _ = self.jvm_locals.pop_local()?;
        let _ = self.jvm_locals.pop_local()?;
        let _ = self.jvm_locals.pop_local()?;
        self.jvm_code
            .kill_top_local(dst_off_idx, Some(FieldType::int()))?;
        self.jvm_code
            .kill_top_local(src_off_idx, Some(FieldType::int()))?;
        self.jvm_code
            .kill_top_local(len_idx, Some(FieldType::int()))?;

        Ok(())
    }

    /// Visit a memory grow operator
    fn visit_memory_grow(&mut self, memory_idx: u32) -> Result<(), Error> {
        let desc = MethodDescriptor {
            parameters: vec![FieldType::int()],
            return_type: Some(FieldType::int()),
        };
        self.visit_memory_operator(memory_idx, UnqualifiedName::MEMORYGROW, desc)?;

        Ok(())
    }

    /// Visit a memory fill operator
    fn visit_memory_fill(&mut self, memory_idx: u32) -> Result<(), Error> {
        let desc = MethodDescriptor {
            parameters: vec![FieldType::int(), FieldType::int(), FieldType::int()],
            return_type: None,
        };
        self.visit_memory_operator(memory_idx, UnqualifiedName::MEMORYFILL, desc)?;

        Ok(())
    }

    /// Visit a memory size operator
    fn visit_memory_size(&mut self, memory_idx: u32) -> Result<(), Error> {
        let desc = MethodDescriptor {
            parameters: vec![],
            return_type: Some(FieldType::int()),
        };
        self.visit_memory_operator(memory_idx, UnqualifiedName::MEMORYSIZE, desc)?;

        Ok(())
    }

    /// Visit a memory operator that is handled by the memory bootstrap method and issue the
    /// corresponding `invokedynamic` instruction
    fn visit_memory_operator(
        &mut self,
        memory_idx: u32,
        method_name: UnqualifiedName,
        mut method_type: MethodDescriptor<ClassId<'g>>,
    ) -> Result<(), Error> {
        let memory = &self.wasm_memories[memory_idx as usize];

        // Compute the method descriptor we'll actually be calling
        method_type.parameters.push(FieldType::object(self.class));

        let this_off = self.jvm_locals.lookup_this()?.0;
        let bootstrap_method = self.bootstrap_utilities.get_memory_bootstrap(
            memory_idx,
            memory,
            self.jvm_code.class_graph,
            self.utilities,
            self.jvm_code.java,
            self.runtime,
        )?;

        self.jvm_code
            .push_instruction(Instruction::ALoad(this_off))?;
        self.jvm_code
            .invoke_dynamic(bootstrap_method, method_name, method_type)?;

        Ok(())
    }
}

// #[derive(Debug)]
struct LocalsLayout<'g> {
    /// Stack of locals, built up of
    ///
    ///   * the function arguments
    ///   * a reference to the module class
    ///   * a stack of additional tempporary locals
    ///
    jvm_locals: OffsetVec<FieldType<ClassId<'g>>>,

    /// Index into `jvm_locals` for getting the "this" argument
    jvm_module_idx: usize,
}

impl<'g> LocalsLayout<'g> {
    fn new(
        method_arguments: impl Iterator<Item = FieldType<ClassId<'g>>>,
        module_typ: RefType<ClassId<'g>>,
    ) -> Self {
        let mut jvm_locals = OffsetVec::from_iter(method_arguments);
        let jvm_module_idx = jvm_locals.len();
        jvm_locals.push(FieldType::Ref(module_typ));
        LocalsLayout {
            jvm_locals,
            jvm_module_idx,
        }
    }

    /// Lookup the JVM local and index associated with the "this" argument
    fn lookup_this(&self) -> Result<(u16, FieldType<ClassId<'g>>), Error> {
        let (off, field_type) = self
            .jvm_locals
            .get_index(self.jvm_module_idx)
            .expect("missing this local");
        Ok((off.0 as u16, *field_type))
    }

    /// Lookup the JVM local and type associated with a WASM local index
    ///
    /// Adjusts for the fact that JVM locals sometimes take two slots, and that there is an extra
    /// local argument corresponding to the parameter that is used to pass around the module.
    fn lookup_local(&self, mut local_idx: u32) -> Result<(u16, FieldType<ClassId<'g>>), Error> {
        if local_idx as usize >= self.jvm_module_idx {
            local_idx += 1;
        }
        let (off, field_type) = self
            .jvm_locals
            .get_index(local_idx as usize)
            .expect("missing local");
        Ok((off.0 as u16, *field_type))
    }

    /// Push a new local onto our "stack" of locals
    fn push_local(&mut self, field_type: FieldType<ClassId<'g>>) -> Result<u16, Error> {
        let next_local_idx =
            u16::try_from(self.jvm_locals.offset_len().0).map_err(|_| Error::LocalsOverflow)?;
        self.jvm_locals.push(field_type);
        Ok(next_local_idx)
    }

    /// Pop a local from our "stack" of locals
    fn pop_local(&mut self) -> Result<(u16, FieldType<ClassId<'g>>), Error> {
        self.jvm_locals
            .pop()
            .map(|(offset, _, field_type)| (offset.0 as u16, field_type))
            .ok_or(Error::LocalsOverflow)
    }
}
