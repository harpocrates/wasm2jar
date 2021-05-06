use super::{
    ClassAccessFlags, ClassConstantIndex, Constant, FieldAccessFlags, MethodAccessFlags, OffsetVec,
    Serialize, Utf8ConstantIndex, Version,
};
use byteorder::WriteBytesExt;

/// Representation of the [`class` file format of the JVM][0]
///
/// [0]: https://docs.oracle.com/javase/specs/jvms/se15/html/jvms-4.html
#[derive(Debug)]
pub struct ClassFile {
    pub version: Version,
    pub constants: OffsetVec<Constant>,
    pub access_flags: ClassAccessFlags,
    pub this_class: ClassConstantIndex,
    pub super_class: ClassConstantIndex,
    pub interfaces: Vec<ClassConstantIndex>,
    pub fields: Vec<Field>,
    pub methods: Vec<Method>,
    pub attributes: Vec<Attribute>,
}
impl ClassFile {
    /// Magic header bytes that go at the front of the serialized class file
    const MAGIC: [u8; 4] = [0xCA, 0xFE, 0xBA, 0xBE];
}

impl Serialize for ClassFile {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> std::io::Result<()> {
        writer.write_all(&ClassFile::MAGIC)?;
        self.version.serialize(writer)?;
        self.constants.serialize(writer)?;
        self.access_flags.serialize(writer)?;
        self.this_class.serialize(writer)?;
        self.super_class.serialize(writer)?;
        self.interfaces.serialize(writer)?;
        self.fields.serialize(writer)?;
        self.methods.serialize(writer)?;
        self.attributes.serialize(writer)?;
        Ok(())
    }
}

/// Field declared by a class or interface
///
/// [0]: https://docs.oracle.com/javase/specs/jvms/se15/html/jvms-4.html#jvms-4.5
#[derive(Debug)]
pub struct Field {
    pub access_flags: FieldAccessFlags,
    pub name_index: Utf8ConstantIndex,
    pub descriptor_index: Utf8ConstantIndex,
    pub attributes: Vec<Attribute>,
}

impl Serialize for Field {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> std::io::Result<()> {
        self.access_flags.serialize(writer)?;
        self.name_index.serialize(writer)?;
        self.descriptor_index.serialize(writer)?;
        self.attributes.serialize(writer)?;
        Ok(())
    }
}

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
