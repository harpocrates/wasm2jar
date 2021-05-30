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
    pub const BITCOUNT: Self = Self::name("bitCount");
    pub const BYTEVALUE: Self = Self::name("byteValue");
    pub const CEIL: Self = Self::name("ceil");
    pub const COMPARE: Self = Self::name("compare");
    pub const COMPAREUNSIGNED: Self = Self::name("compareUnsigned");
    pub const COPYOF: Self = Self::name("copyOf");
    pub const COPYSIGN: Self = Self::name("copySign");
    pub const DIVIDEUNSIGNED: Self = Self::name("divideUnsigned");
    pub const DOUBLETORAWLONGBITS: Self = Self::name("doubleToRawLongBits");
    pub const DOUBLEVALUE: Self = Self::name("doubleValue");
    pub const EQUALS: Self = Self::name("equals");
    pub const FILL: Self = Self::name("fill");
    pub const FLOATTORAWINTBITS: Self = Self::name("floatToRawIntBits");
    pub const FLOATVALUE: Self = Self::name("floatValue");
    pub const FLOOR: Self = Self::name("floor");
    pub const GETBYTES: Self = Self::name("getBytes");
    pub const HASHCODE: Self = Self::name("hashCode");
    pub const INTBITSTOFLOAT: Self = Self::name("intBitsToFloat");
    pub const INTVALUE: Self = Self::name("intValue");
    pub const LENGTH: Self = Self::name("length");
    pub const LONGBITSTODOUBLE: Self = Self::name("longBitsToDouble");
    pub const LONGVALUE: Self = Self::name("longValue");
    pub const MAX: Self = Self::name("max");
    pub const MAXVALUE: Self = Self::name("MAX_VALUE");
    pub const MIN: Self = Self::name("min");
    pub const MINVALUE: Self = Self::name("MIN_VALUE");
    pub const NAN: Self = Self::name("NaN");
    pub const NEGATIVEINFINITY: Self = Self::name("NEGATIVE_INFINITY");
    pub const NUMBEROFLEADINGZEROS: Self = Self::name("numberOfLeadingZeros");
    pub const NUMBEROFTRAILINGZEROS: Self = Self::name("numberOfTrailingZeros");
    pub const POSITIVEINFINITY: Self = Self::name("POSITIVE_INFINITY");
    pub const REMAINDERUNSIGNED: Self = Self::name("remainderUnsigned");
    pub const RINT: Self = Self::name("rint");
    pub const ROTATELEFT: Self = Self::name("rotateLeft");
    pub const ROTATERIGHT: Self = Self::name("rotateRight");
    pub const SHORTVALUE: Self = Self::name("shortValue");
    pub const SQRT: Self = Self::name("sqrt");
    pub const TOINTEXACT: Self = Self::name("toIntExact");
    pub const VALUEOF: Self = Self::name("valueOf");

    // Special unqualified names - only these are allowed to have angle brackets in them
    pub const INIT: Self = Self::name("<init>");
    pub const CLINIT: Self = Self::name("<clinit>");

    // Names we generate
    pub const EXTERNREFTABLEBOOTSTRAP: Self = Self::name("externrefTableBootstrap");
    pub const F32ABS: Self = Self::name("f32Abs");
    pub const F32CONVERTI32U: Self = Self::name("f32ConvertI32U");
    pub const F32CONVERTI64U: Self = Self::name("f32ConvertI64U");
    pub const F32TRUNC: Self = Self::name("f32Trunc");
    pub const F64ABS: Self = Self::name("f64Abs");
    pub const F64CONVERTI32U: Self = Self::name("f64ConvertI32U");
    pub const F64CONVERTI64U: Self = Self::name("f64ConvertI64U");
    pub const F64TRUNC: Self = Self::name("f64Trunc");
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
    pub const CALLSITE: Self = Self::name("java/lang/invoke/CallSite");
    pub const CHARSEQUENCE: Self = Self::name("java/lang/CharSequence");
    pub const CLASS: Self = Self::name("java/lang/Class");
    pub const CLONEABLE: Self = Self::name("java/lang/Cloneable");
    pub const DOUBLE: Self = Self::name("java/lang/Double");
    pub const ERROR: Self = Self::name("java/lang/Error");
    pub const EXCEPTION: Self = Self::name("java/lang/Exception");
    pub const FLOAT: Self = Self::name("java/lang/Float");
    pub const INTEGER: Self = Self::name("java/lang/Integer");
    pub const LONG: Self = Self::name("java/lang/Long");
    pub const MATH: Self = Self::name("java/lang/Math");
    pub const METHODHANDLE_LOOKUP: Self = Self::name("java/lang/invoke/MethodHandle$Lookup");
    pub const METHODHANDLE: Self = Self::name("java/lang/invoke/MethodHandle");
    pub const METHODTYPE: Self = Self::name("java/lang/invoke/MethodType");
    pub const NUMBER: Self = Self::name("java/lang/Number");
    pub const OBJECT: Self = Self::name("java/lang/Object");
    pub const RUNTIMEEXCEPTION: Self = Self::name("java/lang/RuntimeException");
    pub const SERIALIZABLE: Self = Self::name("java/io/Serializable");
    pub const STRING: Self = Self::name("java/lang/String");
    pub const THROWABLE: Self = Self::name("java/lang/Throwable");
}
