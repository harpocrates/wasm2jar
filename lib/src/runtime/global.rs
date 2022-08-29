use super::{RuntimeClasses, WasmRuntime};
use crate::jvm::class_graph::{
    ClassGraph, FieldData, FieldId, JavaClasses, JavaLibrary, MethodData, MethodId,
};
use crate::jvm::code::CodeBuilder;
use crate::jvm::model::{Class, Field, Method};
use crate::jvm::{
    Error, FieldAccessFlags, FieldType, MethodAccessFlags, MethodDescriptor, Name, UnqualifiedName,
};

/// Members of `org.wasm2jar.Global`
pub struct GlobalMembers<'g> {
    pub init: MethodId<'g>,
    pub value: FieldId<'g>,
}

impl<'g> GlobalMembers<'g> {
    pub fn add_to_graph(
        class_graph: &ClassGraph<'g>,
        java_classes: &JavaClasses<'g>,
        classes: &RuntimeClasses<'g>,
    ) -> GlobalMembers<'g> {
        let class = classes.global;
        let init = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::INIT,
            access_flags: MethodAccessFlags::PUBLIC,
            descriptor: MethodDescriptor {
                parameters: vec![FieldType::object(java_classes.lang.object)],
                return_type: None,
            },
        });
        let value = class_graph.add_field(FieldData {
            class,
            name: UnqualifiedName::from_str_unsafe("value"),
            access_flags: FieldAccessFlags::PUBLIC,
            descriptor: FieldType::object(java_classes.lang.object),
        });

        GlobalMembers { init, value }
    }
}

pub fn make_global_class<'g>(
    class_graph: &'g ClassGraph<'g>,
    java: &'g JavaLibrary<'g>,
    runtime: &WasmRuntime<'g>,
) -> Result<Class<'g>, Error> {
    use crate::jvm::code::{BranchInstruction::*, Instruction::*, InvokeType};

    let mut class = Class::new(runtime.classes.global);
    class.add_field(Field::new(runtime.members.global.value));

    // Constructor
    let mut code = CodeBuilder::new(&class_graph, &java, runtime.members.global.init);
    code.push_instruction(ALoad(0))?;
    code.push_instruction(Invoke(InvokeType::Special, java.members.lang.object.init))?;
    code.push_instruction(ALoad(0))?;
    code.push_instruction(ALoad(1))?;
    code.push_instruction(PutField(runtime.members.global.value))?;
    code.push_branch_instruction(Return)?;

    let mut constructor = Method::new(runtime.members.global.init);
    constructor.code_impl = Some(code.result()?);
    class.add_method(constructor);

    Ok(class)
}
