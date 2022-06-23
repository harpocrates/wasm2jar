use super::java_classes::JavaClasses;
use super::java_members::JavaMembers;
use super::ClassGraph;

pub struct JavaLibrary<'g> {
    pub classes: JavaClasses<'g>,
    pub members: JavaMembers<'g>,
}

impl<'g> JavaLibrary<'g> {
    pub fn add_to_graph(class_graph: &ClassGraph<'g>) -> JavaLibrary<'g> {
        let classes = JavaClasses::add_to_graph(class_graph);
        let members = JavaMembers::add_to_graph(class_graph, &classes);
        JavaLibrary { classes, members }
    }
}
