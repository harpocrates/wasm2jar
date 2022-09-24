use super::{RuntimeClasses, WasmRuntime};
use crate::jvm::class_graph::{
    ClassGraph, FieldData, FieldId, JavaClasses, JavaLibrary, MethodData, MethodId,
};
use crate::jvm::code::CodeBuilder;
use crate::jvm::model::{Class, Field, Method};
use crate::jvm::{
    Error, FieldAccessFlags, FieldType, MethodAccessFlags, MethodDescriptor, Name, UnqualifiedName,
};

/// Members of `org.wasm2jar.FunctionTable`
pub struct FunctionTableMembers<'g> {
    pub init: MethodId<'g>,
    pub table: FieldId<'g>,
}

/// Members of `org.wasm2jar.ReferenceTable`
pub struct ReferenceTableMembers<'g> {
    pub init: MethodId<'g>,
    pub table: FieldId<'g>,
}

impl<'g> FunctionTableMembers<'g> {
    pub fn add_to_graph(
        class_graph: &ClassGraph<'g>,
        java_classes: &JavaClasses<'g>,
        classes: &RuntimeClasses<'g>,
    ) -> FunctionTableMembers<'g> {
        let class = classes.function_table;
        let arr_type = FieldType::array(FieldType::object(java_classes.lang.invoke.method_handle));
        let init = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::INIT,
            access_flags: MethodAccessFlags::PUBLIC,
            descriptor: MethodDescriptor {
                parameters: vec![arr_type],
                return_type: None,
            },
        });
        let table = class_graph.add_field(FieldData {
            class,
            name: UnqualifiedName::from_str_unsafe("table"),
            access_flags: FieldAccessFlags::PUBLIC,
            descriptor: arr_type,
        });

        FunctionTableMembers { init, table }
    }
}

impl<'g> ReferenceTableMembers<'g> {
    pub fn add_to_graph(
        class_graph: &ClassGraph<'g>,
        java_classes: &JavaClasses<'g>,
        classes: &RuntimeClasses<'g>,
    ) -> ReferenceTableMembers<'g> {
        let class = classes.reference_table;
        let arr_type = FieldType::array(FieldType::object(java_classes.lang.object));
        let init = class_graph.add_method(MethodData {
            class,
            name: UnqualifiedName::INIT,
            access_flags: MethodAccessFlags::PUBLIC,
            descriptor: MethodDescriptor {
                parameters: vec![arr_type],
                return_type: None,
            },
        });
        let table = class_graph.add_field(FieldData {
            class,
            name: UnqualifiedName::from_str_unsafe("table"),
            access_flags: FieldAccessFlags::PUBLIC,
            descriptor: arr_type,
        });

        ReferenceTableMembers { init, table }
    }
}

pub fn make_function_table_class<'g>(
    class_graph: &'g ClassGraph<'g>,
    java: &'g JavaLibrary<'g>,
    runtime: &WasmRuntime<'g>,
) -> Result<Class<'g>, Error> {
    use crate::jvm::code::{BranchInstruction::*, Instruction::*, InvokeType};

    let mut class = Class::new(runtime.classes.function_table);
    class.add_field(Field::new(runtime.members.function_table.table));

    // Constructor
    let mut code = CodeBuilder::new(class_graph, java, runtime.members.function_table.init);
    code.push_instruction(ALoad(0))?;
    code.push_instruction(Invoke(InvokeType::Special, java.members.lang.object.init))?;
    code.push_instruction(ALoad(0))?;
    code.push_instruction(ALoad(1))?;
    code.push_instruction(PutField(runtime.members.function_table.table))?;
    code.push_branch_instruction(Return)?;

    let mut constructor = Method::new(runtime.members.function_table.init);
    constructor.code_impl = Some(code.result()?);
    class.add_method(constructor);

    Ok(class)
}

pub fn make_reference_table_class<'g>(
    class_graph: &'g ClassGraph<'g>,
    java: &'g JavaLibrary<'g>,
    runtime: &WasmRuntime<'g>,
) -> Result<Class<'g>, Error> {
    use crate::jvm::code::{BranchInstruction::*, Instruction::*, InvokeType};

    let mut class = Class::new(runtime.classes.reference_table);
    class.add_field(Field::new(runtime.members.reference_table.table));

    // Constructor
    let mut code = CodeBuilder::new(class_graph, java, runtime.members.reference_table.init);
    code.push_instruction(ALoad(0))?;
    code.push_instruction(Invoke(InvokeType::Special, java.members.lang.object.init))?;
    code.push_instruction(ALoad(0))?;
    code.push_instruction(ALoad(1))?;
    code.push_instruction(PutField(runtime.members.reference_table.table))?;
    code.push_branch_instruction(Return)?;

    let mut constructor = Method::new(runtime.members.reference_table.init);
    constructor.code_impl = Some(code.result()?);
    class.add_method(constructor);

    Ok(class)
}
