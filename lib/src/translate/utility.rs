use super::{AccessMode, CodeBuilderExts, Error, Settings};
use crate::jvm::{
    BranchInstruction, ClassAccessFlags, ClassBuilder, ClassGraph, CompareMode, FieldType,
    InnerClass, InnerClassAccessFlags, InnerClasses, Instruction, InvokeType, MethodAccessFlags,
    MethodDescriptor, OrdComparison, RefType, ShiftType, BinaryName, UnqualifiedName
};
use std::cell::RefCell;
use std::collections::HashSet;
use std::convert::TryFrom;
use std::rc::Rc;

/// Potential utility methods.
///
/// Whenever code-gen incurs more than a couple extra bytes worth of conversion instructions, it is
/// worth abstracting into a utility  method.
#[derive(Copy, Clone, Hash, PartialEq, Eq)]
pub enum UtilityMethod {
    /// Signed division of two `int`s, but throwing an exception if we try to divide the minimum
    /// `int` value by `-1`
    I32DivS,

    /// Signed division of two `long`s, but throwing an exception if we try to divide the minimum
    /// `long` value by `-1`
    I64DivS,

    /// Bitwise absolute value of a `float`
    F32Abs,

    /// Bitwise absolute value of a `double`
    F64Abs,

    /// Round a `float` towards 0 to the nearest integral `float`
    F32Trunc,

    /// Round a `double` towards 0 to the nearest integral `double`
    F64Trunc,

    /// Unreachable (returns a fresh `AssertError("unreachable", null)` instance to throw)
    Unreachable,

    /// Convert a `float` to an `int` and throw an `ArithmeticException` error if the output
    /// doesn't fit in an `int`
    I32TruncF32S,

    /// Convert a `float` to an unsigned `int` and throw an `ArithmeticException` error if the
    /// output doesn't fit in an `int`
    I32TruncF32U,

    /// Convert a `double` to an `int` and throw an `ArithmeticException` error if the output
    /// doesn't fit in an `int`
    I32TruncF64S,

    /// Convert a `double` to an unsigned `int` and throw an `ArithmeticException` error if the
    /// output doesn't fit in an `int`
    I32TruncF64U,

    /// Convert an unsigned `int` to a `long`
    I64ExtendI32U,

    /// Convert a `float` to an `long` and throw an `ArithmeticException` error if the output
    /// doesn't fit in an `long`
    I64TruncF32S,

    /// Convert a `float` to an unsigned `long` and throw an `ArithmeticException` error if the
    /// output doesn't fit in an `long`
    I64TruncF32U,

    /// Convert a `double` to an `long` and throw an `ArithmeticException` error if the output
    /// doesn't fit in an `long`
    I64TruncF64S,

    /// Convert a `double` to an unsigned `long` and throw an `ArithmeticException` error if the
    /// output doesn't fit in an `long`
    I64TruncF64U,

    /// Convert an unsigned `int` to a `float`
    F32ConvertI32U,

    /// Convert an unsigned `long` to a `float`
    F32ConvertI64U,

    /// Convert an unsigned `int` to a `double`
    F64ConvertI32U,

    /// Convert an unsigned `long` to a `double`
    F64ConvertI64U,

    /// Perform a saturating conversion of a `float` to an unsigned `int` (don't throw, just pick
    /// the "best" `int` available)
    I32TruncSatF32U,

    /// Perform a saturating conversion of a `double` to an unsigned `int` (don't throw, just pick
    /// the "best" `int` available)
    I32TruncSatF64U,

    /// Perform a saturating conversion of a `float` to an unsigned `long` (don't throw, just pick
    /// the "best" `long` available)
    I64TruncSatF32U,

    /// Perform a saturating conversion of a `double` to an unsigned `long` (don't throw, just pick
    /// the "best" `long` available)
    I64TruncSatF64U,

    /// Bootstrap method for performing `funcref` table operations through `invokedynamic`. Also
    /// handles `call_indirect`.
    FuncrefTableBootstrap,

    /// Bootstrap method for performing `externref` table operations through `invokedynamic`.
    ExternrefTableBootstrap,
}
impl UtilityMethod {
    /// Get the method name
    pub const fn name(&self) -> UnqualifiedName<'static> {
        match self {
            UtilityMethod::I32DivS => UnqualifiedName::I32DIVS,
            UtilityMethod::I64DivS => UnqualifiedName::I64DIVS,
            UtilityMethod::F32Abs => UnqualifiedName::F32ABS,
            UtilityMethod::F64Abs => UnqualifiedName::F64ABS,
            UtilityMethod::F32Trunc => UnqualifiedName::F32TRUNC,
            UtilityMethod::F64Trunc => UnqualifiedName::F64TRUNC,
            UtilityMethod::Unreachable => UnqualifiedName::UNREACHABLE,
            UtilityMethod::I32TruncF32S => UnqualifiedName::I32TRUNCF32S,
            UtilityMethod::I32TruncF32U => UnqualifiedName::I32TRUNCF32U,
            UtilityMethod::I32TruncF64S => UnqualifiedName::I32TRUNCF64S,
            UtilityMethod::I32TruncF64U => UnqualifiedName::I32TRUNCF64U,
            UtilityMethod::I64ExtendI32U => UnqualifiedName::I64EXTENDI32U,
            UtilityMethod::I64TruncF32S => UnqualifiedName::I64TRUNCF32S,
            UtilityMethod::I64TruncF32U => UnqualifiedName::I64TRUNCF32U,
            UtilityMethod::I64TruncF64S => UnqualifiedName::I64TRUNCF64S,
            UtilityMethod::I64TruncF64U => UnqualifiedName::I64TRUNCF64U,
            UtilityMethod::F32ConvertI32U => UnqualifiedName::F32CONVERTI32U,
            UtilityMethod::F32ConvertI64U => UnqualifiedName::F32CONVERTI64U,
            UtilityMethod::F64ConvertI32U => UnqualifiedName::F64CONVERTI32U,
            UtilityMethod::F64ConvertI64U => UnqualifiedName::F64CONVERTI64U,
            UtilityMethod::I32TruncSatF32U => UnqualifiedName::I32TRUNCSATF32U,
            UtilityMethod::I32TruncSatF64U => UnqualifiedName::I32TRUNCSATF64U,
            UtilityMethod::I64TruncSatF32U => UnqualifiedName::I64TRUNCSATF32U,
            UtilityMethod::I64TruncSatF64U => UnqualifiedName::I64TRUNCSATF64U,
            UtilityMethod::FuncrefTableBootstrap => UnqualifiedName::FUNCREFTABLEBOOTSTRAP,
            UtilityMethod::ExternrefTableBootstrap => UnqualifiedName::EXTERNREFTABLEBOOTSTRAP,
        }
    }

    /// Get the method descriptor
    pub fn descriptor(&self) -> MethodDescriptor {
        match self {
            UtilityMethod::I32DivS => MethodDescriptor {
                parameters: vec![FieldType::INT, FieldType::INT],
                return_type: Some(FieldType::INT),
            },
            UtilityMethod::I64DivS => MethodDescriptor {
                parameters: vec![FieldType::LONG, FieldType::LONG],
                return_type: Some(FieldType::LONG),
            },
            UtilityMethod::F32Abs => MethodDescriptor {
                parameters: vec![FieldType::FLOAT],
                return_type: Some(FieldType::FLOAT),
            },
            UtilityMethod::F64Abs => MethodDescriptor {
                parameters: vec![FieldType::DOUBLE],
                return_type: Some(FieldType::DOUBLE),
            },
            UtilityMethod::F32Trunc => MethodDescriptor {
                parameters: vec![FieldType::FLOAT],
                return_type: Some(FieldType::FLOAT),
            },
            UtilityMethod::F64Trunc => MethodDescriptor {
                parameters: vec![FieldType::DOUBLE],
                return_type: Some(FieldType::DOUBLE),
            },
            UtilityMethod::Unreachable => MethodDescriptor {
                parameters: vec![],
                return_type: Some(FieldType::Ref(RefType::ASSERTIONERROR)),
            },
            UtilityMethod::I32TruncF32S => MethodDescriptor {
                parameters: vec![FieldType::FLOAT],
                return_type: Some(FieldType::INT),
            },
            UtilityMethod::I32TruncF32U => MethodDescriptor {
                parameters: vec![FieldType::FLOAT],
                return_type: Some(FieldType::INT),
            },
            UtilityMethod::I32TruncF64S => MethodDescriptor {
                parameters: vec![FieldType::DOUBLE],
                return_type: Some(FieldType::INT),
            },
            UtilityMethod::I32TruncF64U => MethodDescriptor {
                parameters: vec![FieldType::DOUBLE],
                return_type: Some(FieldType::INT),
            },
            UtilityMethod::I64ExtendI32U => MethodDescriptor {
                parameters: vec![FieldType::INT],
                return_type: Some(FieldType::LONG),
            },
            UtilityMethod::I64TruncF32S => MethodDescriptor {
                parameters: vec![FieldType::FLOAT],
                return_type: Some(FieldType::LONG),
            },
            UtilityMethod::I64TruncF32U => MethodDescriptor {
                parameters: vec![FieldType::FLOAT],
                return_type: Some(FieldType::LONG),
            },
            UtilityMethod::I64TruncF64S => MethodDescriptor {
                parameters: vec![FieldType::DOUBLE],
                return_type: Some(FieldType::LONG),
            },
            UtilityMethod::I64TruncF64U => MethodDescriptor {
                parameters: vec![FieldType::DOUBLE],
                return_type: Some(FieldType::LONG),
            },
            UtilityMethod::F32ConvertI32U => MethodDescriptor {
                parameters: vec![FieldType::INT],
                return_type: Some(FieldType::FLOAT),
            },
            UtilityMethod::F32ConvertI64U => MethodDescriptor {
                parameters: vec![FieldType::LONG],
                return_type: Some(FieldType::FLOAT),
            },
            UtilityMethod::F64ConvertI32U => MethodDescriptor {
                parameters: vec![FieldType::INT],
                return_type: Some(FieldType::DOUBLE),
            },
            UtilityMethod::F64ConvertI64U => MethodDescriptor {
                parameters: vec![FieldType::LONG],
                return_type: Some(FieldType::DOUBLE),
            },
            UtilityMethod::I32TruncSatF32U => MethodDescriptor {
                parameters: vec![FieldType::FLOAT],
                return_type: Some(FieldType::INT),
            },
            UtilityMethod::I32TruncSatF64U => MethodDescriptor {
                parameters: vec![FieldType::DOUBLE],
                return_type: Some(FieldType::INT),
            },
            UtilityMethod::I64TruncSatF32U => MethodDescriptor {
                parameters: vec![FieldType::FLOAT],
                return_type: Some(FieldType::LONG),
            },
            UtilityMethod::I64TruncSatF64U => MethodDescriptor {
                parameters: vec![FieldType::DOUBLE],
                return_type: Some(FieldType::LONG),
            },
            UtilityMethod::ExternrefTableBootstrap => todo!(),
            UtilityMethod::FuncrefTableBootstrap => todo!(),
        }
    }
}

/// Class that serves a shared carrier of utility methods. In the name of keeping the translation
/// outputs lean, these features are enumerated so that they can be requested then generated only
/// on demand.
pub struct UtilityClass<'a> {
    pub class: ClassBuilder<'a>,
    methods: HashSet<UtilityMethod>,
}

impl<'a> UtilityClass<'a> {
    pub fn new(
        settings: &Settings,
        class_graph: Rc<RefCell<ClassGraph<'a>>>,
    ) -> Result<UtilityClass<'a>, Error> {
        let mut class = ClassBuilder::new(
            ClassAccessFlags::SYNTHETIC,
            BinaryName::try_from(format!(
                "{}${}",
                settings.output_full_class_name, settings.utilities_short_class_name
            ).as_str()).unwrap(),
            BinaryName::OBJECT,
            false,
            vec![],
            class_graph.clone(),
        )?;

        // Add the `InnerClasses` attribute
        let inner_classes: InnerClasses = {
            let mut constants = class.constants();
            let outer_class_name = constants.get_utf8(settings.output_full_class_name.as_ref())?;
            let outer_class = constants.get_class(outer_class_name)?;
            let inner_class_name = constants.get_utf8(class.class_name().as_ref())?;
            let inner_class = constants.get_class(inner_class_name)?;
            let inner_name = constants.get_utf8(settings.utilities_short_class_name.as_ref())?;
            let inner_class_attr = InnerClass {
                inner_class,
                outer_class,
                inner_name,
                access_flags: InnerClassAccessFlags::STATIC,
            };
            InnerClasses(vec![inner_class_attr])
        };
        class.add_attribute(inner_classes)?;

        Ok(UtilityClass {
            class,
            methods: HashSet::new(),
        })
    }

    /// Ensure the utility is defined, then call it on the specified code builder
    pub fn invoke_utility<B: CodeBuilderExts<'a>>(
        &mut self,
        method: UtilityMethod,
        code: &mut B,
    ) -> Result<(), Error> {
        let _ = self.add_utility_method(method)?;
        let class_name = self.class.class_name();
        let method_name = method.name();
        code.invoke_explicit(
            InvokeType::Static,
            &class_name,
            &method_name,
            &method.descriptor(),
        )?;
        Ok(())
    }

    /// Add a utility method and return if it was already there
    pub fn add_utility_method(&mut self, method: UtilityMethod) -> Result<bool, Error> {
        if !self.methods.insert(method) {
            return Ok(false);
        }

        let descriptor = method.descriptor();
        let mut method_builder = self.class.start_method(
            MethodAccessFlags::STATIC,
            method.name().to_owned(),
            descriptor,
        )?;
        let code = &mut method_builder.code;

        match method {
            UtilityMethod::I32DivS => Self::generate_i32_div_s(code)?,
            UtilityMethod::I64DivS => Self::generate_i64_div_s(code)?,
            UtilityMethod::F32Abs => Self::generate_f32_abs(code)?,
            UtilityMethod::F64Abs => Self::generate_f64_abs(code)?,
            UtilityMethod::F32Trunc => Self::generate_f32_trunc(code)?,
            UtilityMethod::F64Trunc => Self::generate_f64_trunc(code)?,
            UtilityMethod::Unreachable => Self::generate_unreachable(code)?,
            UtilityMethod::I32TruncF32S => Self::generate_i32_trunc_f32_s(code)?,
            UtilityMethod::I32TruncF32U => Self::generate_i32_trunc_f32_u(code)?,
            UtilityMethod::I32TruncF64S => Self::generate_i32_trunc_f64_s(code)?,
            UtilityMethod::I32TruncF64U => Self::generate_i32_trunc_f64_u(code)?,
            UtilityMethod::I64ExtendI32U => Self::generate_i64_extend_i32_u(code)?,
            UtilityMethod::I64TruncF32S => Self::generate_i64_trunc_f32_s(code)?,
            UtilityMethod::I64TruncF32U => Self::generate_i64_trunc_f32_u(code)?,
            UtilityMethod::I64TruncF64S => Self::generate_i64_trunc_f64_s(code)?,
            UtilityMethod::I64TruncF64U => Self::generate_i64_trunc_f64_u(code)?,
            UtilityMethod::F32ConvertI32U => Self::generate_f32_convert_i32_u(code)?,
            UtilityMethod::F32ConvertI64U => Self::generate_f32_convert_i64_u(code)?,
            UtilityMethod::F64ConvertI32U => Self::generate_f64_convert_i32_u(code)?,
            UtilityMethod::F64ConvertI64U => Self::generate_f64_convert_i64_u(code)?,
            UtilityMethod::I32TruncSatF32U => Self::generate_i32_trunc_sat_f32_u(code)?,
            UtilityMethod::I32TruncSatF64U => Self::generate_i32_trunc_sat_f64_u(code)?,
            UtilityMethod::I64TruncSatF32U => Self::generate_i64_trunc_sat_f32_u(code)?,
            UtilityMethod::I64TruncSatF64U => Self::generate_i64_trunc_sat_f64_u(code)?,
            UtilityMethod::ExternrefTableBootstrap => todo!(),
            UtilityMethod::FuncrefTableBootstrap => todo!(),
        }

        self.class.finish_method(method_builder)?;
        Ok(true)
    }

    fn generate_i32_div_s<'b, B: CodeBuilderExts<'b>>(code: &mut B) -> Result<(), Error> {
        let regular_div = code.fresh_label();

        // Check if second argument is -1...
        code.push_instruction(Instruction::ILoad(1))?;
        code.push_instruction(Instruction::IConstM1)?;
        code.push_branch_instruction(BranchInstruction::IfICmp(
            OrdComparison::NE,
            regular_div,
            (),
        ))?;

        // Check if first argument is `Integer.MIN_VALUE`
        code.push_instruction(Instruction::ILoad(0))?;
        code.access_field(&BinaryName::INTEGER, &UnqualifiedName::MINVALUE, AccessMode::Read)?;
        code.push_branch_instruction(BranchInstruction::IfICmp(
            OrdComparison::NE,
            regular_div,
            (),
        ))?;

        let cls_idx = code.get_class_idx(&RefType::ARITHMETICEXCEPTION)?;
        code.push_instruction(Instruction::New(cls_idx))?;
        code.push_instruction(Instruction::Dup)?;
        code.const_string("integer overflow")?;
        code.invoke(&BinaryName::ARITHMETICEXCEPTION, &UnqualifiedName::INIT)?;
        code.push_branch_instruction(BranchInstruction::AThrow)?;

        // This is the usual path: where we aren't dividing `Integer.MIN_VALUE` by `-1`
        code.place_label(regular_div)?;
        code.push_instruction(Instruction::ILoad(0))?;
        code.push_instruction(Instruction::ILoad(1))?;
        code.push_instruction(Instruction::IDiv)?;
        code.push_branch_instruction(BranchInstruction::IReturn)?;

        Ok(())
    }

    fn generate_i64_div_s<'b, B: CodeBuilderExts<'b>>(code: &mut B) -> Result<(), Error> {
        let regular_div = code.fresh_label();

        // Check if second argument is -1...
        code.push_instruction(Instruction::LLoad(2))?;
        code.push_instruction(Instruction::IConstM1)?;
        code.push_instruction(Instruction::I2L)?;
        code.push_instruction(Instruction::LCmp)?;
        code.push_branch_instruction(BranchInstruction::If(OrdComparison::NE, regular_div, ()))?;

        // Check if first argument is `Long.MIN_VALUE`
        code.push_instruction(Instruction::LLoad(0))?;
        code.access_field(&BinaryName::LONG, &UnqualifiedName::MINVALUE, AccessMode::Read)?;
        code.push_instruction(Instruction::LCmp)?;
        code.push_branch_instruction(BranchInstruction::If(OrdComparison::NE, regular_div, ()))?;

        let cls_idx = code.get_class_idx(&RefType::ARITHMETICEXCEPTION)?;
        code.push_instruction(Instruction::New(cls_idx))?;
        code.push_instruction(Instruction::Dup)?;
        code.const_string("integer overflow")?;
        code.invoke(&BinaryName::ARITHMETICEXCEPTION, &UnqualifiedName::INIT)?;
        code.push_branch_instruction(BranchInstruction::AThrow)?;

        // This is the usual path: where we aren't dividing `Long.MIN_VALUE` by `-1`
        code.place_label(regular_div)?;
        code.push_instruction(Instruction::LLoad(0))?;
        code.push_instruction(Instruction::LLoad(2))?;
        code.push_instruction(Instruction::LDiv)?;
        code.push_branch_instruction(BranchInstruction::LReturn)?;

        Ok(())
    }

    fn generate_f32_abs<'b, B: CodeBuilderExts<'b>>(code: &mut B) -> Result<(), Error> {
        code.push_instruction(Instruction::FLoad(0))?;
        code.invoke(&BinaryName::FLOAT, &UnqualifiedName::FLOATTORAWINTBITS)?;
        code.const_int(0x7FFF_FFFF)?;
        code.push_instruction(Instruction::IAnd)?;
        code.invoke(&BinaryName::FLOAT, &UnqualifiedName::INTBITSTOFLOAT)?;
        code.push_branch_instruction(BranchInstruction::FReturn)?;

        Ok(())
    }

    fn generate_f64_abs<'b, B: CodeBuilderExts<'b>>(code: &mut B) -> Result<(), Error> {
        code.push_instruction(Instruction::DLoad(0))?;
        code.invoke(&BinaryName::DOUBLE, &UnqualifiedName::DOUBLETORAWLONGBITS)?;
        code.const_long(0x7FFF_FFFF_FFFF_FFFF)?;
        code.push_instruction(Instruction::LAnd)?;
        code.invoke(&BinaryName::DOUBLE, &UnqualifiedName::LONGBITSTODOUBLE)?;
        code.push_branch_instruction(BranchInstruction::DReturn)?;

        Ok(())
    }

    fn generate_f32_trunc<'b, B: CodeBuilderExts<'b>>(code: &mut B) -> Result<(), Error> {
        let negative = code.fresh_label();

        code.push_instruction(Instruction::FLoad(0))?;
        code.push_instruction(Instruction::F2D)?;
        code.push_instruction(Instruction::Dup2)?;
        code.push_instruction(Instruction::DConst0)?;
        code.push_instruction(Instruction::DCmp(CompareMode::G))?;

        // positive argument
        code.push_branch_instruction(BranchInstruction::If(OrdComparison::LT, negative, ()))?;
        code.invoke(&BinaryName::MATH, &UnqualifiedName::FLOOR)?;
        code.push_instruction(Instruction::D2F)?;
        code.push_branch_instruction(BranchInstruction::FReturn)?;

        // negative argument
        code.place_label(negative)?;
        code.invoke(&BinaryName::MATH, &UnqualifiedName::CEIL)?;
        code.push_instruction(Instruction::D2F)?;
        code.push_branch_instruction(BranchInstruction::FReturn)?;

        Ok(())
    }

    fn generate_f64_trunc<'b, B: CodeBuilderExts<'b>>(code: &mut B) -> Result<(), Error> {
        let negative = code.fresh_label();

        code.push_instruction(Instruction::DLoad(0))?;
        code.push_instruction(Instruction::DConst0)?;
        code.push_instruction(Instruction::DCmp(CompareMode::G))?;

        // positive argument
        code.push_branch_instruction(BranchInstruction::If(OrdComparison::LT, negative, ()))?;
        code.push_instruction(Instruction::DLoad(0))?;
        code.invoke(&BinaryName::MATH, &UnqualifiedName::FLOOR)?;
        code.push_branch_instruction(BranchInstruction::DReturn)?;

        // negative argument
        code.place_label(negative)?;
        code.push_instruction(Instruction::DLoad(0))?;
        code.invoke(&BinaryName::MATH, &UnqualifiedName::CEIL)?;
        code.push_branch_instruction(BranchInstruction::DReturn)?;

        Ok(())
    }

    fn generate_unreachable<'b, B: CodeBuilderExts<'b>>(code: &mut B) -> Result<(), Error> {
        let cls_idx = code.get_class_idx(&RefType::ASSERTIONERROR)?;
        code.push_instruction(Instruction::New(cls_idx))?;
        code.push_instruction(Instruction::Dup)?;
        code.invoke(&BinaryName::ASSERTIONERROR, &UnqualifiedName::INIT)?;
        code.push_branch_instruction(BranchInstruction::AReturn)?;

        Ok(())
    }

    fn generate_i32_trunc_f32_s<'b, B: CodeBuilderExts<'b>>(code: &mut B) -> Result<(), Error> {
        let error_case = code.fresh_label();

        // Check if the argument is too small...
        let min_float = -2f32.powi(31);
        code.const_float(min_float)?;
        code.push_instruction(Instruction::FLoad(0))?;
        code.push_instruction(Instruction::FCmp(CompareMode::G))?;
        code.push_branch_instruction(BranchInstruction::If(OrdComparison::GT, error_case, ()))?;

        // Check if argument is too large...
        let max_float = 2f32.powi(31);
        code.const_float(max_float)?;
        code.push_instruction(Instruction::FLoad(0))?;
        code.push_instruction(Instruction::FCmp(CompareMode::G))?;
        code.push_branch_instruction(BranchInstruction::If(OrdComparison::LE, error_case, ()))?;

        // Now that we know the conversion is safe, do it
        code.push_instruction(Instruction::FLoad(0))?;
        code.push_instruction(Instruction::F2I)?;
        code.push_branch_instruction(BranchInstruction::IReturn)?;

        // Error case
        let cls_idx = code.get_class_idx(&RefType::ARITHMETICEXCEPTION)?;
        code.place_label(error_case)?;
        code.push_instruction(Instruction::New(cls_idx))?;
        code.push_instruction(Instruction::Dup)?;
        code.const_string("float to int overflow")?;
        code.invoke(&BinaryName::ARITHMETICEXCEPTION, &UnqualifiedName::INIT)?;
        code.push_branch_instruction(BranchInstruction::AThrow)?;

        Ok(())
    }

    fn generate_i32_trunc_f32_u<'b, B: CodeBuilderExts<'b>>(code: &mut B) -> Result<(), Error> {
        let error_case = code.fresh_label();

        // temp variable
        code.push_instruction(Instruction::LConst0)?;
        code.push_instruction(Instruction::LStore(1))?;

        // Check if the argument is too small...
        code.push_instruction(Instruction::IConstM1)?;
        code.push_instruction(Instruction::I2F)?;
        code.push_instruction(Instruction::FLoad(0))?;
        code.push_instruction(Instruction::FCmp(CompareMode::G))?;
        code.push_branch_instruction(BranchInstruction::If(OrdComparison::GE, error_case, ()))?;

        // Check if argument is too large...
        code.push_instruction(Instruction::FLoad(0))?;
        code.push_instruction(Instruction::F2L)?;
        code.push_instruction(Instruction::Dup2)?;
        code.push_instruction(Instruction::LStore(1))?;
        code.const_long(0x0000_0000_ffff_ffff)?;
        code.push_instruction(Instruction::LCmp)?;
        code.push_branch_instruction(BranchInstruction::If(OrdComparison::GT, error_case, ()))?;

        // Now that we know the conversion is safe, do it
        code.push_instruction(Instruction::LLoad(1))?;
        code.push_instruction(Instruction::L2I)?;
        code.push_branch_instruction(BranchInstruction::IReturn)?;

        // Error case
        let cls_idx = code.get_class_idx(&RefType::ARITHMETICEXCEPTION)?;
        code.place_label(error_case)?;
        code.push_instruction(Instruction::New(cls_idx))?;
        code.push_instruction(Instruction::Dup)?;
        code.const_string("float to unsigned int overflow")?;
        code.invoke(&BinaryName::ARITHMETICEXCEPTION, &UnqualifiedName::INIT)?;
        code.push_branch_instruction(BranchInstruction::AThrow)?;

        Ok(())
    }

    fn generate_i32_trunc_f64_s<'b, B: CodeBuilderExts<'b>>(code: &mut B) -> Result<(), Error> {
        let error_case = code.fresh_label();

        // Check if the argument is too small...
        let min_double = -2f64.powi(31) - 1f64;
        code.const_double(min_double)?;
        code.push_instruction(Instruction::DLoad(0))?;
        code.push_instruction(Instruction::DCmp(CompareMode::G))?;
        code.push_branch_instruction(BranchInstruction::If(OrdComparison::GE, error_case, ()))?;

        // Check if first argument is too large...
        let max_double = 2f64.powi(31);
        code.const_double(max_double)?;
        code.push_instruction(Instruction::DLoad(0))?;
        code.push_instruction(Instruction::DCmp(CompareMode::G))?;
        code.push_branch_instruction(BranchInstruction::If(OrdComparison::LE, error_case, ()))?;

        // Now that we know the conversion is safe, do it
        code.push_instruction(Instruction::DLoad(0))?;
        code.push_instruction(Instruction::D2I)?;
        code.push_branch_instruction(BranchInstruction::IReturn)?;

        // Error case
        let cls_idx = code.get_class_idx(&RefType::ARITHMETICEXCEPTION)?;
        code.place_label(error_case)?;
        code.push_instruction(Instruction::New(cls_idx))?;
        code.push_instruction(Instruction::Dup)?;
        code.const_string("double to int overflow")?;
        code.invoke(&BinaryName::ARITHMETICEXCEPTION, &UnqualifiedName::INIT)?;
        code.push_branch_instruction(BranchInstruction::AThrow)?;

        Ok(())
    }

    fn generate_i32_trunc_f64_u<'b, B: CodeBuilderExts<'b>>(code: &mut B) -> Result<(), Error> {
        let error_case = code.fresh_label();

        // temp variable
        code.push_instruction(Instruction::LConst0)?;
        code.push_instruction(Instruction::LStore(2))?;

        // Check if the argument is too small...
        code.push_instruction(Instruction::IConstM1)?;
        code.push_instruction(Instruction::I2D)?;
        code.push_instruction(Instruction::DLoad(0))?;
        code.push_instruction(Instruction::DCmp(CompareMode::G))?;
        code.push_branch_instruction(BranchInstruction::If(OrdComparison::GE, error_case, ()))?;

        // Check if argument is too large...
        code.push_instruction(Instruction::DLoad(0))?;
        code.push_instruction(Instruction::D2L)?;
        code.push_instruction(Instruction::Dup2)?;
        code.push_instruction(Instruction::LStore(2))?;
        code.const_long(0x0000_0000_ffff_ffff)?;
        code.push_instruction(Instruction::LCmp)?;
        code.push_branch_instruction(BranchInstruction::If(OrdComparison::GT, error_case, ()))?;

        // Now that we know the conversion is safe, do it
        code.push_instruction(Instruction::LLoad(2))?;
        code.push_instruction(Instruction::L2I)?;
        code.push_branch_instruction(BranchInstruction::IReturn)?;

        // Error case
        let cls_idx = code.get_class_idx(&RefType::ARITHMETICEXCEPTION)?;
        code.place_label(error_case)?;
        code.push_instruction(Instruction::New(cls_idx))?;
        code.push_instruction(Instruction::Dup)?;
        code.const_string("double to unsigned int overflow")?;
        code.invoke(&BinaryName::ARITHMETICEXCEPTION, &UnqualifiedName::INIT)?;
        code.push_branch_instruction(BranchInstruction::AThrow)?;

        Ok(())
    }

    fn generate_i64_trunc_f32_s<'b, B: CodeBuilderExts<'b>>(code: &mut B) -> Result<(), Error> {
        let error_case = code.fresh_label();

        // Check if the argument is too small...
        let min_float = -2f32.powi(63);
        code.const_float(min_float)?;
        code.push_instruction(Instruction::FLoad(0))?;
        code.push_instruction(Instruction::FCmp(CompareMode::G))?;
        code.push_branch_instruction(BranchInstruction::If(OrdComparison::GT, error_case, ()))?;

        // Check if first argument is too large...
        let max_float = 2f32.powi(63);
        code.const_float(max_float)?;
        code.push_instruction(Instruction::FLoad(0))?;
        code.push_instruction(Instruction::FCmp(CompareMode::G))?;
        code.push_branch_instruction(BranchInstruction::If(OrdComparison::LE, error_case, ()))?;

        // Now that we know the conversion is safe, do it
        code.push_instruction(Instruction::FLoad(0))?;
        code.push_instruction(Instruction::F2L)?;
        code.push_branch_instruction(BranchInstruction::LReturn)?;

        // Error case
        let cls_idx = code.get_class_idx(&RefType::ARITHMETICEXCEPTION)?;
        code.place_label(error_case)?;
        code.push_instruction(Instruction::New(cls_idx))?;
        code.push_instruction(Instruction::Dup)?;
        code.const_string("float to long overflow")?;
        code.invoke(&BinaryName::ARITHMETICEXCEPTION, &UnqualifiedName::INIT)?;
        code.push_branch_instruction(BranchInstruction::AThrow)?;

        Ok(())
    }

    fn generate_i64_trunc_f32_u<'b, B: CodeBuilderExts<'b>>(code: &mut B) -> Result<(), Error> {
        let error_case = code.fresh_label();
        let is_first_bit_one = code.fresh_label();

        // Check if the argument is too small...
        code.push_instruction(Instruction::IConstM1)?;
        code.push_instruction(Instruction::I2F)?;
        code.push_instruction(Instruction::FLoad(0))?;
        code.push_instruction(Instruction::FCmp(CompareMode::G))?;
        code.push_branch_instruction(BranchInstruction::If(OrdComparison::GE, error_case, ()))?;

        // Check if argument is too large...
        let max_float = 2f32.powi(64);
        code.const_float(max_float)?;
        code.push_instruction(Instruction::FLoad(0))?;
        code.push_instruction(Instruction::FCmp(CompareMode::G))?;
        code.push_branch_instruction(BranchInstruction::If(OrdComparison::LE, error_case, ()))?;

        // Check if the float fits in 63 bits
        let min_first_bit_one = 2f32.powi(63);
        code.const_float(min_first_bit_one)?;
        code.push_instruction(Instruction::FLoad(0))?;
        code.push_instruction(Instruction::FCmp(CompareMode::G))?;
        code.push_branch_instruction(BranchInstruction::If(
            OrdComparison::LE,
            is_first_bit_one,
            (),
        ))?;

        // Float fits in the first 63 bits
        code.push_instruction(Instruction::FLoad(0))?;
        code.push_instruction(Instruction::F2L)?;
        code.push_branch_instruction(BranchInstruction::LReturn)?;

        // Float does not fit in the first 63 bits
        code.place_label(is_first_bit_one)?;
        code.push_instruction(Instruction::FLoad(0))?;
        code.const_float(min_first_bit_one)?;
        code.push_instruction(Instruction::FSub)?;
        code.push_instruction(Instruction::F2L)?;
        code.const_long(-0x8000_0000_0000_0000)?;
        code.push_instruction(Instruction::LOr)?;
        code.push_branch_instruction(BranchInstruction::LReturn)?;

        // Error case
        let cls_idx = code.get_class_idx(&RefType::ARITHMETICEXCEPTION)?;
        code.place_label(error_case)?;
        code.push_instruction(Instruction::New(cls_idx))?;
        code.push_instruction(Instruction::Dup)?;
        code.const_string("float to unsigned long overflow")?;
        code.invoke(&BinaryName::ARITHMETICEXCEPTION, &UnqualifiedName::INIT)?;
        code.push_branch_instruction(BranchInstruction::AThrow)?;

        Ok(())
    }

    fn generate_i64_trunc_f64_s<'b, B: CodeBuilderExts<'b>>(code: &mut B) -> Result<(), Error> {
        let error_case = code.fresh_label();

        // Check if the argument is too small...
        let min_double = -2f64.powi(63);
        code.const_double(min_double)?;
        code.push_instruction(Instruction::DLoad(0))?;
        code.push_instruction(Instruction::DCmp(CompareMode::G))?;
        code.push_branch_instruction(BranchInstruction::If(OrdComparison::GT, error_case, ()))?;

        // Check if argument is too large...
        let max_double = 2f64.powi(63);
        code.const_double(max_double)?;
        code.push_instruction(Instruction::DLoad(0))?;
        code.push_instruction(Instruction::DCmp(CompareMode::G))?;
        code.push_branch_instruction(BranchInstruction::If(OrdComparison::LE, error_case, ()))?;

        // Now that we know the conversion is safe, do it
        code.push_instruction(Instruction::DLoad(0))?;
        code.push_instruction(Instruction::D2L)?;
        code.push_branch_instruction(BranchInstruction::LReturn)?;

        // Error case
        let cls_idx = code.get_class_idx(&RefType::ARITHMETICEXCEPTION)?;
        code.place_label(error_case)?;
        code.push_instruction(Instruction::New(cls_idx))?;
        code.push_instruction(Instruction::Dup)?;
        code.const_string("double to long overflow")?;
        code.invoke(&BinaryName::ARITHMETICEXCEPTION, &UnqualifiedName::INIT)?;
        code.push_branch_instruction(BranchInstruction::AThrow)?;

        Ok(())
    }

    fn generate_i64_trunc_f64_u<'b, B: CodeBuilderExts<'b>>(code: &mut B) -> Result<(), Error> {
        let error_case = code.fresh_label();
        let is_first_bit_one = code.fresh_label();

        // Check if the argument is too small...
        code.push_instruction(Instruction::IConstM1)?;
        code.push_instruction(Instruction::I2D)?;
        code.push_instruction(Instruction::DLoad(0))?;
        code.push_instruction(Instruction::DCmp(CompareMode::G))?;
        code.push_branch_instruction(BranchInstruction::If(OrdComparison::GE, error_case, ()))?;

        // Check if argument is too large...
        let max_double = 2f64.powi(64);
        code.const_double(max_double)?;
        code.push_instruction(Instruction::DLoad(0))?;
        code.push_instruction(Instruction::DCmp(CompareMode::G))?;
        code.push_branch_instruction(BranchInstruction::If(OrdComparison::LE, error_case, ()))?;

        // Check if the float fits in 63 bits
        let min_first_bit_one = 2f64.powi(63);
        code.const_double(min_first_bit_one)?;
        code.push_instruction(Instruction::DLoad(0))?;
        code.push_instruction(Instruction::DCmp(CompareMode::G))?;
        code.push_branch_instruction(BranchInstruction::If(
            OrdComparison::LE,
            is_first_bit_one,
            (),
        ))?;

        // Double fits in the first 63 bits
        code.push_instruction(Instruction::DLoad(0))?;
        code.push_instruction(Instruction::D2L)?;
        code.push_branch_instruction(BranchInstruction::LReturn)?;

        // Double does not fit in the first 63 bits
        code.place_label(is_first_bit_one)?;
        code.push_instruction(Instruction::DLoad(0))?;
        code.const_double(min_first_bit_one)?;
        code.push_instruction(Instruction::DSub)?;
        code.push_instruction(Instruction::D2L)?;
        code.const_long(-0x8000_0000_0000_0000)?;
        code.push_instruction(Instruction::LOr)?;
        code.push_branch_instruction(BranchInstruction::LReturn)?;

        // Error case
        let cls_idx = code.get_class_idx(&RefType::ARITHMETICEXCEPTION)?;
        code.place_label(error_case)?;
        code.push_instruction(Instruction::New(cls_idx))?;
        code.push_instruction(Instruction::Dup)?;
        code.const_string("double to unsigned long overflow")?;
        code.invoke(&BinaryName::ARITHMETICEXCEPTION, &UnqualifiedName::INIT)?;
        code.push_branch_instruction(BranchInstruction::AThrow)?;

        Ok(())
    }

    fn generate_i64_extend_i32_u<'b, B: CodeBuilderExts<'b>>(code: &mut B) -> Result<(), Error> {
        code.push_instruction(Instruction::ILoad(0))?;
        code.push_instruction(Instruction::I2L)?;
        code.const_long(0x0000_0000_ffff_ffff)?;
        code.push_instruction(Instruction::LAnd)?;
        code.push_branch_instruction(BranchInstruction::LReturn)?;

        Ok(())
    }

    fn generate_f32_convert_i32_u<'b, B: CodeBuilderExts<'b>>(code: &mut B) -> Result<(), Error> {
        code.push_instruction(Instruction::ILoad(0))?;
        code.push_instruction(Instruction::I2L)?;
        code.const_long(0x0000_0000_ffff_ffff)?;
        code.push_instruction(Instruction::LAnd)?;
        code.push_instruction(Instruction::L2F)?;
        code.push_branch_instruction(BranchInstruction::FReturn)?;

        Ok(())
    }

    fn generate_f32_convert_i64_u<'b, B: CodeBuilderExts<'b>>(code: &mut B) -> Result<(), Error> {
        let first_bit_one = code.fresh_label();

        code.push_instruction(Instruction::LLoad(0))?;
        code.push_instruction(Instruction::LConst0)?;
        code.push_instruction(Instruction::LCmp)?;

        // The first bit of the unsigned integer is 0
        code.push_branch_instruction(BranchInstruction::If(OrdComparison::LT, first_bit_one, ()))?;
        code.push_instruction(Instruction::LLoad(0))?;
        code.push_instruction(Instruction::L2F)?;
        code.push_branch_instruction(BranchInstruction::FReturn)?;

        // The first bit of the unsigned integer is 1
        code.place_label(first_bit_one)?;
        code.push_instruction(Instruction::LLoad(0))?;
        code.push_instruction(Instruction::IConst1)?;
        code.push_instruction(Instruction::LSh(ShiftType::LogicalRight))?;
        code.push_instruction(Instruction::LLoad(0))?;
        code.push_instruction(Instruction::LConst1)?;
        code.push_instruction(Instruction::LAnd)?;
        code.push_instruction(Instruction::LOr)?;
        code.push_instruction(Instruction::L2F)?;
        code.push_instruction(Instruction::FConst2)?;
        code.push_instruction(Instruction::FMul)?;
        code.push_branch_instruction(BranchInstruction::FReturn)?;

        Ok(())
    }

    fn generate_f64_convert_i32_u<'b, B: CodeBuilderExts<'b>>(code: &mut B) -> Result<(), Error> {
        code.push_instruction(Instruction::ILoad(0))?;
        code.push_instruction(Instruction::I2L)?;
        code.const_long(0x0000_0000_ffff_ffff)?;
        code.push_instruction(Instruction::LAnd)?;
        code.push_instruction(Instruction::L2D)?;
        code.push_branch_instruction(BranchInstruction::DReturn)?;

        Ok(())
    }

    fn generate_f64_convert_i64_u<'b, B: CodeBuilderExts<'b>>(code: &mut B) -> Result<(), Error> {
        let first_bit_one = code.fresh_label();

        code.push_instruction(Instruction::LLoad(0))?;
        code.push_instruction(Instruction::LConst0)?;
        code.push_instruction(Instruction::LCmp)?;

        // The first bit of the unsigned integer is 0
        code.push_branch_instruction(BranchInstruction::If(OrdComparison::LT, first_bit_one, ()))?;
        code.push_instruction(Instruction::LLoad(0))?;
        code.push_instruction(Instruction::L2D)?;
        code.push_branch_instruction(BranchInstruction::DReturn)?;

        // The first bit of the unsigned integer is 1
        code.place_label(first_bit_one)?;
        code.push_instruction(Instruction::LLoad(0))?;
        code.push_instruction(Instruction::IConst1)?;
        code.push_instruction(Instruction::LSh(ShiftType::LogicalRight))?;
        code.push_instruction(Instruction::LLoad(0))?;
        code.push_instruction(Instruction::LConst1)?;
        code.push_instruction(Instruction::LAnd)?;
        code.push_instruction(Instruction::LOr)?;
        code.push_instruction(Instruction::L2D)?;
        code.push_instruction(Instruction::IConst2)?;
        code.push_instruction(Instruction::I2D)?;
        code.push_instruction(Instruction::DMul)?;
        code.push_branch_instruction(BranchInstruction::DReturn)?;

        Ok(())
    }

    fn generate_i32_trunc_sat_f32_u<'b, B: CodeBuilderExts<'b>>(code: &mut B) -> Result<(), Error> {
        let is_positive = code.fresh_label();
        let is_too_big = code.fresh_label();

        // temp variable
        code.push_instruction(Instruction::LConst0)?;
        code.push_instruction(Instruction::LStore(1))?;

        code.push_instruction(Instruction::FConst0)?;
        code.push_instruction(Instruction::FLoad(0))?;
        code.push_instruction(Instruction::FCmp(CompareMode::G))?;

        // Float is negative, so just return 0
        code.push_branch_instruction(BranchInstruction::If(OrdComparison::LT, is_positive, ()))?;
        code.push_instruction(Instruction::IConst0)?;
        code.push_branch_instruction(BranchInstruction::IReturn)?;

        code.place_label(is_positive)?;
        code.push_instruction(Instruction::FLoad(0))?;
        code.push_instruction(Instruction::F2L)?;
        code.push_instruction(Instruction::Dup2)?;
        code.push_instruction(Instruction::LStore(1))?;
        code.const_long(0x0000_0000_ffff_ffff)?;
        code.push_instruction(Instruction::LCmp)?;
        code.push_branch_instruction(BranchInstruction::If(OrdComparison::GT, is_too_big, ()))?;

        // Float fits in the range of an unsigned int
        code.push_instruction(Instruction::LLoad(1))?;
        code.push_instruction(Instruction::L2I)?;
        code.push_branch_instruction(BranchInstruction::IReturn)?;

        code.place_label(is_too_big)?;
        code.const_int(-1)?;
        code.push_branch_instruction(BranchInstruction::IReturn)?;

        Ok(())
    }

    fn generate_i32_trunc_sat_f64_u<'b, B: CodeBuilderExts<'b>>(code: &mut B) -> Result<(), Error> {
        let is_positive = code.fresh_label();
        let is_too_big = code.fresh_label();

        code.push_instruction(Instruction::DConst0)?;
        code.push_instruction(Instruction::DLoad(0))?;
        code.push_instruction(Instruction::DCmp(CompareMode::G))?;

        // Double is negative, so just return 0
        code.push_branch_instruction(BranchInstruction::If(OrdComparison::LT, is_positive, ()))?;
        code.push_instruction(Instruction::IConst0)?;
        code.push_branch_instruction(BranchInstruction::IReturn)?;

        code.place_label(is_positive)?;
        code.push_instruction(Instruction::DLoad(0))?;
        code.push_instruction(Instruction::D2L)?;
        code.push_instruction(Instruction::Dup2)?;
        code.const_long(0x0000_0000_ffff_ffff)?;
        code.push_instruction(Instruction::LCmp)?;
        code.push_branch_instruction(BranchInstruction::If(OrdComparison::GT, is_too_big, ()))?;

        // Double fits in the range of an unsigned int
        code.push_instruction(Instruction::L2I)?;
        code.push_branch_instruction(BranchInstruction::IReturn)?;

        code.place_label(is_too_big)?;
        code.const_int(-1)?;
        code.push_branch_instruction(BranchInstruction::IReturn)?;

        Ok(())
    }

    fn generate_i64_trunc_sat_f32_u<'b, B: CodeBuilderExts<'b>>(code: &mut B) -> Result<(), Error> {
        let is_positive = code.fresh_label();
        let is_first_bit_one = code.fresh_label();

        code.push_instruction(Instruction::FConst0)?;
        code.push_instruction(Instruction::FLoad(0))?;
        code.push_instruction(Instruction::FCmp(CompareMode::G))?;

        // Float is negative, so just return 0
        code.push_branch_instruction(BranchInstruction::If(OrdComparison::LT, is_positive, ()))?;
        code.push_instruction(Instruction::LConst0)?;
        code.push_branch_instruction(BranchInstruction::LReturn)?;

        code.place_label(is_positive)?;
        let min_first_bit_one = 2f32.powi(63);
        code.const_float(min_first_bit_one)?;
        code.push_instruction(Instruction::FLoad(0))?;
        code.push_instruction(Instruction::FCmp(CompareMode::G))?;
        code.push_branch_instruction(BranchInstruction::If(
            OrdComparison::LE,
            is_first_bit_one,
            (),
        ))?;

        // Float fits in the first 63 bits
        code.push_instruction(Instruction::FLoad(0))?;
        code.push_instruction(Instruction::F2L)?;
        code.push_branch_instruction(BranchInstruction::LReturn)?;

        // Float does not fit in the first 63 bits
        code.place_label(is_first_bit_one)?;
        code.push_instruction(Instruction::FLoad(0))?;
        code.const_float(min_first_bit_one)?;
        code.push_instruction(Instruction::FSub)?;
        code.push_instruction(Instruction::F2L)?;
        code.const_long(-0x8000_0000_0000_0000)?;
        code.push_instruction(Instruction::LOr)?;
        code.push_branch_instruction(BranchInstruction::LReturn)?;

        Ok(())
    }

    fn generate_i64_trunc_sat_f64_u<'b, B: CodeBuilderExts<'b>>(code: &mut B) -> Result<(), Error> {
        let is_positive = code.fresh_label();
        let is_first_bit_one = code.fresh_label();

        code.push_instruction(Instruction::DConst0)?;
        code.push_instruction(Instruction::DLoad(0))?;
        code.push_instruction(Instruction::DCmp(CompareMode::G))?;

        // Float is negative, so just return 0
        code.push_branch_instruction(BranchInstruction::If(OrdComparison::LT, is_positive, ()))?;
        code.push_instruction(Instruction::LConst0)?;
        code.push_branch_instruction(BranchInstruction::LReturn)?;

        code.place_label(is_positive)?;
        let min_first_bit_one = 2f64.powi(63);
        code.const_double(min_first_bit_one)?;
        code.push_instruction(Instruction::DLoad(0))?;
        code.push_instruction(Instruction::DCmp(CompareMode::G))?;
        code.push_branch_instruction(BranchInstruction::If(
            OrdComparison::LE,
            is_first_bit_one,
            (),
        ))?;

        // Float fits in the first 63 bits
        code.push_instruction(Instruction::DLoad(0))?;
        code.push_instruction(Instruction::D2L)?;
        code.push_branch_instruction(BranchInstruction::LReturn)?;

        // Float does not fit in the first 63 bits
        code.place_label(is_first_bit_one)?;
        code.push_instruction(Instruction::DLoad(0))?;
        code.const_double(min_first_bit_one)?;
        code.push_instruction(Instruction::DSub)?;
        code.push_instruction(Instruction::D2L)?;
        code.const_long(-0x8000_0000_0000_0000)?;
        code.push_instruction(Instruction::LOr)?;
        code.push_branch_instruction(BranchInstruction::LReturn)?;

        Ok(())
    }
}
