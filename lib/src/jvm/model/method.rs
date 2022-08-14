use crate::jvm::class_file;
use crate::jvm::class_file::ConstantsPool;
use crate::jvm::class_graph::{BootstrapMethodId, ClassId, MethodId};
use crate::jvm::code::Code;
use crate::jvm::descriptors::RenderDescriptor;
use crate::jvm::names::Name;
use crate::jvm::Error;
use std::collections::HashMap;

/// Semantic representation of a method
pub struct Method<'g> {
    /// The current method
    pub id: MethodId<'g>,

    /// Method code implementation
    pub code_impl: Option<Code<'g>>,

    /// Which exceptions can this method throw?
    ///
    /// Note: this does not need to include `RuntimeException`, `Error`, or subclasses
    pub exceptions: Vec<ClassId<'g>>,

    /// Generic method signature
    ///
    /// [Format](https://docs.oracle.com/javase/specs/jvms/se11/html/jvms-4.html#jvms-4.7.9.1)
    pub generic_signature: Option<String>,
}

impl<'g> Method<'g> {
    /// Create a new method
    pub fn new(id: MethodId<'g>) -> Method<'g> {
        Method {
            id,
            code_impl: None,
            exceptions: vec![],
            generic_signature: None,
        }
    }

    /// Serialize the method
    pub fn serialize_method(
        self,
        constants_pool: &mut ConstantsPool<'g>,
        bootstrap_methods: &mut HashMap<BootstrapMethodId<'g>, u16>,
    ) -> Result<class_file::Method, Error> {
        let access_flags = self.id.access_flags;
        let name_index = constants_pool.get_utf8(self.id.name.as_str())?;
        let descriptor_index = constants_pool.get_utf8(&self.id.descriptor.render())?;

        let mut attributes = vec![];

        // `Code` attribute
        if let Some(code) = self.code_impl {
            let code = code.serialize_code(constants_pool, bootstrap_methods)?;
            attributes.push(constants_pool.get_attribute(code)?);
        }

        // `Signature` attribute
        if let Some(signature) = self.generic_signature {
            let signature = constants_pool.get_utf8(signature)?;
            let signature = class_file::Signature { signature };
            attributes.push(constants_pool.get_attribute(signature)?);
        }

        Ok(class_file::Method {
            access_flags,
            name_index,
            descriptor_index,
            attributes,
        })
    }
}
