//! This module contains the AST of JVM bytecode. The representations is slightly different from
//! the usual presentation to make it more convenient to construct bytecode. For instance:
//!
//!   - The "wide" instruction doesn't show up at all, but instead gets merged into the
//!     instructions it is allowed to modify
//!
//!   - Some instructions (like the branches) get abstracted into one instruction with a field.
//!     This helps with repetitive pattern matches and also simplifies tasks like inverting a
//!     branch condition.
//!
//!   - Some instructions (like `jsr`) are just omitted. We just don't need them since we never
//!     emit them
//!

use super::*;
use byteorder::WriteBytesExt;
use std::convert::TryFrom;
use std::io::Result;
use std::ops::Not;
use crate::jvm::class_file::Serialize;
use crate::util::{Width};

/// Non-branching JVM bytecode instruction
#[derive(Clone, Debug)]
pub enum Instruction<ClassHint, Class, Constant, Field, Method, IndyMethod> {
    Nop,
    AConstNull,
    IConstM1,
    IConst0,
    IConst1,
    IConst2,
    IConst3,
    IConst4,
    IConst5,
    LConst0,
    LConst1,
    FConst0,
    FConst1,
    FConst2,
    DConst0,
    DConst1,
    BiPush(i8),
    SiPush(i16),
    Ldc(Constant), // covers both `ldc` and `ldc_w`
    Ldc2(Constant),
    ILoad(u16), // covers `iload`, `iload{0,3}`, and `wide iload`
    LLoad(u16),
    FLoad(u16),
    DLoad(u16),
    ALoad(u16),
    IALoad,
    LALoad,
    FALoad,
    DALoad,
    AALoad,
    BALoad,
    CALoad,
    SALoad,
    IStore(u16), // covers `istore`, `istore{0,3}`, and `wide istore`
    LStore(u16),
    FStore(u16),
    DStore(u16),
    AStore(u16),
    IKill(u16), // imaginary instruction to signal a local is no longer to be used
    LKill(u16),
    FKill(u16),
    DKill(u16),
    AKill(u16),
    AHint(ClassHint), // hint for the verifier to infer a more general type
    IAStore,
    LAStore,
    FAStore,
    DAStore,
    AAStore,
    BAStore,
    CAStore,
    SAStore,
    Pop,
    Pop2,
    Dup,
    DupX1,
    DupX2,
    Dup2,
    Dup2X1,
    Dup2X2,
    Swap,
    IAdd,
    LAdd,
    FAdd,
    DAdd,
    ISub,
    LSub,
    FSub,
    DSub,
    IMul,
    LMul,
    FMul,
    DMul,
    IDiv,
    LDiv,
    FDiv,
    DDiv,
    IRem,
    LRem,
    FRem,
    DRem,
    INeg,
    LNeg,
    FNeg,
    DNeg,
    ISh(ShiftType), // covers `ishr`, `ishl`, and `iushr`
    LSh(ShiftType), // covers `lshr`, `lshl`, and `lushr`
    IAnd,
    LAnd,
    IOr,
    LOr,
    IXor,
    LXor,
    IInc(u16, i16), // covers `iinc` and `wide iinc`
    I2L,
    I2F,
    I2D,
    L2I,
    L2F,
    L2D,
    F2I,
    F2L,
    F2D,
    D2I,
    D2L,
    D2F,
    I2B,
    I2C,
    I2S,
    LCmp,
    FCmp(CompareMode), // covers `fcmpl` and `fcmpg`
    DCmp(CompareMode), // covers `dcmpl` and `dcmpg`
    GetStatic(Field),
    PutStatic(Field),
    GetField(Field),
    PutField(Field),
    Invoke(InvokeType, Method),
    InvokeDynamic(IndyMethod),
    New(Class),
    NewArray(BaseType),
    ANewArray(Class),
    ArrayLength,
    CheckCast(Class),
    InstanceOf(Class),
}

pub type SerializableInstruction = Instruction<
    (),
    ClassConstantIndex,
    ConstantIndex,
    FieldRefConstantIndex,
    MethodRefConstantIndex,
    InvokeDynamicConstantIndex,
>;

impl<ClassHint, Class, Constant, Field, Method, IndyMethod>
    Instruction<ClassHint, Class, Constant, Field, Method, IndyMethod>
{
    pub fn map<ClassHint2, Class2, Constant2, Field2, Method2, IndyMethod2, E>(
        &self,
        map_class_hint: impl Fn(&ClassHint) -> std::result::Result<ClassHint2, E>,
        map_class: impl Fn(&Class) -> std::result::Result<Class2, E>,
        map_constant: impl Fn(&Constant) -> std::result::Result<Constant2, E>,
        map_field: impl Fn(&Field) -> std::result::Result<Field2, E>,
        map_method: impl Fn(&Method) -> std::result::Result<Method2, E>,
        map_indy_method: impl Fn(&IndyMethod) -> std::result::Result<IndyMethod2, E>,
    ) -> std::result::Result<
        Instruction<ClassHint2, Class2, Constant2, Field2, Method2, IndyMethod2>,
        E,
    > {
        use Instruction::*;
        Ok(match self {
            Nop => Nop,
            AConstNull => AConstNull,
            IConstM1 => IConstM1,
            IConst0 => IConst0,
            IConst1 => IConst1,
            IConst2 => IConst2,
            IConst3 => IConst3,
            IConst4 => IConst4,
            IConst5 => IConst5,
            LConst0 => LConst0,
            LConst1 => LConst1,
            FConst0 => FConst0,
            FConst1 => FConst1,
            FConst2 => FConst2,
            DConst0 => DConst0,
            DConst1 => DConst1,
            BiPush(b) => BiPush(*b),
            SiPush(s) => SiPush(*s),
            Ldc(constant) => Ldc(map_constant(constant)?),
            Ldc2(constant) => Ldc2(map_constant(constant)?),
            ILoad(idx) => ILoad(*idx),
            LLoad(idx) => LLoad(*idx),
            FLoad(idx) => FLoad(*idx),
            DLoad(idx) => DLoad(*idx),
            ALoad(idx) => ALoad(*idx),
            IALoad => IALoad,
            LALoad => LALoad,
            FALoad => FALoad,
            DALoad => DALoad,
            AALoad => AALoad,
            BALoad => BALoad,
            CALoad => CALoad,
            SALoad => SALoad,
            IStore(idx) => IStore(*idx),
            LStore(idx) => LStore(*idx),
            FStore(idx) => FStore(*idx),
            DStore(idx) => DStore(*idx),
            AStore(idx) => AStore(*idx),
            IKill(idx) => IKill(*idx),
            LKill(idx) => LKill(*idx),
            FKill(idx) => FKill(*idx),
            DKill(idx) => FKill(*idx),
            AKill(idx) => AKill(*idx),
            AHint(hint) => AHint(map_class_hint(hint)?),
            IAStore => IAStore,
            LAStore => LAStore,
            FAStore => FAStore,
            DAStore => DAStore,
            AAStore => AAStore,
            BAStore => BAStore,
            CAStore => CAStore,
            SAStore => SAStore,
            Pop => Pop,
            Pop2 => Pop2,
            Dup => Dup,
            DupX1 => DupX1,
            DupX2 => DupX2,
            Dup2 => Dup2,
            Dup2X1 => Dup2X1,
            Dup2X2 => Dup2X2,
            Swap => Swap,
            IAdd => IAdd,
            LAdd => LAdd,
            FAdd => FAdd,
            DAdd => DAdd,
            ISub => ISub,
            LSub => LSub,
            FSub => FSub,
            DSub => DSub,
            IMul => IMul,
            LMul => LMul,
            FMul => FMul,
            DMul => DMul,
            IDiv => IDiv,
            LDiv => LDiv,
            FDiv => FDiv,
            DDiv => DDiv,
            IRem => IRem,
            LRem => LRem,
            FRem => FRem,
            DRem => DRem,
            INeg => INeg,
            LNeg => LNeg,
            FNeg => FNeg,
            DNeg => DNeg,
            ISh(s) => ISh(*s),
            LSh(s) => LSh(*s),
            IAnd => IAnd,
            LAnd => LAnd,
            IOr => IOr,
            LOr => LOr,
            IXor => IXor,
            LXor => LXor,
            IInc(idx, by) => IInc(*idx, *by),
            I2L => I2L,
            I2F => I2F,
            I2D => I2D,
            L2I => L2I,
            L2F => L2F,
            L2D => L2D,
            F2I => F2I,
            F2L => F2L,
            F2D => F2D,
            D2I => D2I,
            D2L => D2L,
            D2F => D2F,
            I2B => I2B,
            I2C => I2C,
            I2S => I2S,
            LCmp => LCmp,
            FCmp(m) => FCmp(*m),
            DCmp(m) => DCmp(*m),
            GetStatic(field) => GetStatic(map_field(field)?),
            PutStatic(field) => PutStatic(map_field(field)?),
            GetField(field) => GetField(map_field(field)?),
            PutField(field) => PutField(map_field(field)?),
            Invoke(typ, method) => Invoke(*typ, map_method(method)?),
            InvokeDynamic(indy_method) => InvokeDynamic(map_indy_method(indy_method)?),
            New(class) => New(map_class(class)?),
            NewArray(bt) => NewArray(*bt),
            ANewArray(class) => ANewArray(map_class(class)?),
            ArrayLength => ArrayLength,
            CheckCast(class) => CheckCast(map_class(class)?),
            InstanceOf(class) => InstanceOf(map_class(class)?),
        })
    }
}

impl<ClassHint, Class, Field, Method, IndyMethod> Width
    for Instruction<ClassHint, Class, ConstantIndex, Field, Method, IndyMethod>
{
    fn width(&self) -> usize {
        match self {
          Instruction::IKill(_)
          | Instruction::LKill(_)
          | Instruction::FKill(_)
          | Instruction::DKill(_)
          | Instruction::AKill(_)
          | Instruction::AHint(_)
          => 0,

          Instruction::Nop
          | Instruction::AConstNull
          | Instruction::IConstM1
          | Instruction::IConst0
          | Instruction::IConst1
          | Instruction::IConst2
          | Instruction::IConst3
          | Instruction::IConst4
          | Instruction::IConst5
          | Instruction::LConst0
          | Instruction::LConst1
          | Instruction::FConst0
          | Instruction::FConst1
          | Instruction::FConst2
          | Instruction::DConst0
          | Instruction::DConst1
          | Instruction::ILoad(0..=3)
          | Instruction::LLoad(0..=3)
          | Instruction::FLoad(0..=3)
          | Instruction::DLoad(0..=3)
          | Instruction::ALoad(0..=3)
          | Instruction::IALoad
          | Instruction::LALoad
          | Instruction::FALoad
          | Instruction::DALoad
          | Instruction::AALoad
          | Instruction::BALoad
          | Instruction::CALoad
          | Instruction::SALoad
          | Instruction::IStore(0..=3)
          | Instruction::LStore(0..=3)
          | Instruction::FStore(0..=3)
          | Instruction::DStore(0..=3)
          | Instruction::AStore(0..=3)
          | Instruction::IAStore
          | Instruction::LAStore
          | Instruction::FAStore
          | Instruction::DAStore
          | Instruction::AAStore
          | Instruction::BAStore
          | Instruction::CAStore
          | Instruction::SAStore
          | Instruction::Pop
          | Instruction::Pop2
          | Instruction::Dup
          | Instruction::DupX1
          | Instruction::DupX2
          | Instruction::Dup2
          | Instruction::Dup2X1
          | Instruction::Dup2X2
          | Instruction::Swap
          | Instruction::IAdd
          | Instruction::LAdd
          | Instruction::FAdd
          | Instruction::DAdd
          | Instruction::ISub
          | Instruction::LSub
          | Instruction::FSub
          | Instruction::DSub
          | Instruction::IMul
          | Instruction::LMul
          | Instruction::FMul
          | Instruction::DMul
          | Instruction::IDiv
          | Instruction::LDiv
          | Instruction::FDiv
          | Instruction::DDiv
          | Instruction::IRem
          | Instruction::LRem
          | Instruction::FRem
          | Instruction::DRem
          | Instruction::INeg
          | Instruction::LNeg
          | Instruction::FNeg
          | Instruction::DNeg
          | Instruction::ISh(_)
          | Instruction::LSh(_)
          | Instruction::IAnd
          | Instruction::LAnd
          | Instruction::IOr
          | Instruction::LOr
          | Instruction::IXor
          | Instruction::LXor
          | Instruction::I2L
          | Instruction::I2F
          | Instruction::I2D
          | Instruction::L2I
          | Instruction::L2F
          | Instruction::L2D
          | Instruction::F2I
          | Instruction::F2L
          | Instruction::F2D
          | Instruction::D2I
          | Instruction::D2L
          | Instruction::D2F
          | Instruction::I2B
          | Instruction::I2C
          | Instruction::I2S
          | Instruction::LCmp
          | Instruction::FCmp(_)
          | Instruction::DCmp(_)
          | Instruction::ArrayLength
          => 1,

          Instruction::BiPush(_)
          | Instruction::ILoad(4..=255)
          | Instruction::LLoad(4..=255)
          | Instruction::FLoad(4..=255)
          | Instruction::DLoad(4..=255)
          | Instruction::ALoad(4..=255)
          | Instruction::IStore(4..=255)
          | Instruction::LStore(4..=255)
          | Instruction::FStore(4..=255)
          | Instruction::DStore(4..=255)
          | Instruction::AStore(4..=255)
          | Instruction::Ldc(ConstantIndex(0..=255))
          | Instruction::NewArray(_)
          => 2,

          Instruction::SiPush(_)
          | Instruction::Ldc(_)
          | Instruction::Ldc2(_) // always wide, unlike `ldc` vs. `ldc_w`
          | Instruction::IInc(0..=255, -128..=127)
          | Instruction::GetStatic(_)
          | Instruction::PutStatic(_)
          | Instruction::GetField(_)
          | Instruction::PutField(_)
          | Instruction::Invoke(InvokeType::Special, _)
          | Instruction::Invoke(InvokeType::Static, _)
          | Instruction::Invoke(InvokeType::Virtual, _)
          | Instruction::New(_)
          | Instruction::ANewArray(_)
          | Instruction::CheckCast(_)
          | Instruction::InstanceOf(_)
          => 3,

          Instruction::ILoad(_)
          | Instruction::LLoad(_)
          | Instruction::FLoad(_)
          | Instruction::DLoad(_)
          | Instruction::ALoad(_)
          | Instruction::IStore(_)
          | Instruction::LStore(_)
          | Instruction::FStore(_)
          | Instruction::DStore(_)
          | Instruction::AStore(_)
          => 4,

          Instruction::Invoke(InvokeType::Interface(_), _)
          | Instruction::InvokeDynamic(_)
          => 5,

          Instruction::IInc(_, _)
          => 6,
        }
    }
}

impl Serialize for SerializableInstruction {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> Result<()> {
        /* The load/store instructions follow the same pattern:
         *
         *   - short form (0-3) have special bytes
         *   - normal form (0-255) use `iload` plus a byte operand
         *   - wide form (255-65535) use `wide iload` plus two byte operands
         */
        fn serialize_load_or_store<W: WriteBytesExt>(
            idx: u16,
            short_form_start: u8,
            normal_form: u8,
            writer: &mut W,
        ) -> Result<()> {
            match u8::try_from(idx) {
                Ok(n @ 0..=3) => (short_form_start + n).serialize(writer),
                Ok(n) => {
                    normal_form.serialize(writer)?;
                    n.serialize(writer)
                }
                Err(_) => {
                    0xC4u8.serialize(writer)?;
                    normal_form.serialize(writer)?;
                    idx.serialize(writer)
                }
            }
        }

        match self {
            Instruction::Nop => 0x00u8.serialize(writer)?,
            Instruction::AConstNull => 0x01u8.serialize(writer)?,
            Instruction::IConstM1 => 0x02u8.serialize(writer)?,
            Instruction::IConst0 => 0x03u8.serialize(writer)?,
            Instruction::IConst1 => 0x04u8.serialize(writer)?,
            Instruction::IConst2 => 0x05u8.serialize(writer)?,
            Instruction::IConst3 => 0x06u8.serialize(writer)?,
            Instruction::IConst4 => 0x07u8.serialize(writer)?,
            Instruction::IConst5 => 0x08u8.serialize(writer)?,
            Instruction::LConst0 => 0x09u8.serialize(writer)?,
            Instruction::LConst1 => 0x0au8.serialize(writer)?,
            Instruction::FConst0 => 0x0bu8.serialize(writer)?,
            Instruction::FConst1 => 0x0cu8.serialize(writer)?,
            Instruction::FConst2 => 0x0du8.serialize(writer)?,
            Instruction::DConst0 => 0x0eu8.serialize(writer)?,
            Instruction::DConst1 => 0x0fu8.serialize(writer)?,
            Instruction::BiPush(b) => {
                u8::serialize(&0x10, writer)?;
                b.serialize(writer)?;
            }
            Instruction::SiPush(s) => {
                0x11u8.serialize(writer)?;
                s.serialize(writer)?;
            }
            Instruction::Ldc(ConstantIndex(idx)) => match u8::try_from(*idx) {
                Ok(b) => {
                    0x12u8.serialize(writer)?;
                    b.serialize(writer)?;
                }
                Err(_) => {
                    0x13u8.serialize(writer)?;
                    idx.serialize(writer)?;
                }
            },
            Instruction::Ldc2(ConstantIndex(idx)) => {
                0x14u8.serialize(writer)?;
                idx.serialize(writer)?;
            }
            Instruction::ILoad(idx) => serialize_load_or_store(*idx, 0x1A, 0x15, writer)?,
            Instruction::LLoad(idx) => serialize_load_or_store(*idx, 0x1E, 0x16, writer)?,
            Instruction::FLoad(idx) => serialize_load_or_store(*idx, 0x22, 0x17, writer)?,
            Instruction::DLoad(idx) => serialize_load_or_store(*idx, 0x26, 0x18, writer)?,
            Instruction::ALoad(idx) => serialize_load_or_store(*idx, 0x2A, 0x19, writer)?,
            Instruction::IALoad => 0x2eu8.serialize(writer)?,
            Instruction::LALoad => 0x2fu8.serialize(writer)?,
            Instruction::FALoad => 0x30u8.serialize(writer)?,
            Instruction::DALoad => 0x31u8.serialize(writer)?,
            Instruction::AALoad => 0x32u8.serialize(writer)?,
            Instruction::BALoad => 0x33u8.serialize(writer)?,
            Instruction::CALoad => 0x34u8.serialize(writer)?,
            Instruction::SALoad => 0x35u8.serialize(writer)?,
            Instruction::IStore(idx) => serialize_load_or_store(*idx, 0x3B, 0x36, writer)?,
            Instruction::LStore(idx) => serialize_load_or_store(*idx, 0x3F, 0x37, writer)?,
            Instruction::FStore(idx) => serialize_load_or_store(*idx, 0x43, 0x38, writer)?,
            Instruction::DStore(idx) => serialize_load_or_store(*idx, 0x47, 0x39, writer)?,
            Instruction::AStore(idx) => serialize_load_or_store(*idx, 0x4B, 0x3A, writer)?,
            Instruction::IKill(_)
            | Instruction::LKill(_)
            | Instruction::FKill(_)
            | Instruction::DKill(_)
            | Instruction::AKill(_)
            | Instruction::AHint(_) => (),
            Instruction::IAStore => 0x4fu8.serialize(writer)?,
            Instruction::LAStore => 0x50u8.serialize(writer)?,
            Instruction::FAStore => 0x51u8.serialize(writer)?,
            Instruction::DAStore => 0x52u8.serialize(writer)?,
            Instruction::AAStore => 0x53u8.serialize(writer)?,
            Instruction::BAStore => 0x54u8.serialize(writer)?,
            Instruction::CAStore => 0x55u8.serialize(writer)?,
            Instruction::SAStore => 0x56u8.serialize(writer)?,
            Instruction::Pop => 0x57u8.serialize(writer)?,
            Instruction::Pop2 => 0x58u8.serialize(writer)?,
            Instruction::Dup => 0x59u8.serialize(writer)?,
            Instruction::DupX1 => 0x5au8.serialize(writer)?,
            Instruction::DupX2 => 0x5bu8.serialize(writer)?,
            Instruction::Dup2 => 0x5cu8.serialize(writer)?,
            Instruction::Dup2X1 => 0x5du8.serialize(writer)?,
            Instruction::Dup2X2 => 0x5eu8.serialize(writer)?,
            Instruction::Swap => 0x5fu8.serialize(writer)?,
            Instruction::IAdd => 0x60u8.serialize(writer)?,
            Instruction::LAdd => 0x61u8.serialize(writer)?,
            Instruction::FAdd => 0x62u8.serialize(writer)?,
            Instruction::DAdd => 0x63u8.serialize(writer)?,
            Instruction::ISub => 0x64u8.serialize(writer)?,
            Instruction::LSub => 0x65u8.serialize(writer)?,
            Instruction::FSub => 0x66u8.serialize(writer)?,
            Instruction::DSub => 0x67u8.serialize(writer)?,
            Instruction::IMul => 0x68u8.serialize(writer)?,
            Instruction::LMul => 0x69u8.serialize(writer)?,
            Instruction::FMul => 0x6au8.serialize(writer)?,
            Instruction::DMul => 0x6bu8.serialize(writer)?,
            Instruction::IDiv => 0x6cu8.serialize(writer)?,
            Instruction::LDiv => 0x6du8.serialize(writer)?,
            Instruction::FDiv => 0x6eu8.serialize(writer)?,
            Instruction::DDiv => 0x6fu8.serialize(writer)?,
            Instruction::IRem => 0x70u8.serialize(writer)?,
            Instruction::LRem => 0x71u8.serialize(writer)?,
            Instruction::FRem => 0x72u8.serialize(writer)?,
            Instruction::DRem => 0x73u8.serialize(writer)?,
            Instruction::INeg => 0x74u8.serialize(writer)?,
            Instruction::LNeg => 0x75u8.serialize(writer)?,
            Instruction::FNeg => 0x76u8.serialize(writer)?,
            Instruction::DNeg => 0x77u8.serialize(writer)?,
            Instruction::ISh(ShiftType::Left) => 0x78u8.serialize(writer)?,
            Instruction::LSh(ShiftType::Left) => 0x79u8.serialize(writer)?,
            Instruction::ISh(ShiftType::ArithmeticRight) => 0x7au8.serialize(writer)?,
            Instruction::LSh(ShiftType::ArithmeticRight) => 0x7bu8.serialize(writer)?,
            Instruction::ISh(ShiftType::LogicalRight) => 0x7cu8.serialize(writer)?,
            Instruction::LSh(ShiftType::LogicalRight) => 0x7du8.serialize(writer)?,
            Instruction::IAnd => 0x7eu8.serialize(writer)?,
            Instruction::LAnd => 0x7fu8.serialize(writer)?,
            Instruction::IOr => 0x80u8.serialize(writer)?,
            Instruction::LOr => 0x81u8.serialize(writer)?,
            Instruction::IXor => 0x82u8.serialize(writer)?,
            Instruction::LXor => 0x83u8.serialize(writer)?,
            Instruction::IInc(idx, diff) => match (u8::try_from(*idx), i8::try_from(*diff)) {
                (Ok(b), Ok(d)) => {
                    0x84u8.serialize(writer)?;
                    b.serialize(writer)?;
                    d.serialize(writer)?;
                }
                _ => {
                    0xc4u8.serialize(writer)?;
                    0x84u8.serialize(writer)?;
                    idx.serialize(writer)?;
                    diff.serialize(writer)?;
                }
            },
            Instruction::I2L => 0x85u8.serialize(writer)?,
            Instruction::I2F => 0x86u8.serialize(writer)?,
            Instruction::I2D => 0x87u8.serialize(writer)?,
            Instruction::L2I => 0x88u8.serialize(writer)?,
            Instruction::L2F => 0x89u8.serialize(writer)?,
            Instruction::L2D => 0x8au8.serialize(writer)?,
            Instruction::F2I => 0x8bu8.serialize(writer)?,
            Instruction::F2L => 0x8cu8.serialize(writer)?,
            Instruction::F2D => 0x8du8.serialize(writer)?,
            Instruction::D2I => 0x8eu8.serialize(writer)?,
            Instruction::D2L => 0x8fu8.serialize(writer)?,
            Instruction::D2F => 0x90u8.serialize(writer)?,
            Instruction::I2B => 0x91u8.serialize(writer)?,
            Instruction::I2C => 0x92u8.serialize(writer)?,
            Instruction::I2S => 0x93u8.serialize(writer)?,
            Instruction::LCmp => 0x94u8.serialize(writer)?,
            Instruction::FCmp(CompareMode::L) => 0x95u8.serialize(writer)?,
            Instruction::FCmp(CompareMode::G) => 0x96u8.serialize(writer)?,
            Instruction::DCmp(CompareMode::L) => 0x97u8.serialize(writer)?,
            Instruction::DCmp(CompareMode::G) => 0x98u8.serialize(writer)?,
            Instruction::GetStatic(idx) => {
                0xb2u8.serialize(writer)?;
                idx.serialize(writer)?;
            }
            Instruction::PutStatic(idx) => {
                0xb3u8.serialize(writer)?;
                idx.serialize(writer)?;
            }
            Instruction::GetField(idx) => {
                0xb4u8.serialize(writer)?;
                idx.serialize(writer)?;
            }
            Instruction::PutField(idx) => {
                0xb5u8.serialize(writer)?;
                idx.serialize(writer)?;
            }
            Instruction::Invoke(InvokeType::Virtual, idx) => {
                0xb6u8.serialize(writer)?;
                idx.serialize(writer)?;
            }
            Instruction::Invoke(InvokeType::Special, idx) => {
                0xb7u8.serialize(writer)?;
                idx.serialize(writer)?;
            }
            Instruction::Invoke(InvokeType::Static, idx) => {
                0xb8u8.serialize(writer)?;
                idx.serialize(writer)?;
            }
            Instruction::Invoke(InvokeType::Interface(cnt), idx) => {
                0xb9u8.serialize(writer)?;
                idx.serialize(writer)?;
                cnt.serialize(writer)?;
                0u8.serialize(writer)?;
            }
            Instruction::InvokeDynamic(idx) => {
                0xbau8.serialize(writer)?;
                idx.serialize(writer)?;
                0u16.serialize(writer)?;
            }
            Instruction::New(idx) => {
                0xbbu8.serialize(writer)?;
                idx.serialize(writer)?;
            }
            Instruction::NewArray(basetype) => {
                let atype: u8 = match basetype {
                    BaseType::Boolean => 4,
                    BaseType::Char => 5,
                    BaseType::Float => 6,
                    BaseType::Double => 7,
                    BaseType::Byte => 8,
                    BaseType::Short => 9,
                    BaseType::Int => 10,
                    BaseType::Long => 11,
                };
                0xbcu8.serialize(writer)?;
                atype.serialize(writer)?;
            }
            Instruction::ANewArray(idx) => {
                0xbdu8.serialize(writer)?;
                idx.serialize(writer)?;
            }
            Instruction::ArrayLength => 0xbeu8.serialize(writer)?,
            Instruction::CheckCast(idx) => {
                0xc0u8.serialize(writer)?;
                idx.serialize(writer)?;
            }
            Instruction::InstanceOf(idx) => {
                0xc1u8.serialize(writer)?;
                idx.serialize(writer)?;
            }
        }
        Ok(())
    }
}

/// Branching JVM bytecode instruction
///
/// The type parameters let us abstract over the representation of
///
///   * __regular relative jump targets__: used in almost all branch instructions
///   * __wide relative jump targets__: used only in `goto_w`
///   * __fallthough targets__: used in all instructions that fall through
///
/// Shortly before the final serialization step, regular jump targets will become signed 16-bit
/// offsets into the code array, wide jump targets will become signed 32-bit offsets into the code
/// array, and fallthrough targets will be replaced with unit (since they are implicit from the
/// order of the blocks in the code array).
#[derive(Clone, Debug)]
pub enum BranchInstruction<Lbl, LblWide, LblNext> {
    If(OrdComparison, Lbl, LblNext), // covers `ifeq`, `ifne`, `iflt`, `ifge`, `ifgt`, `ifle`
    IfICmp(OrdComparison, Lbl, LblNext), // covers `if_icmpeq`, `if_icmpne`, `if_icmplt`, ... `if_icmple`
    IfACmp(EqComparison, Lbl, LblNext),  // covers `if_acmpeq`, `if_acmpne`
    Goto(Lbl),
    GotoW(LblWide),
    TableSwitch {
        /// `default` must be at a multiple of four bytes from the start of the current method, so
        /// there must be a 0-3 inclusive byte padding
        padding: u8,

        /// Jump target if the argument is less than `low` or greater than
        /// `low + targets.len()`
        default: LblWide,

        /// Value associated with the first jump target
        low: i32,

        /// Jump targets
        targets: Vec<LblWide>,
    },
    LookupSwitch {
        /// `default` must be at a multiple of four bytes from the start of the current method, so
        /// there must be a 0-3 inclusive byte padding
        padding: u8,

        /// Jump target if there is no corresponding key
        default: LblWide,

        /// Jump targets (sorted so that the keys are ascending)
        targets: Vec<(i32, LblWide)>,
    },
    IReturn,
    LReturn,
    FReturn,
    DReturn,
    AReturn,
    Return,
    AThrow,
    IfNull(EqComparison, Lbl, LblNext), // covers `ifnull`, `ifnonnull`

    /// This is a synthetic marker used to explicitly end a block which just falls through to the
    /// next block. In the JVM, this is implicit when a block ends without a jump. Making it
    /// explicit allows us to enforce that all blocks end in a branch instruction.
    FallThrough(LblNext),
}

impl<Lbl: Copy, LblWide: Copy, LblNext: Copy> BranchInstruction<Lbl, LblWide, LblNext> {
    /// If the instruction can fall through to the next block, get that next block
    pub fn fallthrough_target(&self) -> Option<LblNext> {
        match self {
            BranchInstruction::Goto(_)
            | BranchInstruction::GotoW(_)
            | BranchInstruction::TableSwitch { .. }
            | BranchInstruction::LookupSwitch { .. }
            | BranchInstruction::IReturn
            | BranchInstruction::LReturn
            | BranchInstruction::FReturn
            | BranchInstruction::DReturn
            | BranchInstruction::AReturn
            | BranchInstruction::Return
            | BranchInstruction::AThrow => None,

            BranchInstruction::If(_, _, lbl)
            | BranchInstruction::IfICmp(_, _, lbl)
            | BranchInstruction::IfACmp(_, _, lbl)
            | BranchInstruction::IfNull(_, _, lbl)
            | BranchInstruction::FallThrough(lbl) => Some(*lbl),
        }
    }

    /// If the instruction can jump to another block (non-fallthrough), get that block
    pub fn jump_targets(&self) -> JumpTargets<Lbl, LblWide> {
        match self {
            BranchInstruction::If(_, lbl, _) => JumpTargets::Regular(*lbl),
            BranchInstruction::IfICmp(_, lbl, _) => JumpTargets::Regular(*lbl),
            BranchInstruction::IfACmp(_, lbl, _) => JumpTargets::Regular(*lbl),
            BranchInstruction::Goto(lbl) => JumpTargets::Regular(*lbl),
            BranchInstruction::GotoW(lbl_w) => JumpTargets::Wide(*lbl_w),
            BranchInstruction::TableSwitch {
                default, targets, ..
            } => {
                let mut ts = vec![*default];
                ts.extend(targets.iter().copied());
                JumpTargets::WideMany(ts)
            }
            BranchInstruction::LookupSwitch {
                default, targets, ..
            } => {
                let mut ts = vec![*default];
                ts.extend(targets.iter().map(|(_, target)| *target));
                JumpTargets::WideMany(ts)
            }
            BranchInstruction::IReturn => JumpTargets::None,
            BranchInstruction::LReturn => JumpTargets::None,
            BranchInstruction::FReturn => JumpTargets::None,
            BranchInstruction::DReturn => JumpTargets::None,
            BranchInstruction::AReturn => JumpTargets::None,
            BranchInstruction::Return => JumpTargets::None,
            BranchInstruction::AThrow => JumpTargets::None,
            BranchInstruction::IfNull(_, lbl, _) => JumpTargets::Regular(*lbl),
            BranchInstruction::FallThrough(_) => JumpTargets::None,
        }
    }

    pub fn map_labels<Lbl2, LblWide2, LblNext2>(
        &self,
        map_label: impl FnOnce(&Lbl) -> Lbl2,
        map_wide_label: impl Fn(&LblWide) -> LblWide2,
        map_next_label: impl FnOnce(&LblNext) -> LblNext2,
    ) -> BranchInstruction<Lbl2, LblWide2, LblNext2> {
        use BranchInstruction::*;

        match self {
            If(op, lbl, next) => If(*op, map_label(lbl), map_next_label(next)),
            IfICmp(op, lbl, next) => IfICmp(*op, map_label(lbl), map_next_label(next)),
            IfACmp(op, lbl, next) => IfACmp(*op, map_label(lbl), map_next_label(next)),
            Goto(lbl) => Goto(map_label(lbl)),
            GotoW(wide) => GotoW(map_wide_label(wide)),
            TableSwitch {
                padding,
                default,
                low,
                targets,
            } => TableSwitch {
                padding: *padding,
                default: map_wide_label(default),
                low: *low,
                targets: targets.iter().map(map_wide_label).collect(),
            },
            LookupSwitch {
                padding,
                default,
                targets,
            } => LookupSwitch {
                padding: *padding,
                default: map_wide_label(default),
                targets: targets
                    .iter()
                    .map(|(key, lbl)| (*key, map_wide_label(lbl)))
                    .collect(),
            },
            IReturn => IReturn,
            LReturn => LReturn,
            FReturn => FReturn,
            DReturn => DReturn,
            AReturn => AReturn,
            Return => Return,
            AThrow => AThrow,
            IfNull(op, lbl, next) => IfNull(*op, map_label(lbl), map_next_label(next)),
            FallThrough(next) => FallThrough(map_next_label(next)),
        }
    }
}

impl<Lbl, LblWide, LblFall> Width for BranchInstruction<Lbl, LblWide, LblFall> {
    fn width(&self) -> usize {
        match self {
            BranchInstruction::FallThrough(_) => 0,

            BranchInstruction::IReturn
            | BranchInstruction::LReturn
            | BranchInstruction::FReturn
            | BranchInstruction::DReturn
            | BranchInstruction::AReturn
            | BranchInstruction::Return
            | BranchInstruction::AThrow => 1,

            BranchInstruction::Goto(_)
            | BranchInstruction::If(_, _, _)
            | BranchInstruction::IfICmp(_, _, _)
            | BranchInstruction::IfACmp(_, _, _)
            | BranchInstruction::IfNull(_, _, _) => 3,

            BranchInstruction::GotoW(_) => 5,

            BranchInstruction::TableSwitch {
                padding, targets, ..
            } => 1 + *padding as usize + 4 * (3 + targets.len()),

            BranchInstruction::LookupSwitch {
                padding, targets, ..
            } => 1 + *padding as usize + 8 * (1 + targets.len()),
        }
    }
}

impl Serialize for BranchInstruction<i16, i32, ()> {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> Result<()> {
        match self {
            BranchInstruction::If(comp, lbl, ()) => {
                let opcode: u8 = match comp {
                    OrdComparison::EQ => 0x99,
                    OrdComparison::NE => 0x9a,
                    OrdComparison::LT => 0x9b,
                    OrdComparison::GE => 0x9c,
                    OrdComparison::GT => 0x9d,
                    OrdComparison::LE => 0x9e,
                };
                opcode.serialize(writer)?;
                lbl.serialize(writer)?;
            }
            BranchInstruction::IfICmp(comp, lbl, ()) => {
                let opcode: u8 = match comp {
                    OrdComparison::EQ => 0x9f,
                    OrdComparison::NE => 0xa0,
                    OrdComparison::LT => 0xa1,
                    OrdComparison::GE => 0xa2,
                    OrdComparison::GT => 0xa3,
                    OrdComparison::LE => 0xa4,
                };
                opcode.serialize(writer)?;
                lbl.serialize(writer)?;
            }
            BranchInstruction::IfACmp(comp, lbl, ()) => {
                let opcode: u8 = match comp {
                    EqComparison::EQ => 0xa5,
                    EqComparison::NE => 0xa6,
                };
                opcode.serialize(writer)?;
                lbl.serialize(writer)?;
            }
            BranchInstruction::Goto(lbl) => {
                0xa7u8.serialize(writer)?;
                lbl.serialize(writer)?;
            }
            BranchInstruction::GotoW(lbl_ext) => {
                0xa8u8.serialize(writer)?;
                lbl_ext.serialize(writer)?;
            }
            BranchInstruction::TableSwitch {
                padding,
                default,
                low,
                targets,
            } => {
                0xaau8.serialize(writer)?;
                for _ in 0..*padding {
                    0x00u8.serialize(writer)?;
                }
                default.serialize(writer)?;
                low.serialize(writer)?;
                (low + targets.len() as i32 - 1).serialize(writer)?;
                for target in targets {
                    target.serialize(writer)?;
                }
            }
            BranchInstruction::LookupSwitch {
                padding,
                default,
                targets,
            } => {
                0xabu8.serialize(writer)?;
                for _ in 0..*padding {
                    0x00u8.serialize(writer)?;
                }
                default.serialize(writer)?;
                (targets.len() as i32).serialize(writer)?;
                for (key, target) in targets {
                    key.serialize(writer)?;
                    target.serialize(writer)?;
                }
            }
            BranchInstruction::IReturn => 0xacu8.serialize(writer)?,
            BranchInstruction::LReturn => 0xadu8.serialize(writer)?,
            BranchInstruction::FReturn => 0xaeu8.serialize(writer)?,
            BranchInstruction::DReturn => 0xafu8.serialize(writer)?,
            BranchInstruction::AReturn => 0xb0u8.serialize(writer)?,
            BranchInstruction::Return => 0xb1u8.serialize(writer)?,
            BranchInstruction::AThrow => 0xbfu8.serialize(writer)?,
            BranchInstruction::IfNull(comp, lbl, ()) => {
                let opcode: u8 = match comp {
                    EqComparison::EQ => 0xc6,
                    EqComparison::NE => 0xc7,
                };
                opcode.serialize(writer)?;
                lbl.serialize(writer)?;
            }
            BranchInstruction::FallThrough(()) => (),
        }
        Ok(())
    }
}

/// Non-fallthrough jump target of a `BranchInstruction`
pub enum JumpTargets<Lbl, LblWide> {
    None,
    Regular(Lbl),
    Wide(LblWide),
    WideMany(Vec<LblWide>),
}

impl<A> JumpTargets<A, A> {
    /// If all targets are the same type, extract them
    pub fn targets(&self) -> &[A] {
        match self {
            JumpTargets::None => &[],
            JumpTargets::Regular(a) => std::slice::from_ref(a),
            JumpTargets::Wide(a) => std::slice::from_ref(a),
            JumpTargets::WideMany(a_many) => &a_many,
        }
    }
}

/// Possible bit shifts
#[derive(Copy, Clone, Eq, PartialEq, Debug, Hash)]
pub enum ShiftType {
    Left,
    LogicalRight,
    ArithmeticRight,
}

/// Comparison modes for floating point
#[derive(Copy, Clone, Eq, PartialEq, Debug, Hash)]
pub enum CompareMode {
    /// -1 on NaN
    L,

    /// 1 on NaN
    G,
}

/// Binary comparison operators available for `int` branches
#[derive(Copy, Clone, Eq, PartialEq, Debug, Hash)]
pub enum OrdComparison {
    EQ,
    GE,
    GT,
    LE,
    LT,
    NE,
}

impl Not for OrdComparison {
    type Output = Self;

    fn not(self) -> Self::Output {
        match self {
            OrdComparison::EQ => OrdComparison::NE,
            OrdComparison::GE => OrdComparison::LT,
            OrdComparison::GT => OrdComparison::LE,
            OrdComparison::LE => OrdComparison::GT,
            OrdComparison::LT => OrdComparison::GE,
            OrdComparison::NE => OrdComparison::EQ,
        }
    }
}

/// Equality/inequality comparison operators
#[derive(Copy, Clone, Eq, PartialEq, Debug, Hash)]
pub enum EqComparison {
    EQ,
    NE,
}

impl Not for EqComparison {
    type Output = Self;

    fn not(self) -> Self::Output {
        match self {
            EqComparison::EQ => EqComparison::NE,
            EqComparison::NE => EqComparison::EQ,
        }
    }
}

/// Type of method to invoke
///
/// Note: `InvokeDynamic` is kept separate because the constant argument it expects is not to a
/// `Constant::MethodRef`.
#[derive(Copy, Clone, Eq, PartialEq, Debug, Hash)]
pub enum InvokeType {
    Virtual,
    Special,
    Static,
    Interface(u8), // `count` is of total arguments, where `long`/`double` count for 2
}
