use super::{RuntimeClasses, WasmRuntime};
use crate::jvm::class_graph::{
    ClassGraph, FieldData, FieldId, JavaClasses, JavaLibrary, MethodData, MethodId,
};
use crate::jvm::code::CodeBuilder;
use crate::jvm::model::{Class, Field, Method};
use crate::jvm::{
    Error, FieldAccessFlags, FieldType, MethodAccessFlags, MethodDescriptor, Name, UnqualifiedName,
};

/// Members of `org.wasm2jar.Function`
pub struct FunctionMembers<'g> {
    pub init: MethodId<'g>,
    pub handle: FieldId<'g>,
}

impl<'g> FunctionMembers<'g> {
    pub fn add_to_graph(
        class_graph: &ClassGraph<'g>,
        java: &JavaClasses<'g>,
        runtime: &RuntimeClasses<'g>,
    ) -> FunctionMembers<'g> {
        let class = runtime.function;
        let init = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::INIT,
            access_flags: MethodAccessFlags::PUBLIC,
            descriptor: MethodDescriptor {
                parameters: vec![FieldType::object(java.lang.invoke.method_handle)],
                return_type: None,
            },
        });
        let handle = class_graph.add_field(FieldData {
            class,
            name: UnqualifiedName::from_str_unsafe("handle"),
            access_flags: FieldAccessFlags::PUBLIC | FieldAccessFlags::FINAL,
            descriptor: FieldType::object(java.lang.invoke.method_handle),
        });

        FunctionMembers { init, handle }
    }
}

pub fn make_function_class<'g>(
    class_graph: &'g ClassGraph<'g>,
    java: &'g JavaLibrary<'g>,
    runtime: &WasmRuntime<'g>,
) -> Result<Class<'g>, Error> {
    use crate::jvm::code::{BranchInstruction::*, Instruction::*, InvokeType};

    let mut class = Class::new(runtime.classes.function);
    class.add_field(Field::new(runtime.members.function.handle));

    // Constructor
    let mut code = CodeBuilder::new(class_graph, java, runtime.members.function.init);
    code.push_instruction(ALoad(0))?;
    code.push_instruction(Invoke(InvokeType::Special, java.members.lang.object.init))?;
    code.push_instruction(ALoad(0))?;
    code.push_instruction(ALoad(1))?;
    code.push_instruction(PutField(runtime.members.function.handle))?;
    code.push_branch_instruction(Return)?;

    let mut constructor = Method::new(runtime.members.function.init);
    constructor.code_impl = Some(code.result()?);
    class.add_method(constructor);

    Ok(class)
}
