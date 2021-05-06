use super::*;

use std::cell::RefCell;
use std::collections::HashMap;
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
        // Make sure this class is in the class graph
        class_graph
            .borrow_mut()
            .classes
            .entry(this_class.clone())
            .or_insert(ClassData {
                superclass: Some(super_class.clone()),
                interfaces: interfaces.iter().cloned().collect(),
                is_interface,
                members: HashMap::new(),
            });

        // Construct a fresh constant pool
        let mut constants = ConstantsPool::new();
        let this_class_utf8 = constants.get_utf8(&this_class)?;
        let super_class_utf8 = constants.get_utf8(&super_class)?;

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
        is_static: bool,
        access_flags: FieldAccessFlags,
        name: String,
        descriptor: String,
    ) -> Result<(), Error> {
        let name_index = self.constants_pool.borrow_mut().get_utf8(&name)?;
        let descriptor_index = self.constants_pool.borrow_mut().get_utf8(&descriptor)?;
        let descriptor = MethodDescriptor::parse(&descriptor).map_err(Error::IoError)?;

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
            .members
            .entry(name)
            .or_insert(ClassMember::Field {
                is_static,
                descriptor,
            });

        Ok(())
    }

    /// Add a method to the class
    pub fn add_method(
        &mut self,
        is_static: bool,
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

        self.class_graph
            .borrow_mut()
            .classes
            .get_mut(&self.this_class)
            .expect("class cannot be found in class graph")
            .members
            .entry(name)
            .or_insert(ClassMember::Method {
                is_static,
                descriptor,
            });

        Ok(())
    }
}
