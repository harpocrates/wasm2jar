use super::{BranchCond, BytecodeBuilderExts, Error};
use crate::jvm::{
    BranchInstruction, BytecodeBuilder, EqComparison, Instruction, Offset, OffsetVec, OrdComparison,
};
use crate::wasm::{ref_type_from_general, ControlFrame, FunctionType, StackType};
use std::convert::TryFrom;
use std::ops::Not;
use wasmparser::{
    FuncValidator, FunctionBody, Operator, TypeOrFuncType, WasmFeatures, WasmModuleResources,
};

/// Context for translating a WASM function into a JVM one
pub struct FunctionTranslator<'a, B: BytecodeBuilder + Sized, R> {
    jvm_code: B,
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
    pub fn new(
        jvm_code: B,
        wasm_function: FunctionBody<'a>,
        wasm_ty: u32,
        wasm_offset: usize,
        wasm_resources: R,
        wasm_features: &WasmFeatures,
    ) -> Result<FunctionTranslator<'a, B, R>, Error> {
        let wasm_validator =
            FuncValidator::new(wasm_ty, wasm_offset, wasm_resources, wasm_features)?;

        Ok(FunctionTranslator {
            jvm_code,
            jvm_locals: OffsetVec::new(), // TODO: include `this`
            wasm_validator,
            wasm_function,
            wasm_frames: vec![],
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
        self.jvm_code.zero_local(next_local_idx, field_type)?;
        self.jvm_locals.push(local_type);
        Ok(())
    }

    /// Pop a local from our "stack" of locals
    fn pop_locals(&mut self) -> Result<(), Error> {
        if let Some((offset, _, local_type)) = self.jvm_locals.pop() {
            let field_type = local_type.field_type();
            self.jvm_code.kill_local(offset.0 as u16, field_type)?;
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
        use crate::jvm::Instruction::*;
        use crate::jvm::ShiftType::*;

        let (operator, offset) = operator_offset;
        let next_op = next_operator_offset;
        self.wasm_validator.op(offset, &operator)?;

        match operator {
            Operator::Nop => self.jvm_code.push_instruction(Instruction::Nop)?,

            // Control flow
            Operator::If { ty } => self.visit_if(ty, BranchCond::If(OrdComparison::EQ))?,
            Operator::Else => self.visit_else()?,
            Operator::End => self.visit_end()?,

            // Constants
            Operator::I32Const { value } => self.jvm_code.const_int(value)?,
            Operator::I64Const { value } => self.jvm_code.const_long(value)?,
            Operator::F32Const { value } => {
                self.jvm_code.const_float(f32::from_bits(value.bits()))?
            }
            Operator::F64Const { value } => {
                self.jvm_code.const_double(f64::from_bits(value.bits()))?
            }
            Operator::RefNull { ty } => {
                let ref_type = ref_type_from_general(&ty);
                self.jvm_code
                    .const_null(ref_type.ok_or_else(|| Error::UnsupportedReferenceType(ty))?)?;
            }

            // Arithmetic
            Operator::I32Add => self.jvm_code.push_instruction(IAdd)?,
            Operator::I64Add => self.jvm_code.push_instruction(LAdd)?,
            Operator::F32Add => self.jvm_code.push_instruction(FAdd)?,
            Operator::F64Add => self.jvm_code.push_instruction(DAdd)?,
            Operator::I32Sub => self.jvm_code.push_instruction(ISub)?,
            Operator::I64Sub => self.jvm_code.push_instruction(LSub)?,
            Operator::F32Sub => self.jvm_code.push_instruction(FSub)?,
            Operator::F64Sub => self.jvm_code.push_instruction(DSub)?,
            Operator::I32Mul => self.jvm_code.push_instruction(IMul)?,
            Operator::I64Mul => self.jvm_code.push_instruction(LMul)?,
            Operator::F32Mul => self.jvm_code.push_instruction(FMul)?,
            Operator::F64Mul => self.jvm_code.push_instruction(DMul)?,
            Operator::I32DivS => self.jvm_code.push_instruction(IDiv)?,
            Operator::I64DivS => self.jvm_code.push_instruction(LDiv)?,
            Operator::F32Div => self.jvm_code.push_instruction(FDiv)?,
            Operator::F64Div => self.jvm_code.push_instruction(DDiv)?,
            Operator::I32RemS => self.jvm_code.push_instruction(IRem)?,
            Operator::I64RemS => self.jvm_code.push_instruction(LRem)?,
            Operator::F32Neg => self.jvm_code.push_instruction(FNeg)?,
            Operator::F64Neg => self.jvm_code.push_instruction(DNeg)?,

            // Bitwise
            Operator::I32And => self.jvm_code.push_instruction(IAnd)?,
            Operator::I64And => self.jvm_code.push_instruction(LAnd)?,
            Operator::I32Or => self.jvm_code.push_instruction(IOr)?,
            Operator::I64Or => self.jvm_code.push_instruction(LOr)?,
            Operator::I32Xor => self.jvm_code.push_instruction(IXor)?,
            Operator::I64Xor => self.jvm_code.push_instruction(LXor)?,

            // Shifts
            Operator::I32Shl => self.jvm_code.push_instruction(ISh(Left))?,
            Operator::I64Shl => self.jvm_code.push_instruction(LSh(Left))?,
            Operator::I32ShrS => self.jvm_code.push_instruction(ISh(ArithmeticRight))?,
            Operator::I64ShrS => self.jvm_code.push_instruction(LSh(ArithmeticRight))?,
            Operator::I32ShrU => self.jvm_code.push_instruction(ISh(LogicalRight))?,
            Operator::I64ShrU => self.jvm_code.push_instruction(LSh(LogicalRight))?,

            // Locals
            Operator::LocalGet { local_index } => {
                let (Offset(off), stack_type) = self
                    .jvm_locals
                    .get_index(local_index as usize)
                    .expect("missing local");
                self.jvm_code
                    .get_local(off as u16, &stack_type.field_type())?;
            }
            Operator::LocalSet { local_index } => {
                let (Offset(off), stack_type) = self
                    .jvm_locals
                    .get_index(local_index as usize)
                    .expect("missing local");
                self.jvm_code
                    .set_local(off as u16, &stack_type.field_type())?;
            }
            Operator::LocalTee { local_index } => {
                let (Offset(off), stack_type) = self
                    .jvm_locals
                    .get_index(local_index as usize)
                    .expect("missing local");
                self.jvm_code.dup()?;
                self.jvm_code
                    .set_local(off as u16, &stack_type.field_type())?;
            }

            // Conditions
            Operator::I32Eqz => self.visit_cond(BranchCond::If(OrdComparison::EQ), next_op)?,
            Operator::RefIsNull => {
                self.visit_cond(BranchCond::IfNull(EqComparison::EQ), next_op)?
            }
            Operator::I32Eq => self.visit_cond(BranchCond::IfICmp(OrdComparison::EQ), next_op)?,
            Operator::I32Ne => self.visit_cond(BranchCond::IfICmp(OrdComparison::NE), next_op)?,
            Operator::I32LtS => self.visit_cond(BranchCond::IfICmp(OrdComparison::LT), next_op)?,
            Operator::I32GtS => self.visit_cond(BranchCond::IfICmp(OrdComparison::GT), next_op)?,
            Operator::I32GeS => self.visit_cond(BranchCond::IfICmp(OrdComparison::GE), next_op)?,
            Operator::I32LeS => self.visit_cond(BranchCond::IfICmp(OrdComparison::LE), next_op)?,

            Operator::Drop => self.jvm_code.pop()?,

            _ => todo!(),
        }

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

            other => {
                self.jvm_code.condition(&condition)?;
                *next_operator_offset = other;
            }
        }

        Ok(())
    }

    /// Visit the start of an `if` block
    fn visit_if(&mut self, ty: TypeOrFuncType, condition: BranchCond) -> Result<(), Error> {
        let ty = self.block_type(ty)?;

        #[cfg(debug_assertions)]
        self.assert_top_stack(&ty.inputs);

        let base_stack_height = self.wasm_validator.operand_stack_height() - ty.inputs.len() as u32;
        let else_block = self.jvm_code.fresh_label();
        let end_block = self.jvm_code.fresh_label();
        let return_values = ty.outputs;

        self.wasm_frames.push(ControlFrame::If {
            else_block,
            end_block,
            return_values,
            base_stack_height,
        });
        self.jvm_code
            .push_branch_instruction(condition.not().into_instruction(else_block, ()))?;

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

    /// Visit the end of a block
    fn visit_end(&mut self) -> Result<(), Error> {
        Ok(match self.wasm_frames.pop() {
            // all functions end with one final `End`
            // TODO: review this
            None => (),

            // at the end of all control flow blocks, we just fallthrough
            Some(control_frame) => {
                self.jvm_code.place_label(control_frame.end_label())?;
                debug_assert_eq!(
                    control_frame.base_stack_height() + control_frame.return_values().len() as u32,
                    self.wasm_validator.operand_stack_height(),
                    "Stack does not have the expected height",
                );
            }
        })
    }

    /// Convert a block type into a function type
    pub fn block_type(&self, type_: TypeOrFuncType) -> Result<FunctionType, Error> {
        match type_ {
            TypeOrFuncType::Type(typ) => {
                let output_ty = StackType::from_general(&typ)
                    .ok_or_else(|| Error::UnsupportedReferenceType(typ))?;
                Ok(FunctionType {
                    inputs: vec![],
                    outputs: vec![output_ty],
                })
            }
            TypeOrFuncType::FuncType(type_idx) => {
                let func_ty = self
                    .wasm_validator
                    .resources()
                    .func_type_at(type_idx)
                    .ok_or_else(|| Error::UnsupportedFunctionType(type_))?;
                FunctionType::from_general(func_ty)
                    .ok_or_else(|| Error::UnsupportedFunctionType(type_))
            }
        }
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
