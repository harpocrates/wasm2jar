use std::convert::TryFrom;
use std::fmt::{Formatter, Display, Debug, Error as FmtError};

/// Names of methods, fields
///
/// See <https://docs.oracle.com/javase/specs/jvms/se16/html/jvms-4.html#jvms-4.2.2>
#[derive(Copy, Clone, Hash, Eq, PartialEq)]
pub struct UnqualifiedName<'a>(&'a str);

/// Names of classes and interfaces
///
/// See <https://docs.oracle.com/javase/specs/jvms/se16/html/jvms-4.html#jvms-4.2.1>
#[derive(Copy, Clone, Hash, Eq, PartialEq)]
pub struct BinaryName<'a>(&'a str);

/// Extracts the raw underlying string name
impl AsRef<str> for UnqualifiedName<'_> {
    fn as_ref(&self) -> &str {
        self.0
    }
}

/// Extracts the raw underlying string name
impl AsRef<str> for BinaryName<'_> {
    fn as_ref(&self) -> &str {
        self.0
    }
}

impl<'a> UnqualifiedName<'a> {

    /// Check if a string would be a valid unqualified name
    pub fn check_valid(name: impl AsRef<str>) -> Result<(), String> {
        let name = name.as_ref();
        if name.contains(&['.', ';', '[', '/'][..]) {
            Err(format!("Unqualified name '{}' contains an illegal character", name))
        } else if name.is_empty() {
            Err(format!("Unqualified name '{}' is empty", name))
        } else {
            Ok(())
        }
    }
}
impl<'a> BinaryName<'a> {

    /// Check if a string would be a valid binary name
    pub fn check_valid(name: impl AsRef<str>) -> Result<(), String> {
        let name = name.as_ref();
        if name.is_empty() {
            Err(format!("Binary name '{}' is empty", name))
        } else {
            name.split('/').map(UnqualifiedName::check_valid).collect()
        }
    }
}


impl<'a> TryFrom<&'a str> for UnqualifiedName<'a> {
    type Error = String;

    fn try_from(value: &'a str) -> Result<Self, Self::Error> {
        UnqualifiedName::check_valid(value).map(|_| UnqualifiedName(value))
    }
}
impl<'a> TryFrom<&'a str> for BinaryName<'a> {
    type Error = String;

    fn try_from(value: &'a str) -> Result<Self, Self::Error> {
        BinaryName::check_valid(value).map(|_| BinaryName(value))
    }
}


impl<'a> Display for UnqualifiedName<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), FmtError> {
        let name_str = self.0;
        if name_str.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$') {
            f.write_str(name_str)?;
        } else {
            f.write_str("\"")?;
            f.write_str(name_str)?;
            f.write_str("\"")?;
        }
        Ok(())
    }
}
impl<'a> Display for BinaryName<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), FmtError> {
        let name_str = self.0;
        let mut is_first = true;
        for unqualified_segment in name_str.split('/') {
            if is_first {
                is_first = false;
            } else {
                f.write_str("/")?;
            }
            Display::fmt(&UnqualifiedName(unqualified_segment), f)?;
        }
        Ok(())
    }
}


impl<'a> Debug for UnqualifiedName<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), FmtError> {
        f.write_str(self.0.as_ref())
    }
}
impl<'a> Debug for BinaryName<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), FmtError> {
        f.write_str(self.0.as_ref())
    }
}

impl UnqualifiedName<'static> {

    // JDK names
    pub const ABS: Self = UnqualifiedName("abs");
    pub const BITCOUNT: Self = UnqualifiedName("bitCount");
    pub const BYTEVALUE: Self = UnqualifiedName("byteValue");
    pub const CEIL: Self = UnqualifiedName("ceil");
    pub const COMPARE: Self = UnqualifiedName("compare");
    pub const COMPAREUNSIGNED: Self = UnqualifiedName("compareUnsigned");
    pub const COPYOF: Self = UnqualifiedName("copyOf");
    pub const COPYSIGN: Self = UnqualifiedName("copySign");
    pub const DIVIDEUNSIGNED: Self = UnqualifiedName("divideUnsigned");
    pub const DOUBLETORAWLONGBITS: Self = UnqualifiedName("doubleToRawLongBits");
    pub const DOUBLEVALUE: Self = UnqualifiedName("doubleValue");
    pub const EQUALS: Self = UnqualifiedName("equals");
    pub const FILL: Self = UnqualifiedName("fill");
    pub const FLOATTORAWINTBITS: Self = UnqualifiedName("floatToRawIntBits");
    pub const FLOATVALUE: Self = UnqualifiedName("floatValue");
    pub const FLOOR: Self = UnqualifiedName("floor");
    pub const GETBYTES: Self = UnqualifiedName("getBytes");
    pub const HASHCODE: Self = UnqualifiedName("hashCode");
    pub const INTBITSTOFLOAT: Self = UnqualifiedName("intBitsToFloat");
    pub const INTVALUE: Self = UnqualifiedName("intValue");
    pub const LENGTH: Self = UnqualifiedName("length");
    pub const LONGBITSTODOUBLE: Self = UnqualifiedName("longBitsToDouble");
    pub const LONGVALUE: Self = UnqualifiedName("longValue");
    pub const MAX: Self = UnqualifiedName("max");
    pub const MAXVALUE: Self = UnqualifiedName("MAX_VALUE");
    pub const MIN: Self = UnqualifiedName("min");
    pub const MINVALUE: Self = UnqualifiedName("MIN_VALUE");
    pub const NAN: Self = UnqualifiedName("NaN");
    pub const NEGATIVEINFINITY: Self = UnqualifiedName("NEGATIVE_INFINITY");
    pub const NUMBEROFLEADINGZEROS: Self = UnqualifiedName("numberOfLeadingZeros");
    pub const NUMBEROFTRAILINGZEROS: Self = UnqualifiedName("numberOfTrailingZeros");
    pub const POSITIVEINFINITY: Self = UnqualifiedName("POSITIVE_INFINITY");
    pub const REMAINDERUNSIGNED: Self = UnqualifiedName("remainderUnsigned");
    pub const RINT: Self = UnqualifiedName("rint");
    pub const ROTATELEFT: Self = UnqualifiedName("rotateLeft");
    pub const ROTATERIGHT: Self = UnqualifiedName("rotateRight");
    pub const SHORTVALUE: Self = UnqualifiedName("shortValue");
    pub const SQRT: Self = UnqualifiedName("sqrt");
    pub const TOINTEXACT: Self = UnqualifiedName("toIntExact");
    pub const VALUEOF: Self = UnqualifiedName("valueOf");

    // Special unqualified names - only these are allowed to have angle brackets in them
    pub const INIT: Self = UnqualifiedName("<init>");
    pub const CLINIT: Self = UnqualifiedName("<clinit>");

    // Names we generate
    pub const I32DIVS: Self = UnqualifiedName("i32DivS");
    pub const I64DIVS: Self = UnqualifiedName("i64DivS");
    pub const F32ABS: Self = UnqualifiedName("f32Abs");
    pub const F64ABS: Self = UnqualifiedName("f64Abs");
    pub const F32TRUNC: Self = UnqualifiedName("f32Trunc");
    pub const F64TRUNC: Self = UnqualifiedName("f64Trunc");
    pub const UNREACHABLE: Self = UnqualifiedName("unreachable");
    pub const I32TRUNCF32S: Self = UnqualifiedName("i32TruncF32S");
    pub const I32TRUNCF32U: Self = UnqualifiedName("i32TruncF32U");
    pub const I32TRUNCF64S: Self = UnqualifiedName("i32TruncF64S");
    pub const I32TRUNCF64U: Self = UnqualifiedName("i32TruncF64U");
    pub const I64EXTENDI32U: Self = UnqualifiedName("i64ExtendI32U");
    pub const I64TRUNCF32S: Self = UnqualifiedName("i64TruncF32S");
    pub const I64TRUNCF32U: Self = UnqualifiedName("i64TruncF32U");
    pub const I64TRUNCF64S: Self = UnqualifiedName("i64TruncF64S");
    pub const I64TRUNCF64U: Self = UnqualifiedName("i64TruncF64U");
    pub const F32CONVERTI32U: Self = UnqualifiedName("f32ConvertI32U");
    pub const F32CONVERTI64U: Self = UnqualifiedName("f32ConvertI64U");
    pub const F64CONVERTI32U: Self = UnqualifiedName("f64ConvertI32U");
    pub const F64CONVERTI64U: Self = UnqualifiedName("f64ConvertI64U");
    pub const I32TRUNCSATF32U: Self = UnqualifiedName("i32TruncSatF32U");
    pub const I32TRUNCSATF64U: Self = UnqualifiedName("i32TruncSatF64U");
    pub const I64TRUNCSATF32U: Self = UnqualifiedName("i64TruncSatF32U");
    pub const I64TRUNCSATF64U: Self = UnqualifiedName("i64TruncSatF64U");
    pub const FUNCREFTABLEBOOTSTRAP: Self = UnqualifiedName("funcrefTableBootstrap");
    pub const EXTERNREFTABLEBOOTSTRAP: Self = UnqualifiedName("externrefTableBootstrap");
}

impl BinaryName<'static> {

    // JDK names
    pub const ARITHMETICEXCEPTION: Self = BinaryName("java/lang/ArithmeticException");
    pub const ARRAYS: Self = BinaryName("java/util/Arrays");
    pub const ASSERTIONERROR: Self = BinaryName("java/lang/AssertionError");
    pub const CALLSITE: Self = BinaryName("java/lang/invoke/CallSite");
    pub const CHARSEQUENCE: Self = BinaryName("java/lang/CharSequence");
    pub const CLASS: Self = BinaryName("java/lang/Class");
    pub const CLONEABLE: Self = BinaryName("java/lang/Cloneable");
    pub const DOUBLE: Self = BinaryName("java/lang/Double");
    pub const ERROR: Self = BinaryName("java/lang/Error");
    pub const EXCEPTION: Self = BinaryName("java/lang/Exception");
    pub const FLOAT: Self = BinaryName("java/lang/Float");
    pub const INTEGER: Self = BinaryName("java/lang/Integer");
    pub const LONG: Self = BinaryName("java/lang/Long");
    pub const MATH: Self = BinaryName("java/lang/Math");
    pub const METHODHANDLE_LOOKUP: Self = BinaryName("java/lang/invoke/MethodHandle$Lookup");
    pub const METHODHANDLE: Self = BinaryName("java/lang/invoke/MethodHandle");
    pub const METHODTYPE: Self = BinaryName("java/lang/invoke/MethodType");
    pub const NUMBER: Self = BinaryName("java/lang/Number");
    pub const OBJECT: Self = BinaryName("java/lang/Object");
    pub const RUNTIMEEXCEPTION: Self = BinaryName("java/lang/RuntimeException");
    pub const SERIALIZABLE: Self = BinaryName("java/io/Serializable");
    pub const STRING: Self = BinaryName("java/lang/String");
    pub const THROWABLE: Self = BinaryName("java/lang/Throwable");
}
