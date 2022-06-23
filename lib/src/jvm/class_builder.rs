use super::*;

use elsa::FrozenVec;

pub struct ClassBuilder<'g> {
    /// Class file version
    version: Version,

    /// Constants pool
    pub constants_pool: ConstantsPool,

    /// Class access flags
    access_flags: ClassAccessFlags,

    /// Class name constant
    this_class_index: ClassConstantIndex,

    /// Superclass name constant
    super_class_index: ClassConstantIndex,

    /// Implemented interfaces constants
    interfaces: Vec<ClassConstantIndex>,

    /// Fields
    fields: FrozenVec<Box<Field>>,

    /// Methods
    methods: FrozenVec<Box<Method>>,

    /// Attributes
    attributes: FrozenVec<Box<Attribute>>,

    /// Class graph
    pub class_graph: &'g ClassGraph<'g>,

    /// Java
    pub java: &'g JavaLibrary<'g>,

    /// Reference to class data in the class graph
    pub class: &'g ClassData<'g>,

    /// Bootstrap methods
    pub bootstrap_methods: FrozenVec<Box<BootstrapMethodData<'g>>>,
}

impl<'g> ClassBuilder<'g> {
    /// Create a new class builder
    pub fn new(
        access_flags: ClassAccessFlags,
        this_class: BinaryName,
        super_class: &'g ClassData<'g>,
        is_interface: bool,
        interfaces: Vec<&'g ClassData<'g>>,
        class_graph: &'g ClassGraph<'g>,
        java: &'g JavaLibrary<'g>,
    ) -> Result<ClassBuilder<'g>, Error> {
        // Make sure this class is in the class graph
        let class = class_graph.add_class(ClassData::new(this_class, super_class, is_interface));
        for interface in &interfaces {
            class.interfaces.push(interface);
        }

        // Construct a fresh constant pool
        let constants_pool = ConstantsPool::new();
        let this_class_utf8 = constants_pool.get_utf8(class.name.as_str())?;
        let super_class_utf8 = constants_pool.get_utf8(super_class.name.as_str())?;
        let this_class_index = constants_pool.get_class(this_class_utf8)?;
        let super_class_index = constants_pool.get_class(super_class_utf8)?;
        let interfaces = interfaces
            .iter()
            .map(|interface| {
                let interface_utf8 = constants_pool.get_utf8(interface.name.as_str())?;
                constants_pool.get_class(interface_utf8)
            })
            .collect::<Result<_, _>>()?;

        Ok(ClassBuilder {
            version: Version::JAVA11,
            constants_pool,
            access_flags,
            this_class_index,
            super_class_index,
            interfaces,
            fields: FrozenVec::new(),
            methods: FrozenVec::new(),
            attributes: FrozenVec::new(),
            class_graph,
            java,
            class,
            bootstrap_methods: FrozenVec::new(),
        })
    }

    /// Consume the builder and return the file class file
    ///
    /// Only call this if all associated builders have been released
    pub fn result(self) -> Result<ClassFile, Error> {
        let constants_pool = &self.constants_pool;
        let bootstrap_methods: Vec<BootstrapMethod> = self
            .bootstrap_methods
            .iter()
            .map(|bootstrap_method_data: &BootstrapMethodData<'g>| -> Result<BootstrapMethod, ConstantPoolOverflow> {
                let bootstrap_method: ConstantIndex = bootstrap_method_data.method
                    .constant_index(constants_pool)?
                    .into();
                let bootstrap_method: ConstantIndex = constants_pool.get_method_handle(
                    HandleKind::InvokeStatic,
                    bootstrap_method,
                )?;

                let bootstrap_arguments: Vec<ConstantIndex> = bootstrap_method_data
                    .arguments
                    .iter()
                    .map(|constant| constant.constant_index(constants_pool))
                    .collect::<Result<Vec<_>, ConstantPoolOverflow>>()?;

                Ok(BootstrapMethod { bootstrap_method, bootstrap_arguments })
            })
            .collect::<Result<Vec<_>, ConstantPoolOverflow>>()?;
        self.add_attribute(BootstrapMethods(bootstrap_methods))?;

        Ok(ClassFile {
            version: self.version,
            constants: self.constants_pool.into_offset_vec(),
            access_flags: self.access_flags,
            this_class: self.this_class_index,
            super_class: self.super_class_index,
            interfaces: self.interfaces,
            fields: self.fields.into_vec().into_iter().map(|x| *x).collect(),
            methods: self.methods.into_vec().into_iter().map(|x| *x).collect(),
            attributes: self.attributes.into_vec().into_iter().map(|x| *x).collect(),
        })
    }

    /// Add an attribute to the class
    pub fn add_attribute(&self, attribute: impl AttributeLike) -> Result<(), Error> {
        let attribute = self.constants_pool.get_attribute(attribute)?;
        self.attributes.push(Box::new(attribute));
        Ok(())
    }

    /// Add a field to the class
    pub fn add_field(
        &self,
        access_flags: FieldAccessFlags,
        name: UnqualifiedName,
        descriptor: FieldType<&'g ClassData<'g>>,
    ) -> Result<&'g FieldData<'g>, Error> {
        self.add_field_with_signature(access_flags, name, descriptor, None)
    }

    /// Add a field with a generic signature to the class
    pub fn add_field_with_signature(
        &self,
        access_flags: FieldAccessFlags,
        name: UnqualifiedName,
        descriptor: FieldType<&'g ClassData<'g>>,
        signature: Option<String>,
    ) -> Result<&'g FieldData<'g>, Error> {
        let name_index = self.constants_pool.get_utf8(name.as_str())?;
        let descriptor_index = self.constants_pool.get_utf8(&descriptor.render())?;
        let mut attributes: Vec<Attribute> = vec![];

        // Add the optional generic `Signature` attribute
        if let Some(generic_sig) = signature {
            let sig = self.constants_pool.get_utf8(generic_sig)?;
            let sig = Signature { signature: sig };
            attributes.push(self.constants_pool.get_attribute(sig)?);
        }

        self.fields.push(Box::new(Field {
            access_flags,
            name_index,
            descriptor_index,
            attributes,
        }));

        let field = self.class_graph.add_field(FieldData {
            class: self.class,
            name,
            descriptor,
            is_static: access_flags.contains(FieldAccessFlags::STATIC),
        });

        Ok(field)
    }

    /// Add a method to the class
    pub fn add_method(
        &self,
        access_flags: MethodAccessFlags,
        name: UnqualifiedName,
        descriptor: MethodDescriptor<&'g ClassData<'g>>,
        code: Option<Code>,
    ) -> Result<(), Error> {
        let name_index = self.constants_pool.get_utf8(name.as_str())?;
        let descriptor_index = self.constants_pool.get_utf8(&descriptor.render())?;
        let mut attributes = vec![];

        if let Some(code) = code {
            attributes.push(self.constants_pool.get_attribute(code)?);
        }

        self.methods.push(Box::new(Method {
            access_flags,
            name_index,
            descriptor_index,
            attributes,
        }));

        self.class_graph.add_method(MethodData {
            class: self.class,
            name,
            descriptor,
            is_static: access_flags.contains(MethodAccessFlags::STATIC),
        });

        Ok(())
    }

    pub fn start_method<'a>(
        &'a self,
        access_flags: MethodAccessFlags,
        name: UnqualifiedName,
        descriptor: MethodDescriptor<&'g ClassData<'g>>,
    ) -> Result<MethodBuilder<'a, 'g>, Error> {
        let is_static = access_flags.contains(MethodAccessFlags::STATIC);
        let method = self.class_graph.add_method(MethodData {
            class: self.class,
            name,
            descriptor,
            is_static,
        });
        self.implement_method(access_flags, method)
    }

    /// Start implementing a new method (that is already recorded in the class graph)
    pub fn implement_method<'a>(
        &'a self,
        access_flags: MethodAccessFlags,
        method: &'g MethodData<'g>,
    ) -> Result<MethodBuilder<'a, 'g>, Error> {
        let code = BytecodeBuilder::new(
            self.class_graph,
            self.java,
            &self.constants_pool,
            &self.bootstrap_methods,
            method,
        );

        Ok(MethodBuilder {
            access_flags,
            code,
            methods: &self.methods,
            constants_pool: &self.constants_pool,
            method,
            attributes: vec![],
        })
    }
}

pub struct MethodBuilder<'a, 'g> {
    /// Access flags
    access_flags: MethodAccessFlags,

    /// Code builder
    pub code: BytecodeBuilder<'a, 'g>,

    /// Where to ultimately push the result
    methods: &'a FrozenVec<Box<Method>>,

    /// Constants pool
    constants_pool: &'a ConstantsPool,

    /// The current method
    pub method: &'g MethodData<'g>,

    /// Method attributes
    ///
    /// The "Code" attribute will be automatically added
    pub attributes: Vec<Attribute>,
}

impl<'a, 'g> MethodBuilder<'a, 'g> {
    pub fn add_generic_signature(&mut self, signature: &str) -> Result<(), Error> {
        let signature = self.constants_pool.get_utf8(signature)?;
        let signature = Signature { signature };
        self.attributes
            .push(self.constants_pool.get_attribute(signature)?);
        Ok(())
    }

    pub fn finish(self) -> Result<(), Error> {
        let name_index = self.constants_pool.get_utf8(self.method.name.as_str())?;
        let descriptor_index = self
            .constants_pool
            .get_utf8(&self.method.descriptor.render())?;

        let code = self.code.result()?;
        let mut attributes = self.attributes;
        attributes.push(self.constants_pool.get_attribute(code)?);
        let method = Method {
            access_flags: self.access_flags,
            name_index,
            descriptor_index,
            attributes,
        };

        self.methods.push(Box::new(method));

        Ok(())
    }
}

#[test]
fn sample_class() -> Result<(), Error> {
    use BranchInstruction::*;
    use Instruction::*;

    let class_graph_arenas = ClassGraphArenas::new();
    let class_graph = ClassGraph::new(&class_graph_arenas);
    let java = class_graph.insert_java_library_types();

    let class_builder = ClassBuilder::new(
        ClassAccessFlags::PUBLIC,
        BinaryName::from_string(String::from("me/alec/Point")).unwrap(),
        java.classes.lang.object,
        false,
        vec![],
        &class_graph,
        &java,
    )?;

    let field_x = class_builder.add_field(
        FieldAccessFlags::PUBLIC,
        UnqualifiedName::from_string(String::from("x")).unwrap(),
        FieldType::int(),
    )?;
    let field_y = class_builder.add_field(
        FieldAccessFlags::PUBLIC,
        UnqualifiedName::from_string(String::from("y")).unwrap(),
        FieldType::int(),
    )?;

    let mut method_builder = class_builder.start_method(
        MethodAccessFlags::PUBLIC,
        UnqualifiedName::INIT,
        MethodDescriptor {
            parameters: vec![FieldType::int(), FieldType::int()],
            return_type: None,
        },
    )?;
    let code = &mut method_builder.code;

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

    method_builder.finish()?;

    let class_file = class_builder.result()?;

    let mut f: Vec<u8> = vec![];
    class_file.serialize(&mut f).map_err(Error::IoError)?;

    Ok(())
}
