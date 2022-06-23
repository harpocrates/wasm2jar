use super::*;
use std::result::Result;

pub trait ConstantsWriter<Index = ConstantIndex> {
    /// Get or insert a constant into the constant pool and return the associated index
    fn constant_index(&self, constants_pool: &ConstantsPool)
        -> Result<Index, ConstantPoolOverflow>;
}

impl<'g> ConstantsWriter<ClassConstantIndex> for ClassData<'g> {
    fn constant_index(
        &self,
        constants: &ConstantsPool,
    ) -> Result<ClassConstantIndex, ConstantPoolOverflow> {
        let class_name = constants.get_utf8(self.name.as_str())?;
        constants.get_class(class_name)
    }
}

/// When making a `CONSTANT_Class_info`, reference types are almost always objects. However,
/// there are a handful of places where an array type needs to be fit in (eg. for a `checkcast`
/// to an array type). See section 4.4.1 for more.
impl<'g> ConstantsWriter<ClassConstantIndex> for RefType<&'g ClassData<'g>> {
    fn constant_index(
        &self,
        constants: &ConstantsPool,
    ) -> Result<ClassConstantIndex, ConstantPoolOverflow> {
        let utf8_idx = match self {
            RefType::Object(class) => constants.get_utf8(class.name.as_str())?,
            other => constants.get_utf8(other.render())?,
        };
        constants.get_class(utf8_idx)
    }
}

impl<'g> ConstantsWriter<MethodRefConstantIndex> for MethodData<'g> {
    fn constant_index(
        &self,
        constants: &ConstantsPool,
    ) -> Result<MethodRefConstantIndex, ConstantPoolOverflow> {
        let class_idx = self.class.constant_index(constants)?;
        let method_utf8 = constants.get_utf8(self.name.as_str())?;
        let desc_utf8 = constants.get_utf8(&self.descriptor.render())?;
        let name_and_type_idx = constants.get_name_and_type(method_utf8, desc_utf8)?;
        constants.get_method_ref(class_idx, name_and_type_idx, self.class.is_interface)
    }
}

impl<'g> ConstantsWriter<FieldRefConstantIndex> for FieldData<'g> {
    fn constant_index(
        &self,
        constants: &ConstantsPool,
    ) -> Result<FieldRefConstantIndex, ConstantPoolOverflow> {
        let class_idx = self.class.constant_index(constants)?;
        let field_utf8 = constants.get_utf8(self.name.as_str())?;
        let desc_utf8 = constants.get_utf8(&self.descriptor.render())?;
        let name_and_type_idx = constants.get_name_and_type(field_utf8, desc_utf8)?;
        constants.get_field_ref(class_idx, name_and_type_idx)
    }
}

impl<'g> ConstantsWriter<ConstantIndex> for ConstantData<'g> {
    fn constant_index(
        &self,
        constants: &ConstantsPool,
    ) -> Result<ConstantIndex, ConstantPoolOverflow> {
        match self {
            ConstantData::String(string) => {
                let str_utf8 = constants.get_utf8(&**string)?;
                let str_idx = constants.get_string(str_utf8)?;
                Ok(str_idx.into())
            }
            ConstantData::Class(class) => Ok(class.constant_index(constants)?.into()),
            ConstantData::Integer(integer) => constants.get_integer(*integer),
            ConstantData::Long(long) => constants.get_long(*long),
            ConstantData::Float(float) => constants.get_float(*float),
            ConstantData::Double(double) => constants.get_double(*double),
            ConstantData::FieldGetterHandle(field) => {
                let field_idx = field.constant_index(constants)?;
                let handle = if field.is_static {
                    HandleKind::GetStatic
                } else {
                    HandleKind::GetField
                };
                constants.get_method_handle(handle, field_idx.into())
            }
            ConstantData::FieldSetterHandle(field) => {
                let field_idx = field.constant_index(constants)?;
                let handle = if field.is_static {
                    HandleKind::PutStatic
                } else {
                    HandleKind::PutField
                };
                constants.get_method_handle(handle, field_idx.into())
            }
            ConstantData::MethodHandle(method) => {
                let method_idx = method.constant_index(constants)?;
                let handle = if method.is_static {
                    HandleKind::InvokeStatic
                } else if method.class.is_interface {
                    HandleKind::InvokeInterface
                } else {
                    HandleKind::InvokeVirtual
                };
                constants.get_method_handle(handle, method_idx.into())
            }
        }
    }
}
