use super::*;

use std::borrow::Cow;
use std::cell::{RefCell, RefMut};
use std::rc::Rc;

pub struct ClassBuilder {
    /// This class name
    this_class: String,

    /// Class file, but with `constants` left blank
    class: ClassFile,

    /// Class graph
    class_graph: Rc<RefCell<ClassGraph>>,

    /// Constants pool
    constants_pool: Rc<RefCell<ConstantsPool>>,
}

impl ClassBuilder {
    /// Create a new class builder
    pub fn new(
        access_flags: ClassAccessFlags,
        this_class: String,
        super_class: String,
        is_interface: bool,
        interfaces: Vec<String>,
        class_graph: Rc<RefCell<ClassGraph>>,
    ) -> Result<ClassBuilder, Error> {
        let mut class_data = ClassData::new(super_class.clone(), is_interface);
        class_data.add_interfaces(interfaces.iter().cloned());

        // Make sure this class is in the class graph
        class_graph
            .borrow_mut()
            .classes
            .entry(Cow::Owned(this_class.clone()))
            .or_insert(class_data);

        // Construct a fresh constant pool
        let mut constants = ConstantsPool::new();
        let this_class_utf8 = constants.get_utf8(&this_class)?;
        let super_class_utf8 = constants.get_utf8(super_class)?;

        let class = ClassFile {
            version: Version::JAVA8,
            constants: OffsetVec::new(),
            access_flags,
            this_class: constants.get_class(this_class_utf8)?,
            super_class: constants.get_class(super_class_utf8)?,
            interfaces: interfaces
                .iter()
                .map(|interface| {
                    let interface_utf8 = constants.get_utf8(interface)?;
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
        name: String,
        descriptor: String,
    ) -> Result<(), Error> {
        let name_index = self.constants_pool.borrow_mut().get_utf8(&name)?;
        let descriptor_index = self.constants_pool.borrow_mut().get_utf8(&descriptor)?;
        let descriptor = FieldType::parse(&descriptor).map_err(Error::IoError)?;

        self.class.fields.push(Field {
            access_flags,
            name_index,
            descriptor_index,
            attributes: vec![],
        });

        let class_str: &str = &self.this_class;
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
        &mut self,
        access_flags: MethodAccessFlags,
        name: String,
        descriptor: String,
        code: Option<Code>,
    ) -> Result<(), Error> {
        let name_index = self.constants_pool.borrow_mut().get_utf8(&name)?;
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

        let class_str: &str = &self.this_class;
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
        &mut self,
        access_flags: MethodAccessFlags,
        name: String,
        descriptor: MethodDescriptor,
    ) -> Result<MethodBuilder, Error> {
        let is_static = access_flags.contains(MethodAccessFlags::STATIC);
        let class_str: &str = &self.this_class;
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
            &name == "<init>",
            self.class_graph.clone(),
            self.constants_pool.clone(),
            RefType::object(self.this_class.clone()),
        );

        Ok(MethodBuilder {
            name,
            access_flags,
            descriptor: rendered_descriptor,
            code,
        })
    }

    pub fn finish_method(&mut self, builder: MethodBuilder) -> Result<(), Error> {
        let name_index = self.constants_pool.borrow_mut().get_utf8(&builder.name)?;
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

    pub fn class_name(&self) -> &str {
        &self.this_class
    }
}

pub struct MethodBuilder {
    /// This method name
    name: String,

    /// Access flags
    access_flags: MethodAccessFlags,

    /// This method descriptor
    descriptor: String,

    /// Code builder
    pub code: BytecodeBuilder,
}
