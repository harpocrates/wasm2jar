use super::{FieldType, MethodDescriptor, RefType};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

/// Tracks the relationships between classes/interfaces and the members on those classes
///
/// When generating multiple classes, it is quite convenient to maintain one unified graph of all
/// of the types/members in the generated code. Then, when a class needs to access some member, it
/// can import the necessary segment of the class graph into its constant pool.
pub struct ClassGraph {
    pub classes: HashMap<Cow<'static, str>, ClassData>,
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
                object_type == RefType::OBJECT_NAME
                    || object_type == RefType::CLONEABLE_NAME
                    || object_type == RefType::SERIALIZABLE_NAME
            }

            // Cursed (unsound) covariance of arrays
            (RefType::Array(elem_type1), RefType::Array(elem_type2)) => {
                self.is_field_type_assignable(&elem_type1, &elem_type2)
            }

            // Object-to-object assignability holds if there is a path through super type edges
            (RefType::Object(elem_type1), RefType::Object(elem_type2)) => {
                let mut supertypes_to_visit: Vec<&str> = vec![elem_type1];
                let mut dont_revisit: HashSet<&str> = supertypes_to_visit.iter().cloned().collect();

                // Optimization: if the super type is a class, then skip visiting interfaces
                let super_is_class: bool = match self.classes.get(elem_type2.as_ref()) {
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
            let java_lang_object = self
                .classes
                .entry(Cow::Borrowed(RefType::OBJECT_NAME))
                .or_insert(ClassData {
                    superclass: None,
                    interfaces: HashSet::new(),
                    is_interface: false,
                    methods: HashMap::new(),
                    fields: HashMap::new(),
                });
            java_lang_object.add_method(
                false,
                "equals",
                MethodDescriptor {
                    parameters: vec![FieldType::Ref(RefType::OBJECT_CLASS)],
                    return_type: Some(FieldType::BOOLEAN),
                },
            );
            java_lang_object.add_method(
                false,
                "hashCode",
                MethodDescriptor {
                    parameters: vec![FieldType::Ref(RefType::OBJECT_CLASS)],
                    return_type: Some(FieldType::INT),
                },
            );
            java_lang_object.add_method(
                false,
                "<init>",
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
                .entry(Cow::Borrowed(RefType::CHARSEQUENCE_NAME))
                .or_insert(ClassData::new(RefType::OBJECT_NAME, true));
            java_lang_charsequence.add_method(
                false,
                "length",
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
                .entry(Cow::Borrowed(RefType::STRING_NAME))
                .or_insert(ClassData::new(RefType::OBJECT_NAME, false));
            java_lang_string.add_interfaces(vec![RefType::CHARSEQUENCE_NAME]);
            java_lang_string.add_method(
                false,
                "getBytes",
                MethodDescriptor {
                    parameters: vec![FieldType::Ref(RefType::STRING_CLASS)],
                    return_type: None,
                },
            );
        }

        // java.lang.Class
        {
            let _java_lang_class = self
                .classes
                .entry(Cow::Borrowed(RefType::CLASS_NAME))
                .or_insert(ClassData::new(RefType::OBJECT_NAME, false));
        }

        // java.lang.invoke.MethodType
        {
            let _java_lang_invoke_methodtype = self
                .classes
                .entry(Cow::Borrowed(RefType::METHOD_TYPE_NAME))
                .or_insert(ClassData::new(RefType::OBJECT_NAME, false));
        }

        // java.lang.invoke.MethodHandle
        {
            let _java_lang_invoke_methodtype = self
                .classes
                .entry(Cow::Borrowed(RefType::METHOD_HANDLE_NAME))
                .or_insert(ClassData::new(RefType::OBJECT_NAME, false));
        }

        // java.lang.Number
        {
            let java_lang_number = self
                .classes
                .entry(Cow::Borrowed(RefType::NUMBER_NAME))
                .or_insert(ClassData::new(RefType::OBJECT_NAME, false));
            for (extractor, extracted_type) in vec![
                ("byteValue", FieldType::BYTE),
                ("doubleValue", FieldType::DOUBLE),
                ("floatValue", FieldType::FLOAT),
                ("intValue", FieldType::INT),
                ("longValue", FieldType::LONG),
                ("shortValue", FieldType::SHORT),
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
                .entry(Cow::Borrowed(RefType::INTEGER_NAME))
                .or_insert(ClassData::new(RefType::NUMBER_NAME, false));
            for (name, output_ty) in vec![
                ("valueOf", FieldType::object(RefType::INTEGER_NAME)),
                ("bitCount", FieldType::INT),
                ("numberOfLeadingZeros", FieldType::INT),
                ("numberOfTrailingZeros", FieldType::INT),
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
                "compare",
                "compareUnsigned",
                "divideUnsigned",
                "remainderUnsigned",
                "rotateLeft",
                "rotateRight",
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
        }

        // java.lang.Float
        {
            let java_lang_float = self
                .classes
                .entry(Cow::Borrowed(RefType::FLOAT_NAME))
                .or_insert(ClassData::new(RefType::NUMBER_NAME, false));
            for (name, input_ty, output_ty) in vec![
                (
                    "valueOf",
                    FieldType::FLOAT,
                    FieldType::object(RefType::FLOAT_NAME),
                ),
                ("floatToRawIntBits", FieldType::FLOAT, FieldType::INT),
                ("intBitsToFloat", FieldType::INT, FieldType::FLOAT),
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
            for name in vec!["max", "min"] {
                java_lang_float.add_method(
                    true,
                    name,
                    MethodDescriptor {
                        parameters: vec![FieldType::FLOAT, FieldType::FLOAT],
                        return_type: Some(FieldType::FLOAT),
                    },
                );
            }
        }

        // java.lang.Long
        {
            let java_lang_long = self
                .classes
                .entry(Cow::Borrowed(RefType::LONG_NAME))
                .or_insert(ClassData::new(RefType::NUMBER_NAME, false));
            for (name, output_ty) in vec![
                ("valueOf", FieldType::object(RefType::LONG_NAME)),
                ("bitCount", FieldType::INT),
                ("numberOfLeadingZeros", FieldType::INT),
                ("numberOfTrailingZeros", FieldType::INT),
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
                    "compare",
                    vec![FieldType::LONG, FieldType::LONG],
                    FieldType::INT,
                ),
                (
                    "compareUnsigned",
                    vec![FieldType::LONG, FieldType::LONG],
                    FieldType::INT,
                ),
                (
                    "divideUnsigned",
                    vec![FieldType::LONG, FieldType::LONG],
                    FieldType::LONG,
                ),
                (
                    "remainderUnsigned",
                    vec![FieldType::LONG, FieldType::LONG],
                    FieldType::LONG,
                ),
                (
                    "rotateLeft",
                    vec![FieldType::LONG, FieldType::INT],
                    FieldType::LONG,
                ),
                (
                    "rotateRight",
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
        }

        // java.lang.Double
        {
            let java_lang_double = self
                .classes
                .entry(Cow::Borrowed(RefType::DOUBLE_NAME))
                .or_insert(ClassData::new(RefType::NUMBER_NAME, false));
            for (name, input_ty, output_ty) in vec![
                (
                    "valueOf",
                    FieldType::DOUBLE,
                    FieldType::object(RefType::DOUBLE_NAME),
                ),
                ("doubleToRawLongBits", FieldType::DOUBLE, FieldType::LONG),
                ("longBitsToDouble", FieldType::LONG, FieldType::DOUBLE),
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
            for name in vec!["max", "min"] {
                java_lang_double.add_method(
                    true,
                    name,
                    MethodDescriptor {
                        parameters: vec![FieldType::DOUBLE, FieldType::DOUBLE],
                        return_type: Some(FieldType::DOUBLE),
                    },
                );
            }
        }

        // java.lang.Math
        {
            let java_lang_math = self
                .classes
                .entry(Cow::Borrowed(RefType::MATH_NAME))
                .or_insert(ClassData::new(RefType::OBJECT_NAME, false));
            for name in vec!["ceil", "floor", "sqrt", "rint"] {
                java_lang_math.add_method(
                    true,
                    name,
                    MethodDescriptor {
                        parameters: vec![FieldType::DOUBLE],
                        return_type: Some(FieldType::DOUBLE),
                    },
                );
            }
            for input_output_ty in vec![FieldType::FLOAT, FieldType::DOUBLE] {
                java_lang_math.add_method(
                    true,
                    "copySign",
                    MethodDescriptor {
                        parameters: vec![input_output_ty.clone(), input_output_ty.clone()],
                        return_type: Some(input_output_ty),
                    },
                );
            }
            java_lang_math.add_method(
                true,
                "toIntExact",
                MethodDescriptor {
                    parameters: vec![FieldType::LONG],
                    return_type: Some(FieldType::INT),
                },
            );
        }
    }
}

// TODO: should we track subclasses?
pub struct ClassData {
    /// Superclass is only ever `null` for `java/lang/Object` itself
    pub superclass: Option<Cow<'static, str>>,

    /// Interfaces implemented (or super-interfaces)
    pub interfaces: HashSet<Cow<'static, str>>,

    /// Is this an interface?
    pub is_interface: bool,

    /// Methods
    pub methods: HashMap<Cow<'static, str>, HashMap<MethodDescriptor, bool>>,

    /// Fields
    pub fields: HashMap<Cow<'static, str>, (bool, FieldType)>,
}

impl ClassData {
    pub fn new<S: Into<Cow<'static, str>>>(superclass: S, is_interface: bool) -> ClassData {
        let superclass = Some(superclass.into());
        ClassData {
            superclass,
            interfaces: HashSet::new(),
            is_interface,
            methods: HashMap::new(),
            fields: HashMap::new(),
        }
    }

    pub fn add_interfaces<S>(&mut self, interfaces: impl IntoIterator<Item = S>)
    where
        S: Into<Cow<'static, str>>,
    {
        self.interfaces
            .extend(interfaces.into_iter().map(|s| s.into()));
    }

    pub fn add_field<S>(&mut self, is_static: bool, name: S, descriptor: FieldType)
    where
        S: Into<Cow<'static, str>>,
    {
        self.fields.insert(name.into(), (is_static, descriptor));
    }

    pub fn add_method<S>(&mut self, is_static: bool, name: S, descriptor: MethodDescriptor)
    where
        S: Into<Cow<'static, str>>,
    {
        self.methods
            .entry(name.into())
            .or_insert(HashMap::new())
            .insert(descriptor, is_static);
    }
}
