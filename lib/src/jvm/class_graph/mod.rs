//! Capture class relationships, member signatures, etc. of known classes
//!
//! While you can always add new classes and members to the class graph, you cannot remove them.
//! The intuition there is that the class graph contains the subgraph of classes you intend to
//! interact with. As that surface area grows, more classes can be declared. Since the graph is
//! append-only, the append operations do _not_ require a mutable reference.
//!
//! ### `*Id<'g>` types
//!
//! With the exception of [`crate::jvm::class_file`] (whose concern is really more about
//! serialization to/from the class file format), Java entities are represented here with types
//! whose identity, equality, etc. are just wrapping equality of the reference to something in the
//! class graph:
//!
//!   - __Class__ is identified by a [`ClassId`] (pointing to a [`ClassData`])
//!   - __Method__ is identified by a [`MethodId`] (pointing to a [`MethodData`])
//!   - __Field__ is identified by a [`FieldId`] (pointing to a [`FieldData`])
//!   - __Bootstrap method__ is identified by a [`BootstrapMethodId`] (pointing to a
//!     [`BootstrapMethodData`])
//!
//! Since these just wrap references, you can dereference them and start crawling the class graph
//! to collect related information.
//!
//! ### Relationship to [`crate::jvm::model`]
//!
//! The class graph contains just the signature/schema of classes. Some of those classes may be
//! ones being created, others may be external. The [`crate::jvm::model`] module provides the
//! actual backing for those data structures. The class graph is the first step in code generation:
//!
//!   1. Add the class (and relevant methods or fields) to the class graph and get back an ID
//!   2. Create a [`crate::jvm::model::Class`] using the ID and fill in fields/methods
//!   3. Consume that class model into a [`crate::jvm::class_file::ClassFile`]
//!   4. Serialize the class file into bytes using [`crate::jvm::class_file::Serialize`]
//!
//! Step 3 is the moment that types switch from using the class graph over to offsets into a
//! constant pool.

use crate::jvm::code::InvokeType;
use crate::jvm::{
    BinaryName, ClassAccessFlags, FieldAccessFlags, FieldType, InnerClassAccessFlags,
    MethodAccessFlags, MethodDescriptor, Name, RefType, RenderDescriptor, UnqualifiedName,
};
use crate::util::RefId;
use elsa::map::FrozenMap;
use elsa::FrozenVec;
use std::borrow::Cow;
use std::collections::HashSet;
use std::fmt;
use std::fmt::Debug;
use typed_arena::Arena;

mod java_classes;
mod java_lib_types;
mod java_members;

pub use java_classes::*;
pub use java_lib_types::*;
pub use java_members::*;

pub struct ClassGraphArenas<'g> {
    class_arena: Arena<ClassData<'g>>,
    method_arena: Arena<MethodData<'g>>,
    field_arena: Arena<FieldData<'g>>,
    bootstrap_method_arena: Arena<BootstrapMethodData<'g>>,
}

impl<'g> ClassGraphArenas<'g> {
    pub fn new() -> Self {
        ClassGraphArenas {
            class_arena: Arena::new(),
            method_arena: Arena::new(),
            field_arena: Arena::new(),
            bootstrap_method_arena: Arena::new(),
        }
    }
}

/// Tracks the relationships between classes/interfaces and the members on those classes
///
/// Whenever you intend to interact/create a certain set of classes/members, the recommended
/// approach is to register those onto the class graph as early as possible. This makes it possible
/// to use references to the same classes/members throughout code generation, making it easier to
/// have a single consistent view of what exists.
pub struct ClassGraph<'g> {
    arenas: &'g ClassGraphArenas<'g>,
    classes: FrozenMap<&'g BinaryName, ClassId<'g>>,
}

impl<'g> ClassGraph<'g> {
    /// New empty graph
    pub fn new(arenas: &'g ClassGraphArenas<'g>) -> Self {
        ClassGraph {
            arenas,
            classes: FrozenMap::new(),
        }
    }

    /// Query if one type is assignable to another
    ///
    /// This matches the semantics of the prolog predicate `isJavaAssignable(sub_type, super_type)`
    /// in the JVM verifier specification.
    pub fn is_java_assignable(
        sub_type: &RefType<ClassId<'g>>,
        super_type: &RefType<ClassId<'g>>,
    ) -> bool {
        match (sub_type, super_type) {
            // Special superclass and interfaces of all arrays
            (
                RefType::PrimitiveArray(_) | RefType::ObjectArray(_),
                RefType::Object(object_type),
            ) => Self::is_array_type_assignable(&object_type.name),

            // Primitive arrays must match in dimension and type
            (RefType::PrimitiveArray(arr1), RefType::PrimitiveArray(arr2)) => arr1 == arr2,

            // Cursed (unsound) covariance of arrays
            (RefType::ObjectArray(arr1), RefType::ObjectArray(arr2)) => {
                if arr1.additional_dimensions < arr2.additional_dimensions {
                    false
                } else if arr1.additional_dimensions == arr2.additional_dimensions {
                    Self::is_object_type_assignable(arr1.element_type, arr2.element_type)
                } else {
                    Self::is_array_type_assignable(&arr2.element_type.name)
                }
            }

            // Object-to-object assignability holds if there is a path through super type edges
            (RefType::Object(elem_type1), RefType::Object(elem_type2)) => {
                Self::is_object_type_assignable(*elem_type1, *elem_type2)
            }

            _ => false,
        }
    }

    /// Object to object assignability
    ///
    /// This does a search up the superclasses and superinterfaces looking for the super type.
    fn is_object_type_assignable(sub_type: ClassId<'g>, super_type: ClassId<'g>) -> bool {
        let mut supertypes_to_visit: Vec<ClassId<'g>> = vec![super_type];
        let mut dont_revisit: HashSet<ClassId<'g>> = HashSet::new();
        dont_revisit.insert(sub_type);

        // Optimization: if the super type is a class, then skip visiting interfaces
        let super_is_class: bool = !super_type.is_interface();

        while let Some(class_data) = supertypes_to_visit.pop() {
            if class_data == super_type {
                return true;
            }
            let class_data = class_data.0;

            // Enqueue next types to visit
            if let Some(superclass) = class_data.superclass {
                if dont_revisit.insert(superclass) {
                    supertypes_to_visit.push(superclass);
                }
            }
            if !super_is_class {
                for interface in &class_data.interfaces {
                    let interface = RefId(interface);
                    if dont_revisit.insert(interface) {
                        supertypes_to_visit.push(interface);
                    }
                }
            }
        }

        false
    }

    /// Check if arrays can be assigned to a super type
    ///
    /// This bakes in knowledge of the small, finite set of super types arrays have.
    fn is_array_type_assignable(super_type: &BinaryName) -> bool {
        super_type == &BinaryName::OBJECT
            || super_type == &BinaryName::CLONEABLE
            || super_type == &BinaryName::SERIALIZABLE
    }

    /// Is this object type throwable?
    pub fn is_throwable(class: ClassId<'g>) -> bool {
        let mut next_class = Some(class);
        while let Some(class) = next_class {
            if class.name == BinaryName::THROWABLE {
                return true;
            }
            next_class = class.superclass;
        }

        false
    }

    // TODO: remove uses of this
    pub fn lookup_class(&'g self, name: &BinaryName) -> Option<ClassId<'g>> {
        self.classes.get(name).map(|cid| RefId(cid))
    }

    /// Add a new class to the class graph
    ///
    /// TODO: add validation here (eg. not extending final class, etc.)
    pub fn add_class(&self, data: ClassData<'g>) -> ClassId<'g> {
        let data: &'g ClassData<'g> = self.arenas.class_arena.alloc(data);
        let class_id: ClassId<'g> = RefId(data);
        self.classes.insert(&data.name, class_id);

        // Register inner classes with their nest host
        if let NestData::Member { .. } = data.nest {
            let nest_host = class_id.nest_host();
            if let NestData::Host { members } = &nest_host.nest {
                members.push(class_id)
            } else {
                unreachable!(
                    "The nest host of {:?} (computed to be {:?}) thinks it is a nest member",
                    data, nest_host
                )
            }
        }

        class_id
    }

    /// Add a field to the class graph and to its class
    ///
    /// TODO: validate that the class doesn't have any conflicting fields
    pub fn add_field(&self, field: FieldData<'g>) -> FieldId<'g> {
        let data = RefId(&*self.arenas.field_arena.alloc(field));
        data.class.fields.push(data);
        data
    }

    /// Add a method to the class graph and to its class
    ///
    /// TODO: validate that the class doesn't have any conflicting methods
    pub fn add_method(&self, method: MethodData<'g>) -> MethodId<'g> {
        if let Some(m) = method.class.0.methods.iter().find(|m| {
            m.name == method.name
                && m.descriptor == method.descriptor
                && m.is_static() == method.is_static()
        }) {
            RefId(m)
        } else {
            let data = RefId(&*self.arenas.method_arena.alloc(method));
            data.class.methods.push(data);
            data
        }
    }

    pub fn add_bootstrap_method(
        &self,
        bootstrap_method: BootstrapMethodData<'g>,
    ) -> BootstrapMethodId<'g> {
        RefId(self.arenas.bootstrap_method_arena.alloc(bootstrap_method))
    }

    /// Add standard types to the class graph
    pub fn insert_java_library_types(&self) -> java_lib_types::JavaLibrary<'g> {
        java_lib_types::JavaLibrary::add_to_graph(&self)
    }
}

/// Reference to a class in the class graph
pub type ClassId<'g> = RefId<'g, ClassData<'g>>;

/// Reference to a method in the class graph
pub type MethodId<'g> = RefId<'g, MethodData<'g>>;

/// Reference to a field in the class graph
pub type FieldId<'g> = RefId<'g, FieldData<'g>>;

/// Reference to a bootstrap method in the class graph
pub type BootstrapMethodId<'g> = RefId<'g, BootstrapMethodData<'g>>;

pub struct ClassData<'g> {
    /// Name of the class
    pub name: BinaryName,

    /// Superclass is only ever missing for `java/lang/Object` itself
    pub superclass: Option<ClassId<'g>>,

    /// Interfaces implemented (or super-interfaces)
    pub interfaces: FrozenVec<ClassId<'g>>,

    /// Class access flags
    pub access_flags: ClassAccessFlags,

    /// Methods
    pub methods: FrozenVec<MethodId<'g>>,

    /// Fields
    pub fields: FrozenVec<FieldId<'g>>,

    /// Nesting information
    pub nest: NestData<'g>,
}

/// Nesting information for the class.
///
/// Every class must either be a host or be nested inside a host.
pub enum NestData<'g> {
    Host {
        /// All members claiming membership in this nest.
        ///
        /// This includes all transitive inner classes.
        members: FrozenVec<ClassId<'g>>,
    },
    Member(NestedClassData<'g>),
}

/// Information tracked for classes nested inside other classes
pub struct NestedClassData<'g> {
    /// Inner class access flags with respect to the immediately enclosing class.
    pub access_flags: InnerClassAccessFlags,

    /// Simple name
    pub simple_name: Option<UnqualifiedName>,

    /// Immediately enclosing class.
    ///
    /// This is _not_ the nest host, though following the `enclosing_class` chain should
    /// eventually lead to the nest host.
    pub enclosing_class: ClassId<'g>,
}

impl<'g> RenderDescriptor for ClassData<'g> {
    fn render_to(&self, write_to: &mut String) {
        self.name.render_to(write_to)
    }
}

impl<'a, 'g> RenderDescriptor for &'a ClassData<'g> {
    fn render_to(&self, write_to: &mut String) {
        self.name.render_to(write_to)
    }
}

impl<'g> Debug for ClassData<'g> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name.as_str())
    }
}

#[derive(PartialEq, Eq)]
pub struct MethodData<'g> {
    /// Class
    pub class: ClassId<'g>,

    /// Name of the method
    pub name: UnqualifiedName,

    /// Method access flags
    pub access_flags: MethodAccessFlags,

    /// Type of the method
    pub descriptor: MethodDescriptor<ClassId<'g>>,
}

impl<'g> Debug for MethodData<'g> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!(
            "{}.{}:{}",
            self.class.name.as_str(),
            self.name.as_str(),
            self.descriptor.render(),
        ))
    }
}

impl<'g> MethodData<'g> {
    /// Infer the way to invoke a method
    ///
    /// There is one situation where this is not unambiguous: virtual methods may be called with
    /// either of `invokespecial` or `invokevirtual` (to represent static dispatch vs dynamic
    /// dispatch). This function chooses `invokevirtual`.
    pub fn infer_invoke_type(&self) -> InvokeType {
        if self.is_static() {
            InvokeType::Static
        } else if self.name == UnqualifiedName::INIT || self.name == UnqualifiedName::CLINIT {
            InvokeType::Special
        } else if self.class.is_interface() {
            let n = self.descriptor.parameter_length(true) as u8;
            InvokeType::Interface(n)
        } else {
            InvokeType::Virtual
        }
    }

    /// Is this a static method?
    pub fn is_static(&self) -> bool {
        self.access_flags.contains(MethodAccessFlags::STATIC)
    }
}

#[derive(PartialEq, Eq)]
pub struct FieldData<'g> {
    /// Class
    ///
    /// Note: this is a pointer back to the class (so don't derive `Debug`)
    pub class: ClassId<'g>,

    /// Name of the field
    pub name: UnqualifiedName,

    /// Field access flags
    pub access_flags: FieldAccessFlags,

    /// Type of the field
    pub descriptor: FieldType<ClassId<'g>>,
}

impl<'g> FieldData<'g> {
    /// Is this a static field?
    pub fn is_static(&self) -> bool {
        self.access_flags.contains(FieldAccessFlags::STATIC)
    }
}

impl<'g> Debug for FieldData<'g> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!(
            "{}.{}:{}",
            self.class.name.as_str(),
            self.name.as_str(),
            self.descriptor.render(),
        ))
    }
}

impl<'g> ClassData<'g> {
    pub fn new(
        name: BinaryName,
        superclass: ClassId<'g>,
        access_flags: ClassAccessFlags,
        outer_class: Option<NestedClassData<'g>>,
    ) -> ClassData<'g> {
        let nest = match outer_class {
            None => NestData::Host {
                members: FrozenVec::new(),
            },
            Some(data) => NestData::Member(data),
        };
        ClassData {
            name,
            superclass: Some(superclass),
            interfaces: FrozenVec::new(),
            access_flags,
            methods: FrozenVec::new(),
            fields: FrozenVec::new(),
            nest,
        }
    }

    /// Is this an interface?
    pub fn is_interface(&self) -> bool {
        self.access_flags.contains(ClassAccessFlags::INTERFACE)
    }
}

impl<'g> ClassId<'g> {
    /// Find the nest host of a class
    pub fn nest_host(&self) -> ClassId<'g> {
        let mut host_candidate = *self;
        loop {
            match &host_candidate.nest {
                NestData::Host { .. } => return host_candidate,
                NestData::Member(data) => host_candidate = data.enclosing_class,
            }
        }
    }
}

#[derive(Clone)]
pub struct InvokeDynamicData<'g> {
    /// Name of the dynamically invoked method
    pub name: UnqualifiedName,

    /// Type of the dynamically invoked method
    pub descriptor: MethodDescriptor<ClassId<'g>>,

    /// Bootstrap method
    pub bootstrap: BootstrapMethodId<'g>,
}

// TODO: show bootstrap
impl<'g> Debug for InvokeDynamicData<'g> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!(
            "[{:?}]{}:{}",
            self.bootstrap,
            self.name.as_str(),
            self.descriptor.render(),
        ))
    }
}

#[derive(PartialEq, Eq, Clone)]
pub struct BootstrapMethodData<'g> {
    /// Bootstrap method
    pub method: MethodId<'g>,

    /// Boostrap arguments
    pub arguments: Vec<ConstantData<'g>>,
}

impl<'g> Debug for BootstrapMethodData<'g> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut tuple = f.debug_tuple(&format!(
            "{}.{}",
            self.method.class.name.as_str(),
            self.method.name.as_str(),
        ));
        for argument in &self.arguments {
            tuple.field(argument);
        }
        tuple.finish()
    }
}

#[derive(Hash, Eq, PartialEq, Clone)]
pub enum ConstantData<'g> {
    /// String constant of type `java.lang.String`
    String(Cow<'static, str>),

    /// Class constant of type `java.lang.Class`
    Class(RefType<ClassId<'g>>),

    /// Integer constant of type `int`
    Integer(i32),

    /// Long constant of type `long`
    Long(i64),

    /// Float constant of type `float`, represented in little-endian bytes
    Float([u8; 4]),

    /// Double constant of type `double`, represented in little-endian bytes
    Double([u8; 8]),

    /// Field-backed method handle constant of type `java.lang.invoke.MethodHandle`
    ///
    /// Whether this is a getter or setter field is determined by the access mode and whether this
    /// is a static or non-static handle is determined by whether the field is static or not.
    FieldHandle(AccessMode, FieldId<'g>),

    /// Method-backed method handle constant of type `java.lang.invoke.MethodHandle`
    ///
    /// Whether this is a virtual, static, or interface handle is determined using the same method
    /// as [`MethodData::infer_invoke_type`].
    MethodHandle(MethodId<'g>),

    /// Method type of type `java.lang.invoke.MethodType`
    MethodType(MethodDescriptor<ClassId<'g>>),
}

impl<'g> ConstantData<'g> {
    pub fn float(value: f32) -> ConstantData<'g> {
        ConstantData::Float(f32::to_le_bytes(value))
    }

    pub fn double(value: f64) -> ConstantData<'g> {
        ConstantData::Double(f64::to_le_bytes(value))
    }
}

// TODO: revisit this?
impl<'g> Debug for ConstantData<'g> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConstantData::String(string) => string.fmt(f),
            ConstantData::Class(ref_type) => ref_type.fmt(f),
            ConstantData::Integer(integer) => integer.fmt(f),
            ConstantData::Long(long) => long.fmt(f),
            ConstantData::Float(float) => f32::from_le_bytes(*float).fmt(f),
            ConstantData::Double(double) => f64::from_le_bytes(*double).fmt(f),
            ConstantData::FieldHandle(_access_mode, field) => field.fmt(f),
            ConstantData::MethodHandle(method) => method.fmt(f),
            ConstantData::MethodType(method_type) => method_type.fmt(f),
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum AccessMode {
    Read,
    Write,
}
