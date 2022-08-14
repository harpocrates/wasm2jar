use std::borrow::Cow;
use std::fmt::{Debug, Error as FmtError, Formatter};

/// Names of methods, fields
///
/// See <https://docs.oracle.com/javase/specs/jvms/se16/html/jvms-4.html#jvms-4.2.2>
#[derive(Clone, Hash, Eq, PartialEq)]
pub struct UnqualifiedName(Cow<'static, str>);

/// Names of classes and interfaces
///
/// See <https://docs.oracle.com/javase/specs/jvms/se16/html/jvms-4.html#jvms-4.2.1>
#[derive(Clone, Hash, Eq, PartialEq)]
pub struct BinaryName(Cow<'static, str>);

/// Extracts the raw underlying string name
impl AsRef<str> for UnqualifiedName {
    fn as_ref(&self) -> &str {
        self.0.as_ref()
    }
}

/// Extracts the raw underlying string name
impl AsRef<str> for BinaryName {
    fn as_ref(&self) -> &str {
        self.0.as_ref()
    }
}

pub trait Name: Sized {
    /// Check if a string would be a valid unqualified name
    fn check_valid(name: impl AsRef<str>) -> Result<(), String>;

    /// Extact the raw underlying string data:
    fn as_cow(&self) -> &Cow<'static, str>;

    /// Extact the raw underlying string name
    fn as_str(&self) -> &str {
        self.as_cow().as_ref()
    }

    /// Try to construct a name from a string
    fn from_string(name: String) -> Result<Self, String>;
}

impl Name for UnqualifiedName {
    fn check_valid(name: impl AsRef<str>) -> Result<(), String> {
        let name = name.as_ref();
        if name.contains(&['.', ';', '[', '/'][..]) {
            Err(format!(
                "Unqualified name '{}' contains an illegal character",
                name
            ))
        } else if name.is_empty() {
            Err(format!("Unqualified name '{}' is empty", name))
        } else {
            Ok(())
        }
    }

    fn as_cow(&self) -> &Cow<'static, str> {
        &self.0
    }

    fn from_string(name: String) -> Result<Self, String> {
        match Self::check_valid(&name) {
            Ok(()) => Ok(UnqualifiedName(Cow::Owned(name))),
            Err(msg) => Err(msg),
        }
    }
}

impl Name for BinaryName {
    fn check_valid(name: impl AsRef<str>) -> Result<(), String> {
        let name = name.as_ref();
        if name.is_empty() {
            Err(format!("Binary name '{}' is empty", name))
        } else {
            name.split('/').map(UnqualifiedName::check_valid).collect()
        }
    }

    fn as_cow(&self) -> &Cow<'static, str> {
        &self.0
    }

    fn from_string(name: String) -> Result<Self, String> {
        match Self::check_valid(&name) {
            Ok(()) => Ok(BinaryName(Cow::Owned(name))),
            Err(msg) => Err(msg),
        }
    }
}

impl Debug for UnqualifiedName {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), FmtError> {
        f.write_str(self.0.as_ref())
    }
}
impl Debug for BinaryName {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), FmtError> {
        f.write_str(self.0.as_ref())
    }
}

impl From<UnqualifiedName> for BinaryName {
    fn from(name: UnqualifiedName) -> BinaryName {
        BinaryName(name.0)
    }
}

impl UnqualifiedName {
    /// Concatenate the contents of two unqualified names to produce a third
    pub fn concat(&self, other: &UnqualifiedName) -> UnqualifiedName {
        UnqualifiedName(Cow::Owned(format!("{}{}", self.as_str(), other.as_str())))
    }

    /// Construct an unqualified name that is just a number
    pub fn number(n: usize) -> UnqualifiedName {
        UnqualifiedName(Cow::Owned(n.to_string()))
    }

    const fn name(value: &'static str) -> UnqualifiedName {
        UnqualifiedName(Cow::Borrowed(value))
    }

    // JDK names
    pub const ABS: Self = Self::name("abs");
    pub const ADDEXACT: Self = Self::name("addExact");
    pub const ALLOCATE: Self = Self::name("allocate");
    pub const ALLOCATEDIRECT: Self = Self::name("allocateDirect");
    pub const ARRAYCONSTRUCTOR: Self = Self::name("arrayConstructor");
    pub const ARRAYCOPY: Self = Self::name("arraycopy");
    pub const ARRAYELEMENTGETTER: Self = Self::name("arrayElementGetter");
    pub const ARRAYELEMENTSETTER: Self = Self::name("arrayElementSetter");
    pub const ARRAYLENGTH: Self = Self::name("arrayLength");
    pub const ASTYPE: Self = Self::name("asType");
    pub const BIGENDIAN: Self = Self::name("BIG_ENDIAN");
    pub const BITCOUNT: Self = Self::name("bitCount");
    pub const BYTEVALUE: Self = Self::name("byteValue");
    pub const CAPACITY: Self = Self::name("capacity");
    pub const CEIL: Self = Self::name("ceil");
    pub const CHANGERETURNTYPE: Self = Self::name("changeReturnType");
    pub const COLLECTARGUMENTS: Self = Self::name("collectArguments");
    pub const COMPARE: Self = Self::name("compare");
    pub const COMPAREUNSIGNED: Self = Self::name("compareUnsigned");
    pub const CONSTANT: Self = Self::name("constant");
    pub const COPYOF: Self = Self::name("copyOf");
    pub const COPYSIGN: Self = Self::name("copySign");
    pub const DIVIDEUNSIGNED: Self = Self::name("divideUnsigned");
    pub const DOUBLETORAWLONGBITS: Self = Self::name("doubleToRawLongBits");
    pub const DOUBLEVALUE: Self = Self::name("doubleValue");
    pub const DROPARGUMENTS: Self = Self::name("dropArguments");
    pub const DROPPARAMETERTYPES: Self = Self::name("dropParameterTypes");
    pub const DYNAMICINVOKER: Self = Self::name("dynamicInvoker");
    pub const EMPTY: Self = Self::name("empty");
    pub const EQUALS: Self = Self::name("equals");
    pub const EXACTINVOKER: Self = Self::name("exactInvoker");
    pub const FILL: Self = Self::name("fill");
    pub const FILTERRETURNVALUE: Self = Self::name("filterReturnValue");
    pub const FINDSTATIC: Self = Self::name("findStatic");
    pub const FLOATTORAWINTBITS: Self = Self::name("floatToRawIntBits");
    pub const FLOATVALUE: Self = Self::name("floatValue");
    pub const FLOOR: Self = Self::name("floor");
    pub const GET: Self = Self::name("get");
    pub const GETBYTES: Self = Self::name("getBytes");
    pub const GETDOUBLE: Self = Self::name("getDouble");
    pub const GETFLOAT: Self = Self::name("getFloat");
    pub const GETINT: Self = Self::name("getInt");
    pub const GETLONG: Self = Self::name("getLong");
    pub const GETSHORT: Self = Self::name("getShort");
    pub const GETTARGET: Self = Self::name("getTarget");
    pub const GUARDWITHTEST: Self = Self::name("guardWithTest");
    pub const HASHCODE: Self = Self::name("hashCode");
    pub const INSERTARGUMENTS: Self = Self::name("insertArguments");
    pub const INTBITSTOFLOAT: Self = Self::name("intBitsToFloat");
    pub const INTVALUE: Self = Self::name("intValue");
    pub const INVOKEEXACT: Self = Self::name("invokeExact");
    pub const LENGTH: Self = Self::name("length");
    pub const LITTLEENDIAN: Self = Self::name("LITTLE_ENDIAN");
    pub const LONGBITSTODOUBLE: Self = Self::name("longBitsToDouble");
    pub const LONGVALUE: Self = Self::name("longValue");
    pub const LOOKUP: Self = Self::name("Lookup");
    pub const MAX: Self = Self::name("max");
    pub const MAXVALUE: Self = Self::name("MAX_VALUE");
    pub const METHODTYPE: Self = Self::name("methodType");
    pub const MIN: Self = Self::name("min");
    pub const MINVALUE: Self = Self::name("MIN_VALUE");
    pub const NAN: Self = Self::name("NaN");
    pub const NEGATIVEINFINITY: Self = Self::name("NEGATIVE_INFINITY");
    pub const NUMBEROFLEADINGZEROS: Self = Self::name("numberOfLeadingZeros");
    pub const NUMBEROFTRAILINGZEROS: Self = Self::name("numberOfTrailingZeros");
    pub const ORDER: Self = Self::name("order");
    pub const PARAMETERARRAY: Self = Self::name("parameterArray");
    pub const PARAMETERCOUNT: Self = Self::name("parameterCount");
    pub const PARAMETERTYPE: Self = Self::name("parameterType");
    pub const PERMUTEARGUMENTS: Self = Self::name("permuteArguments");
    pub const POSITION: Self = Self::name("position");
    pub const POSITIVEINFINITY: Self = Self::name("POSITIVE_INFINITY");
    pub const PUT: Self = Self::name("put");
    pub const PUTDOUBLE: Self = Self::name("putDouble");
    pub const PUTFLOAT: Self = Self::name("putFloat");
    pub const PUTINT: Self = Self::name("putInt");
    pub const PUTLONG: Self = Self::name("putLong");
    pub const PUTSHORT: Self = Self::name("putShort");
    pub const REMAINDERUNSIGNED: Self = Self::name("remainderUnsigned");
    pub const RETURNTYPE: Self = Self::name("returnType");
    pub const RINT: Self = Self::name("rint");
    pub const ROTATELEFT: Self = Self::name("rotateLeft");
    pub const ROTATERIGHT: Self = Self::name("rotateRight");
    pub const SETTARGET: Self = Self::name("setTarget");
    pub const SHORTVALUE: Self = Self::name("shortValue");
    pub const SQRT: Self = Self::name("sqrt");
    pub const SYNCALL: Self = Self::name("syncAll");
    pub const TOINTEXACT: Self = Self::name("toIntExact");
    pub const TYPE: Self = Self::name("type");
    pub const UPPERCASE_TYPE: Self = Self::name("TYPE");
    pub const VALUEOF: Self = Self::name("valueOf");

    // Special unqualified names - only these are allowed to have angle brackets in them
    pub const INIT: Self = Self::name("<init>");
    pub const CLINIT: Self = Self::name("<clinit>");

    // Names we generate
    pub const BOOTSTRAPMEMORY: Self = Self::name("bootstrapMemory");
    pub const BOOTSTRAPTABLE: Self = Self::name("bootstrapTable");
    pub const BYTESTOPAGES: Self = Self::name("bytesToPages");
    pub const CALLINDIRECT: Self = Self::name("call_indirect");
    pub const COPYRESIZEDARRAY: Self = Self::name("copyResizedArray");
    pub const COPYRESIZEDBYTEBUFFER: Self = Self::name("copyResizedByteBuffer");
    pub const EXPORTS: Self = Self::name("exports");
    pub const EXTERNREFTABLEBOOTSTRAP: Self = Self::name("externrefTableBootstrap");
    pub const F32ABS: Self = Self::name("f32Abs");
    pub const F32CONVERTI32U: Self = Self::name("f32ConvertI32U");
    pub const F32CONVERTI64U: Self = Self::name("f32ConvertI64U");
    pub const F32TRUNC: Self = Self::name("f32Trunc");
    pub const F64ABS: Self = Self::name("f64Abs");
    pub const F64CONVERTI32U: Self = Self::name("f64ConvertI32U");
    pub const F64CONVERTI64U: Self = Self::name("f64ConvertI64U");
    pub const F64TRUNC: Self = Self::name("f64Trunc");
    pub const FILLARRAYRANGE: Self = Self::name("fillArrayRange");
    pub const FILLBYTEBUFFERRANGE: Self = Self::name("fillByteBufferRange");
    pub const FUNCREFTABLEBOOTSTRAP: Self = Self::name("funcrefTableBootstrap");
    pub const I32DIVS: Self = Self::name("i32DivS");
    pub const I32TRUNCF32S: Self = Self::name("i32TruncF32S");
    pub const I32TRUNCF32U: Self = Self::name("i32TruncF32U");
    pub const I32TRUNCF64S: Self = Self::name("i32TruncF64S");
    pub const I32TRUNCF64U: Self = Self::name("i32TruncF64U");
    pub const I32TRUNCSATF32U: Self = Self::name("i32TruncSatF32U");
    pub const I32TRUNCSATF64U: Self = Self::name("i32TruncSatF64U");
    pub const I64DIVS: Self = Self::name("i64DivS");
    pub const I64EXTENDI32U: Self = Self::name("i64ExtendI32U");
    pub const I64TRUNCF32S: Self = Self::name("i64TruncF32S");
    pub const I64TRUNCF32U: Self = Self::name("i64TruncF32U");
    pub const I64TRUNCF64S: Self = Self::name("i64TruncF64S");
    pub const I64TRUNCF64U: Self = Self::name("i64TruncF64U");
    pub const I64TRUNCSATF32U: Self = Self::name("i64TruncSatF32U");
    pub const I64TRUNCSATF64U: Self = Self::name("i64TruncSatF64U");
    pub const INTISNEGATIVEONE: Self = Self::name("intIsNegativeOne");
    pub const MEMORYFILL: Self = Self::name("memory_fill");
    pub const MEMORYGROW: Self = Self::name("memory_grow");
    pub const MEMORYSIZE: Self = Self::name("memory_size");
    pub const NEXTSIZE: Self = Self::name("nextSize");
    pub const PAGESTOBYTES: Self = Self::name("pagesToBytes");
    pub const TABLECOPY: Self = Self::name("table_copy");
    pub const TABLEFILL: Self = Self::name("table_fill");
    pub const TABLEGET: Self = Self::name("table_get");
    pub const TABLEGROW: Self = Self::name("table_grow");
    pub const TABLEINIT: Self = Self::name("table_init");
    pub const TABLESET: Self = Self::name("table_set");
    pub const TABLESIZE: Self = Self::name("table_size");
    pub const UNREACHABLE: Self = Self::name("unreachable");

    pub const DOLLAR: Self = Self::name("$");
}

impl BinaryName {
    /// Concatenate the contents of an unqualified name onto the end of the binary name to produce
    /// a third. If you want a new segment, use `join` instead.
    pub fn concat(&self, other: &UnqualifiedName) -> BinaryName {
        BinaryName(Cow::Owned(format!("{}{}", self.as_str(), other.as_str())))
    }

    /// Join segments from the other name onto the end of this binary name
    pub fn join(&self, other: impl Name) -> BinaryName {
        BinaryName(Cow::Owned(format!("{}/{}", self.as_str(), other.as_str())))
    }

    const fn name(value: &'static str) -> BinaryName {
        BinaryName(Cow::Borrowed(value))
    }

    // JDK names
    pub const ARITHMETICEXCEPTION: Self = Self::name("java/lang/ArithmeticException");
    pub const ARRAYS: Self = Self::name("java/util/Arrays");
    pub const ASSERTIONERROR: Self = Self::name("java/lang/AssertionError");
    pub const BOOLEAN: Self = Self::name("java/lang/Boolean");
    pub const BUFFER: Self = Self::name("java/nio/Buffer");
    pub const BYTEBUFFER: Self = Self::name("java/nio/ByteBuffer");
    pub const BYTEORDER: Self = Self::name("java/nio/ByteOrder");
    pub const CALLSITE: Self = Self::name("java/lang/invoke/CallSite");
    pub const CHARSEQUENCE: Self = Self::name("java/lang/CharSequence");
    pub const CLASS: Self = Self::name("java/lang/Class");
    pub const CLONEABLE: Self = Self::name("java/lang/Cloneable");
    pub const CONSTANTCALLSITE: Self = Self::name("java/lang/invoke/ConstantCallSite");
    pub const DOUBLE: Self = Self::name("java/lang/Double");
    pub const ERROR: Self = Self::name("java/lang/Error");
    pub const EXCEPTION: Self = Self::name("java/lang/Exception");
    pub const FLOAT: Self = Self::name("java/lang/Float");
    pub const HASHMAP: Self = Self::name("java/util/HashMap");
    pub const ILLEGALARGUMENTEXCEPTION: Self = Self::name("java/lang/IllegalArgumentException");
    pub const INTEGER: Self = Self::name("java/lang/Integer");
    pub const LONG: Self = Self::name("java/lang/Long");
    pub const MAP: Self = Self::name("java/util/Map");
    pub const MATH: Self = Self::name("java/lang/Math");
    pub const METHODHANDLE: Self = Self::name("java/lang/invoke/MethodHandle");
    pub const METHODHANDLES: Self = Self::name("java/lang/invoke/MethodHandles");
    pub const METHODHANDLES_LOOKUP: Self = Self::name("java/lang/invoke/MethodHandles$Lookup");
    pub const METHODTYPE: Self = Self::name("java/lang/invoke/MethodType");
    pub const MUTABLECALLSITE: Self = Self::name("java/lang/invoke/MutableCallSite");
    pub const NUMBER: Self = Self::name("java/lang/Number");
    pub const OBJECT: Self = Self::name("java/lang/Object");
    pub const RUNTIMEEXCEPTION: Self = Self::name("java/lang/RuntimeException");
    pub const SERIALIZABLE: Self = Self::name("java/io/Serializable");
    pub const STRING: Self = Self::name("java/lang/String");
    pub const SYSTEM: Self = Self::name("java/lang/System");
    pub const THROWABLE: Self = Self::name("java/lang/Throwable");
    pub const VOID: Self = Self::name("java/lang/Void");
}
