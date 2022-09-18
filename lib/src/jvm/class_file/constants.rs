use crate::jvm::class_file::{Attribute, AttributeLike, Serialize};
use crate::jvm::class_graph::{AccessMode, ClassId, ConstantData, FieldId, MethodId};
use crate::jvm::code::InvokeType;
use crate::jvm::descriptors::RenderDescriptor;
use crate::jvm::names::Name;
use crate::jvm::{Error, RefType};
use crate::util::{Offset, OffsetVec, Width};
use byteorder::WriteBytesExt;
use std::borrow::{Borrow, Cow};
use std::collections::HashMap;
use std::result::Result;

/// Class file constants pool builder
///
/// The pool is append only and only after the pool is fully built up, it can be consumed into a
/// regular [`OffsetVec`]. The [`ConstantsWriter`] trait exposes inserting types into the constants
/// pool.
pub struct ConstantsPool<'g> {
    constants: OffsetVec<Constant>,

    classes: HashMap<RefType<ClassId<'g>>, ClassConstantIndex>,
    fieldrefs: HashMap<FieldId<'g>, FieldRefConstantIndex>,
    methodrefs: HashMap<MethodId<'g>, MethodRefConstantIndex>,
    strings: HashMap<Utf8ConstantIndex, StringConstantIndex>,
    integers: HashMap<i32, ConstantIndex>,
    floats: HashMap<[u8; 4], ConstantIndex>,
    longs: HashMap<i64, ConstantIndex>,
    doubles: HashMap<[u8; 8], ConstantIndex>,
    name_and_types: HashMap<(Utf8ConstantIndex, Utf8ConstantIndex), NameAndTypeConstantIndex>,
    utf8s: HashMap<String, Utf8ConstantIndex>,
    method_handles: HashMap<(HandleKind, ConstantIndex), ConstantIndex>,
    method_types: HashMap<Utf8ConstantIndex, ConstantIndex>,
    invoke_dynamics: HashMap<(u16, NameAndTypeConstantIndex), InvokeDynamicConstantIndex>,
}

impl<'g> ConstantsPool<'g> {
    /// Make a fresh empty constants pool
    pub fn new() -> ConstantsPool<'g> {
        ConstantsPool {
            constants: OffsetVec::new_starting_at(Offset(1)),
            classes: HashMap::new(),
            fieldrefs: HashMap::new(),
            methodrefs: HashMap::new(),
            strings: HashMap::new(),
            integers: HashMap::new(),
            floats: HashMap::new(),
            longs: HashMap::new(),
            doubles: HashMap::new(),
            name_and_types: HashMap::new(),
            utf8s: HashMap::new(),
            method_handles: HashMap::new(),
            method_types: HashMap::new(),
            invoke_dynamics: HashMap::new(),
        }
    }

    /// List out all of the classes referenced in the constant pool
    pub fn referenced_classes(&self) -> impl Iterator<Item = ClassId<'g>> + '_ {
        self.classes
            .keys()
            .filter_map(|class: &RefType<ClassId<'g>>| -> Option<ClassId<'g>> {
                match class {
                    RefType::Object(cls) => Some(*cls),
                    RefType::ObjectArray(arr) => Some(arr.element_type),
                    _ => None,
                }
            })
    }

    /// Push a constant into the constant pool, provided there is space for it
    ///
    /// Note: the largest valid index is 65536, indexing starts at 1, and some constants take two
    /// spaces.
    fn push_constant(&mut self, constant: Constant) -> Result<ConstantIndex, ConstantPoolOverflow> {
        // Compute the offset at which this constant will be inserted
        let offset: u16 = self.constants.offset_len().0 as u16;

        // Detect if the next constant would overflow the pool
        if offset.checked_add(constant.width() as u16).is_none() {
            return Err(ConstantPoolOverflow { constant, offset });
        }

        self.constants.push(constant);
        Ok(ConstantIndex(offset))
    }

    /// Consume the pool and return the final vector of constants
    pub fn into_offset_vec(self) -> OffsetVec<Constant> {
        self.constants
    }

    /// Get or insert a utf8 constant from the constant pool
    pub fn get_utf8<'a, S: Into<Cow<'a, str>>>(
        &mut self,
        utf8: S,
    ) -> Result<Utf8ConstantIndex, ConstantPoolOverflow> {
        let cow = utf8.into();

        if let Some(idx) = self.utf8s.get::<str>(cow.borrow()) {
            Ok(*idx)
        } else {
            let owned = cow.into_owned();
            let constant = Constant::Utf8(owned.clone());
            let idx = Utf8ConstantIndex(self.push_constant(constant)?);
            self.utf8s.insert(owned, idx);
            Ok(idx)
        }
    }

    /// Get or insert a string constant from the constant pool
    pub fn get_string(
        &mut self,
        utf8: Utf8ConstantIndex,
    ) -> Result<StringConstantIndex, ConstantPoolOverflow> {
        if let Some(idx) = self.strings.get(&utf8) {
            Ok(*idx)
        } else {
            let constant = Constant::String(utf8);
            let idx = StringConstantIndex(self.push_constant(constant)?);
            self.strings.insert(utf8, idx);
            Ok(idx)
        }
    }

    /// Get or insert a name & type constant from the constant pool
    pub fn get_name_and_type(
        &mut self,
        name: Utf8ConstantIndex,
        descriptor: Utf8ConstantIndex,
    ) -> Result<NameAndTypeConstantIndex, ConstantPoolOverflow> {
        let name_and_type_key = (name, descriptor);
        if let Some(idx) = self.name_and_types.get(&name_and_type_key) {
            Ok(*idx)
        } else {
            let constant = Constant::NameAndType { name, descriptor };
            let idx = NameAndTypeConstantIndex(self.push_constant(constant)?);
            self.name_and_types.insert(name_and_type_key, idx);
            Ok(idx)
        }
    }

    /// Get or insert a method handle constant from the constant pool
    fn get_method_handle(
        &mut self,
        handle_kind: HandleKind,
        member: ConstantIndex,
    ) -> Result<ConstantIndex, ConstantPoolOverflow> {
        let handle_key = (handle_kind, member);
        if let Some(idx) = self.method_handles.get(&handle_key) {
            Ok(*idx)
        } else {
            let constant = Constant::MethodHandle {
                handle_kind,
                member,
            };
            let idx = self.push_constant(constant)?;
            self.method_handles.insert(handle_key, idx);
            Ok(idx)
        }
    }

    /// Get or insert an invoke dynamic constant from the constant pool
    pub fn get_invoke_dynamic(
        &mut self,
        bootstrap_method: u16,
        method_descriptor: NameAndTypeConstantIndex,
    ) -> Result<InvokeDynamicConstantIndex, ConstantPoolOverflow> {
        let indy_key = (bootstrap_method, method_descriptor);
        if let Some(idx) = self.invoke_dynamics.get(&indy_key) {
            Ok(*idx)
        } else {
            let constant = Constant::InvokeDynamic {
                bootstrap_method,
                method_descriptor,
            };
            let idx = InvokeDynamicConstantIndex(self.push_constant(constant)?);
            self.invoke_dynamics.insert(indy_key, idx);
            Ok(idx)
        }
    }

    /// Add an attribute to the constant pool
    pub fn get_attribute<A: AttributeLike>(&mut self, attribute: A) -> Result<Attribute, Error> {
        let name_index = self.get_utf8(A::NAME)?;
        let mut info = vec![];

        attribute.serialize(&mut info).map_err(Error::IoError)?;

        Ok(Attribute { name_index, info })
    }
}

#[derive(Debug)]
pub struct ConstantPoolOverflow {
    pub constant: Constant,
    pub offset: u16,
}

/// Constants as in the constant pool
///
/// Note: some constant types added after Java 8 are not included (since we don't generate them)
///
/// [0]: https://docs.oracle.com/javase/specs/jvms/se15/html/jvms-4.html#jvms-4.4
#[derive(Debug, Clone)]
pub enum Constant {
    /// Class or an interface
    Class(Utf8ConstantIndex),

    /// Field
    FieldRef(ClassConstantIndex, NameAndTypeConstantIndex),

    /// Method (this combines `Methodref` and `InterfaceMethodref`
    MethodRef {
        class: ClassConstantIndex,
        name_and_type: NameAndTypeConstantIndex,
        is_interface: bool,
    },

    /// Constant object of type `java.lang.String`
    String(Utf8ConstantIndex),

    /// Constant primitive of type `int`
    Integer(i32),

    /// Constant primitive of type `float`
    Float(f32),

    /// Constant primitive of type `long`
    Long(i64),

    /// Constant primitive of type `double`
    Double(f64),

    /// Name and a type (eg. for a field or a method)
    NameAndType {
        name: Utf8ConstantIndex,
        descriptor: Utf8ConstantIndex,
    },

    /// Constant UTF-8 encoded raw string value
    ///
    /// Despite the name, the encoding is not quite UTF-8 (the encoding of the
    /// null character `\u{0000}` and the encoding of supplementary characters
    /// is different).
    Utf8(String),

    /// Constant object of type `java.lang.invoke.MethodHandle`
    MethodHandle {
        handle_kind: HandleKind,

        /// Depending on the method kind, this points to different things:
        ///
        ///   - `FieldRef` for `GetField`, `GetStatic`, `PutField`, `PutStatic`
        ///   - `MethodRef` for the rest
        member: ConstantIndex,
    },

    /// Method type
    MethodType { descriptor: Utf8ConstantIndex },

    /// Dynamically-computed call site
    InvokeDynamic {
        /// Index into the `BootstrapMethods` attribute
        bootstrap_method: u16,
        method_descriptor: NameAndTypeConstantIndex,
    },
}

impl Serialize for Constant {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> std::io::Result<()> {
        match self {
            Constant::Utf8(string) => {
                1u8.serialize(writer)?;
                let mut buffer: Vec<u8> = encode_modified_utf8(string);
                (buffer.len() as u16).serialize(writer)?;
                writer.write_all(&mut buffer)?;
            }
            Constant::Integer(integer) => {
                3u8.serialize(writer)?;
                integer.serialize(writer)?; // TODO: confirm big endian is right
            }
            Constant::Float(float) => {
                4u8.serialize(writer)?;
                float.serialize(writer)?;
            }
            Constant::Long(long) => {
                5u8.serialize(writer)?;
                long.serialize(writer)?;
            }
            Constant::Double(double) => {
                6u8.serialize(writer)?;
                double.serialize(writer)?;
            }
            Constant::Class(name) => {
                7u8.serialize(writer)?;
                name.serialize(writer)?;
            }
            Constant::String(bytes) => {
                8u8.serialize(writer)?;
                bytes.serialize(writer)?;
            }
            Constant::FieldRef(class, name_and_type) => {
                9u8.serialize(writer)?;
                class.serialize(writer)?;
                name_and_type.serialize(writer)?;
            }
            Constant::MethodRef {
                class,
                name_and_type,
                is_interface,
            } => {
                (if !is_interface { 10u8 } else { 11u8 }).serialize(writer)?;
                class.serialize(writer)?;
                name_and_type.serialize(writer)?;
            }
            Constant::NameAndType { name, descriptor } => {
                12u8.serialize(writer)?;
                name.serialize(writer)?;
                descriptor.serialize(writer)?;
            }
            Constant::MethodHandle {
                handle_kind,
                member,
            } => {
                15u8.serialize(writer)?;
                handle_kind.serialize(writer)?;
                member.serialize(writer)?;
            }
            Constant::MethodType { descriptor } => {
                16u8.serialize(writer)?;
                descriptor.serialize(writer)?;
            }
            Constant::InvokeDynamic {
                bootstrap_method,
                method_descriptor,
            } => {
                18u8.serialize(writer)?;
                bootstrap_method.serialize(writer)?;
                method_descriptor.serialize(writer)?;
            }
        };
        Ok(())
    }
}

/// Modified UTF-8 format used in class files.
///
/// See [this `DataInput` section for details][0]. Quoting from that section:
///
/// > The differences between this format and the standard UTF-8 format are the following:
/// >
/// >  * The null byte `\u0000` is encoded in 2-byte format rather than 1-byte, so that the encoded
/// >    strings never have embedded nulls.
/// >  * Only the 1-byte, 2-byte, and 3-byte formats are used.
/// >  * Supplementary characters are represented in the form of surrogate pairs.
///
/// [0]: https://docs.oracle.com/en/java/javase/17/docs/api/java.base/java/io/DataInput.html#modified-utf-8
pub fn encode_modified_utf8(string: &str) -> Vec<u8> {
    let mut buffer: Vec<u8> = vec![];
    for c in string.chars() {
        // Handle the exception for how `\u{0000}` is represented
        let len: usize = if c == '\u{0000}' { 2 } else { c.len_utf8() };
        let code: u32 = c as u32;

        match len {
            1 => buffer.push(code as u8),
            2 => {
                buffer.push((code >> 6 & 0x1F) as u8 | 0b1100_0000);
                buffer.push((code & 0x3F) as u8 | 0b1000_0000);
            }
            3 => {
                buffer.push((code >> 12 & 0x0F) as u8 | 0b1110_0000);
                buffer.push((code >> 6 & 0x3F) as u8 | 0b1000_0000);
                buffer.push((code & 0x3F) as u8 | 0b1000_0000);
            }

            // Supplementary characters: main divergence from unicode
            _ => {
                buffer.push(0b1110_1101);
                buffer.push(((code >> 16 & 0x0F) as u8).wrapping_sub(1) & 0x0F | 0b1010_0000);
                buffer.push((code >> 10 & 0x3F) as u8 | 0b1000_0000);

                buffer.push(0b1110_1101);
                buffer.push(((code >> 6 & 0x1F) as u8) | 0b1011_0000);
                buffer.push((code & 0x3F) as u8 | 0b1000_0000);
            }
        }
    }
    buffer
}

#[cfg(test)]
mod encode_modified_utf8_tests {
    use super::*;

    #[test]
    fn containing_null_byte() {
        assert_eq!(encode_modified_utf8("a\x00a"), vec![97, 192, 128, 97]);
    }

    #[test]
    fn simple_ascii() {
        assert_eq!(encode_modified_utf8("foo"), vec![102, 111, 111]);
        assert_eq!(
            encode_modified_utf8("hel10_World"),
            vec![104, 101, 108, 49, 48, 95, 87, 111, 114, 108, 100]
        );
    }

    #[test]
    fn two_and_three_byte_encodings() {
        assert_eq!(
            encode_modified_utf8("ĄǍǞǠǺȀȂȦȺӐӒ"),
            vec![
                196, 132, 199, 141, 199, 158, 199, 160, 199, 186, 200, 128, 200, 130, 200, 166,
                200, 186, 211, 144, 211, 146
            ]
        );
        assert_eq!(
            encode_modified_utf8("ऄअॲঅਅઅଅஅఅಅഅะະ༁ཨ"),
            vec![
                224, 164, 132, 224, 164, 133, 224, 165, 178, 224, 166, 133, 224, 168, 133, 224,
                170, 133, 224, 172, 133, 224, 174, 133, 224, 176, 133, 224, 178, 133, 224, 180,
                133, 224, 184, 176, 224, 186, 176, 224, 188, 129, 224, 189, 168
            ]
        );
    }

    #[test]
    fn supplementary_characters() {
        assert_eq!(
            encode_modified_utf8("\u{10000}\u{dffff}\u{10FFFF}"),
            vec![
                237, 160, 128, 237, 176, 128, 237, 172, 191, 237, 191, 191, 237, 175, 191, 237,
                191, 191
            ]
        );
    }
}

/// Almost all constants have width 1, except for `Constant::Long` and `Constant::Double`. Quoting
/// the spec:
///
/// > All 8-byte constants take up two entries in the constant_pool table of the class file. If a
/// > CONSTANT_Long_info or CONSTANT_Double_info structure is the item in the constant_pool table
/// > at index n, then the next usable item in the pool is located at index n+2. The constant_pool
/// > index n+1 must be valid but is considered unusable.
/// >
/// > In retrospect, making 8-byte constants take two constant pool entries was a poor choice.
impl Width for Constant {
    fn width(&self) -> usize {
        match self {
            Constant::Long(_) | Constant::Double(_) => 2,
            _ => 1,
        }
    }
}

#[derive(Copy, Clone, Hash, Eq, PartialEq, Debug)]
pub struct ConstantIndex(pub u16);

#[derive(Copy, Clone, Hash, Eq, PartialEq, Debug)]
pub struct Utf8ConstantIndex(pub ConstantIndex);

#[derive(Copy, Clone, Hash, Eq, PartialEq, Debug)]
pub struct StringConstantIndex(pub ConstantIndex);

#[derive(Copy, Clone, Hash, Eq, PartialEq, Debug)]
pub struct NameAndTypeConstantIndex(ConstantIndex);

#[derive(Copy, Clone, Hash, Eq, PartialEq, Debug)]
pub struct MethodTypeConstantIndex(ConstantIndex);

#[derive(Copy, Clone, Hash, Eq, PartialEq, Debug)]
pub struct ClassConstantIndex(ConstantIndex);

#[derive(Copy, Clone, Hash, Eq, PartialEq, Debug)]
pub struct FieldRefConstantIndex(ConstantIndex);

#[derive(Copy, Clone, Hash, Eq, PartialEq, Debug)]
pub struct MethodRefConstantIndex(ConstantIndex);

#[derive(Copy, Clone, Hash, Eq, PartialEq, Debug)]
pub struct InvokeDynamicConstantIndex(ConstantIndex);

impl Into<ConstantIndex> for Utf8ConstantIndex {
    fn into(self) -> ConstantIndex {
        self.0
    }
}
impl Into<ConstantIndex> for StringConstantIndex {
    fn into(self) -> ConstantIndex {
        self.0
    }
}
impl Into<ConstantIndex> for NameAndTypeConstantIndex {
    fn into(self) -> ConstantIndex {
        self.0
    }
}
impl Into<ConstantIndex> for MethodTypeConstantIndex {
    fn into(self) -> ConstantIndex {
        self.0
    }
}
impl Into<ConstantIndex> for ClassConstantIndex {
    fn into(self) -> ConstantIndex {
        self.0
    }
}
impl Into<ConstantIndex> for FieldRefConstantIndex {
    fn into(self) -> ConstantIndex {
        self.0
    }
}
impl Into<ConstantIndex> for MethodRefConstantIndex {
    fn into(self) -> ConstantIndex {
        self.0
    }
}
impl Into<ConstantIndex> for InvokeDynamicConstantIndex {
    fn into(self) -> ConstantIndex {
        self.0
    }
}

impl Serialize for ConstantIndex {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> std::io::Result<()> {
        self.0.serialize(writer)
    }
}
impl Serialize for Utf8ConstantIndex {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> std::io::Result<()> {
        self.0.serialize(writer)
    }
}
impl Serialize for StringConstantIndex {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> std::io::Result<()> {
        self.0.serialize(writer)
    }
}
impl Serialize for NameAndTypeConstantIndex {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> std::io::Result<()> {
        self.0.serialize(writer)
    }
}
impl Serialize for MethodTypeConstantIndex {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> std::io::Result<()> {
        self.0.serialize(writer)
    }
}
impl Serialize for ClassConstantIndex {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> std::io::Result<()> {
        self.0.serialize(writer)
    }
}
impl Serialize for FieldRefConstantIndex {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> std::io::Result<()> {
        self.0.serialize(writer)
    }
}
impl Serialize for MethodRefConstantIndex {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> std::io::Result<()> {
        self.0.serialize(writer)
    }
}
impl Serialize for InvokeDynamicConstantIndex {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> std::io::Result<()> {
        self.0.serialize(writer)
    }
}

/// Type of method handle
///
/// [0]: https://docs.oracle.com/javase/specs/jvms/se15/html/jvms-5.html#jvms-5.4.3.5-220
#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
pub enum HandleKind {
    GetField,
    GetStatic,
    PutField,
    PutStatic,
    InvokeVirtual,
    InvokeStatic,
    InvokeSpecial,
    NewInvokeSpecial,
    InvokeInterface,
}

impl Serialize for HandleKind {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> std::io::Result<()> {
        let byte: u8 = match self {
            HandleKind::GetField => 1,
            HandleKind::GetStatic => 2,
            HandleKind::PutField => 3,
            HandleKind::PutStatic => 4,
            HandleKind::InvokeVirtual => 5,
            HandleKind::InvokeStatic => 6,
            HandleKind::InvokeSpecial => 7,
            HandleKind::NewInvokeSpecial => 8,
            HandleKind::InvokeInterface => 9,
        };
        byte.serialize(writer)
    }
}

pub trait ConstantsWriter<'g, Index = ConstantIndex> {
    /// Get or insert a constant into the constant pool and return the associated index
    fn constant_index(
        &self,
        constants_pool: &mut ConstantsPool<'g>,
    ) -> Result<Index, ConstantPoolOverflow>;
}

/// When making a `CONSTANT_Class_info`, reference types are almost always objects. However,
/// there are a handful of places where an array type needs to be fit in (eg. for a `checkcast`
/// to an array type). See [this section of the spec][0] for more.
///
/// [0]: https://docs.oracle.com/javase/specs/jvms/se17/html/jvms-4.html#jvms-4.4.1
impl<'g> ConstantsWriter<'g, ClassConstantIndex> for RefType<ClassId<'g>> {
    fn constant_index(
        &self,
        constants: &mut ConstantsPool<'g>,
    ) -> Result<ClassConstantIndex, ConstantPoolOverflow> {
        if let Some(idx) = constants.classes.get(self) {
            Ok(*idx)
        } else {
            let name = match self {
                RefType::Object(class) => constants.get_utf8(class.name.as_str())?,
                other => constants.get_utf8(other.render())?,
            };
            let constant = Constant::Class(name);
            let idx = ClassConstantIndex(constants.push_constant(constant)?);
            constants.classes.insert(*self, idx);
            Ok(idx)
        }
    }
}

/// Write a `CONSTANT_Class_info`
impl<'g> ConstantsWriter<'g, ClassConstantIndex> for ClassId<'g> {
    fn constant_index(
        &self,
        constants: &mut ConstantsPool<'g>,
    ) -> Result<ClassConstantIndex, ConstantPoolOverflow> {
        RefType::Object(*self).constant_index(constants)
    }
}

/// Write a `CONSTANT_Methodref_info` or `CONSTANT_InterfaceMethodref_info`
impl<'g> ConstantsWriter<'g, MethodRefConstantIndex> for MethodId<'g> {
    fn constant_index(
        &self,
        constants: &mut ConstantsPool<'g>,
    ) -> Result<MethodRefConstantIndex, ConstantPoolOverflow> {
        if let Some(idx) = constants.methodrefs.get(self) {
            Ok(*idx)
        } else {
            let class_idx = self.class.constant_index(constants)?;
            let method_utf8 = constants.get_utf8(self.name.as_str())?;
            let desc_utf8 = constants.get_utf8(&self.descriptor.render())?;
            let name_and_type_idx = constants.get_name_and_type(method_utf8, desc_utf8)?;
            let constant = Constant::MethodRef {
                class: class_idx,
                name_and_type: name_and_type_idx,
                is_interface: self.class.is_interface(),
            };
            let idx = MethodRefConstantIndex(constants.push_constant(constant)?);
            constants.methodrefs.insert(*self, idx);
            Ok(idx)
        }
    }
}

/// Write a `CONSTANT_Fieldref_info`
impl<'g> ConstantsWriter<'g, FieldRefConstantIndex> for FieldId<'g> {
    fn constant_index(
        &self,
        constants: &mut ConstantsPool<'g>,
    ) -> Result<FieldRefConstantIndex, ConstantPoolOverflow> {
        if let Some(idx) = constants.fieldrefs.get(self) {
            Ok(*idx)
        } else {
            let class_idx = self.class.constant_index(constants)?;
            let field_utf8 = constants.get_utf8(self.name.as_str())?;
            let desc_utf8 = constants.get_utf8(&self.descriptor.render())?;
            let name_and_type_idx = constants.get_name_and_type(field_utf8, desc_utf8)?;
            let constant = Constant::FieldRef(class_idx, name_and_type_idx);
            let idx = FieldRefConstantIndex(constants.push_constant(constant)?);
            constants.fieldrefs.insert(*self, idx);
            Ok(idx)
        }
    }
}

/// Write a constant which can be loaded up using `ldc` or `ldc_2`
impl<'g> ConstantsWriter<'g, ConstantIndex> for ConstantData<'g> {
    fn constant_index(
        &self,
        constants: &mut ConstantsPool<'g>,
    ) -> Result<ConstantIndex, ConstantPoolOverflow> {
        match self {
            ConstantData::String(string) => {
                let str_utf8 = constants.get_utf8(&**string)?;
                let str_idx = constants.get_string(str_utf8)?;
                Ok(str_idx.into())
            }
            ConstantData::Class(class) => Ok(class.constant_index(constants)?.into()),
            ConstantData::Integer(integer) => {
                if let Some(idx) = constants.integers.get(integer) {
                    Ok(*idx)
                } else {
                    let idx = constants.push_constant(Constant::Integer(*integer))?;
                    constants.integers.insert(*integer, idx);
                    Ok(idx)
                }
            }
            ConstantData::Long(long) => {
                if let Some(idx) = constants.longs.get(long) {
                    Ok(*idx)
                } else {
                    let idx = constants.push_constant(Constant::Long(*long))?;
                    constants.longs.insert(*long, idx);
                    Ok(idx)
                }
            }
            ConstantData::Float(float_bytes) => {
                if let Some(idx) = constants.floats.get(float_bytes) {
                    Ok(*idx)
                } else {
                    let float = f32::from_le_bytes(*float_bytes);
                    let idx = constants.push_constant(Constant::Float(float))?;
                    constants.floats.insert(*float_bytes, idx);
                    Ok(idx)
                }
            }
            ConstantData::Double(double_bytes) => {
                if let Some(idx) = constants.doubles.get(double_bytes) {
                    Ok(*idx)
                } else {
                    let double = f64::from_le_bytes(*double_bytes);
                    let idx = constants.push_constant(Constant::Double(double))?;
                    constants.doubles.insert(*double_bytes, idx);
                    Ok(idx)
                }
            }
            ConstantData::FieldHandle(access_mode, field) => {
                let field_idx = field.constant_index(constants)?;
                let handle = match (access_mode, field.is_static()) {
                    (AccessMode::Read, true) => HandleKind::GetStatic,
                    (AccessMode::Read, false) => HandleKind::GetField,
                    (AccessMode::Write, true) => HandleKind::PutStatic,
                    (AccessMode::Write, false) => HandleKind::PutField,
                };
                constants.get_method_handle(handle, field_idx.into())
            }
            ConstantData::MethodHandle(method) => {
                let method_idx = method.constant_index(constants)?;
                let handle = match method.infer_invoke_type() {
                    InvokeType::Static => HandleKind::InvokeStatic,
                    InvokeType::Special => HandleKind::NewInvokeSpecial,
                    InvokeType::Interface(_) => HandleKind::InvokeInterface,
                    InvokeType::Virtual => HandleKind::InvokeVirtual,
                };
                constants.get_method_handle(handle, method_idx.into())
            }
            ConstantData::MethodType(method) => {
                let descriptor = constants.get_utf8(method.render())?;
                if let Some(idx) = constants.method_types.get(&descriptor) {
                    Ok(*idx)
                } else {
                    let constant = Constant::MethodType { descriptor };
                    let idx = constants.push_constant(constant)?;
                    constants.method_types.insert(descriptor, idx);
                    Ok(idx)
                }
            }
        }
    }
}
