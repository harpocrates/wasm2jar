use super::{BinaryName, FieldType, MethodDescriptor, RefType, UnqualifiedName};
use std::collections::{HashMap, HashSet};

/// Tracks the relationships between classes/interfaces and the members on those classes
///
/// When generating multiple classes, it is quite convenient to maintain one unified graph of all
/// of the types/members in the generated code. Then, when a class needs to access some member, it
/// can import the necessary segment of the class graph into its constant pool.
pub struct ClassGraph {
    pub classes: HashMap<BinaryName, ClassData>,
}

impl ClassGraph {
    /// New empty graph
    pub fn new() -> ClassGraph {
        ClassGraph {
            classes: HashMap::new(),
        }
    }

    /// Query if one type is assignable to another
    ///
    /// This matches the semantics of the prolog predicate `isJavaAssignable(sub_type, super_type)`
    /// in the JVM verifier specification.
    ///
    /// Note: if some of the types queried aren't in `ClassGraph`, this may return false negatives.
    pub fn is_java_assignable(&self, sub_type: &RefType, super_type: &RefType) -> bool {
        match (sub_type, super_type) {
            // Special superclass and interfaces of all arrays
            (RefType::Array(_), RefType::Object(object_type)) => {
                object_type == &BinaryName::OBJECT
                    || object_type == &BinaryName::CLONEABLE
                    || object_type == &BinaryName::SERIALIZABLE
            }

            // Cursed (unsound) covariance of arrays
            (RefType::Array(elem_type1), RefType::Array(elem_type2)) => {
                self.is_field_type_assignable(&elem_type1, &elem_type2)
            }

            // Object-to-object assignability holds if there is a path through super type edges
            (RefType::Object(elem_type1), RefType::Object(elem_type2)) => {
                let mut supertypes_to_visit: Vec<&BinaryName> = vec![elem_type1];
                let mut dont_revisit: HashSet<&BinaryName> =
                    supertypes_to_visit.iter().cloned().collect();

                // Optimization: if the super type is a class, then skip visiting interfaces
                let super_is_class: bool = match self.classes.get(elem_type2) {
                    None => false,
                    Some(ClassData { is_interface, .. }) => !is_interface,
                };

                while let Some(next_supertype) = supertypes_to_visit.pop() {
                    if next_supertype == elem_type2 {
                        return true;
                    } else if let Some(class_data) = self.classes.get(next_supertype) {
                        if let Some(superclass) = &class_data.superclass {
                            if dont_revisit.insert(&superclass) {
                                supertypes_to_visit.push(&superclass);
                            }
                        }
                        if !super_is_class {
                            for interface in &class_data.interfaces {
                                if dont_revisit.insert(&interface) {
                                    supertypes_to_visit.push(&interface);
                                }
                            }
                        }
                    }
                }

                false
            }

            _ => false,
        }
    }

    fn is_field_type_assignable(&self, sub_type: &FieldType, super_type: &FieldType) -> bool {
        match (sub_type, super_type) {
            (FieldType::Base(base1), FieldType::Base(base2)) => base1 == base2,
            (FieldType::Ref(ref1), FieldType::Ref(ref2)) => self.is_java_assignable(ref1, ref2),
            (_, _) => false,
        }
    }

    /// Add standard types to the class graph
    pub fn insert_lang_types(&mut self) {
        // java.lang.Object
        {
            let java_lang_object = self.classes.entry(BinaryName::OBJECT).or_insert(ClassData {
                superclass: None,
                interfaces: HashSet::new(),
                is_interface: false,
                methods: HashMap::new(),
                fields: HashMap::new(),
            });
            java_lang_object.add_method(
                false,
                UnqualifiedName::EQUALS,
                MethodDescriptor {
                    parameters: vec![FieldType::Ref(RefType::OBJECT)],
                    return_type: Some(FieldType::BOOLEAN),
                },
            );
            java_lang_object.add_method(
                false,
                UnqualifiedName::HASHCODE,
                MethodDescriptor {
                    parameters: vec![FieldType::Ref(RefType::OBJECT)],
                    return_type: Some(FieldType::INT),
                },
            );
            java_lang_object.add_method(
                false,
                UnqualifiedName::INIT,
                MethodDescriptor {
                    parameters: vec![],
                    return_type: None,
                },
            );
        }

        // java.lang.CharSequence
        {
            let java_lang_charsequence = self
                .classes
                .entry(BinaryName::CHARSEQUENCE)
                .or_insert(ClassData::new(BinaryName::OBJECT, true));
            java_lang_charsequence.add_method(
                false,
                UnqualifiedName::LENGTH,
                MethodDescriptor {
                    parameters: vec![],
                    return_type: Some(FieldType::INT),
                },
            );
        }

        // java.lang.String
        {
            let java_lang_string = self
                .classes
                .entry(BinaryName::STRING)
                .or_insert(ClassData::new(BinaryName::OBJECT, false));
            java_lang_string.interfaces.insert(BinaryName::CHARSEQUENCE);
            java_lang_string.add_method(
                false,
                UnqualifiedName::GETBYTES,
                MethodDescriptor {
                    parameters: vec![FieldType::Ref(RefType::STRING)],
                    return_type: None,
                },
            );
        }

        // java.lang.Class
        {
            let _java_lang_class = self
                .classes
                .entry(BinaryName::CLASS)
                .or_insert(ClassData::new(BinaryName::OBJECT, false));
        }

        // java.lang.invoke.MethodType
        {
            let _java_lang_invoke_methodtype = self
                .classes
                .entry(BinaryName::METHODTYPE)
                .or_insert(ClassData::new(BinaryName::OBJECT, false));
        }

        // java.lang.invoke.MethodHandle
        {
            let _java_lang_invoke_methodtype = self
                .classes
                .entry(BinaryName::METHODHANDLE)
                .or_insert(ClassData::new(BinaryName::OBJECT, false));
        }

        // java.lang.Number
        {
            let java_lang_number = self
                .classes
                .entry(BinaryName::NUMBER)
                .or_insert(ClassData::new(BinaryName::OBJECT, false));
            for (extractor, extracted_type) in vec![
                (UnqualifiedName::BYTEVALUE, FieldType::BYTE),
                (UnqualifiedName::DOUBLEVALUE, FieldType::DOUBLE),
                (UnqualifiedName::FLOATVALUE, FieldType::FLOAT),
                (UnqualifiedName::INTVALUE, FieldType::INT),
                (UnqualifiedName::LONGVALUE, FieldType::LONG),
                (UnqualifiedName::SHORTVALUE, FieldType::SHORT),
            ] {
                java_lang_number.add_method(
                    false,
                    extractor,
                    MethodDescriptor {
                        parameters: vec![],
                        return_type: Some(extracted_type),
                    },
                );
            }
        }

        // java.lang.Integer
        {
            let java_lang_integer = self
                .classes
                .entry(BinaryName::INTEGER)
                .or_insert(ClassData::new(BinaryName::NUMBER, false));
            for (name, output_ty) in vec![
                (UnqualifiedName::VALUEOF, FieldType::Ref(RefType::INTEGER)),
                (UnqualifiedName::BITCOUNT, FieldType::INT),
                (UnqualifiedName::NUMBEROFLEADINGZEROS, FieldType::INT),
                (UnqualifiedName::NUMBEROFTRAILINGZEROS, FieldType::INT),
            ] {
                java_lang_integer.add_method(
                    true,
                    name,
                    MethodDescriptor {
                        parameters: vec![FieldType::INT],
                        return_type: Some(output_ty),
                    },
                );
            }
            for name in vec![
                UnqualifiedName::COMPARE,
                UnqualifiedName::COMPAREUNSIGNED,
                UnqualifiedName::DIVIDEUNSIGNED,
                UnqualifiedName::REMAINDERUNSIGNED,
                UnqualifiedName::ROTATELEFT,
                UnqualifiedName::ROTATERIGHT,
            ] {
                java_lang_integer.add_method(
                    true,
                    name,
                    MethodDescriptor {
                        parameters: vec![FieldType::INT, FieldType::INT],
                        return_type: Some(FieldType::INT),
                    },
                );
            }
            for name in vec![UnqualifiedName::MAXVALUE, UnqualifiedName::MINVALUE] {
                java_lang_integer.add_field(true, name, FieldType::INT)
            }
        }

        // java.lang.Float
        {
            let java_lang_float = self
                .classes
                .entry(BinaryName::FLOAT)
                .or_insert(ClassData::new(BinaryName::NUMBER, false));
            for (name, input_ty, output_ty) in vec![
                (
                    UnqualifiedName::VALUEOF,
                    FieldType::FLOAT,
                    FieldType::Ref(RefType::FLOAT),
                ),
                (
                    UnqualifiedName::FLOATTORAWINTBITS,
                    FieldType::FLOAT,
                    FieldType::INT,
                ),
                (
                    UnqualifiedName::INTBITSTOFLOAT,
                    FieldType::INT,
                    FieldType::FLOAT,
                ),
            ] {
                java_lang_float.add_method(
                    true,
                    name,
                    MethodDescriptor {
                        parameters: vec![input_ty],
                        return_type: Some(output_ty),
                    },
                );
            }
            for name in vec![UnqualifiedName::MAX, UnqualifiedName::MIN] {
                java_lang_float.add_method(
                    true,
                    name,
                    MethodDescriptor {
                        parameters: vec![FieldType::FLOAT, FieldType::FLOAT],
                        return_type: Some(FieldType::FLOAT),
                    },
                );
            }
            for name in vec![
                UnqualifiedName::MAXVALUE,
                UnqualifiedName::MINVALUE,
                UnqualifiedName::NAN,
                UnqualifiedName::NEGATIVEINFINITY,
                UnqualifiedName::POSITIVEINFINITY,
            ] {
                java_lang_float.add_field(true, name, FieldType::FLOAT)
            }
        }

        // java.lang.Long
        {
            let java_lang_long = self
                .classes
                .entry(BinaryName::LONG)
                .or_insert(ClassData::new(BinaryName::NUMBER, false));
            for (name, output_ty) in vec![
                (UnqualifiedName::VALUEOF, FieldType::Ref(RefType::LONG)),
                (UnqualifiedName::BITCOUNT, FieldType::INT),
                (UnqualifiedName::NUMBEROFLEADINGZEROS, FieldType::INT),
                (UnqualifiedName::NUMBEROFTRAILINGZEROS, FieldType::INT),
            ] {
                java_lang_long.add_method(
                    true,
                    name,
                    MethodDescriptor {
                        parameters: vec![FieldType::LONG],
                        return_type: Some(output_ty),
                    },
                );
            }
            for (name, input_tys, output_ty) in vec![
                (
                    UnqualifiedName::COMPARE,
                    vec![FieldType::LONG, FieldType::LONG],
                    FieldType::INT,
                ),
                (
                    UnqualifiedName::COMPAREUNSIGNED,
                    vec![FieldType::LONG, FieldType::LONG],
                    FieldType::INT,
                ),
                (
                    UnqualifiedName::DIVIDEUNSIGNED,
                    vec![FieldType::LONG, FieldType::LONG],
                    FieldType::LONG,
                ),
                (
                    UnqualifiedName::REMAINDERUNSIGNED,
                    vec![FieldType::LONG, FieldType::LONG],
                    FieldType::LONG,
                ),
                (
                    UnqualifiedName::ROTATELEFT,
                    vec![FieldType::LONG, FieldType::INT],
                    FieldType::LONG,
                ),
                (
                    UnqualifiedName::ROTATERIGHT,
                    vec![FieldType::LONG, FieldType::INT],
                    FieldType::LONG,
                ),
            ] {
                java_lang_long.add_method(
                    true,
                    name,
                    MethodDescriptor {
                        parameters: input_tys,
                        return_type: Some(output_ty),
                    },
                );
            }
            for name in vec![UnqualifiedName::MAXVALUE, UnqualifiedName::MINVALUE] {
                java_lang_long.add_field(true, name, FieldType::LONG)
            }
        }

        // java.lang.Double
        {
            let java_lang_double = self
                .classes
                .entry(BinaryName::DOUBLE)
                .or_insert(ClassData::new(BinaryName::NUMBER, false));
            for (name, input_ty, output_ty) in vec![
                (
                    UnqualifiedName::VALUEOF,
                    FieldType::DOUBLE,
                    FieldType::Ref(RefType::DOUBLE),
                ),
                (
                    UnqualifiedName::DOUBLETORAWLONGBITS,
                    FieldType::DOUBLE,
                    FieldType::LONG,
                ),
                (
                    UnqualifiedName::LONGBITSTODOUBLE,
                    FieldType::LONG,
                    FieldType::DOUBLE,
                ),
            ] {
                java_lang_double.add_method(
                    true,
                    name,
                    MethodDescriptor {
                        parameters: vec![input_ty],
                        return_type: Some(output_ty),
                    },
                );
            }
            for name in vec![UnqualifiedName::MAX, UnqualifiedName::MIN] {
                java_lang_double.add_method(
                    true,
                    name,
                    MethodDescriptor {
                        parameters: vec![FieldType::DOUBLE, FieldType::DOUBLE],
                        return_type: Some(FieldType::DOUBLE),
                    },
                );
            }
            for name in vec![
                UnqualifiedName::MAXVALUE,
                UnqualifiedName::MINVALUE,
                UnqualifiedName::NAN,
                UnqualifiedName::NEGATIVEINFINITY,
                UnqualifiedName::POSITIVEINFINITY,
            ] {
                java_lang_double.add_field(true, name, FieldType::FLOAT)
            }
        }

        // java.lang.Math
        {
            let java_lang_math = self
                .classes
                .entry(BinaryName::MATH)
                .or_insert(ClassData::new(BinaryName::OBJECT, false));
            for name in vec![
                UnqualifiedName::CEIL,
                UnqualifiedName::FLOOR,
                UnqualifiedName::SQRT,
                UnqualifiedName::RINT,
            ] {
                java_lang_math.add_method(
                    true,
                    name,
                    MethodDescriptor {
                        parameters: vec![FieldType::DOUBLE],
                        return_type: Some(FieldType::DOUBLE),
                    },
                );
            }
            for input_output_ty in &[FieldType::FLOAT, FieldType::DOUBLE] {
                java_lang_math.add_method(
                    true,
                    UnqualifiedName::COPYSIGN,
                    MethodDescriptor {
                        parameters: vec![input_output_ty.clone(), input_output_ty.clone()],
                        return_type: Some(input_output_ty.clone()),
                    },
                );
                java_lang_math.add_method(
                    true,
                    UnqualifiedName::ABS,
                    MethodDescriptor {
                        parameters: vec![input_output_ty.clone()],
                        return_type: Some(input_output_ty.clone()),
                    },
                );
            }
            java_lang_math.add_method(
                true,
                UnqualifiedName::TOINTEXACT,
                MethodDescriptor {
                    parameters: vec![FieldType::LONG],
                    return_type: Some(FieldType::INT),
                },
            );
        }
    }

    /// Add standard exception/error types to the class graph
    pub fn insert_error_types(&mut self) {
        // java.lang.Throwable
        {
            let java_lang_throwable = self
                .classes
                .entry(BinaryName::THROWABLE)
                .or_insert(ClassData::new(BinaryName::OBJECT, false));
            java_lang_throwable.add_method(
                false,
                UnqualifiedName::INIT,
                MethodDescriptor {
                    parameters: vec![FieldType::Ref(RefType::STRING)],
                    return_type: None,
                },
            );
        }

        // java.lang.Error
        {
            let java_lang_error = self
                .classes
                .entry(BinaryName::ERROR)
                .or_insert(ClassData::new(BinaryName::THROWABLE, false));
            java_lang_error.add_method(
                false,
                UnqualifiedName::INIT,
                MethodDescriptor {
                    parameters: vec![FieldType::Ref(RefType::STRING)],
                    return_type: None,
                },
            );
        }

        // java.lang.AssertionError
        {
            let java_lang_assertionerror = self
                .classes
                .entry(BinaryName::ASSERTIONERROR)
                .or_insert(ClassData::new(BinaryName::ERROR, false));
            java_lang_assertionerror.add_method(
                false,
                UnqualifiedName::INIT,
                MethodDescriptor {
                    parameters: vec![FieldType::Ref(RefType::STRING)],
                    return_type: None,
                },
            );
        }

        // java.lang.Exception
        {
            let java_lang_error = self
                .classes
                .entry(BinaryName::EXCEPTION)
                .or_insert(ClassData::new(BinaryName::THROWABLE, false));
            java_lang_error.add_method(
                false,
                UnqualifiedName::INIT,
                MethodDescriptor {
                    parameters: vec![FieldType::Ref(RefType::STRING)],
                    return_type: None,
                },
            );
        }

        // java.lang.RuntimeException
        {
            let java_lang_error = self
                .classes
                .entry(BinaryName::RUNTIMEEXCEPTION)
                .or_insert(ClassData::new(BinaryName::EXCEPTION, false));
            java_lang_error.add_method(
                false,
                UnqualifiedName::INIT,
                MethodDescriptor {
                    parameters: vec![FieldType::Ref(RefType::STRING)],
                    return_type: None,
                },
            );
        }

        // java.lang.ArithmeticException
        {
            let java_lang_error = self
                .classes
                .entry(BinaryName::ARITHMETICEXCEPTION)
                .or_insert(ClassData::new(BinaryName::RUNTIMEEXCEPTION, false));
            java_lang_error.add_method(
                false,
                UnqualifiedName::INIT,
                MethodDescriptor {
                    parameters: vec![FieldType::Ref(RefType::STRING)],
                    return_type: None,
                },
            );
        }
    }

    /// Add standard util types to the class graph
    pub fn insert_util_types(&mut self) {
        // java.util.Arrays
        {
            let java_util_arrays = self
                .classes
                .entry(BinaryName::ARRAYS)
                .or_insert(ClassData::new(BinaryName::OBJECT, false));
            java_util_arrays.add_method(
                true,
                UnqualifiedName::COPYOF,
                MethodDescriptor {
                    parameters: vec![FieldType::array(FieldType::OBJECT), FieldType::INT],
                    return_type: Some(FieldType::array(FieldType::OBJECT)),
                },
            );
            java_util_arrays.add_method(
                true,
                UnqualifiedName::FILL,
                MethodDescriptor {
                    parameters: vec![FieldType::array(FieldType::OBJECT), FieldType::OBJECT],
                    return_type: None,
                },
            );
        }
    }
}

// TODO: should we track subclasses?
pub struct ClassData {
    /// Superclass is only ever `null` for `java/lang/Object` itself
    pub superclass: Option<BinaryName>,

    /// Interfaces implemented (or super-interfaces)
    pub interfaces: HashSet<BinaryName>,

    /// Is this an interface?
    pub is_interface: bool,

    /// Methods
    pub methods: HashMap<UnqualifiedName, HashMap<MethodDescriptor, bool>>,

    /// Fields
    pub fields: HashMap<UnqualifiedName, (bool, FieldType)>,
}

impl ClassData {
    pub fn new(superclass: BinaryName, is_interface: bool) -> ClassData {
        ClassData {
            superclass: Some(superclass),
            interfaces: HashSet::new(),
            is_interface,
            methods: HashMap::new(),
            fields: HashMap::new(),
        }
    }

    pub fn add_interfaces(&mut self, interfaces: impl IntoIterator<Item = BinaryName>) {
        self.interfaces.extend(interfaces);
    }

    pub fn add_field(&mut self, is_static: bool, name: UnqualifiedName, descriptor: FieldType) {
        self.fields.insert(name, (is_static, descriptor));
    }

    pub fn add_method(
        &mut self,
        is_static: bool,
        name: UnqualifiedName,
        descriptor: MethodDescriptor,
    ) {
        self.methods
            .entry(name)
            .or_insert(HashMap::new())
            .insert(descriptor, is_static);
    }
}
