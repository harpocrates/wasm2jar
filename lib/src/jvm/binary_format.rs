use byteorder::{BigEndian, WriteBytesExt};
use std::io::Result;

/// Utility trait for serializing data inside class files
///
/// Java class files have some peculiarities that make it useful to define an extra trait (instead
/// of just using `serde`):
///
///   - tags are always `u8`
///   - when serializing a sequence, the length of the sequence is usually `u16`
///
pub trait Serialize: Sized {
    /// Serialize construct into a binary output stream
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> Result<()>;
}

impl Serialize for u8 {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> Result<()> {
        writer.write_u8(*self)
    }
}

impl Serialize for u16 {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> Result<()> {
        writer.write_u16::<BigEndian>(*self)
    }
}

impl Serialize for u32 {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> Result<()> {
        writer.write_u32::<BigEndian>(*self)
    }
}

impl Serialize for u64 {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> Result<()> {
        writer.write_u64::<BigEndian>(*self)
    }
}

impl Serialize for i8 {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> Result<()> {
        writer.write_i8(*self)
    }
}

impl Serialize for i16 {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> Result<()> {
        writer.write_i16::<BigEndian>(*self)
    }
}

impl Serialize for i32 {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> Result<()> {
        writer.write_i32::<BigEndian>(*self)
    }
}

impl Serialize for i64 {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> Result<()> {
        writer.write_i64::<BigEndian>(*self)
    }
}

impl Serialize for f32 {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> Result<()> {
        writer.write_f32::<BigEndian>(*self)
    }
}

impl Serialize for f64 {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> Result<()> {
        writer.write_f64::<BigEndian>(*self)
    }
}

/// Size in `u16` is the first thing serialized/deserialized
impl<A: Serialize> Serialize for Vec<A> {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> Result<()> {
        (self.len() as u16).serialize(writer)?;
        for elem in self {
            elem.serialize(writer)?;
        }
        Ok(())
    }
}
