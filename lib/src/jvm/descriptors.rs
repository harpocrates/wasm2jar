use super::{BinaryName, Name};
use crate::util::{RefId, Width};
use std::io::{Error, ErrorKind, Result};
use std::iter::Peekable;
use std::str::Chars;

/// Utility trait for converting descriptors to and from string representations
pub trait RenderDescriptor {
    /// Turn the descriptor into a string
    fn render(&self) -> String {
        let mut string = String::new();
        self.render_to(&mut string);
        string
    }

    /// Write the descriptor to a string
    fn render_to(&self, write_to: &mut String);
}

impl<'g, T: RenderDescriptor> RenderDescriptor for RefId<'g, T> {
    fn render_to(&self, write_to: &mut String) {
        self.0.render_to(write_to)
    }
}

pub trait ParseDescriptor: Sized {
    /// Parse a descriptor from a string
    fn parse(source: &str) -> Result<Self> {
        let mut chars = source.chars().peekable();
        let ret = Self::parse_from(&mut chars)?;
        match chars.next() {
            None => Ok(ret),
            Some(c) => {
                let msg = format!("Unexpected leftover input '{}'", c);
                Err(Error::new(ErrorKind::InvalidInput, msg))
            }
        }
    }

    /// Read the descriptor from a character buffer
    fn parse_from(source: &mut Peekable<Chars>) -> Result<Self>;
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

impl RenderDescriptor for BaseType {
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
}

impl ParseDescriptor for BaseType {
    fn parse_from<'a>(source: &mut Peekable<Chars>) -> Result<Self> {
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
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum RefType<Class> {
    Object(Class),
    ObjectArray(ArrayType<Class>),
    PrimitiveArray(ArrayType<BaseType>),
}

/// Generic array type
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct ArrayType<T> {
    /// Additional dimensions (`A[]` has 0 additional dimensions, `A[][][][]` has 3)
    pub additional_dimensions: usize,

    /// Underlying element type (`A` is the underlying element type of `A[][]`)
    pub element_type: T,
}

impl<T> ArrayType<T> {
    pub fn map<T2>(&self, map_element: impl FnOnce(&T) -> T2) -> ArrayType<T2> {
        ArrayType {
            additional_dimensions: self.additional_dimensions,
            element_type: map_element(&self.element_type),
        }
    }

    /// Total number of dimensions in the array type
    ///
    /// This is always just `additional_dimensions + 1`
    pub const fn dimensions(&self) -> usize {
        self.additional_dimensions + 1
    }
}

impl<T: RenderDescriptor> RenderDescriptor for ArrayType<T> {
    fn render_to(&self, write_to: &mut String) {
        for _ in 0..=self.additional_dimensions {
            write_to.push('[');
        }
        self.element_type.render_to(write_to);
    }
}

impl<T: ParseDescriptor> ParseDescriptor for ArrayType<T> {
    fn parse_from(source: &mut Peekable<Chars>) -> Result<Self> {
        let mut additional_dimensions = 0;
        while source.next_if_eq(&'[').is_some() {
            additional_dimensions += 1;
        }
        if additional_dimensions < 1 {
            let msg = "Expected at least on `[` for array type";
            return Err(Error::new(ErrorKind::InvalidInput, msg));
        }
        Ok(ArrayType {
            additional_dimensions: additional_dimensions - 1,
            element_type: T::parse_from(source)?,
        })
    }
}

impl RenderDescriptor for BinaryName {
    fn render_to(&self, write_to: &mut String) {
        write_to.push('L');
        write_to.push_str(self.as_str());
        write_to.push(';');
    }
}

impl ParseDescriptor for BinaryName {
    fn parse_from(source: &mut Peekable<Chars>) -> Result<Self> {
        if let Some('L') = source.next() {
            let mut class_name = String::new();
            loop {
                let c: char = source.next().ok_or_else(|| {
                    let msg = format!("Missing terminator for 'L{}'", class_name);
                    Error::new(ErrorKind::UnexpectedEof, msg)
                })?;
                if c == ';' {
                    return BinaryName::from_string(class_name)
                        .map_err(|msg| Error::new(ErrorKind::InvalidInput, msg));
                } else {
                    class_name.push(c)
                }
            }
        } else {
            Err(Error::new(
                ErrorKind::InvalidInput,
                "Expected object type to start with `L`",
            ))
        }
    }
}

impl<C: RenderDescriptor> RenderDescriptor for RefType<C> {
    fn render_to(&self, write_to: &mut String) {
        match self {
            RefType::Object(cls) => {
                cls.render_to(write_to);
            }
            RefType::PrimitiveArray(arr) => {
                arr.render_to(write_to);
            }
            RefType::ObjectArray(arr) => {
                arr.render_to(write_to);
            }
        }
    }
}

impl<C: ParseDescriptor> ParseDescriptor for RefType<C> {
    fn parse_from(source: &mut Peekable<Chars>) -> Result<Self> {
        Ok(match source.peek().copied() {
            Some('L') => RefType::Object(C::parse_from(source)?),
            Some('[') => {
                source.next();
                let mut additional_dimensions = 0;
                while let Some('[') = source.peek().copied() {
                    additional_dimensions += 1;
                    source.next();
                }
                if let Some('L') = source.peek().copied() {
                    RefType::ObjectArray(ArrayType {
                        additional_dimensions,
                        element_type: C::parse_from(source)?,
                    })
                } else {
                    RefType::PrimitiveArray(ArrayType {
                        additional_dimensions,
                        element_type: BaseType::parse_from(source)?,
                    })
                }
            }
            Some(c) => {
                let msg = format!("Invalid reference type character '{}'", c);
                return Err(Error::new(ErrorKind::InvalidInput, msg));
            }
            None => {
                let msg = "Missing field type";
                return Err(Error::new(ErrorKind::UnexpectedEof, msg));
            }
        })
    }
}

impl<C> RefType<C> {
    pub fn map<C2>(&self, map_class: impl FnOnce(&C) -> C2) -> RefType<C2> {
        match self {
            RefType::Object(cls) => RefType::Object(map_class(cls)),
            RefType::PrimitiveArray(arr) => RefType::PrimitiveArray(*arr),
            RefType::ObjectArray(arr) => RefType::ObjectArray(arr.map(map_class)),
        }
    }

    pub fn array(field_type: FieldType<C>) -> RefType<C> {
        match field_type {
            FieldType::Base(element_type) => RefType::PrimitiveArray(ArrayType {
                additional_dimensions: 0,
                element_type,
            }),
            FieldType::Ref(RefType::Object(element_type)) => RefType::ObjectArray(ArrayType {
                additional_dimensions: 0,
                element_type,
            }),
            FieldType::Ref(RefType::PrimitiveArray(arr)) => RefType::PrimitiveArray(ArrayType {
                additional_dimensions: arr.additional_dimensions + 1,
                element_type: arr.element_type,
            }),
            FieldType::Ref(RefType::ObjectArray(arr)) => RefType::ObjectArray(ArrayType {
                additional_dimensions: arr.additional_dimensions + 1,
                element_type: arr.element_type,
            }),
        }
    }
}

/// Type of a class, instance, or local variable
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum FieldType<Class> {
    Base(BaseType),
    Ref(RefType<Class>),
}

impl<C> Width for FieldType<C> {
    fn width(&self) -> usize {
        match self {
            FieldType::Base(base_type) => base_type.width(),
            FieldType::Ref(_) => 1,
        }
    }
}

impl<C> FieldType<C> {
    pub fn array(field_type: FieldType<C>) -> FieldType<C> {
        FieldType::Ref(RefType::array(field_type))
    }

    pub const fn object(class_name: C) -> FieldType<C> {
        FieldType::Ref(RefType::Object(class_name))
    }

    pub const fn int() -> FieldType<C> {
        FieldType::Base(BaseType::Int)
    }

    pub const fn long() -> FieldType<C> {
        FieldType::Base(BaseType::Long)
    }

    pub const fn float() -> FieldType<C> {
        FieldType::Base(BaseType::Float)
    }

    pub const fn double() -> FieldType<C> {
        FieldType::Base(BaseType::Double)
    }

    pub const fn char() -> FieldType<C> {
        FieldType::Base(BaseType::Char)
    }

    pub const fn short() -> FieldType<C> {
        FieldType::Base(BaseType::Short)
    }

    pub const fn byte() -> FieldType<C> {
        FieldType::Base(BaseType::Byte)
    }

    pub const fn boolean() -> FieldType<C> {
        FieldType::Base(BaseType::Boolean)
    }
}

impl<C: RenderDescriptor> RenderDescriptor for FieldType<C> {
    fn render_to(&self, write_to: &mut String) {
        match self {
            FieldType::Base(base_type) => base_type.render_to(write_to),
            FieldType::Ref(reference_type) => reference_type.render_to(write_to),
        }
    }
}

impl<C: ParseDescriptor> ParseDescriptor for FieldType<C> {
    fn parse_from(source: &mut Peekable<Chars>) -> Result<Self> {
        match source.peek().copied() {
            None => Err(Error::new(ErrorKind::UnexpectedEof, "Missing field type")),
            Some('B' | 'C' | 'D' | 'F' | 'I' | 'J' | 'S' | 'Z') => {
                BaseType::parse_from(source).map(FieldType::Base)
            }
            Some('L' | '[') => RefType::parse_from(source).map(FieldType::Ref),
            Some(c) => {
                let msg = format!("Invalid reference type character '{}'", c);
                Err(Error::new(ErrorKind::InvalidInput, msg))
            }
        }
    }
}

/// Signature of a method
#[derive(PartialEq, Eq, Hash, Debug, Clone)]
pub struct MethodDescriptor<Class> {
    pub parameters: Vec<FieldType<Class>>,
    pub return_type: Option<FieldType<Class>>, // `None` is for `void` (ie. no return)
}

impl<C> MethodDescriptor<C> {
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

impl<C: RenderDescriptor> RenderDescriptor for MethodDescriptor<C> {
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
}

impl<C: ParseDescriptor> ParseDescriptor for MethodDescriptor<C> {
    fn parse_from(source: &mut Peekable<Chars>) -> Result<Self> {
        // Assert open paren
        if let Some('(') = source.next() {
        } else {
            let msg = "Expected '(' for method";
            return Err(Error::new(ErrorKind::InvalidInput, msg));
        }

        // Parse parameters
        let mut parameters = vec![];
        while source.peek().copied() != Some(')') {
            parameters.push(FieldType::<C>::parse_from(source)?);
        }

        // Assert close paren
        if let Some(')') = source.next() {
        } else {
            let msg = "Expected ')' for method";
            return Err(Error::new(ErrorKind::InvalidInput, msg));
        }

        // Parse return
        let return_type = if let Some('V') = source.peek().copied() {
            let _ = source.next();
            None
        } else {
            Some(FieldType::<C>::parse_from(source)?)
        };

        Ok(MethodDescriptor {
            parameters,
            return_type,
        })
    }
}
/*
/// Any JVM signature
#[derive(PartialEq, Eq, Hash, Debug)]
pub enum JavaTypeSignature {
    Base(BaseType),
    Reference(ReferenceTypeSignature),
}

impl RenderDescriptor for JavaTypeSignature {
    fn render_to(&self, write_to: &mut String) {
        match self {
            JavaTypeSignature::Base(typ) => typ.render_to(write_to),
            JavaTypeSignature::Reference(typ) => typ.render_to(write_to),
        }
    }
}

impl ParseDescriptor for JavaTypeSignature {
    fn parse_from(source: &mut Peekable<Chars>) -> Result<Self> {
        match source.peek().copied() {
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

impl RenderDescriptor for ReferenceTypeSignature {
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
}

impl ParseDescriptor for ReferenceTypeSignature {
    fn parse_from(source: &mut Peekable<Chars>) -> Result<Self> {
        let sig = match source.peek() {
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

impl RenderDescriptor for ClassTypeSignature {
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
}

impl ParseDescriptor for ClassTypeSignature {
    fn parse_from(source: &mut Peekable<Chars>) -> Result<Self> {
        // Assert leading `L`
        if let Some('L') = source.next() {
        } else {
            let msg = "Expected 'L' for class type signature";
            return Err(Error::new(ErrorKind::InvalidInput, msg));
        }

        // TODO: filter out empty idents?
        fn parse_ident(source: &mut Peekable<Chars>) -> Result<(String, Option<char>)> {
            let mut name = String::new();
            loop {
                let c_opt = source.peek().copied();
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
                while source.peek().copied() != Some('>') {
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
        while let Some('.') = source.peek().copied() {
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

impl RenderDescriptor for SimpleClassTypeSignature {
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
}

impl ParseDescriptor for SimpleClassTypeSignature {
    fn parse_from(source: &mut Peekable<Chars>) -> Result<Self> {
        let mut name = String::new();
        let mut arguments = vec![];
        loop {
            match source.peek().copied() {
                Some('<') => {
                    while source.peek().copied() != Some('>') {
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

impl RenderDescriptor for TypeArgument {
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
}

impl ParseDescriptor for TypeArgument {
    fn parse_from(source: &mut Peekable<Chars>) -> Result<Self> {
        let ty_arg = match source.peek().copied() {
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
*/

#[cfg(test)]
mod test {
    use super::*;
    use std::fmt::Debug;

    fn round_trip<T: RenderDescriptor + ParseDescriptor + Debug + Eq>(rendered: &str, parsed: T) {
        assert_eq!(rendered, parsed.render());
        assert_eq!(T::parse(rendered).unwrap(), parsed);
    }

    type FT = FieldType<BinaryName>;

    const INT: FT = FieldType::Base(BaseType::Int);
    const DOUBLE: FT = FieldType::Base(BaseType::Double);
    const OBJECT: FT = FieldType::object(BinaryName::OBJECT);
    const STRING: FT = FieldType::object(BinaryName::STRING);
    const INTEGER: FT = FieldType::object(BinaryName::INTEGER);

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
        round_trip("I", INT);
        round_trip("Ljava/lang/Object;", OBJECT);
        round_trip(
            "[[[D",
            FieldType::array(FieldType::array(FieldType::array(DOUBLE))),
        );
        round_trip("[Ljava/lang/String;", FieldType::array(STRING));
    }

    #[test]
    fn method_descriptors() {
        round_trip(
            "(IDLjava/lang/Integer;)Ljava/lang/Object;",
            MethodDescriptor {
                parameters: vec![INT, DOUBLE, INTEGER],
                return_type: Some(OBJECT),
            },
        );
        round_trip(
            "()V",
            MethodDescriptor {
                parameters: Vec::<FT>::new(),
                return_type: None,
            },
        );
    }
}
