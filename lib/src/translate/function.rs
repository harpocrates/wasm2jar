use super::{BranchCond, CodeBuilderExts, Error, Settings};
use crate::jvm::{
    BranchInstruction, CodeBuilder, EqComparison, FieldType, Instruction, InvokeType,
    MethodDescriptor, OffsetVec, OrdComparison, RefType, Width,
};
use crate::wasm::{
    ref_type_from_general, ControlFrame, FunctionType, StackType, WasmModuleResourcesExt,
};
use std::borrow::Cow;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::iter::FromIterator;
use std::ops::Not;
use wasmparser::{BrTable, FuncValidator, FunctionBody, Operator, Type, TypeOrFuncType};

/// Context for translating a WASM function into a JVM one
pub struct FunctionTranslator<'a, 'b, B: CodeBuilder + Sized, R> {
    /// WASM type of the function being translated
    function_typ: FunctionType,

    /// Translation settings
    settings: &'b Settings,

    /// Code builder
    jvm_code: &'b mut B,

    /// Local variables
    jvm_locals: LocalsLayout,

    /// Validator for the WASM function
    wasm_validator: FuncValidator<R>,

    /// Previous height of the WASM stack
    wasm_prev_operand_stack_height: u32,

    /// WASM function being translated
    wasm_function: FunctionBody<'a>,

    /// Stack of WASM structured control flow frames
    wasm_frames: Vec<ControlFrame<B::Lbl>>,

    /// Count of WASM control frames which are unreachable
    wasm_unreachable_frame_count: usize,
}

impl<'a, 'b, B, R> FunctionTranslator<'a, 'b, B, R>
where
    B: CodeBuilderExts + Sized,
    R: WasmModuleResourcesExt,
{
    pub fn new(
        function_typ: FunctionType,
        settings: &'b Settings,
        jvm_code: &'b mut B,
        wasm_function: FunctionBody<'a>,
        wasm_validator: FuncValidator<R>,
    ) -> Result<FunctionTranslator<'a, 'b, B, R>, Error> {
        let jvm_locals = LocalsLayout::new(
            function_typ
                .inputs
                .iter()
                .map(|wasm_ty| wasm_ty.field_type()),
            RefType::object(settings.output_full_class_name.clone()),
        );

        Ok(FunctionTranslator {
            function_typ,
            settings,
            jvm_code,
            jvm_locals,
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
        use crate::jvm::BranchInstruction::*;
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
            Operator::Unreachable => todo!(),
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
            Operator::CallIndirect { .. } => todo!(),

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
            Operator::GlobalGet { .. } => todo!(),
            Operator::GlobalSet { .. } => todo!(),

            // Table instructions
            Operator::TableGet { .. } => todo!(),
            Operator::TableSet { .. } => todo!(),
            Operator::TableInit { .. } => todo!(),
            Operator::TableCopy { .. } => todo!(),
            Operator::TableGrow { .. } => todo!(),
            Operator::TableSize { .. } => todo!(),
            Operator::TableFill { .. } => todo!(),

            // Memory Instructions
            Operator::I32Load { .. } => todo!(),
            Operator::I64Load { .. } => todo!(),
            Operator::F32Load { .. } => todo!(),
            Operator::F64Load { .. } => todo!(),
            Operator::I32Load8S { .. } => todo!(),
            Operator::I32Load8U { .. } => todo!(),
            Operator::I32Load16S { .. } => todo!(),
            Operator::I32Load16U { .. } => todo!(),
            Operator::I64Load8S { .. } => todo!(),
            Operator::I64Load8U { .. } => todo!(),
            Operator::I64Load16S { .. } => todo!(),
            Operator::I64Load16U { .. } => todo!(),
            Operator::I64Load32S { .. } => todo!(),
            Operator::I64Load32U { .. } => todo!(),
            Operator::I32Store { .. } => todo!(),
            Operator::I64Store { .. } => todo!(),
            Operator::F32Store { .. } => todo!(),
            Operator::F64Store { .. } => todo!(),
            Operator::I32Store8 { .. } => todo!(),
            Operator::I32Store16 { .. } => todo!(),
            Operator::I64Store8 { .. } => todo!(),
            Operator::I64Store16 { .. } => todo!(),
            Operator::I64Store32 { .. } => todo!(),
            Operator::MemorySize { .. } => todo!(),
            Operator::MemoryGrow { .. } => todo!(),
            Operator::MemoryInit { .. } => todo!(),
            Operator::MemoryCopy { .. } => todo!(),
            Operator::MemoryFill { .. } => todo!(),
            Operator::DataDrop { .. } => todo!(),
            Operator::ElemDrop { .. } => todo!(),

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
                    .invoke(RefType::INTEGER_NAME, "compareUnsigned")?;
                self.visit_cond(BranchCond::If(OrdComparison::LT), next_op)?;
            }
            Operator::I32GtS => self.visit_cond(BranchCond::IfICmp(OrdComparison::GT), next_op)?,
            Operator::I32GtU => {
                self.jvm_code
                    .invoke(RefType::INTEGER_NAME, "compareUnsigned")?;
                self.visit_cond(BranchCond::If(OrdComparison::GT), next_op)?;
            }
            Operator::I32LeS => self.visit_cond(BranchCond::IfICmp(OrdComparison::LE), next_op)?,
            Operator::I32LeU => {
                self.jvm_code
                    .invoke(RefType::INTEGER_NAME, "compareUnsigned")?;
                self.visit_cond(BranchCond::If(OrdComparison::LE), next_op)?;
            }
            Operator::I32GeS => self.visit_cond(BranchCond::IfICmp(OrdComparison::GE), next_op)?,
            Operator::I32GeU => {
                self.jvm_code
                    .invoke(RefType::INTEGER_NAME, "compareUnsigned")?;
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
                    .invoke(RefType::LONG_NAME, "compareUnsigned")?;
                self.visit_cond(BranchCond::If(OrdComparison::LT), next_op)?;
            }
            Operator::I64GtS => {
                self.jvm_code.push_instruction(LCmp)?;
                self.visit_cond(BranchCond::If(OrdComparison::GT), next_op)?;
            }
            Operator::I64GtU => {
                self.jvm_code
                    .invoke(RefType::LONG_NAME, "compareUnsigned")?;
                self.visit_cond(BranchCond::If(OrdComparison::GT), next_op)?;
            }
            Operator::I64LeS => {
                self.jvm_code.push_instruction(LCmp)?;
                self.visit_cond(BranchCond::If(OrdComparison::LE), next_op)?;
            }
            Operator::I64LeU => {
                self.jvm_code
                    .invoke(RefType::LONG_NAME, "compareUnsigned")?;
                self.visit_cond(BranchCond::If(OrdComparison::LE), next_op)?;
            }
            Operator::I64GeS => {
                self.jvm_code.push_instruction(LCmp)?;
                self.visit_cond(BranchCond::If(OrdComparison::GE), next_op)?;
            }
            Operator::I64GeU => {
                self.jvm_code
                    .invoke(RefType::LONG_NAME, "compareUnsigned")?;
                self.visit_cond(BranchCond::If(OrdComparison::GE), next_op)?;
            }

            Operator::I32Clz => self
                .jvm_code
                .invoke(RefType::INTEGER_NAME, "numberOfLeadingZeros")?,
            Operator::I32Ctz => self
                .jvm_code
                .invoke(RefType::INTEGER_NAME, "numberOfTrailingZeros")?,
            Operator::I32Popcnt => self.jvm_code.invoke(RefType::INTEGER_NAME, "bitCount")?,
            Operator::I32Add => self.jvm_code.push_instruction(IAdd)?,
            Operator::I32Sub => self.jvm_code.push_instruction(ISub)?,
            Operator::I32Mul => self.jvm_code.push_instruction(IMul)?,
            Operator::I32DivS => {
                if self.settings.trap_integer_division_overflow {
                    // TODO: if we're dividing i32.min_value by -1, throw an exception
                    self.jvm_code.push_instruction(IDiv)?;
                } else {
                    self.jvm_code.push_instruction(IDiv)?;
                }
            }
            Operator::I32DivU => self
                .jvm_code
                .invoke(RefType::INTEGER_NAME, "divideUnsigned")?,
            Operator::I32RemS => self.jvm_code.push_instruction(IRem)?,
            Operator::I32RemU => self
                .jvm_code
                .invoke(RefType::INTEGER_NAME, "remainderUnsigned")?,
            Operator::I32And => self.jvm_code.push_instruction(IAnd)?,
            Operator::I32Or => self.jvm_code.push_instruction(IOr)?,
            Operator::I32Xor => self.jvm_code.push_instruction(IXor)?,
            Operator::I32Shl => self.jvm_code.push_instruction(ISh(Left))?,
            Operator::I32ShrS => self.jvm_code.push_instruction(ISh(ArithmeticRight))?,
            Operator::I32ShrU => self.jvm_code.push_instruction(ISh(LogicalRight))?,
            Operator::I32Rotl => self.jvm_code.invoke(RefType::INTEGER_NAME, "rotateLeft")?,
            Operator::I32Rotr => self.jvm_code.invoke(RefType::INTEGER_NAME, "rotateRight")?,

            Operator::I64Clz => {
                self.jvm_code
                    .invoke(RefType::LONG_NAME, "numberOfLeadingZeros")?;
                self.jvm_code.push_instruction(I2L)?;
            }
            Operator::I64Ctz => {
                self.jvm_code
                    .invoke(RefType::LONG_NAME, "numberOfTrailingZeros")?;
                self.jvm_code.push_instruction(I2L)?;
            }
            Operator::I64Popcnt => {
                self.jvm_code.invoke(RefType::LONG_NAME, "bitCount")?;
                self.jvm_code.push_instruction(I2L)?;
            }
            Operator::I64Add => self.jvm_code.push_instruction(LAdd)?,
            Operator::I64Sub => self.jvm_code.push_instruction(LSub)?,
            Operator::I64Mul => self.jvm_code.push_instruction(LMul)?,
            Operator::I64DivS => {
                if self.settings.trap_integer_division_overflow {
                    // TODO: if we're dividing i64.min_value by -1, throw an exception
                    self.jvm_code.push_instruction(LDiv)?;
                } else {
                    self.jvm_code.push_instruction(LDiv)?;
                }
            }
            Operator::I64RemS => self.jvm_code.push_instruction(LRem)?,
            Operator::I64DivU => self.jvm_code.invoke(RefType::LONG_NAME, "divideUnsigned")?,
            Operator::I64RemU => self
                .jvm_code
                .invoke(RefType::LONG_NAME, "remainderUnsigned")?,
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
                self.jvm_code.invoke(RefType::LONG_NAME, "rotateLeft")?;
            }
            Operator::I64Rotr => {
                self.jvm_code.push_instruction(L2I)?;
                self.jvm_code.invoke(RefType::LONG_NAME, "rotateRight")?;
            }

            // Note: we don't use `abs(F)F` because that does not flip the NaN bit
            Operator::F32Abs => {
                self.jvm_code.push_instruction(F2D)?;
                let desc = MethodDescriptor {
                    parameters: vec![FieldType::DOUBLE],
                    return_type: Some(FieldType::DOUBLE),
                };
                self.jvm_code.invoke_explicit(
                    InvokeType::Static,
                    RefType::MATH_NAME,
                    "abs",
                    &desc,
                )?;
                self.jvm_code.push_instruction(D2F)?;
            }
            Operator::F32Neg => self.jvm_code.push_instruction(FNeg)?,
            Operator::F32Ceil => {
                self.jvm_code.push_instruction(F2D)?;
                self.jvm_code.invoke(RefType::MATH_NAME, "ceil")?;
                self.jvm_code.push_instruction(D2F)?;
            }
            Operator::F32Floor => {
                self.jvm_code.push_instruction(F2D)?;
                self.jvm_code.invoke(RefType::MATH_NAME, "floor")?;
                self.jvm_code.push_instruction(D2F)?;
            }
            Operator::F32Trunc => {
                // TODO: move this to a utility method
                let negative = self.jvm_code.fresh_label();
                let end = self.jvm_code.fresh_label();
                self.jvm_code.push_instruction(F2D)?;
                self.jvm_code.push_instruction(Dup2)?;
                self.jvm_code.push_instruction(DConst0)?;
                self.jvm_code.push_instruction(DCmp(G))?;
                self.jvm_code
                    .push_branch_instruction(If(OrdComparison::LT, negative, ()))?;
                self.jvm_code.invoke(RefType::MATH_NAME, "floor")?;
                self.jvm_code.push_branch_instruction(Goto(end))?;
                self.jvm_code.place_label(negative)?;
                self.jvm_code.invoke(RefType::MATH_NAME, "ceil")?;
                self.jvm_code.place_label(end)?;
                self.jvm_code.push_instruction(D2F)?;
            }
            Operator::F32Nearest => {
                self.jvm_code.push_instruction(F2D)?;
                self.jvm_code.invoke(RefType::MATH_NAME, "rint")?;
                self.jvm_code.push_instruction(D2F)?;
            }
            Operator::F32Sqrt => {
                self.jvm_code.push_instruction(F2D)?;
                self.jvm_code.invoke(RefType::MATH_NAME, "sqrt")?;
                self.jvm_code.push_instruction(D2F)?;
            }
            Operator::F32Add => self.jvm_code.push_instruction(FAdd)?,
            Operator::F32Sub => self.jvm_code.push_instruction(FSub)?,
            Operator::F32Mul => self.jvm_code.push_instruction(FMul)?,
            Operator::F32Div => self.jvm_code.push_instruction(FDiv)?,
            Operator::F32Min => self.jvm_code.invoke(RefType::FLOAT_NAME, "min")?,
            Operator::F32Max => self.jvm_code.invoke(RefType::FLOAT_NAME, "max")?,
            Operator::F32Copysign => {
                let desc = MethodDescriptor {
                    parameters: vec![FieldType::FLOAT, FieldType::FLOAT],
                    return_type: Some(FieldType::FLOAT),
                };
                self.jvm_code.invoke_explicit(
                    InvokeType::Static,
                    RefType::MATH_NAME,
                    "copySign",
                    &desc,
                )?;
            }
            Operator::F64Abs => {
                let desc = MethodDescriptor {
                    parameters: vec![FieldType::DOUBLE],
                    return_type: Some(FieldType::DOUBLE),
                };
                self.jvm_code.invoke_explicit(
                    InvokeType::Static,
                    RefType::MATH_NAME,
                    "abs",
                    &desc,
                )?;
            }
            Operator::F64Neg => self.jvm_code.push_instruction(DNeg)?,
            Operator::F64Ceil => self.jvm_code.invoke(RefType::MATH_NAME, "ceil")?,
            Operator::F64Floor => self.jvm_code.invoke(RefType::MATH_NAME, "floor")?,
            Operator::F64Trunc => {
                // TODO: move this to a utility method
                let negative = self.jvm_code.fresh_label();
                let end = self.jvm_code.fresh_label();
                self.jvm_code.push_instruction(Dup2)?;
                self.jvm_code.push_instruction(DConst0)?;
                self.jvm_code.push_instruction(DCmp(G))?;
                self.jvm_code
                    .push_branch_instruction(If(OrdComparison::LT, negative, ()))?;
                self.jvm_code.invoke(RefType::MATH_NAME, "floor")?;
                self.jvm_code.push_branch_instruction(Goto(end))?;
                self.jvm_code.place_label(negative)?;
                self.jvm_code.invoke(RefType::MATH_NAME, "ceil")?;
                self.jvm_code.place_label(end)?;
            }
            Operator::F64Nearest => self.jvm_code.invoke(RefType::MATH_NAME, "rint")?,
            Operator::F64Sqrt => self.jvm_code.invoke(RefType::MATH_NAME, "sqrt")?,
            Operator::F64Add => self.jvm_code.push_instruction(DAdd)?,
            Operator::F64Sub => self.jvm_code.push_instruction(DSub)?,
            Operator::F64Mul => self.jvm_code.push_instruction(DMul)?,
            Operator::F64Div => self.jvm_code.push_instruction(DDiv)?,
            Operator::F64Min => self.jvm_code.invoke(RefType::DOUBLE_NAME, "min")?,
            Operator::F64Max => self.jvm_code.invoke(RefType::DOUBLE_NAME, "max")?,
            Operator::F64Copysign => {
                let desc = MethodDescriptor {
                    parameters: vec![FieldType::DOUBLE, FieldType::DOUBLE],
                    return_type: Some(FieldType::DOUBLE),
                };
                self.jvm_code.invoke_explicit(
                    InvokeType::Static,
                    RefType::MATH_NAME,
                    "copySign",
                    &desc,
                )?;
            }

            Operator::I32WrapI64 => self.jvm_code.push_instruction(L2I)?,
            Operator::I32TruncF32S => todo!("utility method"),
            Operator::I32TruncF32U => todo!(),
            Operator::I32TruncF64S => todo!("utility method"),
            Operator::I32TruncF64U => todo!(),
            Operator::I64ExtendI32S => self.jvm_code.push_instruction(I2L)?,
            Operator::I64ExtendI32U => {
                // TODO: move this to a utility method
                self.jvm_code.push_instruction(I2L)?;
                self.jvm_code.const_long(0x0000_0000_ffff_ffff)?;
                self.jvm_code.push_instruction(LAnd)?;
            }
            Operator::I64TruncF32S => todo!("utility method"),
            Operator::I64TruncF32U => todo!(),
            Operator::I64TruncF64S => todo!("utility method"),
            Operator::I64TruncF64U => todo!(),
            Operator::F32ConvertI32S => self.jvm_code.push_instruction(I2F)?,
            Operator::F32ConvertI32U => {
                // TODO: move this to a utility method
                self.jvm_code.push_instruction(I2L)?;
                self.jvm_code.const_long(0x0000_0000_ffff_ffff)?;
                self.jvm_code.push_instruction(LAnd)?;
                self.jvm_code.push_instruction(L2F)?;
            }
            Operator::F32ConvertI64S => self.jvm_code.push_instruction(L2F)?,
            Operator::F32ConvertI64U => todo!(),
            Operator::F32DemoteF64 => self.jvm_code.push_instruction(D2F)?,
            Operator::F64ConvertI32S => self.jvm_code.push_instruction(I2D)?,
            Operator::F64ConvertI32U => {
                // TODO: move this to a utility method
                self.jvm_code.push_instruction(I2L)?;
                self.jvm_code.const_long(0x0000_0000_ffff_ffff)?;
                self.jvm_code.push_instruction(LAnd)?;
                self.jvm_code.push_instruction(L2D)?;
            }
            Operator::F64ConvertI64S => self.jvm_code.push_instruction(L2D)?,
            Operator::F64ConvertI64U => {
                // TODO: move this to a utility method
                let first_bit_one = self.jvm_code.fresh_label();
                let end = self.jvm_code.fresh_label();
                self.jvm_code.push_instruction(Dup2)?;
                self.jvm_code.push_instruction(LConst0)?;
                self.jvm_code.push_instruction(LCmp)?;
                self.jvm_code
                    .push_branch_instruction(If(OrdComparison::LT, first_bit_one, ()))?;
                self.jvm_code.push_instruction(L2D)?;
                self.jvm_code.push_branch_instruction(Goto(end))?;
                self.jvm_code.place_label(first_bit_one)?;
                self.jvm_code.push_instruction(Dup2)?;
                self.jvm_code.push_instruction(IConst1)?;
                self.jvm_code.push_instruction(LSh(LogicalRight))?;
                self.jvm_code.push_instruction(Dup2X2)?;
                self.jvm_code.push_instruction(Pop2)?;
                self.jvm_code.push_instruction(LConst1)?;
                self.jvm_code.push_instruction(LAnd)?;
                self.jvm_code.push_instruction(LOr)?;
                self.jvm_code.push_instruction(L2D)?;
                self.jvm_code.push_instruction(IConst2)?;
                self.jvm_code.push_instruction(I2D)?;
                self.jvm_code.push_instruction(DMul)?;
                self.jvm_code.place_label(end)?;
            }
            Operator::F64PromoteF32 => self.jvm_code.push_instruction(F2D)?,

            Operator::I32ReinterpretF32 => self
                .jvm_code
                .invoke(RefType::FLOAT_NAME, "floatToRawIntBits")?,
            Operator::I64ReinterpretF64 => self
                .jvm_code
                .invoke(RefType::DOUBLE_NAME, "doubleToRawLongBits")?,
            Operator::F32ReinterpretI32 => self
                .jvm_code
                .invoke(RefType::FLOAT_NAME, "intBitsToFloat")?,
            Operator::F64ReinterpretI64 => self
                .jvm_code
                .invoke(RefType::DOUBLE_NAME, "longBitsToDouble")?,

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
            Operator::I32TruncSatF32U => todo!(),
            Operator::I32TruncSatF64S => self.jvm_code.push_instruction(D2I)?,
            Operator::I32TruncSatF64U => todo!(),
            Operator::I64TruncSatF32S => self.jvm_code.push_instruction(F2L)?,
            Operator::I64TruncSatF32U => todo!(),
            Operator::I64TruncSatF64S => self.jvm_code.push_instruction(D2L)?,
            Operator::I64TruncSatF64U => todo!(),

            // Reference Instructions
            Operator::RefNull { ty } => {
                let ref_type = ref_type_from_general(ty)?;
                self.jvm_code.const_null(ref_type)?;
            }
            Operator::RefIsNull => {
                self.visit_cond(BranchCond::IfNull(EqComparison::EQ), next_op)?
            }
            Operator::RefFunc { .. } => todo!(),

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
    fn visit_if(&mut self, ty: TypeOrFuncType, condition: BranchCond) -> Result<(), Error> {
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
    fn visit_block(&mut self, ty: TypeOrFuncType) -> Result<(), Error> {
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
    fn visit_loop(&mut self, ty: TypeOrFuncType) -> Result<(), Error> {
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

        for target in table.targets() {
            let (relative_depth, _is_default) = target?;
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

        // TODO: this is a terrible, no good hack:
        //
        //   - we shouldn't be modifying the class graph in the function translation (worse even:
        //     the addition we make here might be redundant!)
        //   - we shouldn't assume that the function is in `Part0`
        //   - we shouldn't assume that the function index is directly into functions defined in
        //     this module (imported functions come first!)
        //
        let class_name = format!(
            "{}${}0",
            self.settings.output_full_class_name, self.settings.part_short_class_name,
        );
        let method_name = format!("{}{}", self.settings.wasm_function_name_prefix, func_idx);
        {
            let mut class_graph = self.jvm_code.class_graph();
            let mut desc = func_typ.method_descriptor();
            desc.parameters.push(FieldType::object(
                self.settings.output_full_class_name.clone(),
            ));
            class_graph
                .classes
                .get_mut(&Cow::Owned(class_name.clone()))
                .expect("part class not in class graph")
                .add_method(true, method_name.clone(), desc);
        }

        self.jvm_code.invoke(class_name, method_name)?;
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
        let object_cls = self.jvm_code.get_class_idx(&RefType::OBJECT_CLASS)?;
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
                StackType::I32 => self.jvm_code.invoke(RefType::INTEGER_NAME, "valueOf")?,
                StackType::I64 => self.jvm_code.invoke(RefType::LONG_NAME, "valueOf")?,
                StackType::F32 => self.jvm_code.invoke(RefType::FLOAT_NAME, "valueOf")?,
                StackType::F64 => self.jvm_code.invoke(RefType::DOUBLE_NAME, "valueOf")?,
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

        for stack_value in expected.iter().rev() {
            // Put onto the top of stack the next element in the array
            self.jvm_code
                .push_instruction(Instruction::ALoad(arr_offset))?;
            self.jvm_code
                .push_instruction(Instruction::ILoad(idx_offset))?;
            self.jvm_code.push_instruction(Instruction::AALoad)?;

            // Unbox the top of the stack
            match stack_value {
                StackType::I32 => {
                    let integer_cls = self.jvm_code.get_class_idx(&RefType::INTEGER_CLASS)?;
                    self.jvm_code
                        .push_instruction(Instruction::CheckCast(integer_cls))?;
                    self.jvm_code.invoke(RefType::NUMBER_NAME, "intValue")?;
                }
                StackType::I64 => {
                    let long_cls = self.jvm_code.get_class_idx(&RefType::LONG_CLASS)?;
                    self.jvm_code
                        .push_instruction(Instruction::CheckCast(long_cls))?;
                    self.jvm_code.invoke(RefType::NUMBER_NAME, "longValue")?;
                }
                StackType::F32 => {
                    let float_cls = self.jvm_code.get_class_idx(&RefType::FLOAT_CLASS)?;
                    self.jvm_code
                        .push_instruction(Instruction::CheckCast(float_cls))?;
                    self.jvm_code.invoke(RefType::NUMBER_NAME, "floatValue")?;
                }
                StackType::F64 => {
                    let double_cls = self.jvm_code.get_class_idx(&RefType::DOUBLE_CLASS)?;
                    self.jvm_code
                        .push_instruction(Instruction::CheckCast(double_cls))?;
                    self.jvm_code.invoke(RefType::NUMBER_NAME, "doubleValue")?;
                }
                StackType::FuncRef => {
                    let handle_cls = self.jvm_code.get_class_idx(&RefType::METHOD_HANDLE_CLASS)?;
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
