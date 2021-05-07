use wasm2jar::*;

fn main() {
    hello();
    make_class().unwrap();
}

fn make_class() -> Result<(), wasm2jar::jvm::Error> {
    use std::cell::RefCell;
    use std::fs::File;
    use std::rc::Rc;
    use wasm2jar::jvm::BranchInstruction::*;
    use wasm2jar::jvm::Instruction::*;
    use wasm2jar::jvm::*;

    let mut class_graph = ClassGraph::new();
    class_graph.insert_lang_types();
    let class_graph = Rc::new(RefCell::new(class_graph));

    let mut class_builder = ClassBuilder::new(
        ClassAccessFlags::PUBLIC,
        String::from("me/alec/Point"),
        String::from("java/lang/Object"),
        false,
        vec![],
        class_graph,
    )?;

    class_builder.add_field(
        FieldAccessFlags::PUBLIC,
        String::from("x"),
        String::from("I"),
    )?;
    class_builder.add_field(
        FieldAccessFlags::PUBLIC,
        String::from("y"),
        String::from("I"),
    )?;

    let mut method_builder = class_builder.start_method(
        MethodAccessFlags::PUBLIC,
        String::from("<init>"),
        String::from("(II)V"),
    )?;
    let code = &mut method_builder.code;

    let object_name = code.constants().get_utf8("java/lang/Object")?;
    let object_cls = code.constants().get_class(object_name)?;
    let init_name = code.constants().get_utf8("<init>")?;
    let type_name = code.constants().get_utf8("()V")?;
    let name_and_type = code.constants().get_name_and_type(init_name, type_name)?;
    let init_ref = code
        .constants()
        .get_method_ref(object_cls, name_and_type, false)?;

    let this_name = code.constants().get_utf8("me/alec/Point")?;
    let this_cls = code.constants().get_class(this_name)?;
    let field_name_x = code.constants().get_utf8("x")?;
    let field_name_y = code.constants().get_utf8("y")?;
    let field_typ = code.constants().get_utf8("I")?;
    let x_name_and_type = code
        .constants()
        .get_name_and_type(field_name_x, field_typ)?;
    let y_name_and_type = code
        .constants()
        .get_name_and_type(field_name_y, field_typ)?;
    let field_x = code.constants().get_field_ref(this_cls, x_name_and_type)?;
    let field_y = code.constants().get_field_ref(this_cls, y_name_and_type)?;

    let end = code.fresh_label();

    code.push_instruction(ALoad(0))?;
    code.push_instruction(Invoke(InvokeType::Special, init_ref))?;
    code.push_instruction(ALoad(0))?;
    code.push_instruction(ILoad(1))?;
    code.push_instruction(PutField(field_x))?;
    code.push_instruction(ILoad(2))?;
    code.push_branch_instruction(If(OrdComparison::LT, end, ()))?;

    code.push_instruction(ALoad(0))?;
    code.push_instruction(ILoad(2))?;
    code.push_instruction(PutField(field_y))?;

    code.place_label(end)?;
    code.push_branch_instruction(Return)?;

    class_builder.finish_method(method_builder)?;

    let class_file = class_builder.result();

    let mut f = File::create("Point.class").map_err(Error::IoError)?;
    class_file.serialize(&mut f).map_err(Error::IoError)?;

    Ok(())
}
