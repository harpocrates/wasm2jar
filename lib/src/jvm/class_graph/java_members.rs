use super::{
    ClassData, ClassGraph, FieldData, FieldType, MethodData, MethodDescriptor, UnqualifiedName,
};

use super::java_classes::JavaClasses;

/// Members of classes inside `java.*`
pub struct JavaMembers<'g> {
    pub lang: LangMembers<'g>,
    pub nio: NioMembers<'g>,
    pub util: UtilMembers<'g>,
}

/// Members of classes inside `java.lang.*`
pub struct LangMembers<'g> {
    pub object: ObjectMembers<'g>,
    pub char_sequence: CharSequenceMembers<'g>,
    pub string: StringMembers<'g>,
    pub number: NumberMembers<'g>,
    pub integer: IntegerMembers<'g>,
    pub float: FloatMembers<'g>,
    pub long: LongMembers<'g>,
    pub double: DoubleMembers<'g>,
    pub void: VoidMembers<'g>,
    pub boolean: BooleanMembers<'g>,
    pub math: MathMembers<'g>,
    pub system: SystemMembers<'g>,
    pub invoke: InvokeMembers<'g>,
    pub throwable: ThrowableMembers<'g>,
    pub error: ErrorMembers<'g>,
    pub assertion_error: AssertionErrorMembers<'g>,
    pub exception: ExceptionMembers<'g>,
    pub runtime_exception: RuntimeExceptionMembers<'g>,
    pub arithmetic_exception: ArithmeticExceptionMembers<'g>,
    pub illegal_argument_exception: IllegalArgumentExceptionMembers<'g>,
}

/// Members of `java.lang.Object`
pub struct ObjectMembers<'g> {
    pub equals: &'g MethodData<'g>,
    pub hash_code: &'g MethodData<'g>,
    pub init: &'g MethodData<'g>,
}

/// Members of `java.lang.CharSequence`
pub struct CharSequenceMembers<'g> {
    pub length: &'g MethodData<'g>,
}

/// Members of `java.lang.String`
pub struct StringMembers<'g> {
    pub get_bytes: &'g MethodData<'g>,
}

/// Members of `java.lang.Number`
pub struct NumberMembers<'g> {
    pub byte_value: &'g MethodData<'g>,
    pub double_value: &'g MethodData<'g>,
    pub float_value: &'g MethodData<'g>,
    pub int_value: &'g MethodData<'g>,
    pub long_value: &'g MethodData<'g>,
    pub short_value: &'g MethodData<'g>,
}

/// Members of `java.lang.Integer`
pub struct IntegerMembers<'g> {
    pub value_of: &'g MethodData<'g>,
    pub bit_count: &'g MethodData<'g>,
    pub number_of_leading_zeros: &'g MethodData<'g>,
    pub number_of_trailing_zeros: &'g MethodData<'g>,
    pub compare: &'g MethodData<'g>,
    pub compare_unsigned: &'g MethodData<'g>,
    pub divide_unsigned: &'g MethodData<'g>,
    pub remainder_unsigned: &'g MethodData<'g>,
    pub rotate_left: &'g MethodData<'g>,
    pub rotate_right: &'g MethodData<'g>,
    pub max_value: &'g FieldData<'g>,
    pub min_value: &'g FieldData<'g>,
    pub r#type: &'g FieldData<'g>,
}

/// Members of `java.lang.Integer`
pub struct FloatMembers<'g> {
    pub value_of: &'g MethodData<'g>,
    pub float_to_raw_int_bits: &'g MethodData<'g>,
    pub int_bits_to_float: &'g MethodData<'g>,
    pub max: &'g MethodData<'g>,
    pub min: &'g MethodData<'g>,
    pub max_value: &'g FieldData<'g>,
    pub min_value: &'g FieldData<'g>,
    pub nan: &'g FieldData<'g>,
    pub negative_infinity: &'g FieldData<'g>,
    pub positive_infinity: &'g FieldData<'g>,
    pub r#type: &'g FieldData<'g>,
}

/// Members of `java.lang.Long`
pub struct LongMembers<'g> {
    pub value_of: &'g MethodData<'g>,
    pub bit_count: &'g MethodData<'g>,
    pub number_of_leading_zeros: &'g MethodData<'g>,
    pub number_of_trailing_zeros: &'g MethodData<'g>,
    pub compare: &'g MethodData<'g>,
    pub compare_unsigned: &'g MethodData<'g>,
    pub divide_unsigned: &'g MethodData<'g>,
    pub remainder_unsigned: &'g MethodData<'g>,
    pub rotate_left: &'g MethodData<'g>,
    pub rotate_right: &'g MethodData<'g>,
    pub max_value: &'g FieldData<'g>,
    pub min_value: &'g FieldData<'g>,
    pub r#type: &'g FieldData<'g>,
}

/// Members of `java.lang.Integer`
pub struct DoubleMembers<'g> {
    pub value_of: &'g MethodData<'g>,
    pub double_to_raw_long_bits: &'g MethodData<'g>,
    pub long_bits_to_double: &'g MethodData<'g>,
    pub max: &'g MethodData<'g>,
    pub min: &'g MethodData<'g>,
    pub max_value: &'g FieldData<'g>,
    pub min_value: &'g FieldData<'g>,
    pub nan: &'g FieldData<'g>,
    pub negative_infinity: &'g FieldData<'g>,
    pub positive_infinity: &'g FieldData<'g>,
    pub r#type: &'g FieldData<'g>,
}

/// Members of `java.lang.Void`
pub struct VoidMembers<'g> {
    pub r#type: &'g FieldData<'g>,
}

/// Members of `java.lang.Boolean`
pub struct BooleanMembers<'g> {
    pub value_of: &'g MethodData<'g>,
    pub r#type: &'g FieldData<'g>,
}

/// Members of `java.lang.Math`
pub struct MathMembers<'g> {
    pub ceil: &'g MethodData<'g>,
    pub floor: &'g MethodData<'g>,
    pub sqrt: &'g MethodData<'g>,
    pub rint: &'g MethodData<'g>,
    pub copy_sign_float: &'g MethodData<'g>,
    pub copy_sign_double: &'g MethodData<'g>,
    pub abs_float: &'g MethodData<'g>,
    pub abs_double: &'g MethodData<'g>,
    pub to_int_exact: &'g MethodData<'g>,
    pub add_exact: &'g MethodData<'g>,
}

/// Members of `java.lang.System`
pub struct SystemMembers<'g> {
    pub arraycopy: &'g MethodData<'g>,
}

/// Members of classes inside `java.lang.invoke`
pub struct InvokeMembers<'g> {
    pub method_type: MethodTypeMembers<'g>,
    pub method_handle: MethodHandleMembers<'g>,
    pub method_handles: MethodHandlesMembers<'g>,
    pub call_site: CallSiteMembers<'g>,
    pub constant_call_site: ConstantCallSiteMembers<'g>,
    pub mutable_call_site: MutableCallSiteMembers<'g>,
}

/// Members of `java.lang.invoke.MethodType`
pub struct MethodTypeMembers<'g> {
    pub parameter_count: &'g MethodData<'g>,
    pub parameter_type: &'g MethodData<'g>,
    pub parameter_array: &'g MethodData<'g>,
    pub drop_parameter_types: &'g MethodData<'g>,
    pub return_type: &'g MethodData<'g>,
    pub method_type: &'g MethodData<'g>,
}

/// Members of `java.lang.invoke.MethodHandle`
pub struct MethodHandleMembers<'g> {
    pub r#type: &'g MethodData<'g>,
    pub as_type: &'g MethodData<'g>,
    pub change_return_type: &'g MethodData<'g>,
}

/// Members of `java.lang.invoke.MethodHandles`
pub struct MethodHandlesMembers<'g> {
    pub drop_arguments: &'g MethodData<'g>,
    pub permute_arguments: &'g MethodData<'g>,
    pub collect_arguments: &'g MethodData<'g>,
    pub insert_arguments: &'g MethodData<'g>,
    pub exact_invoker: &'g MethodData<'g>,
    pub filter_return_value: &'g MethodData<'g>,
    pub guard_with_test: &'g MethodData<'g>,
    pub array_constructor: &'g MethodData<'g>,
    pub array_element_getter: &'g MethodData<'g>,
    pub array_element_setter: &'g MethodData<'g>,
    pub array_length: &'g MethodData<'g>,
    pub empty: &'g MethodData<'g>,
    pub constant: &'g MethodData<'g>,
}

/// Members of `java.lang.invoke.CallSite`
pub struct CallSiteMembers<'g> {
    pub dynamic_invoker: &'g MethodData<'g>,
    pub get_target: &'g MethodData<'g>,
    pub set_target: &'g MethodData<'g>,
    pub r#type: &'g MethodData<'g>,
}

/// Members of `java.lang.invoke.ConstantCallSite`
pub struct ConstantCallSiteMembers<'g> {
    pub init: &'g MethodData<'g>,
}

/// Members of `java.lang.invoke.MutableCallSite`
pub struct MutableCallSiteMembers<'g> {
    pub sync_all: &'g MethodData<'g>,
    pub init: &'g MethodData<'g>,
}

/// Members of `java.lang.Throwable`
pub struct ThrowableMembers<'g> {
    pub init: &'g MethodData<'g>,
}

/// Members of `java.lang.Error`
pub struct ErrorMembers<'g> {
    pub init: &'g MethodData<'g>,
}

/// Members of `java.lang.AssertionError`
pub struct AssertionErrorMembers<'g> {
    pub init: &'g MethodData<'g>,
}

/// Members of `java.lang.Exception`
pub struct ExceptionMembers<'g> {
    pub init: &'g MethodData<'g>,
}

/// Members of `java.lang.RuntimeException`
pub struct RuntimeExceptionMembers<'g> {
    pub init: &'g MethodData<'g>,
}

/// Members of `java.lang.ArithmeticException`
pub struct ArithmeticExceptionMembers<'g> {
    pub init: &'g MethodData<'g>,
}

/// Members of `java.lang.IllegalArgumentException`
pub struct IllegalArgumentExceptionMembers<'g> {
    pub init: &'g MethodData<'g>,
}

/// Members of classes inside `java.nio.*`
pub struct NioMembers<'g> {
    pub buffer: BufferMembers<'g>,
    pub byte_buffer: ByteBufferMembers<'g>,
    pub byte_order: ByteOrderMembers<'g>,
}

/// Members of `java.nio.Buffer`
pub struct BufferMembers<'g> {
    pub position: &'g MethodData<'g>,
    pub capacity: &'g MethodData<'g>,
}

/// Members of `java.nio.ByteBuffer`
pub struct ByteBufferMembers<'g> {
    pub allocate: &'g MethodData<'g>,
    pub allocate_direct: &'g MethodData<'g>,
    pub capacity: &'g MethodData<'g>,
    pub get_byte: &'g MethodData<'g>,
    pub put_byte: &'g MethodData<'g>,
    pub get_short: &'g MethodData<'g>,
    pub put_short: &'g MethodData<'g>,
    pub get_int: &'g MethodData<'g>,
    pub put_int: &'g MethodData<'g>,
    pub get_float: &'g MethodData<'g>,
    pub put_float: &'g MethodData<'g>,
    pub get_long: &'g MethodData<'g>,
    pub put_long: &'g MethodData<'g>,
    pub get_double: &'g MethodData<'g>,
    pub put_double: &'g MethodData<'g>,
    pub put_bytebuffer: &'g MethodData<'g>,
    pub put_bytearray: &'g MethodData<'g>,
    pub put_byte_relative: &'g MethodData<'g>,
    pub position: &'g MethodData<'g>,
    pub order: &'g MethodData<'g>,
}

/// Members of `java.nio.ByteOrder`
pub struct ByteOrderMembers<'g> {
    pub big_endian: &'g FieldData<'g>,
    pub little_endian: &'g FieldData<'g>,
}

/// Members of classes inside `java.util.*`
pub struct UtilMembers<'g> {
    pub arrays: ArraysMembers<'g>,
    pub map: MapMembers<'g>,
    pub hash_map: HashMapMembers<'g>,
}

/// Members of `java.util.Arrays`
pub struct ArraysMembers<'g> {
    pub copy_of: &'g MethodData<'g>,
    pub fill: &'g MethodData<'g>,
}

/// Members of `java.util.Map`
pub struct MapMembers<'g> {
    pub get: &'g MethodData<'g>,
    pub put: &'g MethodData<'g>,
}

/// Members of `java.util.HashMap`
pub struct HashMapMembers<'g> {
    pub init: &'g MethodData<'g>,
}

impl<'g> JavaMembers<'g> {
    pub fn add_to_graph(
        class_graph: &ClassGraph<'g>,
        classes: &JavaClasses<'g>,
    ) -> JavaMembers<'g> {
        let lang = LangMembers::add_to_graph(class_graph, classes);
        let nio = NioMembers::add_to_graph(class_graph, classes);
        let util = UtilMembers::add_to_graph(class_graph, classes);
        JavaMembers { lang, nio, util }
    }
}

impl<'g> LangMembers<'g> {
    pub fn add_to_graph(
        class_graph: &ClassGraph<'g>,
        classes: &JavaClasses<'g>,
    ) -> LangMembers<'g> {
        let object = ObjectMembers::add_to_graph(class_graph, classes);
        let char_sequence = CharSequenceMembers::add_to_graph(class_graph, classes);
        let string = StringMembers::add_to_graph(class_graph, classes);
        let number = NumberMembers::add_to_graph(class_graph, classes);
        let integer = IntegerMembers::add_to_graph(class_graph, classes);
        let float = FloatMembers::add_to_graph(class_graph, classes);
        let long = LongMembers::add_to_graph(class_graph, classes);
        let double = DoubleMembers::add_to_graph(class_graph, classes);
        let void = VoidMembers::add_to_graph(class_graph, classes);
        let boolean = BooleanMembers::add_to_graph(class_graph, classes);
        let math = MathMembers::add_to_graph(class_graph, classes);
        let system = SystemMembers::add_to_graph(class_graph, classes);
        let invoke = InvokeMembers::add_to_graph(class_graph, classes);
        let throwable = ThrowableMembers::add_to_graph(class_graph, classes);
        let error = ErrorMembers::add_to_graph(class_graph, classes);
        let assertion_error = AssertionErrorMembers::add_to_graph(class_graph, classes);
        let exception = ExceptionMembers::add_to_graph(class_graph, classes);
        let runtime_exception = RuntimeExceptionMembers::add_to_graph(class_graph, classes);
        let arithmetic_exception = ArithmeticExceptionMembers::add_to_graph(class_graph, classes);
        let illegal_argument_exception =
            IllegalArgumentExceptionMembers::add_to_graph(class_graph, classes);
        LangMembers {
            object,
            char_sequence,
            string,
            number,
            integer,
            float,
            long,
            double,
            void,
            boolean,
            math,
            system,
            invoke,
            throwable,
            error,
            assertion_error,
            exception,
            runtime_exception,
            arithmetic_exception,
            illegal_argument_exception,
        }
    }
}

impl<'g> ObjectMembers<'g> {
    pub fn add_to_graph(
        class_graph: &ClassGraph<'g>,
        classes: &JavaClasses<'g>,
    ) -> ObjectMembers<'g> {
        let class = classes.lang.object;
        let equals = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::EQUALS,
            descriptor: MethodDescriptor {
                parameters: vec![FieldType::object(classes.lang.object)],
                return_type: Some(FieldType::boolean()),
            },
            is_static: false,
        });
        let hash_code = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::HASHCODE,
            descriptor: MethodDescriptor {
                parameters: vec![],
                return_type: Some(FieldType::int()),
            },
            is_static: false,
        });
        let init = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::INIT,
            descriptor: MethodDescriptor {
                parameters: vec![],
                return_type: None,
            },
            is_static: false,
        });
        ObjectMembers {
            equals,
            hash_code,
            init,
        }
    }
}

impl<'g> CharSequenceMembers<'g> {
    pub fn add_to_graph(
        class_graph: &ClassGraph<'g>,
        classes: &JavaClasses<'g>,
    ) -> CharSequenceMembers<'g> {
        let class = classes.lang.char_sequence;
        let length = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::LENGTH,
            descriptor: MethodDescriptor {
                parameters: vec![],
                return_type: Some(FieldType::int()),
            },
            is_static: false,
        });
        CharSequenceMembers { length }
    }
}

impl<'g> StringMembers<'g> {
    pub fn add_to_graph(
        class_graph: &ClassGraph<'g>,
        classes: &JavaClasses<'g>,
    ) -> StringMembers<'g> {
        let class = classes.lang.string;
        let get_bytes = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::GETBYTES,
            descriptor: MethodDescriptor {
                parameters: vec![FieldType::object(classes.lang.string)],
                return_type: Some(FieldType::array(FieldType::byte())),
            },
            is_static: false,
        });
        StringMembers { get_bytes }
    }
}

impl<'g> NumberMembers<'g> {
    pub fn add_to_graph(
        class_graph: &ClassGraph<'g>,
        classes: &JavaClasses<'g>,
    ) -> NumberMembers<'g> {
        let class = classes.lang.number;
        let add_extractor = |name: UnqualifiedName,
                             extracted_type: FieldType<&'g ClassData<'g>>|
         -> &'g MethodData<'g> {
            class_graph.add_method(MethodData {
                class,
                name,
                descriptor: MethodDescriptor {
                    parameters: vec![],
                    return_type: Some(extracted_type),
                },
                is_static: false,
            })
        };

        let byte_value = add_extractor(UnqualifiedName::BYTEVALUE, FieldType::byte());
        let double_value = add_extractor(UnqualifiedName::DOUBLEVALUE, FieldType::double());
        let float_value = add_extractor(UnqualifiedName::FLOATVALUE, FieldType::float());
        let int_value = add_extractor(UnqualifiedName::INTVALUE, FieldType::int());
        let long_value = add_extractor(UnqualifiedName::LONGVALUE, FieldType::long());
        let short_value = add_extractor(UnqualifiedName::SHORTVALUE, FieldType::short());

        NumberMembers {
            byte_value,
            double_value,
            float_value,
            int_value,
            long_value,
            short_value,
        }
    }
}

impl<'g> IntegerMembers<'g> {
    pub fn add_to_graph(
        class_graph: &ClassGraph<'g>,
        classes: &JavaClasses<'g>,
    ) -> IntegerMembers<'g> {
        let class = classes.lang.integer;

        let add_static_unary = |name: UnqualifiedName,
                                output_type: FieldType<&'g ClassData<'g>>|
         -> &'g MethodData<'g> {
            class_graph.add_method(MethodData {
                class,
                name,
                descriptor: MethodDescriptor {
                    parameters: vec![FieldType::int()],
                    return_type: Some(output_type),
                },
                is_static: true,
            })
        };
        let value_of = add_static_unary(
            UnqualifiedName::VALUEOF,
            FieldType::object(classes.lang.integer),
        );
        let bit_count = add_static_unary(UnqualifiedName::BITCOUNT, FieldType::int());
        let number_of_leading_zeros =
            add_static_unary(UnqualifiedName::NUMBEROFLEADINGZEROS, FieldType::int());
        let number_of_trailing_zeros =
            add_static_unary(UnqualifiedName::NUMBEROFTRAILINGZEROS, FieldType::int());

        let add_static_binary = |name: UnqualifiedName| -> &'g MethodData<'g> {
            class_graph.add_method(MethodData {
                class,
                name,
                descriptor: MethodDescriptor {
                    parameters: vec![FieldType::int(), FieldType::int()],
                    return_type: Some(FieldType::int()),
                },
                is_static: true,
            })
        };
        let compare = add_static_binary(UnqualifiedName::COMPARE);
        let compare_unsigned = add_static_binary(UnqualifiedName::COMPAREUNSIGNED);
        let divide_unsigned = add_static_binary(UnqualifiedName::DIVIDEUNSIGNED);
        let remainder_unsigned = add_static_binary(UnqualifiedName::REMAINDERUNSIGNED);
        let rotate_left = add_static_binary(UnqualifiedName::ROTATELEFT);
        let rotate_right = add_static_binary(UnqualifiedName::ROTATERIGHT);

        let add_static_field = |name: UnqualifiedName,
                                descriptor: FieldType<&'g ClassData<'g>>|
         -> &'g FieldData<'g> {
            class_graph.add_field(FieldData {
                class,
                name,
                descriptor,
                is_static: true,
            })
        };
        let max_value = add_static_field(UnqualifiedName::MAXVALUE, FieldType::int());
        let min_value = add_static_field(UnqualifiedName::MINVALUE, FieldType::int());
        let r#type = add_static_field(
            UnqualifiedName::UPPERCASE_TYPE,
            FieldType::object(classes.lang.class),
        );

        IntegerMembers {
            value_of,
            bit_count,
            number_of_leading_zeros,
            number_of_trailing_zeros,
            compare,
            compare_unsigned,
            divide_unsigned,
            remainder_unsigned,
            rotate_left,
            rotate_right,
            max_value,
            min_value,
            r#type,
        }
    }
}

impl<'g> FloatMembers<'g> {
    pub fn add_to_graph(
        class_graph: &ClassGraph<'g>,
        classes: &JavaClasses<'g>,
    ) -> FloatMembers<'g> {
        let class = classes.lang.float;

        let add_unary_operator = |name: UnqualifiedName,
                                  input: FieldType<&'g ClassData<'g>>,
                                  output: FieldType<&'g ClassData<'g>>|
         -> &'g MethodData<'g> {
            class_graph.add_method(MethodData {
                class,
                name,
                descriptor: MethodDescriptor {
                    parameters: vec![input],
                    return_type: Some(output),
                },
                is_static: true,
            })
        };
        let value_of = add_unary_operator(
            UnqualifiedName::VALUEOF,
            FieldType::float(),
            FieldType::object(classes.lang.float),
        );
        let float_to_raw_int_bits = add_unary_operator(
            UnqualifiedName::FLOATTORAWINTBITS,
            FieldType::float(),
            FieldType::int(),
        );
        let int_bits_to_float = add_unary_operator(
            UnqualifiedName::INTBITSTOFLOAT,
            FieldType::int(),
            FieldType::float(),
        );

        let add_binary_operator = |name: UnqualifiedName| -> &'g MethodData<'g> {
            class_graph.add_method(MethodData {
                class,
                name,
                descriptor: MethodDescriptor {
                    parameters: vec![FieldType::float(), FieldType::float()],
                    return_type: Some(FieldType::float()),
                },
                is_static: true,
            })
        };
        let max = add_binary_operator(UnqualifiedName::MAX);
        let min = add_binary_operator(UnqualifiedName::MIN);

        let add_static_field =
            |name: UnqualifiedName, field_ty: FieldType<&'g ClassData<'g>>| -> &'g FieldData<'g> {
                class_graph.add_field(FieldData {
                    class,
                    name,
                    descriptor: field_ty,
                    is_static: true,
                })
            };
        let max_value = add_static_field(UnqualifiedName::MAXVALUE, FieldType::float());
        let min_value = add_static_field(UnqualifiedName::MINVALUE, FieldType::float());
        let nan = add_static_field(UnqualifiedName::NAN, FieldType::float());
        let negative_infinity =
            add_static_field(UnqualifiedName::NEGATIVEINFINITY, FieldType::float());
        let positive_infinity =
            add_static_field(UnqualifiedName::POSITIVEINFINITY, FieldType::float());
        let r#type = add_static_field(
            UnqualifiedName::UPPERCASE_TYPE,
            FieldType::object(classes.lang.class),
        );

        FloatMembers {
            value_of,
            float_to_raw_int_bits,
            int_bits_to_float,
            max,
            min,
            max_value,
            min_value,
            nan,
            negative_infinity,
            positive_infinity,
            r#type,
        }
    }
}

impl<'g> LongMembers<'g> {
    pub fn add_to_graph(
        class_graph: &ClassGraph<'g>,
        classes: &JavaClasses<'g>,
    ) -> LongMembers<'g> {
        let class = classes.lang.long;

        let add_static_unary = |name: UnqualifiedName,
                                output_type: FieldType<&'g ClassData<'g>>|
         -> &'g MethodData<'g> {
            class_graph.add_method(MethodData {
                class,
                name,
                descriptor: MethodDescriptor {
                    parameters: vec![FieldType::long()],
                    return_type: Some(output_type),
                },
                is_static: true,
            })
        };
        let value_of = add_static_unary(
            UnqualifiedName::VALUEOF,
            FieldType::object(classes.lang.long),
        );
        let bit_count = add_static_unary(UnqualifiedName::BITCOUNT, FieldType::int());
        let number_of_leading_zeros =
            add_static_unary(UnqualifiedName::NUMBEROFLEADINGZEROS, FieldType::int());
        let number_of_trailing_zeros =
            add_static_unary(UnqualifiedName::NUMBEROFTRAILINGZEROS, FieldType::int());

        let add_static_binary = |name: UnqualifiedName,
                                 parameters: Vec<FieldType<&'g ClassData<'g>>>,
                                 ret: FieldType<&'g ClassData<'g>>|
         -> &'g MethodData<'g> {
            class_graph.add_method(MethodData {
                class,
                name,
                descriptor: MethodDescriptor {
                    parameters,
                    return_type: Some(ret),
                },
                is_static: true,
            })
        };
        let compare = add_static_binary(
            UnqualifiedName::COMPARE,
            vec![FieldType::long(), FieldType::long()],
            FieldType::int(),
        );
        let compare_unsigned = add_static_binary(
            UnqualifiedName::COMPAREUNSIGNED,
            vec![FieldType::long(), FieldType::long()],
            FieldType::int(),
        );
        let divide_unsigned = add_static_binary(
            UnqualifiedName::DIVIDEUNSIGNED,
            vec![FieldType::long(), FieldType::long()],
            FieldType::long(),
        );
        let remainder_unsigned = add_static_binary(
            UnqualifiedName::REMAINDERUNSIGNED,
            vec![FieldType::long(), FieldType::long()],
            FieldType::long(),
        );
        let rotate_left = add_static_binary(
            UnqualifiedName::ROTATELEFT,
            vec![FieldType::long(), FieldType::int()],
            FieldType::long(),
        );
        let rotate_right = add_static_binary(
            UnqualifiedName::ROTATERIGHT,
            vec![FieldType::long(), FieldType::int()],
            FieldType::long(),
        );

        let add_static_field = |name: UnqualifiedName,
                                descriptor: FieldType<&'g ClassData<'g>>|
         -> &'g FieldData<'g> {
            class_graph.add_field(FieldData {
                class,
                name,
                descriptor,
                is_static: true,
            })
        };
        let max_value = add_static_field(UnqualifiedName::MAXVALUE, FieldType::long());
        let min_value = add_static_field(UnqualifiedName::MINVALUE, FieldType::long());
        let r#type = add_static_field(
            UnqualifiedName::UPPERCASE_TYPE,
            FieldType::object(classes.lang.class),
        );

        LongMembers {
            value_of,
            bit_count,
            number_of_leading_zeros,
            number_of_trailing_zeros,
            compare,
            compare_unsigned,
            divide_unsigned,
            remainder_unsigned,
            rotate_left,
            rotate_right,
            max_value,
            min_value,
            r#type,
        }
    }
}

impl<'g> DoubleMembers<'g> {
    pub fn add_to_graph(
        class_graph: &ClassGraph<'g>,
        classes: &JavaClasses<'g>,
    ) -> DoubleMembers<'g> {
        let class = classes.lang.double;

        let add_unary_operator = |name: UnqualifiedName,
                                  input: FieldType<&'g ClassData<'g>>,
                                  output: FieldType<&'g ClassData<'g>>|
         -> &'g MethodData<'g> {
            class_graph.add_method(MethodData {
                class,
                name,
                descriptor: MethodDescriptor {
                    parameters: vec![input],
                    return_type: Some(output),
                },
                is_static: true,
            })
        };
        let value_of = add_unary_operator(
            UnqualifiedName::VALUEOF,
            FieldType::double(),
            FieldType::object(class),
        );
        let double_to_raw_long_bits = add_unary_operator(
            UnqualifiedName::DOUBLETORAWLONGBITS,
            FieldType::double(),
            FieldType::long(),
        );
        let long_bits_to_double = add_unary_operator(
            UnqualifiedName::LONGBITSTODOUBLE,
            FieldType::long(),
            FieldType::double(),
        );

        let add_binary_operator = |name: UnqualifiedName| -> &'g MethodData<'g> {
            class_graph.add_method(MethodData {
                class,
                name,
                descriptor: MethodDescriptor {
                    parameters: vec![FieldType::double(), FieldType::double()],
                    return_type: Some(FieldType::double()),
                },
                is_static: true,
            })
        };
        let max = add_binary_operator(UnqualifiedName::MAX);
        let min = add_binary_operator(UnqualifiedName::MIN);

        let add_static_field =
            |name: UnqualifiedName, field_ty: FieldType<&'g ClassData<'g>>| -> &'g FieldData<'g> {
                class_graph.add_field(FieldData {
                    class,
                    name,
                    descriptor: field_ty,
                    is_static: true,
                })
            };
        let max_value = add_static_field(UnqualifiedName::MAXVALUE, FieldType::double());
        let min_value = add_static_field(UnqualifiedName::MINVALUE, FieldType::double());
        let nan = add_static_field(UnqualifiedName::NAN, FieldType::double());
        let negative_infinity =
            add_static_field(UnqualifiedName::NEGATIVEINFINITY, FieldType::double());
        let positive_infinity =
            add_static_field(UnqualifiedName::POSITIVEINFINITY, FieldType::double());
        let r#type = add_static_field(
            UnqualifiedName::UPPERCASE_TYPE,
            FieldType::object(classes.lang.class),
        );

        DoubleMembers {
            value_of,
            double_to_raw_long_bits,
            long_bits_to_double,
            max,
            min,
            max_value,
            min_value,
            nan,
            negative_infinity,
            positive_infinity,
            r#type,
        }
    }
}

impl<'g> VoidMembers<'g> {
    pub fn add_to_graph(
        class_graph: &ClassGraph<'g>,
        classes: &JavaClasses<'g>,
    ) -> VoidMembers<'g> {
        let class = classes.lang.void;
        let r#type = class_graph.add_field(FieldData {
            class,
            name: UnqualifiedName::UPPERCASE_TYPE,
            descriptor: FieldType::object(classes.lang.class),
            is_static: true,
        });
        VoidMembers { r#type }
    }
}

impl<'g> BooleanMembers<'g> {
    pub fn add_to_graph(
        class_graph: &ClassGraph<'g>,
        classes: &JavaClasses<'g>,
    ) -> BooleanMembers<'g> {
        let class = classes.lang.boolean;
        let value_of = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::VALUEOF,
            descriptor: MethodDescriptor {
                parameters: vec![FieldType::boolean()],
                return_type: Some(FieldType::object(classes.lang.boolean)),
            },
            is_static: true,
        });
        let r#type = class_graph.add_field(FieldData {
            class,
            name: UnqualifiedName::UPPERCASE_TYPE,
            descriptor: FieldType::object(classes.lang.class),
            is_static: true,
        });
        BooleanMembers { value_of, r#type }
    }
}

impl<'g> MathMembers<'g> {
    pub fn add_to_graph(
        class_graph: &ClassGraph<'g>,
        classes: &JavaClasses<'g>,
    ) -> MathMembers<'g> {
        let class = classes.lang.math;
        let add_double_transformer = |name: UnqualifiedName| -> &'g MethodData<'g> {
            class_graph.add_method(MethodData {
                class,
                name,
                descriptor: MethodDescriptor {
                    parameters: vec![FieldType::double()],
                    return_type: Some(FieldType::double()),
                },
                is_static: true,
            })
        };
        let ceil = add_double_transformer(UnqualifiedName::CEIL);
        let floor = add_double_transformer(UnqualifiedName::FLOOR);
        let sqrt = add_double_transformer(UnqualifiedName::SQRT);
        let rint = add_double_transformer(UnqualifiedName::RINT);

        let add_binary_operator = |name: UnqualifiedName,
                                   operator_type: FieldType<&'g ClassData<'g>>|
         -> &'g MethodData<'g> {
            class_graph.add_method(MethodData {
                class,
                name,
                descriptor: MethodDescriptor {
                    parameters: vec![operator_type.clone(), operator_type.clone()],
                    return_type: Some(operator_type),
                },
                is_static: true,
            })
        };
        let copy_sign_float = add_binary_operator(UnqualifiedName::COPYSIGN, FieldType::float());
        let copy_sign_double = add_binary_operator(UnqualifiedName::COPYSIGN, FieldType::double());
        let add_exact = add_binary_operator(UnqualifiedName::ADDEXACT, FieldType::int());

        let add_unary_operator = |name: UnqualifiedName,
                                  operator_type: FieldType<&'g ClassData<'g>>|
         -> &'g MethodData<'g> {
            class_graph.add_method(MethodData {
                class,
                name,
                descriptor: MethodDescriptor {
                    parameters: vec![operator_type.clone()],
                    return_type: Some(operator_type),
                },
                is_static: true,
            })
        };
        let abs_float = add_unary_operator(UnqualifiedName::ABS, FieldType::float());
        let abs_double = add_unary_operator(UnqualifiedName::ABS, FieldType::double());

        let to_int_exact = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::TOINTEXACT,
            descriptor: MethodDescriptor {
                parameters: vec![FieldType::long()],
                return_type: Some(FieldType::int()),
            },
            is_static: true,
        });

        MathMembers {
            ceil,
            floor,
            sqrt,
            rint,
            copy_sign_float,
            copy_sign_double,
            abs_float,
            abs_double,
            to_int_exact,
            add_exact,
        }
    }
}

impl<'g> SystemMembers<'g> {
    pub fn add_to_graph(
        class_graph: &ClassGraph<'g>,
        classes: &JavaClasses<'g>,
    ) -> SystemMembers<'g> {
        let class = classes.lang.system;
        let arraycopy = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::ARRAYCOPY,
            descriptor: MethodDescriptor {
                parameters: vec![
                    FieldType::object(classes.lang.object),
                    FieldType::int(),
                    FieldType::object(classes.lang.object),
                    FieldType::int(),
                    FieldType::int(),
                ],
                return_type: None,
            },
            is_static: true,
        });
        SystemMembers { arraycopy }
    }
}

impl<'g> InvokeMembers<'g> {
    pub fn add_to_graph(
        class_graph: &ClassGraph<'g>,
        classes: &JavaClasses<'g>,
    ) -> InvokeMembers<'g> {
        let method_type = MethodTypeMembers::add_to_graph(class_graph, classes);
        let method_handle = MethodHandleMembers::add_to_graph(class_graph, classes);
        let method_handles = MethodHandlesMembers::add_to_graph(class_graph, classes);
        let call_site = CallSiteMembers::add_to_graph(class_graph, classes);
        let constant_call_site = ConstantCallSiteMembers::add_to_graph(class_graph, classes);
        let mutable_call_site = MutableCallSiteMembers::add_to_graph(class_graph, classes);
        InvokeMembers {
            method_type,
            method_handle,
            method_handles,
            call_site,
            constant_call_site,
            mutable_call_site,
        }
    }
}

impl<'g> MethodTypeMembers<'g> {
    pub fn add_to_graph(
        class_graph: &ClassGraph<'g>,
        classes: &JavaClasses<'g>,
    ) -> MethodTypeMembers<'g> {
        let class = classes.lang.invoke.method_type;
        let parameter_count = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::PARAMETERCOUNT,
            descriptor: MethodDescriptor {
                parameters: vec![],
                return_type: Some(FieldType::int()),
            },
            is_static: false,
        });
        let parameter_type = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::PARAMETERTYPE,
            descriptor: MethodDescriptor {
                parameters: vec![FieldType::int()],
                return_type: Some(FieldType::object(classes.lang.class)),
            },
            is_static: false,
        });
        let parameter_array = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::PARAMETERARRAY,
            descriptor: MethodDescriptor {
                parameters: vec![],
                return_type: Some(FieldType::array(FieldType::object(classes.lang.class))),
            },
            is_static: false,
        });
        let drop_parameter_types = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::DROPPARAMETERTYPES,
            descriptor: MethodDescriptor {
                parameters: vec![FieldType::int(), FieldType::int()],
                return_type: Some(FieldType::object(classes.lang.invoke.method_type)),
            },
            is_static: false,
        });
        let return_type = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::RETURNTYPE,
            descriptor: MethodDescriptor {
                parameters: vec![],
                return_type: Some(FieldType::object(classes.lang.class)),
            },
            is_static: false,
        });
        let method_type = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::METHODTYPE,
            descriptor: MethodDescriptor {
                parameters: vec![
                    FieldType::object(classes.lang.class),
                    FieldType::array(FieldType::object(classes.lang.class)),
                ],
                return_type: Some(FieldType::object(classes.lang.invoke.method_type)),
            },
            is_static: true,
        });
        MethodTypeMembers {
            parameter_count,
            parameter_type,
            parameter_array,
            drop_parameter_types,
            return_type,
            method_type,
        }
    }
}

impl<'g> MethodHandleMembers<'g> {
    pub fn add_to_graph(
        class_graph: &ClassGraph<'g>,
        classes: &JavaClasses<'g>,
    ) -> MethodHandleMembers<'g> {
        let class = classes.lang.invoke.method_handle;
        let r#type = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::TYPE,
            descriptor: MethodDescriptor {
                parameters: vec![],
                return_type: Some(FieldType::object(classes.lang.invoke.method_type)),
            },
            is_static: false,
        });
        let as_type = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::ASTYPE,
            descriptor: MethodDescriptor {
                parameters: vec![FieldType::object(classes.lang.invoke.method_type)],
                return_type: Some(FieldType::object(classes.lang.invoke.method_handle)),
            },
            is_static: false,
        });
        let change_return_type = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::CHANGERETURNTYPE,
            descriptor: MethodDescriptor {
                parameters: vec![FieldType::object(classes.lang.class)],
                return_type: Some(FieldType::object(classes.lang.invoke.method_type)),
            },
            is_static: false,
        });
        MethodHandleMembers {
            r#type,
            as_type,
            change_return_type,
        }
    }
}

impl<'g> MethodHandlesMembers<'g> {
    pub fn add_to_graph(
        class_graph: &ClassGraph<'g>,
        classes: &JavaClasses<'g>,
    ) -> MethodHandlesMembers<'g> {
        let class = classes.lang.invoke.method_handles;
        let drop_arguments = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::DROPARGUMENTS,
            descriptor: MethodDescriptor {
                parameters: vec![
                    FieldType::object(classes.lang.invoke.method_handle),
                    FieldType::int(),
                    FieldType::array(FieldType::object(classes.lang.class)),
                ],
                return_type: Some(FieldType::object(classes.lang.invoke.method_handle)),
            },
            is_static: true,
        });
        let permute_arguments = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::PERMUTEARGUMENTS,
            descriptor: MethodDescriptor {
                parameters: vec![
                    FieldType::object(classes.lang.invoke.method_handle),
                    FieldType::object(classes.lang.invoke.method_type),
                    FieldType::array(FieldType::int()),
                ],
                return_type: Some(FieldType::object(classes.lang.invoke.method_handle)),
            },
            is_static: true,
        });
        let collect_arguments = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::COLLECTARGUMENTS,
            descriptor: MethodDescriptor {
                parameters: vec![
                    FieldType::object(classes.lang.invoke.method_handle),
                    FieldType::int(),
                    FieldType::object(classes.lang.invoke.method_handle),
                ],
                return_type: Some(FieldType::object(classes.lang.invoke.method_handle)),
            },
            is_static: true,
        });
        let insert_arguments = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::INSERTARGUMENTS,
            descriptor: MethodDescriptor {
                parameters: vec![
                    FieldType::object(classes.lang.invoke.method_handle),
                    FieldType::int(),
                    FieldType::array(FieldType::object(classes.lang.object)),
                ],
                return_type: Some(FieldType::object(classes.lang.invoke.method_handle)),
            },
            is_static: true,
        });
        let exact_invoker = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::EXACTINVOKER,
            descriptor: MethodDescriptor {
                parameters: vec![FieldType::object(classes.lang.invoke.method_type)],
                return_type: Some(FieldType::object(classes.lang.invoke.method_handle)),
            },
            is_static: true,
        });
        let filter_return_value = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::FILTERRETURNVALUE,
            descriptor: MethodDescriptor {
                parameters: vec![
                    FieldType::object(classes.lang.invoke.method_handle),
                    FieldType::object(classes.lang.invoke.method_handle),
                ],
                return_type: Some(FieldType::object(classes.lang.invoke.method_handle)),
            },
            is_static: true,
        });
        let guard_with_test = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::GUARDWITHTEST,
            descriptor: MethodDescriptor {
                parameters: vec![
                    FieldType::object(classes.lang.invoke.method_handle),
                    FieldType::object(classes.lang.invoke.method_handle),
                    FieldType::object(classes.lang.invoke.method_handle),
                ],
                return_type: Some(FieldType::object(classes.lang.invoke.method_handle)),
            },
            is_static: true,
        });
        let array_constructor = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::ARRAYCONSTRUCTOR,
            descriptor: MethodDescriptor {
                parameters: vec![FieldType::object(classes.lang.class)],
                return_type: Some(FieldType::object(classes.lang.invoke.method_handle)),
            },
            is_static: true,
        });
        let array_element_getter = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::ARRAYELEMENTGETTER,
            descriptor: MethodDescriptor {
                parameters: vec![FieldType::object(classes.lang.class)],
                return_type: Some(FieldType::object(classes.lang.invoke.method_handle)),
            },
            is_static: true,
        });
        let array_element_setter = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::ARRAYELEMENTSETTER,
            descriptor: MethodDescriptor {
                parameters: vec![FieldType::object(classes.lang.class)],
                return_type: Some(FieldType::object(classes.lang.invoke.method_handle)),
            },
            is_static: true,
        });
        let array_length = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::ARRAYLENGTH,
            descriptor: MethodDescriptor {
                parameters: vec![FieldType::object(classes.lang.class)],
                return_type: Some(FieldType::object(classes.lang.invoke.method_handle)),
            },
            is_static: true,
        });
        let empty = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::EMPTY,
            descriptor: MethodDescriptor {
                parameters: vec![FieldType::object(classes.lang.invoke.method_type)],
                return_type: Some(FieldType::object(classes.lang.invoke.method_handle)),
            },
            is_static: true,
        });
        let constant = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::CONSTANT,
            descriptor: MethodDescriptor {
                parameters: vec![
                    FieldType::object(classes.lang.class),
                    FieldType::object(classes.lang.object),
                ],
                return_type: Some(FieldType::object(classes.lang.invoke.method_handle)),
            },
            is_static: true,
        });

        MethodHandlesMembers {
            drop_arguments,
            permute_arguments,
            collect_arguments,
            insert_arguments,
            exact_invoker,
            filter_return_value,
            guard_with_test,
            array_constructor,
            array_element_getter,
            array_element_setter,
            array_length,
            empty,
            constant,
        }
    }
}

impl<'g> CallSiteMembers<'g> {
    pub fn add_to_graph(
        class_graph: &ClassGraph<'g>,
        classes: &JavaClasses<'g>,
    ) -> CallSiteMembers<'g> {
        let class = classes.lang.invoke.call_site;
        let dynamic_invoker = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::DYNAMICINVOKER,
            descriptor: MethodDescriptor {
                parameters: vec![],
                return_type: Some(FieldType::object(classes.lang.invoke.method_handle)),
            },
            is_static: false,
        });
        let get_target = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::GETTARGET,
            descriptor: MethodDescriptor {
                parameters: vec![],
                return_type: Some(FieldType::object(classes.lang.invoke.method_handle)),
            },
            is_static: false,
        });
        let set_target = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::SETTARGET,
            descriptor: MethodDescriptor {
                parameters: vec![FieldType::object(classes.lang.invoke.method_handle)],
                return_type: None,
            },
            is_static: false,
        });
        let r#type = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::TYPE,
            descriptor: MethodDescriptor {
                parameters: vec![],
                return_type: Some(FieldType::object(classes.lang.invoke.method_type)),
            },
            is_static: false,
        });

        CallSiteMembers {
            dynamic_invoker,
            get_target,
            set_target,
            r#type,
        }
    }
}

impl<'g> ConstantCallSiteMembers<'g> {
    pub fn add_to_graph(
        class_graph: &ClassGraph<'g>,
        classes: &JavaClasses<'g>,
    ) -> ConstantCallSiteMembers<'g> {
        let class = classes.lang.invoke.constant_call_site;
        let init = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::INIT,
            descriptor: MethodDescriptor {
                parameters: vec![FieldType::object(classes.lang.invoke.method_handle)],
                return_type: None,
            },
            is_static: false,
        });
        ConstantCallSiteMembers { init }
    }
}

impl<'g> MutableCallSiteMembers<'g> {
    pub fn add_to_graph(
        class_graph: &ClassGraph<'g>,
        classes: &JavaClasses<'g>,
    ) -> MutableCallSiteMembers<'g> {
        let class = classes.lang.invoke.mutable_call_site;
        let sync_all = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::SYNCALL,
            descriptor: MethodDescriptor {
                parameters: vec![FieldType::array(FieldType::object(
                    classes.lang.invoke.mutable_call_site,
                ))],
                return_type: None,
            },
            is_static: true,
        });
        let init = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::INIT,
            descriptor: MethodDescriptor {
                parameters: vec![FieldType::object(classes.lang.invoke.method_handle)],
                return_type: None,
            },
            is_static: false,
        });
        MutableCallSiteMembers { sync_all, init }
    }
}

impl<'g> ThrowableMembers<'g> {
    pub fn add_to_graph(
        class_graph: &ClassGraph<'g>,
        classes: &JavaClasses<'g>,
    ) -> ThrowableMembers<'g> {
        let class = classes.lang.throwable;
        let init = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::INIT,
            descriptor: MethodDescriptor {
                parameters: vec![FieldType::object(classes.lang.string)],
                return_type: None,
            },
            is_static: false,
        });
        ThrowableMembers { init }
    }
}

impl<'g> ErrorMembers<'g> {
    pub fn add_to_graph(
        class_graph: &ClassGraph<'g>,
        classes: &JavaClasses<'g>,
    ) -> ErrorMembers<'g> {
        let class = classes.lang.error;
        let init = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::INIT,
            descriptor: MethodDescriptor {
                parameters: vec![FieldType::object(classes.lang.string)],
                return_type: None,
            },
            is_static: false,
        });
        ErrorMembers { init }
    }
}

impl<'g> AssertionErrorMembers<'g> {
    pub fn add_to_graph(
        class_graph: &ClassGraph<'g>,
        classes: &JavaClasses<'g>,
    ) -> AssertionErrorMembers<'g> {
        let class = classes.lang.assertion_error;
        let init = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::INIT,
            descriptor: MethodDescriptor {
                parameters: vec![FieldType::object(classes.lang.string)],
                return_type: None,
            },
            is_static: false,
        });
        AssertionErrorMembers { init }
    }
}

impl<'g> ExceptionMembers<'g> {
    pub fn add_to_graph(
        class_graph: &ClassGraph<'g>,
        classes: &JavaClasses<'g>,
    ) -> ExceptionMembers<'g> {
        let class = classes.lang.exception;
        let init = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::INIT,
            descriptor: MethodDescriptor {
                parameters: vec![FieldType::object(classes.lang.string)],
                return_type: None,
            },
            is_static: false,
        });
        ExceptionMembers { init }
    }
}

impl<'g> RuntimeExceptionMembers<'g> {
    pub fn add_to_graph(
        class_graph: &ClassGraph<'g>,
        classes: &JavaClasses<'g>,
    ) -> RuntimeExceptionMembers<'g> {
        let class = classes.lang.runtime_exception;
        let init = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::INIT,
            descriptor: MethodDescriptor {
                parameters: vec![FieldType::object(classes.lang.string)],
                return_type: None,
            },
            is_static: false,
        });
        RuntimeExceptionMembers { init }
    }
}

impl<'g> ArithmeticExceptionMembers<'g> {
    pub fn add_to_graph(
        class_graph: &ClassGraph<'g>,
        classes: &JavaClasses<'g>,
    ) -> ArithmeticExceptionMembers<'g> {
        let class = classes.lang.arithmetic_exception;
        let init = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::INIT,
            descriptor: MethodDescriptor {
                parameters: vec![FieldType::object(classes.lang.string)],
                return_type: None,
            },
            is_static: false,
        });
        ArithmeticExceptionMembers { init }
    }
}

impl<'g> IllegalArgumentExceptionMembers<'g> {
    pub fn add_to_graph(
        class_graph: &ClassGraph<'g>,
        classes: &JavaClasses<'g>,
    ) -> IllegalArgumentExceptionMembers<'g> {
        let class = classes.lang.illegal_argument_exception;
        let init = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::INIT,
            descriptor: MethodDescriptor {
                parameters: vec![FieldType::object(classes.lang.string)],
                return_type: None,
            },
            is_static: false,
        });
        IllegalArgumentExceptionMembers { init }
    }
}

impl<'g> NioMembers<'g> {
    pub fn add_to_graph(class_graph: &ClassGraph<'g>, classes: &JavaClasses<'g>) -> NioMembers<'g> {
        let buffer = BufferMembers::add_to_graph(class_graph, classes);
        let byte_buffer = ByteBufferMembers::add_to_graph(class_graph, classes);
        let byte_order = ByteOrderMembers::add_to_graph(class_graph, classes);
        NioMembers {
            buffer,
            byte_buffer,
            byte_order,
        }
    }
}

impl<'g> BufferMembers<'g> {
    pub fn add_to_graph(
        class_graph: &ClassGraph<'g>,
        classes: &JavaClasses<'g>,
    ) -> BufferMembers<'g> {
        let class = classes.nio.buffer;
        let position = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::POSITION,
            descriptor: MethodDescriptor {
                parameters: vec![FieldType::int()],
                return_type: Some(FieldType::object(classes.nio.buffer)),
            },
            is_static: false,
        });
        let capacity = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::CAPACITY,
            descriptor: MethodDescriptor {
                parameters: vec![],
                return_type: Some(FieldType::int()),
            },
            is_static: false,
        });
        BufferMembers { position, capacity }
    }
}

impl<'g> ByteBufferMembers<'g> {
    pub fn add_to_graph(
        class_graph: &ClassGraph<'g>,
        classes: &JavaClasses<'g>,
    ) -> ByteBufferMembers<'g> {
        let class = classes.nio.byte_buffer;
        let add_allocate = |name: UnqualifiedName| -> &'g MethodData<'g> {
            class_graph.add_method(MethodData {
                class,
                name,
                descriptor: MethodDescriptor {
                    parameters: vec![FieldType::int()],
                    return_type: Some(FieldType::object(classes.nio.byte_buffer)),
                },
                is_static: true,
            })
        };
        let allocate = add_allocate(UnqualifiedName::ALLOCATE);
        let allocate_direct = add_allocate(UnqualifiedName::ALLOCATEDIRECT);
        let capacity = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::CAPACITY,
            descriptor: MethodDescriptor {
                parameters: vec![],
                return_type: Some(FieldType::int()),
            },
            is_static: false,
        });

        let add_get =
            |name: UnqualifiedName, typ: FieldType<&'g ClassData<'g>>| -> &'g MethodData<'g> {
                class_graph.add_method(MethodData {
                    class,
                    name,
                    descriptor: MethodDescriptor {
                        parameters: vec![FieldType::int()],
                        return_type: Some(typ),
                    },
                    is_static: false,
                })
            };
        let get_byte = add_get(UnqualifiedName::GET, FieldType::byte());
        let get_short = add_get(UnqualifiedName::GETSHORT, FieldType::short());
        let get_int = add_get(UnqualifiedName::GETINT, FieldType::int());
        let get_float = add_get(UnqualifiedName::GETFLOAT, FieldType::float());
        let get_long = add_get(UnqualifiedName::GETLONG, FieldType::long());
        let get_double = add_get(UnqualifiedName::GETDOUBLE, FieldType::double());

        let add_put =
            |name: UnqualifiedName, typ: FieldType<&'g ClassData<'g>>| -> &'g MethodData<'g> {
                class_graph.add_method(MethodData {
                    class,
                    name,
                    descriptor: MethodDescriptor {
                        parameters: vec![FieldType::int(), typ],
                        return_type: Some(FieldType::object(classes.nio.byte_buffer)),
                    },
                    is_static: false,
                })
            };
        let put_byte = add_put(UnqualifiedName::PUT, FieldType::byte());
        let put_short = add_put(UnqualifiedName::PUTSHORT, FieldType::short());
        let put_int = add_put(UnqualifiedName::PUTINT, FieldType::int());
        let put_float = add_put(UnqualifiedName::PUTFLOAT, FieldType::float());
        let put_long = add_put(UnqualifiedName::PUTLONG, FieldType::long());
        let put_double = add_put(UnqualifiedName::PUTDOUBLE, FieldType::double());

        let add_relative_put = |typ: FieldType<&'g ClassData<'g>>| -> &'g MethodData<'g> {
            class_graph.add_method(MethodData {
                class,
                name: UnqualifiedName::PUT,
                descriptor: MethodDescriptor {
                    parameters: vec![typ],
                    return_type: Some(FieldType::object(classes.nio.byte_buffer)),
                },
                is_static: false,
            })
        };
        let put_bytebuffer = add_relative_put(FieldType::object(classes.nio.byte_buffer));
        let put_bytearray = add_relative_put(FieldType::array(FieldType::byte()));
        let put_byte_relative = add_relative_put(FieldType::byte());

        let position = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::POSITION,
            descriptor: MethodDescriptor {
                parameters: vec![FieldType::int()],
                return_type: Some(FieldType::object(classes.nio.buffer)),
            },
            is_static: false,
        });
        let order = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::ORDER,
            descriptor: MethodDescriptor {
                parameters: vec![FieldType::object(classes.nio.byte_order)],
                return_type: Some(FieldType::object(classes.nio.byte_buffer)),
            },
            is_static: false,
        });

        ByteBufferMembers {
            allocate,
            allocate_direct,
            capacity,
            get_byte,
            put_byte,
            get_short,
            put_short,
            get_int,
            put_int,
            get_float,
            put_float,
            get_long,
            put_long,
            get_double,
            put_double,
            put_bytebuffer,
            put_bytearray,
            put_byte_relative,
            position,
            order,
        }
    }
}

impl<'g> ByteOrderMembers<'g> {
    pub fn add_to_graph(
        class_graph: &ClassGraph<'g>,
        classes: &JavaClasses<'g>,
    ) -> ByteOrderMembers<'g> {
        let class = classes.nio.byte_order;
        let add_endian = |name: UnqualifiedName| -> &'g FieldData<'g> {
            class_graph.add_field(FieldData {
                class,
                name,
                descriptor: FieldType::object(classes.nio.byte_order),
                is_static: true,
            })
        };
        let big_endian = add_endian(UnqualifiedName::BIGENDIAN);
        let little_endian = add_endian(UnqualifiedName::LITTLEENDIAN);
        ByteOrderMembers {
            big_endian,
            little_endian,
        }
    }
}

impl<'g> UtilMembers<'g> {
    pub fn add_to_graph(
        class_graph: &ClassGraph<'g>,
        classes: &JavaClasses<'g>,
    ) -> UtilMembers<'g> {
        let arrays = ArraysMembers::add_to_graph(class_graph, classes);
        let map = MapMembers::add_to_graph(class_graph, classes);
        let hash_map = HashMapMembers::add_to_graph(class_graph, classes);
        UtilMembers {
            arrays,
            map,
            hash_map,
        }
    }
}

impl<'g> ArraysMembers<'g> {
    pub fn add_to_graph(
        class_graph: &ClassGraph<'g>,
        classes: &JavaClasses<'g>,
    ) -> ArraysMembers<'g> {
        let class = classes.util.arrays;
        let copy_of = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::COPYOF,
            descriptor: MethodDescriptor {
                parameters: vec![
                    FieldType::array(FieldType::object(classes.lang.object)),
                    FieldType::int(),
                ],
                return_type: Some(FieldType::array(FieldType::object(classes.lang.object))),
            },
            is_static: true,
        });
        let fill = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::FILL,
            descriptor: MethodDescriptor {
                parameters: vec![
                    FieldType::array(FieldType::object(classes.lang.object)),
                    FieldType::int(),
                    FieldType::int(),
                    FieldType::object(classes.lang.object),
                ],
                return_type: None,
            },
            is_static: true,
        });
        ArraysMembers { copy_of, fill }
    }
}

impl<'g> MapMembers<'g> {
    pub fn add_to_graph(class_graph: &ClassGraph<'g>, classes: &JavaClasses<'g>) -> MapMembers<'g> {
        let class = classes.util.map;
        let get = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::GET,
            descriptor: MethodDescriptor {
                parameters: vec![FieldType::object(classes.lang.object)],
                return_type: Some(FieldType::object(classes.lang.object)),
            },
            is_static: false,
        });
        let put = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::PUT,
            descriptor: MethodDescriptor {
                parameters: vec![
                    FieldType::object(classes.lang.object),
                    FieldType::object(classes.lang.object),
                ],
                return_type: Some(FieldType::object(classes.lang.object)),
            },
            is_static: false,
        });
        MapMembers { get, put }
    }
}

impl<'g> HashMapMembers<'g> {
    pub fn add_to_graph(
        class_graph: &ClassGraph<'g>,
        classes: &JavaClasses<'g>,
    ) -> HashMapMembers<'g> {
        let class = classes.util.hash_map;
        let init = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::INIT,
            descriptor: MethodDescriptor {
                parameters: vec![],
                return_type: None,
            },
            is_static: false,
        });
        HashMapMembers { init }
    }
}
