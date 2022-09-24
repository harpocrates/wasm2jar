use super::{RuntimeClasses, WasmRuntime};
use crate::jvm::class_graph::{
    ClassGraph, FieldData, FieldId, JavaClasses, JavaLibrary, MethodData, MethodId,
};
use crate::jvm::code::CodeBuilder;
use crate::jvm::model::{Class, Field, Method};
use crate::jvm::{
    Error, FieldAccessFlags, FieldType, MethodAccessFlags, MethodDescriptor, Name, UnqualifiedName,
};

/// Members of `org.wasm2jar.Memory`
pub struct MemoryMembers<'g> {
    pub init: MethodId<'g>,
    pub bytes: FieldId<'g>,
}

impl<'g> MemoryMembers<'g> {
    pub fn add_to_graph(
        class_graph: &ClassGraph<'g>,
        java_classes: &JavaClasses<'g>,
        classes: &RuntimeClasses<'g>,
    ) -> MemoryMembers<'g> {
        let class = classes.memory;
        let init = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::INIT,
            access_flags: MethodAccessFlags::PUBLIC,
            descriptor: MethodDescriptor {
                parameters: vec![FieldType::object(java_classes.nio.byte_buffer)],
                return_type: None,
            },
        });
        let bytes = class_graph.add_field(FieldData {
            class,
            name: UnqualifiedName::from_str_unsafe("bytes"),
            access_flags: FieldAccessFlags::PUBLIC,
            descriptor: FieldType::object(java_classes.nio.byte_buffer),
        });

        MemoryMembers { init, bytes }
    }
}

pub fn make_memory_class<'g>(
    class_graph: &'g ClassGraph<'g>,
    java: &'g JavaLibrary<'g>,
    runtime: &WasmRuntime<'g>,
) -> Result<Class<'g>, Error> {
    use crate::jvm::code::{BranchInstruction::*, Instruction::*, InvokeType};

    let mut class = Class::new(runtime.classes.memory);
    class.add_field(Field::new(runtime.members.memory.bytes));

    // Constructor
    let mut code = CodeBuilder::new(class_graph, java, runtime.members.memory.init);
    code.push_instruction(ALoad(0))?;
    code.push_instruction(Invoke(InvokeType::Special, java.members.lang.object.init))?;
    code.push_instruction(ALoad(0))?;
    code.push_instruction(ALoad(1))?;
    code.push_instruction(PutField(runtime.members.memory.bytes))?;
    code.push_branch_instruction(Return)?;

    let mut constructor = Method::new(runtime.members.memory.init);
    constructor.code_impl = Some(code.result()?);
    class.add_method(constructor);

    Ok(class)
}
