use super::{
    Attribute, AttributeLike, ConstantPoolOverflow, Error, Offset, OffsetResult, OffsetVec,
    Serialize, Width,
};
use byteorder::WriteBytesExt;
use elsa::map::FrozenMap;
use elsa::vec::FrozenVec;
use std::borrow::{Borrow, Cow};
use std::result::Result;

/// Class file constants pool builder
///
/// The pool is append only, which makes it possible to get-or-else-insert without requiring
/// mutable access. After the pool is fully built up, it can be consumed into a regular
/// `OffsetVec`.
pub struct ConstantsPool {
    constants: FrozenVec<Box<(u16, Constant)>>,

    classes: FrozenMap<Utf8ConstantIndex, Box<ClassConstantIndex>>,
    fieldrefs:
        FrozenMap<(ClassConstantIndex, NameAndTypeConstantIndex), Box<FieldRefConstantIndex>>,
    methodrefs: FrozenMap<
        (ClassConstantIndex, NameAndTypeConstantIndex, bool),
        Box<MethodRefConstantIndex>,
    >,
    strings: FrozenMap<Utf8ConstantIndex, Box<StringConstantIndex>>,
    integers: FrozenMap<i32, Box<ConstantIndex>>,
    floats: FrozenMap<[u8; 4], Box<ConstantIndex>>,
    longs: FrozenMap<i64, Box<ConstantIndex>>,
    doubles: FrozenMap<[u8; 8], Box<ConstantIndex>>,
    name_and_types:
        FrozenMap<(Utf8ConstantIndex, Utf8ConstantIndex), Box<NameAndTypeConstantIndex>>,
    utf8s: FrozenMap<String, Box<Utf8ConstantIndex>>,
    method_handles: FrozenMap<(HandleKind, ConstantIndex), Box<ConstantIndex>>,
    method_types: FrozenMap<Utf8ConstantIndex, Box<ConstantIndex>>,
    invoke_dynamics: FrozenMap<(u16, NameAndTypeConstantIndex), Box<InvokeDynamicConstantIndex>>,
}

impl ConstantsPool {
    /// Make a fresh empty constants pool
    pub fn new() -> ConstantsPool {
        ConstantsPool {
            constants: FrozenVec::new(),
            classes: FrozenMap::new(),
            fieldrefs: FrozenMap::new(),
            methodrefs: FrozenMap::new(),
            strings: FrozenMap::new(),
            integers: FrozenMap::new(),
            floats: FrozenMap::new(),
            longs: FrozenMap::new(),
            doubles: FrozenMap::new(),
            name_and_types: FrozenMap::new(),
            utf8s: FrozenMap::new(),
            method_handles: FrozenMap::new(),
            method_types: FrozenMap::new(),
            invoke_dynamics: FrozenMap::new(),
        }
    }

    /// Push a constant into the constant pool, provided there is space for it
    ///
    /// Note: the largest valid index is 65536, indexing starts at 1, and some constants take two
    /// spaces.
    fn push_constant(&self, constant: Constant) -> Result<ConstantIndex, ConstantPoolOverflow> {
        // Compute the offset at which this constant will be inserted
        let offset: u16 = match self.constants.last() {
            None => 1, // constant pool starts at 1, not 0
            Some((off, cnst)) => off + (cnst.width() as u16),
        };

        // Detect if the next constant would overflow the pool
        if offset.checked_add(constant.width() as u16).is_none() {
            return Err(ConstantPoolOverflow { constant, offset });
        }

        self.constants.push(Box::new((offset, constant)));
        Ok(ConstantIndex(offset))
    }

    /// Consume the pool and return the final vector of constants
    pub fn into_offset_vec(self) -> OffsetVec<Constant> {
        let mut output = OffsetVec::new_starting_at(Offset(1));
        output.extend(self.constants.into_vec().into_iter().map(|entry| entry.1));
        output
    }

    /// Get a constant from the pool
    pub fn get(&self, index: ConstantIndex) -> OffsetResult<Constant> {
        match self
            .constants
            .binary_search_by_key(&index.0, |entry| entry.0)
        {
            Err(insert_at) if insert_at == self.constants.len() => OffsetResult::TooLarge,
            Err(insert_at) => OffsetResult::InvalidOffset(insert_at),
            Ok(found_idx) => OffsetResult::Ok(found_idx, &self.constants[found_idx].1),
        }
    }

    /// Get or insert a class constant from the constant pool
    pub fn get_class(
        &self,
        name: Utf8ConstantIndex,
    ) -> Result<ClassConstantIndex, ConstantPoolOverflow> {
        if let Some(idx) = self.classes.get(&name) {
            Ok(*idx)
        } else {
            let constant = Constant::Class(name);
            let idx = ClassConstantIndex(self.push_constant(constant)?);
            self.classes.insert(name, Box::new(idx));
            Ok(idx)
        }
    }

    /// Get or insert an integer constant from the constant pool
    pub fn get_integer(&self, integer: i32) -> Result<ConstantIndex, ConstantPoolOverflow> {
        if let Some(idx) = self.integers.get(&integer) {
            Ok(*idx)
        } else {
            let idx = self.push_constant(Constant::Integer(integer))?;
            self.integers.insert(integer, Box::new(idx));
            Ok(idx)
        }
    }

    /// Get or insert a long constant from the constant pool
    pub fn get_long(&self, long: i64) -> Result<ConstantIndex, ConstantPoolOverflow> {
        if let Some(idx) = self.longs.get(&long) {
            Ok(*idx)
        } else {
            let idx = self.push_constant(Constant::Long(long))?;
            self.longs.insert(long, Box::new(idx));
            Ok(idx)
        }
    }

    /// Get or insert a float constant from the constant pool
    pub fn get_float(&self, float: f32) -> Result<ConstantIndex, ConstantPoolOverflow> {
        let float_bytes = float.to_ne_bytes();
        if let Some(idx) = self.floats.get(&float_bytes) {
            Ok(*idx)
        } else {
            let idx = self.push_constant(Constant::Float(float))?;
            self.floats.insert(float_bytes, Box::new(idx));
            Ok(idx)
        }
    }

    /// Get or insert a double constant from the constant pool
    pub fn get_double(&self, double: f64) -> Result<ConstantIndex, ConstantPoolOverflow> {
        let double_bytes = double.to_ne_bytes();
        if let Some(idx) = self.doubles.get(&double_bytes) {
            Ok(*idx)
        } else {
            let idx = self.push_constant(Constant::Double(double))?;
            self.doubles.insert(double_bytes, Box::new(idx));
            Ok(idx)
        }
    }

    /// Get or insert a utf8 constant from the constant pool
    pub fn get_utf8<'a, S: Into<Cow<'a, str>>>(
        &self,
        utf8: S,
    ) -> Result<Utf8ConstantIndex, ConstantPoolOverflow> {
        let cow = utf8.into();

        if let Some(idx) = self.utf8s.get::<str>(cow.borrow()) {
            Ok(*idx)
        } else {
            let owned = cow.into_owned();
            let constant = Constant::Utf8(owned.clone());
            let idx = Utf8ConstantIndex(self.push_constant(constant)?);
            self.utf8s.insert(owned, Box::new(idx));
            Ok(idx)
        }
    }

    /// Get or insert a string constant from the constant pool
    pub fn get_string(
        &self,
        utf8: Utf8ConstantIndex,
    ) -> Result<StringConstantIndex, ConstantPoolOverflow> {
        if let Some(idx) = self.strings.get(&utf8) {
            Ok(*idx)
        } else {
            let constant = Constant::String(utf8);
            let idx = StringConstantIndex(self.push_constant(constant)?);
            self.strings.insert(utf8, Box::new(idx));
            Ok(idx)
        }
    }

    /// Get or insert a name & type constant from the constant pool
    pub fn get_name_and_type(
        &self,
        name: Utf8ConstantIndex,
        descriptor: Utf8ConstantIndex,
    ) -> Result<NameAndTypeConstantIndex, ConstantPoolOverflow> {
        let name_and_type_key = (name, descriptor);
        if let Some(idx) = self.name_and_types.get(&name_and_type_key) {
            Ok(*idx)
        } else {
            let constant = Constant::NameAndType { name, descriptor };
            let idx = NameAndTypeConstantIndex(self.push_constant(constant)?);
            self.name_and_types.insert(name_and_type_key, Box::new(idx));
            Ok(idx)
        }
    }

    /// Get or insert a name & type constant from the constant pool
    pub fn get_field_ref(
        &self,
        class_: ClassConstantIndex,
        name_and_type: NameAndTypeConstantIndex,
    ) -> Result<FieldRefConstantIndex, ConstantPoolOverflow> {
        let field_key = (class_, name_and_type);
        if let Some(idx) = self.fieldrefs.get(&field_key) {
            Ok(*idx)
        } else {
            let constant = Constant::FieldRef(class_, name_and_type);
            let idx = FieldRefConstantIndex(self.push_constant(constant)?);
            self.fieldrefs.insert(field_key, Box::new(idx));
            Ok(idx)
        }
    }

    /// Get or insert a method reference constant from the constant pool
    pub fn get_method_ref(
        &self,
        class: ClassConstantIndex,
        name_and_type: NameAndTypeConstantIndex,
        is_interface: bool,
    ) -> Result<MethodRefConstantIndex, ConstantPoolOverflow> {
        let method_key = (class, name_and_type, is_interface);
        if let Some(idx) = self.methodrefs.get(&method_key) {
            Ok(*idx)
        } else {
            let constant = Constant::MethodRef {
                class,
                name_and_type,
                is_interface,
            };
            let idx = MethodRefConstantIndex(self.push_constant(constant)?);
            self.methodrefs.insert(method_key, Box::new(idx));
            Ok(idx)
        }
    }

    /// Get or insert a method handle constant from the constant pool
    pub fn get_method_handle(
        &self,
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
            self.method_handles.insert(handle_key, Box::new(idx));
            Ok(idx)
        }
    }

    /// Get or insert a method type constant from the constant pool
    pub fn get_method_type(
        &self,
        descriptor: Utf8ConstantIndex,
    ) -> Result<ConstantIndex, ConstantPoolOverflow> {
        if let Some(idx) = self.method_types.get(&descriptor) {
            Ok(*idx)
        } else {
            let constant = Constant::MethodType { descriptor };
            let idx = self.push_constant(constant)?;
            self.method_types.insert(descriptor, Box::new(idx));
            Ok(idx)
        }
    }

    /// Get or insert an invoke dynamic constant from the constant pool
    pub fn get_invoke_dynamic(
        &self,
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
            self.invoke_dynamics.insert(indy_key, Box::new(idx));
            Ok(idx)
        }
    }

    /// Add an attribute to the constant pool
    pub fn get_attribute<'g, A: AttributeLike>(&self, attribute: A) -> Result<Attribute, Error> {
        let name_index = self.get_utf8(A::NAME)?;
        let mut info = vec![];

        attribute.serialize(&mut info).map_err(Error::IoError)?;

        Ok(Attribute { name_index, info })
    }
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
            // Despite the name, this is _not_ exactly UTF-8, but it is similar
            Constant::Utf8(string) => {
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
                            buffer.push(((code >> 16 & 0x0F) as u8 - 1) | 0b1010_0000);
                            buffer.push((code >> 10 & 0x3F) as u8 | 0b1000_0000);

                            buffer.push(0b1110_1101);
                            buffer.push(((code >> 6 & 0x1F) as u8 - 1) | 0b1011_0000);
                            buffer.push((code & 0x3F) as u8 | 0b1000_0000);
                        }
                    }
                }
                1u8.serialize(writer)?;
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
pub struct Utf8ConstantIndex(ConstantIndex);

#[derive(Copy, Clone, Hash, Eq, PartialEq, Debug)]
pub struct StringConstantIndex(ConstantIndex);

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
