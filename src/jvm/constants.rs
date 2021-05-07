use super::{
    Attribute, AttributeLike, ConstantsReader, Descriptor, Error, FieldType, MethodDescriptor,
    Offset, OffsetResult, OffsetVec, RefType, Serialize, VerifierErrorKind, Width,
};
use byteorder::WriteBytesExt;
use std::borrow::{Borrow, Cow};
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::result::Result;

// Note: elements can be easily added to the pool, but not so easily removed
pub struct ConstantsPool {
    constants: OffsetVec<Constant>,

    classes: HashMap<Utf8ConstantIndex, ClassConstantIndex>,
    fieldrefs: HashMap<(ClassConstantIndex, NameAndTypeConstantIndex), FieldRefConstantIndex>,
    methodrefs:
        HashMap<(ClassConstantIndex, NameAndTypeConstantIndex, bool), MethodRefConstantIndex>,
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

impl ConstantsPool {
    /// Make a fresh empty constants pool
    pub fn new() -> ConstantsPool {
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

    /// Push a constant into the constant pool, provided there is space for it
    ///
    /// Note: the largest valid index is 65536, indexing starts at 1, and some constants take two
    /// spaces.
    fn push_constant(
        constants: &mut OffsetVec<Constant>,
        constant: Constant,
    ) -> Result<ConstantIndex, Error> {
        let Offset(offset) = constants.offset_len();

        if offset + constant.width() <= u16::MAX.into() {
            let _ = constants.push(constant);
            Ok(ConstantIndex(offset as u16))
        } else {
            Err(Error::ConstantPoolOverflow { constant, offset })
        }
    }

    /// Consume the pool and return the final vector of constants
    pub fn into_offset_vec(self) -> OffsetVec<Constant> {
        self.constants
    }

    /// Get a constant from the pool
    pub fn get(&self, index: ConstantIndex) -> OffsetResult<Constant> {
        self.constants.get_offset(Offset(index.0 as usize))
    }

    /// Get or insert a class constant from the constant pool
    pub fn get_class(&mut self, name: Utf8ConstantIndex) -> Result<ClassConstantIndex, Error> {
        match self.classes.entry(name) {
            Entry::Occupied(occupied) => Ok(*occupied.get()),
            Entry::Vacant(vacant) => {
                let constant = Constant::Class(name);
                let idx = ClassConstantIndex(Self::push_constant(&mut self.constants, constant)?);
                let _ = vacant.insert(idx);
                Ok(idx)
            }
        }
    }

    /// Get or insert an integer constant from the constant pool
    pub fn get_integer(&mut self, integer: i32) -> Result<ConstantIndex, Error> {
        match self.integers.entry(integer) {
            Entry::Occupied(occupied) => Ok(*occupied.get()),
            Entry::Vacant(vacant) => {
                let idx = Self::push_constant(&mut self.constants, Constant::Integer(integer))?;
                let _ = vacant.insert(idx);
                Ok(idx)
            }
        }
    }

    /// Get or insert a long constant from the constant pool
    pub fn get_long(&mut self, long: i64) -> Result<ConstantIndex, Error> {
        match self.longs.entry(long) {
            Entry::Occupied(occupied) => Ok(*occupied.get()),
            Entry::Vacant(vacant) => {
                let idx = Self::push_constant(&mut self.constants, Constant::Long(long))?;
                let _ = vacant.insert(idx);
                Ok(idx)
            }
        }
    }

    /// Get or insert a float constant from the constant pool
    pub fn get_float(&mut self, float: f32) -> Result<ConstantIndex, Error> {
        match self.floats.entry(float.to_ne_bytes()) {
            Entry::Occupied(occupied) => Ok(*occupied.get()),
            Entry::Vacant(vacant) => {
                let idx = Self::push_constant(&mut self.constants, Constant::Float(float))?;
                let _ = vacant.insert(idx);
                Ok(idx)
            }
        }
    }

    /// Get or insert a double constant from the constant pool
    pub fn get_double(&mut self, double: f64) -> Result<ConstantIndex, Error> {
        match self.doubles.entry(double.to_ne_bytes()) {
            Entry::Occupied(occupied) => Ok(*occupied.get()),
            Entry::Vacant(vacant) => {
                let idx = Self::push_constant(&mut self.constants, Constant::Double(double))?;
                let _ = vacant.insert(idx);
                Ok(idx)
            }
        }
    }

    /// Get or insert a utf8 constant from the constant pool
    pub fn get_utf8<'a, S: Into<Cow<'a, str>>>(
        &mut self,
        utf8: S,
    ) -> Result<Utf8ConstantIndex, Error> {
        let cow = utf8.into();

        // We don't use `entry` here. That means two lookups into the map (one `get` then one
        // `insert`) but it avoids allocating a `String` unless we are actually going to need it.
        if let Some(idx) = self.utf8s.get::<str>(cow.borrow()) {
            Ok(*idx)
        } else {
            let owned = cow.into_owned();
            let constant = Constant::Utf8(owned.clone());
            let idx = Utf8ConstantIndex(Self::push_constant(&mut self.constants, constant)?);
            self.utf8s.insert(owned, idx);
            Ok(idx)
        }
    }

    /// Get or insert a string constant from the constant pool
    pub fn get_string(&mut self, utf8: Utf8ConstantIndex) -> Result<StringConstantIndex, Error> {
        match self.strings.entry(utf8) {
            Entry::Occupied(occupied) => Ok(*occupied.get()),
            Entry::Vacant(vacant) => {
                let constant = Constant::String(utf8);
                let idx = StringConstantIndex(Self::push_constant(&mut self.constants, constant)?);
                let _ = vacant.insert(idx);
                Ok(idx)
            }
        }
    }

    /// Get or insert a name & type constant from the constant pool
    pub fn get_name_and_type(
        &mut self,
        name: Utf8ConstantIndex,
        descriptor: Utf8ConstantIndex,
    ) -> Result<NameAndTypeConstantIndex, Error> {
        match self.name_and_types.entry((name, descriptor)) {
            Entry::Occupied(occupied) => Ok(*occupied.get()),
            Entry::Vacant(vacant) => {
                let constant = Constant::NameAndType { name, descriptor };
                let idx =
                    NameAndTypeConstantIndex(Self::push_constant(&mut self.constants, constant)?);
                let _ = vacant.insert(idx);
                Ok(idx)
            }
        }
    }

    /// Get or insert a name & type constant from the constant pool
    pub fn get_field_ref(
        &mut self,
        class_: ClassConstantIndex,
        name_and_type: NameAndTypeConstantIndex,
    ) -> Result<FieldRefConstantIndex, Error> {
        match self.fieldrefs.entry((class_, name_and_type)) {
            Entry::Occupied(occupied) => Ok(*occupied.get()),
            Entry::Vacant(vacant) => {
                let constant = Constant::FieldRef(class_, name_and_type);
                let idx =
                    FieldRefConstantIndex(Self::push_constant(&mut self.constants, constant)?);
                let _ = vacant.insert(idx);
                Ok(idx)
            }
        }
    }

    /// Get or insert a method reference constant from the constant pool
    pub fn get_method_ref(
        &mut self,
        class: ClassConstantIndex,
        name_and_type: NameAndTypeConstantIndex,
        is_interface: bool,
    ) -> Result<MethodRefConstantIndex, Error> {
        match self.methodrefs.entry((class, name_and_type, is_interface)) {
            Entry::Occupied(occupied) => Ok(*occupied.get()),
            Entry::Vacant(vacant) => {
                let constant = Constant::MethodRef {
                    class,
                    name_and_type,
                    is_interface,
                };
                let idx =
                    MethodRefConstantIndex(Self::push_constant(&mut self.constants, constant)?);
                let _ = vacant.insert(idx);
                Ok(idx)
            }
        }
    }

    /// Get or insert a method handle constant from the constant pool
    pub fn get_method_handle(
        &mut self,
        handle_kind: HandleKind,
        member: ConstantIndex,
    ) -> Result<ConstantIndex, Error> {
        match self.method_handles.entry((handle_kind, member)) {
            Entry::Occupied(occupied) => Ok(*occupied.get()),
            Entry::Vacant(vacant) => {
                let constant = Constant::MethodHandle {
                    handle_kind,
                    member,
                };
                let idx = Self::push_constant(&mut self.constants, constant)?;
                let _ = vacant.insert(idx);
                Ok(idx)
            }
        }
    }

    /// Get or insert a method type constant from the constant pool
    pub fn get_method_type(
        &mut self,
        descriptor: Utf8ConstantIndex,
    ) -> Result<ConstantIndex, Error> {
        match self.method_types.entry(descriptor) {
            Entry::Occupied(occupied) => Ok(*occupied.get()),
            Entry::Vacant(vacant) => {
                let constant = Constant::MethodType { descriptor };
                let idx = Self::push_constant(&mut self.constants, constant)?;
                let _ = vacant.insert(idx);
                Ok(idx)
            }
        }
    }

    /// Get or insert an invoke dynamic constant from the constant pool
    pub fn get_invoke_dynamic(
        &mut self,
        bootstrap_method: u16,
        method_descriptor: NameAndTypeConstantIndex,
    ) -> Result<InvokeDynamicConstantIndex, Error> {
        match self
            .invoke_dynamics
            .entry((bootstrap_method, method_descriptor))
        {
            Entry::Occupied(occupied) => Ok(*occupied.get()),
            Entry::Vacant(vacant) => {
                let constant = Constant::InvokeDynamic {
                    bootstrap_method,
                    method_descriptor,
                };
                let idx =
                    InvokeDynamicConstantIndex(Self::push_constant(&mut self.constants, constant)?);
                let _ = vacant.insert(idx);
                Ok(idx)
            }
        }
    }

    /// Add an attribute to the constant pool
    pub fn get_attribute<A: AttributeLike>(&mut self, attribute: A) -> Result<Attribute, Error> {
        let name_index = self.get_utf8(A::NAME)?;
        let mut info = vec![];

        attribute.serialize(&mut info).map_err(Error::IoError)?;

        Ok(Attribute { name_index, info })
    }

    fn lookup_utf8(&self, utf8_index: Utf8ConstantIndex) -> Result<&str, VerifierErrorKind> {
        match self.get(utf8_index.into()).ok() {
            Some(Constant::Utf8(desc)) => Ok(&desc),
            Some(other) => Err(VerifierErrorKind::NotLoadableConstant(other.clone())),
            None => Err(VerifierErrorKind::MissingConstant(utf8_index.into())),
        }
    }
}

impl Serialize for ConstantsPool {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> std::io::Result<()> {
        self.constants.serialize(writer)
    }
}

impl ConstantsReader for ConstantsPool {
    fn lookup_constant_type(&self, index: ConstantIndex) -> Result<FieldType, VerifierErrorKind> {
        match self.get(index).ok() {
            Some(Constant::Class(_)) => Ok(FieldType::Ref(RefType::CLASS_CLASS)),
            Some(Constant::String(_)) => Ok(FieldType::Ref(RefType::STRING_CLASS)),
            Some(Constant::Integer(_)) => Ok(FieldType::INT),
            Some(Constant::Float(_)) => Ok(FieldType::FLOAT),
            Some(Constant::Long(_)) => Ok(FieldType::LONG),
            Some(Constant::Double(_)) => Ok(FieldType::DOUBLE),
            Some(Constant::MethodHandle { .. }) => Ok(FieldType::Ref(RefType::METHOD_HANDLE_CLASS)),
            Some(Constant::MethodType { .. }) => Ok(FieldType::Ref(RefType::METHOD_TYPE_CLASS)),
            Some(other) => Err(VerifierErrorKind::NotLoadableConstant(other.clone())),
            None => Err(VerifierErrorKind::MissingConstant(index)),
        }
    }

    fn lookup_class_reftype(
        &self,
        cls_index: ClassConstantIndex,
    ) -> Result<RefType, VerifierErrorKind> {
        let utf8_index = match self.get(cls_index.into()).ok() {
            Some(Constant::Class(utf8_index)) => *utf8_index,
            Some(other) => return Err(VerifierErrorKind::NotLoadableConstant(other.clone())),
            None => return Err(VerifierErrorKind::MissingConstant(cls_index.into())),
        };

        let desc = self.lookup_utf8(utf8_index)?;

        /* TODO: I don't understand how sometimes we use the regular `pkg/Cls` syntax (and not
         *       `Lpkg/Cls;` but then it is also OK to have `[Lpkg/Cls;`. Motivating example:
         *
         *          (Foo)my_object         =>     checkcast #7      // class pkg/Foo
         *          (Foo[])my_object       =>     checkcast #8      // class "[Lpkg/Foo;"
         */
        let ref_type = if let Some('[') = desc.chars().next() {
            RefType::parse(&desc).map_err(|_| VerifierErrorKind::BadDescriptor(desc.to_string()))?
        } else {
            RefType::object(String::from(desc))
        };

        Ok(ref_type)
    }

    fn lookup_field(
        &self,
        field_index: FieldRefConstantIndex,
    ) -> Result<(RefType, FieldType), VerifierErrorKind> {
        let (class_index, name_and_type_index) = match self.get(field_index.into()).ok() {
            Some(Constant::FieldRef(class_, name_and_type)) => (*class_, *name_and_type),
            Some(other) => return Err(VerifierErrorKind::NotLoadableConstant(other.clone())),
            None => return Err(VerifierErrorKind::MissingConstant(field_index.into())),
        };

        let utf8_index = match self.get(name_and_type_index.into()).ok() {
            Some(Constant::NameAndType { descriptor, .. }) => *descriptor,
            Some(other) => return Err(VerifierErrorKind::NotLoadableConstant(other.clone())),
            None => {
                return Err(VerifierErrorKind::MissingConstant(
                    name_and_type_index.into(),
                ))
            }
        };

        let desc = self.lookup_utf8(utf8_index)?;

        let class_reftype = self.lookup_class_reftype(class_index)?;
        let field_type = FieldType::parse(&desc)
            .map_err(|_| VerifierErrorKind::BadDescriptor(desc.to_string()))?;

        Ok((class_reftype, field_type))
    }

    fn lookup_method(
        &self,
        method_index: MethodRefConstantIndex,
    ) -> Result<(ClassConstantIndex, bool, bool, MethodDescriptor), VerifierErrorKind> {
        let (class, name_and_type, is_interface) = match self.get(method_index.into()).ok() {
            Some(Constant::MethodRef {
                class,
                name_and_type,
                is_interface,
            }) => (*class, *name_and_type, *is_interface),
            Some(other) => return Err(VerifierErrorKind::NotLoadableConstant(other.clone())),
            None => return Err(VerifierErrorKind::MissingConstant(method_index.into())),
        };

        let (name_utf8, descriptor_utf8) = match self.get(name_and_type.into()).ok() {
            Some(Constant::NameAndType { name, descriptor }) => (*name, *descriptor),
            Some(other) => return Err(VerifierErrorKind::NotLoadableConstant(other.clone())),
            None => return Err(VerifierErrorKind::MissingConstant(name_and_type.into())),
        };

        let desc = self.lookup_utf8(descriptor_utf8)?;
        let is_init = self.lookup_utf8(name_utf8)? == "<init>";

        let method_descriptor = MethodDescriptor::parse(&desc)
            .map_err(|_| VerifierErrorKind::BadDescriptor(desc.to_string()))?;

        Ok((class, is_interface, is_init, method_descriptor))
    }

    fn lookup_invoke_dynamic(
        &self,
        invoke_dynamic: InvokeDynamicConstantIndex,
    ) -> Result<MethodDescriptor, VerifierErrorKind> {
        let name_and_type = match self.get(invoke_dynamic.into()).ok() {
            Some(Constant::InvokeDynamic {
                method_descriptor, ..
            }) => *method_descriptor,
            Some(other) => return Err(VerifierErrorKind::NotLoadableConstant(other.clone())),
            None => return Err(VerifierErrorKind::MissingConstant(invoke_dynamic.into())),
        };

        let descriptor_utf8 = match self.get(name_and_type.into()).ok() {
            Some(Constant::NameAndType { descriptor, .. }) => *descriptor,
            Some(other) => return Err(VerifierErrorKind::NotLoadableConstant(other.clone())),
            None => return Err(VerifierErrorKind::MissingConstant(name_and_type.into())),
        };

        let desc = self.lookup_utf8(descriptor_utf8)?;

        let method_descriptor = MethodDescriptor::parse(&desc)
            .map_err(|_| VerifierErrorKind::BadDescriptor(desc.to_string()))?;

        Ok(method_descriptor)
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
