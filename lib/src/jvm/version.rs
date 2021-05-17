use super::Serialize;
use byteorder::WriteBytesExt;
use std::io::Result;

/// Version of the class file, which is used to verify that the JVM has the
/// necessary features to interpret the class
#[derive(PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct Version {
    pub minor_version: u16,
    pub major_version: u16,
}

impl Version {
    /// JVM class file version corresponding to Java SE 8 (released March 2014)
    pub const JAVA8: Version = Version {
        minor_version: 0,
        major_version: 52,
    };
}

impl Serialize for Version {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> Result<()> {
        self.minor_version.serialize(writer)?;
        self.major_version.serialize(writer)?;
        Ok(())
    }
}
