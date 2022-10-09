use crate::jvm::class_file::{Attribute, AttributeLike, Serialize};
use crate::jvm::class_graph::{AccessMode, ClassId, ConstantData, FieldId, MethodId};
use crate::jvm::code::InvokeType;
use crate::jvm::descriptors::RenderDescriptor;
use crate::jvm::names::Name;
use crate::jvm::{Error, RefType};
use crate::util::{Offset, OffsetVec, Width};
use byteorder::WriteBytesExt;
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
    pub fn get_utf8(
        &mut self,
        utf8: impl AsRef<str>,
    ) -> Result<Utf8ConstantIndex, ConstantPoolOverflow> {
        if let Some(idx) = self.utf8s.get(utf8.as_ref()) {
            Ok(*idx)
        } else {
            let owned = utf8.as_ref().to_string();
            let constant = Constant::Utf8(owned.clone());
            let idx = self.push_constant(constant)?;
            self.utf8s.insert(owned, idx);
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
            let idx = self.push_constant(constant)?;
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
            let idx = self.push_constant(constant)?;
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

impl<'g> Default for ConstantsPool<'g> {
    fn default() -> Self {
        Self::new()
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
/// [0]: https://docs.oracle.com/javase/specs/jvms/se17/html/jvms-4.html#jvms-4.4
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
                let buffer: Vec<u8> = encode_modified_utf8(string);
                (buffer.len() as u16).serialize(writer)?;
                writer.write_all(&buffer)?;
            }
            Constant::Integer(integer) => {
                3u8.serialize(writer)?;
                integer.serialize(writer)?;
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

/// Index into a constant pool.
///
/// Note that the constant pool indexing starts at one, not zero.
#[derive(Copy, Clone, Hash, Eq, PartialEq, Debug)]
pub struct ConstantIndex(pub u16);

impl ConstantIndex {
    /// The zero-index is special because the constant pool starts indexing at one. Consequently,
    /// this index serves as a sort of "null" index (see the superclass name for a use).
    pub const ZERO: ConstantIndex = ConstantIndex(0);
}

/// Constant index pointing to a [`Constant::String`]
pub type Utf8ConstantIndex = ConstantIndex;

/// Constant index pointing to a [`Constant::String`]
pub type StringConstantIndex = ConstantIndex;

/// Constant index pointing to a [`Constant::NameAndType`]
pub type NameAndTypeConstantIndex = ConstantIndex;

/// Constant index pointing to a [`Constant::MethodType`]
pub type MethodTypeConstantIndex = ConstantIndex;

/// Constant index pointing to a [`Constant::Class`]
pub type ClassConstantIndex = ConstantIndex;

/// Constant index pointing to a [`Constant::FieldRef`]
pub type FieldRefConstantIndex = ConstantIndex;

/// Constant index pointing to a [`Constant::MethodRef`]
pub type MethodRefConstantIndex = ConstantIndex;

/// Constant index pointing to a [`Constant::InvokeDynamic`]
pub type InvokeDynamicConstantIndex = ConstantIndex;

impl Serialize for ConstantIndex {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> std::io::Result<()> {
        self.0.serialize(writer)
    }
}

/// Type of method handle
///
/// [0]: https://docs.oracle.com/javase/specs/jvms/se17/html/jvms-5.html#jvms-5.4.3.5-220
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
            let idx = constants.push_constant(constant)?;
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
            let idx = constants.push_constant(constant)?;
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
            let idx = constants.push_constant(constant)?;
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
                let utf8 = constants.get_utf8(&**string)?;
                if let Some(idx) = constants.strings.get(&utf8) {
                    Ok(*idx)
                } else {
                    let constant = Constant::String(utf8);
                    let idx = constants.push_constant(constant)?;
                    constants.strings.insert(utf8, idx);
                    Ok(idx)
                }
            }
            ConstantData::Class(class) => Ok(class.constant_index(constants)?),
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
                constants.get_method_handle(handle, field_idx)
            }
            ConstantData::MethodHandle(method) => {
                let method_idx = method.constant_index(constants)?;
                let handle = match method.infer_invoke_type() {
                    InvokeType::Static => HandleKind::InvokeStatic,
                    InvokeType::Special => HandleKind::NewInvokeSpecial,
                    InvokeType::Interface(_) => HandleKind::InvokeInterface,
                    InvokeType::Virtual => HandleKind::InvokeVirtual,
                };
                constants.get_method_handle(handle, method_idx)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jvm::class_graph::{ClassData, ClassGraph, ClassGraphArenas, FieldData};
    use crate::jvm::{
        ArrayType, BinaryName, ClassAccessFlags, FieldAccessFlags, FieldType, UnqualifiedName,
    };

    fn off(constant_idx: ConstantIndex) -> Offset {
        Offset(constant_idx.0 as usize)
    }

    fn assert_constant_eq(found: &Constant, expected: &Constant) {
        match (found, expected) {
            (Constant::Class(utf81), Constant::Class(utf82)) => {
                assert_eq!(utf81, utf82, "class name");
            }
            (Constant::FieldRef(cls1, typ1), Constant::FieldRef(cls2, typ2)) => {
                assert_eq!(cls1, cls2, "field class");
                assert_eq!(typ1, typ2, "field name and type");
            }
            (
                Constant::MethodRef {
                    class: cls1,
                    name_and_type: typ1,
                    is_interface: i1,
                },
                Constant::MethodRef {
                    class: cls2,
                    name_and_type: typ2,
                    is_interface: i2,
                },
            ) => {
                assert_eq!(cls1, cls2, "method class");
                assert_eq!(typ1, typ2, "method name and type");
                assert_eq!(i1, i2, "method interface status");
            }
            (Constant::String(utf81), Constant::String(utf82)) => {
                assert_eq!(utf81, utf82, "string contents");
            }
            (Constant::Integer(i1), Constant::Integer(i2)) => assert_eq!(i1, i2),
            (Constant::Float(f1), Constant::Float(f2)) => assert_eq!(f1, f2),
            (Constant::Long(l1), Constant::Long(l2)) => assert_eq!(l1, l2),
            (Constant::Double(d1), Constant::Double(d2)) => assert_eq!(d1, d2),
            (
                Constant::NameAndType {
                    name: name1,
                    descriptor: typ1,
                },
                Constant::NameAndType {
                    name: name2,
                    descriptor: typ2,
                },
            ) => {
                assert_eq!(name1, name2, "name");
                assert_eq!(typ1, typ2, "type");
            }
            (
                Constant::MethodHandle {
                    handle_kind: kind1,
                    member: member1,
                },
                Constant::MethodHandle {
                    handle_kind: kind2,
                    member: member2,
                },
            ) => {
                assert_eq!(kind1, kind2, "method handle kind");
                assert_eq!(member1, member2, "method handle member");
            }
            (
                Constant::MethodType { descriptor: typ1 },
                Constant::MethodType { descriptor: typ2 },
            ) => {
                assert_eq!(typ1, typ2, "method type");
            }
            (
                Constant::InvokeDynamic {
                    bootstrap_method: bootstrap1,
                    method_descriptor: typ1,
                },
                Constant::InvokeDynamic {
                    bootstrap_method: bootstrap2,
                    method_descriptor: typ2,
                },
            ) => {
                assert_eq!(bootstrap1, bootstrap2, "invoke dynamic bootstrap method");
                assert_eq!(typ1, typ2, "invoke dynamic descriptor");
            }
            (Constant::Utf8(s1), Constant::Utf8(s2)) => assert_eq!(s1, s2),
            _ => panic!("Found {:?} but expected {:?}", found, expected),
        }
    }

    // Assert the found constants match the expected constants
    fn assert_constants_eq(
        found: impl IntoIterator<Item = Constant>,
        expected: impl IntoIterator<Item = Constant>,
    ) {
        let mut found_iter = found.into_iter();
        let mut expected_iter = expected.into_iter();
        loop {
            match (found_iter.next(), expected_iter.next()) {
                (None, None) => return,
                (Some(left), None) => panic!("Expected {:?} but found no more elements", left),
                (None, Some(right)) => panic!("Expected no more elements but found {:?}", right),
                (Some(left), Some(right)) => {
                    assert_constant_eq(&left, &right);
                }
            }
        }
    }

    #[test]
    #[allow(illegal_floating_point_literal_pattern)]
    fn numeric_constants() {
        let mut pool = ConstantsPool::new();
        let integer_idx = ConstantData::Integer(123)
            .constant_index(&mut pool)
            .unwrap();
        let long_idx = ConstantData::Long(123).constant_index(&mut pool).unwrap();
        let float_idx = ConstantData::float(123.0)
            .constant_index(&mut pool)
            .unwrap();
        let double_idx = ConstantData::double(123.0)
            .constant_index(&mut pool)
            .unwrap();
        let constants = pool.into_offset_vec();

        assert_eq!(constants.len(), 4);
        assert_eq!(integer_idx, ConstantIndex(1));
        assert_eq!(long_idx, ConstantIndex(2));
        assert_eq!(float_idx, ConstantIndex(4));
        assert_eq!(double_idx, ConstantIndex(5));
        assert_eq!(constants.offset_len(), Offset(7));

        assert!(matches!(
            constants.get_offset(off(integer_idx)).ok(),
            Some(&Constant::Integer(123))
        ));
        assert!(matches!(
            constants.get_offset(off(long_idx)).ok(),
            Some(&Constant::Long(123))
        ));
        assert!(matches!(
            constants.get_offset(off(float_idx)).ok(),
            Some(&Constant::Float(123.0))
        ));
        assert!(matches!(
            constants.get_offset(off(double_idx)).ok(),
            Some(&Constant::Double(123.0))
        ));
    }

    #[test]
    fn string_constants() {
        let mut pool = ConstantsPool::new();
        let utf8_idx = pool.get_utf8("foo bar").unwrap();
        let string_idx = ConstantData::String("hello world".into())
            .constant_index(&mut pool)
            .unwrap();
        let constants = pool.into_offset_vec();

        assert_eq!(constants.len(), 3);
        assert_eq!(utf8_idx, ConstantIndex(1));
        assert_eq!(string_idx, ConstantIndex(3));
        assert_eq!(constants.offset_len(), Offset(4));

        assert_constants_eq(
            constants.into_iter().map(|(_, _, c)| c),
            [
                Constant::Utf8("foo bar".to_string()),
                Constant::Utf8("hello world".to_string()),
                Constant::String(ConstantIndex(2)),
            ],
        );
    }

    #[test]
    fn classgraph_id_constants() {
        let class_graph_arenas = ClassGraphArenas::new();
        let class_graph = ClassGraph::new(&class_graph_arenas);
        let java = class_graph.insert_java_library_types();

        let mut pool = ConstantsPool::new();
        let integer_cls_idx = java.classes.lang.integer.constant_index(&mut pool).unwrap();
        let maxvalue_fld_idx = java
            .members
            .lang
            .long
            .max_value
            .constant_index(&mut pool)
            .unwrap();
        let sqrt_mthd_idx = java
            .members
            .lang
            .math
            .sqrt
            .constant_index(&mut pool)
            .unwrap();
        let constants = pool.into_offset_vec();

        assert_eq!(integer_cls_idx, ConstantIndex(2));
        assert_eq!(maxvalue_fld_idx, ConstantIndex(8));
        assert_eq!(sqrt_mthd_idx, ConstantIndex(14));

        assert_constants_eq(
            constants.into_iter().map(|(_, _, c)| c),
            [
                Constant::Utf8("java/lang/Integer".to_string()),
                Constant::Class(ConstantIndex(1)),
                Constant::Utf8("java/lang/Long".to_string()),
                Constant::Class(ConstantIndex(3)),
                Constant::Utf8("MAX_VALUE".to_string()),
                Constant::Utf8("J".to_string()),
                Constant::NameAndType {
                    name: ConstantIndex(5),
                    descriptor: ConstantIndex(6),
                },
                Constant::FieldRef(ConstantIndex(4), ConstantIndex(7)),
                Constant::Utf8("java/lang/Math".to_string()),
                Constant::Class(ConstantIndex(9)),
                Constant::Utf8("sqrt".to_string()),
                Constant::Utf8("(D)D".to_string()),
                Constant::NameAndType {
                    name: ConstantIndex(11),
                    descriptor: ConstantIndex(12),
                },
                Constant::MethodRef {
                    class: ConstantIndex(10),
                    name_and_type: ConstantIndex(13),
                    is_interface: false,
                },
            ],
        );
    }

    #[test]
    fn method_handle_constants() {
        let class_graph_arenas = ClassGraphArenas::new();
        let class_graph = ClassGraph::new(&class_graph_arenas);
        let java = class_graph.insert_java_library_types();

        let my_class = class_graph.add_class(ClassData::new(
            BinaryName::from_str("me/MyClass").unwrap(),
            java.classes.lang.object,
            ClassAccessFlags::PUBLIC,
            None,
        ));
        let my_field = class_graph.add_field(FieldData {
            class: my_class,
            name: UnqualifiedName::from_str("myField").unwrap(),
            access_flags: FieldAccessFlags::PUBLIC,
            descriptor: FieldType::long(),
        });
        let my_field2 = class_graph.add_field(FieldData {
            class: my_class,
            name: UnqualifiedName::from_str("myField2").unwrap(),
            access_flags: FieldAccessFlags::PUBLIC | FieldAccessFlags::STATIC,
            descriptor: FieldType::long(),
        });

        let mut pool = ConstantsPool::new();
        let get_field_idx = ConstantData::FieldHandle(AccessMode::Read, my_field)
            .constant_index(&mut pool)
            .unwrap();
        let put_field_idx = ConstantData::FieldHandle(AccessMode::Write, my_field)
            .constant_index(&mut pool)
            .unwrap();
        let get_static_idx = ConstantData::FieldHandle(AccessMode::Read, my_field2)
            .constant_index(&mut pool)
            .unwrap();
        let put_static_idx = ConstantData::FieldHandle(AccessMode::Write, my_field2)
            .constant_index(&mut pool)
            .unwrap();
        let interface_idx = ConstantData::MethodHandle(java.members.lang.char_sequence.length)
            .constant_index(&mut pool)
            .unwrap();
        let virtual_idx = ConstantData::MethodHandle(java.members.lang.string.get_bytes)
            .constant_index(&mut pool)
            .unwrap();
        let static_idx = ConstantData::MethodHandle(java.members.lang.integer.bit_count)
            .constant_index(&mut pool)
            .unwrap();
        let special_idx = ConstantData::MethodHandle(java.members.lang.object.init)
            .constant_index(&mut pool)
            .unwrap();
        let constants = pool.into_offset_vec();

        assert_eq!(get_field_idx, ConstantIndex(7));
        assert_eq!(put_field_idx, ConstantIndex(8));
        assert_eq!(get_static_idx, ConstantIndex(12));
        assert_eq!(put_static_idx, ConstantIndex(13));
        assert_eq!(interface_idx, ConstantIndex(20));
        assert_eq!(virtual_idx, ConstantIndex(27));
        assert_eq!(static_idx, ConstantIndex(34));
        assert_eq!(special_idx, ConstantIndex(41));

        use HandleKind::*;

        assert_constants_eq(
            constants.into_iter().map(|(_, _, c)| c),
            [
                Constant::Utf8("me/MyClass".to_string()),
                Constant::Class(ConstantIndex(1)),
                Constant::Utf8("myField".to_string()),
                Constant::Utf8("J".to_string()),
                Constant::NameAndType {
                    name: ConstantIndex(3),
                    descriptor: ConstantIndex(4),
                },
                Constant::FieldRef(ConstantIndex(2), ConstantIndex(5)),
                Constant::MethodHandle {
                    handle_kind: GetField,
                    member: ConstantIndex(6),
                },
                Constant::MethodHandle {
                    handle_kind: PutField,
                    member: ConstantIndex(6),
                },
                Constant::Utf8("myField2".to_string()),
                Constant::NameAndType {
                    name: ConstantIndex(9),
                    descriptor: ConstantIndex(4),
                },
                Constant::FieldRef(ConstantIndex(2), ConstantIndex(10)),
                Constant::MethodHandle {
                    handle_kind: GetStatic,
                    member: ConstantIndex(11),
                },
                Constant::MethodHandle {
                    handle_kind: PutStatic,
                    member: ConstantIndex(11),
                },
                Constant::Utf8("java/lang/CharSequence".to_string()),
                Constant::Class(ConstantIndex(14)),
                Constant::Utf8("length".to_string()),
                Constant::Utf8("()I".to_string()),
                Constant::NameAndType {
                    name: ConstantIndex(16),
                    descriptor: ConstantIndex(17),
                },
                Constant::MethodRef {
                    class: ConstantIndex(15),
                    name_and_type: ConstantIndex(18),
                    is_interface: true,
                },
                Constant::MethodHandle {
                    handle_kind: InvokeInterface,
                    member: ConstantIndex(19),
                },
                Constant::Utf8("java/lang/String".to_string()),
                Constant::Class(ConstantIndex(21)),
                Constant::Utf8("getBytes".to_string()),
                Constant::Utf8("(Ljava/lang/String;)[B".to_string()),
                Constant::NameAndType {
                    name: ConstantIndex(23),
                    descriptor: ConstantIndex(24),
                },
                Constant::MethodRef {
                    class: ConstantIndex(22),
                    name_and_type: ConstantIndex(25),
                    is_interface: false,
                },
                Constant::MethodHandle {
                    handle_kind: InvokeVirtual,
                    member: ConstantIndex(26),
                },
                Constant::Utf8("java/lang/Integer".to_string()),
                Constant::Class(ConstantIndex(28)),
                Constant::Utf8("bitCount".to_string()),
                Constant::Utf8("(I)I".to_string()),
                Constant::NameAndType {
                    name: ConstantIndex(30),
                    descriptor: ConstantIndex(31),
                },
                Constant::MethodRef {
                    class: ConstantIndex(29),
                    name_and_type: ConstantIndex(32),
                    is_interface: false,
                },
                Constant::MethodHandle {
                    handle_kind: InvokeStatic,
                    member: ConstantIndex(33),
                },
                Constant::Utf8("java/lang/Object".to_string()),
                Constant::Class(ConstantIndex(35)),
                Constant::Utf8("<init>".to_string()),
                Constant::Utf8("()V".to_string()),
                Constant::NameAndType {
                    name: ConstantIndex(37),
                    descriptor: ConstantIndex(38),
                },
                Constant::MethodRef {
                    class: ConstantIndex(36),
                    name_and_type: ConstantIndex(39),
                    is_interface: false,
                },
                Constant::MethodHandle {
                    handle_kind: NewInvokeSpecial,
                    member: ConstantIndex(40),
                },
            ],
        );
    }

    // See the comment on the `RefType` impl of `ConstantsWriter`
    #[test]
    fn array_type_constants() {
        let class_graph_arenas = ClassGraphArenas::new();
        let class_graph = ClassGraph::new(&class_graph_arenas);
        let java = class_graph.insert_java_library_types();

        let mut pool = ConstantsPool::new();
        let integer1 = RefType::Object(java.classes.lang.integer)
            .constant_index(&mut pool)
            .unwrap();
        let integer2 = java.classes.lang.integer.constant_index(&mut pool).unwrap();
        let integer_arr = RefType::ObjectArray(ArrayType {
            additional_dimensions: 0,
            element_type: java.classes.lang.integer,
        })
        .constant_index(&mut pool)
        .unwrap();
        let int_arr = RefType::ObjectArray(ArrayType {
            additional_dimensions: 2,
            element_type: java.classes.lang.integer,
        })
        .constant_index(&mut pool)
        .unwrap();
        let constants = pool.into_offset_vec();

        assert_eq!(constants.len(), 6);
        assert_eq!(integer1, ConstantIndex(2));
        assert_eq!(integer2, integer1);
        assert_eq!(integer_arr, ConstantIndex(4));
        assert_eq!(int_arr, ConstantIndex(6));
        assert_eq!(constants.offset_len(), Offset(7));

        assert_constants_eq(
            constants.into_iter().map(|(_, _, c)| c),
            [
                Constant::Utf8("java/lang/Integer".to_string()),
                Constant::Class(ConstantIndex(1)),
                Constant::Utf8("[Ljava/lang/Integer;".to_string()),
                Constant::Class(ConstantIndex(3)),
                Constant::Utf8("[[[Ljava/lang/Integer;".to_string()),
                Constant::Class(ConstantIndex(5)),
            ],
        );
    }
}
