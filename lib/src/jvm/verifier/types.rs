use crate::jvm::class_file::{
    BytecodeIndex, ClassConstantIndex, ConstantPoolOverflow, ConstantsPool, ConstantsWriter,
    Serialize,
};
use crate::jvm::class_graph::{Assignable, ClassId};
use crate::jvm::code::SynLabel;
use crate::jvm::{BaseType, FieldType, RefType};
use crate::util::{Offset, Width};
use byteorder::WriteBytesExt;
use std::collections::HashMap;

/// Type used during verification inside stack map frames.
///
/// These types are as defined in [this hierarchy][0].
///
/// [0]: https://docs.oracle.com/javase/specs/jvms/se17/html/jvms-4.html#jvms-4.10.1.2
#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
pub enum VerificationType<Cls, Uninit> {
    /// Verification type for primitive types `int`, `byte`, `char`, `short`, and `boolean`
    Integer,

    /// Verification type for primitive type `float`
    Float,

    /// Verification type for primitive type `double`
    Double,

    /// Verification type for primitive type `long`
    Long,

    /// Null reference type, with exactly one value: `null`. This is a special case so that `null`
    /// can be assigned to any initialized reference type.
    Null,

    /// Initialized reference type
    Object(Cls),

    /// Uninitialized reference type corresponding to `this` in the body of a constructor, before
    /// `<init>` is called. As soon as `<init>` is called, all occurrences of this turn into
    /// initialized reference types.
    UninitializedThis,

    /// Uninitialized reference types, introduced by `new` and eliminated by `<init>`.
    ///
    /// During the initial bytecode construction, this uses [`UninitializedRefType`] for tracking
    /// both the type of the unintiialized object (so we can quickly figure out which type to
    /// replace it with after `<init>` is called), the offset of the `new` instruction in that
    /// basic block, and the basic block label.
    ///
    /// When serializing into a classfile, this gets flattened into a single `u16` corresponding to
    /// the offset of the `new` instruction from the start of the method body.
    Uninitialized(Uninit),
}

impl<Cls, Uninit> VerificationType<Cls, Uninit> {
    /// Is this type is a reference type?
    pub fn is_reference(&self) -> bool {
        match self {
            VerificationType::Integer
            | VerificationType::Float
            | VerificationType::Double
            | VerificationType::Long => false,

            VerificationType::Null
            | VerificationType::UninitializedThis
            | VerificationType::Object(_)
            | VerificationType::Uninitialized(_) => true,
        }
    }
}

impl<C, U> From<FieldType<C>> for VerificationType<RefType<C>, U> {
    fn from(field_type: FieldType<C>) -> Self {
        match field_type {
            FieldType::Base(BaseType::Int)
            | FieldType::Base(BaseType::Char)
            | FieldType::Base(BaseType::Short)
            | FieldType::Base(BaseType::Byte)
            | FieldType::Base(BaseType::Boolean) => VerificationType::Integer,
            FieldType::Base(BaseType::Float) => VerificationType::Float,
            FieldType::Base(BaseType::Long) => VerificationType::Long,
            FieldType::Base(BaseType::Double) => VerificationType::Double,
            FieldType::Ref(ref_type) => VerificationType::Object(ref_type),
        }
    }
}

impl Serialize for VerificationType<ClassConstantIndex, BytecodeIndex> {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> std::io::Result<()> {
        match self {
            VerificationType::Integer => 1u8.serialize(writer)?,
            VerificationType::Float => 2u8.serialize(writer)?,
            VerificationType::Double => 3u8.serialize(writer)?,
            VerificationType::Long => 4u8.serialize(writer)?,
            VerificationType::Null => 5u8.serialize(writer)?,
            VerificationType::UninitializedThis => 6u8.serialize(writer)?,
            VerificationType::Object(cls) => {
                7u8.serialize(writer)?;
                cls.serialize(writer)?;
            }
            VerificationType::Uninitialized(off) => {
                8u8.serialize(writer)?;
                off.serialize(writer)?;
            }
        };
        Ok(())
    }
}

impl<Cls, A> Width for VerificationType<Cls, A> {
    fn width(&self) -> usize {
        match self {
            VerificationType::Double | VerificationType::Long => 2,
            _ => 1,
        }
    }
}

impl<'g, U> Assignable for VerificationType<RefType<ClassId<'g>>, U> {
    /// Check if one verification type is assignable to another
    ///
    /// TODO: there is no handling of uninitialized yet. This just means that we might get false
    /// verification failures.
    fn is_assignable(&self, super_type: &Self) -> bool {
        match (self, super_type) {
            (Self::Integer, Self::Integer) => true,
            (Self::Float, Self::Float) => true,
            (Self::Long, Self::Long) => true,
            (Self::Double, Self::Double) => true,
            (Self::Null, Self::Null) => true,
            (Self::Null, Self::Object(_)) => true,
            (Self::Object(t1), Self::Object(t2)) => t1.is_assignable(t2),
            _ => false,
        }
    }
}

impl<'g> VerificationType<RefType<ClassId<'g>>, UninitializedRefType<'g>> {
    /// Resolve the type into its serializable form
    pub fn into_serializable(
        &self,
        constants_pool: &mut ConstantsPool<'g>,
        block_offsets: &HashMap<SynLabel, Offset>,
    ) -> Result<VerificationType<ClassConstantIndex, BytecodeIndex>, ConstantPoolOverflow> {
        match self {
            VerificationType::Integer => Ok(VerificationType::Integer),
            VerificationType::Float => Ok(VerificationType::Float),
            VerificationType::Long => Ok(VerificationType::Long),
            VerificationType::Double => Ok(VerificationType::Double),
            VerificationType::Null => Ok(VerificationType::Null),
            VerificationType::UninitializedThis => Ok(VerificationType::UninitializedThis),
            VerificationType::Object(ref_type) => {
                let class_index = ref_type.constant_index(constants_pool)?;
                Ok(VerificationType::Object(class_index))
            }
            VerificationType::Uninitialized(uninitialized_ref_type) => {
                let absolute_block_offset = block_offsets[&uninitialized_ref_type.block];
                let offset_in_block = uninitialized_ref_type.offset_in_block.0;
                let absolute_offset = absolute_block_offset.0 + offset_in_block;
                Ok(VerificationType::Uninitialized(BytecodeIndex(
                    absolute_offset as u16,
                )))
            }
        }
    }
}

impl<C, U> VerificationType<C, U> {
    pub fn map<C2, U2>(
        &self,
        map_class: impl Fn(&C) -> C2,
        map_uninitialized: impl Fn(&U) -> U2,
    ) -> VerificationType<C2, U2> {
        match self {
            VerificationType::Integer => VerificationType::Integer,
            VerificationType::Float => VerificationType::Float,
            VerificationType::Long => VerificationType::Long,
            VerificationType::Double => VerificationType::Double,
            VerificationType::Null => VerificationType::Null,
            VerificationType::UninitializedThis => VerificationType::UninitializedThis,
            VerificationType::Object(cls) => VerificationType::Object(map_class(cls)),
            VerificationType::Uninitialized(uninit) => {
                VerificationType::Uninitialized(map_uninitialized(uninit))
            }
        }
    }
}

/// Uninitialized reference type during verification
///
/// During code generation, after a `new` instruction, the top of the stack will contain an
/// uninitialized value. Although ultimately the stack map frame will contain only an absolute
/// offset into the code array where that `new` instruction is located, that's not something that
/// is convenient to produce or query while producing code.
///
///   - we don't yet know what the offset of the `new` instruction will really be (it could even
///     wiggle around a bit thanks to needing to widen some jumps)
///
///   - we want to store information about the type that will be there _once_ it is initialized
///     (eg. so we can effectively verify the `<init>` call)
///
#[derive(PartialEq, Eq, Clone, Debug, Copy)]
pub struct UninitializedRefType<'g> {
    /// Once the type is initialized, what will it be?
    pub verification_type: RefType<ClassId<'g>>,

    /// Offset of the `new` instruction from the start of the basic block containing it
    pub offset_in_block: Offset,

    /// Label of the basic block containing the `new` instruction
    pub block: SynLabel,
}
