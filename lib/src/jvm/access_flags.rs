use super::Serialize;
use bitflags::bitflags;
use byteorder::WriteBytesExt;
use std::io::Result;

bitflags! {
    /// Access flags on classes
    ///
    /// [0]: https://docs.oracle.com/javase/specs/jvms/se15/html/jvms-4.html#jvms-4.1-200-E.1
    pub struct ClassAccessFlags: u16 {
        const PUBLIC = 0x0001;
        const FINAL = 0x0010;
        const SUPER = 0x0020;
        const INTERFACE = 0x0200;
        const ABSTRACT = 0x0400;
        const SYNTHETIC = 0x1000;
        const ANNOTATION = 0x2000;
        const ENUM = 0x4000;
        const MODULE = 0x8000;
    }
}

bitflags! {
    /// Access flags on methods
    ///
    /// [0]: https://docs.oracle.com/javase/specs/jvms/se15/html/jvms-4.html#jvms-4.6-200-A.1
    pub struct MethodAccessFlags: u16 {
        const PUBLIC = 0x0001;
        const PRIVATE = 0x0002;
        const PROTECTED = 0x0004;
        const STATIC = 0x0008;
        const FINAL = 0x0010;
        const SYNCHRONIZED = 0x0020;
        const BRIDGE = 0x0040;
        const VARARGS = 0x0080;
        const NATIVE = 0x0100;
        const ABSTRACT = 0x0400;
        const STRICT = 0x0800;
        const SYNTHETIC = 0x1000;
    }
}

bitflags! {
    /// Access flags on fields
    ///
    /// [0]: https://docs.oracle.com/javase/specs/jvms/se15/html/jvms-4.html#jvms-4.5-200-A.1
    pub struct FieldAccessFlags: u16 {
        const PUBLIC = 0x0001;
        const PRIVATE = 0x0002;
        const PROTECTED = 0x0004;
        const STATIC = 0x0008;
        const FINAL = 0x0010;
        const VOLATILE = 0x0040;
        const TRANSIENT = 0x0080;
        const SYNTHETIC = 0x1000;
        const ENUM = 0x4000;
    }
}

bitflags! {
    /// Access flags on inner classes
    ///
    /// [0]: https://docs.oracle.com/javase/specs/jvms/se15/html/jvms-4.html#jvms-4.7.6-300-D.1-D.1
    pub struct InnerClassAccessFlags: u16 {
        const PUBLIC = 0x0001;
        const PRIVATE = 0x0002;
        const PROTECTED = 0x0004;
        const STATIC = 0x0008;
        const FINAL = 0x0010;
        const INTERFACE = 0x0200;
        const ABSTRACT = 0x0400;
        const SYNTHETIC = 0x1000;
        const ANNOTATION = 0x2000;
        const ENUM = 0x4000;
    }
}

impl Serialize for ClassAccessFlags {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> Result<()> {
        self.bits().serialize(writer)
    }
}

impl Serialize for MethodAccessFlags {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> Result<()> {
        self.bits().serialize(writer)
    }
}

impl Serialize for FieldAccessFlags {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> Result<()> {
        self.bits().serialize(writer)
    }
}

impl Serialize for InnerClassAccessFlags {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> Result<()> {
        self.bits().serialize(writer)
    }
}
