use crate::jvm::class_file::{ClassConstantIndex, ConstantIndex, Serialize, Utf8ConstantIndex};
use crate::jvm::verifier::VerificationType;
use crate::jvm::InnerClassAccessFlags;
use byteorder::WriteBytesExt;

/// [Attributes][0] used in classes, fields, methods, and even on some attributes.
///
/// The representation is designed to be easily extended with custom attributes. While some
/// attributes aren't essential, others are really important (eg. the [`Code`] attribute on a
/// method carries the actual bytecode).
///
/// [0]: https://docs.oracle.com/javase/specs/jvms/se15/html/jvms-4.html#jvms-4.7
#[derive(Debug)]
pub struct Attribute {
    /// Name of the attribute
    pub name_index: Utf8ConstantIndex,

    /// Encoded content of the attribute. The name of the attribute determines the structure of the
    /// encoded information.
    pub info: Vec<u8>,
}

impl Serialize for Attribute {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> std::io::Result<()> {
        self.name_index.serialize(writer)?;

        // Attribute info length is 4 bytes
        (self.info.len() as u32).serialize(writer)?;
        writer.write_all(&self.info)?;

        Ok(())
    }
}

/// Attributes are all stored in the same way (see `Attribute`), but internally
/// they represent very different things. This trait is implemented by things
/// which can be turned into attributes.
pub trait AttributeLike: Serialize {
    /// Name of the attribute
    const NAME: &'static str;
}

/// [Attribute][0] used to indicate a field has a constant value.
///
/// [0]: https://docs.oracle.com/javase/specs/jvms/se17/html/jvms-4.html#jvms-4.7.2
pub struct ConstantValue(ConstantIndex);

impl Serialize for ConstantValue {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> std::io::Result<()> {
        self.0.serialize(writer)
    }
}

impl AttributeLike for ConstantValue {
    const NAME: &'static str = "ConstantValue";
}

/// [Attribute][0] used to store method bytecode.
///
/// See [`crate::jvm::code::CodeBuilder`] for an interface through which to construct bytecode.
///
/// [0]: https://docs.oracle.com/javase/specs/jvms/se17/html/jvms-4.html#jvms-4.7.3
pub struct Code {
    /// Maximum depth of the operand stack throughout the body of the method (this takes into
    /// account that some types are wider)
    pub max_stack: u16,

    /// Maximum size of locals throughout the body of the method (this takes into account that some
    /// types are wider)
    pub max_locals: u16,

    /// Array of encoded bytecode
    pub code_array: BytecodeArray,

    /// Exception handlers within the code
    pub exception_table: Vec<ExceptionHandler>,

    /// Code attributes
    pub attributes: Vec<Attribute>,
}

impl Serialize for Code {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> std::io::Result<()> {
        self.max_stack.serialize(writer)?;
        self.max_locals.serialize(writer)?;
        self.code_array.serialize(writer)?;
        self.exception_table.serialize(writer)?;
        self.attributes.serialize(writer)?;
        Ok(())
    }
}

impl AttributeLike for Code {
    const NAME: &'static str = "Code";
}

/// Exception handler block as in [`Code`]
pub struct ExceptionHandler {
    /// Start of exception handler range (inclusive)
    pub start_pc: BytecodeIndex,

    /// End of exception handler range (exclusive)
    pub end_pc: BytecodeIndex,

    /// Start of the exception handler
    pub handler_pc: BytecodeIndex,

    /// Exception type that is caught and handled by the handler
    pub catch_type: ClassConstantIndex,
}

impl Serialize for ExceptionHandler {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> std::io::Result<()> {
        self.start_pc.serialize(writer)?;
        self.end_pc.serialize(writer)?;
        self.handler_pc.serialize(writer)?;
        self.catch_type.serialize(writer)?;
        Ok(())
    }
}

/// Encoded bytecode instructions as in [`Code`]
pub struct BytecodeArray(pub Vec<u8>);

impl Serialize for BytecodeArray {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> std::io::Result<()> {
        let len = self.0.len() as u32;
        len.serialize(writer)?;
        writer.write_all(&self.0)?;
        Ok(())
    }
}

/// Index into a [`BytecodeArray`]
///
/// Note that since instructions in the bytecode have variable widths, not every index points to an
/// instruction - some point to the middle of an instruction. These situations are usually invalid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BytecodeIndex(pub u16);

impl Serialize for BytecodeIndex {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> std::io::Result<()> {
        self.0.serialize(writer)
    }
}

/// [Attribute][0] used to store stack map frames for a [`Code`] section
///
/// See [`crate::jvm::verifier`] for more details on stack maps.
///
/// [0]: https://docs.oracle.com/javase/specs/jvms/se17/html/jvms-4.html#jvms-4.7.4
#[derive(Debug)]
pub struct StackMapTable(pub Vec<StackMapFrame>);

impl AttributeLike for StackMapTable {
    const NAME: &'static str = "StackMapTable";
}

impl Serialize for StackMapTable {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> std::io::Result<()> {
        self.0.serialize(writer)
    }
}

/// Stack map frame as in [`StackMapTable`]
///
/// See [`crate::jvm::verifier`] for more details on stack maps and
/// [`crate::jvm::verifier::Frame::stack_map_frame`] for how these are computed. The `offset_delta`
/// field present in all variants is 1 less than the offset from the previous previous frame,
/// unless the previous frame is the initial implicit frame (in which case it is just the offset
/// without the off-by one).
#[derive(Debug)]
pub enum StackMapFrame {
    /// Frame has the same locals as the previous frame and number of stack items is zero
    SameLocalsNoStack { offset_delta: u16 },

    /// Frame has the same locals as the previous frame and number of stack items is one
    SameLocalsOneStack {
        offset_delta: u16,

        /// Single element on the stack
        stack: VerificationType<ClassConstantIndex, BytecodeIndex>,
    },

    /// Frame is like the previous frame, but with 1 to 3 inclusive less locals
    ChopLocalsNoStack {
        offset_delta: u16,

        /// Number of locals "chopped" off, must be in the range 1 to 3 inclusive
        chopped_k: u8,
    },

    /// Frame is like the previous frame, but with extra locals
    AppendLocalsNoStack {
        offset_delta: u16,

        /// Extra locals added to the top of the existing stack of locals
        locals: Vec<VerificationType<ClassConstantIndex, BytecodeIndex>>,
    },

    /// Frame has exactly the locals and stack specified
    Full {
        offset_delta: u16,

        /// Complete list of locals
        locals: Vec<VerificationType<ClassConstantIndex, BytecodeIndex>>,

        /// Complete stack
        stack: Vec<VerificationType<ClassConstantIndex, BytecodeIndex>>,
    },
}

impl Serialize for StackMapFrame {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> std::io::Result<()> {
        match self {
            // `same_frame` and `same_frame_extended`
            StackMapFrame::SameLocalsNoStack { offset_delta } => {
                if *offset_delta <= 63 {
                    (*offset_delta as u8).serialize(writer)?;
                } else {
                    251u8.serialize(writer)?;
                    offset_delta.serialize(writer)?;
                }
            }

            // `same_locals_1_stack_item_frame` and `same_locals_1_stack_item_frame_extended`
            StackMapFrame::SameLocalsOneStack {
                offset_delta,
                stack,
            } => {
                if *offset_delta <= 63 {
                    (*offset_delta as u8 + 64).serialize(writer)?;
                } else {
                    247u8.serialize(writer)?;
                    offset_delta.serialize(writer)?;
                }
                stack.serialize(writer)?;
            }

            // `chop_frame`
            StackMapFrame::ChopLocalsNoStack {
                offset_delta,
                chopped_k,
            } => {
                assert!(
                    0 < *chopped_k && *chopped_k < 4,
                    "ChopLocalsNoStack chops 1-3 locals"
                );
                (251 - chopped_k).serialize(writer)?;
                offset_delta.serialize(writer)?;
            }

            // `append_frame`
            StackMapFrame::AppendLocalsNoStack {
                offset_delta,
                locals,
            } => {
                let added_k = locals.len();
                assert!(
                    0 < added_k && added_k < 4,
                    "AppendLocalsNoStack adds 1-3 locals"
                );
                (251 + added_k as u8).serialize(writer)?;
                offset_delta.serialize(writer)?;
                for local in locals {
                    local.serialize(writer)?;
                }
            }

            // `full_frame`
            StackMapFrame::Full {
                offset_delta,
                locals,
                stack,
            } => {
                255u8.serialize(writer)?;
                offset_delta.serialize(writer)?;
                locals.serialize(writer)?;
                stack.serialize(writer)?;
            }
        };
        Ok(())
    }
}

/// [Attribute][0] specifying the bootstrap methods on a class
///
/// Bootstrap methods are referred to in `invokedynamic` instructions (by their offset in the
/// bootstrap method array in this attribute).
///
/// [0]: https://docs.oracle.com/javase/specs/jvms/se17/html/jvms-4.html#jvms-4.7.23
#[derive(Debug)]
pub struct BootstrapMethods(pub Vec<BootstrapMethod>);

/// Bootstrap method as in [`BootstrapMethods`]
#[derive(Debug)]
pub struct BootstrapMethod {
    /// Method handle to use for bootstrapping
    pub bootstrap_method: ConstantIndex,

    /// Bootstrap method constant arguments
    pub bootstrap_arguments: Vec<ConstantIndex>,
}

impl AttributeLike for BootstrapMethods {
    const NAME: &'static str = "BootstrapMethods";
}

impl Serialize for BootstrapMethods {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> std::io::Result<()> {
        self.0.serialize(writer)
    }
}

impl Serialize for BootstrapMethod {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> std::io::Result<()> {
        self.bootstrap_method.serialize(writer)?;
        self.bootstrap_arguments.serialize(writer)?;
        Ok(())
    }
}

/// [Attribute][0] specifying the nest host of a class
///
/// If this attribute is not present, the class is the host of a nest (possibly implicitly so).
/// This attribute must not be present at the same time as [`NestMembers`].
///
/// [0]: https://docs.oracle.com/javase/specs/jvms/se15/html/jvms-4.html#jvms-4.7.28
#[derive(Debug)]
pub struct NestHost(pub ClassConstantIndex);

impl AttributeLike for NestHost {
    const NAME: &'static str = "NestHost";
}

impl Serialize for NestHost {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> std::io::Result<()> {
        self.0.serialize(writer)
    }
}

/// [Attribute][0] specifying the nest members of a class
///
/// Every class without a [`NestHost`] attribute is a nest host. If the nest host has members, the
/// class should have a [`NestMembers`] attribute to list them out.
///
/// [0]: https://docs.oracle.com/javase/specs/jvms/se15/html/jvms-4.html#jvms-4.7.29
#[derive(Debug)]
pub struct NestMembers(pub Vec<ClassConstantIndex>);

impl AttributeLike for NestMembers {
    const NAME: &'static str = "NestMembers";
}

impl Serialize for NestMembers {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> std::io::Result<()> {
        self.0.serialize(writer)
    }
}

/// [Attribute][0] elaborating the inner class relationship of every class in the constant pool
/// which is not a nest host.
///
/// [0]: https://docs.oracle.com/javase/specs/jvms/se15/html/jvms-4.html#jvms-4.7.6
#[derive(Debug)]
pub struct InnerClasses(pub Vec<InnerClass>);

impl AttributeLike for InnerClasses {
    const NAME: &'static str = "InnerClasses";
}

impl Serialize for InnerClasses {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> std::io::Result<()> {
        self.0.serialize(writer)
    }
}

/// Inner class as in [`InnerClasses`]
#[derive(Debug)]
pub struct InnerClass {
    /// Inner (nested) class
    pub inner_class: ClassConstantIndex,

    /// Outer class (note this class may also be itself nested)
    pub outer_class: ClassConstantIndex,

    /// Simple name of the inner class
    pub inner_name: Utf8ConstantIndex,

    /// Inner class access modifiers
    pub access_flags: InnerClassAccessFlags,
}

impl Serialize for InnerClass {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> std::io::Result<()> {
        self.inner_class.serialize(writer)?;
        self.outer_class.serialize(writer)?;
        self.inner_name.serialize(writer)?;
        self.access_flags.serialize(writer)?;
        Ok(())
    }
}

/// [Attribute][0] for specifying the generic signature of a class, method, or field.
///
/// The [format of the signature is an extension][1] of the format used for descriptors that
/// includes support for type parameters, wildcards, bounds, and checked exceptions.
///
/// [0]: https://docs.oracle.com/javase/specs/jvms/se17/html/jvms-4.html#jvms-4.7.9
/// [1]: https://docs.oracle.com/javase/specs/jvms/se17/html/jvms-4.html#jvms-4.7.9.1
#[derive(Debug)]
pub struct Signature {
    pub signature: Utf8ConstantIndex,
}

impl AttributeLike for Signature {
    const NAME: &'static str = "Signature";
}

impl Serialize for Signature {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> std::io::Result<()> {
        self.signature.serialize(writer)?;
        Ok(())
    }
}
