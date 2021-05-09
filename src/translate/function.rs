use super::{BytecodeBuilderExts, Error};
use crate::jvm::{BytecodeBuilder, FieldType, Instruction, OffsetVec, ShiftType, Width};
use crate::wasm::{ControlFrame, StackType};
use std::convert::TryFrom;
use wasmparser::{FuncValidator, FunctionBody, Operator, WasmModuleResources};

/// Context for translating a WASM function into a JVM one
pub struct FunctionTranslator<'a, B: BytecodeBuilder + Sized, R> {
    code: B,
    jvm_locals: OffsetVec<StackType>,
    wasm_validator: FuncValidator<R>,
    wasm_function: FunctionBody<'a>,
    wasm_frames: Vec<ControlFrame<B::Lbl>>,
}

impl<'a, B, R> FunctionTranslator<'a, B, R>
where
    B: BytecodeBuilderExts + Sized,
    R: WasmModuleResources,
{
    /// Visit all locals
    ///
    /// This also handles zero-initializing the locals (as is required by WASM)
    fn visit_locals(&mut self) -> Result<(), Error> {
        let locals_reader = self
            .wasm_function
            .get_locals_reader()
            .map_err(Error::WasmParser)?;
        for local in locals_reader {
            let (count, local_type) = local.map_err(Error::WasmParser)?;
            let local_type = StackType::from_general(&local_type)
                .ok_or_else(|| Error::UnsupportedStackType(local_type))?;
            for _ in 0..count {
                self.push_local(local_type)?;
            }
        }
        Ok(())
    }

    /// Push a new local onto our "stack" of locals
    fn push_local(&mut self, local_type: StackType) -> Result<(), Error> {
        let field_type = local_type.field_type();
        let next_local_idx =
            u16::try_from(self.jvm_locals.offset_len().0).map_err(|_| Error::LocalsOverflow)?;
        self.code
            .zero_local(next_local_idx, field_type)
            .map_err(Error::BytecodeGen)?;
        self.jvm_locals.push(local_type);
        Ok(())
    }

    /// Pop a local from our "stack" of locals
    fn pop_locals(&mut self) -> Result<(), Error> {
        if let Some((offset, _, local_type)) = self.jvm_locals.pop() {
            let field_type = local_type.field_type();
            self.code
                .kill_local(offset.0 as u16, field_type)
                .map_err(Error::BytecodeGen)?;
        }
        Ok(())
    }

    /// Visit all operators
    fn visit_operators(&mut self) -> Result<(), Error> {
        let op_reader = self
            .wasm_function
            .get_operators_reader()
            .map_err(Error::WasmParser)?;
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
                op_offset.map_err(Error::WasmParser)?
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
        let stack_height = self.wasm_validator.operand_stack_height();
        let (operator, offset) = operator_offset;
        self.wasm_validator
            .op(offset, &operator)
            .map_err(Error::WasmParser)?;

        match operator {
            Operator::Nop => self.code.push_instruction(Instruction::Nop),

            // Constants
            Operator::I32Const { value } => self.code.const_int(value),
            Operator::I64Const { value } => self.code.const_long(value),
            Operator::F32Const { value } => self.code.const_float(f32::from_bits(value.bits())),
            Operator::F64Const { value } => self.code.const_double(f64::from_bits(value.bits())),

            // Arithmetic
            Operator::I32Add => self.code.push_instruction(Instruction::IAdd),
            Operator::I64Add => self.code.push_instruction(Instruction::LAdd),
            Operator::F32Add => self.code.push_instruction(Instruction::FAdd),
            Operator::F64Add => self.code.push_instruction(Instruction::DAdd),
            Operator::I32Sub => self.code.push_instruction(Instruction::ISub),
            Operator::I64Sub => self.code.push_instruction(Instruction::LSub),
            Operator::F32Sub => self.code.push_instruction(Instruction::FSub),
            Operator::F64Sub => self.code.push_instruction(Instruction::DSub),
            Operator::I32Mul => self.code.push_instruction(Instruction::IMul),
            Operator::I64Mul => self.code.push_instruction(Instruction::LMul),
            Operator::F32Mul => self.code.push_instruction(Instruction::FMul),
            Operator::F64Mul => self.code.push_instruction(Instruction::DMul),
            Operator::I32DivS => self.code.push_instruction(Instruction::IDiv),
            Operator::I64DivS => self.code.push_instruction(Instruction::LDiv),
            Operator::F32Div => self.code.push_instruction(Instruction::FDiv),
            Operator::F64Div => self.code.push_instruction(Instruction::DDiv),
            Operator::I32RemS => self.code.push_instruction(Instruction::IRem),
            Operator::I64RemS => self.code.push_instruction(Instruction::LRem),
            Operator::F32Neg => self.code.push_instruction(Instruction::FNeg),
            Operator::F64Neg => self.code.push_instruction(Instruction::DNeg),

            // Bitwise
            Operator::I32And => self.code.push_instruction(Instruction::IAnd),
            Operator::I64And => self.code.push_instruction(Instruction::LAnd),
            Operator::I32Or => self.code.push_instruction(Instruction::IOr),
            Operator::I64Or => self.code.push_instruction(Instruction::LOr),
            Operator::I32Xor => self.code.push_instruction(Instruction::IXor),
            Operator::I64Xor => self.code.push_instruction(Instruction::LXor),

            // Shifts
            Operator::I32Shl => self
                .code
                .push_instruction(Instruction::ISh(ShiftType::Left)),
            Operator::I64Shl => self
                .code
                .push_instruction(Instruction::LSh(ShiftType::Left)),
            Operator::I32ShrS => self
                .code
                .push_instruction(Instruction::ISh(ShiftType::ArithmeticRight)),
            Operator::I64ShrS => self
                .code
                .push_instruction(Instruction::LSh(ShiftType::ArithmeticRight)),
            Operator::I32ShrU => self
                .code
                .push_instruction(Instruction::ISh(ShiftType::LogicalRight)),
            Operator::I64ShrU => self
                .code
                .push_instruction(Instruction::LSh(ShiftType::LogicalRight)),

            _ => todo!(),
        }
        .map_err(Error::BytecodeGen)?;

        Ok(())
    }

    //    fn visit_condition_and_operator(
    //        &mut self,
    //        condition: &Operator,
    //        operator: &Operator,
    //        offset: usize
    //    ) -> Result<(), Error> {
    //
    //    }
    //
    //    /// Checks if the operator is one which `visit_condition_and_operator` can handle
    //    fn is_test_operator(operator: &Operator)
}
