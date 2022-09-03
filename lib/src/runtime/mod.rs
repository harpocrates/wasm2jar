//! Runtime types used in the `wasm2jar` interface
mod function;
mod global;
mod memory;
mod table;

pub use function::*;
pub use global::*;
pub use memory::*;
pub use table::*;

// TODO: support generating these in a custom package (eg. `org.wasm2jar`)
// TODO: consider a more complex class hierarchy (immutable or not, resizable or not, specialized
// globals)

use crate::jvm::class_graph::{ClassData, ClassGraph, ClassId, JavaClasses};
use crate::jvm::{BinaryName, ClassAccessFlags, Name};

pub struct WasmRuntime<'g> {
    pub classes: RuntimeClasses<'g>,
    pub members: RuntimeMembers<'g>,
}

impl<'g> WasmRuntime<'g> {
    // TODO: customize package
    pub fn add_to_graph(
        class_graph: &ClassGraph<'g>,
        java_classes: &JavaClasses<'g>,
    ) -> WasmRuntime<'g> {
        let classes = RuntimeClasses::add_to_graph(class_graph, java_classes);
        let members = RuntimeMembers::add_to_graph(class_graph, java_classes, &classes);
        WasmRuntime { classes, members }
    }
}

/// Classes inside `org.wasm2jar.*`
pub struct RuntimeClasses<'g> {
    pub function: ClassId<'g>,
    pub global: ClassId<'g>,
    pub function_table: ClassId<'g>,
    pub reference_table: ClassId<'g>,
    pub memory: ClassId<'g>,
}

/// Members of classes inside `org.wasm2jar.*`
pub struct RuntimeMembers<'g> {
    pub function: FunctionMembers<'g>,
    pub global: GlobalMembers<'g>,
    pub function_table: FunctionTableMembers<'g>,
    pub reference_table: ReferenceTableMembers<'g>,
    pub memory: MemoryMembers<'g>,
}

impl<'g> RuntimeClasses<'g> {
    pub fn add_to_graph(
        class_graph: &ClassGraph<'g>,
        java_classes: &JavaClasses<'g>,
    ) -> RuntimeClasses<'g> {
        let function = class_graph.add_class(ClassData::new(
            BinaryName::from_str_unsafe("org/wasm2jar/Function"),
            java_classes.lang.object,
            ClassAccessFlags::SUPER | ClassAccessFlags::PUBLIC,
            None,
        ));
        let global = class_graph.add_class(ClassData::new(
            BinaryName::from_str_unsafe("org/wasm2jar/Global"),
            java_classes.lang.object,
            ClassAccessFlags::SUPER | ClassAccessFlags::PUBLIC,
            None,
        ));
        let function_table = class_graph.add_class(ClassData::new(
            BinaryName::from_str_unsafe("org/wasm2jar/FunctionTable"),
            java_classes.lang.object,
            ClassAccessFlags::SUPER | ClassAccessFlags::PUBLIC,
            None,
        ));
        let reference_table = class_graph.add_class(ClassData::new(
            BinaryName::from_str_unsafe("org/wasm2jar/ReferenceTable"),
            java_classes.lang.object,
            ClassAccessFlags::SUPER | ClassAccessFlags::PUBLIC,
            None,
        ));
        let memory = class_graph.add_class(ClassData::new(
            BinaryName::from_str_unsafe("org/wasm2jar/Memory"),
            java_classes.lang.object,
            ClassAccessFlags::SUPER | ClassAccessFlags::PUBLIC,
            None,
        ));

        RuntimeClasses {
            function,
            global,
            function_table,
            reference_table,
            memory,
        }
    }
}

impl<'g> RuntimeMembers<'g> {
    pub fn add_to_graph(
        class_graph: &ClassGraph<'g>,
        java_classes: &JavaClasses<'g>,
        classes: &RuntimeClasses<'g>,
    ) -> RuntimeMembers<'g> {
        let function = FunctionMembers::add_to_graph(class_graph, java_classes, classes);
        let global = GlobalMembers::add_to_graph(class_graph, java_classes, classes);
        let function_table = FunctionTableMembers::add_to_graph(class_graph, java_classes, classes);
        let reference_table =
            ReferenceTableMembers::add_to_graph(class_graph, java_classes, classes);
        let memory = MemoryMembers::add_to_graph(class_graph, java_classes, classes);

        RuntimeMembers {
            function,
            global,
            function_table,
            reference_table,
            memory,
        }
    }
}
