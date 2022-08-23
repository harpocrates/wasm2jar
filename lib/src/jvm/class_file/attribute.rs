use crate::jvm::class_file::Serialize;
use crate::jvm::verifier::VerificationType;
use crate::jvm::{ClassConstantIndex, ConstantIndex, InnerClassAccessFlags, Utf8ConstantIndex};
use byteorder::WriteBytesExt;

/// Attributes (used in classes, fields, methods, and even on some attributes)
///
/// The representation is designed to be easily extended with custom attributes.
/// While some attributes aren't essential, others are really important (eg. the
/// code attribute for including the actual bytecode).
///
/// [0]: https://docs.oracle.com/javase/specs/jvms/se15/html/jvms-4.html#jvms-4.7
#[derive(Debug)]
pub struct Attribute {
    pub name_index: Utf8ConstantIndex,
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

/// [0]: https://docs.oracle.com/javase/specs/jvms/se15/html/jvms-4.html#jvms-4.7.2
pub struct ConstantValue(ConstantIndex);

impl Serialize for ConstantValue {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> std::io::Result<()> {
        self.0.serialize(writer)
    }
}

impl AttributeLike for ConstantValue {
    const NAME: &'static str = "ConstantValue";
}

/// [0]: https://docs.oracle.com/javase/specs/jvms/se15/html/jvms-4.html#jvms-4.7.3
pub struct Code {
    pub max_stack: u16,
    pub max_locals: u16,
    pub code_array: BytecodeArray,
    pub exception_table: Vec<ExceptionHandler>,
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

pub struct ExceptionHandler {
    /// Start of exception handler range (inclusive)
    pub start_pc: BytecodeIndex,

    /// End of exception handler range (exclusive)
    pub end_pc: BytecodeIndex,

    /// Start of the exception handler
    pub handler_pc: BytecodeIndex,

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

/// Encoded bytecode instructions
pub struct BytecodeArray(pub Vec<u8>);

impl Serialize for BytecodeArray {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> std::io::Result<()> {
        let len = self.0.len() as u32;
        len.serialize(writer)?;
        writer.write_all(&self.0)?;
        Ok(())
    }
}

/// Index into `BytecodeArray`
pub struct BytecodeIndex(pub u16);

impl Serialize for BytecodeIndex {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> std::io::Result<()> {
        self.0.serialize(writer)
    }
}

/// [0]: https://docs.oracle.com/javase/specs/jvms/se7/html/jvms-4.html#jvms-4.7.4
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

#[derive(Debug)]
pub enum StackMapFrame {
    /// Frame has the same locals as the previous frame and number of stack items is zero
    /// Tags: 0-63 or 251
    SameLocalsNoStack { offset_delta: u16 },

    /// Frame has the same locals as the previous frame and number of stack items is one
    /// Tags: 64-127 or 247
    SameLocalsOneStack {
        offset_delta: u16,
        stack: VerificationType<ClassConstantIndex, u16>,
    },

    /// Frame is like the previous frame, but without the last `chopped_k` locals
    ///
    /// Note: `chopped_k` must be in the range 1 to 3 inclusive
    /// Tags: 248-250
    ChopLocalsNoStack { offset_delta: u16, chopped_k: u8 },

    /// Frame is like the previous frame, but with extra locals
    /// Tags: 252-254
    AppendLocalsNoStack {
        offset_delta: u16,
        locals: Vec<VerificationType<ClassConstantIndex, u16>>,
    },

    /// Frame has exactly the locals and stack specified
    /// Tag: 255
    Full {
        offset_delta: u16,
        locals: Vec<VerificationType<ClassConstantIndex, u16>>,
        stack: Vec<VerificationType<ClassConstantIndex, u16>>,
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

/// [0]: https://docs.oracle.com/javase/specs/jvms/se15/html/jvms-4.html#jvms-4.7.23
#[derive(Debug)]
pub struct BootstrapMethods(pub Vec<BootstrapMethod>);

#[derive(Debug)]
pub struct BootstrapMethod {
    pub bootstrap_method: ConstantIndex,
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

/// Every inner class referenced in a class' constant pool must be included in the inner classes
/// attribute on the class.
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

#[derive(Debug)]
pub struct InnerClass {
    pub inner_class: ClassConstantIndex,
    pub outer_class: ClassConstantIndex,
    pub inner_name: Utf8ConstantIndex,
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
