use crate::jvm::class_file;
use crate::jvm::class_file::ConstantsPool;
use crate::jvm::class_graph::{ConstantData, FieldId};
use crate::jvm::descriptors::RenderDescriptor;
use crate::jvm::names::Name;
use crate::jvm::Error;

/// Semantic representation of a field
pub struct Field<'g> {
    /// The current field
    pub id: FieldId<'g>,

    /// Generic method signature
    ///
    /// [Format](https://docs.oracle.com/javase/specs/jvms/se11/html/jvms-4.html#jvms-4.7.9.1)
    pub generic_signature: Option<String>,

    /// Constant field value
    pub constant_value: Option<ConstantData<'g>>,
}

impl<'g> Field<'g> {
    /// Create a new field
    pub fn new(id: FieldId<'g>) -> Field<'g> {
        Field {
            id,
            generic_signature: None,
            constant_value: None,
        }
    }

    /// Serialize the field
    pub fn serialize_field(
        self,
        constants_pool: &mut ConstantsPool<'g>,
    ) -> Result<class_file::Field, Error> {
        let access_flags = self.id.access_flags;
        let name_index = constants_pool.get_utf8(self.id.name.as_str())?;
        let descriptor_index = constants_pool.get_utf8(&self.id.descriptor.render())?;

        let mut attributes = vec![];

        // `Signature` attribute
        if let Some(signature) = self.generic_signature {
            let signature = constants_pool.get_utf8(signature)?;
            let signature = class_file::Signature { signature };
            attributes.push(constants_pool.get_attribute(signature)?);
        }

        // `ConstantValue` attribute
        if self.constant_value.is_some() {
            todo!();
        }

        Ok(class_file::Field {
            access_flags,
            name_index,
            descriptor_index,
            attributes,
        })
    }
}
