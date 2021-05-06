use super::{
    Attribute, ClassConstantIndex, ConstantIndex, InnerClassAccessFlags, Serialize,
    Utf8ConstantIndex, VerificationType, Width,
};
use byteorder::WriteBytesExt;

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
        stack_verification: VerificationType<ClassConstantIndex, u16>,
    },

    /// Frame is like the previous frame, but without the last `chopped_k` locals
    /// Tags: 248-250
    ChoppedFrameNoStack { offset_delta: u16, chopped_k: u8 },

    /// Frame is like the previous frame, but with an extra `locals_verifications.len()` locals
    /// Tags: 252-254
    AppendFrameNoStack {
        offset_delta: u16,
        local_verifications: Vec<VerificationType<ClassConstantIndex, u16>>,
    },

    /// Frame has exactly the locals and stack specified
    /// Tag: 255
    FullFrame {
        offset_delta: u16,
        local_verifications: Vec<VerificationType<ClassConstantIndex, u16>>,
        stack_verifications: Vec<VerificationType<ClassConstantIndex, u16>>,
    },
}

impl Serialize for StackMapFrame {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> std::io::Result<()> {
        match self {
            StackMapFrame::SameLocalsNoStack {
                offset_delta: o @ 0..=63,
            } => {
                (*o as u8).serialize(writer)?;
            }
            StackMapFrame::SameLocalsNoStack { offset_delta } => {
                251u8.serialize(writer)?;
                offset_delta.serialize(writer)?;
            }
            StackMapFrame::SameLocalsOneStack {
                offset_delta: o @ 0..=63,
                stack_verification,
            } => {
                (*o as u8 + 64).serialize(writer)?;
                stack_verification.serialize(writer)?;
            }
            StackMapFrame::FullFrame {
                offset_delta,
                local_verifications,
                stack_verifications,
            } => {
                255u8.serialize(writer)?;
                offset_delta.serialize(writer)?;

                let local_size = local_verifications.len() as u16;
                local_size.serialize(writer)?;
                for ver in local_verifications {
                    ver.serialize(writer)?;
                }

                let stack_size =
                    stack_verifications.iter().map(|v| v.width()).sum::<usize>() as u16;
                stack_size.serialize(writer)?;
                for ver in stack_verifications {
                    ver.serialize(writer)?;
                }
            }
            _ => todo!(),
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
    pub boostrap_arguments: Vec<ConstantIndex>,
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
        self.boostrap_arguments.serialize(writer)?;
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
