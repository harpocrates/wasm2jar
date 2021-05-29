use super::Width;
use std::borrow::Cow;
use std::io::{Error, ErrorKind, Result};
use std::str::Chars;

/// Utility trait for converting descriptors to and from string representations
pub trait Descriptor: Sized {
    /// Turn the descriptor into a string
    fn render(&self) -> String {
        let mut string = String::new();
        self.render_to(&mut string);
        string
    }

    /// Parse a descriptor from a string
    fn parse(source: &str) -> Result<Self> {
        let mut chars = source.chars();
        let ret = Descriptor::parse_from(&mut chars)?;
        let rest = chars.as_str();
        if rest.is_empty() {
            Ok(ret)
        } else {
            let msg = format!("Unexpected leftover input '{}'", rest);
            Err(Error::new(ErrorKind::InvalidInput, msg))
        }
    }

    /// Write the descriptor to a string
    fn render_to(&self, write_to: &mut String);

    /// Read the descriptor from a character buffer
    fn parse_from(source: &mut Chars) -> Result<Self>;
}

/// Primitive value types
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum BaseType {
    Byte,
    Char,
    Double,
    Float,
    Int,
    Long,
    Short,
    Boolean,
}

impl Width for BaseType {
    fn width(&self) -> usize {
        match self {
            BaseType::Byte
            | BaseType::Char
            | BaseType::Float
            | BaseType::Int
            | BaseType::Short
            | BaseType::Boolean => 1,
            BaseType::Double | BaseType::Long => 2,
        }
    }
}

impl Descriptor for BaseType {
    fn render_to(&self, write_to: &mut String) {
        let c = match self {
            BaseType::Byte => 'B',
            BaseType::Char => 'C',
            BaseType::Double => 'D',
            BaseType::Float => 'F',
            BaseType::Int => 'I',
            BaseType::Long => 'J',
            BaseType::Short => 'S',
            BaseType::Boolean => 'Z',
        };
        write_to.push(c);
    }

    fn parse_from<'a>(source: &mut Chars) -> Result<Self> {
        let typ = match source.next() {
            Some('B') => BaseType::Byte,
            Some('C') => BaseType::Char,
            Some('D') => BaseType::Double,
            Some('F') => BaseType::Float,
            Some('I') => BaseType::Int,
            Some('J') => BaseType::Long,
            Some('S') => BaseType::Short,
            Some('Z') => BaseType::Boolean,
            Some(c) => {
                let msg = format!("Invalid base type character '{}'", c);
                return Err(Error::new(ErrorKind::InvalidInput, msg));
            }
            None => {
                let msg = "Missing base type character";
                return Err(Error::new(ErrorKind::UnexpectedEof, msg));
            }
        };
        Ok(typ)
    }
}

/// Reference type
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum RefType {
    Object(Cow<'static, str>),
    Array(Box<FieldType>), // TODO: consider including dimension depth so as to avoid nested boxes
}

impl Descriptor for RefType {
    fn render_to(&self, write_to: &mut String) {
        match self {
            RefType::Object(class_name) => {
                write_to.push('L');
                write_to.push_str(class_name);
                write_to.push(';');
            }
            RefType::Array(field_type) => {
                write_to.push('[');
                field_type.render_to(write_to);
            }
        }
    }

    fn parse_from(source: &mut Chars) -> Result<Self> {
        match source.next() {
            Some('L') => {
                let mut class_name = String::new();
                loop {
                    let c: char = source.next().ok_or_else(|| {
                        let msg = format!("Missing terminator for 'L{}'", class_name);
                        Error::new(ErrorKind::UnexpectedEof, msg)
                    })?;
                    if c == ';' {
                        return Ok(RefType::object(class_name));
                    } else {
                        class_name.push(c)
                    }
                }
            }
            Some('[') => {
                let elem_typ = FieldType::parse_from(source)?;
                Ok(RefType::array(elem_typ))
            }
            Some(c) => {
                let msg = format!("Invalid reference type character '{}'", c);
                return Err(Error::new(ErrorKind::InvalidInput, msg));
            }
            None => {
                let msg = "Missing field type";
                Err(Error::new(ErrorKind::UnexpectedEof, msg))
            }
        }
    }
}

impl RefType {
    pub fn array(field_type: FieldType) -> RefType {
        RefType::Array(Box::new(field_type))
    }

    pub fn object(class_name: impl Into<Cow<'static, str>>) -> RefType {
        RefType::Object(class_name.into())
    }

    /// Render the type for a class info
    ///
    /// When making a `CONSTANT_Class_info`, reference types are almost always objects. However,
    /// there are a handful of places where an array type needs to be fit in (eg. for a `checkcast`
    /// to an array type). See section 4.4.1 for more.
    pub fn render_class_info(&self) -> Cow<'static, str> {
        match self {
            RefType::Object(Cow::Borrowed(object_desc)) => Cow::Borrowed(object_desc),
            RefType::Object(Cow::Owned(object_desc)) => Cow::Owned(object_desc.clone()),
            array => Cow::Owned(array.render()),
        }
    }

    /// Parse a class from a class info
    pub fn parse_class_info(descriptor: &str) -> Result<RefType> {
        if let Some('[') = descriptor.chars().next() {
            RefType::parse(&descriptor)
        } else {
            Ok(RefType::object(descriptor.to_string()))
        }
    }

    pub const OBJECT_NAME: &'static str = "java/lang/Object";

    pub const CLASS_NAME: &'static str = "java/lang/Class";
    pub const STRING_NAME: &'static str = "java/lang/String";
    pub const METHOD_HANDLE_NAME: &'static str = "java/lang/invoke/MethodHandle";
    pub const METHOD_TYPE_NAME: &'static str = "java/lang/invoke/MethodType";

    pub const CLONEABLE_NAME: &'static str = "java/lang/Cloneable";
    pub const SERIALIZABLE_NAME: &'static str = "java/io/Serializable";
    pub const ERROR_NAME: &'static str = "java/lang/Error";
    pub const THROWABLE_NAME: &'static str = "java/lang/Throwable";
    pub const EXCEPTION_NAME: &'static str = "java/lang/Exception";
    pub const RUNTIMEEXCEPTION_NAME: &'static str = "java/lang/RuntimeException";
    pub const ARITHMETIC_NAME: &'static str = "java/lang/ArithmeticException";
    pub const ASSERTION_NAME: &'static str = "java/lang/AssertionError";
    pub const CHARSEQUENCE_NAME: &'static str = "java/lang/CharSequence";
    pub const NUMBER_NAME: &'static str = "java/lang/Number";
    pub const INTEGER_NAME: &'static str = "java/lang/Integer";
    pub const FLOAT_NAME: &'static str = "java/lang/Float";
    pub const LONG_NAME: &'static str = "java/lang/Long";
    pub const DOUBLE_NAME: &'static str = "java/lang/Double";
    pub const MATH_NAME: &'static str = "java/lang/Math";
    pub const ARRAYS_NAME: &'static str = "java/util/Arrays";

    pub const OBJECT_CLASS: RefType = Self::Object(Cow::Borrowed(Self::OBJECT_NAME));

    pub const CLASS_CLASS: RefType = Self::Object(Cow::Borrowed(Self::CLASS_NAME));
    pub const STRING_CLASS: RefType = Self::Object(Cow::Borrowed(Self::STRING_NAME));
    pub const METHOD_HANDLE_CLASS: RefType = Self::Object(Cow::Borrowed(Self::METHOD_HANDLE_NAME));
    pub const METHOD_TYPE_CLASS: RefType = Self::Object(Cow::Borrowed(Self::METHOD_TYPE_NAME));

    pub const ERROR_CLASS: RefType = Self::Object(Cow::Borrowed(Self::ERROR_NAME));
    pub const THROWABLE_CLASS: RefType = Self::Object(Cow::Borrowed(Self::THROWABLE_NAME));
    pub const EXCEPTION_CLASS: RefType = Self::Object(Cow::Borrowed(Self::EXCEPTION_NAME));
    pub const RUNTIMEEXCEPTION_CLASS: RefType =
        Self::Object(Cow::Borrowed(Self::RUNTIMEEXCEPTION_NAME));
    pub const ARITHMETIC_CLASS: RefType = Self::Object(Cow::Borrowed(Self::ARITHMETIC_NAME));
    pub const ASSERTION_CLASS: RefType = Self::Object(Cow::Borrowed(Self::ASSERTION_NAME));
    pub const INTEGER_CLASS: RefType = Self::Object(Cow::Borrowed(Self::INTEGER_NAME));
    pub const FLOAT_CLASS: RefType = Self::Object(Cow::Borrowed(Self::FLOAT_NAME));
    pub const LONG_CLASS: RefType = Self::Object(Cow::Borrowed(Self::LONG_NAME));
    pub const DOUBLE_CLASS: RefType = Self::Object(Cow::Borrowed(Self::DOUBLE_NAME));
}

/// Type of a class, instance, or local variable
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum FieldType {
    Base(BaseType),
    Ref(RefType),
}

impl Width for FieldType {
    fn width(&self) -> usize {
        match self {
            FieldType::Base(base_type) => base_type.width(),
            FieldType::Ref(_) => 1,
        }
    }
}

impl FieldType {
    pub const INT: FieldType = FieldType::Base(BaseType::Int);
    pub const LONG: FieldType = FieldType::Base(BaseType::Long);
    pub const FLOAT: FieldType = FieldType::Base(BaseType::Float);
    pub const DOUBLE: FieldType = FieldType::Base(BaseType::Double);
    pub const CHAR: FieldType = FieldType::Base(BaseType::Char);
    pub const SHORT: FieldType = FieldType::Base(BaseType::Short);
    pub const BYTE: FieldType = FieldType::Base(BaseType::Byte);
    pub const BOOLEAN: FieldType = FieldType::Base(BaseType::Boolean);
    pub const OBJECT: FieldType = FieldType::Ref(RefType::OBJECT_CLASS);

    pub fn array(field_type: FieldType) -> FieldType {
        FieldType::Ref(RefType::array(field_type))
    }

    pub fn object(class_name: impl Into<Cow<'static, str>>) -> FieldType {
        FieldType::Ref(RefType::object(class_name))
    }
}

impl Descriptor for FieldType {
    fn render_to(&self, write_to: &mut String) {
        match self {
            FieldType::Base(base_type) => base_type.render_to(write_to),
            FieldType::Ref(reference_type) => reference_type.render_to(write_to),
        }
    }

    fn parse_from(source: &mut Chars) -> Result<Self> {
        match source.clone().next() {
            None => Err(Error::new(ErrorKind::UnexpectedEof, "Missing field type")),
            Some('B') | Some('C') | Some('D') | Some('F') | Some('I') | Some('J') | Some('S')
            | Some('Z') => BaseType::parse_from(source).map(FieldType::Base),
            Some('L') | Some('[') => RefType::parse_from(source).map(FieldType::Ref),
            Some(c) => {
                let msg = format!("Invalid reference type character '{}'", c);
                Err(Error::new(ErrorKind::InvalidInput, msg))
            }
        }
    }
}

/// Signature of a method
#[derive(PartialEq, Eq, Hash, Debug, Clone)]
pub struct MethodDescriptor {
    pub parameters: Vec<FieldType>,
    pub return_type: Option<FieldType>, // `None` is for `void` (ie. no return)
}

impl MethodDescriptor {
    /// Total length of parameters (not the same as the length of the vector),
    /// which must be 255 or less for it to be valid
    pub fn parameter_length(&self, has_this_param: bool) -> usize {
        let mut len = if has_this_param { 1 } else { 0 };
        for parameter in &self.parameters {
            len += match parameter {
                FieldType::Base(BaseType::Double) | FieldType::Base(BaseType::Long) => 2,
                _ => 1,
            }
        }
        len
    }
}

impl Descriptor for MethodDescriptor {
    fn render_to(&self, write_to: &mut String) {
        write_to.push('(');
        for parameter in &self.parameters {
            parameter.render_to(write_to);
        }
        write_to.push(')');
        match &self.return_type {
            None => write_to.push('V'),
            Some(typ) => typ.render_to(write_to),
        };
    }

    fn parse_from(source: &mut Chars) -> Result<Self> {
        // Assert open paren
        if let Some('(') = source.next() {
        } else {
            let msg = "Expected '(' for method";
            return Err(Error::new(ErrorKind::InvalidInput, msg));
        }

        // Parse parameters
        let mut parameters = vec![];
        while source.clone().next() != Some(')') {
            parameters.push(FieldType::parse_from(source)?);
        }

        // Assert close paren
        if let Some(')') = source.next() {
        } else {
            let msg = "Expected ')' for method";
            return Err(Error::new(ErrorKind::InvalidInput, msg));
        }

        // Parse return
        let return_type = if let Some('V') = source.clone().next() {
            let _ = source.next();
            None
        } else {
            Some(FieldType::parse_from(source)?)
        };

        Ok(MethodDescriptor {
            parameters,
            return_type,
        })
    }
}

/// Any JVM signature
#[derive(PartialEq, Eq, Hash, Debug)]
pub enum JavaTypeSignature {
    Base(BaseType),
    Reference(ReferenceTypeSignature),
}

impl Descriptor for JavaTypeSignature {
    fn render_to(&self, write_to: &mut String) {
        match self {
            JavaTypeSignature::Base(typ) => typ.render_to(write_to),
            JavaTypeSignature::Reference(typ) => typ.render_to(write_to),
        }
    }

    fn parse_from(source: &mut Chars) -> Result<Self> {
        match source.clone().next() {
            Some('L') | Some('T') | Some('[') => {
                let sig = ReferenceTypeSignature::parse_from(source)?;
                Ok(JavaTypeSignature::Reference(sig))
            }
            _ => {
                let base = BaseType::parse_from(source)?;
                Ok(JavaTypeSignature::Base(base))
            }
        }
    }
}

/// Signature for a reference type
#[derive(PartialEq, Eq, Hash, Debug)]
pub enum ReferenceTypeSignature {
    Class(ClassTypeSignature),
    TypeVariable(String),
    Array(Box<JavaTypeSignature>),
}

impl Descriptor for ReferenceTypeSignature {
    fn render_to(&self, write_to: &mut String) {
        match self {
            ReferenceTypeSignature::Class(class) => class.render_to(write_to),
            ReferenceTypeSignature::TypeVariable(ty_var) => {
                write_to.push('T');
                write_to.push_str(ty_var);
                write_to.push(';');
            }
            ReferenceTypeSignature::Array(sig) => {
                write_to.push('[');
                sig.render_to(write_to);
            }
        }
    }

    fn parse_from(source: &mut Chars) -> Result<Self> {
        let sig = match source.clone().next() {
            Some('L') => {
                let class = ClassTypeSignature::parse_from(source)?;
                ReferenceTypeSignature::Class(class)
            }
            Some('T') => {
                let _ = source.next();
                let mut name = String::new();
                loop {
                    match source.next() {
                        None => {
                            let msg = "Type parameter terminator ';'";
                            return Err(Error::new(ErrorKind::UnexpectedEof, msg));
                        }
                        Some(';') => return Ok(ReferenceTypeSignature::TypeVariable(name)),
                        Some(c) => name.push(c),
                    }
                }
            }
            Some('[') => {
                let _ = source.next();
                let sig = JavaTypeSignature::parse_from(source)?;
                ReferenceTypeSignature::Array(Box::new(sig))
            }
            Some(c) => {
                let msg = format!("Invalid start to reference type: {}", c);
                return Err(Error::new(ErrorKind::InvalidInput, msg));
            }
            None => {
                let msg = "Expected reference type";
                return Err(Error::new(ErrorKind::UnexpectedEof, msg));
            }
        };
        Ok(sig)
    }
}

/// Type signature for a class or an interface
#[derive(PartialEq, Eq, Hash, Debug)]
pub struct ClassTypeSignature {
    pub packages: Vec<String>,
    pub class: SimpleClassTypeSignature,
    pub projections: Vec<SimpleClassTypeSignature>,
}

impl Descriptor for ClassTypeSignature {
    fn render_to(&self, write_to: &mut String) {
        write_to.push('L');
        for package in &self.packages {
            write_to.push_str(package);
            write_to.push('/')
        }
        self.class.render_to(write_to);
        for projection in &self.projections {
            write_to.push('.');
            projection.render_to(write_to);
        }
        write_to.push(';')
    }

    fn parse_from(source: &mut Chars) -> Result<Self> {
        // Assert leading `L`
        if let Some('L') = source.next() {
        } else {
            let msg = "Expected 'L' for class type signature";
            return Err(Error::new(ErrorKind::InvalidInput, msg));
        }

        // TODO: filter out empty idents?
        fn parse_ident(source: &mut Chars) -> Result<(String, Option<char>)> {
            let mut name = String::new();
            loop {
                let c_opt = source.clone().next();
                match c_opt {
                    Some('/') | Some('<') | Some('.') | Some(';') | None => {
                        return Ok((name, c_opt))
                    }
                    Some(c) => name.push(c),
                }
            }
        }

        // Parse identifiers
        let mut packages = vec![];
        let mut arguments = vec![];
        loop {
            let (ident, next_char) = parse_ident(source)?;
            packages.push(ident);
            if next_char == None {
                break;
            } else if next_char == Some('<') {
                while source.clone().next() != Some('>') {
                    arguments.push(TypeArgument::parse_from(source)?);
                }
                let _ = source.next();
                break;
            }
        }
        let class = SimpleClassTypeSignature {
            name: packages.pop().unwrap(),
            arguments,
        };

        // Projections
        let mut projections = vec![];
        while let Some('.') = source.clone().next() {
            projections.push(SimpleClassTypeSignature::parse_from(source)?);
        }

        Ok(ClassTypeSignature {
            packages,
            class,
            projections,
        })
    }
}

/// Type signature without a package prefix
#[derive(PartialEq, Eq, Hash, Debug)]
pub struct SimpleClassTypeSignature {
    pub name: String,
    pub arguments: Vec<TypeArgument>,
}

impl Descriptor for SimpleClassTypeSignature {
    fn render_to(&self, write_to: &mut String) {
        write_to.push_str(&self.name);
        if !self.arguments.is_empty() {
            write_to.push('<');
            for argument in &self.arguments {
                argument.render_to(write_to);
            }
            write_to.push('>');
        }
    }

    fn parse_from(source: &mut Chars) -> Result<Self> {
        let mut name = String::new();
        let mut arguments = vec![];
        loop {
            match source.clone().next() {
                Some('<') => {
                    while source.clone().next() != Some('>') {
                        arguments.push(TypeArgument::parse_from(source)?);
                    }
                    let _ = source.next();
                    break;
                }
                Some(other) => name.push(other),
                None => break,
            }
        }
        Ok(SimpleClassTypeSignature { name, arguments })
    }
}

/// Type argument (needed to complete signatures for generic classes)
#[derive(PartialEq, Eq, Hash, Debug)]
pub enum TypeArgument {
    Concrete(Option<WildcardIndicator>, ReferenceTypeSignature),
    Wildcard,
}

impl Descriptor for TypeArgument {
    fn render_to(&self, write_to: &mut String) {
        match self {
            TypeArgument::Wildcard => write_to.push('*'),
            TypeArgument::Concrete(indicator, reference_type) => {
                indicator.iter().for_each(|wi| {
                    write_to.push(match wi {
                        WildcardIndicator::Plus => '+',
                        WildcardIndicator::Minus => '-',
                    });
                });
                reference_type.render_to(write_to);
            }
        };
    }

    fn parse_from(source: &mut Chars) -> Result<Self> {
        let ty_arg = match source.clone().next() {
            Some('*') => {
                let _ = source.next();
                TypeArgument::Wildcard
            }
            Some('+') => {
                let _ = source.next();
                let ref_type = ReferenceTypeSignature::parse_from(source)?;
                TypeArgument::Concrete(Some(WildcardIndicator::Plus), ref_type)
            }
            Some('-') => {
                let _ = source.next();
                let ref_type = ReferenceTypeSignature::parse_from(source)?;
                TypeArgument::Concrete(Some(WildcardIndicator::Minus), ref_type)
            }
            _ => {
                let ref_type = ReferenceTypeSignature::parse_from(source)?;
                TypeArgument::Concrete(None, ref_type)
            }
        };
        Ok(ty_arg)
    }
}

// TODO: what is this for!?
#[derive(PartialEq, Eq, Hash, Debug)]
pub enum WildcardIndicator {
    Plus,
    Minus,
}

#[cfg(test)]
mod test {
    use super::*;
    use std::fmt::Debug;

    fn round_trip<T: Descriptor + Debug + Eq>(rendered: &str, parsed: T) {
        assert_eq!(rendered, parsed.render());
        assert_eq!(T::parse(rendered).unwrap(), parsed);
    }

    #[test]
    fn base_types() {
        round_trip("B", BaseType::Byte);
        round_trip("B", BaseType::Byte);
        round_trip("C", BaseType::Char);
        round_trip("D", BaseType::Double);
        round_trip("F", BaseType::Float);
        round_trip("I", BaseType::Int);
        round_trip("J", BaseType::Long);
        round_trip("S", BaseType::Short);
        round_trip("Z", BaseType::Boolean);
    }

    #[test]
    fn field_types() {
        round_trip("I", FieldType::Base(BaseType::Int));
        round_trip("Ljava/lang/Object;", FieldType::object("java/lang/Object"));
        round_trip(
            "[[[D",
            FieldType::array(FieldType::array(FieldType::array(FieldType::DOUBLE))),
        );
        round_trip(
            "[Ljava/lang/String;",
            FieldType::array(FieldType::object("java/lang/String")),
        );
    }

    #[test]
    fn method_descriptors() {
        round_trip(
            "(IDLjava/lang/Thread;)Ljava/lang/Object;",
            MethodDescriptor {
                parameters: vec![
                    FieldType::INT,
                    FieldType::DOUBLE,
                    FieldType::object("java/lang/Thread"),
                ],
                return_type: Some(FieldType::object("java/lang/Object")),
            },
        )
    }
}
