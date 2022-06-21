use super::{AccessMode, CodeBuilderExts, Error, Memory, Settings, Table, UtilitiesStrategy};
use crate::jvm::{
    BaseType, BinaryName, BootstrapMethod, BootstrapMethods, BranchInstruction, ClassAccessFlags,
    ClassBuilder, ClassGraph, CompareMode, ConstantIndex, ConstantsPool, Descriptor, FieldType,
    HandleKind, InnerClass, InnerClassAccessFlags, InnerClasses, Instruction, InvokeType,
    MethodAccessFlags, MethodDescriptor, Name, OrdComparison, RefType, ShiftType, UnqualifiedName,
};
use std::collections::{HashMap, HashSet};

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

    /// Compute the next size of a table or memory (or -1 if it exceeds a limit)
    NextSize,

    /// Copy an array into a bigger array and fill the rest of the entries with a default entry.
    /// Return the length of the smaller array.
    CopyResizedArray,

    /// Copy a bytebuffer into a bigger bytebuffer. Return the size of the smaller bytebuffer, in
    /// units of memory pages.
    CopyResizedByteBuffer,

    /// Return true if the input is equal to negative one
    IntIsNegativeOne,

    /// Fill a range of an object array
    FillArrayRange,

    /// Fill a range of a bytebuffer
    FillByteBufferRange,

    /// Convert a number of bytes to a number of memory pages
    BytesToMemoryPages,

    /// Convert a number of memory pages into bytes
    MemoryPagesToBytes,

    /// Bootstrap method for table utilities
    BootstrapTable,

    /// Bootstrap method for memory utilities
    BootstrapMemory,
}
impl UtilityMethod {
    /// Get the method name
    pub const fn name(&self) -> UnqualifiedName {
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
            UtilityMethod::NextSize => UnqualifiedName::NEXTSIZE,
            UtilityMethod::CopyResizedArray => UnqualifiedName::COPYRESIZEDARRAY,
            UtilityMethod::CopyResizedByteBuffer => UnqualifiedName::COPYRESIZEDBYTEBUFFER,
            UtilityMethod::IntIsNegativeOne => UnqualifiedName::INTISNEGATIVEONE,
            UtilityMethod::FillArrayRange => UnqualifiedName::FILLARRAYRANGE,
            UtilityMethod::FillByteBufferRange => UnqualifiedName::FILLBYTEBUFFERRANGE,
            UtilityMethod::BytesToMemoryPages => UnqualifiedName::BYTESTOPAGES,
            UtilityMethod::MemoryPagesToBytes => UnqualifiedName::PAGESTOBYTES,
            UtilityMethod::BootstrapTable => UnqualifiedName::BOOTSTRAPTABLE,
            UtilityMethod::BootstrapMemory => UnqualifiedName::BOOTSTRAPMEMORY,
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
            UtilityMethod::NextSize => MethodDescriptor {
                parameters: vec![FieldType::INT, FieldType::INT, FieldType::LONG],
                return_type: Some(FieldType::INT),
            },
            UtilityMethod::CopyResizedArray => MethodDescriptor {
                parameters: vec![
                    FieldType::array(FieldType::OBJECT), // new bigger array
                    FieldType::array(FieldType::OBJECT), // old array
                    FieldType::OBJECT,                   // filler value for extra slots
                ],
                return_type: Some(FieldType::INT), // old size
            },
            UtilityMethod::CopyResizedByteBuffer => MethodDescriptor {
                parameters: vec![
                    FieldType::object(BinaryName::BYTEBUFFER), // new bigger bytebuffer
                    FieldType::object(BinaryName::BYTEBUFFER), // old bytebuffer
                ],
                return_type: Some(FieldType::INT), // old size (in memory pages)
            },
            UtilityMethod::IntIsNegativeOne => MethodDescriptor {
                parameters: vec![FieldType::INT],
                return_type: Some(FieldType::BOOLEAN),
            },
            UtilityMethod::FillArrayRange => MethodDescriptor {
                parameters: vec![
                    FieldType::INT,    // start index (inclusive)
                    FieldType::OBJECT, // filler value
                    FieldType::INT,    // how many entries to fill
                    FieldType::array(FieldType::OBJECT),
                ],
                return_type: None,
            },
            UtilityMethod::FillByteBufferRange => MethodDescriptor {
                parameters: vec![
                    FieldType::INT, // start index (inclusive)
                    FieldType::INT, // filler value (as byte)
                    FieldType::INT, // how many entries to fill
                    FieldType::Ref(RefType::Object(BinaryName::BYTEBUFFER)),
                ],
                return_type: None,
            },
            UtilityMethod::BytesToMemoryPages => MethodDescriptor {
                parameters: vec![FieldType::INT],
                return_type: Some(FieldType::INT),
            },
            UtilityMethod::MemoryPagesToBytes => MethodDescriptor {
                parameters: vec![FieldType::INT],
                return_type: Some(FieldType::INT),
            },
            UtilityMethod::BootstrapTable => MethodDescriptor {
                parameters: vec![
                    FieldType::Ref(RefType::Object(BinaryName::METHODHANDLES_LOOKUP)),
                    FieldType::Ref(RefType::STRING),
                    FieldType::Ref(RefType::METHODTYPE),
                    FieldType::Ref(RefType::METHODHANDLE), // getter
                    FieldType::Ref(RefType::METHODHANDLE), // setter
                    FieldType::LONG,                       // maximum table size
                ],
                return_type: Some(FieldType::Ref(RefType::Object(
                    BinaryName::CONSTANTCALLSITE,
                ))),
            },
            UtilityMethod::BootstrapMemory => MethodDescriptor {
                parameters: vec![
                    FieldType::Ref(RefType::Object(BinaryName::METHODHANDLES_LOOKUP)),
                    FieldType::Ref(RefType::STRING),
                    FieldType::Ref(RefType::METHODTYPE),
                    FieldType::Ref(RefType::METHODHANDLE), // getter
                    FieldType::Ref(RefType::METHODHANDLE), // setter
                    FieldType::LONG,                       // maximum memory size
                                                           // FieldType::BOOLEAN,                    // is shared
                ],
                return_type: Some(FieldType::Ref(RefType::Object(
                    BinaryName::CONSTANTCALLSITE,
                ))),
            },
        }
    }
}

/// Class that serves a shared carrier of utility methods. In the name of keeping the translation
/// outputs lean, these features are enumerated so that they can be requested then generated only
/// on demand.
enum UtilityClassInner<'g> {
    /// Use an external class with this name
    External(BinaryName),

    /// Generate an internal utility class
    Internal {
        /// Builder for the inner class
        class: ClassBuilder<'g>,

        /// Set of the utility methods that have already been generated
        methods: HashSet<UtilityMethod>,
    },
}

pub struct UtilityClass<'g>(UtilityClassInner<'g>);

impl<'g> UtilityClass<'g> {
    pub fn new(
        settings: &Settings,
        class_graph: &'g ClassGraph,
    ) -> Result<UtilityClass<'g>, Error> {
        // TODO: generate_all
        let (inner_class_short_name, _generate_all) = match &settings.utilities_strategy {
            UtilitiesStrategy::ReferenceExisting(external) => {
                let inner = UtilityClassInner::External(external.clone());
                return Ok(UtilityClass(inner));
            }
            UtilitiesStrategy::GenerateNested {
                inner_class,
                generate_all,
            } => (inner_class, generate_all),
        };

        let class_name = settings
            .output_full_class_name
            .concat(&UnqualifiedName::DOLLAR)
            .concat(&inner_class_short_name);

        let class = ClassBuilder::new(
            ClassAccessFlags::SYNTHETIC,
            class_name,
            BinaryName::OBJECT,
            false,
            vec![],
            class_graph,
        )?;

        // Add the `InnerClasses` attribute
        let inner_classes: InnerClasses = {
            let constants = &class.constants_pool;
            let outer_class_name = constants.get_utf8(settings.output_full_class_name.as_str())?;
            let outer_class = constants.get_class(outer_class_name)?;
            let inner_class_name = constants.get_utf8(class.class_name().as_str())?;
            let inner_class = constants.get_class(inner_class_name)?;
            let inner_name = constants.get_utf8(inner_class_short_name.as_str())?;
            let inner_class_attr = InnerClass {
                inner_class,
                outer_class,
                inner_name,
                access_flags: InnerClassAccessFlags::STATIC,
            };
            InnerClasses(vec![inner_class_attr])
        };
        class.add_attribute(inner_classes)?;

        Ok(UtilityClass(UtilityClassInner::Internal {
            class,
            methods: HashSet::new(),
        }))
    }

    /// Extract the class name
    pub fn class_name(&self) -> &BinaryName {
        match &self.0 {
            UtilityClassInner::External(name) => name,
            UtilityClassInner::Internal { class, .. } => class.class_name(),
        }
    }

    /// If there is a class being built, finalize and return it
    pub fn into_builder(self) -> Option<ClassBuilder<'g>> {
        match self.0 {
            UtilityClassInner::External(_) => None,
            UtilityClassInner::Internal { class, .. } => Some(class),
        }
    }

    /// Ensure the utility is defined, then call it on the specified code builder
    pub fn invoke_utility<B: CodeBuilderExts>(
        &mut self,
        method: UtilityMethod,
        code: &mut B,
    ) -> Result<(), Error> {
        let _ = self.add_utility_method(method)?;
        let class_name = self.class_name();
        let method_name = method.name();
        code.invoke_explicit(
            InvokeType::Static,
            class_name,
            &method_name,
            &method.descriptor(),
        )?;
        Ok(())
    }

    /// Add a utility method and return if it was already there
    pub fn add_utility_method(&mut self, method: UtilityMethod) -> Result<bool, Error> {
        // Nothing for external utility classes or if the method is already generated
        match &mut self.0 {
            UtilityClassInner::External(_) => return Ok(false),
            UtilityClassInner::Internal { methods, .. } => {
                if !methods.insert(method) {
                    return Ok(false);
                }
            }
        }

        // Dependencies
        match method {
            UtilityMethod::BootstrapTable => {
                self.add_utility_method(UtilityMethod::NextSize)?;
                self.add_utility_method(UtilityMethod::CopyResizedArray)?;
                self.add_utility_method(UtilityMethod::IntIsNegativeOne)?;
                self.add_utility_method(UtilityMethod::FillArrayRange)?;
            }
            UtilityMethod::BootstrapMemory => {
                self.add_utility_method(UtilityMethod::NextSize)?;
                self.add_utility_method(UtilityMethod::CopyResizedByteBuffer)?;
                self.add_utility_method(UtilityMethod::IntIsNegativeOne)?;
                self.add_utility_method(UtilityMethod::FillByteBufferRange)?;
                self.add_utility_method(UtilityMethod::BytesToMemoryPages)?;
                self.add_utility_method(UtilityMethod::MemoryPagesToBytes)?;
            }
            _ => (),
        }

        let descriptor = method.descriptor();
        let class: &mut ClassBuilder = match &mut self.0 {
            UtilityClassInner::Internal { class, .. } => class,
            _ => unreachable!("external utility classes should be filtered earlier"),
        };
        let mut method_builder =
            class.start_method(MethodAccessFlags::STATIC, method.name(), descriptor)?;
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
            UtilityMethod::NextSize => Self::generate_next_size(code)?,
            UtilityMethod::CopyResizedArray => Self::generate_copy_resized_array(code)?,
            UtilityMethod::CopyResizedByteBuffer => Self::generate_copy_resized_bytebuffer(code)?,
            UtilityMethod::IntIsNegativeOne => Self::generate_int_is_negative_one(code)?,
            UtilityMethod::FillArrayRange => Self::generate_fill_array_range(code)?,
            UtilityMethod::FillByteBufferRange => Self::generate_fill_bytebuffer_range(code)?,
            UtilityMethod::BytesToMemoryPages => Self::generate_bytes_to_memory_pages(code)?,
            UtilityMethod::MemoryPagesToBytes => Self::generate_memory_pages_to_bytes(code)?,

            UtilityMethod::BootstrapTable => {
                Self::generate_bootstrap_table(code, &class.class_name())?
            }
            UtilityMethod::BootstrapMemory => {
                Self::generate_bootstrap_memory(code, &class.class_name())?
            }
        }

        method_builder.finish()?;
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
        code.access_field(
            &BinaryName::INTEGER,
            &UnqualifiedName::MINVALUE,
            AccessMode::Read,
        )?;
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
        code.access_field(
            &BinaryName::LONG,
            &UnqualifiedName::MINVALUE,
            AccessMode::Read,
        )?;
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

    fn generate_f32_abs<B: CodeBuilderExts>(code: &mut B) -> Result<(), Error> {
        code.push_instruction(Instruction::FLoad(0))?;
        code.invoke(&BinaryName::FLOAT, &UnqualifiedName::FLOATTORAWINTBITS)?;
        code.const_int(0x7FFF_FFFF)?;
        code.push_instruction(Instruction::IAnd)?;
        code.invoke(&BinaryName::FLOAT, &UnqualifiedName::INTBITSTOFLOAT)?;
        code.push_branch_instruction(BranchInstruction::FReturn)?;

        Ok(())
    }

    fn generate_f64_abs<B: CodeBuilderExts>(code: &mut B) -> Result<(), Error> {
        code.push_instruction(Instruction::DLoad(0))?;
        code.invoke(&BinaryName::DOUBLE, &UnqualifiedName::DOUBLETORAWLONGBITS)?;
        code.const_long(0x7FFF_FFFF_FFFF_FFFF)?;
        code.push_instruction(Instruction::LAnd)?;
        code.invoke(&BinaryName::DOUBLE, &UnqualifiedName::LONGBITSTODOUBLE)?;
        code.push_branch_instruction(BranchInstruction::DReturn)?;

        Ok(())
    }

    fn generate_f32_trunc<B: CodeBuilderExts>(code: &mut B) -> Result<(), Error> {
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

    fn generate_f64_trunc<B: CodeBuilderExts>(code: &mut B) -> Result<(), Error> {
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

    fn generate_unreachable<B: CodeBuilderExts>(code: &mut B) -> Result<(), Error> {
        let cls_idx = code.get_class_idx(&RefType::ASSERTIONERROR)?;
        code.push_instruction(Instruction::New(cls_idx))?;
        code.push_instruction(Instruction::Dup)?;
        code.const_string("unreachable")?;
        code.invoke(&BinaryName::ASSERTIONERROR, &UnqualifiedName::INIT)?;
        code.push_branch_instruction(BranchInstruction::AReturn)?;

        Ok(())
    }

    fn generate_i32_trunc_f32_s<B: CodeBuilderExts>(code: &mut B) -> Result<(), Error> {
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

    fn generate_i32_trunc_f32_u<B: CodeBuilderExts>(code: &mut B) -> Result<(), Error> {
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

    fn generate_i32_trunc_f64_s<B: CodeBuilderExts>(code: &mut B) -> Result<(), Error> {
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

    fn generate_i32_trunc_f64_u<B: CodeBuilderExts>(code: &mut B) -> Result<(), Error> {
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

    fn generate_i64_trunc_f32_s<B: CodeBuilderExts>(code: &mut B) -> Result<(), Error> {
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

    fn generate_i64_trunc_f32_u<B: CodeBuilderExts>(code: &mut B) -> Result<(), Error> {
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

    fn generate_i64_trunc_f64_s<B: CodeBuilderExts>(code: &mut B) -> Result<(), Error> {
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

    fn generate_i64_trunc_f64_u<B: CodeBuilderExts>(code: &mut B) -> Result<(), Error> {
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

    fn generate_i64_extend_i32_u<B: CodeBuilderExts>(code: &mut B) -> Result<(), Error> {
        code.push_instruction(Instruction::ILoad(0))?;
        code.push_instruction(Instruction::I2L)?;
        code.const_long(0x0000_0000_ffff_ffff)?;
        code.push_instruction(Instruction::LAnd)?;
        code.push_branch_instruction(BranchInstruction::LReturn)?;

        Ok(())
    }

    fn generate_f32_convert_i32_u<B: CodeBuilderExts>(code: &mut B) -> Result<(), Error> {
        code.push_instruction(Instruction::ILoad(0))?;
        code.push_instruction(Instruction::I2L)?;
        code.const_long(0x0000_0000_ffff_ffff)?;
        code.push_instruction(Instruction::LAnd)?;
        code.push_instruction(Instruction::L2F)?;
        code.push_branch_instruction(BranchInstruction::FReturn)?;

        Ok(())
    }

    fn generate_f32_convert_i64_u<B: CodeBuilderExts>(code: &mut B) -> Result<(), Error> {
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

    fn generate_f64_convert_i32_u<B: CodeBuilderExts>(code: &mut B) -> Result<(), Error> {
        code.push_instruction(Instruction::ILoad(0))?;
        code.push_instruction(Instruction::I2L)?;
        code.const_long(0x0000_0000_ffff_ffff)?;
        code.push_instruction(Instruction::LAnd)?;
        code.push_instruction(Instruction::L2D)?;
        code.push_branch_instruction(BranchInstruction::DReturn)?;

        Ok(())
    }

    fn generate_f64_convert_i64_u<B: CodeBuilderExts>(code: &mut B) -> Result<(), Error> {
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

    fn generate_i32_trunc_sat_f32_u<B: CodeBuilderExts>(code: &mut B) -> Result<(), Error> {
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

    fn generate_i32_trunc_sat_f64_u<B: CodeBuilderExts>(code: &mut B) -> Result<(), Error> {
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

    fn generate_i64_trunc_sat_f32_u<B: CodeBuilderExts>(code: &mut B) -> Result<(), Error> {
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

    fn generate_i64_trunc_sat_f64_u<B: CodeBuilderExts>(code: &mut B) -> Result<(), Error> {
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

    /// Helper method for checking `grow` instructions
    ///
    /// Analagous to
    ///
    /// ```java
    /// static int nextSize(int currSize, int growBy, long maxSize) {
    ///   if (growBy < 0) return -1;
    ///   long proposed = (long) currSize + (long) growBy;
    ///   if (proposed > maxSize) return -1;
    ///   return (int) proposed;
    /// }
    /// ```
    fn generate_next_size<B: CodeBuilderExts>(code: &mut B) -> Result<(), Error> {
        let curr_size_argument = 0;
        let grow_by_argument = 1;
        let max_size_argument = 2;

        let ok_case1 = code.fresh_label();
        let ok_case2 = code.fresh_label();

        // if (growBy < 0) return -1;
        code.push_instruction(Instruction::ILoad(grow_by_argument))?;
        code.push_branch_instruction(BranchInstruction::If(OrdComparison::GE, ok_case1, ()))?;
        code.push_instruction(Instruction::IConstM1)?;
        code.push_branch_instruction(BranchInstruction::IReturn)?;
        code.place_label(ok_case1)?;

        // long proposed = (long) currSize + (long) growBy;
        code.push_instruction(Instruction::ILoad(curr_size_argument))?;
        code.push_instruction(Instruction::I2L)?;
        code.push_instruction(Instruction::ILoad(grow_by_argument))?;
        code.push_instruction(Instruction::I2L)?;
        code.push_instruction(Instruction::LAdd)?;

        // if (proposed >= maxSize) return -1;
        code.push_instruction(Instruction::Dup2)?;
        code.push_instruction(Instruction::LLoad(max_size_argument))?;
        code.push_instruction(Instruction::LCmp)?;
        code.push_branch_instruction(BranchInstruction::If(OrdComparison::LE, ok_case2, ()))?;
        code.push_instruction(Instruction::IConstM1)?;
        code.push_branch_instruction(BranchInstruction::IReturn)?;

        //  return (int) proposed;
        code.place_label(ok_case2)?;
        code.push_instruction(Instruction::L2I)?;
        code.push_branch_instruction(BranchInstruction::IReturn)?;

        Ok(())
    }

    /// Helper method for copying old data into resized new tables
    ///
    /// Analagous to
    ///
    /// ```java
    /// static int copyResizedArray(Object[] newTable, Object[] oldTable, Object filler) {
    ///   System.arraycopy(oldTable, 0, newTable, 0, oldTable.length);
    ///   Arrays.fill(newTable, oldTable.length, newTable.length, filler);
    ///   return oldTable.length;
    /// }
    /// ```
    fn generate_copy_resized_array<B: CodeBuilderExts>(code: &mut B) -> Result<(), Error> {
        let new_table_argument = 0;
        let old_table_argument = 1;
        let filler_argument = 2;

        // System.arraycopy(oldTable, 0, newTable, 0, oldTable.length);
        code.push_instruction(Instruction::ALoad(old_table_argument))?;
        code.push_instruction(Instruction::IConst0)?;
        code.push_instruction(Instruction::ALoad(new_table_argument))?;
        code.push_instruction(Instruction::IConst0)?;
        code.push_instruction(Instruction::ALoad(old_table_argument))?;
        code.push_instruction(Instruction::ArrayLength)?;
        code.invoke(&BinaryName::SYSTEM, &UnqualifiedName::ARRAYCOPY)?;

        // Arrays.fill(newTable, oldTable.length, newTable.length, filler);
        code.push_instruction(Instruction::ALoad(new_table_argument))?;
        code.push_instruction(Instruction::ALoad(old_table_argument))?;
        code.push_instruction(Instruction::ArrayLength)?;
        code.push_instruction(Instruction::ALoad(new_table_argument))?;
        code.push_instruction(Instruction::ArrayLength)?;
        code.push_instruction(Instruction::ALoad(filler_argument))?;
        code.invoke(&BinaryName::ARRAYS, &UnqualifiedName::FILL)?;

        // return oldTable.length;
        code.push_instruction(Instruction::ALoad(old_table_argument))?;
        code.push_instruction(Instruction::ArrayLength)?;
        code.push_branch_instruction(BranchInstruction::IReturn)?;

        Ok(())
    }

    /// Helper method for copying old data into resized new memories
    ///
    /// Analagous to
    ///
    /// ```java
    /// static int copyResizedByteBuffer(ByteBuffer newMemory, ByteBuffer oldMemory) {
    ///   oldMemory.position(0);
    ///   newMemory.put(oldMemory);
    ///   return oldMemory.capacity() / 65536;
    /// }
    /// ```
    fn generate_copy_resized_bytebuffer<B: CodeBuilderExts>(code: &mut B) -> Result<(), Error> {
        let new_memory_argument = 0;
        let old_memory_argument = 1;

        // oldMemory.position(0);
        code.push_instruction(Instruction::ALoad(old_memory_argument))?;
        code.push_instruction(Instruction::IConst0)?;
        code.invoke(&BinaryName::BYTEBUFFER, &UnqualifiedName::POSITION)?;
        code.push_instruction(Instruction::Pop)?;

        // newMemory.put(oldMemory);
        code.push_instruction(Instruction::ALoad(new_memory_argument))?;
        code.push_instruction(Instruction::ALoad(old_memory_argument))?;
        code.invoke_explicit(
            InvokeType::Virtual,
            &BinaryName::BYTEBUFFER,
            &UnqualifiedName::PUT,
            &MethodDescriptor {
                parameters: vec![FieldType::object(BinaryName::BYTEBUFFER)],
                return_type: Some(FieldType::object(BinaryName::BYTEBUFFER)),
            },
        )?;

        // return oldMemory.capacity() / 65536;
        code.push_instruction(Instruction::ALoad(old_memory_argument))?;
        code.invoke(&BinaryName::BYTEBUFFER, &UnqualifiedName::CAPACITY)?;
        code.const_int(16)?;
        code.push_instruction(Instruction::ISh(ShiftType::ArithmeticRight))?;
        code.push_branch_instruction(BranchInstruction::IReturn)?;

        Ok(())
    }

    /// Helper method for checking if a value is equal to negative 1
    ///
    /// Analagous to
    ///
    /// ```java
    /// static boolean intIsNegativeOne(int i) {
    ///   return (i == -1) ? true : false;
    /// }
    /// ```
    fn generate_int_is_negative_one<B: CodeBuilderExts>(code: &mut B) -> Result<(), Error> {
        let not_equal = code.fresh_label();

        code.push_instruction(Instruction::ILoad(0))?;
        code.push_instruction(Instruction::IConstM1)?;
        code.push_branch_instruction(BranchInstruction::IfICmp(OrdComparison::NE, not_equal, ()))?;
        code.push_instruction(Instruction::IConst1)?;
        code.push_branch_instruction(BranchInstruction::IReturn)?;

        code.place_label(not_equal)?;
        code.push_instruction(Instruction::IConst0)?;
        code.push_branch_instruction(BranchInstruction::IReturn)?;

        Ok(())
    }

    /// Helper method for filling a range of values in an array
    ///
    /// Analagous to
    ///
    /// ```java
    /// static void fillArrayRange(int from, Object filler, int numToFill, Object[] arr) {
    ///   java.util.Arrays.fill(arr, from, Math.addExact(from, numToFill), filler);
    /// }
    /// ```
    fn generate_fill_array_range<B: CodeBuilderExts>(code: &mut B) -> Result<(), Error> {
        code.push_instruction(Instruction::ALoad(3))?;
        code.push_instruction(Instruction::ILoad(0))?;
        code.push_instruction(Instruction::ILoad(0))?;
        code.push_instruction(Instruction::ILoad(2))?;
        code.invoke(&BinaryName::MATH, &UnqualifiedName::ADDEXACT)?;
        code.push_instruction(Instruction::ALoad(1))?;
        code.invoke(&BinaryName::ARRAYS, &UnqualifiedName::FILL)?;
        code.push_branch_instruction(BranchInstruction::Return)?;

        Ok(())
    }

    /// Helper method for filling a range of bytes in a bytebuffer
    ///
    /// Analagous to
    ///
    /// ```java
    /// static void fillByteBufferRange(int from, int filler, int numToFill, ByteBuffer buf) {
    ///   if (numToFill < 0) {
    ///     throw new IllegalArgumentException("memory.fill: negative number of bytes");
    ///   }
    ///   buf.position(from);
    ///   byte fillerByte = (byte) filler;
    ///   while (numToFill > 0) {
    ///     buf.put(fillerByte);
    ///     numToFill--;
    ///   }
    /// }
    /// ```
    fn generate_fill_bytebuffer_range<B: CodeBuilderExts>(code: &mut B) -> Result<(), Error> {
        let ok_case = code.fresh_label();

        // if (numToFill < 0) {
        code.push_instruction(Instruction::ILoad(2))?;
        code.push_branch_instruction(BranchInstruction::If(OrdComparison::GE, ok_case, ()))?;

        // throw new IllegalArgumentException("memory.fill: negative number of bytes");
        let cls_idx = code.get_class_idx(&RefType::Object(BinaryName::ILLEGALARGUMENTEXCEPTION))?;
        code.push_instruction(Instruction::New(cls_idx))?;
        code.push_instruction(Instruction::Dup)?;
        code.const_string("memory.fill: negative number of bytes")?;
        code.invoke(
            &BinaryName::ILLEGALARGUMENTEXCEPTION,
            &UnqualifiedName::INIT,
        )?;
        code.push_branch_instruction(BranchInstruction::AThrow)?;
        code.place_label(ok_case)?;

        // buf.position(from);
        code.push_instruction(Instruction::ALoad(3))?;
        code.push_instruction(Instruction::ILoad(0))?;
        code.invoke(&BinaryName::BYTEBUFFER, &UnqualifiedName::POSITION)?;
        code.push_instruction(Instruction::Pop)?;

        // byte fillerByte = (byte) filler;
        code.push_instruction(Instruction::ILoad(1))?;
        code.push_instruction(Instruction::I2B)?;
        code.push_instruction(Instruction::IStore(1))?;

        let loop_entry = code.fresh_label();
        let loop_exit = code.fresh_label();

        // while (numToFill > 0) {
        code.place_label(loop_entry)?;
        code.push_instruction(Instruction::ILoad(2))?;
        code.push_branch_instruction(BranchInstruction::If(OrdComparison::LE, loop_exit, ()))?;

        // buf.put(fillerByte);
        code.push_instruction(Instruction::ALoad(3))?;
        code.push_instruction(Instruction::ILoad(1))?;
        code.invoke_explicit(
            InvokeType::Virtual,
            &BinaryName::BYTEBUFFER,
            &UnqualifiedName::PUT,
            &MethodDescriptor {
                parameters: vec![FieldType::BYTE],
                return_type: Some(FieldType::object(BinaryName::BYTEBUFFER)),
            },
        )?;
        code.push_instruction(Instruction::Pop)?;

        // numToFill--;
        code.push_instruction(Instruction::IInc(2, -1))?;

        code.push_branch_instruction(BranchInstruction::Goto(loop_entry))?;
        code.place_label(loop_exit)?;
        code.push_branch_instruction(BranchInstruction::Return)?;

        Ok(())
    }

    /// Helper method for converting a number of bytes into a number of memory pages. This assumes
    /// that the bytes are a multiple of the memory page size.
    ///
    /// Analagous to
    ///
    /// ```java
    /// static int bytesToMemoryPages(int byteCount) {
    ///   return byteCount / 65536;
    /// }
    /// ```
    fn generate_bytes_to_memory_pages<B: CodeBuilderExts>(code: &mut B) -> Result<(), Error> {
        code.push_instruction(Instruction::ILoad(0))?;
        code.const_int(16)?;
        code.push_instruction(Instruction::ISh(ShiftType::ArithmeticRight))?;
        code.push_branch_instruction(BranchInstruction::IReturn)?;

        Ok(())
    }

    /// Helper method for converting a number of memory pages into a number of bytes. This assumes
    /// that the number of pages is small enough for bytes to not overflow `int`.
    ///
    /// Analagous to
    ///
    /// ```java
    /// static int memoryPagesToBytes(int memoryPages) {
    ///   return memoryPages * 65536;
    /// }
    /// ```
    fn generate_memory_pages_to_bytes<B: CodeBuilderExts>(code: &mut B) -> Result<(), Error> {
        code.push_instruction(Instruction::ILoad(0))?;
        code.const_int(16)?;
        code.push_instruction(Instruction::ISh(ShiftType::Left))?;
        code.push_branch_instruction(BranchInstruction::IReturn)?;

        Ok(())
    }

    /// Generate the bootstrap method used for table operators, including indirect calls. Here lie
    /// some dragons. The output is sensible, but the "how" is not obvious.
    fn generate_bootstrap_table<B: CodeBuilderExts>(
        code: &mut B,
        utility_class_name: &BinaryName,
    ) -> Result<(), Error> {
        let call_indirect_case = code.fresh_label();
        let table_get_case = code.fresh_label();
        let table_set_case = code.fresh_label();
        let table_size_case = code.fresh_label();
        let table_grow_case = code.fresh_label();
        let table_fill_case = code.fresh_label();
        let table_copy_case = code.fresh_label();
        let table_init_case = code.fresh_label();
        let bad_name_case = code.fresh_label();

        code.push_instruction(Instruction::ALoad(1))?;
        code.invoke(&BinaryName::OBJECT, &UnqualifiedName::HASHCODE)?;
        code.push_branch_instruction(BranchInstruction::LookupSwitch {
            padding: 0,
            default: bad_name_case,
            targets: {
                let mut targets = vec![
                    (Self::java_hash_string(b"call_indirect"), call_indirect_case),
                    (Self::java_hash_string(b"table_get"), table_get_case),
                    (Self::java_hash_string(b"table_set"), table_set_case),
                    (Self::java_hash_string(b"table_size"), table_size_case),
                    (Self::java_hash_string(b"table_grow"), table_grow_case),
                    (Self::java_hash_string(b"table_fill"), table_fill_case),
                    (Self::java_hash_string(b"table_copy"), table_copy_case),
                    (Self::java_hash_string(b"table_init"), table_init_case),
                ];
                targets.sort_by_key(|(key, _)| *key);
                targets
            },
        })?;

        // call.indirect
        code.place_label(call_indirect_case)?;
        code.push_instruction(Instruction::ALoad(1))?;
        code.const_string("call_indirect")?;
        code.invoke(&BinaryName::OBJECT, &UnqualifiedName::EQUALS)?;
        code.push_branch_instruction(BranchInstruction::If(OrdComparison::EQ, bad_name_case, ()))?;
        Self::generate_call_indirect_table_case(code)?;

        // table.get
        code.place_label(table_get_case)?;
        code.push_instruction(Instruction::ALoad(1))?;
        code.const_string("table_get")?;
        code.invoke(&BinaryName::OBJECT, &UnqualifiedName::EQUALS)?;
        code.push_branch_instruction(BranchInstruction::If(OrdComparison::EQ, bad_name_case, ()))?;
        Self::generate_get_table_case(code)?;

        // table.set
        code.place_label(table_set_case)?;
        code.push_instruction(Instruction::ALoad(1))?;
        code.const_string("table_set")?;
        code.invoke(&BinaryName::OBJECT, &UnqualifiedName::EQUALS)?;
        code.push_branch_instruction(BranchInstruction::If(OrdComparison::EQ, bad_name_case, ()))?;
        Self::generate_set_table_case(code)?;

        // table.size
        code.place_label(table_size_case)?;
        code.push_instruction(Instruction::ALoad(1))?;
        code.const_string("table_size")?;
        code.invoke(&BinaryName::OBJECT, &UnqualifiedName::EQUALS)?;
        code.push_branch_instruction(BranchInstruction::If(OrdComparison::EQ, bad_name_case, ()))?;
        Self::generate_size_table_case(code)?;

        // table.grow
        code.place_label(table_grow_case)?;
        code.push_instruction(Instruction::ALoad(1))?;
        code.const_string("table_grow")?;
        code.invoke(&BinaryName::OBJECT, &UnqualifiedName::EQUALS)?;
        code.push_branch_instruction(BranchInstruction::If(OrdComparison::EQ, bad_name_case, ()))?;
        Self::generate_grow_table_case(code, utility_class_name)?;

        // table.fill
        code.place_label(table_fill_case)?;
        code.push_instruction(Instruction::ALoad(1))?;
        code.const_string("table_fill")?;
        code.invoke(&BinaryName::OBJECT, &UnqualifiedName::EQUALS)?;
        code.push_branch_instruction(BranchInstruction::If(OrdComparison::EQ, bad_name_case, ()))?;
        Self::generate_fill_table_case(code, utility_class_name)?;

        // table.copy
        code.place_label(table_copy_case)?;
        code.push_instruction(Instruction::ALoad(1))?;
        code.const_string("table_copy")?;
        code.invoke(&BinaryName::OBJECT, &UnqualifiedName::EQUALS)?;
        code.push_branch_instruction(BranchInstruction::If(OrdComparison::EQ, bad_name_case, ()))?;
        Self::generate_copy_table_case(code)?;

        // table.init
        code.place_label(table_init_case)?;
        code.push_instruction(Instruction::ALoad(1))?;
        code.const_string("table_init")?;
        code.invoke(&BinaryName::OBJECT, &UnqualifiedName::EQUALS)?;
        code.push_branch_instruction(BranchInstruction::If(OrdComparison::EQ, bad_name_case, ()))?;
        Self::generate_init_table_case(code)?;

        // Catch all case
        let cls_idx = code.get_class_idx(&RefType::Object(BinaryName::ILLEGALARGUMENTEXCEPTION))?;
        code.place_label(bad_name_case)?;
        code.push_instruction(Instruction::New(cls_idx))?;
        code.push_instruction(Instruction::Dup)?;
        code.push_instruction(Instruction::ALoad(1))?;
        code.invoke(
            &BinaryName::ILLEGALARGUMENTEXCEPTION,
            &UnqualifiedName::INIT,
        )?;
        code.push_branch_instruction(BranchInstruction::AThrow)?;

        Ok(())
    }

    /// For `call_indirect`, we need to take an index as input to choose which method in an array
    /// to call. We could do this by doing an array lookup into the `table` field and then calling
    /// `invokeExact` on the `MethodHandle` we extract from there however:
    ///
    ///   - that's a fair bit of duplicated code (or else an extra function call)
    ///   - there is something faster and that fits our needs better: `invokedynamic`!
    ///
    /// In order to leverage `invokedynamic`, we need a bootstrap method. This bootstrap method
    /// will be called with an expected type and return a callsite. We generate bytecode analagous
    /// to the following method:
    ///
    /// ```java
    /// import java.lang.invoke.*;
    ///
    /// static CallSite bootstrap(
    ///   MethodHandles.Lookup lookup,
    ///   String name,
    ///   MethodType type,                                // (A₀A₁..ILMyWasmModule;)R
    ///   MethodHandle getter,                            // (LMyWasmModule;)[LMethodHandle;
    ///   MethodHandle setter                             // (LMyWasmModule;[LMethodHandle)V
    /// ) throws Exception {
    ///
    ///   int paramCount = type.parameterCount();
    ///   MethodType targetType =                         // (A₀A₁..LMyWasmModule;)R
    ///     type.dropParameterTypes(paramCount - 2, paramCount - 1);
    ///
    ///   int[] permutation = new int[paramCount + 1];
    ///   permutation[0] = paramCount - 1;
    ///   permutation[1] = paramCount - 2;
    ///   for (int i = 2; i < paramCount; i++) {
    ///     permutation[i] = i - 2;
    ///   }
    ///   permutation[paramCount] = paramCount - 1;
    ///
    ///   MethodHandle handle =
    ///     MethodHandles.permuteArguments(               // (A₀A₁..ILMyWasmModule;)R
    ///       MethodHandles.collectArguments(             // (LMyWasmModule;IA₀A₁..LMyWasmModule;)R
    ///         MethodHandles.collectArguments(           // ([LMethodHandle;IA₀A₁..LMyWasmModule;)R
    ///           MethodHandles.exactInvoker(targetType), // (LMethodHandle;A₀A₁..LMyWasmModule;)R
    ///           0,
    ///           MethodHandles.arrayElementGetter(MethodHandle[].class)
    ///         ),
    ///         0,
    ///         getter
    ///       ),
    ///       type,
    ///       permutation
    ///     );
    ///
    ///   return new ConstantCallSite(handle);
    /// }
    /// ```
    fn generate_call_indirect_table_case<B: CodeBuilderExts>(code: &mut B) -> Result<(), Error> {
        let type_argument = 2;
        let getter_argument = 3;
        let param_count_local = 7;
        let permutation_local = 8;

        // int paramCount = type.parameterCount();
        // int[] permutation = new int[paramCount + 1];
        code.push_instruction(Instruction::ALoad(type_argument))?;
        code.invoke(&BinaryName::METHODTYPE, &UnqualifiedName::PARAMETERCOUNT)?;
        code.push_instruction(Instruction::Dup)?;
        code.push_instruction(Instruction::IStore(param_count_local))?;
        code.push_instruction(Instruction::IConst1)?;
        code.push_instruction(Instruction::IAdd)?;
        code.push_instruction(Instruction::NewArray(BaseType::Int))?;
        code.push_instruction(Instruction::AStore(permutation_local))?;

        // initialize `permutation[0]`
        code.push_instruction(Instruction::ALoad(permutation_local))?;
        code.push_instruction(Instruction::IConst0)?;
        code.push_instruction(Instruction::ILoad(param_count_local))?;
        code.push_instruction(Instruction::IConst1)?;
        code.push_instruction(Instruction::ISub)?;
        code.push_instruction(Instruction::IAStore)?;

        // initialize `permutation[1]`
        code.push_instruction(Instruction::ALoad(permutation_local))?;
        code.push_instruction(Instruction::IConst1)?;
        code.push_instruction(Instruction::ILoad(param_count_local))?;
        code.push_instruction(Instruction::IConst2)?;
        code.push_instruction(Instruction::ISub)?;
        code.push_instruction(Instruction::IAStore)?;

        // initialize `permutation[2]` until and including `permutation[paramCount - 1]`
        code.push_instruction(Instruction::ALoad(permutation_local))?;
        code.push_instruction(Instruction::IConst2)?;
        let loop_start = code.fresh_label();
        let loop_end = code.fresh_label();
        code.place_label(loop_start)?;
        code.push_instruction(Instruction::Dup)?;
        code.push_instruction(Instruction::ILoad(param_count_local))?;
        code.push_branch_instruction(BranchInstruction::IfICmp(OrdComparison::GE, loop_end, ()))?;
        code.push_instruction(Instruction::Dup2)?;
        code.push_instruction(Instruction::Dup)?;
        code.push_instruction(Instruction::IConst2)?;
        code.push_instruction(Instruction::ISub)?;
        code.push_instruction(Instruction::IAStore)?;
        code.push_instruction(Instruction::IConst1)?;
        code.push_instruction(Instruction::IAdd)?;
        code.push_branch_instruction(BranchInstruction::Goto(loop_start))?;
        code.place_label(loop_end)?;
        code.push_instruction(Instruction::Pop2)?;

        // initialize `permutation[paramCount] = paramCount - 1`
        code.push_instruction(Instruction::ALoad(permutation_local))?;
        code.push_instruction(Instruction::ILoad(param_count_local))?;
        code.push_instruction(Instruction::Dup)?;
        code.push_instruction(Instruction::IConst1)?;
        code.push_instruction(Instruction::ISub)?;
        code.push_instruction(Instruction::IAStore)?;

        // MethodType targetType = type.dropParameterTypes(paramCount - 2, paramCount - 1);
        // Stack after: [ .., targetType ]
        code.push_instruction(Instruction::ALoad(type_argument))?;
        code.push_instruction(Instruction::ILoad(param_count_local))?;
        code.push_instruction(Instruction::IConst2)?;
        code.push_instruction(Instruction::ISub)?;
        code.push_instruction(Instruction::ILoad(param_count_local))?;
        code.push_instruction(Instruction::IConst1)?;
        code.push_instruction(Instruction::ISub)?;
        code.invoke(
            &BinaryName::METHODTYPE,
            &UnqualifiedName::DROPPARAMETERTYPES,
        )?;

        /* MethodHandle handle = MethodHandles.permuteArguments(
         *   MethodHandles.collectArguments(
         *     MethodHandles.collectArguments(
         *       MethodHandles.exactInvoker(targetType),
         *       0,
         *       MethodHandles.arrayElementGetter(MethodHandle[].class)
         *     ),
         *     0,
         *     getter
         *   ),
         *   type,
         *   permutation
         * )
         * Stack after: [ .., methodhandle ]
         */
        code.invoke(&BinaryName::METHODHANDLES, &UnqualifiedName::EXACTINVOKER)?;
        code.push_instruction(Instruction::IConst0)?;
        code.const_class(&FieldType::array(FieldType::Ref(RefType::METHODHANDLE)))?;
        code.invoke(
            &BinaryName::METHODHANDLES,
            &UnqualifiedName::ARRAYELEMENTGETTER,
        )?;
        code.invoke(
            &BinaryName::METHODHANDLES,
            &UnqualifiedName::COLLECTARGUMENTS,
        )?;
        code.push_instruction(Instruction::IConst0)?;
        code.push_instruction(Instruction::ALoad(getter_argument))?;
        code.invoke(
            &BinaryName::METHODHANDLES,
            &UnqualifiedName::COLLECTARGUMENTS,
        )?;
        code.push_instruction(Instruction::ALoad(type_argument))?;
        code.push_instruction(Instruction::ALoad(permutation_local))?;
        code.invoke(
            &BinaryName::METHODHANDLES,
            &UnqualifiedName::PERMUTEARGUMENTS,
        )?;

        // return new ConstantCallSite(methodhandle);
        let constant_callsite_cls =
            code.get_class_idx(&RefType::Object(BinaryName::CONSTANTCALLSITE))?;
        code.push_instruction(Instruction::New(constant_callsite_cls))?;
        code.push_instruction(Instruction::DupX1)?;
        code.push_instruction(Instruction::Swap)?;
        code.invoke(&BinaryName::CONSTANTCALLSITE, &UnqualifiedName::INIT)?;
        code.push_branch_instruction(BranchInstruction::AReturn)?;

        Ok(())
    }

    fn generate_get_table_case<B: CodeBuilderExts>(code: &mut B) -> Result<(), Error> {
        let type_argument = 2;
        let getter_argument = 3;

        // Class<?> tableType = getter.type().returnType();
        code.push_instruction(Instruction::ALoad(getter_argument))?;
        code.invoke(&BinaryName::METHODHANDLE, &UnqualifiedName::TYPE)?;
        code.invoke(&BinaryName::METHODTYPE, &UnqualifiedName::RETURNTYPE)?;

        /* MethodHandles.permuteArguments(                  // (ILMyWasmModule;)LTableElem;
         *   MethodHandles.collectArguments(                // (LMyWasmModule;I)LTableElem;
         *     MethodHandles.arrayElementGetter(tableType), // ([LTableElem;I)LTableElem;
         *     0,
         *     getter
         *   ),
         *   type,
         *   new int[2] { 1, 0 }
         * )
         */
        code.invoke(
            &BinaryName::METHODHANDLES,
            &UnqualifiedName::ARRAYELEMENTGETTER,
        )?;
        code.push_instruction(Instruction::IConst0)?;
        code.push_instruction(Instruction::ALoad(getter_argument))?;
        code.invoke(
            &BinaryName::METHODHANDLES,
            &UnqualifiedName::COLLECTARGUMENTS,
        )?;
        code.push_instruction(Instruction::ALoad(type_argument))?;
        code.push_instruction(Instruction::IConst2)?;
        code.push_instruction(Instruction::NewArray(BaseType::Int))?;
        code.push_instruction(Instruction::Dup)?;
        code.push_instruction(Instruction::IConst0)?;
        code.push_instruction(Instruction::IConst1)?;
        code.push_instruction(Instruction::IAStore)?;
        code.invoke(
            &BinaryName::METHODHANDLES,
            &UnqualifiedName::PERMUTEARGUMENTS,
        )?;

        // return new ConstantCallSite(methodhandle);
        let constant_callsite_cls =
            code.get_class_idx(&RefType::Object(BinaryName::CONSTANTCALLSITE))?;
        code.push_instruction(Instruction::New(constant_callsite_cls))?;
        code.push_instruction(Instruction::DupX1)?;
        code.push_instruction(Instruction::Swap)?;
        code.invoke(&BinaryName::CONSTANTCALLSITE, &UnqualifiedName::INIT)?;
        code.push_branch_instruction(BranchInstruction::AReturn)?;

        Ok(())
    }

    fn generate_set_table_case<B: CodeBuilderExts>(code: &mut B) -> Result<(), Error> {
        let type_argument = 2;
        let getter_argument = 3;

        // Class<?> tableType = getter.type().returnType();
        code.push_instruction(Instruction::ALoad(getter_argument))?;
        code.invoke(&BinaryName::METHODHANDLE, &UnqualifiedName::TYPE)?;
        code.invoke(&BinaryName::METHODTYPE, &UnqualifiedName::RETURNTYPE)?;

        /* MethodHandles.permuteArguments(                  // (ILTableElem;LMyWasmModule;)V
         *   MethodHandles.collectArguments(                // (LMyWasmModule;ILTableElem;)V
         *     MethodHandles.arrayElementSetter(tableType), // ([LTableElem;ILTableElem;)V
         *     0,
         *     getter
         *   ),
         *   type,
         *   new int[3] { 2, 0, 1 }
         * )
         */
        code.invoke(
            &BinaryName::METHODHANDLES,
            &UnqualifiedName::ARRAYELEMENTSETTER,
        )?;
        code.push_instruction(Instruction::IConst0)?;
        code.push_instruction(Instruction::ALoad(getter_argument))?;
        code.invoke(
            &BinaryName::METHODHANDLES,
            &UnqualifiedName::COLLECTARGUMENTS,
        )?;
        code.push_instruction(Instruction::ALoad(type_argument))?;
        code.push_instruction(Instruction::IConst3)?;
        code.push_instruction(Instruction::NewArray(BaseType::Int))?;
        code.push_instruction(Instruction::Dup)?;
        code.push_instruction(Instruction::IConst0)?;
        code.push_instruction(Instruction::IConst2)?;
        code.push_instruction(Instruction::IAStore)?;
        code.push_instruction(Instruction::Dup)?;
        code.push_instruction(Instruction::IConst1)?;
        code.push_instruction(Instruction::IConst0)?;
        code.push_instruction(Instruction::IAStore)?;
        code.push_instruction(Instruction::Dup)?;
        code.push_instruction(Instruction::IConst2)?;
        code.push_instruction(Instruction::IConst1)?;
        code.push_instruction(Instruction::IAStore)?;
        code.invoke(
            &BinaryName::METHODHANDLES,
            &UnqualifiedName::PERMUTEARGUMENTS,
        )?;

        // return new ConstantCallSite(methodhandle);
        let constant_callsite_cls =
            code.get_class_idx(&RefType::Object(BinaryName::CONSTANTCALLSITE))?;
        code.push_instruction(Instruction::New(constant_callsite_cls))?;
        code.push_instruction(Instruction::DupX1)?;
        code.push_instruction(Instruction::Swap)?;
        code.invoke(&BinaryName::CONSTANTCALLSITE, &UnqualifiedName::INIT)?;
        code.push_branch_instruction(BranchInstruction::AReturn)?;

        Ok(())
    }

    fn generate_size_table_case<B: CodeBuilderExts>(code: &mut B) -> Result<(), Error> {
        let getter_argument = 3;

        // Class<?> tableType = getter.type().returnType();
        code.push_instruction(Instruction::ALoad(getter_argument))?;
        code.invoke(&BinaryName::METHODHANDLE, &UnqualifiedName::TYPE)?;
        code.invoke(&BinaryName::METHODTYPE, &UnqualifiedName::RETURNTYPE)?;

        /* MethodHandles.filterReturnValue(                 // (LMyWasmModule)I
         *   getter,                                        // (LMyWasmModule)[LTableElem;
         *   MethodHandles.arrayLength(tableType)           // ([LTableElem;)I
         * )
         */
        code.invoke(&BinaryName::METHODHANDLES, &UnqualifiedName::ARRAYLENGTH)?;
        code.push_instruction(Instruction::ALoad(getter_argument))?;
        code.push_instruction(Instruction::Swap)?;
        code.invoke(
            &BinaryName::METHODHANDLES,
            &UnqualifiedName::FILTERRETURNVALUE,
        )?;

        // return new ConstantCallSite(methodhandle);
        let constant_callsite_cls =
            code.get_class_idx(&RefType::Object(BinaryName::CONSTANTCALLSITE))?;
        code.push_instruction(Instruction::New(constant_callsite_cls))?;
        code.push_instruction(Instruction::DupX1)?;
        code.push_instruction(Instruction::Swap)?;
        code.invoke(&BinaryName::CONSTANTCALLSITE, &UnqualifiedName::INIT)?;
        code.push_branch_instruction(BranchInstruction::AReturn)?;

        Ok(())
    }

    // TODO: avoid allocating a new table for `table.grow 0`
    fn generate_grow_table_case<B: CodeBuilderExts>(
        code: &mut B,
        utility_class_name: &BinaryName,
    ) -> Result<(), Error> {
        let requested_type_argument = 2; // MethodType
        let getter_argument = 3; // MethodHandle
        let setter_argument = 4; // MethodHandle
        let max_size_argument = 5; // long
        let table_typ = 7; // Class<TableElem[]>
        let table_elem_typ = 8; // Class<TableElem>
        let module_typ = 9; // Class<WasmModule>
        let create_and_update_new_table = 10; // MethodHandle
        let cls_cls_idx = code.get_class_idx(&RefType::CLASS)?;

        // Class<?> tableType = getter.type().returnType();
        code.push_instruction(Instruction::ALoad(getter_argument))?;
        code.invoke(&BinaryName::METHODHANDLE, &UnqualifiedName::TYPE)?;
        code.invoke(&BinaryName::METHODTYPE, &UnqualifiedName::RETURNTYPE)?;
        code.push_instruction(Instruction::AStore(table_typ))?;

        // Class<?> tableElemType = methodType.parameterType(0);
        code.push_instruction(Instruction::ALoad(requested_type_argument))?;
        code.push_instruction(Instruction::IConst0)?;
        code.invoke(&BinaryName::METHODTYPE, &UnqualifiedName::PARAMETERTYPE)?;
        code.push_instruction(Instruction::AStore(table_elem_typ))?;

        // Class<?> moduleType = getter.type().parameterType(0);
        code.push_instruction(Instruction::ALoad(getter_argument))?;
        code.invoke(&BinaryName::METHODHANDLE, &UnqualifiedName::TYPE)?;
        code.push_instruction(Instruction::IConst0)?;
        code.invoke(&BinaryName::METHODTYPE, &UnqualifiedName::PARAMETERTYPE)?;
        code.push_instruction(Instruction::AStore(module_typ))?;

        /* MethodHandle updateEffects = MethodHandles.collectArguments(
         *   copyResizedArrayHandle.asType(
         *     MethodType.methodType(
         *       int.class,
         *       new Class[] {
         *         tableType,
         *         tableType,
         *         tableElemType
         *       }
         *     )
         *   ),
         *   0,
         *   setter
         * );
         */
        code.const_methodhandle(utility_class_name, &UnqualifiedName::COPYRESIZEDARRAY)?;
        code.const_class(&FieldType::INT)?;
        code.push_instruction(Instruction::IConst3)?;
        code.push_instruction(Instruction::ANewArray(cls_cls_idx))?;
        for (arr_idx, variable_to_load) in vec![table_typ, table_typ, table_elem_typ]
            .into_iter()
            .enumerate()
        {
            code.push_instruction(Instruction::Dup)?;
            code.const_int(arr_idx as i32)?;
            code.push_instruction(Instruction::ALoad(variable_to_load))?;
            code.push_instruction(Instruction::AAStore)?;
        }
        code.invoke(&BinaryName::METHODTYPE, &UnqualifiedName::METHODTYPE)?;
        code.invoke(&BinaryName::METHODHANDLE, &UnqualifiedName::ASTYPE)?;
        code.push_instruction(Instruction::IConst0)?;
        code.push_instruction(Instruction::ALoad(setter_argument))?;
        code.invoke(
            &BinaryName::METHODHANDLES,
            &UnqualifiedName::COLLECTARGUMENTS,
        )?;

        /* MethodHandle permutedEffects = MethodHandles.permuteArguments(
         *   updateEffects,
         *   MethodType.methodType(
         *     int.class,
         *     new Class[] {
         *       tableType,     // newTable
         *       moduleType,    // module
         *       tableType,     // oldTable
         *       tableElemType  // filler
         *     }
         *   ),
         *   new int[] { 1, 0, 0, 2, 3 }
         * );
         */
        code.const_class(&FieldType::INT)?;
        code.push_instruction(Instruction::IConst4)?;
        code.push_instruction(Instruction::ANewArray(cls_cls_idx))?;
        for (arr_idx, variable_to_load) in vec![table_typ, module_typ, table_typ, table_elem_typ]
            .into_iter()
            .enumerate()
        {
            code.push_instruction(Instruction::Dup)?;
            code.const_int(arr_idx as i32)?;
            code.push_instruction(Instruction::ALoad(variable_to_load))?;
            code.push_instruction(Instruction::AAStore)?;
        }
        code.invoke(&BinaryName::METHODTYPE, &UnqualifiedName::METHODTYPE)?;
        code.push_instruction(Instruction::IConst5)?;
        code.push_instruction(Instruction::NewArray(BaseType::Int))?;
        for (arr_idx, array_elem) in vec![1, 0, 0, 2, 3].into_iter().enumerate() {
            code.push_instruction(Instruction::Dup)?;
            code.const_int(arr_idx as i32)?;
            code.const_int(array_elem as i32)?;
            code.push_instruction(Instruction::IAStore)?;
        }
        code.invoke(
            &BinaryName::METHODHANDLES,
            &UnqualifiedName::PERMUTEARGUMENTS,
        )?;

        /* MethodHandle createAndUpdateNewTable = MethodHandles.collectArguments(
         *   permutedEffects,
         *   0,
         *   MethodHandles.arrayConstructor(tableType)
         * );
         */
        code.push_instruction(Instruction::IConst0)?;
        code.push_instruction(Instruction::ALoad(table_typ))?;
        code.invoke(
            &BinaryName::METHODHANDLES,
            &UnqualifiedName::ARRAYCONSTRUCTOR,
        )?;
        code.invoke(
            &BinaryName::METHODHANDLES,
            &UnqualifiedName::COLLECTARGUMENTS,
        )?;
        code.push_instruction(Instruction::AStore(create_and_update_new_table))?;

        /* MethodHandle createAndUpdateNewTableIfValid = MethodHandles.guardWithTest(
         *   intIsNegativeOneHandle,
         *   MethodHandles.dropArguments(
         *     MethodHandles.constant(int.class, -1),
         *     0,
         *     createAndUpdateNewTable.type().parameterArray()
         *   ),
         *   createAndUpdateNewTable
         * );
         */
        code.const_methodhandle(utility_class_name, &UnqualifiedName::INTISNEGATIVEONE)?;
        code.const_class(&FieldType::INT)?;
        code.push_instruction(Instruction::IConstM1)?;
        code.invoke(&BinaryName::INTEGER, &UnqualifiedName::VALUEOF)?;
        code.invoke(&BinaryName::METHODHANDLES, &UnqualifiedName::CONSTANT)?;
        code.push_instruction(Instruction::IConst0)?;
        code.push_instruction(Instruction::ALoad(create_and_update_new_table))?;
        code.invoke(&BinaryName::METHODHANDLE, &UnqualifiedName::TYPE)?;
        code.invoke(&BinaryName::METHODTYPE, &UnqualifiedName::PARAMETERARRAY)?;
        code.invoke(&BinaryName::METHODHANDLES, &UnqualifiedName::DROPARGUMENTS)?;
        code.push_instruction(Instruction::ALoad(create_and_update_new_table))?;
        code.invoke(&BinaryName::METHODHANDLES, &UnqualifiedName::GUARDWITHTEST)?;

        /* MethodHandle checkSizeAndCreate = MethodHandles.collectArguments(
         *   createAndUpdateNewTableIfValid,
         *   0,
         *   MethodHandles.collectArguments(
         *     MethodHandles.collectArguments(
         *       nextSizeHandle,
         *       2,
         *       MethodHandles.constant(long.class, maxSize)
         *     ),
         *     0,
         *     MethodHandles.arrayLength(tableType)
         *   )
         * );
         */
        code.push_instruction(Instruction::IConst0)?;
        code.const_methodhandle(utility_class_name, &UnqualifiedName::NEXTSIZE)?;
        code.push_instruction(Instruction::IConst2)?;
        code.const_class(&FieldType::LONG)?;
        code.push_instruction(Instruction::LLoad(max_size_argument))?;
        code.invoke(&BinaryName::LONG, &UnqualifiedName::VALUEOF)?;
        code.invoke(&BinaryName::METHODHANDLES, &UnqualifiedName::CONSTANT)?;
        code.invoke(
            &BinaryName::METHODHANDLES,
            &UnqualifiedName::COLLECTARGUMENTS,
        )?;
        code.push_instruction(Instruction::IConst0)?;
        code.push_instruction(Instruction::ALoad(table_typ))?;
        code.invoke(&BinaryName::METHODHANDLES, &UnqualifiedName::ARRAYLENGTH)?;
        code.invoke(
            &BinaryName::METHODHANDLES,
            &UnqualifiedName::COLLECTARGUMENTS,
        )?;
        code.invoke(
            &BinaryName::METHODHANDLES,
            &UnqualifiedName::COLLECTARGUMENTS,
        )?;

        /* MethodHandle toReturn = MethodHandles.permuteArguments(
         *   MethodHandles.collectArguments(
         *     MethodHandles.permuteArguments(
         *       checkSizeAndCreate,
         *       MethodType.methodType(
         *         int.class,
         *         new Class[] {
         *           tableType,      // oldTable
         *           moduleType,     // module
         *           tableElemType,  // filler
         *           int.class       // growBy
         *         }
         *       ),
         *       new int[] { 0, 3, 1, 0, 2 }
         *     ),
         *     0,
         *     getter
         *   ),
         *   methodType,
         *   new int[] { 2, 2, 0, 1 }
         * );
         */
        code.const_class(&FieldType::INT)?;
        code.push_instruction(Instruction::IConst4)?;
        code.push_instruction(Instruction::ANewArray(cls_cls_idx))?;
        for (arr_idx, variable_to_load) in vec![table_typ, module_typ, table_elem_typ]
            .into_iter()
            .enumerate()
        {
            code.push_instruction(Instruction::Dup)?;
            code.const_int(arr_idx as i32)?;
            code.push_instruction(Instruction::ALoad(variable_to_load))?;
            code.push_instruction(Instruction::AAStore)?;
        }
        code.push_instruction(Instruction::Dup)?;
        code.push_instruction(Instruction::IConst3)?;
        code.const_class(&FieldType::INT)?;
        code.push_instruction(Instruction::AAStore)?;
        code.invoke(&BinaryName::METHODTYPE, &UnqualifiedName::METHODTYPE)?;
        code.push_instruction(Instruction::IConst5)?;
        code.push_instruction(Instruction::NewArray(BaseType::Int))?;
        for (arr_idx, array_elem) in vec![0, 3, 1, 0, 2].into_iter().enumerate() {
            code.push_instruction(Instruction::Dup)?;
            code.const_int(arr_idx as i32)?;
            code.const_int(array_elem as i32)?;
            code.push_instruction(Instruction::IAStore)?;
        }
        code.invoke(
            &BinaryName::METHODHANDLES,
            &UnqualifiedName::PERMUTEARGUMENTS,
        )?;
        code.push_instruction(Instruction::IConst0)?;
        code.push_instruction(Instruction::ALoad(getter_argument))?;
        code.invoke(
            &BinaryName::METHODHANDLES,
            &UnqualifiedName::COLLECTARGUMENTS,
        )?;
        code.push_instruction(Instruction::ALoad(requested_type_argument))?;
        code.push_instruction(Instruction::IConst4)?;
        code.push_instruction(Instruction::NewArray(BaseType::Int))?;
        for (arr_idx, array_elem) in vec![2, 2, 0, 1].into_iter().enumerate() {
            code.push_instruction(Instruction::Dup)?;
            code.const_int(arr_idx as i32)?;
            code.const_int(array_elem as i32)?;
            code.push_instruction(Instruction::IAStore)?;
        }
        code.invoke(
            &BinaryName::METHODHANDLES,
            &UnqualifiedName::PERMUTEARGUMENTS,
        )?;

        // return new ConstantCallSite(toReturn);
        let constant_callsite_cls =
            code.get_class_idx(&RefType::Object(BinaryName::CONSTANTCALLSITE))?;
        code.push_instruction(Instruction::New(constant_callsite_cls))?;
        code.push_instruction(Instruction::DupX1)?;
        code.push_instruction(Instruction::Swap)?;
        code.invoke(&BinaryName::CONSTANTCALLSITE, &UnqualifiedName::INIT)?;
        code.push_branch_instruction(BranchInstruction::AReturn)?;

        Ok(())
    }

    // (I LTableElem; I)V
    fn generate_fill_table_case<B: CodeBuilderExts>(
        code: &mut B,
        utility_class_name: &BinaryName,
    ) -> Result<(), Error> {
        let requested_type_argument = 2; // MethodType
        let getter_argument = 3; // MethodHandle
        let table_typ = 7; // Class<TableElem[]>
        let table_elem_typ = 8; // Class<TableElem>
        let cls_cls_idx = code.get_class_idx(&RefType::CLASS)?;

        // Class<?> tableType = getter.type().returnType();
        code.push_instruction(Instruction::ALoad(getter_argument))?;
        code.invoke(&BinaryName::METHODHANDLE, &UnqualifiedName::TYPE)?;
        code.invoke(&BinaryName::METHODTYPE, &UnqualifiedName::RETURNTYPE)?;
        code.push_instruction(Instruction::AStore(table_typ))?;

        // Class<?> tableElemType = methodType.parameterType(1);
        code.push_instruction(Instruction::ALoad(requested_type_argument))?;
        code.push_instruction(Instruction::IConst1)?;
        code.invoke(&BinaryName::METHODTYPE, &UnqualifiedName::PARAMETERTYPE)?;
        code.push_instruction(Instruction::AStore(table_elem_typ))?;

        /* MethodHandle fillEffects = MethodHandles.collectArguments(
         *   fillArrayRangeHandle.asType(
         *     MethodType.methodType(
         *       void.class,
         *       new Class[] {
         *         int.class,
         *         tableElemType,
         *         int.class,
         *         tableType
         *       }
         *     )
         *   ),
         *   3,
         *   getter
         * );
         */
        code.const_methodhandle(utility_class_name, &UnqualifiedName::FILLARRAYRANGE)?;
        code.access_field(
            &BinaryName::VOID,
            &UnqualifiedName::UPPERCASE_TYPE,
            AccessMode::Read,
        )?;
        code.push_instruction(Instruction::IConst4)?;
        code.push_instruction(Instruction::ANewArray(cls_cls_idx))?;
        code.push_instruction(Instruction::Dup)?;
        code.push_instruction(Instruction::IConst0)?;
        code.const_class(&FieldType::INT)?;
        code.push_instruction(Instruction::AAStore)?;
        code.push_instruction(Instruction::Dup)?;
        code.push_instruction(Instruction::IConst1)?;
        code.push_instruction(Instruction::ALoad(table_elem_typ))?;
        code.push_instruction(Instruction::AAStore)?;
        code.push_instruction(Instruction::Dup)?;
        code.push_instruction(Instruction::IConst2)?;
        code.const_class(&FieldType::INT)?;
        code.push_instruction(Instruction::AAStore)?;
        code.push_instruction(Instruction::Dup)?;
        code.push_instruction(Instruction::IConst3)?;
        code.push_instruction(Instruction::ALoad(table_typ))?;
        code.push_instruction(Instruction::AAStore)?;
        code.invoke(&BinaryName::METHODTYPE, &UnqualifiedName::METHODTYPE)?;
        code.invoke(&BinaryName::METHODHANDLE, &UnqualifiedName::ASTYPE)?;
        code.push_instruction(Instruction::IConst3)?;
        code.push_instruction(Instruction::ALoad(getter_argument))?;
        code.invoke(
            &BinaryName::METHODHANDLES,
            &UnqualifiedName::COLLECTARGUMENTS,
        )?;

        // return new ConstantCallSite(toReturn);
        let constant_callsite_cls =
            code.get_class_idx(&RefType::Object(BinaryName::CONSTANTCALLSITE))?;
        code.push_instruction(Instruction::New(constant_callsite_cls))?;
        code.push_instruction(Instruction::DupX1)?;
        code.push_instruction(Instruction::Swap)?;
        code.invoke(&BinaryName::CONSTANTCALLSITE, &UnqualifiedName::INIT)?;
        code.push_branch_instruction(BranchInstruction::AReturn)?;

        Ok(())
    }

    fn generate_copy_table_case<B: CodeBuilderExts>(code: &mut B) -> Result<(), Error> {
        code.push_instruction(Instruction::AConstNull)?;
        code.push_branch_instruction(BranchInstruction::AThrow)?;
        Ok(())
    }

    fn generate_init_table_case<B: CodeBuilderExts>(code: &mut B) -> Result<(), Error> {
        code.push_instruction(Instruction::AConstNull)?;
        code.push_branch_instruction(BranchInstruction::AThrow)?;
        Ok(())
    }

    /// Generate the bootstrap method used for memory operators
    fn generate_bootstrap_memory<B: CodeBuilderExts>(
        code: &mut B,
        utility_class_name: &BinaryName,
    ) -> Result<(), Error> {
        let memory_size_case = code.fresh_label();
        let memory_grow_case = code.fresh_label();
        let memory_fill_case = code.fresh_label();
        let bad_name_case = code.fresh_label();

        code.push_instruction(Instruction::ALoad(1))?;
        code.invoke(&BinaryName::OBJECT, &UnqualifiedName::HASHCODE)?;
        code.push_branch_instruction(BranchInstruction::LookupSwitch {
            padding: 0,
            default: bad_name_case,
            targets: {
                let mut targets = vec![
                    (Self::java_hash_string(b"memory_size"), memory_size_case),
                    (Self::java_hash_string(b"memory_grow"), memory_grow_case),
                    (Self::java_hash_string(b"memory_fill"), memory_fill_case),
                ];
                targets.sort_by_key(|(key, _)| *key);
                targets
            },
        })?;

        // memory.size
        code.place_label(memory_size_case)?;
        code.push_instruction(Instruction::ALoad(1))?;
        code.const_string("memory_size")?;
        code.invoke(&BinaryName::OBJECT, &UnqualifiedName::EQUALS)?;
        code.push_branch_instruction(BranchInstruction::If(OrdComparison::EQ, bad_name_case, ()))?;
        Self::generate_size_memory_case(code, utility_class_name)?;

        // memory.grow
        code.place_label(memory_grow_case)?;
        code.push_instruction(Instruction::ALoad(1))?;
        code.const_string("memory_grow")?;
        code.invoke(&BinaryName::OBJECT, &UnqualifiedName::EQUALS)?;
        code.push_branch_instruction(BranchInstruction::If(OrdComparison::EQ, bad_name_case, ()))?;
        Self::generate_grow_memory_case(code, utility_class_name)?;

        // memory.fill
        code.place_label(memory_fill_case)?;
        code.push_instruction(Instruction::ALoad(1))?;
        code.const_string("memory_fill")?;
        code.invoke(&BinaryName::OBJECT, &UnqualifiedName::EQUALS)?;
        code.push_branch_instruction(BranchInstruction::If(OrdComparison::EQ, bad_name_case, ()))?;
        Self::generate_fill_memory_case(code, utility_class_name)?;

        // Catch all case
        let cls_idx = code.get_class_idx(&RefType::Object(BinaryName::ILLEGALARGUMENTEXCEPTION))?;
        code.place_label(bad_name_case)?;
        code.push_instruction(Instruction::New(cls_idx))?;
        code.push_instruction(Instruction::Dup)?;
        code.push_instruction(Instruction::ALoad(1))?;
        code.invoke(
            &BinaryName::ILLEGALARGUMENTEXCEPTION,
            &UnqualifiedName::INIT,
        )?;
        code.push_branch_instruction(BranchInstruction::AThrow)?;

        Ok(())
    }

    fn generate_size_memory_case<B: CodeBuilderExts>(
        code: &mut B,
        utility_class_name: &BinaryName,
    ) -> Result<(), Error> {
        let getter_argument = 3;

        /* MethodHandls.filterReturnValue(                    // (LMyWasmModule;)I
         *   MethodHandles.filterReturnValue(                 // (LMyWasmModule;)I
         *     getter,                                        // (LMyWasmModule)LByteBuffer;
         *     capacityHandle                                 // (LByteBuffer;)I
         *   ),
         *   bytesToMemoryPagesHandle                         // (I)I
         */
        code.push_instruction(Instruction::ALoad(getter_argument))?;
        code.const_methodhandle(&BinaryName::BYTEBUFFER, &UnqualifiedName::CAPACITY)?;
        code.invoke(
            &BinaryName::METHODHANDLES,
            &UnqualifiedName::FILTERRETURNVALUE,
        )?;
        code.const_methodhandle(utility_class_name, &UnqualifiedName::BYTESTOPAGES)?;
        code.invoke(
            &BinaryName::METHODHANDLES,
            &UnqualifiedName::FILTERRETURNVALUE,
        )?;

        // return new ConstantCallSite(methodhandle);
        let constant_callsite_cls =
            code.get_class_idx(&RefType::Object(BinaryName::CONSTANTCALLSITE))?;
        code.push_instruction(Instruction::New(constant_callsite_cls))?;
        code.push_instruction(Instruction::DupX1)?;
        code.push_instruction(Instruction::Swap)?;
        code.invoke(&BinaryName::CONSTANTCALLSITE, &UnqualifiedName::INIT)?;
        code.push_branch_instruction(BranchInstruction::AReturn)?;

        Ok(())
    }

    // TODO: avoid allocating a new memory for `memory.grow 0`
    fn generate_grow_memory_case<B: CodeBuilderExts>(
        code: &mut B,
        utility_class_name: &BinaryName,
    ) -> Result<(), Error> {
        let requested_type_argument = 2; // MethodType
        let getter_argument = 3; // MethodHandle
        let setter_argument = 4; // MethodHandle
        let max_size_argument = 5; // long
        let module_typ = 7; // Class<?>
        let create_and_update_new_memory = 8; // MethodHandle
        let cls_cls_idx = code.get_class_idx(&RefType::CLASS)?;
        let object_cls_idx = code.get_class_idx(&RefType::OBJECT)?;

        // Class<?> moduleType = getter.type().parameterType(0);
        code.push_instruction(Instruction::ALoad(getter_argument))?;
        code.invoke(&BinaryName::METHODHANDLE, &UnqualifiedName::TYPE)?;
        code.push_instruction(Instruction::IConst0)?;
        code.invoke(&BinaryName::METHODTYPE, &UnqualifiedName::PARAMETERTYPE)?;
        code.push_instruction(Instruction::AStore(module_typ))?;

        /* MethodHandle updateEffects = MethodHandles.collectArguments(
         *   copyResizedByteBuffer,
         *   0,
         *   setter
         * );
         */
        code.const_methodhandle(utility_class_name, &UnqualifiedName::COPYRESIZEDBYTEBUFFER)?;
        code.push_instruction(Instruction::IConst0)?;
        code.push_instruction(Instruction::ALoad(setter_argument))?;
        code.invoke(
            &BinaryName::METHODHANDLES,
            &UnqualifiedName::COLLECTARGUMENTS,
        )?;

        /* MethodHandle permutedEffects = MethodHandles.permuteArguments(
         *   updateEffects,
         *   MethodType.methodType(
         *     int.class,
         *     new Class[] {
         *       ByteBuffer.class, // newMemory
         *       moduleTyp,        // module
         *       ByteBuffer.class  // oldMemory
         *     }
         *   ),
         *   new int[] { 1, 0, 0, 2 }
         * );
         */
        code.const_class(&FieldType::INT)?;
        code.push_instruction(Instruction::IConst3)?;
        code.push_instruction(Instruction::ANewArray(cls_cls_idx))?;
        code.push_instruction(Instruction::Dup)?;
        code.push_instruction(Instruction::IConst0)?;
        code.const_class(&FieldType::Ref(RefType::Object(BinaryName::BYTEBUFFER)))?;
        code.push_instruction(Instruction::AAStore)?;
        code.push_instruction(Instruction::Dup)?;
        code.push_instruction(Instruction::IConst1)?;
        code.push_instruction(Instruction::ALoad(module_typ))?;
        code.push_instruction(Instruction::AAStore)?;
        code.push_instruction(Instruction::Dup)?;
        code.push_instruction(Instruction::IConst2)?;
        code.const_class(&FieldType::Ref(RefType::Object(BinaryName::BYTEBUFFER)))?;
        code.push_instruction(Instruction::AAStore)?;
        code.invoke(&BinaryName::METHODTYPE, &UnqualifiedName::METHODTYPE)?;
        code.push_instruction(Instruction::IConst4)?;
        code.push_instruction(Instruction::NewArray(BaseType::Int))?;
        for (idx, value) in vec![1, 0, 0, 2].into_iter().enumerate() {
            code.push_instruction(Instruction::Dup)?;
            code.const_int(idx as i32)?;
            code.const_int(value)?;
            code.push_instruction(Instruction::IAStore)?;
        }
        code.invoke(
            &BinaryName::METHODHANDLES,
            &UnqualifiedName::PERMUTEARGUMENTS,
        )?;

        /* MethodHandle createAndUpdateNewMemory = MethodHandles.collectArguments(
         *   permutedEffects,
         *   0,
         *   MethodHandles.filterReturnValue(
         *     MethodHandles.filterReturnValue(pagesToBytes, bytebufferAllocate),
         *     MethodHandles.insertArguments(byteBufferByteOrder, 1, new Object[] { ByteOrder.LITTLE_ENDIAN })
         *   )
         * );
         */
        code.push_instruction(Instruction::IConst0)?;
        code.const_methodhandle(utility_class_name, &UnqualifiedName::PAGESTOBYTES)?;
        code.const_methodhandle(&BinaryName::BYTEBUFFER, &UnqualifiedName::ALLOCATE)?;
        code.invoke(
            &BinaryName::METHODHANDLES,
            &UnqualifiedName::FILTERRETURNVALUE,
        )?;
        code.const_methodhandle(&BinaryName::BYTEBUFFER, &UnqualifiedName::ORDER)?;
        code.push_instruction(Instruction::IConst1)?;
        code.push_instruction(Instruction::IConst1)?;
        code.push_instruction(Instruction::ANewArray(object_cls_idx))?;
        code.push_instruction(Instruction::Dup)?;
        code.push_instruction(Instruction::IConst0)?;
        code.access_field(
            &BinaryName::BYTEORDER,
            &UnqualifiedName::LITTLEENDIAN,
            AccessMode::Read,
        )?;
        code.push_instruction(Instruction::AAStore)?;
        code.invoke(
            &BinaryName::METHODHANDLES,
            &UnqualifiedName::INSERTARGUMENTS,
        )?;
        code.invoke(
            &BinaryName::METHODHANDLES,
            &UnqualifiedName::FILTERRETURNVALUE,
        )?;
        code.invoke(
            &BinaryName::METHODHANDLES,
            &UnqualifiedName::COLLECTARGUMENTS,
        )?;
        code.push_instruction(Instruction::AStore(create_and_update_new_memory))?;

        /* MethodHandle createAndUpdateNewMemoryIfValid = MethodHandles.guardWithTest(
         *   intIsNegativeOneHandle,
         *   MethodHandles.dropArguments(
         *     MethodHandles.constant(int.class, -1),
         *     0,
         *     createAndUpdateNewMemory.type().parameterArray()
         *   ),
         *   createAndUpdateNewMemory
         * );
         */
        code.const_methodhandle(utility_class_name, &UnqualifiedName::INTISNEGATIVEONE)?;
        code.const_class(&FieldType::INT)?;
        code.push_instruction(Instruction::IConstM1)?;
        code.invoke(&BinaryName::INTEGER, &UnqualifiedName::VALUEOF)?;
        code.invoke(&BinaryName::METHODHANDLES, &UnqualifiedName::CONSTANT)?;
        code.push_instruction(Instruction::IConst0)?;
        code.push_instruction(Instruction::ALoad(create_and_update_new_memory))?;
        code.invoke(&BinaryName::METHODHANDLE, &UnqualifiedName::TYPE)?;
        code.invoke(&BinaryName::METHODTYPE, &UnqualifiedName::PARAMETERARRAY)?;
        code.invoke(&BinaryName::METHODHANDLES, &UnqualifiedName::DROPARGUMENTS)?;
        code.push_instruction(Instruction::ALoad(create_and_update_new_memory))?;
        code.invoke(&BinaryName::METHODHANDLES, &UnqualifiedName::GUARDWITHTEST)?;

        /* MethodHandle checkSizeAndCreate = MethodHandles.collectArguments(
         *   createAndUpdateNewMemoryIfValid,
         *   0,
         *   MethodHandles.collectArguments(
         *     MethodHandles.collectArguments(
         *       nextSizeHandle,
         *       2,
         *       MethodHandles.constant(long.class, maxSize)
         *     ),
         *     0,
         *     MethodHandles.filterReturnValue(bytebufferCapacity, bytesToPages)
         *   )
         * );
         */
        code.push_instruction(Instruction::IConst0)?;
        code.const_methodhandle(utility_class_name, &UnqualifiedName::NEXTSIZE)?;
        code.push_instruction(Instruction::IConst2)?;
        code.const_class(&FieldType::LONG)?;
        code.push_instruction(Instruction::LLoad(max_size_argument))?;
        code.invoke(&BinaryName::LONG, &UnqualifiedName::VALUEOF)?;
        code.invoke(&BinaryName::METHODHANDLES, &UnqualifiedName::CONSTANT)?;
        code.invoke(
            &BinaryName::METHODHANDLES,
            &UnqualifiedName::COLLECTARGUMENTS,
        )?;
        code.push_instruction(Instruction::IConst0)?;
        code.const_methodhandle(&BinaryName::BYTEBUFFER, &UnqualifiedName::CAPACITY)?;
        code.const_methodhandle(utility_class_name, &UnqualifiedName::BYTESTOPAGES)?;
        code.invoke(
            &BinaryName::METHODHANDLES,
            &UnqualifiedName::FILTERRETURNVALUE,
        )?;
        code.invoke(
            &BinaryName::METHODHANDLES,
            &UnqualifiedName::COLLECTARGUMENTS,
        )?;
        code.invoke(
            &BinaryName::METHODHANDLES,
            &UnqualifiedName::COLLECTARGUMENTS,
        )?;

        /* MethodHandle toReturn = MethodHandles.permuteArguments(
         *   MethodHandles.collectArguments(
         *     MethodHandles.permuteArguments(
         *       checkSizeAndCreate,
         *       MethodType.methodType(
         *         int.class,
         *         new Class[] {
         *           ByteBuffer.class,  // oldMemory
         *           moduleTyp,         // module
         *           int.class          // growBy
         *         }
         *       ),
         *       new int[] { 0, 2, 1, 0 }
         *     ),
         *     0,
         *     getter
         *   ),
         *   methodType,
         *   new int[] { 1, 1, 0 }
         * );
         */
        code.const_class(&FieldType::INT)?;
        code.push_instruction(Instruction::IConst3)?;
        code.push_instruction(Instruction::ANewArray(cls_cls_idx))?;
        code.push_instruction(Instruction::Dup)?;
        code.push_instruction(Instruction::IConst0)?;
        code.const_class(&FieldType::Ref(RefType::Object(BinaryName::BYTEBUFFER)))?;
        code.push_instruction(Instruction::AAStore)?;
        code.push_instruction(Instruction::Dup)?;
        code.push_instruction(Instruction::IConst1)?;
        code.push_instruction(Instruction::ALoad(module_typ))?;
        code.push_instruction(Instruction::AAStore)?;
        code.push_instruction(Instruction::Dup)?;
        code.push_instruction(Instruction::IConst2)?;
        code.const_class(&FieldType::INT)?;
        code.push_instruction(Instruction::AAStore)?;
        code.invoke(&BinaryName::METHODTYPE, &UnqualifiedName::METHODTYPE)?;
        code.push_instruction(Instruction::IConst4)?;
        code.push_instruction(Instruction::NewArray(BaseType::Int))?;
        for (arr_idx, array_elem) in vec![0, 2, 1, 0].into_iter().enumerate() {
            code.push_instruction(Instruction::Dup)?;
            code.const_int(arr_idx as i32)?;
            code.const_int(array_elem as i32)?;
            code.push_instruction(Instruction::IAStore)?;
        }
        code.invoke(
            &BinaryName::METHODHANDLES,
            &UnqualifiedName::PERMUTEARGUMENTS,
        )?;
        code.push_instruction(Instruction::IConst0)?;
        code.push_instruction(Instruction::ALoad(getter_argument))?;
        code.invoke(
            &BinaryName::METHODHANDLES,
            &UnqualifiedName::COLLECTARGUMENTS,
        )?;
        code.push_instruction(Instruction::ALoad(requested_type_argument))?;
        code.push_instruction(Instruction::IConst3)?;
        code.push_instruction(Instruction::NewArray(BaseType::Int))?;
        for (arr_idx, array_elem) in vec![1, 1, 0].into_iter().enumerate() {
            code.push_instruction(Instruction::Dup)?;
            code.const_int(arr_idx as i32)?;
            code.const_int(array_elem as i32)?;
            code.push_instruction(Instruction::IAStore)?;
        }
        code.invoke(
            &BinaryName::METHODHANDLES,
            &UnqualifiedName::PERMUTEARGUMENTS,
        )?;

        // return new ConstantCallSite(toReturn);
        let constant_callsite_cls =
            code.get_class_idx(&RefType::Object(BinaryName::CONSTANTCALLSITE))?;
        code.push_instruction(Instruction::New(constant_callsite_cls))?;
        code.push_instruction(Instruction::DupX1)?;
        code.push_instruction(Instruction::Swap)?;
        code.invoke(&BinaryName::CONSTANTCALLSITE, &UnqualifiedName::INIT)?;
        code.push_branch_instruction(BranchInstruction::AReturn)?;

        Ok(())
    }

    fn generate_fill_memory_case<B: CodeBuilderExts>(
        code: &mut B,
        utility_class_name: &BinaryName,
    ) -> Result<(), Error> {
        let getter_argument = 3; // MethodHandle

        /* MethodHandle fillEffects = MethodHandles.collectArguments(
         *   fillByteBufferRange,
         *   3,
         *   getter
         * );
         */
        code.const_methodhandle(utility_class_name, &UnqualifiedName::FILLBYTEBUFFERRANGE)?;
        code.push_instruction(Instruction::IConst3)?;
        code.push_instruction(Instruction::ALoad(getter_argument))?;
        code.invoke(
            &BinaryName::METHODHANDLES,
            &UnqualifiedName::COLLECTARGUMENTS,
        )?;

        // return new ConstantCallSite(toReturn);
        let constant_callsite_cls =
            code.get_class_idx(&RefType::Object(BinaryName::CONSTANTCALLSITE))?;
        code.push_instruction(Instruction::New(constant_callsite_cls))?;
        code.push_instruction(Instruction::DupX1)?;
        code.push_instruction(Instruction::Swap)?;
        code.invoke(&BinaryName::CONSTANTCALLSITE, &UnqualifiedName::INIT)?;
        code.push_branch_instruction(BranchInstruction::AReturn)?;

        Ok(())
    }

    /// Compute Java's `.hashCode` on simple ASCII strings
    const fn java_hash_string(string: &[u8]) -> i32 {
        let mut hash: i32 = 0;
        let mut i = 0;
        while i < string.len() {
            hash = hash.wrapping_mul(31).wrapping_add(string[i] as i32);
            i += 1;
        }
        hash
    }
}

/// Tracks utility bootstrap methods inside a given class
#[derive(Default)]
pub struct BootstrapUtilities {
    /// Mapping from the table index to a bootstrap method index
    table_bootstrap_methods: HashMap<u32, u16>,

    /// Mapping from the memory index to a bootstrap method index
    memory_bootstrap_methods: HashMap<u32, u16>,

    /// Ordered list of bootstrap methods
    bootstrap_methods: Vec<BootstrapMethod>,
}
impl BootstrapUtilities {
    /// Get (and create if missing) a bootstrap method for a given table
    ///
    /// Note: the `constants` argument must correspond to the class in which the bootstrap method
    /// is going to be _used_ (not where it is defined).
    pub fn get_table_bootstrap(
        &mut self,
        table_index: u32,
        table: &Table,
        table_field_class: &BinaryName,
        utilities: &mut UtilityClass,
        constants: &ConstantsPool,
    ) -> Result<u16, Error> {
        if let Some(bootstrap) = self.table_bootstrap_methods.get(&table_index) {
            return Ok(*bootstrap);
        }

        // Ensure the bootstrapping method is defined
        let _ = utilities.add_utility_method(UtilityMethod::BootstrapTable)?;

        // Compute a method handle for the bootstrap
        let table_bootstrap_handle: ConstantIndex = {
            let utilities_utf8 = constants.get_utf8(utilities.class_name().as_str())?;
            let utilities_cls = constants.get_class(utilities_utf8)?;
            let tablebootstrap_utf8 =
                constants.get_utf8(UtilityMethod::BootstrapTable.name().as_str())?;
            let tablebootstrap_typ =
                constants.get_utf8(UtilityMethod::BootstrapTable.descriptor().render())?;
            let tablebootstrap_nt =
                constants.get_name_and_type(tablebootstrap_utf8, tablebootstrap_typ)?;
            let tablebootstrap_method_ref =
                constants.get_method_ref(utilities_cls, tablebootstrap_nt, false)?;
            constants
                .get_method_handle(HandleKind::InvokeStatic, tablebootstrap_method_ref.into())?
        };

        // Compute the getter and setter constant arguments for the bootstrap method
        let table_field_typ = FieldType::array(table.table_type.field_type());
        let table_fieldref: ConstantIndex = {
            let class_utf8 = constants.get_utf8(table_field_class.as_str())?;
            let class_idx = constants.get_class(class_utf8)?;
            let field_utf8 = constants.get_utf8(table.field_name.as_str())?;
            let desc_utf8 = constants.get_utf8(table_field_typ.render())?;
            let name_and_type_idx = constants.get_name_and_type(field_utf8, desc_utf8)?;
            constants
                .get_field_ref(class_idx, name_and_type_idx)?
                .into()
        };
        let table_get_handle = constants.get_method_handle(HandleKind::GetField, table_fieldref)?;
        let table_set_handle = constants.get_method_handle(HandleKind::PutField, table_fieldref)?;

        /* Compute the maximum table size based on two constraints:
         *
         *   - the JVM's inherent limit of using signed 32-bit integers for array indices
         *   - a declared constraint in the WASM module
         */
        let max_table_size = constants.get_long(i64::min(
            i32::MAX as i64,
            table.maximum.unwrap_or(u32::MAX) as i64,
        ))?;

        // Generate the bootstrap attribute and return the index
        let bootstrap_index = self.bootstrap_methods.len() as u16; // TODO: detect overflow
        self.bootstrap_methods.push(BootstrapMethod {
            bootstrap_method: table_bootstrap_handle,
            bootstrap_arguments: vec![table_get_handle, table_set_handle, max_table_size],
        });
        self.table_bootstrap_methods
            .insert(table_index, bootstrap_index);

        Ok(bootstrap_index)
    }

    /// Get (and create if missing) a bootstrap method for a given memory
    ///
    /// Note: the `constants` argument must correspond to the class in which the bootstrap method
    /// is going to be _used_ (not where it is defined).
    pub fn get_memory_bootstrap(
        &mut self,
        memory_index: u32,
        memory: &Memory,
        memory_field_class: &BinaryName,
        utilities: &mut UtilityClass,
        constants: &ConstantsPool,
    ) -> Result<u16, Error> {
        if let Some(bootstrap) = self.memory_bootstrap_methods.get(&memory_index) {
            return Ok(*bootstrap);
        }

        // Ensure the bootstrapping method is defined
        let _ = utilities.add_utility_method(UtilityMethod::BootstrapMemory)?;

        // Compute a method handle for the bootstrap
        let memory_bootstrap_handle: ConstantIndex = {
            let utilities_utf8 = constants.get_utf8(utilities.class_name().as_str())?;
            let utilities_cls = constants.get_class(utilities_utf8)?;
            let membootstrap_utf8 =
                constants.get_utf8(UtilityMethod::BootstrapMemory.name().as_str())?;
            let membootstrap_typ =
                constants.get_utf8(UtilityMethod::BootstrapMemory.descriptor().render())?;
            let membootstrap_nt =
                constants.get_name_and_type(membootstrap_utf8, membootstrap_typ)?;
            let membootstrap_method_ref =
                constants.get_method_ref(utilities_cls, membootstrap_nt, false)?;
            constants.get_method_handle(HandleKind::InvokeStatic, membootstrap_method_ref.into())?
        };

        // Compute the getter and setter constant arguments for the bootstrap method
        let memory_field_typ = FieldType::Ref(RefType::Object(BinaryName::BYTEBUFFER));
        let memory_fieldref: ConstantIndex = {
            let class_utf8 = constants.get_utf8(memory_field_class.as_str())?;
            let class_idx = constants.get_class(class_utf8)?;
            let field_utf8 = constants.get_utf8(memory.field_name.as_str())?;
            let desc_utf8 = constants.get_utf8(memory_field_typ.render())?;
            let name_and_type_idx = constants.get_name_and_type(field_utf8, desc_utf8)?;
            constants
                .get_field_ref(class_idx, name_and_type_idx)?
                .into()
        };
        let memory_get_handle =
            constants.get_method_handle(HandleKind::GetField, memory_fieldref)?;
        let memory_set_handle =
            constants.get_method_handle(HandleKind::PutField, memory_fieldref)?;

        let memory_maximum = memory.memory_type.maximum;

        /* Compute the maximum memory size based on two constraints:
         *
         *   - the JVM's inherent limit of using signed 32-bit integers for bytebuffer indices
         *   - a declared constraint in the WASM module
         */
        let max_memory_size = constants.get_long(i64::min(
            (i32::MAX as i64) / (u16::MAX as i64),
            memory_maximum.unwrap_or(u32::MAX as u64) as i64,
        ))?;

        // Generate the bootstrap attribute and return the index
        let bootstrap_index = self.bootstrap_methods.len() as u16; // TODO: detect overflow
        self.bootstrap_methods.push(BootstrapMethod {
            bootstrap_method: memory_bootstrap_handle,
            bootstrap_arguments: vec![memory_get_handle, memory_set_handle, max_memory_size],
        });
        self.memory_bootstrap_methods
            .insert(memory_index, bootstrap_index);

        Ok(bootstrap_index)
    }
}

impl From<BootstrapUtilities> for BootstrapMethods {
    fn from(bootstrap_utils: BootstrapUtilities) -> BootstrapMethods {
        BootstrapMethods(bootstrap_utils.bootstrap_methods)
    }
}
