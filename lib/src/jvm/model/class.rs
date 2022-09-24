use crate::jvm::class_file;
use crate::jvm::class_file::{
    BootstrapMethod, BootstrapMethods, ClassConstantIndex, ClassFile, ConstantIndex,
    ConstantPoolOverflow, ConstantsPool, ConstantsWriter, InnerClass, InnerClasses, NestHost,
    NestMembers, Version,
};
use crate::jvm::class_graph::{ClassId, ConstantData, NestData};
use crate::jvm::model::{Field, Method};
use crate::jvm::{Error, Name};
use crate::util::RefId;
use std::collections::HashMap;

/// Semantic representation of a class
pub struct Class<'g> {
    /// The current class
    pub id: ClassId<'g>,

    /// Fields
    ///
    /// Use [`Self::add_field`] for additional validation (like sanity checking that the field
    /// added really does belong on this class)
    pub fields: Vec<Field<'g>>,

    /// Methods
    ///
    /// Use [`Self::add_method`] for additional validation (like sanity checking that the method
    /// added really does belong on this class)
    pub methods: Vec<Method<'g>>,
}

impl<'g> Class<'g> {
    /// Create a new class
    pub fn new(id: ClassId<'g>) -> Class<'g> {
        Class {
            id,
            fields: vec![],
            methods: vec![],
        }
    }

    /// Serialize the class into a class file
    ///
    /// This handles settings several attributes:
    ///
    ///   - [`BootstrapMethods`] based on the `invokedynamic` calls in all of the methods
    ///   - [`NestHost`] based on the outer class chain (if any)
    ///   - [`NestMembers`] based on all members registered on the class graph (note: you should
    ///     make sure all members are registered on the class graph before calling this!)
    ///   - [`InnerClasses`] based on all the classes that show up in the constant pool and which
    ///     are not nest hosts
    pub fn serialize(self, version: Version) -> Result<ClassFile, Error> {
        // Construct a fresh constant pool
        let mut constants_pool: ConstantsPool<'g> = ConstantsPool::new();
        let mut bootstrap_methods = HashMap::new();

        let this_class = self.id.constant_index(&mut constants_pool)?;
        let super_class = self
            .id
            .superclass
            .expect("Super class")
            .constant_index(&mut constants_pool)?;
        let interfaces: Vec<ClassConstantIndex> = self
            .id
            .0
            .interfaces
            .iter()
            .map(|interface| RefId(interface).constant_index(&mut constants_pool))
            .collect::<Result<_, _>>()?;
        let mut attributes = vec![];

        // Serialize fields and methods
        let fields: Vec<class_file::Field> = self
            .fields
            .into_iter()
            .map(|field| field.serialize_field(&mut constants_pool))
            .collect::<Result<Vec<class_file::Field>, Error>>()?;
        let methods: Vec<class_file::Method> = self
            .methods
            .into_iter()
            .map(|method| method.serialize_method(&mut constants_pool, &mut bootstrap_methods))
            .collect::<Result<Vec<class_file::Method>, Error>>()?;

        // `BootstrapMethods` attribute
        let mut bootstrap_methods: Vec<_> = bootstrap_methods.into_iter().collect();
        bootstrap_methods.sort_unstable_by_key(|(_, idx)| *idx);
        let bootstrap_methods = bootstrap_methods
            .into_iter()
            .map(
                |(bootstrap_method_data, _)| -> Result<BootstrapMethod, ConstantPoolOverflow> {
                    let bootstrap_method: ConstantIndex =
                        ConstantData::MethodHandle(bootstrap_method_data.method)
                            .constant_index(&mut constants_pool)?;

                    let bootstrap_arguments: Vec<ConstantIndex> = bootstrap_method_data
                        .arguments
                        .iter()
                        .map(|constant| constant.constant_index(&mut constants_pool))
                        .collect::<Result<Vec<_>, ConstantPoolOverflow>>()?;

                    Ok(BootstrapMethod {
                        bootstrap_method,
                        bootstrap_arguments,
                    })
                },
            )
            .collect::<Result<Vec<_>, ConstantPoolOverflow>>()?;
        attributes.push(constants_pool.get_attribute(BootstrapMethods(bootstrap_methods))?);

        // `NestHost`/`NestMember` attributes
        match &self.id.0.nest {
            _ if version < Version::JAVA11 => (),
            NestData::Host { members } if !members.is_empty() => {
                let nest_members = members
                    .iter()
                    .map(|member| -> Result<ClassConstantIndex, Error> {
                        let nest_member_class =
                            RefId(member).constant_index(&mut constants_pool)?;
                        Ok(nest_member_class)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                attributes.push(constants_pool.get_attribute(NestMembers(nest_members))?);
            }
            NestData::Member { .. } => {
                let nest_host_class = self.id.nest_host().constant_index(&mut constants_pool)?;
                attributes.push(constants_pool.get_attribute(NestHost(nest_host_class))?);
            }
            _ => (),
        }

        // `InnerClasses` attribute
        let classes_referenced: Vec<ClassId<'g>> = constants_pool.referenced_classes().collect();
        let mut inner_classes: Vec<InnerClass> = vec![];
        for class in classes_referenced {
            if let NestData::Member(nested_data) = &class.nest {
                let access_flags = nested_data.access_flags;
                let inner_class = class.constant_index(&mut constants_pool)?;
                let outer_class = nested_data
                    .enclosing_class
                    .constant_index(&mut constants_pool)?;
                let inner_name = match &nested_data.simple_name {
                    Some(name) => constants_pool.get_utf8(name.as_str())?,
                    None => ConstantIndex::ZERO,
                };
                inner_classes.push(InnerClass {
                    inner_class,
                    outer_class,
                    inner_name,
                    access_flags,
                });
            }
        }
        if !inner_classes.is_empty() {
            attributes.push(constants_pool.get_attribute(InnerClasses(inner_classes))?);
        }

        Ok(ClassFile {
            version,
            constants: constants_pool.into_offset_vec(),
            access_flags: self.id.access_flags,
            this_class,
            super_class,
            interfaces,
            fields,
            methods,
            attributes,
        })
    }

    /// Add a method to the class
    pub fn add_method(&mut self, method: Method<'g>) {
        assert_eq!(
            method.id.class, self.id,
            "Method doesn't belong to this class"
        );
        self.methods.push(method);
    }

    /// Add a field to the class
    pub fn add_field(&mut self, field: Field<'g>) {
        assert_eq!(
            field.id.class, self.id,
            "Method doesn't belong to this class"
        );
        self.fields.push(field);
    }
}

#[test]
fn sample_class() -> Result<(), Error> {
    use crate::jvm::class_file::Serialize;
    use crate::jvm::class_graph::{ClassData, ClassGraph, ClassGraphArenas, FieldData, MethodData};
    use crate::jvm::code::{
        BranchInstruction::*, CodeBuilder, Instruction::*, InvokeType, OrdComparison,
    };
    use crate::jvm::{
        BinaryName, ClassAccessFlags, FieldAccessFlags, FieldType, MethodAccessFlags,
        MethodDescriptor, UnqualifiedName,
    };

    let class_graph_arenas = ClassGraphArenas::new();
    let class_graph = ClassGraph::new(&class_graph_arenas);
    let java = class_graph.insert_java_library_types();

    // Declare the class and all the members first
    let class_id = class_graph.add_class(ClassData::new(
        BinaryName::from_string(String::from("me/alec/Point")).unwrap(),
        java.classes.lang.object,
        ClassAccessFlags::PUBLIC,
        None,
    ));
    let field_x = class_graph.add_field(FieldData {
        class: class_id,
        name: UnqualifiedName::from_string(String::from("x")).unwrap(),
        descriptor: FieldType::int(),
        access_flags: FieldAccessFlags::PUBLIC,
    });
    let field_y = class_graph.add_field(FieldData {
        class: class_id,
        name: UnqualifiedName::from_string(String::from("y")).unwrap(),
        descriptor: FieldType::int(),
        access_flags: FieldAccessFlags::PUBLIC,
    });
    let method_id = class_graph.add_method(MethodData {
        class: class_id,
        name: UnqualifiedName::INIT,
        descriptor: MethodDescriptor {
            parameters: vec![FieldType::int(), FieldType::int()],
            return_type: None,
        },
        access_flags: MethodAccessFlags::PUBLIC,
    });

    // Make the class
    let mut class = Class::new(class_id);
    class.add_field(Field {
        id: field_x,
        generic_signature: None,
        constant_value: None,
    });
    class.add_field(Field {
        id: field_y,
        generic_signature: None,
        constant_value: None,
    });

    let mut code = CodeBuilder::new(&class_graph, &java, method_id);

    let end = code.fresh_label();

    code.push_instruction(ALoad(0))?;
    code.push_instruction(Invoke(InvokeType::Special, java.members.lang.object.init))?;
    code.push_instruction(ALoad(0))?;
    code.push_instruction(ILoad(1))?;
    code.push_instruction(PutField(field_x))?;
    code.push_instruction(ILoad(2))?;
    code.push_branch_instruction(If(OrdComparison::LT, end, ()))?;

    code.push_instruction(ALoad(0))?;
    code.push_instruction(ILoad(2))?;
    code.push_instruction(PutField(field_y))?;

    code.place_label(end)?;
    code.push_branch_instruction(Return)?;

    class.add_method(Method {
        id: method_id,
        code_impl: Some(code.result()?),
        exceptions: vec![],
        generic_signature: None,
    });

    let class_file = class.serialize(Version::JAVA11)?;

    let mut f: Vec<u8> = vec![];
    class_file.serialize(&mut f).map_err(Error::IoError)?;

    Ok(())
}
