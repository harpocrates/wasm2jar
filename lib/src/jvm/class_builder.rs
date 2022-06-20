use super::*;

use elsa::FrozenVec;
use std::cell::RefCell;
use std::rc::Rc;

pub struct ClassBuilder {
    /// Class file version
    version: Version,

    /// Constants pool
    pub constants_pool: ConstantsPool,

    /// Class access flags
    access_flags: ClassAccessFlags,

    /// Class name
    this_class: BinaryName,

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
    class_graph: Rc<RefCell<ClassGraph>>,
}

impl ClassBuilder {
    /// Create a new class builder
    pub fn new(
        access_flags: ClassAccessFlags,
        this_class: BinaryName,
        super_class: BinaryName,
        is_interface: bool,
        interfaces: Vec<BinaryName>,
        class_graph: Rc<RefCell<ClassGraph>>,
    ) -> Result<ClassBuilder, Error> {
        let mut class_data = ClassData::new(super_class.clone(), is_interface);
        class_data.add_interfaces(interfaces.iter().cloned());

        // Make sure this class is in the class graph
        class_graph
            .borrow_mut()
            .classes
            .entry(this_class.clone())
            .or_insert(class_data);

        // Construct a fresh constant pool
        let constants_pool = ConstantsPool::new();
        let this_class_utf8 = constants_pool.get_utf8(this_class.as_str())?;
        let super_class_utf8 = constants_pool.get_utf8(super_class.as_str())?;
        let this_class_index = constants_pool.get_class(this_class_utf8)?;
        let super_class_index = constants_pool.get_class(super_class_utf8)?;
        let interfaces = interfaces
            .iter()
            .map(|interface| {
                let interface_utf8 = constants_pool.get_utf8(interface.as_str())?;
                constants_pool.get_class(interface_utf8)
            })
            .collect::<Result<_, _>>()?;

        Ok(ClassBuilder {
            version: Version::JAVA11,
            constants_pool,
            access_flags,
            this_class,
            this_class_index,
            super_class_index,
            interfaces,
            fields: FrozenVec::new(),
            methods: FrozenVec::new(),
            attributes: FrozenVec::new(),
            class_graph,
        })
    }

    /// Consume the builder and return the file class file
    ///
    /// Only call this if all associated builders have been released
    pub fn result(self) -> ClassFile {
        ClassFile {
            version: self.version,
            constants: self.constants_pool.into_offset_vec(),
            access_flags: self.access_flags,
            this_class: self.this_class_index,
            super_class: self.super_class_index,
            interfaces: self.interfaces,
            fields: self.fields.into_vec().into_iter().map(|x| *x).collect(),
            methods: self.methods.into_vec().into_iter().map(|x| *x).collect(),
            attributes: self.attributes.into_vec().into_iter().map(|x| *x).collect(),
        }
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
        descriptor: String,
    ) -> Result<(), Error> {
        self.add_field_with_signature(access_flags, name, descriptor, None)
    }

    /// Add a field with a generic signature to the class
    pub fn add_field_with_signature(
        &self,
        access_flags: FieldAccessFlags,
        name: UnqualifiedName,
        descriptor: String,
        signature: Option<String>,
    ) -> Result<(), Error> {
        let name_index = self.constants_pool.get_utf8(name.as_str())?;
        let descriptor_index = self.constants_pool.get_utf8(&descriptor)?;
        let descriptor = FieldType::parse(&descriptor).map_err(Error::IoError)?;
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

        let class_str: &BinaryName = &self.this_class;
        self.class_graph
            .borrow_mut()
            .classes
            .get_mut(class_str)
            .expect("class cannot be found in class graph")
            .add_field(
                access_flags.contains(FieldAccessFlags::STATIC),
                name,
                descriptor,
            );

        Ok(())
    }

    /// Add a method to the class
    pub fn add_method(
        &self,
        access_flags: MethodAccessFlags,
        name: UnqualifiedName,
        descriptor: String,
        code: Option<Code>,
    ) -> Result<(), Error> {
        let name_index = self.constants_pool.get_utf8(name.as_str())?;
        let descriptor_index = self.constants_pool.get_utf8(&descriptor)?;
        let descriptor = MethodDescriptor::parse(&descriptor).map_err(Error::IoError)?;
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

        let class_str: &BinaryName = &self.this_class;
        self.class_graph
            .borrow_mut()
            .classes
            .get_mut(class_str)
            .expect("class cannot be found in class graph")
            .add_method(
                access_flags.contains(MethodAccessFlags::STATIC),
                name,
                descriptor,
            );

        Ok(())
    }

    pub fn start_method(
        &self,
        access_flags: MethodAccessFlags,
        name: UnqualifiedName,
        descriptor: MethodDescriptor,
    ) -> Result<MethodBuilder, Error> {
        let is_static = access_flags.contains(MethodAccessFlags::STATIC);
        let class_str: &BinaryName = &self.this_class;
        let rendered_descriptor = descriptor.render();
        self.class_graph
            .borrow_mut()
            .classes
            .get_mut(class_str)
            .expect("class cannot be found in class graph")
            .add_method(is_static, name.clone(), descriptor.clone());

        let code = BytecodeBuilder::new(
            descriptor,
            !is_static,
            &name == &UnqualifiedName::INIT,
            self.class_graph.clone(),
            &self.constants_pool,
            RefType::Object(self.this_class.clone()),
        );

        Ok(MethodBuilder {
            name,
            access_flags,
            descriptor: rendered_descriptor,
            code,
            methods: &self.methods,
            constants_pool: &self.constants_pool,
        })
    }

    pub fn class_name(&self) -> &BinaryName {
        &self.this_class
    }
}

pub struct MethodBuilder<'a> {
    /// This method name
    name: UnqualifiedName,

    /// Access flags
    access_flags: MethodAccessFlags,

    /// This method descriptor
    descriptor: String,

    /// Code builder
    pub code: BytecodeBuilder<'a>,

    /// Where to ultimately push the result
    methods: &'a FrozenVec<Box<Method>>,

    /// Constants pool
    constants_pool: &'a ConstantsPool,
}

impl<'a> MethodBuilder<'a> {
    pub fn finish(self) -> Result<(), Error> {
        let name_index = self.constants_pool.get_utf8(self.name.as_str())?;
        let descriptor_index = self.constants_pool.get_utf8(&self.descriptor)?;

        let code = self.code.result()?;
        let code = self.constants_pool.get_attribute(code)?;
        let method = Method {
            access_flags: self.access_flags,
            name_index,
            descriptor_index,
            attributes: vec![code],
        };

        self.methods.push(Box::new(method));

        Ok(())
    }
}

#[test]
fn sample_class() -> Result<(), Error> {
    use BranchInstruction::*;
    use Instruction::*;

    let mut class_graph = ClassGraph::new();
    class_graph.insert_lang_types();
    let class_graph = Rc::new(RefCell::new(class_graph));

    let mut class_builder = ClassBuilder::new(
        ClassAccessFlags::PUBLIC,
        BinaryName::from_string(String::from("me/alec/Point")).unwrap(),
        BinaryName::from_string(String::from("java/lang/Object")).unwrap(),
        false,
        vec![],
        class_graph,
    )?;

    class_builder.add_field(
        FieldAccessFlags::PUBLIC,
        UnqualifiedName::from_string(String::from("x")).unwrap(),
        String::from("I"),
    )?;
    class_builder.add_field(
        FieldAccessFlags::PUBLIC,
        UnqualifiedName::from_string(String::from("y")).unwrap(),
        String::from("I"),
    )?;

    let mut method_builder = class_builder.start_method(
        MethodAccessFlags::PUBLIC,
        UnqualifiedName::INIT,
        MethodDescriptor {
            parameters: vec![FieldType::INT, FieldType::INT],
            return_type: None,
        },
    )?;
    let code = &mut method_builder.code;

    let object_name = code.constants().get_utf8("java/lang/Object")?;
    let object_cls = code.constants().get_class(object_name)?;
    let init_name = code.constants().get_utf8("<init>")?;
    let type_name = code.constants().get_utf8("()V")?;
    let name_and_type = code.constants().get_name_and_type(init_name, type_name)?;
    let init_ref = code
        .constants()
        .get_method_ref(object_cls, name_and_type, false)?;

    let this_name = code.constants().get_utf8("me/alec/Point")?;
    let this_cls = code.constants().get_class(this_name)?;
    let field_name_x = code.constants().get_utf8("x")?;
    let field_name_y = code.constants().get_utf8("y")?;
    let field_typ = code.constants().get_utf8("I")?;
    let x_name_and_type = code
        .constants()
        .get_name_and_type(field_name_x, field_typ)?;
    let y_name_and_type = code
        .constants()
        .get_name_and_type(field_name_y, field_typ)?;
    let field_x = code.constants().get_field_ref(this_cls, x_name_and_type)?;
    let field_y = code.constants().get_field_ref(this_cls, y_name_and_type)?;

    let end = code.fresh_label();

    code.push_instruction(ALoad(0))?;
    code.push_instruction(Invoke(InvokeType::Special, init_ref))?;
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

    let class_file = class_builder.result();

    let mut f: Vec<u8> = vec![];
    class_file.serialize(&mut f).map_err(Error::IoError)?;

    Ok(())
}
