use super::{
    AccessMode, BootstrapUtilities, BranchCond, CodeBuilderExts, Error, Global, Memory, Settings,
    Table, UtilityClass, UtilityMethod,
};
use crate::jvm::{
    BaseType, BinaryName, BranchInstruction, CodeBuilder, EqComparison, FieldType, Instruction,
    InvokeType, MethodDescriptor, OffsetVec, OrdComparison, RefType, UnqualifiedName, Width,
};
use crate::wasm::{
    ref_type_from_general, ControlFrame, FunctionType, StackType, WasmModuleResourcesExt,
};
use std::collections::HashMap;
use std::convert::TryFrom;
use std::iter::FromIterator;
use std::ops::Not;
use wasmparser::{
    BlockType, BrTable, FuncValidator, FunctionBody, MemoryImmediate, Operator, Type,
    ValidatorResources,
};

/// Context for translating a WASM function into a JVM one
pub struct FunctionTranslator<'a, 'b, B: CodeBuilder + Sized> {
    /// WASM type of the function being translated
    function_typ: FunctionType,

    /// Translation settings
    settings: &'b Settings,

    /// Utilities
    utilities: &'b mut UtilityClass,

    /// Bootstrap utilities (unlike `utilities`, these get cleared across parts)
    bootstrap_utilities: &'b mut BootstrapUtilities,

    /// Code builder
    jvm_code: &'b mut B,

    /// Tables
    wasm_tables: &'b [Table],

    /// Memories
    wasm_memories: &'b [Memory],

    /// Globals
    wasm_globals: &'b [Global<'a>],

    /// Local variables
    jvm_locals: LocalsLayout,

    /// Validator for the WASM function
    wasm_validator: FuncValidator<ValidatorResources>,

    /// Previous height of the WASM stack
    wasm_prev_operand_stack_height: u32,

    /// WASM function being translated
    wasm_function: FunctionBody<'a>,

    /// Stack of WASM structured control flow frames
    wasm_frames: Vec<ControlFrame<B::Lbl>>,

    /// Count of WASM control frames which are unreachable
    wasm_unreachable_frame_count: usize,
}

impl<'a, 'b, B> FunctionTranslator<'a, 'b, B>
where
    B: CodeBuilderExts + Sized,
{
    pub fn new(
        function_typ: FunctionType,
        settings: &'b Settings,
        utilities: &'b mut UtilityClass,
        bootstrap_utilities: &'b mut BootstrapUtilities,
        jvm_code: &'b mut B,
        wasm_tables: &'b [Table],
        wasm_memories: &'b [Memory],
        wasm_globals: &'b [Global<'a>],
        wasm_function: FunctionBody<'a>,
        wasm_validator: FuncValidator<ValidatorResources>,
    ) -> Result<FunctionTranslator<'a, 'b, B>, Error> {
        let jvm_locals = LocalsLayout::new(
            function_typ
                .inputs
                .iter()
                .map(|wasm_ty| wasm_ty.field_type()),
            RefType::Object(settings.output_full_class_name.clone()),
        );

        Ok(FunctionTranslator {
            function_typ,
            settings,
            utilities,
            bootstrap_utilities,
            jvm_code,
            jvm_locals,
            wasm_tables,
            wasm_memories,
            wasm_globals,
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
        // I'd like to use `get_locals_reader`, but that doesn't expose the offset
        let mut reader = self.wasm_function.get_binary_reader();

        for _ in 0..reader.read_var_u32()? {
            let offset = reader.original_position();
            let count = reader.read_var_u32()?;
            let local_type = reader.read_type()?;
            self.wasm_validator
                .define_locals(offset, count, local_type)?;

            // WASM locals are zero initialized
            let local_type = StackType::from_general(local_type)?;
            for _ in 0..count {
                let field_type = local_type.field_type();
                let idx = self.jvm_locals.push_local(field_type.clone())?;
                self.jvm_code.zero_local(idx, field_type)?;
            }
        }

        Ok(())
    }

    /// Visit all operators
    fn visit_operators(&mut self) -> Result<(), Error> {
        let op_reader = self.wasm_function.get_operators_reader()?;
        let mut op_iter = op_reader.into_iter_with_offsets();

        /* When we call `visit_operator`, we need to pass in an operator which we know will get
         * consumed and an option of an operator that may be consumed. We keep a mutable option
         * for the "next" operator, on which `visit_operator` calls `take` if it needs it.
         */
        let mut next_operator: Option<(Operator, usize)> = None;
        loop {
            let this_operator = if let Some(operator) = next_operator.take() {
                operator
            } else if let Some(op_offset) = op_iter.next() {
                op_offset?
            } else {
                break;
            };
            next_operator = match op_iter.next() {
                None => None,
                Some(Ok(next_op)) => Some(next_op),
                Some(Err(err)) => return Err(Error::WasmParser(err)),
            };

            self.visit_operator(this_operator, &mut next_operator)?;
        }

        // If control flow falls through to the end, insert an implicit return
        if self.jvm_code.current_frame().is_some() {
            self.visit_return()?;
        }

        self.wasm_validator.finish(Self::BAD_OFFSET)?;
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
        use crate::jvm::CompareMode::*;
        use crate::jvm::Instruction::*;
        use crate::jvm::ShiftType::*;

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
            Operator::Block { ty } => self.visit_block(ty)?,
            Operator::Loop { ty } => self.visit_loop(ty)?,
            Operator::If { ty } => self.visit_if(ty, BranchCond::If(OrdComparison::NE))?,
            Operator::Else => self.visit_else()?,
            Operator::End => self.visit_end()?,
            Operator::Br { relative_depth } => self.visit_branch(relative_depth)?,
            Operator::BrIf { relative_depth } => {
                self.visit_branch_if(relative_depth, BranchCond::If(OrdComparison::NE))?
            }
            Operator::BrTable { table } => self.visit_branch_table(table)?,
            Operator::Return => self.visit_return()?,
            Operator::Call { function_index } => self.visit_call(function_index)?,
            Operator::CallIndirect {
                index,
                table_index,
                table_byte: _,
            } => {
                // TODO: pick the right table
                self.visit_call_indirect(BlockType::FuncType(index), table_index)?
            }

            // Parametric Instructions
            Operator::Drop => self.jvm_code.pop()?,
            Operator::Select => self.visit_select(None, BranchCond::If(OrdComparison::NE))?,
            Operator::TypedSelect { ty } => {
                self.visit_select(Some(ty), BranchCond::If(OrdComparison::NE))?
            }

            // Variable Instructions
            Operator::LocalGet { local_index } => {
                let (off, field_type) = self.jvm_locals.lookup_local(local_index)?;
                self.jvm_code.get_local(off, field_type)?;
            }
            Operator::LocalSet { local_index } => {
                let (off, field_type) = self.jvm_locals.lookup_local(local_index)?;
                self.jvm_code.set_local(off, field_type)?;
            }
            Operator::LocalTee { local_index } => {
                let (off, field_type) = self.jvm_locals.lookup_local(local_index)?;
                self.jvm_code.dup()?;
                self.jvm_code.set_local(off, field_type)?;
            }
            Operator::GlobalGet { global_index } => self.visit_global_get(global_index)?,
            Operator::GlobalSet { global_index } => self.visit_global_set(global_index)?,

            // Table instructions
            Operator::TableGet { table } => self.visit_table_get(table)?,
            Operator::TableSet { table } => self.visit_table_set(table)?,
            Operator::TableInit { .. } => todo!("table.init"),
            Operator::TableCopy { .. } => todo!("table.copy"),
            Operator::TableGrow { table } => self.visit_table_grow(table)?,
            Operator::TableSize { table } => self.visit_table_size(table)?,
            Operator::TableFill { table } => self.visit_table_fill(table)?,

            // Memory Instructions
            Operator::I32Load { memarg } => {
                self.visit_memory_load(memarg, &UnqualifiedName::GETINT, BaseType::Int)?;
            }
            Operator::I64Load { memarg } => {
                self.visit_memory_load(memarg, &UnqualifiedName::GETLONG, BaseType::Long)?;
            }
            Operator::F32Load { memarg } => {
                self.visit_memory_load(memarg, &UnqualifiedName::GETFLOAT, BaseType::Float)?;
            }
            Operator::F64Load { memarg } => {
                self.visit_memory_load(memarg, &UnqualifiedName::GETDOUBLE, BaseType::Double)?;
            }
            Operator::I32Load8S { memarg } => {
                self.visit_memory_load(memarg, &UnqualifiedName::GET, BaseType::Byte)?;
            }
            Operator::I32Load8U { memarg } => {
                self.visit_memory_load(memarg, &UnqualifiedName::GET, BaseType::Byte)?;
                self.jvm_code.const_int(0xFF)?;
                self.jvm_code.push_instruction(Instruction::IAnd)?;
            }
            Operator::I32Load16S { memarg } => {
                self.visit_memory_load(memarg, &UnqualifiedName::GETSHORT, BaseType::Short)?;
            }
            Operator::I32Load16U { memarg } => {
                self.visit_memory_load(memarg, &UnqualifiedName::GETSHORT, BaseType::Short)?;
                self.jvm_code.const_int(0xFFFF)?;
                self.jvm_code.push_instruction(Instruction::IAnd)?;
            }
            Operator::I64Load8S { memarg } => {
                self.visit_memory_load(memarg, &UnqualifiedName::GET, BaseType::Byte)?;
                self.jvm_code.push_instruction(Instruction::I2L)?;
            }
            Operator::I64Load8U { memarg } => {
                self.visit_memory_load(memarg, &UnqualifiedName::GET, BaseType::Byte)?;
                self.jvm_code.const_int(0xFF)?;
                self.jvm_code.push_instruction(Instruction::IAnd)?;
                self.jvm_code.push_instruction(Instruction::I2L)?;
            }
            Operator::I64Load16S { memarg } => {
                self.visit_memory_load(memarg, &UnqualifiedName::GETSHORT, BaseType::Short)?;
                self.jvm_code.push_instruction(Instruction::I2L)?;
            }
            Operator::I64Load16U { memarg } => {
                self.visit_memory_load(memarg, &UnqualifiedName::GETSHORT, BaseType::Short)?;
                self.jvm_code.const_int(0xFFFF)?;
                self.jvm_code.push_instruction(Instruction::IAnd)?;
                self.jvm_code.push_instruction(Instruction::I2L)?;
            }
            Operator::I64Load32S { memarg } => {
                self.visit_memory_load(memarg, &UnqualifiedName::GETINT, BaseType::Int)?;
                self.jvm_code.push_instruction(Instruction::I2L)?;
            }
            Operator::I64Load32U { memarg } => {
                self.visit_memory_load(memarg, &UnqualifiedName::GETINT, BaseType::Int)?;
                self.jvm_code.push_instruction(Instruction::I2L)?;
                self.jvm_code.const_long(0xFFFFFFFF)?;
                self.jvm_code.push_instruction(Instruction::LAnd)?;
            }
            Operator::I32Store { memarg } => {
                self.visit_memory_store(memarg, &UnqualifiedName::PUTINT, BaseType::Int)?;
            }
            Operator::I64Store { memarg } => {
                self.visit_memory_store(memarg, &UnqualifiedName::PUTLONG, BaseType::Long)?;
            }
            Operator::F32Store { memarg } => {
                self.visit_memory_store(memarg, &UnqualifiedName::PUTFLOAT, BaseType::Float)?;
            }
            Operator::F64Store { memarg } => {
                self.visit_memory_store(memarg, &UnqualifiedName::PUTDOUBLE, BaseType::Double)?;
            }
            Operator::I32Store8 { memarg } => {
                self.visit_memory_store(memarg, &UnqualifiedName::PUT, BaseType::Byte)?;
            }
            Operator::I32Store16 { memarg } => {
                self.visit_memory_store(memarg, &UnqualifiedName::PUTSHORT, BaseType::Short)?;
            }
            Operator::I64Store8 { memarg } => {
                self.jvm_code.const_long(0xFF)?;
                self.jvm_code.push_instruction(Instruction::LAnd)?;
                self.jvm_code.push_instruction(Instruction::L2I)?;
                self.visit_memory_store(memarg, &UnqualifiedName::PUT, BaseType::Byte)?;
            }
            Operator::I64Store16 { memarg } => {
                self.jvm_code.const_long(0xFFFF)?;
                self.jvm_code.push_instruction(Instruction::LAnd)?;
                self.jvm_code.push_instruction(Instruction::L2I)?;
                self.visit_memory_store(memarg, &UnqualifiedName::PUTSHORT, BaseType::Short)?;
            }
            Operator::I64Store32 { memarg } => {
                self.jvm_code.const_long(0xFFFFFFFF)?;
                self.jvm_code.push_instruction(Instruction::LAnd)?;
                self.jvm_code.push_instruction(Instruction::L2I)?;
                self.visit_memory_store(memarg, &UnqualifiedName::PUTINT, BaseType::Int)?;
            }
            Operator::MemorySize { mem, .. } => self.visit_memory_size(mem)?, // TODO: what is `mem_byte` for?
            Operator::MemoryGrow { mem, .. } => self.visit_memory_grow(mem)?,
            Operator::MemoryInit { .. } => todo!("memory.init"),
            Operator::MemoryCopy { .. } => todo!("memory.copy"),
            Operator::MemoryFill { mem } => self.visit_memory_fill(mem)?,
            Operator::DataDrop { .. } => todo!("data.drop"),
            Operator::ElemDrop { .. } => todo!("elem.drop"),

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
                    .invoke(&BinaryName::INTEGER, &UnqualifiedName::COMPAREUNSIGNED)?;
                self.visit_cond(BranchCond::If(OrdComparison::LT), next_op)?;
            }
            Operator::I32GtS => self.visit_cond(BranchCond::IfICmp(OrdComparison::GT), next_op)?,
            Operator::I32GtU => {
                self.jvm_code
                    .invoke(&BinaryName::INTEGER, &UnqualifiedName::COMPAREUNSIGNED)?;
                self.visit_cond(BranchCond::If(OrdComparison::GT), next_op)?;
            }
            Operator::I32LeS => self.visit_cond(BranchCond::IfICmp(OrdComparison::LE), next_op)?,
            Operator::I32LeU => {
                self.jvm_code
                    .invoke(&BinaryName::INTEGER, &UnqualifiedName::COMPAREUNSIGNED)?;
                self.visit_cond(BranchCond::If(OrdComparison::LE), next_op)?;
            }
            Operator::I32GeS => self.visit_cond(BranchCond::IfICmp(OrdComparison::GE), next_op)?,
            Operator::I32GeU => {
                self.jvm_code
                    .invoke(&BinaryName::INTEGER, &UnqualifiedName::COMPAREUNSIGNED)?;
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
                    .invoke(&BinaryName::LONG, &UnqualifiedName::COMPAREUNSIGNED)?;
                self.visit_cond(BranchCond::If(OrdComparison::LT), next_op)?;
            }
            Operator::I64GtS => {
                self.jvm_code.push_instruction(LCmp)?;
                self.visit_cond(BranchCond::If(OrdComparison::GT), next_op)?;
            }
            Operator::I64GtU => {
                self.jvm_code
                    .invoke(&BinaryName::LONG, &UnqualifiedName::COMPAREUNSIGNED)?;
                self.visit_cond(BranchCond::If(OrdComparison::GT), next_op)?;
            }
            Operator::I64LeS => {
                self.jvm_code.push_instruction(LCmp)?;
                self.visit_cond(BranchCond::If(OrdComparison::LE), next_op)?;
            }
            Operator::I64LeU => {
                self.jvm_code
                    .invoke(&BinaryName::LONG, &UnqualifiedName::COMPAREUNSIGNED)?;
                self.visit_cond(BranchCond::If(OrdComparison::LE), next_op)?;
            }
            Operator::I64GeS => {
                self.jvm_code.push_instruction(LCmp)?;
                self.visit_cond(BranchCond::If(OrdComparison::GE), next_op)?;
            }
            Operator::I64GeU => {
                self.jvm_code
                    .invoke(&BinaryName::LONG, &UnqualifiedName::COMPAREUNSIGNED)?;
                self.visit_cond(BranchCond::If(OrdComparison::GE), next_op)?;
            }

            Operator::I32Clz => self
                .jvm_code
                .invoke(&BinaryName::INTEGER, &UnqualifiedName::NUMBEROFLEADINGZEROS)?,
            Operator::I32Ctz => self.jvm_code.invoke(
                &BinaryName::INTEGER,
                &UnqualifiedName::NUMBEROFTRAILINGZEROS,
            )?,
            Operator::I32Popcnt => self
                .jvm_code
                .invoke(&BinaryName::INTEGER, &UnqualifiedName::BITCOUNT)?,
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
                .invoke(&BinaryName::INTEGER, &UnqualifiedName::DIVIDEUNSIGNED)?,
            Operator::I32RemS => self.jvm_code.push_instruction(IRem)?,
            Operator::I32RemU => self
                .jvm_code
                .invoke(&BinaryName::INTEGER, &UnqualifiedName::REMAINDERUNSIGNED)?,
            Operator::I32And => self.jvm_code.push_instruction(IAnd)?,
            Operator::I32Or => self.jvm_code.push_instruction(IOr)?,
            Operator::I32Xor => self.jvm_code.push_instruction(IXor)?,
            Operator::I32Shl => self.jvm_code.push_instruction(ISh(Left))?,
            Operator::I32ShrS => self.jvm_code.push_instruction(ISh(ArithmeticRight))?,
            Operator::I32ShrU => self.jvm_code.push_instruction(ISh(LogicalRight))?,
            Operator::I32Rotl => self
                .jvm_code
                .invoke(&BinaryName::INTEGER, &UnqualifiedName::ROTATELEFT)?,
            Operator::I32Rotr => self
                .jvm_code
                .invoke(&BinaryName::INTEGER, &UnqualifiedName::ROTATERIGHT)?,

            Operator::I64Clz => {
                self.jvm_code
                    .invoke(&BinaryName::LONG, &UnqualifiedName::NUMBEROFLEADINGZEROS)?;
                self.jvm_code.push_instruction(I2L)?;
            }
            Operator::I64Ctz => {
                self.jvm_code
                    .invoke(&BinaryName::LONG, &UnqualifiedName::NUMBEROFTRAILINGZEROS)?;
                self.jvm_code.push_instruction(I2L)?;
            }
            Operator::I64Popcnt => {
                self.jvm_code
                    .invoke(&BinaryName::LONG, &UnqualifiedName::BITCOUNT)?;
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
                .invoke(&BinaryName::LONG, &UnqualifiedName::DIVIDEUNSIGNED)?,
            Operator::I64RemU => self
                .jvm_code
                .invoke(&BinaryName::LONG, &UnqualifiedName::REMAINDERUNSIGNED)?,
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
                    .invoke(&BinaryName::LONG, &UnqualifiedName::ROTATELEFT)?;
            }
            Operator::I64Rotr => {
                self.jvm_code.push_instruction(L2I)?;
                self.jvm_code
                    .invoke(&BinaryName::LONG, &UnqualifiedName::ROTATERIGHT)?;
            }

            Operator::F32Abs => {
                if self.settings.bitwise_floating_abs {
                    self.utilities
                        .invoke_utility(UtilityMethod::F32Abs, self.jvm_code)?;
                } else {
                    let desc = MethodDescriptor {
                        parameters: vec![FieldType::FLOAT],
                        return_type: Some(FieldType::FLOAT),
                    };
                    self.jvm_code.invoke_explicit(
                        InvokeType::Static,
                        &BinaryName::MATH,
                        &UnqualifiedName::ABS,
                        &desc,
                    )?;
                }
            }
            Operator::F32Neg => self.jvm_code.push_instruction(FNeg)?,
            Operator::F32Ceil => {
                self.jvm_code.push_instruction(F2D)?;
                self.jvm_code
                    .invoke(&BinaryName::MATH, &UnqualifiedName::CEIL)?;
                self.jvm_code.push_instruction(D2F)?;
            }
            Operator::F32Floor => {
                self.jvm_code.push_instruction(F2D)?;
                self.jvm_code
                    .invoke(&BinaryName::MATH, &UnqualifiedName::FLOOR)?;
                self.jvm_code.push_instruction(D2F)?;
            }
            Operator::F32Trunc => {
                self.utilities
                    .invoke_utility(UtilityMethod::F32Trunc, self.jvm_code)?;
            }
            Operator::F32Nearest => {
                self.jvm_code.push_instruction(F2D)?;
                self.jvm_code
                    .invoke(&BinaryName::MATH, &UnqualifiedName::RINT)?;
                self.jvm_code.push_instruction(D2F)?;
            }
            Operator::F32Sqrt => {
                self.jvm_code.push_instruction(F2D)?;
                self.jvm_code
                    .invoke(&BinaryName::MATH, &UnqualifiedName::SQRT)?;
                self.jvm_code.push_instruction(D2F)?;
            }
            Operator::F32Add => self.jvm_code.push_instruction(FAdd)?,
            Operator::F32Sub => self.jvm_code.push_instruction(FSub)?,
            Operator::F32Mul => self.jvm_code.push_instruction(FMul)?,
            Operator::F32Div => self.jvm_code.push_instruction(FDiv)?,
            Operator::F32Min => self
                .jvm_code
                .invoke(&BinaryName::FLOAT, &UnqualifiedName::MIN)?,
            Operator::F32Max => self
                .jvm_code
                .invoke(&BinaryName::FLOAT, &UnqualifiedName::MAX)?,
            Operator::F32Copysign => {
                let desc = MethodDescriptor {
                    parameters: vec![FieldType::FLOAT, FieldType::FLOAT],
                    return_type: Some(FieldType::FLOAT),
                };
                self.jvm_code.invoke_explicit(
                    InvokeType::Static,
                    &BinaryName::MATH,
                    &UnqualifiedName::COPYSIGN,
                    &desc,
                )?;
            }
            Operator::F64Abs => {
                if self.settings.bitwise_floating_abs {
                    self.utilities
                        .invoke_utility(UtilityMethod::F64Abs, self.jvm_code)?;
                } else {
                    let desc = MethodDescriptor {
                        parameters: vec![FieldType::DOUBLE],
                        return_type: Some(FieldType::DOUBLE),
                    };
                    self.jvm_code.invoke_explicit(
                        InvokeType::Static,
                        &BinaryName::MATH,
                        &UnqualifiedName::ABS,
                        &desc,
                    )?;
                }
            }
            Operator::F64Neg => self.jvm_code.push_instruction(DNeg)?,
            Operator::F64Ceil => self
                .jvm_code
                .invoke(&BinaryName::MATH, &UnqualifiedName::CEIL)?,
            Operator::F64Floor => self
                .jvm_code
                .invoke(&BinaryName::MATH, &UnqualifiedName::FLOOR)?,
            Operator::F64Trunc => {
                self.utilities
                    .invoke_utility(UtilityMethod::F64Trunc, self.jvm_code)?;
            }
            Operator::F64Nearest => self
                .jvm_code
                .invoke(&BinaryName::MATH, &UnqualifiedName::RINT)?,
            Operator::F64Sqrt => self
                .jvm_code
                .invoke(&BinaryName::MATH, &UnqualifiedName::SQRT)?,
            Operator::F64Add => self.jvm_code.push_instruction(DAdd)?,
            Operator::F64Sub => self.jvm_code.push_instruction(DSub)?,
            Operator::F64Mul => self.jvm_code.push_instruction(DMul)?,
            Operator::F64Div => self.jvm_code.push_instruction(DDiv)?,
            Operator::F64Min => self
                .jvm_code
                .invoke(&BinaryName::DOUBLE, &UnqualifiedName::MIN)?,
            Operator::F64Max => self
                .jvm_code
                .invoke(&BinaryName::DOUBLE, &UnqualifiedName::MAX)?,
            Operator::F64Copysign => {
                let desc = MethodDescriptor {
                    parameters: vec![FieldType::DOUBLE, FieldType::DOUBLE],
                    return_type: Some(FieldType::DOUBLE),
                };
                self.jvm_code.invoke_explicit(
                    InvokeType::Static,
                    &BinaryName::MATH,
                    &UnqualifiedName::COPYSIGN,
                    &desc,
                )?;
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
                .invoke(&BinaryName::FLOAT, &UnqualifiedName::FLOATTORAWINTBITS)?,
            Operator::I64ReinterpretF64 => self
                .jvm_code
                .invoke(&BinaryName::DOUBLE, &UnqualifiedName::DOUBLETORAWLONGBITS)?,
            Operator::F32ReinterpretI32 => self
                .jvm_code
                .invoke(&BinaryName::FLOAT, &UnqualifiedName::INTBITSTOFLOAT)?,
            Operator::F64ReinterpretI64 => self
                .jvm_code
                .invoke(&BinaryName::DOUBLE, &UnqualifiedName::LONGBITSTODOUBLE)?,

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
                let ref_type = ref_type_from_general(ty)?;
                self.jvm_code.const_null(ref_type)?;
            }
            Operator::RefIsNull => {
                self.visit_cond(BranchCond::IfNull(EqComparison::EQ), next_op)?
            }
            Operator::RefFunc { function_index } => {
                let func_typ = self
                    .wasm_validator
                    .resources()
                    .function_idx_type(function_index)?;

                let (class_name, method_name) = self.bad_get_func(function_index, &func_typ);
                self.jvm_code
                    .const_methodhandle(&class_name, &method_name)?;
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
            Some((Operator::If { ty }, offset)) => {
                self.wasm_validator.op(offset, &Operator::If { ty })?;
                self.visit_if(ty, condition)?;
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

                ()
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
    fn prepare_for_branch(&self, relative_depth: u32) -> (u32, Vec<StackType>, Option<B::Lbl>) {
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
        target_label: B::Lbl,
    ) -> Result<(), Error> {
        #[cfg(debug_assertions)]
        self.assert_top_stack(&branch_values);

        if required_pops > 0 {
            // Stash branch values (so we can unwind the stack under them)
            for branch_value in branch_values.iter().rev() {
                let field_type = branch_value.field_type();
                let local_idx = self.jvm_locals.push_local(field_type.clone())?;
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
                self.jvm_code.kill_local(local_idx, field_type)?;
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
    fn visit_select(&mut self, ty: Option<Type>, condition: BranchCond) -> Result<(), Error> {
        let ty = match ty {
            None => None,
            Some(ty) => Some(StackType::from_general(ty)?),
        };

        // The hint only matter for reference types
        let ref_ty_hint: Option<RefType> = ty.and_then(|st| match st.field_type() {
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
        if let Some(ref_ty) = ref_ty_hint.clone() {
            self.jvm_code.push_instruction(Instruction::AHint(ref_ty))?;
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
            self.jvm_code.push_instruction(Instruction::AHint(ref_ty))?;
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

        self.jvm_code
            .return_(self.function_typ.method_descriptor().return_type)?;
        Ok(())
    }

    /// Visit a call
    fn visit_call(&mut self, func_idx: u32) -> Result<(), Error> {
        let func_typ = self
            .wasm_validator
            .resources()
            .function_idx_type(func_idx)?;

        // Load the module reference onto the stack (it is always the last argument)
        let (off, field_type) = self.jvm_locals.lookup_this()?;
        self.jvm_code.get_local(off, field_type)?;

        let (class_name, method_name) = self.bad_get_func(func_idx, &func_typ);

        self.jvm_code.invoke(&class_name, &method_name)?;
        if func_typ.outputs.len() > 1 {
            self.unpack_stack_from_array(&func_typ.outputs)?;
        }

        Ok(())
    }

    // TODO: this is a terrible, no good hack:
    //
    //   - we shouldn't be modifying the class graph in the function translation (worse even:
    //     the addition we make here might be redundant!)
    //   - we shouldn't assume that the function is in `Part0`
    //   - we shouldn't assume that the function index is directly into functions defined in
    //     this module (imported functions come first!)
    //
    fn bad_get_func(
        &self,
        func_idx: u32,
        func_typ: &FunctionType,
    ) -> (BinaryName, UnqualifiedName) {
        let class_name = self
            .settings
            .output_full_class_name
            .concat(&UnqualifiedName::DOLLAR)
            .concat(&self.settings.part_short_class_name)
            .concat(&UnqualifiedName::number(0));
        let method_name = self
            .settings
            .wasm_function_name_prefix
            .concat(&UnqualifiedName::number(func_idx as usize));

        let mut class_graph = self.jvm_code.class_graph();
        let mut desc = func_typ.method_descriptor();
        desc.parameters.push(FieldType::object(
            self.settings.output_full_class_name.clone(),
        ));
        class_graph
            .classes
            .get_mut(&class_name)
            .expect("part class not in class graph")
            .add_method(true, method_name.clone(), desc);

        (class_name, method_name)
    }

    /// Visit a `call_indirect`
    fn visit_call_indirect(&mut self, typ: BlockType, table_idx: u32) -> Result<(), Error> {
        let func_typ = self.wasm_validator.resources().block_type(typ)?;
        let table = &self.wasm_tables[table_idx as usize];

        // Compute the method descriptor we'll actually be calling
        let mut desc: MethodDescriptor = func_typ.method_descriptor();
        desc.parameters.push(FieldType::INT);
        desc.parameters.push(FieldType::object(
            self.settings.output_full_class_name.clone(),
        ));

        let this_off = self.jvm_locals.lookup_this()?.0;
        let bootstrap_method: u16 = self.bootstrap_utilities.get_table_bootstrap(
            table_idx,
            table,
            &self.settings.output_full_class_name,
            &mut self.utilities,
            &mut self.jvm_code.constants(),
        )?;

        self.jvm_code
            .push_instruction(Instruction::ALoad(this_off))?;
        self.jvm_code
            .invoke_dynamic(bootstrap_method, &UnqualifiedName::CALLINDIRECT, &desc)?;
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
        // Initialize the variable containing the array for packing values
        let arr_offset = self
            .jvm_locals
            .push_local(FieldType::array(FieldType::OBJECT))?;
        let object_cls = self.jvm_code.get_class_idx(&RefType::OBJECT)?;
        self.jvm_code.const_int(expected.len() as i32)?;
        self.jvm_code
            .push_instruction(Instruction::ANewArray(object_cls))?;
        self.jvm_code
            .set_local(arr_offset, &FieldType::array(FieldType::OBJECT))?;

        // Initialize the variable containing the index
        let idx_offset = self.jvm_locals.push_local(FieldType::INT)?;
        self.jvm_code.const_int(expected.len() as i32 - 1)?;
        self.jvm_code.set_local(idx_offset, &FieldType::INT)?;

        // Initialize the a temporary variable for stashing boxed values
        let tmp_offset = self.jvm_locals.push_local(FieldType::OBJECT)?;
        self.jvm_code.zero_local(tmp_offset, FieldType::OBJECT)?;

        for stack_value in expected.iter().rev() {
            // Turn the top value into an object and stack it in the temp variable
            match stack_value {
                StackType::I32 => self
                    .jvm_code
                    .invoke(&BinaryName::INTEGER, &UnqualifiedName::VALUEOF)?,
                StackType::I64 => self
                    .jvm_code
                    .invoke(&BinaryName::LONG, &UnqualifiedName::VALUEOF)?,
                StackType::F32 => self
                    .jvm_code
                    .invoke(&BinaryName::FLOAT, &UnqualifiedName::VALUEOF)?,
                StackType::F64 => self
                    .jvm_code
                    .invoke(&BinaryName::DOUBLE, &UnqualifiedName::VALUEOF)?,
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
        self.jvm_code
            .push_instruction(Instruction::AKill(tmp_offset))?;
        self.jvm_code
            .push_instruction(Instruction::IKill(idx_offset))?;
        self.jvm_code
            .push_instruction(Instruction::AKill(arr_offset))?;

        Ok(())
    }

    /// Unpack the top stack elements from an array
    ///
    /// This is used when calling functions that return multiple values.
    fn unpack_stack_from_array(&mut self, expected: &[StackType]) -> Result<(), Error> {
        // Initialize the variable containing the array for packing values
        let arr_offset = self
            .jvm_locals
            .push_local(FieldType::array(FieldType::OBJECT))?;
        self.jvm_code
            .push_instruction(Instruction::AStore(arr_offset))?;

        // Initialize the variable containing the index
        let idx_offset = self.jvm_locals.push_local(FieldType::INT)?;
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
                    let integer_cls = self.jvm_code.get_class_idx(&RefType::INTEGER)?;
                    self.jvm_code
                        .push_instruction(Instruction::CheckCast(integer_cls))?;
                    self.jvm_code
                        .invoke(&BinaryName::NUMBER, &UnqualifiedName::INTVALUE)?;
                }
                StackType::I64 => {
                    let long_cls = self.jvm_code.get_class_idx(&RefType::LONG)?;
                    self.jvm_code
                        .push_instruction(Instruction::CheckCast(long_cls))?;
                    self.jvm_code
                        .invoke(&BinaryName::NUMBER, &UnqualifiedName::LONGVALUE)?;
                }
                StackType::F32 => {
                    let float_cls = self.jvm_code.get_class_idx(&RefType::FLOAT)?;
                    self.jvm_code
                        .push_instruction(Instruction::CheckCast(float_cls))?;
                    self.jvm_code
                        .invoke(&BinaryName::NUMBER, &UnqualifiedName::FLOATVALUE)?;
                }
                StackType::F64 => {
                    let double_cls = self.jvm_code.get_class_idx(&RefType::DOUBLE)?;
                    self.jvm_code
                        .push_instruction(Instruction::CheckCast(double_cls))?;
                    self.jvm_code
                        .invoke(&BinaryName::NUMBER, &UnqualifiedName::DOUBLEVALUE)?;
                }
                StackType::FuncRef => {
                    let handle_cls = self.jvm_code.get_class_idx(&RefType::METHODHANDLE)?;
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
            .push_instruction(Instruction::IKill(idx_offset))?;
        self.jvm_code
            .push_instruction(Instruction::AKill(arr_offset))?;

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
            .all(|(ty1, ty2)| *ty1 == ty2.field_type().into());

        assert!(types_match, "Stack does not match expected input types");
    }

    /// Visit a global get operator
    fn visit_global_get(&mut self, global_index: u32) -> Result<(), Error> {
        let global = &self.wasm_globals[global_index as usize];
        if global.origin.is_internal() {
            let this_off = self.jvm_locals.lookup_this()?.0;
            self.jvm_code
                .push_instruction(Instruction::ALoad(this_off))?;
            self.jvm_code.access_field(
                &self.settings.output_full_class_name,
                &global.field_name,
                AccessMode::Read,
            )?;
        } else {
            todo!()
        }

        Ok(())
    }

    /// Visit a global set operator
    fn visit_global_set(&mut self, global_index: u32) -> Result<(), Error> {
        let global = &self.wasm_globals[global_index as usize];
        let global_field_type = global.global_type.field_type();

        // Stash the value being set in a local
        let temp_index = self.jvm_locals.push_local(global_field_type.clone())?;
        self.jvm_code.set_local(temp_index, &global_field_type)?;

        if global.origin.is_internal() {
            let this_off = self.jvm_locals.lookup_this()?.0;
            self.jvm_code
                .push_instruction(Instruction::ALoad(this_off))?;
            self.jvm_code.get_local(temp_index, &global_field_type)?;
            self.jvm_code.access_field(
                &self.settings.output_full_class_name,
                &global.field_name,
                AccessMode::Write,
            )?;
        } else {
            todo!()
        }

        self.jvm_code.kill_local(temp_index, global_field_type)?;
        self.jvm_locals.pop_local()?;

        Ok(())
    }

    /// Visit a table get operator
    fn visit_table_get(&mut self, table_idx: u32) -> Result<(), Error> {
        let table = &self.wasm_tables[table_idx as usize];

        let desc = MethodDescriptor {
            parameters: vec![FieldType::INT],
            return_type: Some(table.table_type.field_type()),
        };
        self.visit_table_operator(table_idx, &UnqualifiedName::TABLEGET, desc)?;

        Ok(())
    }

    /// Visit a table set operator
    fn visit_table_set(&mut self, table_idx: u32) -> Result<(), Error> {
        let table = &self.wasm_tables[table_idx as usize];

        let desc = MethodDescriptor {
            parameters: vec![FieldType::INT, table.table_type.field_type()],
            return_type: None,
        };
        self.visit_table_operator(table_idx, &UnqualifiedName::TABLESET, desc)?;

        Ok(())
    }

    /// Visit a table grow operator
    fn visit_table_grow(&mut self, table_idx: u32) -> Result<(), Error> {
        let table = &self.wasm_tables[table_idx as usize];

        let desc = MethodDescriptor {
            parameters: vec![table.table_type.field_type(), FieldType::INT],
            return_type: Some(FieldType::INT),
        };
        self.visit_table_operator(table_idx, &UnqualifiedName::TABLEGROW, desc)?;

        Ok(())
    }

    /// Visit a table size operator
    fn visit_table_size(&mut self, table_idx: u32) -> Result<(), Error> {
        let desc = MethodDescriptor {
            parameters: vec![],
            return_type: Some(FieldType::INT),
        };
        self.visit_table_operator(table_idx, &UnqualifiedName::TABLESIZE, desc)?;

        Ok(())
    }

    /// Visit a table fill operator
    fn visit_table_fill(&mut self, table_idx: u32) -> Result<(), Error> {
        let table = &self.wasm_tables[table_idx as usize];

        let desc = MethodDescriptor {
            parameters: vec![
                FieldType::INT,
                table.table_type.field_type(),
                FieldType::INT,
            ],
            return_type: None,
        };
        self.visit_table_operator(table_idx, &UnqualifiedName::TABLEFILL, desc)?;

        Ok(())
    }

    /// Visit a table operator that is handled by the table bootstrap method and issue the
    /// corresponding `invokedynamic` instruction
    fn visit_table_operator(
        &mut self,
        table_idx: u32,
        method_name: &UnqualifiedName,
        mut method_type: MethodDescriptor,
    ) -> Result<(), Error> {
        let table = &self.wasm_tables[table_idx as usize];

        // Compute the method descriptor we'll actually be calling
        method_type.parameters.push(FieldType::object(
            self.settings.output_full_class_name.clone(),
        ));

        let this_off = self.jvm_locals.lookup_this()?.0;
        let bootstrap_method: u16 = self.bootstrap_utilities.get_table_bootstrap(
            table_idx,
            table,
            &self.settings.output_full_class_name,
            &mut self.utilities,
            &mut self.jvm_code.constants(),
        )?;

        self.jvm_code
            .push_instruction(Instruction::ALoad(this_off))?;
        self.jvm_code
            .invoke_dynamic(bootstrap_method, method_name, &method_type)?;

        Ok(())
    }

    fn visit_memory_load(
        &mut self,
        memarg: MemoryImmediate,
        load: &UnqualifiedName,
        ty: BaseType,
    ) -> Result<(), Error> {
        let memory = &self.wasm_memories[memarg.memory as usize];

        // Adjust the offset
        if memarg.offset != 0 {
            self.jvm_code.const_int(memarg.offset as i32)?; // TODO: overflow
            self.jvm_code.push_instruction(Instruction::IAdd)?;
        }

        // Load the memory
        let this_off = self.jvm_locals.lookup_this()?.0;
        self.jvm_code
            .push_instruction(Instruction::ALoad(this_off))?;
        self.jvm_code.access_field(
            &self.settings.output_full_class_name,
            &memory.field_name,
            AccessMode::Read,
        )?;

        // Re-order the stack and call the get function
        self.jvm_code.push_instruction(Instruction::Swap)?;
        self.jvm_code.invoke_explicit(
            InvokeType::Virtual,
            &BinaryName::BYTEBUFFER,
            load,
            &MethodDescriptor {
                parameters: vec![FieldType::INT],
                return_type: Some(FieldType::Base(ty)),
            },
        )?;

        Ok(())
    }

    fn visit_memory_store(
        &mut self,
        memarg: MemoryImmediate,
        store: &UnqualifiedName,
        ty: BaseType,
    ) -> Result<(), Error> {
        let memory = &self.wasm_memories[memarg.memory as usize];

        if ty.width() == 1 && memarg.offset == 0 {
            // Load the memory
            let this_off = self.jvm_locals.lookup_this()?.0;
            self.jvm_code
                .push_instruction(Instruction::ALoad(this_off))?;
            self.jvm_code.access_field(
                &self.settings.output_full_class_name,
                &memory.field_name,
                AccessMode::Read,
            )?;

            // Re-order the stack
            self.jvm_code.push_instruction(Instruction::DupX2)?;
            self.jvm_code.push_instruction(Instruction::Pop)?;
        } else {
            // Stash the value being stored
            let off = self.jvm_locals.push_local(FieldType::Base(ty))?;
            self.jvm_code.set_local(off, &FieldType::Base(ty))?;

            // Adjust the offset
            if memarg.offset != 0 {
                self.jvm_code.const_int(memarg.offset as i32)?; // TODO: overflow
                self.jvm_code.push_instruction(Instruction::IAdd)?;
            }

            // Load the memory
            let this_off = self.jvm_locals.lookup_this()?.0;
            self.jvm_code
                .push_instruction(Instruction::ALoad(this_off))?;
            self.jvm_code.access_field(
                &self.settings.output_full_class_name,
                &memory.field_name,
                AccessMode::Read,
            )?;

            // Re-order the stack
            self.jvm_code.push_instruction(Instruction::Swap)?;
            self.jvm_code.get_local(off, &FieldType::Base(ty))?;
            let (off, ty) = self.jvm_locals.pop_local()?;
            self.jvm_code.kill_local(off, ty)?;
        }

        // Call the store function
        self.jvm_code.invoke_explicit(
            InvokeType::Virtual,
            &BinaryName::BYTEBUFFER,
            store,
            &MethodDescriptor {
                parameters: vec![FieldType::INT, FieldType::Base(ty)],
                return_type: Some(FieldType::object(BinaryName::BYTEBUFFER)),
            },
        )?;
        self.jvm_code.push_instruction(Instruction::Pop)?;

        Ok(())
    }

    /// Visit a memory grow operator
    fn visit_memory_grow(&mut self, memory_idx: u32) -> Result<(), Error> {
        let desc = MethodDescriptor {
            parameters: vec![FieldType::INT],
            return_type: Some(FieldType::INT),
        };
        self.visit_memory_operator(memory_idx, &UnqualifiedName::MEMORYGROW, desc)?;

        Ok(())
    }

    /// Visit a memory fill operator
    fn visit_memory_fill(&mut self, memory_idx: u32) -> Result<(), Error> {
        let desc = MethodDescriptor {
            parameters: vec![FieldType::INT, FieldType::INT, FieldType::INT],
            return_type: None,
        };
        self.visit_memory_operator(memory_idx, &UnqualifiedName::MEMORYFILL, desc)?;

        Ok(())
    }

    /// Visit a memory size operator
    fn visit_memory_size(&mut self, memory_idx: u32) -> Result<(), Error> {
        let desc = MethodDescriptor {
            parameters: vec![],
            return_type: Some(FieldType::INT),
        };
        self.visit_memory_operator(memory_idx, &UnqualifiedName::MEMORYSIZE, desc)?;

        Ok(())
    }

    /// Visit a memory operator that is handled by the memory bootstrap method and issue the
    /// corresponding `invokedynamic` instruction
    fn visit_memory_operator(
        &mut self,
        memory_idx: u32,
        method_name: &UnqualifiedName,
        mut method_type: MethodDescriptor,
    ) -> Result<(), Error> {
        let memory = &self.wasm_memories[memory_idx as usize];

        // Compute the method descriptor we'll actually be calling
        method_type.parameters.push(FieldType::object(
            self.settings.output_full_class_name.clone(),
        ));

        let this_off = self.jvm_locals.lookup_this()?.0;
        let bootstrap_method: u16 = self.bootstrap_utilities.get_memory_bootstrap(
            memory_idx,
            memory,
            &self.settings.output_full_class_name,
            &mut self.utilities,
            &mut self.jvm_code.constants(),
        )?;

        self.jvm_code
            .push_instruction(Instruction::ALoad(this_off))?;
        self.jvm_code
            .invoke_dynamic(bootstrap_method, method_name, &method_type)?;

        Ok(())
    }

    // TODO: everywhere we use this, we should find a way to thread through the _actual_ offset
    const BAD_OFFSET: usize = 0;
}

#[derive(Debug)]
struct LocalsLayout {
    /// Stack of locals, built up of
    ///
    ///   * the function arguments
    ///   * a reference to the module class
    ///   * a stack of additional tempporary locals
    ///
    jvm_locals: OffsetVec<FieldType>,

    /// Index into `jvm_locals` for getting the "this" argument
    jvm_module_idx: usize,
}

impl LocalsLayout {
    fn new(method_arguments: impl Iterator<Item = FieldType>, module_typ: RefType) -> Self {
        let mut jvm_locals = OffsetVec::from_iter(method_arguments);
        let jvm_module_idx = jvm_locals.len();
        jvm_locals.push(FieldType::Ref(module_typ));
        LocalsLayout {
            jvm_locals,
            jvm_module_idx,
        }
    }

    /// Lookup the JVM local and index associated with the "this" argument
    fn lookup_this(&self) -> Result<(u16, &FieldType), Error> {
        let (off, field_type) = self
            .jvm_locals
            .get_index(self.jvm_module_idx)
            .expect("missing this local");
        Ok((off.0 as u16, &field_type))
    }

    /// Lookup the JVM local and type associated with a WASM local index
    ///
    /// Adjusts for the fact that JVM locals sometimes take two slots, and that there is an extra
    /// local argument corresponding to the parameter that is used to pass around the module.
    fn lookup_local(&self, mut local_idx: u32) -> Result<(u16, &FieldType), Error> {
        if local_idx as usize >= self.jvm_module_idx {
            local_idx += 1;
        }
        let (off, field_type) = self
            .jvm_locals
            .get_index(local_idx as usize)
            .expect("missing local");
        Ok((off.0 as u16, &field_type))
    }

    /// Push a new local onto our "stack" of locals
    fn push_local(&mut self, field_type: FieldType) -> Result<u16, Error> {
        let next_local_idx =
            u16::try_from(self.jvm_locals.offset_len().0).map_err(|_| Error::LocalsOverflow)?;
        self.jvm_locals.push(field_type);
        Ok(next_local_idx)
    }

    /// Pop a local from our "stack" of locals
    fn pop_local(&mut self) -> Result<(u16, FieldType), Error> {
        self.jvm_locals
            .pop()
            .map(|(offset, _, field_type)| (offset.0 as u16, field_type))
            .ok_or(Error::LocalsOverflow)
    }
}
