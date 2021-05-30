use crate::jvm::{
    BinaryName, BranchInstruction, ClassConstantIndex, CodeBuilder, Descriptor, EqComparison,
    Error, FieldType, Instruction, InvokeType, MethodDescriptor, Name, OrdComparison, RefType,
    UnqualifiedName, Width,
};
use std::ops::Not;

pub trait CodeBuilderExts: CodeBuilder<Error> {
    /// Zero initialize a local variable
    fn zero_local(&mut self, offset: u16, field_type: FieldType) -> Result<(), Error> {
        match field_type {
            FieldType::INT
            | FieldType::CHAR
            | FieldType::SHORT
            | FieldType::BYTE
            | FieldType::BOOLEAN => {
                self.push_instruction(Instruction::IConst0)?;
                self.push_instruction(Instruction::IStore(offset))?;
            }
            FieldType::FLOAT => {
                self.push_instruction(Instruction::FConst0)?;
                self.push_instruction(Instruction::FStore(offset))?;
            }
            FieldType::LONG => {
                self.push_instruction(Instruction::LConst0)?;
                self.push_instruction(Instruction::LStore(offset))?;
            }
            FieldType::DOUBLE => {
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
    fn const_null(&mut self, ref_type: RefType) -> Result<(), Error> {
        self.push_instruction(Instruction::AConstNull)?;
        self.push_instruction(Instruction::AHint(ref_type))?;
        Ok(())
    }

    /// Push a constant string to the stack
    fn const_string(&mut self, string: &'static str) -> Result<(), Error> {
        let str_idx = {
            let mut constants = self.constants();
            let utf8_idx = constants.get_utf8(string)?;
            constants.get_string(utf8_idx)?
        };
        self.push_instruction(Instruction::Ldc(str_idx.into()))?;
        Ok(())
    }

    /// Get a local at a particular offset
    fn get_local(&mut self, offset: u16, field_type: &FieldType) -> Result<(), Error> {
        let insn = match *field_type {
            FieldType::INT
            | FieldType::CHAR
            | FieldType::SHORT
            | FieldType::BYTE
            | FieldType::BOOLEAN => Instruction::ILoad(offset),
            FieldType::FLOAT => Instruction::FLoad(offset),
            FieldType::LONG => Instruction::LLoad(offset),
            FieldType::DOUBLE => Instruction::DLoad(offset),
            FieldType::Ref(_) => Instruction::ALoad(offset),
        };
        self.push_instruction(insn)
    }

    /// Set a local at a particular offset
    fn set_local(&mut self, offset: u16, field_type: &FieldType) -> Result<(), Error> {
        let insn = match *field_type {
            FieldType::INT
            | FieldType::CHAR
            | FieldType::SHORT
            | FieldType::BYTE
            | FieldType::BOOLEAN => Instruction::IStore(offset),
            FieldType::FLOAT => Instruction::FStore(offset),
            FieldType::LONG => Instruction::LStore(offset),
            FieldType::DOUBLE => Instruction::DStore(offset),
            FieldType::Ref(_) => Instruction::AStore(offset),
        };
        self.push_instruction(insn)
    }

    /// Kill a local variable
    fn kill_local(&mut self, offset: u16, field_type: FieldType) -> Result<(), Error> {
        let insn = match field_type {
            FieldType::INT
            | FieldType::CHAR
            | FieldType::SHORT
            | FieldType::BYTE
            | FieldType::BOOLEAN => Instruction::IKill(offset),
            FieldType::FLOAT => Instruction::FKill(offset),
            FieldType::LONG => Instruction::LKill(offset),
            FieldType::DOUBLE => Instruction::DKill(offset),
            FieldType::Ref(_) => Instruction::AKill(offset),
        };
        self.push_instruction(insn)
    }

    /// Return from the function
    fn return_(&mut self, field_type_opt: Option<FieldType>) -> Result<(), Error> {
        let insn = match field_type_opt {
            None => BranchInstruction::Return,
            Some(FieldType::INT)
            | Some(FieldType::CHAR)
            | Some(FieldType::SHORT)
            | Some(FieldType::BYTE)
            | Some(FieldType::BOOLEAN) => BranchInstruction::IReturn,
            Some(FieldType::FLOAT) => BranchInstruction::FReturn,
            Some(FieldType::LONG) => BranchInstruction::LReturn,
            Some(FieldType::DOUBLE) => BranchInstruction::DReturn,
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
            _ => Instruction::Ldc(self.constants().get_integer(integer)?),
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
            _ => (Instruction::Ldc2(self.constants().get_long(long)?), false),
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
            _ => (Instruction::Ldc(self.constants().get_float(float)?), false),
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
            _ => (
                Instruction::Ldc2(self.constants().get_double(double)?),
                false,
            ),
        };
        self.push_instruction(insn)?;
        if needs_int_to_double_conversion {
            self.push_instruction(Instruction::I2D)?;
        }
        Ok(())
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
    ///
    /// There cannot be any ambiguity about which method this is when using this helper (otherwise
    /// an `Error::AmbiguousMethod` will be returned).
    fn invoke(
        &mut self,
        class_name: &BinaryName,
        method_name: &UnqualifiedName,
    ) -> Result<(), Error> {
        let class_name = class_name.into();
        let method_name = method_name.into();

        // Query the class graph for the descriptor
        let (invoke_typ, descriptor): (InvokeType, MethodDescriptor) = {
            let class_graph = self.class_graph();
            let class = class_graph
                .classes
                .get(class_name)
                .ok_or_else(|| Error::MissingClass(class_name.clone()))?;
            let is_interface = class.is_interface;
            let mut method_overloads = class
                .methods
                .get(method_name)
                .ok_or_else(|| Error::MissingMember(method_name.clone()))?
                .iter()
                .map(|(desc, is_static)| {
                    let typ = if *is_static {
                        InvokeType::Static
                    } else if method_name == &UnqualifiedName::INIT
                        || method_name == &UnqualifiedName::CLINIT
                    {
                        InvokeType::Special
                    } else if is_interface {
                        let n = desc.parameter_length(true) as u8;
                        InvokeType::Interface(n)
                    } else {
                        InvokeType::Virtual
                    };
                    (typ, desc.clone())
                })
                .collect::<Vec<_>>();

            if method_overloads.len() == 1 {
                method_overloads.pop().unwrap()
            } else {
                let mut alts = String::new();
                for (_, alt) in &method_overloads {
                    if !alts.is_empty() {
                        alts.push_str(", ");
                    }
                    alts.push_str(&alt.render());
                }
                log::error!(
                    "Ambiguous overloads for {:?}.{:?}: {}",
                    class_name,
                    method_name,
                    alts
                );
                return Err(Error::AmbiguousMethod(
                    class_name.clone(),
                    method_name.clone(),
                ));
            }
        };

        self.invoke_explicit(invoke_typ, class_name, method_name, &descriptor)
    }

    /// Invoke a method explicitly specifying the invocation type and descriptor
    fn invoke_explicit(
        &mut self,
        invoke_typ: InvokeType,
        class_name: &BinaryName,
        method_name: &UnqualifiedName,
        descriptor: &MethodDescriptor,
    ) -> Result<(), Error> {
        let descriptor = descriptor.render();
        let is_interface = if let InvokeType::Interface(_) = invoke_typ {
            true
        } else {
            false
        };

        let method_ref = {
            let mut constants = self.constants();
            let class_utf8 = constants.get_utf8(class_name.as_str())?;
            let class_idx = constants.get_class(class_utf8)?;
            let method_utf8 = constants.get_utf8(method_name.as_str())?;
            let desc_utf8 = constants.get_utf8(descriptor)?;
            let name_and_type_idx = constants.get_name_and_type(method_utf8, desc_utf8)?;
            constants.get_method_ref(class_idx, name_and_type_idx, is_interface)?
        };

        self.push_instruction(Instruction::Invoke(invoke_typ, method_ref))
    }

    /// Get/put a field
    fn access_field(
        &mut self,
        class_name: &BinaryName,
        field_name: &UnqualifiedName,
        access_mode: AccessMode,
    ) -> Result<(), Error> {
        // Query the class graph for the descriptor
        let (is_static, descriptor): (bool, FieldType) = {
            let class_graph = self.class_graph();
            class_graph
                .classes
                .get(class_name)
                .ok_or_else(|| Error::MissingClass(class_name.clone()))?
                .fields
                .get(field_name)
                .ok_or_else(|| Error::MissingMember(field_name.clone()))?
                .clone()
        };

        self.access_field_explicit(is_static, access_mode, class_name, field_name, &descriptor)
    }

    /// Get a field explicitly specifying the descriptor
    fn access_field_explicit(
        &mut self,
        is_static: bool,
        access_mode: AccessMode,
        class_name: &BinaryName,
        field_name: &UnqualifiedName,
        descriptor: &FieldType,
    ) -> Result<(), Error> {
        let descriptor = descriptor.render();

        let field_ref = {
            let mut constants = self.constants();
            let class_utf8 = constants.get_utf8(class_name.as_str())?;
            let class_idx = constants.get_class(class_utf8)?;
            let field_utf8 = constants.get_utf8(field_name.as_str())?;
            let desc_utf8 = constants.get_utf8(descriptor)?;
            let name_and_type_idx = constants.get_name_and_type(field_utf8, desc_utf8)?;
            constants.get_field_ref(class_idx, name_and_type_idx)?
        };

        self.push_instruction(match (is_static, access_mode) {
            (true, AccessMode::Read) => Instruction::GetStatic(field_ref),
            (true, AccessMode::Write) => Instruction::PutStatic(field_ref),
            (false, AccessMode::Read) => Instruction::GetField(field_ref),
            (false, AccessMode::Write) => Instruction::PutField(field_ref),
        })
    }

    /// Construct a new array of the given type
    fn new_ref_array(&mut self, elem_type: &RefType) -> Result<(), Error> {
        let class_idx = self.get_class_idx(elem_type)?;
        self.push_instruction(Instruction::ANewArray(class_idx))
    }

    /// Get a class index from a name
    fn get_class_idx(&mut self, class_name: &RefType) -> Result<ClassConstantIndex, Error> {
        let rendered = class_name.render_class_info();
        let mut constants = self.constants();
        let object_name = constants.get_utf8(rendered)?;
        Ok(constants.get_class(object_name)?)
    }
}

impl<A: CodeBuilder> CodeBuilderExts for A {}

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
