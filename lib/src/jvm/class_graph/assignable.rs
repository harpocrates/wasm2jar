use crate::jvm::class_graph::ClassId;
use crate::jvm::{BinaryName, RefType};
use crate::util::RefId;
use std::cmp::Ordering;
use std::collections::HashSet;

/// Subtyping relationship between types
pub trait Assignable {
    /// Is the first type assignable to the second?
    fn is_assignable(&self, super_type: &Self) -> bool;
}

/// This does a traversal of super types in the class graph to determine assignability
impl<'g> Assignable for ClassId<'g> {
    fn is_assignable(&self, super_type: &ClassId<'g>) -> bool {
        let mut supertypes_to_visit: Vec<ClassId<'g>> = vec![*self];
        let mut dont_revisit: HashSet<ClassId<'g>> = HashSet::new();
        dont_revisit.insert(*self);

        // Optimization: if the super type is a class, then skip visiting interfaces
        let super_is_class: bool = !super_type.is_interface();

        while let Some(class_data) = supertypes_to_visit.pop() {
            if class_data == *super_type {
                return true;
            }
            let class_data = class_data.0;

            // Enqueue next types to visit
            if let Some(superclass) = class_data.superclass {
                if dont_revisit.insert(superclass) {
                    supertypes_to_visit.push(superclass);
                }
            }
            if !super_is_class {
                for interface in &class_data.interfaces {
                    let interface = RefId(interface);
                    if dont_revisit.insert(interface) {
                        supertypes_to_visit.push(interface);
                    }
                }
            }
        }

        false
    }
}

/// This matches the semantics of the prolog predicate `isJavaAssignable(sub_type, super_type)` in
/// the JVM verifier specification.
impl<'g> Assignable for RefType<ClassId<'g>> {
    fn is_assignable(&self, super_type: &RefType<ClassId<'g>>) -> bool {
        match (self, super_type) {
            // Special superclass and interfaces of all arrays
            (
                RefType::PrimitiveArray(_) | RefType::ObjectArray(_),
                RefType::Object(object_type),
            ) => is_array_type_assignable(&object_type.name),

            // Primitive arrays must match in dimension and type
            (RefType::PrimitiveArray(arr1), RefType::PrimitiveArray(arr2)) => arr1 == arr2,

            // Higher dimensional primitive arrays can be subtypes of object arrays
            (RefType::PrimitiveArray(arr1), RefType::ObjectArray(arr2)) => {
                match arr1.additional_dimensions.cmp(&arr2.additional_dimensions) {
                    Ordering::Less | Ordering::Equal => false,
                    Ordering::Greater => is_array_type_assignable(&arr2.element_type.name),
                }
            }

            // Cursed (unsound) covariance of arrays
            (RefType::ObjectArray(arr1), RefType::ObjectArray(arr2)) => {
                match arr1.additional_dimensions.cmp(&arr2.additional_dimensions) {
                    Ordering::Less => false,
                    Ordering::Equal => arr1.element_type.is_assignable(&arr2.element_type),
                    Ordering::Greater => is_array_type_assignable(&arr2.element_type.name),
                }
            }

            // Object-to-object assignability holds if there is a path through super type edges
            (RefType::Object(cls1), RefType::Object(cls2)) => cls1.is_assignable(cls2),

            _ => false,
        }
    }
}

/// Check if arrays can be assigned to a super type
///
/// This bakes in knowledge of the small, finite set of super types arrays have.
fn is_array_type_assignable(super_type: &BinaryName) -> bool {
    super_type == &BinaryName::OBJECT
        || super_type == &BinaryName::CLONEABLE
        || super_type == &BinaryName::SERIALIZABLE
}

#[cfg(test)]
mod test {
    use crate::jvm::class_graph::{Assignable, ClassGraph, ClassGraphArenas};
    use crate::jvm::{FieldType, RefType};

    #[test]
    fn simple_classes() {
        let arenas = ClassGraphArenas::new();
        let class_graph = ClassGraph::new(&arenas);
        let java = class_graph.insert_java_library_types();

        let object_cls = &java.classes.lang.object;
        let string_cls = &java.classes.lang.string;

        assert!(
            object_cls.is_assignable(object_cls),
            "java.lang.Object <: java.lang.Object"
        );
        assert!(
            string_cls.is_assignable(string_cls),
            "java.lang.String <: java.lang.String"
        );
        assert!(
            string_cls.is_assignable(object_cls),
            "java.lang.String <: java.lang.Object"
        );
        assert!(
            !object_cls.is_assignable(string_cls),
            "java.lang.Object </: java.lang.String"
        );
    }

    #[test]
    fn transitive_classes() {
        let arenas = ClassGraphArenas::new();
        let class_graph = ClassGraph::new(&arenas);
        let java = class_graph.insert_java_library_types();

        let object_cls = &java.classes.lang.object;
        let number_cls = &java.classes.lang.number;
        let integer_cls = &java.classes.lang.integer;

        assert!(
            number_cls.is_assignable(object_cls),
            "java.lang.Number <: java.lang.Object"
        );
        assert!(
            integer_cls.is_assignable(number_cls),
            "java.lang.Integer <: java.lang.Number"
        );
        assert!(
            integer_cls.is_assignable(object_cls),
            "java.lang.Integer <: java.lang.Object"
        );

        assert!(
            !object_cls.is_assignable(number_cls),
            "java.lang.Object </: java.lang.Number"
        );
        assert!(
            !number_cls.is_assignable(integer_cls),
            "java.lang.Number </: java.lang.Integer"
        );
        assert!(
            !object_cls.is_assignable(integer_cls),
            "java.lang.Object </: java.lang.Integer"
        );
    }

    #[test]
    fn simple_interfaces() {
        let arenas = ClassGraphArenas::new();
        let class_graph = ClassGraph::new(&arenas);
        let java = class_graph.insert_java_library_types();

        let object_cls = &java.classes.lang.object;
        let string_cls = &java.classes.lang.string;
        let charsequence_cls = &java.classes.lang.char_sequence;

        assert!(
            string_cls.is_assignable(charsequence_cls),
            "java.lang.String <: java.lang.CharSequence"
        );
        assert!(
            charsequence_cls.is_assignable(object_cls),
            "java.lang.CharSequence <: java.lang.Object"
        );
        assert!(
            !charsequence_cls.is_assignable(string_cls),
            "java.lang.CharSequence </: java.lang.String"
        );
        assert!(
            !object_cls.is_assignable(charsequence_cls),
            "java.lang.Object </: java.lang.CharSequence"
        );
    }

    #[test]
    fn primitive_arrays() {
        let arenas = ClassGraphArenas::new();
        let class_graph = ClassGraph::new(&arenas);
        let java = class_graph.insert_java_library_types();

        let object_cls = &RefType::Object(java.classes.lang.object);
        let int_array = &RefType::array(FieldType::int());
        let long_array = &RefType::array(FieldType::long());

        assert!(
            int_array.is_assignable(object_cls),
            "[]int <: java.lang.Object"
        );
        assert!(
            int_array.is_assignable(object_cls),
            "[]long <: java.lang.Object"
        );
        assert!(
            !object_cls.is_assignable(int_array),
            "java.lang.Object </: []int"
        );
        assert!(
            !object_cls.is_assignable(long_array),
            "java.lang.Object </: []long"
        );

        assert!(int_array.is_assignable(int_array), "[]int <: []int");
        assert!(long_array.is_assignable(long_array), "[]long <: []long");

        assert!(!int_array.is_assignable(long_array), "[]int </: []long");
        assert!(!long_array.is_assignable(int_array), "[]long </: []int");
    }

    #[test]
    fn object_arrays() {
        let arenas = ClassGraphArenas::new();
        let class_graph = ClassGraph::new(&arenas);
        let java = class_graph.insert_java_library_types();

        let object_cls = &RefType::Object(java.classes.lang.object);

        let int_array = &RefType::array(FieldType::int());
        let integer_array = &RefType::array(FieldType::object(java.classes.lang.integer));
        let number_array = &RefType::array(FieldType::object(java.classes.lang.number));

        assert!(
            !int_array.is_assignable(integer_array),
            "[]int </: []java.lang.Integer"
        );
        assert!(
            !integer_array.is_assignable(int_array),
            "[]java.lang.Integer </: []int"
        );

        assert!(
            integer_array.is_assignable(object_cls),
            "[]java.lang.Integer <: java.lang.Object"
        );
        assert!(
            number_array.is_assignable(object_cls),
            "[]java.lang.Number <: java.lang.Object"
        );
        assert!(
            !object_cls.is_assignable(integer_array),
            "java.lang.Object </: []java.lang.Integer"
        );
        assert!(
            !object_cls.is_assignable(number_array),
            "java.lang.Object </: []java.lang.Number"
        );

        assert!(
            integer_array.is_assignable(integer_array),
            "[]java.lang.Integer <: []java.lang.Integer"
        );
        assert!(
            number_array.is_assignable(number_array),
            "[]java.lang.Number <: []java.lang.Number"
        );
        assert!(
            integer_array.is_assignable(number_array),
            "[]java.lang.Integer <: []java.lang.Number"
        );
        assert!(
            !number_array.is_assignable(integer_array),
            "[]java.lang.Number <: []java.lang.Integer"
        );
    }

    #[test]
    fn nested_arrays() {
        let arenas = ClassGraphArenas::new();
        let class_graph = ClassGraph::new(&arenas);
        let java = class_graph.insert_java_library_types();

        let object_cls = &RefType::Object(java.classes.lang.object);

        let object_array = &RefType::array(FieldType::object(java.classes.lang.object));
        let nested_int_array = &RefType::array(FieldType::array(FieldType::int()));
        let nested_integer_array = &RefType::array(FieldType::array(FieldType::object(
            java.classes.lang.integer,
        )));
        let nested_number_array = &RefType::array(FieldType::array(FieldType::object(
            java.classes.lang.number,
        )));

        assert!(
            nested_int_array.is_assignable(nested_int_array),
            "[][]int <: [][]int"
        );
        assert!(
            nested_integer_array.is_assignable(nested_integer_array),
            "[][]java.lang.Integer <: [][]java.lang.Integer"
        );

        assert!(
            nested_int_array.is_assignable(object_cls),
            "[][]int <: java.lang.Object",
        );
        assert!(
            nested_integer_array.is_assignable(object_cls),
            "[][]java.lang.Integer <: java.lang.Object",
        );
        assert!(
            !object_cls.is_assignable(nested_int_array),
            "java.lang.Object <!: [][]int",
        );
        assert!(
            !object_cls.is_assignable(nested_integer_array),
            "java.lang.Object <: [][]java.lang.Integert",
        );

        assert!(
            nested_int_array.is_assignable(object_array),
            "[][]int <: []java.lang.Object"
        );
        assert!(
            nested_integer_array.is_assignable(object_array),
            "[][]java.lang.Integer <: []java.lang.Object"
        );
        assert!(
            nested_number_array.is_assignable(object_array),
            "[][]java.lang.Number <: []java.lang.Object"
        );
        assert!(
            nested_integer_array.is_assignable(nested_number_array),
            "[][]java.lang.Integer <: [][]java.lang.Number"
        );

        assert!(
            !object_array.is_assignable(nested_int_array),
            "[]java.lang.Object <!: [][]int"
        );
        assert!(
            !object_array.is_assignable(nested_integer_array),
            "[]java.lang.Object <!: [][]java.lang.Integer"
        );
        assert!(
            !object_array.is_assignable(nested_number_array),
            "[]java.lang.Object <!: [][]java.lang.Number"
        );
        assert!(
            !nested_number_array.is_assignable(nested_integer_array),
            "[][]java.lang.Number <!: [][]java.lang.Integer"
        );
    }
}
