use super::*;

use std::cell::{RefCell, RefMut};
use std::rc::Rc;

pub struct ClassBuilder<'a> {
    /// This class name
    this_class: BinaryName<'a>,

    /// Class file, but with `constants` left blank
    class: ClassFile,

    /// Class graph
    class_graph: Rc<RefCell<ClassGraph<'a>>>,

    /// Constants pool
    constants_pool: Rc<RefCell<ConstantsPool>>,
}

impl<'a> ClassBuilder<'a> {
    /// Create a new class builder
    pub fn new(
        access_flags: ClassAccessFlags,
        this_class: BinaryName<'a>,
        super_class: BinaryName<'a>,
        is_interface: bool,
        interfaces: Vec<BinaryName<'a>>,
        class_graph: Rc<RefCell<ClassGraph<'a>>>,
    ) -> Result<ClassBuilder<'a>, Error> {

        // Construct a fresh constant pool
        let mut constants = ConstantsPool::new();
        let this_class_utf8 = constants.get_utf8(this_class.as_ref())?;
        let super_class_utf8 = constants.get_utf8(super_class.as_ref())?;

        let mut class_data = ClassData::new(super_class, is_interface);
        class_data.add_interfaces(interfaces.iter().cloned());

        // Make sure this class is in the class graph
        class_graph
            .borrow_mut()
            .classes
            .entry(this_class.clone())
            .or_insert(class_data);

        let class = ClassFile {
            version: Version::JAVA11,
            constants: OffsetVec::new(),
            access_flags,
            this_class: constants.get_class(this_class_utf8)?,
            super_class: constants.get_class(super_class_utf8)?,
            interfaces: interfaces
                .iter()
                .map(|interface| {
                    let interface_utf8 = constants.get_utf8(interface.as_ref())?;
                    constants.get_class(interface_utf8)
                })
                .collect::<Result<_, _>>()?,
            fields: vec![],
            methods: vec![],
            attributes: vec![],
        };

        Ok(ClassBuilder {
            this_class,
            class,
            class_graph,
            constants_pool: Rc::new(RefCell::new(constants)),
        })
    }

    /// Consume the builder and return the file class file
    ///
    /// Only call this if all associated builder have been released
    pub fn result(mut self) -> ClassFile {
        self.class.constants = Rc::try_unwrap(self.constants_pool)
            .ok()
            .expect("cannot unwrap reference constant pool (there is still a reference to it)")
            .into_inner()
            .into_offset_vec();
        self.class
    }

    /// Add an attribute to the class
    pub fn add_attribute(&mut self, attribute: impl AttributeLike) -> Result<(), Error> {
        let attribute = self.constants_pool.borrow_mut().get_attribute(attribute)?;
        self.class.attributes.push(attribute);
        Ok(())
    }

    /// Add a field to the class
    pub fn add_field(
        &mut self,
        access_flags: FieldAccessFlags,
        name: UnqualifiedName<'a>,
        descriptor: String,
    ) -> Result<(), Error> {
        let name_index = self.constants_pool.borrow_mut().get_utf8(name.as_ref())?;
        let descriptor_index = self.constants_pool.borrow_mut().get_utf8(&descriptor)?;
        let descriptor = FieldType::parse(&descriptor).map_err(Error::IoError)?;

        self.class.fields.push(Field {
            access_flags,
            name_index,
            descriptor_index,
            attributes: vec![],
        });

        self.class_graph
            .borrow_mut()
            .classes
            .get_mut(&self.this_class)
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
        &mut self,
        access_flags: MethodAccessFlags,
        name: UnqualifiedName<'a>,
        descriptor: String,
        code: Option<Code>,
    ) -> Result<(), Error> {
        let name_index = self.constants_pool.borrow_mut().get_utf8(name.as_ref())?;
        let descriptor_index = self.constants_pool.borrow_mut().get_utf8(&descriptor)?;
        let descriptor = MethodDescriptor::parse(&descriptor).map_err(Error::IoError)?;
        let mut attributes = vec![];

        if let Some(code) = code {
            attributes.push(self.constants_pool.borrow_mut().get_attribute(code)?);
        }

        self.class.methods.push(Method {
            access_flags,
            name_index,
            descriptor_index,
            attributes,
        });

        self.class_graph
            .borrow_mut()
            .classes
            .get_mut(&self.this_class)
            .expect("class cannot be found in class graph")
            .add_method(
                access_flags.contains(MethodAccessFlags::STATIC),
                name,
                descriptor,
            );

        Ok(())
    }

    pub fn start_method(
        &mut self,
        access_flags: MethodAccessFlags,
        name: UnqualifiedName<'a>,
        descriptor: MethodDescriptor<'a>,
    ) -> Result<MethodBuilder<'a>, Error> {
        let is_static = access_flags.contains(MethodAccessFlags::STATIC);
        let rendered_descriptor = descriptor.render();
        self.class_graph
            .borrow_mut()
            .classes
            .get_mut(&self.this_class)
            .expect("class cannot be found in class graph")
            .add_method(is_static, name, descriptor.clone());

        let code = BytecodeBuilder::new(
            descriptor,
            !is_static,
            name == UnqualifiedName::INIT,
            self.class_graph.clone(),
            self.constants_pool.clone(),
            RefType::Object(self.this_class),
        );

        Ok(MethodBuilder {
            name,
            access_flags,
            descriptor: rendered_descriptor,
            code,
        })
    }

    pub fn finish_method(&mut self, builder: MethodBuilder) -> Result<(), Error> {
        let name_index = self.constants_pool.borrow_mut().get_utf8(builder.name.as_ref())?;
        let descriptor_index = self
            .constants_pool
            .borrow_mut()
            .get_utf8(&builder.descriptor)?;

        let code = builder.code.result()?;
        let code = self.constants_pool.borrow_mut().get_attribute(code)?;

        self.class.methods.push(Method {
            access_flags: builder.access_flags,
            name_index,
            descriptor_index,
            attributes: vec![code],
        });

        Ok(())
    }

    pub fn constants(&self) -> RefMut<ConstantsPool> {
        self.constants_pool.borrow_mut()
    }

    pub fn class_name(&self) -> &BinaryName<'a> {
        &self.this_class
    }
}

pub struct MethodBuilder<'a> {
    /// This method name
    name: UnqualifiedName<'a>,

    /// Access flags
    access_flags: MethodAccessFlags,

    /// This method descriptor
    descriptor: String,

    /// Code builder
    pub code: BytecodeBuilder<'a>,
}

#[test]
fn sample_class() -> Result<(), Error> {
    use BranchInstruction::*;
    use Instruction::*;
    use std::convert::TryInto;

    let mut class_graph = ClassGraph::new();
    class_graph.insert_lang_types();
    let class_graph = Rc::new(RefCell::new(class_graph));

    let mut class_builder = ClassBuilder::new(
        ClassAccessFlags::PUBLIC,
        "me/alec/Point".try_into().unwrap(),
        BinaryName::OBJECT,
        false,
        vec![],
        class_graph,
    )?;

    class_builder.add_field(
        FieldAccessFlags::PUBLIC,
        "x".try_into().unwrap(),
        String::from("I"),
    )?;
    class_builder.add_field(
        FieldAccessFlags::PUBLIC,
        "y".try_into().unwrap(),
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

    class_builder.finish_method(method_builder)?;

    let class_file = class_builder.result();

    let mut f: Vec<u8> = vec![];
    class_file.serialize(&mut f).map_err(Error::IoError)?;

    Ok(())
}
