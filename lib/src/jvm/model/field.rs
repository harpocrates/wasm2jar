use crate::jvm::{FieldId, ConstantData};

/// In-memory representation of a field
pub struct Field<'g> {
    /// The current field
    pub field: FieldId<'g>,

    /// Generic method signature
    ///
    /// [Format](https://docs.oracle.com/javase/specs/jvms/se11/html/jvms-4.html#jvms-4.7.9.1)
    pub generic_signature: Option<String>,

    /// Constant field value
    pub constant_value: Option<ConstantData<'g>>,
}
