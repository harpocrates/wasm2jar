use super::{BinaryName, ClassAccessFlags, ClassData, ClassGraph, InnerClassAccessFlags, NestData};
use elsa::FrozenVec;

/// Classes inside `java.*`
pub struct JavaClasses<'g> {
    pub lang: LangClasses<'g>,
    pub nio: NioClasses<'g>,
    pub util: UtilClasses<'g>,
}

/// Classes inside `java.lang.*`
pub struct LangClasses<'g> {
    pub object: &'g ClassData<'g>,
    pub char_sequence: &'g ClassData<'g>,
    pub string: &'g ClassData<'g>,
    pub class: &'g ClassData<'g>,
    pub number: &'g ClassData<'g>,
    pub integer: &'g ClassData<'g>,
    pub float: &'g ClassData<'g>,
    pub long: &'g ClassData<'g>,
    pub double: &'g ClassData<'g>,
    pub void: &'g ClassData<'g>,
    pub boolean: &'g ClassData<'g>,
    pub math: &'g ClassData<'g>,
    pub system: &'g ClassData<'g>,
    pub invoke: InvokeClasses<'g>,
    pub throwable: &'g ClassData<'g>,
    pub error: &'g ClassData<'g>,
    pub assertion_error: &'g ClassData<'g>,
    pub exception: &'g ClassData<'g>,
    pub runtime_exception: &'g ClassData<'g>,
    pub arithmetic_exception: &'g ClassData<'g>,
    pub illegal_argument_exception: &'g ClassData<'g>,
}

/// Classes inside `java.lang.invoke.*`
pub struct InvokeClasses<'g> {
    pub method_type: &'g ClassData<'g>,
    pub method_handle: &'g ClassData<'g>,
    pub method_handles: &'g ClassData<'g>,
    pub method_handles_lookup: &'g ClassData<'g>,
    pub call_site: &'g ClassData<'g>,
    pub constant_call_site: &'g ClassData<'g>,
    pub mutable_call_site: &'g ClassData<'g>,
}

/// Classes inside `java.nio.*`
pub struct NioClasses<'g> {
    pub buffer: &'g ClassData<'g>,
    pub byte_buffer: &'g ClassData<'g>,
    pub byte_order: &'g ClassData<'g>,
}

/// Classes inside `java.util.*`
pub struct UtilClasses<'g> {
    pub arrays: &'g ClassData<'g>,
    pub map: &'g ClassData<'g>,
    pub hash_map: &'g ClassData<'g>,
}

impl<'g> JavaClasses<'g> {
    pub fn add_to_graph(class_graph: &ClassGraph<'g>) -> JavaClasses<'g> {
        let lang = LangClasses::add_to_graph(class_graph);
        let nio = NioClasses::add_to_graph(class_graph, lang.object);
        let util = UtilClasses::add_to_graph(class_graph, lang.object);

        JavaClasses { lang, nio, util }
    }
}

impl<'g> LangClasses<'g> {
    pub fn add_to_graph(class_graph: &ClassGraph<'g>) -> LangClasses<'g> {
        let object = class_graph.add_class(ClassData {
            name: BinaryName::OBJECT,
            superclass: None,
            interfaces: FrozenVec::new(),
            access_flags: ClassAccessFlags::PUBLIC | ClassAccessFlags::SUPER,
            methods: FrozenVec::new(),
            fields: FrozenVec::new(),
            nest: NestData::Host { members: FrozenVec::new() },
        });

        let char_sequence = class_graph.add_class(ClassData::new(
            BinaryName::CHARSEQUENCE,
            object,
            ClassAccessFlags::SUPER | ClassAccessFlags::PUBLIC | ClassAccessFlags::INTERFACE,
            None,
        ));
        let string =
            class_graph.add_class(ClassData::new(BinaryName::STRING, object, ClassAccessFlags::SUPER | ClassAccessFlags::PUBLIC | ClassAccessFlags::FINAL, None));
        let class = class_graph.add_class(ClassData::new(BinaryName::CLASS, object, ClassAccessFlags::SUPER | ClassAccessFlags::PUBLIC | ClassAccessFlags::FINAL, None));
        let number = class_graph.add_class(ClassData::new(BinaryName::NUMBER, object, ClassAccessFlags::SUPER | ClassAccessFlags::PUBLIC, None));
        let integer =
            class_graph.add_class(ClassData::new(BinaryName::INTEGER, number, ClassAccessFlags::SUPER | ClassAccessFlags::PUBLIC | ClassAccessFlags::FINAL, None));
        let float = class_graph.add_class(ClassData::new(BinaryName::FLOAT, number, ClassAccessFlags::SUPER | ClassAccessFlags::PUBLIC | ClassAccessFlags::FINAL, None));
        let long = class_graph.add_class(ClassData::new(BinaryName::LONG, number, ClassAccessFlags::SUPER | ClassAccessFlags::PUBLIC | ClassAccessFlags::FINAL, None));
        let double =
            class_graph.add_class(ClassData::new(BinaryName::DOUBLE, number, ClassAccessFlags::SUPER | ClassAccessFlags::PUBLIC | ClassAccessFlags::FINAL, None));
        let void = class_graph.add_class(ClassData::new(BinaryName::VOID, object, ClassAccessFlags::SUPER | ClassAccessFlags::PUBLIC | ClassAccessFlags::FINAL, None));
        let boolean =
            class_graph.add_class(ClassData::new(BinaryName::BOOLEAN, object, ClassAccessFlags::SUPER | ClassAccessFlags::PUBLIC | ClassAccessFlags::FINAL, None));
        let math = class_graph.add_class(ClassData::new(BinaryName::MATH, object, ClassAccessFlags::SUPER | ClassAccessFlags::PUBLIC | ClassAccessFlags::FINAL, None));
        let system =
            class_graph.add_class(ClassData::new(BinaryName::SYSTEM, object, ClassAccessFlags::SUPER | ClassAccessFlags::PUBLIC | ClassAccessFlags::FINAL, None));
        let invoke = InvokeClasses::add_to_graph(class_graph, object);
        let throwable =
            class_graph.add_class(ClassData::new(BinaryName::THROWABLE, object, ClassAccessFlags::SUPER | ClassAccessFlags::PUBLIC, None));
        let error = class_graph.add_class(ClassData::new(BinaryName::ERROR, throwable, ClassAccessFlags::SUPER | ClassAccessFlags::PUBLIC, None));
        let assertion_error =
            class_graph.add_class(ClassData::new(BinaryName::ASSERTIONERROR, error, ClassAccessFlags::SUPER | ClassAccessFlags::PUBLIC, None));
        let exception =
            class_graph.add_class(ClassData::new(BinaryName::EXCEPTION, throwable, ClassAccessFlags::SUPER | ClassAccessFlags::PUBLIC, None));
        let runtime_exception = class_graph.add_class(ClassData::new(
            BinaryName::RUNTIMEEXCEPTION,
            exception,
            ClassAccessFlags::SUPER | ClassAccessFlags::PUBLIC,
            None,
        ));
        let arithmetic_exception = class_graph.add_class(ClassData::new(
            BinaryName::ARITHMETICEXCEPTION,
            runtime_exception,
            ClassAccessFlags::SUPER | ClassAccessFlags::PUBLIC,
            None,
        ));
        let illegal_argument_exception = class_graph.add_class(ClassData::new(
            BinaryName::ILLEGALARGUMENTEXCEPTION,
            runtime_exception,
            ClassAccessFlags::SUPER | ClassAccessFlags::PUBLIC,
            None
        ));

        string.interfaces.push(char_sequence);

        LangClasses {
            object,
            char_sequence,
            string,
            class,
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

impl<'g> InvokeClasses<'g> {
    pub fn add_to_graph(
        class_graph: &ClassGraph<'g>,
        object: &'g ClassData<'g>,
    ) -> InvokeClasses<'g> {
        let method_type = class_graph.add_class(ClassData::new(
            BinaryName::METHODTYPE,
            object,
            ClassAccessFlags::SUPER | ClassAccessFlags::PUBLIC | ClassAccessFlags::FINAL,
            None,
        ));
        let method_handle = class_graph.add_class(ClassData::new(
            BinaryName::METHODHANDLE,
            object,
            ClassAccessFlags::SUPER | ClassAccessFlags::PUBLIC | ClassAccessFlags::FINAL,
            None,
        ));
        let method_handles =
            class_graph.add_class(ClassData::new(BinaryName::METHODHANDLES, object, ClassAccessFlags::SUPER | ClassAccessFlags::PUBLIC, None));
        let method_handles_lookup = class_graph.add_class(ClassData::new(
            BinaryName::METHODHANDLES_LOOKUP,
            object,
            ClassAccessFlags::SUPER | ClassAccessFlags::PUBLIC | ClassAccessFlags::FINAL,
            Some((InnerClassAccessFlags::PUBLIC | InnerClassAccessFlags::STATIC, method_handles)),
        ));
        let call_site = class_graph.add_class(ClassData::new(
            BinaryName::CALLSITE,
            object,
            ClassAccessFlags::SUPER | ClassAccessFlags::PUBLIC | ClassAccessFlags::ABSTRACT,
            None
        ));
        let constant_call_site = class_graph.add_class(ClassData::new(
            BinaryName::CONSTANTCALLSITE,
            call_site,
            ClassAccessFlags::SUPER | ClassAccessFlags::PUBLIC,
            None,
        ));
        let mutable_call_site = class_graph.add_class(ClassData::new(
            BinaryName::MUTABLECALLSITE,
            call_site,
            ClassAccessFlags::SUPER | ClassAccessFlags::PUBLIC,
            None,
        ));

        InvokeClasses {
            method_type,
            method_handle,
            method_handles,
            method_handles_lookup,
            call_site,
            constant_call_site,
            mutable_call_site,
        }
    }
}

impl<'g> NioClasses<'g> {
    pub fn add_to_graph(class_graph: &ClassGraph<'g>, object: &'g ClassData<'g>) -> NioClasses<'g> {
        let byte_order =
            class_graph.add_class(ClassData::new(BinaryName::BYTEORDER, object, ClassAccessFlags::SUPER | ClassAccessFlags::PUBLIC | ClassAccessFlags::FINAL, None));
        let buffer = class_graph.add_class(ClassData::new(BinaryName::BUFFER, object, ClassAccessFlags::SUPER | ClassAccessFlags::PUBLIC, None));
        let byte_buffer = class_graph.add_class(ClassData::new(
            BinaryName::BYTEBUFFER,
            buffer,
            ClassAccessFlags::SUPER | ClassAccessFlags::PUBLIC | ClassAccessFlags::ABSTRACT,
            None,
        ));

        NioClasses {
            buffer,
            byte_buffer,
            byte_order,
        }
    }
}

impl<'g> UtilClasses<'g> {
    pub fn add_to_graph(
        class_graph: &ClassGraph<'g>,
        object: &'g ClassData<'g>,
    ) -> UtilClasses<'g> {
        let arrays = class_graph.add_class(ClassData::new(BinaryName::ARRAYS, object, ClassAccessFlags::SUPER | ClassAccessFlags::PUBLIC, None));
        let map = class_graph.add_class(ClassData::new(BinaryName::MAP, object, ClassAccessFlags::SUPER | ClassAccessFlags::PUBLIC | ClassAccessFlags::INTERFACE, None));
        let hash_map = class_graph.add_class(ClassData::new(BinaryName::HASHMAP, object, ClassAccessFlags::SUPER | ClassAccessFlags::PUBLIC, None));

        hash_map.interfaces.push(map);

        UtilClasses {
            arrays,
            map,
            hash_map,
        }
    }
}
