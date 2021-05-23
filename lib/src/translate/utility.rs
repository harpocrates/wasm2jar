use super::{CodeBuilderExts, Error, Settings};
use crate::jvm::{
    BranchInstruction, ClassAccessFlags, ClassBuilder, ClassGraph, FieldType, InnerClass,
    InnerClassAccessFlags, InnerClasses, Instruction, InvokeType, MethodAccessFlags,
    MethodDescriptor, OrdComparison, RefType,
};
use std::cell::RefCell;
use std::collections::HashSet;
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

    /// Round a `float` towards 0 to the nearest integral `float`
    F32Trunc,

    /// Round a `double` towards 0 to the nearest integral `double`
    F64Trunc,

    /// Absolute value of a float
    ///
    /// This is necessary instead of just using `Math.abs(F)F`, because it does not flip the NaN
    /// bit
    F32Abs,

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
}
impl UtilityMethod {
    /// Get the method name
    pub const fn name(&self) -> &'static str {
        match self {
            UtilityMethod::I32DivS => "i32DivS",
            UtilityMethod::I64DivS => "i64DivS",
            UtilityMethod::F32Trunc => "f32Trunc",
            UtilityMethod::F64Trunc => "f64Trunc",
            UtilityMethod::F32Abs => "f32Abs",
            UtilityMethod::Unreachable => "unreachable",
            UtilityMethod::I32TruncF32S => "i32TruncF32S",
            UtilityMethod::I32TruncF32U => "i32TruncF32U",
            UtilityMethod::I32TruncF64S => "i32TruncF64S",
            UtilityMethod::I32TruncF64U => "i32TruncF64U",
            UtilityMethod::I64ExtendI32U => "i64ExtendI32U",
            UtilityMethod::I64TruncF32S => "i64TruncF32S",
            UtilityMethod::I64TruncF32U => "i64TruncF32U",
            UtilityMethod::I64TruncF64S => "i64TruncF64S",
            UtilityMethod::I64TruncF64U => "i64TruncF64U",
            UtilityMethod::F32ConvertI32U => "f32ConvertI32U",
            UtilityMethod::F32ConvertI64U => "f32ConvertI64U",
            UtilityMethod::F64ConvertI32U => "f64ConvertI32U",
            UtilityMethod::F64ConvertI64U => "f64ConvertI64U",
            UtilityMethod::I32TruncSatF32U => "i32TruncSatF32U",
            UtilityMethod::I32TruncSatF64U => "i32TruncSatF64U",
            UtilityMethod::I64TruncSatF32U => "i64TruncSatF32U",
            UtilityMethod::I64TruncSatF64U => "i64TruncSatF64U",
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
            UtilityMethod::F32Trunc => MethodDescriptor {
                parameters: vec![FieldType::FLOAT],
                return_type: Some(FieldType::FLOAT),
            },
            UtilityMethod::F64Trunc => MethodDescriptor {
                parameters: vec![FieldType::DOUBLE],
                return_type: Some(FieldType::DOUBLE),
            },
            UtilityMethod::F32Abs => MethodDescriptor {
                parameters: vec![FieldType::FLOAT],
                return_type: Some(FieldType::FLOAT),
            },
            UtilityMethod::Unreachable => MethodDescriptor {
                parameters: vec![],
                return_type: Some(FieldType::Ref(RefType::ASSERTION_CLASS)),
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
        }
    }
}

/// Class that serves a shared carrier of utility methods. In the name of keeping the translation
/// outputs lean, these features are enumerated so that they can be requested then generated only
/// on demand.
pub struct UtilityClass {
    pub class: ClassBuilder,
    methods: HashSet<UtilityMethod>,
}

impl UtilityClass {
    pub fn new(
        settings: &Settings,
        class_graph: Rc<RefCell<ClassGraph>>,
    ) -> Result<UtilityClass, Error> {
        let mut class = ClassBuilder::new(
            ClassAccessFlags::SYNTHETIC,
            format!(
                "{}${}",
                settings.output_full_class_name, settings.utilities_short_class_name
            ),
            RefType::OBJECT_NAME.to_string(),
            false,
            vec![],
            class_graph.clone(),
        )?;

        // Add the `InnerClasses` attribute
        let inner_classes: InnerClasses = {
            let mut constants = class.constants();
            let outer_class_name = constants.get_utf8(&settings.output_full_class_name)?;
            let outer_class = constants.get_class(outer_class_name)?;
            let inner_class_name = constants.get_utf8(class.class_name())?;
            let inner_class = constants.get_class(inner_class_name)?;
            let inner_name = constants.get_utf8(&settings.utilities_short_class_name)?;
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
    pub fn invoke_utility<B: CodeBuilderExts>(
        &mut self,
        method: UtilityMethod,
        code: &mut B,
    ) -> Result<(), Error> {
        let _ = self.add_utility_method(method)?;
        let class_name = self.class.class_name().to_owned();
        let method_name = method.name();
        code.invoke_explicit(
            InvokeType::Static,
            class_name,
            method_name,
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
            UtilityMethod::Unreachable => Self::generate_unreachable(code)?,
            _ => todo!(),
        }

        self.class.finish_method(method_builder)?;
        Ok(true)
    }

    fn generate_i32_div_s<B: CodeBuilderExts>(code: &mut B) -> Result<(), Error> {
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
        code.access_field(RefType::INTEGER_NAME, "MIN_VALUE", true)?;
        code.push_branch_instruction(BranchInstruction::IfICmp(
            OrdComparison::NE,
            regular_div,
            (),
        ))?;

        let cls_idx = code.get_class_idx(&RefType::ARITHMETIC_CLASS)?;
        code.push_instruction(Instruction::New(cls_idx))?;
        code.push_instruction(Instruction::Dup)?;
        code.const_string("integer overflow")?;
        code.invoke(RefType::ARITHMETIC_NAME, "<init>")?;
        code.push_branch_instruction(BranchInstruction::AThrow)?;

        // This is the usual path: where we aren't dividing `Integer.MIN_VALUE` by `-1`
        code.place_label(regular_div)?;
        code.push_instruction(Instruction::ILoad(0))?;
        code.push_instruction(Instruction::ILoad(1))?;
        code.push_instruction(Instruction::IDiv)?;
        code.push_branch_instruction(BranchInstruction::IReturn)?;

        Ok(())
    }

    fn generate_i64_div_s<B: CodeBuilderExts>(code: &mut B) -> Result<(), Error> {
        let regular_div = code.fresh_label();

        // Check if second argument is -1...
        code.push_instruction(Instruction::LLoad(2))?;
        code.push_instruction(Instruction::IConstM1)?;
        code.push_instruction(Instruction::I2L)?;
        code.push_instruction(Instruction::LCmp)?;
        code.push_branch_instruction(BranchInstruction::If(OrdComparison::NE, regular_div, ()))?;

        // Check if first argument is `Long.MIN_VALUE`
        code.push_instruction(Instruction::LLoad(0))?;
        code.access_field(RefType::LONG_NAME, "MIN_VALUE", true)?;
        code.push_instruction(Instruction::LCmp)?;
        code.push_branch_instruction(BranchInstruction::If(OrdComparison::NE, regular_div, ()))?;

        let cls_idx = code.get_class_idx(&RefType::ARITHMETIC_CLASS)?;
        code.push_instruction(Instruction::New(cls_idx))?;
        code.push_instruction(Instruction::Dup)?;
        code.const_string("integer overflow")?;
        code.invoke(RefType::ARITHMETIC_NAME, "<init>")?;
        code.push_branch_instruction(BranchInstruction::AThrow)?;

        // This is the usual path: where we aren't dividing `Long.MIN_VALUE` by `-1`
        code.place_label(regular_div)?;
        code.push_instruction(Instruction::LLoad(0))?;
        code.push_instruction(Instruction::LLoad(2))?;
        code.push_instruction(Instruction::LDiv)?;
        code.push_branch_instruction(BranchInstruction::LReturn)?;

        Ok(())
    }

    fn generate_unreachable<B: CodeBuilderExts>(code: &mut B) -> Result<(), Error> {
        let cls_idx = code.get_class_idx(&RefType::ASSERTION_CLASS)?;
        code.push_instruction(Instruction::New(cls_idx))?;
        code.push_instruction(Instruction::Dup)?;
        code.invoke(RefType::ASSERTION_NAME, "<init>")?;
        code.push_branch_instruction(BranchInstruction::AReturn)?;

        Ok(())
    }
}
