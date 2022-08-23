use crate::jvm::class_file::{Attribute, Field, Method, Serialize, Version};
use crate::jvm::{ClassAccessFlags, ClassConstantIndex, Constant};
use crate::util::OffsetVec;
use byteorder::WriteBytesExt;
use std::fs;
use std::path::Path;

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

    /// Save the class file to disk
    pub fn save_to_path<P: AsRef<Path>>(
        &self,
        path: P,
        create_missing_directories: bool,
    ) -> std::io::Result<()> {
        let path = path.as_ref();
        if create_missing_directories {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
        }
        let mut class_file = fs::File::create(path)?;
        self.serialize(&mut class_file)
    }
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
