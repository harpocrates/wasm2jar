use super::{
    BinaryName, Descriptor, Error, FieldType, InvokeType, MethodDescriptor, RefType,
    UnqualifiedName,
};
use elsa::map::FrozenMap;
use elsa::FrozenVec;
use std::collections::HashSet;

/// Tracks the relationships between classes/interfaces and the members on those classes
///
/// When generating multiple classes, it is quite convenient to maintain one unified graph of all
/// of the types/members in the generated code. Then, when a class needs to access some member, it
/// can import the necessary segment of the class graph into its constant pool.
pub struct ClassGraph {
    classes: FrozenMap<BinaryName, Box<ClassData>>,
}

impl ClassGraph {
    /// New empty graph
    pub fn new() -> ClassGraph {
        ClassGraph {
            classes: FrozenMap::new(),
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

    /// Look for a method of a given name in a class (or its super classes)
    pub fn lookup_method(
        &self,
        class_name: &BinaryName,
        method_name: &UnqualifiedName,
    ) -> Result<(InvokeType, MethodDescriptor), Error> {
        let mut next_class_name_opt = Some(class_name);
        while let Some(class_name) = next_class_name_opt.take() {
            let class = self
                .classes
                .get(class_name)
                .ok_or_else(|| Error::MissingClass(class_name.clone()))?;
            let is_interface = class.is_interface;
            let mut method_overloads = class
                .methods
                .iter()
                .filter(|(name, _, _)| name == method_name)
                .map(|(_, desc, is_static)| {
                    let typ = if *is_static {
                        InvokeType::Static
                    } else if method_name == &UnqualifiedName::INIT
                        || method_name == &UnqualifiedName::CLINIT
                    {
                        InvokeType::Special
                    } else if is_interface {
                        let n = desc.parameter_length(true) as u8;
                        InvokeType::Interface(n)
                    } else {
                        InvokeType::Virtual
                    };
                    (typ, desc.clone())
                })
                .collect::<Vec<_>>();

            if method_overloads.len() == 1 {
                return Ok(method_overloads.pop().unwrap());
            } else if method_overloads.len() == 0 {
                next_class_name_opt = class.superclass.as_ref();
            } else {
                let mut alts = String::new();
                for (_, alt) in &method_overloads {
                    if !alts.is_empty() {
                        alts.push_str(", ");
                    }
                    alts.push_str(&alt.render());
                }
                log::error!(
                    "Ambiguous overloads for {:?}.{:?}: {}",
                    class_name,
                    method_name,
                    alts
                );
                return Err(Error::AmbiguousMethod(
                    class_name.clone(),
                    method_name.clone(),
                ));
            }
        }

        Err(Error::MissingMember(
            class_name.clone(),
            method_name.clone(),
        ))
    }

    /// Look for a field of a given name in a class (or its super classes) and return whether it is
    /// static as well as the descriptor
    pub fn lookup_field(
        &self,
        class_name: &BinaryName,
        field_name: &UnqualifiedName,
    ) -> Result<(bool, FieldType), Error> {
        let mut next_class_name_opt = Some(class_name);
        while let Some(class_name) = next_class_name_opt.take() {
            let class = self
                .classes
                .get(class_name)
                .ok_or_else(|| Error::MissingClass(class_name.clone()))?;
            if let Some(matching_field) = class.fields.get(field_name) {
                return Ok(matching_field.clone());
            } else {
                next_class_name_opt = class.superclass.as_ref();
            }
        }

        Err(Error::MissingMember(class_name.clone(), field_name.clone()))
    }

    // TODO: remove uses of this
    pub fn lookup_class(&self, name: &BinaryName) -> Option<&ClassData> {
        self.classes.get(name)
    }

    /// Add a new class to the class graph
    pub fn add_class(&self, name: BinaryName, data: ClassData) -> &ClassData {
        self.classes.insert(name, Box::new(data))
    }

    /// Add standard types to the class graph
    pub fn insert_lang_types(&self) {
        // java.lang.Object
        {
            let java_lang_object = ClassData {
                superclass: None,
                interfaces: HashSet::new(),
                is_interface: false,
                methods: FrozenVec::new(),
                fields: FrozenMap::new(),
            };
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
                    parameters: vec![],
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
            self.add_class(BinaryName::OBJECT, java_lang_object);
        }

        // java.lang.CharSequence
        {
            let java_lang_charsequence = ClassData::new(BinaryName::OBJECT, true);
            java_lang_charsequence.add_method(
                false,
                UnqualifiedName::LENGTH,
                MethodDescriptor {
                    parameters: vec![],
                    return_type: Some(FieldType::INT),
                },
            );
            self.add_class(BinaryName::CHARSEQUENCE, java_lang_charsequence);
        }

        // java.lang.String
        {
            let mut java_lang_string = ClassData::new(BinaryName::OBJECT, false);
            java_lang_string.interfaces.insert(BinaryName::CHARSEQUENCE);
            java_lang_string.add_method(
                false,
                UnqualifiedName::GETBYTES,
                MethodDescriptor {
                    parameters: vec![FieldType::Ref(RefType::STRING)],
                    return_type: Some(FieldType::array(FieldType::BYTE)),
                },
            );
            self.add_class(BinaryName::STRING, java_lang_string);
        }

        // java.lang.Class
        {
            let java_lang_class = ClassData::new(BinaryName::OBJECT, false);
            self.add_class(BinaryName::CLASS, java_lang_class);
        }

        // java.lang.invoke.MethodType
        {
            let java_lang_invoke_methodtype = ClassData::new(BinaryName::OBJECT, false);
            java_lang_invoke_methodtype.add_method(
                false,
                UnqualifiedName::PARAMETERCOUNT,
                MethodDescriptor {
                    parameters: vec![],
                    return_type: Some(FieldType::INT),
                },
            );
            java_lang_invoke_methodtype.add_method(
                false,
                UnqualifiedName::PARAMETERTYPE,
                MethodDescriptor {
                    parameters: vec![FieldType::INT],
                    return_type: Some(FieldType::Ref(RefType::CLASS)),
                },
            );
            java_lang_invoke_methodtype.add_method(
                false,
                UnqualifiedName::PARAMETERARRAY,
                MethodDescriptor {
                    parameters: vec![],
                    return_type: Some(FieldType::array(FieldType::Ref(RefType::CLASS))),
                },
            );
            java_lang_invoke_methodtype.add_method(
                false,
                UnqualifiedName::DROPPARAMETERTYPES,
                MethodDescriptor {
                    parameters: vec![FieldType::INT, FieldType::INT],
                    return_type: Some(FieldType::Ref(RefType::METHODTYPE)),
                },
            );
            java_lang_invoke_methodtype.add_method(
                false,
                UnqualifiedName::RETURNTYPE,
                MethodDescriptor {
                    parameters: vec![],
                    return_type: Some(FieldType::Ref(RefType::CLASS)),
                },
            );
            java_lang_invoke_methodtype.add_method(
                true,
                UnqualifiedName::METHODTYPE,
                MethodDescriptor {
                    parameters: vec![
                        FieldType::Ref(RefType::CLASS),
                        FieldType::array(FieldType::Ref(RefType::CLASS)),
                    ],
                    return_type: Some(FieldType::Ref(RefType::METHODTYPE)),
                },
            );
            self.add_class(BinaryName::METHODTYPE, java_lang_invoke_methodtype);
        }

        // java.lang.invoke.MethodHandle
        {
            let java_lang_invoke_methodhandle = ClassData::new(BinaryName::OBJECT, false);
            java_lang_invoke_methodhandle.add_method(
                false,
                UnqualifiedName::TYPE,
                MethodDescriptor {
                    parameters: vec![],
                    return_type: Some(FieldType::Ref(RefType::METHODTYPE)),
                },
            );
            java_lang_invoke_methodhandle.add_method(
                false,
                UnqualifiedName::ASTYPE,
                MethodDescriptor {
                    parameters: vec![FieldType::Ref(RefType::METHODTYPE)],
                    return_type: Some(FieldType::Ref(RefType::METHODHANDLE)),
                },
            );
            java_lang_invoke_methodhandle.add_method(
                false,
                UnqualifiedName::CHANGERETURNTYPE,
                MethodDescriptor {
                    parameters: vec![FieldType::Ref(RefType::CLASS)],
                    return_type: Some(FieldType::Ref(RefType::METHODTYPE)),
                },
            );
            self.add_class(BinaryName::METHODHANDLE, java_lang_invoke_methodhandle);
        }

        // java.lang.invoke.MethodHandles
        {
            let java_lang_invoke_methodhandles = ClassData::new(BinaryName::OBJECT, false);
            java_lang_invoke_methodhandles.add_method(
                true,
                UnqualifiedName::DROPARGUMENTS,
                MethodDescriptor {
                    parameters: vec![
                        FieldType::Ref(RefType::METHODHANDLE),
                        FieldType::INT,
                        FieldType::array(FieldType::Ref(RefType::CLASS)),
                    ],
                    return_type: Some(FieldType::Ref(RefType::METHODHANDLE)),
                },
            );
            java_lang_invoke_methodhandles.add_method(
                true,
                UnqualifiedName::PERMUTEARGUMENTS,
                MethodDescriptor {
                    parameters: vec![
                        FieldType::Ref(RefType::METHODHANDLE),
                        FieldType::Ref(RefType::METHODTYPE),
                        FieldType::Ref(RefType::array(FieldType::INT)),
                    ],
                    return_type: Some(FieldType::Ref(RefType::METHODHANDLE)),
                },
            );
            java_lang_invoke_methodhandles.add_method(
                true,
                UnqualifiedName::COLLECTARGUMENTS,
                MethodDescriptor {
                    parameters: vec![
                        FieldType::Ref(RefType::METHODHANDLE),
                        FieldType::INT,
                        FieldType::Ref(RefType::METHODHANDLE),
                    ],
                    return_type: Some(FieldType::Ref(RefType::METHODHANDLE)),
                },
            );
            java_lang_invoke_methodhandles.add_method(
                true,
                UnqualifiedName::INSERTARGUMENTS,
                MethodDescriptor {
                    parameters: vec![
                        FieldType::Ref(RefType::METHODHANDLE),
                        FieldType::INT,
                        FieldType::array(FieldType::OBJECT),
                    ],
                    return_type: Some(FieldType::Ref(RefType::METHODHANDLE)),
                },
            );
            java_lang_invoke_methodhandles.add_method(
                true,
                UnqualifiedName::EXACTINVOKER,
                MethodDescriptor {
                    parameters: vec![FieldType::Ref(RefType::METHODTYPE)],
                    return_type: Some(FieldType::Ref(RefType::METHODHANDLE)),
                },
            );
            java_lang_invoke_methodhandles.add_method(
                true,
                UnqualifiedName::FILTERRETURNVALUE,
                MethodDescriptor {
                    parameters: vec![
                        FieldType::Ref(RefType::METHODHANDLE),
                        FieldType::Ref(RefType::METHODHANDLE),
                    ],
                    return_type: Some(FieldType::Ref(RefType::METHODHANDLE)),
                },
            );
            java_lang_invoke_methodhandles.add_method(
                true,
                UnqualifiedName::GUARDWITHTEST,
                MethodDescriptor {
                    parameters: vec![
                        FieldType::Ref(RefType::METHODHANDLE),
                        FieldType::Ref(RefType::METHODHANDLE),
                        FieldType::Ref(RefType::METHODHANDLE),
                    ],
                    return_type: Some(FieldType::Ref(RefType::METHODHANDLE)),
                },
            );
            for method in vec![
                UnqualifiedName::ARRAYCONSTRUCTOR,
                UnqualifiedName::ARRAYELEMENTGETTER,
                UnqualifiedName::ARRAYELEMENTSETTER,
                UnqualifiedName::ARRAYLENGTH,
            ] {
                java_lang_invoke_methodhandles.add_method(
                    true,
                    method,
                    MethodDescriptor {
                        parameters: vec![FieldType::Ref(RefType::CLASS)],
                        return_type: Some(FieldType::Ref(RefType::METHODHANDLE)),
                    },
                );
            }
            java_lang_invoke_methodhandles.add_method(
                true,
                UnqualifiedName::EMPTY,
                MethodDescriptor {
                    parameters: vec![FieldType::Ref(RefType::METHODTYPE)],
                    return_type: Some(FieldType::Ref(RefType::METHODHANDLE)),
                },
            );
            java_lang_invoke_methodhandles.add_method(
                true,
                UnqualifiedName::CONSTANT,
                MethodDescriptor {
                    parameters: vec![FieldType::Ref(RefType::CLASS), FieldType::OBJECT],
                    return_type: Some(FieldType::Ref(RefType::METHODHANDLE)),
                },
            );
            self.add_class(BinaryName::METHODHANDLES, java_lang_invoke_methodhandles);
        }

        // java.lang.invoke.MethodHandles#Lookup
        {
            let java_lang_invoke_methodhandles_lookup = ClassData::new(BinaryName::OBJECT, false);
            java_lang_invoke_methodhandles_lookup.add_method(
                true,
                UnqualifiedName::FINDSTATIC,
                MethodDescriptor {
                    parameters: vec![
                        FieldType::Ref(RefType::CLASS),
                        FieldType::Ref(RefType::STRING),
                        FieldType::Ref(RefType::METHODTYPE),
                    ],
                    return_type: Some(FieldType::Ref(RefType::METHODHANDLE)),
                },
            );
            self.add_class(
                BinaryName::METHODHANDLES_LOOKUP,
                java_lang_invoke_methodhandles_lookup,
            );
        }

        // java.lang.invoke.CallSite
        {
            let java_lang_invoke_callsite = ClassData::new(BinaryName::OBJECT, false);
            java_lang_invoke_callsite.add_method(
                false,
                UnqualifiedName::DYNAMICINVOKER,
                MethodDescriptor {
                    parameters: vec![],
                    return_type: Some(FieldType::object(BinaryName::METHODHANDLE)),
                },
            );
            java_lang_invoke_callsite.add_method(
                false,
                UnqualifiedName::GETTARGET,
                MethodDescriptor {
                    parameters: vec![],
                    return_type: Some(FieldType::object(BinaryName::METHODHANDLE)),
                },
            );
            java_lang_invoke_callsite.add_method(
                false,
                UnqualifiedName::SETTARGET,
                MethodDescriptor {
                    parameters: vec![FieldType::object(BinaryName::METHODHANDLE)],
                    return_type: None,
                },
            );
            java_lang_invoke_callsite.add_method(
                false,
                UnqualifiedName::TYPE,
                MethodDescriptor {
                    parameters: vec![],
                    return_type: Some(FieldType::object(BinaryName::METHODTYPE)),
                },
            );
            self.add_class(BinaryName::CALLSITE, java_lang_invoke_callsite);
        }

        // java.lang.invoke.ConstantCallSite
        {
            let java_lang_invoke_constantcallsite = ClassData::new(BinaryName::CALLSITE, false);
            java_lang_invoke_constantcallsite.add_method(
                false,
                UnqualifiedName::INIT,
                MethodDescriptor {
                    parameters: vec![FieldType::object(BinaryName::METHODHANDLE)],
                    return_type: None,
                },
            );
            self.add_class(
                BinaryName::CONSTANTCALLSITE,
                java_lang_invoke_constantcallsite,
            );
        }

        // java.lang.invoke.MutableCallSite
        {
            let java_lang_invoke_mutablecallsite = ClassData::new(BinaryName::CALLSITE, false);
            java_lang_invoke_mutablecallsite.add_method(
                true,
                UnqualifiedName::SYNCALL,
                MethodDescriptor {
                    parameters: vec![FieldType::array(FieldType::object(
                        BinaryName::MUTABLECALLSITE,
                    ))],
                    return_type: None,
                },
            );
            java_lang_invoke_mutablecallsite.add_method(
                false,
                UnqualifiedName::INIT,
                MethodDescriptor {
                    parameters: vec![FieldType::array(FieldType::object(
                        BinaryName::METHODHANDLE,
                    ))],
                    return_type: None,
                },
            );
            self.add_class(
                BinaryName::MUTABLECALLSITE,
                java_lang_invoke_mutablecallsite,
            );
        }

        // java.lang.Number
        {
            let java_lang_number = ClassData::new(BinaryName::OBJECT, false);
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
            self.add_class(BinaryName::NUMBER, java_lang_number);
        }

        // java.lang.Integer
        {
            let java_lang_integer = ClassData::new(BinaryName::NUMBER, false);
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
            java_lang_integer.add_field(
                true,
                UnqualifiedName::UPPERCASE_TYPE,
                FieldType::Ref(RefType::CLASS),
            );
            self.add_class(BinaryName::INTEGER, java_lang_integer);
        }

        // java.lang.Float
        {
            let java_lang_float = ClassData::new(BinaryName::NUMBER, false);
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
            java_lang_float.add_field(
                true,
                UnqualifiedName::UPPERCASE_TYPE,
                FieldType::Ref(RefType::CLASS),
            );
            self.add_class(BinaryName::FLOAT, java_lang_float);
        }

        // java.lang.Long
        {
            let java_lang_long = ClassData::new(BinaryName::NUMBER, false);
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
            java_lang_long.add_field(
                true,
                UnqualifiedName::UPPERCASE_TYPE,
                FieldType::Ref(RefType::CLASS),
            );
            self.add_class(BinaryName::LONG, java_lang_long);
        }

        // java.lang.Double
        {
            let java_lang_double = ClassData::new(BinaryName::NUMBER, false);
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
            java_lang_double.add_field(
                true,
                UnqualifiedName::UPPERCASE_TYPE,
                FieldType::Ref(RefType::CLASS),
            );
            self.add_class(BinaryName::DOUBLE, java_lang_double);
        }

        // java.lang.Void
        {
            let java_lang_void = ClassData::new(BinaryName::OBJECT, false);
            java_lang_void.add_field(
                true,
                UnqualifiedName::UPPERCASE_TYPE,
                FieldType::Ref(RefType::CLASS),
            );
            self.add_class(BinaryName::VOID, java_lang_void);
        }

        // java.lang.Boolean
        {
            let java_lang_boolean = ClassData::new(BinaryName::OBJECT, false);
            java_lang_boolean.add_field(
                true,
                UnqualifiedName::UPPERCASE_TYPE,
                FieldType::Ref(RefType::CLASS),
            );
            java_lang_boolean.add_method(
                true,
                UnqualifiedName::VALUEOF,
                MethodDescriptor {
                    parameters: vec![FieldType::BOOLEAN],
                    return_type: Some(FieldType::Ref(RefType::Object(BinaryName::BOOLEAN))),
                },
            );
            self.add_class(BinaryName::BOOLEAN, java_lang_boolean);
        }

        // java.lang.Math
        {
            let java_lang_math = ClassData::new(BinaryName::OBJECT, false);
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
            java_lang_math.add_method(
                true,
                UnqualifiedName::ADDEXACT,
                MethodDescriptor {
                    parameters: vec![FieldType::INT, FieldType::INT],
                    return_type: Some(FieldType::INT),
                },
            );
            self.add_class(BinaryName::MATH, java_lang_math);
        }

        // java.lang.System
        {
            let java_lang_system = ClassData::new(BinaryName::OBJECT, false);
            java_lang_system.add_method(
                true,
                UnqualifiedName::ARRAYCOPY,
                MethodDescriptor {
                    parameters: vec![
                        FieldType::OBJECT,
                        FieldType::INT,
                        FieldType::OBJECT,
                        FieldType::INT,
                        FieldType::INT,
                    ],
                    return_type: None,
                },
            );
            self.add_class(BinaryName::SYSTEM, java_lang_system);
        }
    }

    /// Add standard exception/error types to the class graph
    pub fn insert_error_types(&self) {
        // java.lang.Throwable
        {
            let java_lang_throwable = ClassData::new(BinaryName::OBJECT, false);
            java_lang_throwable.add_method(
                false,
                UnqualifiedName::INIT,
                MethodDescriptor {
                    parameters: vec![FieldType::Ref(RefType::STRING)],
                    return_type: None,
                },
            );
            self.add_class(BinaryName::THROWABLE, java_lang_throwable);
        }

        // java.lang.Error
        {
            let java_lang_error = ClassData::new(BinaryName::THROWABLE, false);
            java_lang_error.add_method(
                false,
                UnqualifiedName::INIT,
                MethodDescriptor {
                    parameters: vec![FieldType::Ref(RefType::STRING)],
                    return_type: None,
                },
            );
            self.add_class(BinaryName::ERROR, java_lang_error);
        }

        // java.lang.AssertionError
        {
            let java_lang_assertionerror = ClassData::new(BinaryName::ERROR, false);
            java_lang_assertionerror.add_method(
                false,
                UnqualifiedName::INIT,
                MethodDescriptor {
                    parameters: vec![FieldType::Ref(RefType::STRING)],
                    return_type: None,
                },
            );
            self.add_class(BinaryName::ASSERTIONERROR, java_lang_assertionerror);
        }

        // java.lang.Exception
        {
            let java_lang_error = ClassData::new(BinaryName::THROWABLE, false);
            java_lang_error.add_method(
                false,
                UnqualifiedName::INIT,
                MethodDescriptor {
                    parameters: vec![FieldType::Ref(RefType::STRING)],
                    return_type: None,
                },
            );
            self.add_class(BinaryName::EXCEPTION, java_lang_error);
        }

        // java.lang.RuntimeException
        {
            let java_lang_error = ClassData::new(BinaryName::EXCEPTION, false);
            java_lang_error.add_method(
                false,
                UnqualifiedName::INIT,
                MethodDescriptor {
                    parameters: vec![FieldType::Ref(RefType::STRING)],
                    return_type: None,
                },
            );
            self.add_class(BinaryName::RUNTIMEEXCEPTION, java_lang_error);
        }

        // java.lang.ArithmeticException
        {
            let java_lang_error = ClassData::new(BinaryName::RUNTIMEEXCEPTION, false);
            java_lang_error.add_method(
                false,
                UnqualifiedName::INIT,
                MethodDescriptor {
                    parameters: vec![FieldType::Ref(RefType::STRING)],
                    return_type: None,
                },
            );
            self.add_class(BinaryName::ARITHMETICEXCEPTION, java_lang_error);
        }

        // java.lang.IllegalArgumentException
        {
            let java_lang_error = ClassData::new(BinaryName::RUNTIMEEXCEPTION, false);
            java_lang_error.add_method(
                false,
                UnqualifiedName::INIT,
                MethodDescriptor {
                    parameters: vec![FieldType::Ref(RefType::STRING)],
                    return_type: None,
                },
            );
            self.add_class(BinaryName::ILLEGALARGUMENTEXCEPTION, java_lang_error);
        }
    }

    /// Add standard util types to the class graph
    pub fn insert_util_types(&self) {
        // java.util.Arrays
        {
            let java_util_arrays = ClassData::new(BinaryName::OBJECT, false);
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
                    parameters: vec![
                        FieldType::array(FieldType::OBJECT),
                        FieldType::INT,
                        FieldType::INT,
                        FieldType::OBJECT,
                    ],
                    return_type: None,
                },
            );
            self.add_class(BinaryName::ARRAYS, java_util_arrays);
        }

        // java.util.Map
        {
            let java_util_map = ClassData::new(BinaryName::OBJECT, true);
            java_util_map.add_method(
                false,
                UnqualifiedName::PUT,
                MethodDescriptor {
                    parameters: vec![FieldType::OBJECT, FieldType::OBJECT],
                    return_type: Some(FieldType::OBJECT),
                },
            );
            java_util_map.add_method(
                false,
                UnqualifiedName::GET,
                MethodDescriptor {
                    parameters: vec![FieldType::OBJECT],
                    return_type: Some(FieldType::OBJECT),
                },
            );
            self.add_class(BinaryName::MAP, java_util_map);
        }

        // java.util.HashMap
        {
            let mut java_util_hashmap = ClassData::new(BinaryName::OBJECT, true);
            java_util_hashmap.add_interfaces([BinaryName::MAP]);
            java_util_hashmap.add_method(
                false,
                UnqualifiedName::INIT,
                MethodDescriptor {
                    parameters: vec![],
                    return_type: None,
                },
            );
            self.add_class(BinaryName::HASHMAP, java_util_hashmap);
        }
    }

    pub fn insert_buffer_types(&self) {
        // java.nio.Buffer
        {
            let java_nio_buffer = ClassData::new(BinaryName::OBJECT, false);
            java_nio_buffer.add_method(
                false,
                UnqualifiedName::POSITION,
                MethodDescriptor {
                    parameters: vec![FieldType::INT],
                    return_type: Some(FieldType::object(BinaryName::BUFFER)),
                },
            );
            java_nio_buffer.add_method(
                false,
                UnqualifiedName::CAPACITY,
                MethodDescriptor {
                    parameters: vec![],
                    return_type: Some(FieldType::INT),
                },
            );
            self.add_class(BinaryName::BUFFER, java_nio_buffer);
        }

        // java.nio.ByteOrder
        {
            let java_nio_byteorder = ClassData::new(BinaryName::OBJECT, false);
            java_nio_byteorder.add_field(
                true,
                UnqualifiedName::BIGENDIAN,
                FieldType::object(BinaryName::BYTEORDER),
            );
            java_nio_byteorder.add_field(
                true,
                UnqualifiedName::LITTLEENDIAN,
                FieldType::object(BinaryName::BYTEORDER),
            );
            self.add_class(BinaryName::BYTEORDER, java_nio_byteorder);
        }

        // java.nio.ByteBuffer
        {
            let java_nio_bytebuffer = ClassData::new(BinaryName::BUFFER, false);
            java_nio_bytebuffer.add_method(
                true,
                UnqualifiedName::ALLOCATE,
                MethodDescriptor {
                    parameters: vec![FieldType::INT],
                    return_type: Some(FieldType::object(BinaryName::BYTEBUFFER)),
                },
            );
            java_nio_bytebuffer.add_method(
                true,
                UnqualifiedName::ALLOCATEDIRECT,
                MethodDescriptor {
                    parameters: vec![FieldType::INT],
                    return_type: Some(FieldType::object(BinaryName::BYTEBUFFER)),
                },
            );
            for (get_name, put_name, typ) in vec![
                (UnqualifiedName::GET, UnqualifiedName::PUT, FieldType::BYTE),
                (
                    UnqualifiedName::GETDOUBLE,
                    UnqualifiedName::PUTDOUBLE,
                    FieldType::DOUBLE,
                ),
                (
                    UnqualifiedName::GETFLOAT,
                    UnqualifiedName::PUTFLOAT,
                    FieldType::FLOAT,
                ),
                (
                    UnqualifiedName::GETINT,
                    UnqualifiedName::PUTINT,
                    FieldType::INT,
                ),
                (
                    UnqualifiedName::GETLONG,
                    UnqualifiedName::PUTLONG,
                    FieldType::LONG,
                ),
                (
                    UnqualifiedName::GETSHORT,
                    UnqualifiedName::PUTSHORT,
                    FieldType::SHORT,
                ),
            ] {
                java_nio_bytebuffer.add_method(
                    false,
                    get_name,
                    MethodDescriptor {
                        parameters: vec![FieldType::INT],
                        return_type: Some(typ.clone()),
                    },
                );
                java_nio_bytebuffer.add_method(
                    false,
                    put_name,
                    MethodDescriptor {
                        parameters: vec![FieldType::INT, typ],
                        return_type: Some(FieldType::object(BinaryName::BYTEBUFFER)),
                    },
                );
            }
            java_nio_bytebuffer.add_method(
                false,
                UnqualifiedName::PUT,
                MethodDescriptor {
                    parameters: vec![FieldType::object(BinaryName::BYTEBUFFER)],
                    return_type: Some(FieldType::object(BinaryName::BYTEBUFFER)),
                },
            );
            java_nio_bytebuffer.add_method(
                false,
                UnqualifiedName::PUT,
                MethodDescriptor {
                    parameters: vec![FieldType::array(FieldType::BYTE)],
                    return_type: Some(FieldType::object(BinaryName::BYTEBUFFER)),
                },
            );
            java_nio_bytebuffer.add_method(
                false,
                UnqualifiedName::ORDER,
                MethodDescriptor {
                    parameters: vec![FieldType::object(BinaryName::BYTEORDER)],
                    return_type: Some(FieldType::object(BinaryName::BYTEBUFFER)),
                },
            );
            self.add_class(BinaryName::BYTEBUFFER, java_nio_bytebuffer);
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
    pub methods: FrozenVec<Box<(UnqualifiedName, MethodDescriptor, bool)>>,

    /// Fields
    pub fields: FrozenMap<UnqualifiedName, Box<(bool, FieldType)>>,
}

impl ClassData {
    pub fn new(superclass: BinaryName, is_interface: bool) -> ClassData {
        ClassData {
            superclass: Some(superclass),
            interfaces: HashSet::new(),
            is_interface,
            methods: FrozenVec::new(),
            fields: FrozenMap::new(),
        }
    }

    pub fn add_interfaces(&mut self, interfaces: impl IntoIterator<Item = BinaryName>) {
        self.interfaces.extend(interfaces);
    }

    pub fn add_field(&self, is_static: bool, name: UnqualifiedName, descriptor: FieldType) {
        self.fields.insert(name, Box::new((is_static, descriptor)));
    }

    pub fn add_method(&self, is_static: bool, name: UnqualifiedName, descriptor: MethodDescriptor) {
        let method = (name, descriptor, is_static);
        if self.methods.iter().all(|m| m != &method) {
            self.methods.push(Box::new(method));
        }
    }
}
