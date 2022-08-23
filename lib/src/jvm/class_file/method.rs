use crate::jvm::class_file::{Attribute, Serialize};
use crate::jvm::{MethodAccessFlags, Utf8ConstantIndex};
use byteorder::WriteBytesExt;

/// Method declared by a class or interface
///
/// [0]: https://docs.oracle.com/javase/specs/jvms/se15/html/jvms-4.html#jvms-4.6
#[derive(Debug)]
pub struct Method {
    pub access_flags: MethodAccessFlags,
    pub name_index: Utf8ConstantIndex,
    pub descriptor_index: Utf8ConstantIndex,
    pub attributes: Vec<Attribute>,
}

impl Serialize for Method {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> std::io::Result<()> {
        self.access_flags.serialize(writer)?;
        self.name_index.serialize(writer)?;
        self.descriptor_index.serialize(writer)?;
        self.attributes.serialize(writer)?;
        Ok(())
    }
}
