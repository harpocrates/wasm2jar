use crate::jvm::{
    BaseType, BootstrapMethodData, BranchInstruction, BytecodeBuilder, ClassData, ConstantData,
    EqComparison, Error, FieldData, FieldType, Instruction, InvokeDynamicData, InvokeType,
    MethodAccessFlags, MethodData, MethodDescriptor, OrdComparison, RefType, UnqualifiedName,
    Width,
};
use std::borrow::Cow;
use std::ops::Not;

pub trait CodeBuilderExts<'a, 'g> {
    /// Zero initialize a local variable
    fn zero_local(
        &mut self,
        offset: u16,
        field_type: FieldType<&'g ClassData<'g>>,
    ) -> Result<(), Error>;

    /// Push a null of a specific value to the stack
    fn const_null(&mut self, ref_type: RefType<&'g ClassData<'g>>) -> Result<(), Error>;

    /// Push a constant string to the stack
    fn const_string(&mut self, string: impl Into<Cow<'static, str>>) -> Result<(), Error>;

    /// Get a local at a particular offset
    fn get_local(
        &mut self,
        offset: u16,
        field_type: &FieldType<&'g ClassData<'g>>,
    ) -> Result<(), Error>;

    /// Set a local at a particular offset
    fn set_local(
        &mut self,
        offset: u16,
        field_type: &FieldType<&'g ClassData<'g>>,
    ) -> Result<(), Error>;

    /// Kill a local variable
    fn kill_local(
        &mut self,
        offset: u16,
        field_type: FieldType<&'g ClassData<'g>>,
    ) -> Result<(), Error>;

    /// Return from the function
    fn return_(
        &mut self,
        field_type_opt: Option<FieldType<&'g ClassData<'g>>>,
    ) -> Result<(), Error>;

    /// Push an integer constant onto the stack
    fn const_int(&mut self, integer: i32) -> Result<(), Error>;

    /// Push a long constant onto the stack
    ///
    /// In a lot of cases, this will fallback to some `int` instructions followed by a conversion.
    /// This choice is motivated by a desire to avoid filling the constant pool as well as to
    /// reduce the (serialized) length of the bytecode produced. Consider the alternatives for
    /// pushing the `long` 2 onto the stack:
    ///
    ///   * `ldc_w 2` will be 3 bytes in the method body and two slots in the constant pool
    ///   * `iconst2 i2l` will be 2 bytes in the method body and no slots in the constant pool
    ///
    fn const_long(&mut self, long: i64) -> Result<(), Error>;

    /// Push a float constant onto the stack
    fn const_float(&mut self, float: f32) -> Result<(), Error>;

    /// Push a double constant onto the stack
    fn const_double(&mut self, double: f64) -> Result<(), Error>;

    /// Push a value of type `java/lang/Class` onto the stack
    fn const_class(&mut self, ty: FieldType<&'g ClassData<'g>>) -> Result<(), Error>;

    /// Push a value of type `java/lang/invoke/MethodHandle` based on a method onto the stack
    fn const_methodhandle(&mut self, method: &'g MethodData<'g>) -> Result<(), Error>;

    /// Pop the top of the stack, accounting for the different possible type widths
    fn pop(&mut self) -> Result<(), Error>;

    /// Duplicate the top of the stack, accounting for the different possible type widths
    fn dup(&mut self) -> Result<(), Error>;

    /// Push 1 or 0 onto the stack depending if the condition holds or not
    fn condition(&mut self, condition: &BranchCond) -> Result<(), Error>;

    /// Invoke a method
    fn invoke(&mut self, method: &'g MethodData<'g>) -> Result<(), Error>;

    /// Invoke a method explicitly specifying the invocation type
    fn invoke_explicit(
        &mut self,
        invoke_typ: InvokeType,
        method: &'g MethodData<'g>,
    ) -> Result<(), Error>;

    /// Invoke dynamic
    fn invoke_dynamic(
        &mut self,
        bootstrap: &'g BootstrapMethodData<'g>,
        method_name: UnqualifiedName,
        descriptor: MethodDescriptor<&'g ClassData<'g>>,
    ) -> Result<(), Error>;

    /// Invoke `MethodHandle.invokeExact` using the specified method descriptor
    fn invoke_invoke_exact(
        &mut self,
        descriptor: MethodDescriptor<&'g ClassData<'g>>,
    ) -> Result<(), Error>;

    /// Get/put a field
    fn access_field(
        &mut self,
        field: &'g FieldData<'g>,
        access_mode: AccessMode,
    ) -> Result<(), Error>;

    /// Construct a new object of the given type
    fn new(&mut self, class: &'g ClassData<'g>) -> Result<(), Error>;

    /// Construct a new array of the given type
    fn new_ref_array(&mut self, elem_type: RefType<&'g ClassData<'g>>) -> Result<(), Error>;
}

impl<'a, 'g> CodeBuilderExts<'a, 'g> for BytecodeBuilder<'a, 'g> {
    fn zero_local(
        &mut self,
        offset: u16,
        field_type: FieldType<&'g ClassData<'g>>,
    ) -> Result<(), Error> {
        match field_type {
            FieldType::Base(
                BaseType::Int
                | BaseType::Char
                | BaseType::Short
                | BaseType::Byte
                | BaseType::Boolean,
            ) => {
                self.push_instruction(Instruction::IConst0)?;
                self.push_instruction(Instruction::IStore(offset))?;
            }
            FieldType::Base(BaseType::Float) => {
                self.push_instruction(Instruction::FConst0)?;
                self.push_instruction(Instruction::FStore(offset))?;
            }
            FieldType::Base(BaseType::Long) => {
                self.push_instruction(Instruction::LConst0)?;
                self.push_instruction(Instruction::LStore(offset))?;
            }
            FieldType::Base(BaseType::Double) => {
                self.push_instruction(Instruction::DConst0)?;
                self.push_instruction(Instruction::DStore(offset))?;
            }
            FieldType::Ref(ref_type) => {
                self.push_instruction(Instruction::AConstNull)?;
                self.push_instruction(Instruction::AHint(ref_type))?;
                self.push_instruction(Instruction::AStore(offset))?;
            }
        };
        Ok(())
    }

    /// Push a null of a specific value to the stack
    fn const_null(&mut self, ref_type: RefType<&'g ClassData<'g>>) -> Result<(), Error> {
        self.push_instruction(Instruction::AConstNull)?;
        self.push_instruction(Instruction::AHint(ref_type))?;
        Ok(())
    }

    /// Push a constant string to the stack
    fn const_string(&mut self, string: impl Into<Cow<'static, str>>) -> Result<(), Error> {
        let constant = ConstantData::String(string.into());
        self.push_instruction(Instruction::Ldc(constant))?;
        Ok(())
    }

    /// Get a local at a particular offset
    fn get_local(
        &mut self,
        offset: u16,
        field_type: &FieldType<&'g ClassData<'g>>,
    ) -> Result<(), Error> {
        let insn = match *field_type {
            FieldType::Base(
                BaseType::Int
                | BaseType::Char
                | BaseType::Short
                | BaseType::Byte
                | BaseType::Boolean,
            ) => Instruction::ILoad(offset),
            FieldType::Base(BaseType::Float) => Instruction::FLoad(offset),
            FieldType::Base(BaseType::Long) => Instruction::LLoad(offset),
            FieldType::Base(BaseType::Double) => Instruction::DLoad(offset),
            FieldType::Ref(_) => Instruction::ALoad(offset),
        };
        self.push_instruction(insn)
    }

    /// Set a local at a particular offset
    fn set_local(
        &mut self,
        offset: u16,
        field_type: &FieldType<&'g ClassData<'g>>,
    ) -> Result<(), Error> {
        let insn = match *field_type {
            FieldType::Base(
                BaseType::Int
                | BaseType::Char
                | BaseType::Short
                | BaseType::Byte
                | BaseType::Boolean,
            ) => Instruction::IStore(offset),
            FieldType::Base(BaseType::Float) => Instruction::FStore(offset),
            FieldType::Base(BaseType::Long) => Instruction::LStore(offset),
            FieldType::Base(BaseType::Double) => Instruction::DStore(offset),
            FieldType::Ref(_) => Instruction::AStore(offset),
        };
        self.push_instruction(insn)
    }

    /// Kill a local variable
    fn kill_local(
        &mut self,
        offset: u16,
        field_type: FieldType<&'g ClassData<'g>>,
    ) -> Result<(), Error> {
        let insn = match field_type {
            FieldType::Base(
                BaseType::Int
                | BaseType::Char
                | BaseType::Short
                | BaseType::Byte
                | BaseType::Boolean,
            ) => Instruction::IKill(offset),
            FieldType::Base(BaseType::Float) => Instruction::FKill(offset),
            FieldType::Base(BaseType::Long) => Instruction::LKill(offset),
            FieldType::Base(BaseType::Double) => Instruction::DKill(offset),
            FieldType::Ref(_) => Instruction::AKill(offset),
        };
        self.push_instruction(insn)
    }

    /// Return from the function
    fn return_(
        &mut self,
        field_type_opt: Option<FieldType<&'g ClassData<'g>>>,
    ) -> Result<(), Error> {
        let insn = match field_type_opt {
            None => BranchInstruction::Return,
            Some(FieldType::Base(
                BaseType::Int
                | BaseType::Char
                | BaseType::Short
                | BaseType::Byte
                | BaseType::Boolean,
            )) => BranchInstruction::IReturn,
            Some(FieldType::Base(BaseType::Float)) => BranchInstruction::FReturn,
            Some(FieldType::Base(BaseType::Long)) => BranchInstruction::LReturn,
            Some(FieldType::Base(BaseType::Double)) => BranchInstruction::DReturn,
            Some(FieldType::Ref(_)) => BranchInstruction::AReturn,
        };
        self.push_branch_instruction(insn)
    }

    /// Push an integer constant onto the stack
    fn const_int(&mut self, integer: i32) -> Result<(), Error> {
        let insn = match integer {
            -1 => Instruction::IConstM1,
            0 => Instruction::IConst0,
            1 => Instruction::IConst1,
            2 => Instruction::IConst2,
            3 => Instruction::IConst3,
            4 => Instruction::IConst4,
            5 => Instruction::IConst5,
            -128..=127 => Instruction::BiPush(integer as i8),
            -32768..=32767 => Instruction::SiPush(integer as i16),
            _ => Instruction::Ldc(ConstantData::Integer(integer)),
        };
        self.push_instruction(insn)?;
        Ok(())
    }

    /// Push a long constant onto the stack
    ///
    /// In a lot of cases, this will fallback to some `int` instructions followed by a conversion.
    /// This choice is motivated by a desire to avoid filling the constant pool as well as to
    /// reduce the (serialized) length of the bytecode produced. Consider the alternatives for
    /// pushing the `long` 2 onto the stack:
    ///
    ///   * `ldc_w 2` will be 3 bytes in the method body and two slots in the constant pool
    ///   * `iconst2 i2l` will be 2 bytes in the method body and no slots in the constant pool
    ///
    fn const_long(&mut self, long: i64) -> Result<(), Error> {
        let (insn, needs_int_to_long_conversion) = match long {
            -1 => (Instruction::IConstM1, true),
            0 => (Instruction::LConst0, false),
            1 => (Instruction::LConst1, false),
            2 => (Instruction::IConst2, true),
            3 => (Instruction::IConst3, true),
            4 => (Instruction::IConst4, true),
            5 => (Instruction::IConst5, true),
            -128..=127 => (Instruction::BiPush(long as i8), true),
            -32768..=32767 => (Instruction::SiPush(long as i16), true),
            _ => (Instruction::Ldc2(ConstantData::Long(long)), false),
        };
        self.push_instruction(insn)?;
        if needs_int_to_long_conversion {
            self.push_instruction(Instruction::I2L)?;
        }
        Ok(())
    }

    /// Push a float constant onto the stack
    fn const_float(&mut self, float: f32) -> Result<(), Error> {
        let (insn, needs_int_to_float_conversion) = match float {
            f if f == -1.0 => (Instruction::IConstM1, true),
            f if f == 0.0 && f.is_sign_positive() => (Instruction::FConst0, false),
            f if f == 1.0 => (Instruction::FConst1, false),
            f if f == 2.0 => (Instruction::FConst2, false),
            f if f == 3.0 => (Instruction::IConst3, true),
            f if f == 4.0 => (Instruction::IConst4, true),
            f if f == 5.0 => (Instruction::IConst5, true),
            _ => (Instruction::Ldc(ConstantData::Float(float)), false),
        };
        self.push_instruction(insn)?;
        if needs_int_to_float_conversion {
            self.push_instruction(Instruction::I2F)?;
        }
        Ok(())
    }

    /// Push a double constant onto the stack
    fn const_double(&mut self, double: f64) -> Result<(), Error> {
        let (insn, needs_int_to_double_conversion) = match double {
            f if f == -1.0 => (Instruction::IConstM1, true),
            f if f == 0.0 && f.is_sign_positive() => (Instruction::DConst0, false),
            f if f == 1.0 => (Instruction::DConst1, false),
            f if f == 2.0 => (Instruction::IConst2, true),
            f if f == 3.0 => (Instruction::IConst3, true),
            f if f == 4.0 => (Instruction::IConst4, true),
            f if f == 5.0 => (Instruction::IConst5, true),
            _ => (Instruction::Ldc2(ConstantData::Double(double)), false),
        };
        self.push_instruction(insn)?;
        if needs_int_to_double_conversion {
            self.push_instruction(Instruction::I2D)?;
        }
        Ok(())
    }

    /// Push a constant of type `java.lang.Class` onto the stack
    fn const_class(&mut self, ty: FieldType<&'g ClassData<'g>>) -> Result<(), Error> {
        match ty {
            FieldType::Base(base_type) => {
                let type_static_field = match base_type {
                    BaseType::Int => self.java.members.lang.integer.r#type,
                    BaseType::Long => self.java.members.lang.long.r#type,
                    BaseType::Float => self.java.members.lang.long.r#type,
                    BaseType::Double => self.java.members.lang.double.r#type,
                    BaseType::Boolean => self.java.members.lang.boolean.r#type,
                    other => todo!("const_class for {:?}", other),
                };
                self.access_field(type_static_field, AccessMode::Read)
            }
            FieldType::Ref(ref_type) => {
                self.push_instruction(Instruction::Ldc(ConstantData::Class(ref_type)))
            }
        }
    }

    /// Push a constant of type `java.lang.invoke.MethodHandle` onto the stack
    fn const_methodhandle(&mut self, method: &'g MethodData<'g>) -> Result<(), Error> {
        self.push_instruction(Instruction::Ldc(ConstantData::MethodHandle(method)))
    }

    /// Pop the top of the stack, accounting for the different possible type widths
    fn pop(&mut self) -> Result<(), Error> {
        if let Some(frame) = self.current_frame() {
            let wide_typ = frame
                .stack
                .iter()
                .last()
                .map_or(false, |(_, _, t)| t.width() == 2);
            let insn = if wide_typ {
                Instruction::Pop2
            } else {
                Instruction::Pop
            };
            self.push_instruction(insn)?;
        }
        Ok(())
    }

    /// Duplicate the top of the stack, accounting for the different possible type widths
    fn dup(&mut self) -> Result<(), Error> {
        if let Some(frame) = self.current_frame() {
            let wide_typ = frame
                .stack
                .iter()
                .last()
                .map_or(false, |(_, _, t)| t.width() == 2);
            let insn = if wide_typ {
                Instruction::Dup2
            } else {
                Instruction::Dup
            };
            self.push_instruction(insn)?;
        }
        Ok(())
    }

    /// Push 1 or 0 onto the stack depending if the condition holds or not
    fn condition(&mut self, condition: &BranchCond) -> Result<(), Error> {
        let els = self.fresh_label();
        let end = self.fresh_label();

        self.push_branch_instruction(condition.into_instruction(els, ()))?;
        self.push_instruction(Instruction::IConst0)?;
        self.push_branch_instruction(BranchInstruction::Goto(end))?;
        self.place_label(els)?;
        self.push_instruction(Instruction::IConst1)?;
        self.place_label(end)?;

        Ok(())
    }

    /// Invoke a method
    fn invoke(&mut self, method: &'g MethodData<'g>) -> Result<(), Error> {
        self.invoke_explicit(method.infer_invoke_type(), method)
    }

    /// Invoke a method explicitly specifying the invocation type
    fn invoke_explicit(
        &mut self,
        invoke_typ: InvokeType,
        method: &'g MethodData<'g>,
    ) -> Result<(), Error> {
        self.push_instruction(Instruction::Invoke(invoke_typ, method))
    }

    fn invoke_dynamic(
        &mut self,
        bootstrap: &'g BootstrapMethodData<'g>,
        method_name: UnqualifiedName,
        descriptor: MethodDescriptor<&'g ClassData<'g>>,
    ) -> Result<(), Error> {
        let indy = InvokeDynamicData {
            name: method_name,
            descriptor,
            bootstrap,
        };
        self.push_instruction(Instruction::InvokeDynamic(indy))
    }

    fn invoke_invoke_exact(
        &mut self,
        descriptor: MethodDescriptor<&'g ClassData<'g>>,
    ) -> Result<(), Error> {
        let method = self.class_graph.add_method(MethodData {
            class: self.java.classes.lang.invoke.method_handle,
            name: UnqualifiedName::INVOKEEXACT,
            access_flags: MethodAccessFlags::PUBLIC,
            descriptor,
        });
        self.push_instruction(Instruction::Invoke(InvokeType::Virtual, method))
    }

    fn access_field(
        &mut self,
        field: &'g FieldData<'g>,
        access_mode: AccessMode,
    ) -> Result<(), Error> {
        self.push_instruction(match (field.is_static(), access_mode) {
            (true, AccessMode::Read) => Instruction::GetStatic(field),
            (true, AccessMode::Write) => Instruction::PutStatic(field),
            (false, AccessMode::Read) => Instruction::GetField(field),
            (false, AccessMode::Write) => Instruction::PutField(field),
        })
    }

    fn new(&mut self, class: &'g ClassData<'g>) -> Result<(), Error> {
        self.push_instruction(Instruction::New(RefType::Object(class)))
    }

    /// Construct a new array of the given type
    fn new_ref_array(&mut self, elem_type: RefType<&'g ClassData<'g>>) -> Result<(), Error> {
        self.push_instruction(Instruction::ANewArray(elem_type))
    }
}

/// Conditional branch condition
pub enum BranchCond {
    If(OrdComparison),
    IfICmp(OrdComparison),
    IfACmp(EqComparison),
    IfNull(EqComparison),
}

impl Not for BranchCond {
    type Output = Self;

    fn not(self) -> Self::Output {
        match self {
            BranchCond::If(ord) => BranchCond::If(!ord),
            BranchCond::IfICmp(ord) => BranchCond::IfICmp(!ord),
            BranchCond::IfACmp(eq) => BranchCond::IfACmp(!eq),
            BranchCond::IfNull(eq) => BranchCond::IfNull(!eq),
        }
    }
}

impl BranchCond {
    pub fn into_instruction<Lbl, LblWide, LblNext>(
        &self,
        jump_lbl: Lbl,
        fallthrough_lbl: LblNext,
    ) -> BranchInstruction<Lbl, LblWide, LblNext> {
        match self {
            BranchCond::If(ord) => BranchInstruction::If(*ord, jump_lbl, fallthrough_lbl),
            BranchCond::IfICmp(ord) => BranchInstruction::IfICmp(*ord, jump_lbl, fallthrough_lbl),
            BranchCond::IfACmp(eq) => BranchInstruction::IfACmp(*eq, jump_lbl, fallthrough_lbl),
            BranchCond::IfNull(eq) => BranchInstruction::IfNull(*eq, jump_lbl, fallthrough_lbl),
        }
    }
}

pub enum AccessMode {
    Read,
    Write,
}
