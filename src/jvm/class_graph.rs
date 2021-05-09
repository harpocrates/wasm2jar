use super::{FieldType, MethodDescriptor, RefType};
use std::collections::{HashMap, HashSet};

/// Tracks the relationships between classes/interfaces and the members on those classes
///
/// When generating multiple classes, it is quite convenient to maintain one unified graph of all
/// of the types/members in the generated code. Then, when a class needs to access some member, it
/// can import the necessary segment of the class graph into its constant pool.
pub struct ClassGraph {
    pub classes: HashMap<String, ClassData>,
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
        let java_lang_object = self
            .classes
            .entry(String::from(RefType::OBJECT_NAME))
            .or_insert(ClassData {
                superclass: None,
                interfaces: HashSet::new(),
                is_interface: false,
                members: HashMap::new(),
            });

        java_lang_object.members.insert(
            String::from("equals"),
            ClassMember::Method {
                is_static: false,
                descriptor: MethodDescriptor {
                    parameters: vec![FieldType::Ref(RefType::OBJECT_CLASS)],
                    return_type: Some(FieldType::BOOLEAN),
                },
            },
        );
        java_lang_object.members.insert(
            String::from("hashCode"),
            ClassMember::Method {
                is_static: false,
                descriptor: MethodDescriptor {
                    parameters: vec![],
                    return_type: Some(FieldType::INT),
                },
            },
        );
        java_lang_object.members.insert(
            String::from("<init>"),
            ClassMember::Method {
                is_static: false,
                descriptor: MethodDescriptor {
                    parameters: vec![],
                    return_type: None,
                },
            },
        );

        // java.lang.String
        let java_lang_string = self
            .classes
            .entry(String::from(RefType::STRING_NAME))
            .or_insert(ClassData {
                superclass: Some(String::from(RefType::OBJECT_NAME)),
                interfaces: HashSet::new(),
                is_interface: false,
                members: HashMap::new(),
            });

        java_lang_string.members.insert(
            String::from("getBytes"),
            ClassMember::Method {
                is_static: false,
                descriptor: MethodDescriptor {
                    parameters: vec![FieldType::Ref(RefType::STRING_CLASS)],
                    return_type: None,
                },
            },
        );

        // java.lang.Number
        let java_lang_number = self
            .classes
            .entry(String::from("java/lang/Number"))
            .or_insert(ClassData {
                superclass: Some(String::from(RefType::OBJECT_NAME)),
                interfaces: HashSet::new(),
                is_interface: false,
                members: HashMap::new(),
            });

        java_lang_number.members.extend(
            vec![
                ("byteValue", FieldType::BYTE),
                ("doubleValue", FieldType::DOUBLE),
                ("floatValue", FieldType::FLOAT),
                ("intValue", FieldType::INT),
                ("longValue", FieldType::LONG),
                ("shortValue", FieldType::SHORT),
            ]
            .into_iter()
            .map(|(name, typ)| {
                let method = ClassMember::Method {
                    is_static: false,
                    descriptor: MethodDescriptor {
                        parameters: vec![],
                        return_type: Some(typ),
                    },
                };
                (String::from(name), method)
            }),
        );
        java_lang_number.members.insert(
            String::from("<init>"),
            ClassMember::Method {
                is_static: false,
                descriptor: MethodDescriptor {
                    parameters: vec![],
                    return_type: None,
                },
            },
        );

        // java.lang.Integer
        let java_lang_integer = self
            .classes
            .entry(String::from("java/lang/Integer"))
            .or_insert(ClassData {
                superclass: Some(String::from("java/lang/Number")),
                interfaces: HashSet::new(),
                is_interface: false,
                members: HashMap::new(),
            });
        java_lang_integer.members.insert(
            String::from("<init>"),
            ClassMember::Method {
                is_static: false,
                descriptor: MethodDescriptor {
                    parameters: vec![FieldType::INT],
                    return_type: None,
                },
            },
        );
        java_lang_integer.members.insert(
            String::from("valueOf"),
            ClassMember::Method {
                is_static: true,
                descriptor: MethodDescriptor {
                    parameters: vec![FieldType::INT],
                    return_type: Some(FieldType::object("java/lang/Integer")),
                },
            },
        );
        for name in vec!["bitCount", "numberOfLeadingZeros", "numberOfTrailingZeros"] {
            java_lang_integer.members.insert(
                String::from(name),
                ClassMember::Method {
                    is_static: true,
                    descriptor: MethodDescriptor {
                        parameters: vec![FieldType::INT],
                        return_type: Some(FieldType::INT),
                    },
                },
            );
        }
        for name in vec![
            "compareUnsigned",
            "divideUnsigned",
            "remainderUnsigned",
            "rotateLeft",
            "rotateRight",
        ] {
            java_lang_integer.members.insert(
                String::from(name),
                ClassMember::Method {
                    is_static: true,
                    descriptor: MethodDescriptor {
                        parameters: vec![FieldType::INT, FieldType::INT],
                        return_type: Some(FieldType::INT),
                    },
                },
            );
        }

        // java.lang.Float
        let java_lang_float = self
            .classes
            .entry(String::from("java/lang/Float"))
            .or_insert(ClassData {
                superclass: Some(String::from("java/lang/Number")),
                interfaces: HashSet::new(),
                is_interface: false,
                members: HashMap::new(),
            });
        java_lang_float.members.insert(
            String::from("<init>"),
            ClassMember::Method {
                is_static: false,
                descriptor: MethodDescriptor {
                    parameters: vec![FieldType::FLOAT],
                    return_type: None,
                },
            },
        );

        // java.lang.Long
        let java_lang_long = self
            .classes
            .entry(String::from("java/lang/Long"))
            .or_insert(ClassData {
                superclass: Some(String::from("java/lang/Number")),
                interfaces: HashSet::new(),
                is_interface: false,
                members: HashMap::new(),
            });
        java_lang_long.members.insert(
            String::from("<init>"),
            ClassMember::Method {
                is_static: false,
                descriptor: MethodDescriptor {
                    parameters: vec![FieldType::LONG],
                    return_type: None,
                },
            },
        );
        java_lang_long.members.insert(
            String::from("valueOf"),
            ClassMember::Method {
                is_static: true,
                descriptor: MethodDescriptor {
                    parameters: vec![FieldType::LONG],
                    return_type: Some(FieldType::object("java/lang/Long")),
                },
            },
        );
        for name in vec!["bitCount", "numberOfLeadingZeros", "numberOfTrailingZeros"] {
            java_lang_long.members.insert(
                String::from(name),
                ClassMember::Method {
                    is_static: true,
                    descriptor: MethodDescriptor {
                        parameters: vec![FieldType::LONG],
                        return_type: Some(FieldType::INT),
                    },
                },
            );
        }
        java_lang_long.members.insert(
            String::from("compareUnsigned"),
            ClassMember::Method {
                is_static: true,
                descriptor: MethodDescriptor {
                    parameters: vec![FieldType::LONG, FieldType::LONG],
                    return_type: Some(FieldType::INT),
                },
            },
        );
        for name in vec!["divideUnsigned", "remainderUnsigned"] {
            java_lang_long.members.insert(
                String::from(name),
                ClassMember::Method {
                    is_static: true,
                    descriptor: MethodDescriptor {
                        parameters: vec![FieldType::LONG, FieldType::LONG],
                        return_type: Some(FieldType::LONG),
                    },
                },
            );
        }
        for name in vec!["rotateLeft", "rotateRight"] {
            java_lang_long.members.insert(
                String::from(name),
                ClassMember::Method {
                    is_static: true,
                    descriptor: MethodDescriptor {
                        parameters: vec![FieldType::LONG, FieldType::INT],
                        return_type: Some(FieldType::LONG),
                    },
                },
            );
        }

        // java.lang.Double
        let java_lang_double = self
            .classes
            .entry(String::from("java/lang/Double"))
            .or_insert(ClassData {
                superclass: Some(String::from("java/lang/Number")),
                interfaces: HashSet::new(),
                is_interface: false,
                members: HashMap::new(),
            });
        java_lang_double.members.insert(
            String::from("<init>"),
            ClassMember::Method {
                is_static: false,
                descriptor: MethodDescriptor {
                    parameters: vec![FieldType::DOUBLE],
                    return_type: None,
                },
            },
        );

        // java.lang.Math
        let java_lang_math = self
            .classes
            .entry(String::from("java/lang/Math"))
            .or_insert(ClassData {
                superclass: Some(String::from("java/lang/Object")),
                interfaces: HashSet::new(),
                is_interface: false,
                members: HashMap::new(),
            });
        java_lang_math.members.insert(
            String::from("ceil"),
            ClassMember::Method {
                is_static: true,
                descriptor: MethodDescriptor {
                    parameters: vec![FieldType::DOUBLE],
                    return_type: Some(FieldType::DOUBLE),
                },
            },
        );
        for typ in vec![FieldType::FLOAT, FieldType::DOUBLE] {
            java_lang_math.members.insert(
                String::from("copySign"), // TODO: duplicate keys will override each other
                ClassMember::Method {
                    is_static: true,
                    descriptor: MethodDescriptor {
                        parameters: vec![typ.clone(), typ.clone()],
                        return_type: Some(typ),
                    },
                },
            );
        }
    }
}

// TODO: should we track subclasses?
pub struct ClassData {
    /// Superclass is only ever `null` for `java/lang/Object` itself
    pub superclass: Option<String>,

    /// Interfaces implemented (or super-interfaces)
    pub interfaces: HashSet<String>,

    /// Is this an interface?
    pub is_interface: bool,

    /// Fields and methods
    pub members: HashMap<String, ClassMember>,
}

pub enum ClassMember {
    Method {
        is_static: bool,
        descriptor: MethodDescriptor,
    },
    Field {
        is_static: bool,
        descriptor: FieldType,
    },
}
