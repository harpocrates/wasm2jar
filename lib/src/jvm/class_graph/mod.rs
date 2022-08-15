use super::{
    BinaryName, ClassAccessFlags, FieldAccessFlags, FieldType, InvokeType, MethodAccessFlags,
    MethodDescriptor, Name, RefType, RenderDescriptor, UnqualifiedName, InnerClassAccessFlags,
};
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
    classes: FrozenMap<&'g BinaryName, &'g ClassData<'g>>,
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
        sub_type: &RefType<&'g ClassData<'g>>,
        super_type: &RefType<&'g ClassData<'g>>,
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
                    Self::is_object_type_assignable(&arr1.element_type, &arr2.element_type)
                } else {
                    Self::is_array_type_assignable(&arr2.element_type.name)
                }
            }

            // Object-to-object assignability holds if there is a path through super type edges
            (RefType::Object(elem_type1), RefType::Object(elem_type2)) => {
                Self::is_object_type_assignable(elem_type1, elem_type2)
            }

            _ => false,
        }
    }

    /// Object to object assignability
    ///
    /// This does a search up the superclasses and superinterfaces looking for the super type.
    fn is_object_type_assignable(sub_type: &ClassData<'g>, super_type: &ClassData<'g>) -> bool {
        let mut supertypes_to_visit: Vec<&ClassData<'g>> = vec![super_type];
        let mut dont_revisit: HashSet<&BinaryName> = HashSet::new();
        dont_revisit.insert(&sub_type.name);

        // Optimization: if the super type is a class, then skip visiting interfaces
        let super_is_class: bool = !super_type.is_interface();

        while let Some(class_data) = supertypes_to_visit.pop() {
            if class_data.name == super_type.name {
                return true;
            }

            // Enqueue next types to visit
            if let Some(superclass) = &class_data.superclass {
                if dont_revisit.insert(&superclass.name) {
                    supertypes_to_visit.push(&superclass);
                }
            }
            if !super_is_class {
                for interface in &class_data.interfaces {
                    if dont_revisit.insert(&interface.name) {
                        supertypes_to_visit.push(&interface);
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
    pub fn is_throwable(class: &ClassData<'g>) -> bool {
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
    pub fn lookup_class(&'g self, name: &BinaryName) -> Option<&'g ClassData<'g>> {
        self.classes.get(name)
    }

    /// Add a new class to the class graph
    ///
    /// TODO: add validation here (eg. not extending final class, etc.)
    pub fn add_class(&self, data: ClassData<'g>) -> &'g ClassData<'g> {
        let data = &*self.arenas.class_arena.alloc(data);
        self.classes.insert(&data.name, data);

        // Register inner classes with their nest host
        if let NestData::Member { .. } = data.nest {
            let nest_host = data.nest_host();
            if let NestData::Host { members } = &nest_host.nest {
                members.push(data)
            } else {
                unreachable!("The nest host of {:?} (computed to be {:?}) thinks it is a nest member", data, nest_host)
            }
        }

        data
    }

    /// Add a field to the class graph and to its class
    ///
    /// TODO: validate that the class doesn't have any conflicting fields
    pub fn add_field(&self, field: FieldData<'g>) -> &'g FieldData<'g> {
        let data = &*self.arenas.field_arena.alloc(field);
        data.class.fields.push(data);
        data
    }

    /// Add a method to the class graph and to its class
    ///
    /// TODO: validate that the class doesn't have any conflicting methods
    pub fn add_method(&self, method: MethodData<'g>) -> &'g MethodData<'g> {
        if let Some(m) = method.class.methods.iter().find(|m| {
            m.name == method.name
                && m.descriptor == method.descriptor
                && m.is_static() == method.is_static()
        }) {
            m
        } else {
            let data = &*self.arenas.method_arena.alloc(method);
            data.class.methods.push(data);
            data
        }
    }

    pub fn add_bootstrap_method(
        &self,
        bootstrap_method: BootstrapMethodData<'g>,
    ) -> &'g BootstrapMethodData<'g> {
        self.arenas.bootstrap_method_arena.alloc(bootstrap_method)
    }

    /// Add standard types to the class graph
    pub fn insert_java_library_types(&self) -> java_lib_types::JavaLibrary<'g> {
        java_lib_types::JavaLibrary::add_to_graph(&self)
    }
}

pub struct ClassData<'g> {
    /// Name of the class
    pub name: BinaryName,

    /// Superclass is only ever missing for `java/lang/Object` itself
    pub superclass: Option<&'g ClassData<'g>>,

    /// Interfaces implemented (or super-interfaces)
    pub interfaces: FrozenVec<&'g ClassData<'g>>,

    /// Class access flags
    pub access_flags: ClassAccessFlags,

    /// Methods
    pub methods: FrozenVec<&'g MethodData<'g>>,

    /// Fields
    pub fields: FrozenVec<&'g FieldData<'g>>,

    /// Nesting information
    pub nest: NestData<'g>
}

/// Nesting information for the class.
///
/// Every class must either be a host or be nested inside a host.
pub enum NestData<'g> {
    Host {
        /// All members claiming membership in this nest.
        ///
        /// This includes all transitive inner classes.
        members: FrozenVec<&'g ClassData<'g>>,
    },
    Member {
        /// Inner class access flags with respect to the immediately enclosing class.
        access_flags: InnerClassAccessFlags,

        /// Immediately enclosing class.
        ///
        /// This is _not_ the nest host, though following the `enclosing_class` chain should
        /// eventually lead to the nest host.
        enclosing_class: &'g ClassData<'g>,
    }
}

impl<'g> PartialEq for ClassData<'g> {
    fn eq(&self, other: &ClassData<'g>) -> bool {
        self.name == other.name
    }
}

impl<'g> Eq for ClassData<'g> {}

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
    pub class: &'g ClassData<'g>,

    /// Name of the method
    pub name: UnqualifiedName,

    /// Method access flags
    pub access_flags: MethodAccessFlags,

    /// Type of the method
    pub descriptor: MethodDescriptor<&'g ClassData<'g>>,
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
    /// With the exception of `invokespecial` vs. `invokevirtual`, there is usually only one valid
    /// way to invoke a method. This function finds it.
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
    pub class: &'g ClassData<'g>,

    /// Name of the field
    pub name: UnqualifiedName,

    /// Field access flags
    pub access_flags: FieldAccessFlags,

    /// Type of the field
    pub descriptor: FieldType<&'g ClassData<'g>>,
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
        superclass: &'g ClassData<'g>,
        access_flags: ClassAccessFlags,
        outer_class: Option<(InnerClassAccessFlags, &'g ClassData<'g>)>,
    ) -> ClassData<'g> {
        let nest = match outer_class {
            None => NestData::Host { members: FrozenVec::new() },
            Some((access_flags, enclosing_class)) => NestData::Member { access_flags, enclosing_class },
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

    /// Find the nest host of a class
    pub fn nest_host(&'g self) -> &'g ClassData<'g> {
        let mut host_candidate = self;
        loop {
            match host_candidate.nest {
                NestData::Host { .. } => return host_candidate,
                NestData::Member { enclosing_class, .. } => host_candidate = enclosing_class,
            }
        }
    }
}

#[derive(Clone)]
pub struct InvokeDynamicData<'g> {
    /// Name of the dynamically invoked method
    pub name: UnqualifiedName,

    /// Type of the dynamically invoked method
    pub descriptor: MethodDescriptor<&'g ClassData<'g>>,

    /// Bootstrap method
    pub bootstrap: &'g BootstrapMethodData<'g>,
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
    ///
    /// This must be a static method.
    pub method: &'g MethodData<'g>,

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

#[derive(PartialEq, Clone)]
pub enum ConstantData<'g> {
    String(Cow<'static, str>),
    Class(RefType<&'g ClassData<'g>>),
    Integer(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    FieldGetterHandle(&'g FieldData<'g>),
    FieldSetterHandle(&'g FieldData<'g>),
    MethodHandle(&'g MethodData<'g>),
}

impl<'g> Eq for ConstantData<'g> {}

impl<'g> Debug for ConstantData<'g> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConstantData::String(string) => string.fmt(f),
            ConstantData::Class(ref_type) => ref_type.fmt(f),
            ConstantData::Integer(integer) => integer.fmt(f),
            ConstantData::Long(long) => long.fmt(f),
            ConstantData::Float(float) => float.fmt(f),
            ConstantData::Double(double) => double.fmt(f),
            ConstantData::FieldGetterHandle(field) => field.fmt(f),
            ConstantData::FieldSetterHandle(field) => field.fmt(f),
            ConstantData::MethodHandle(method) => method.fmt(f),
        }
    }
}
