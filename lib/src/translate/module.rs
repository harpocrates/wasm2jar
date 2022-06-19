use super::{
    AccessMode, BootstrapUtilities, CodeBuilderExts, Element, Error, FunctionTranslator, Global,
    MemberOrigin, Memory, Settings, Table, UtilitiesStrategy, UtilityClass,
};
use crate::jvm::{
    BinaryName, BootstrapMethods, BranchInstruction, ClassAccessFlags, ClassBuilder, ClassFile,
    ClassGraph, CodeBuilder, ConstantIndex, Descriptor, FieldAccessFlags, FieldType, HandleKind,
    InnerClass, InnerClassAccessFlags, InnerClasses, Instruction, InvokeType, MethodAccessFlags,
    MethodDescriptor, Name, NestHost, NestMembers, RefType, UnqualifiedName, Width,
};
use crate::wasm::{ref_type_from_general, StackType, TableType, WasmModuleResourcesExt};
use std::cell::RefCell;
use std::iter;
use std::rc::Rc;
use wasmparser::types::Types;
use wasmparser::{
    Data, DataKind, DataSectionReader, ElementItem, ElementKind, ElementSectionReader, Export,
    ExportSectionReader, ExternalKind, FunctionBody, GlobalSectionReader, Import,
    ImportSectionReader, InitExpr, MemorySectionReader, Operator, Parser, Payload,
    TableSectionReader, TypeRef, Validator,
};

pub struct ModuleTranslator<'a> {
    settings: Settings,
    validator: Validator,
    #[allow(dead_code)]
    class_graph: Rc<RefCell<ClassGraph>>,
    class: ClassBuilder,
    previous_parts: Vec<ClassBuilder>,
    fields_generated: bool,

    current_part: CurrentPart,

    /// Utility class (just a carrier for whatever helper methods we may want)
    utilities: UtilityClass,

    /// Populated when we visit exports
    exports: Vec<Export<'a>>,

    /// Populated when we visit tables
    tables: Vec<Table>,

    /// Populated when we visit memories
    memories: Vec<Memory>,

    /// Populated when we visit globals
    globals: Vec<Global<'a>>,

    /// Populated when we visit elements
    elements: Vec<Element<'a>>,

    /// Populated when we visit datas
    datas: Vec<Data<'a>>,

    /// Every time we see a new function, this gets incremented
    current_func_idx: u32,
}

struct CurrentPart {
    class: ClassBuilder,
    bootstrap: BootstrapUtilities,
}
impl CurrentPart {
    fn result(mut self) -> Result<ClassBuilder, Error> {
        self.class
            .add_attribute(BootstrapMethods::from(self.bootstrap))?;
        Ok(self.class)
    }
}

impl<'a> ModuleTranslator<'a> {
    pub fn new(settings: Settings) -> Result<ModuleTranslator<'a>, Error> {
        let validator = Validator::new_with_features(settings.wasm_features);

        let mut class_graph = ClassGraph::new();
        class_graph.insert_lang_types();
        class_graph.insert_error_types();
        class_graph.insert_util_types();
        class_graph.insert_buffer_types();
        let class_graph = Rc::new(RefCell::new(class_graph));

        let class = ClassBuilder::new(
            ClassAccessFlags::PUBLIC | ClassAccessFlags::SUPER,
            settings.output_full_class_name.clone(),
            BinaryName::OBJECT,
            false,
            vec![],
            class_graph.clone(),
        )?;
        let current_part = Self::new_part(&settings, class_graph.clone(), 0)?;
        let utilities = UtilityClass::new(&settings, class_graph.clone())?;

        Ok(ModuleTranslator {
            settings,
            validator,
            class_graph,
            class,
            previous_parts: vec![],
            fields_generated: false,
            current_part,
            utilities,
            exports: vec![],
            tables: vec![],
            memories: vec![],
            globals: vec![],
            elements: vec![],
            datas: vec![],
            current_func_idx: 0,
        })
    }

    /// Parse a full module
    pub fn parse_module(&mut self, data: &'a [u8]) -> Result<Types, Error> {
        let parser = Parser::new(0);
        let mut types: Option<Types> = None;
        for payload in parser.parse_all(data) {
            let payload = payload?;
            if let Some(t) = self.process_payload(payload)? {
                types = Some(t);
            }
        }
        Ok(types.expect("Types should be available after having processed all payloads"))
    }

    /// Construct a new inner class part
    fn new_part(
        settings: &Settings,
        class_graph: Rc<RefCell<ClassGraph>>,
        part_idx: usize,
    ) -> Result<CurrentPart, Error> {
        let name = settings
            .part_short_class_name
            .concat(&UnqualifiedName::number(part_idx));
        let mut class = ClassBuilder::new(
            ClassAccessFlags::SYNTHETIC | ClassAccessFlags::SUPER,
            settings
                .output_full_class_name
                .concat(&UnqualifiedName::DOLLAR)
                .concat(&name),
            BinaryName::OBJECT,
            false,
            vec![],
            class_graph.clone(),
        )?;

        // Add the `NestHost` and `InnerClasses` attributes
        let (nest_host, inner_classes): (NestHost, InnerClasses) = {
            let mut constants = class.constants();
            let outer_class_name = constants.get_utf8(settings.output_full_class_name.as_str())?;
            let outer_class = constants.get_class(outer_class_name)?;
            let inner_class_name = constants.get_utf8(class.class_name().as_str())?;
            let inner_class = constants.get_class(inner_class_name)?;
            let inner_name = constants.get_utf8(name.as_str())?;
            let inner_class_attr = InnerClass {
                inner_class,
                outer_class,
                inner_name,
                access_flags: InnerClassAccessFlags::STATIC | InnerClassAccessFlags::PRIVATE,
            };
            (NestHost(outer_class), InnerClasses(vec![inner_class_attr]))
        };
        class.add_attribute(nest_host)?;
        class.add_attribute(inner_classes)?;

        Ok(CurrentPart {
            class,
            bootstrap: BootstrapUtilities::default(),
        })
    }

    /// Process one payload, return types on the final `End` payload
    pub fn process_payload(&mut self, payload: Payload<'a>) -> Result<Option<Types>, Error> {
        log::trace!("Payload {:?}", payload);

        // TODO: find a better place to trigger generation of this code
        if !self.fields_generated
            && matches!(&payload, Payload::CodeSectionStart { .. } | Payload::End(_))
        {
            self.generate_table_fields()?;
            self.generate_memory_fields()?;
            self.generate_global_fields()?;
            self.fields_generated = true
        }

        match payload {
            Payload::Version {
                num,
                encoding,
                range,
            } => self.validator.version(num, encoding, &range)?,
            Payload::TypeSection(section) => self.validator.type_section(&section)?,
            Payload::ImportSection(section) => self.visit_imports(section)?,
            Payload::AliasSection(section) => self.validator.alias_section(&section)?,
            Payload::InstanceSection(section) => self.validator.instance_section(&section)?,
            Payload::TableSection(section) => self.visit_tables(section)?,
            Payload::MemorySection(section) => self.visit_memories(section)?,
            Payload::TagSection(section) => self.validator.tag_section(&section)?,
            Payload::GlobalSection(section) => self.visit_globals(section)?,
            Payload::ExportSection(section) => self.visit_exports(section)?,
            Payload::FunctionSection(section) => self.validator.function_section(&section)?,
            Payload::StartSection { func, range } => self.validator.start_section(func, &range)?,
            Payload::ElementSection(section) => self.visit_elements(section)?,
            Payload::DataCountSection { count, range } => {
                self.validator.data_count_section(count, &range)?
            }
            Payload::DataSection(section) => self.visit_datas(section)?,
            Payload::CustomSection { .. } => (),
            Payload::CodeSectionStart { count, range, .. } => {
                self.validator.code_section_start(count, &range)?
            }
            Payload::CodeSectionEntry(function_body) => self.visit_function_body(function_body)?,
            Payload::ModuleSection { range, .. } => self.validator.module_section(&range)?,
            Payload::UnknownSection { id, range, .. } => {
                self.validator.unknown_section(id, &range)?
            }
            Payload::ComponentTypeSection(section) => {
                self.validator.component_type_section(&section)?
            }
            Payload::ComponentImportSection(section) => {
                self.validator.component_import_section(&section)?
            }
            Payload::ComponentFunctionSection(section) => {
                self.validator.component_function_section(&section)?
            }
            Payload::ComponentSection { range, .. } => self.validator.component_section(&range)?,
            Payload::ComponentExportSection(section) => {
                self.validator.component_export_section(&section)?
            }
            Payload::ComponentStartSection(section) => {
                self.validator.component_start_section(&section)?
            }
            Payload::End(offset) => return Ok(Some(self.validator.end(offset)?)),
        }
        Ok(None)
    }

    /// Visit a function body
    fn visit_function_body(&mut self, function_body: FunctionBody) -> Result<(), Error> {
        let validator = self.validator.code_section_entry(&function_body)?;

        // Build up the type and argument
        let typ = validator
            .resources()
            .function_idx_type(self.current_func_idx)?;

        // Build up a method descriptor, which includes a trailing "WASM module" argument
        let mut method_descriptor = typ.method_descriptor();
        method_descriptor.parameters.push(FieldType::object(
            self.settings.output_full_class_name.clone(),
        ));

        let mut method_builder = self.current_part.class.start_method(
            MethodAccessFlags::STATIC,
            self.settings
                .wasm_function_name_prefix
                .concat(&UnqualifiedName::number(self.current_func_idx as usize)),
            method_descriptor,
        )?;

        let mut function_translator = FunctionTranslator::new(
            typ,
            &self.settings,
            &mut self.utilities,
            &mut self.current_part.bootstrap,
            &mut method_builder.code,
            &self.tables,
            &self.memories,
            &self.globals,
            function_body,
            validator,
        )?;
        function_translator.translate()?;

        self.current_part.class.finish_method(method_builder)?;
        self.current_func_idx += 1;

        Ok(())
    }

    fn visit_globals(&mut self, globals: GlobalSectionReader<'a>) -> Result<(), Error> {
        self.validator.global_section(&globals)?;
        for global in globals {
            let wasmparser::Global { ty, init_expr } = global?;
            let origin = MemberOrigin {
                imported: None,
                exported: false,
            };
            let field_name = self
                .settings
                .wasm_global_name_prefix
                .concat(&UnqualifiedName::number(self.globals.len()));
            let global = Global {
                origin,
                field_name,
                global_type: StackType::from_general(ty.content_type)?,
                mutable: ty.mutable,
                initial: Some(init_expr),
            };
            self.globals.push(global);
        }
        Ok(())
    }

    /// Generate the fields associated with globals
    fn generate_global_fields(&mut self) -> Result<(), Error> {
        for global in &self.globals {
            if !global.origin.is_internal() {
                todo!("exported/imported global")
            }

            let mutable_flag = if global.mutable {
                FieldAccessFlags::empty()
            } else {
                FieldAccessFlags::FINAL
            };

            // TODO: this only works for Java 11+. For other Java versions, private fields from
            // outer classes are not visible - getters/setters must be generated (private functions
            // _are_ visible)
            self.class.add_field(
                FieldAccessFlags::PRIVATE | mutable_flag,
                global.field_name.clone(),
                global.global_type.field_type().render(),
            )?;
        }

        Ok(())
    }

    /// Visit the imports
    fn visit_imports(&mut self, imports: ImportSectionReader<'a>) -> Result<(), Error> {
        self.validator.import_section(&imports)?;
        for import in imports {
            self.visit_import(import?)?;
        }
        Ok(())
    }

    fn visit_import(&mut self, import: Import<'a>) -> Result<(), Error> {
        let origin = MemberOrigin {
            imported: Some(Some(import.module.to_owned())),
            exported: false,
        };

        let name = UnqualifiedName::from_string(import.name.to_owned()).unwrap();

        match import.ty {
            TypeRef::Table(table_type) => self.tables.push(Table {
                origin,
                field_name: name,
                table_type: TableType::from_general(table_type.element_type)?,
                initial: table_type.initial,
                maximum: table_type.maximum,
            }),

            TypeRef::Global(global_type) => {
                self.globals.push(Global {
                    origin,
                    field_name: name,
                    global_type: StackType::from_general(global_type.content_type)?,
                    mutable: global_type.mutable,
                    initial: None,
                });
            }

            TypeRef::Func(_) => todo!("import function"),
            TypeRef::Memory(_) => todo!("import memory"),
            TypeRef::Tag(_) => todo!("import tag"),
        }

        Ok(())
    }

    /// Visit the tables section
    fn visit_tables(&mut self, tables: TableSectionReader<'a>) -> Result<(), Error> {
        self.validator.table_section(&tables)?;
        for table in tables {
            let table = table?;
            let origin = MemberOrigin {
                imported: None,
                exported: false,
            };
            let field_name = self
                .settings
                .wasm_table_name_prefix
                .concat(&UnqualifiedName::number(self.tables.len()));
            self.tables.push(Table {
                origin,
                field_name,
                table_type: TableType::from_general(table.element_type)?,
                initial: table.initial,
                maximum: table.maximum,
            });
        }
        Ok(())
    }

    /// Generate the fields associated with tables
    fn generate_table_fields(&mut self) -> Result<(), Error> {
        for table in &self.tables {
            if !table.origin.is_internal() {
                todo!("exported/imported table")
            }

            // TODO: if the limits on the table constrain it to never grow, make the field final
            self.class.add_field(
                FieldAccessFlags::PRIVATE,
                table.field_name.clone(),
                RefType::array(table.table_type.field_type()).render(),
            )?;
        }

        Ok(())
    }

    /// Visit the memories section
    fn visit_memories(&mut self, memories: MemorySectionReader<'a>) -> Result<(), Error> {
        self.validator.memory_section(&memories)?;
        for memory in memories {
            let memory = memory?;
            let origin = MemberOrigin {
                imported: None,
                exported: false,
            };
            let field_name = self
                .settings
                .wasm_memory_name_prefix
                .concat(&UnqualifiedName::number(self.memories.len()));
            self.memories.push(Memory {
                origin,
                field_name,
                memory_type: memory,
            });
        }

        Ok(())
    }

    /// Generate the fields associated with memories
    fn generate_memory_fields(&mut self) -> Result<(), Error> {
        for memory in &self.memories {
            if !memory.origin.is_internal() {
                todo!("exported/imported memories")
            } else if memory.memory_type.shared {
                todo!("shared memory")
            } else if memory.memory_type.memory64 {
                todo!("64-bit memory")
            } else {
                // TODO: if the limits on the memory constrain it to never grow, make the field final
                self.class.add_field(
                    FieldAccessFlags::PRIVATE,
                    memory.field_name.clone(),
                    RefType::Object(BinaryName::BYTEBUFFER).render(),
                )?;
            }
        }

        Ok(())
    }

    /// Visit the exports
    ///
    /// The actual processing of the exports is in `generate_exports`, since the module resources
    /// aren't ready at this point.
    fn visit_exports(&mut self, exports: ExportSectionReader<'a>) -> Result<(), Error> {
        self.validator.export_section(&exports)?;
        for export in exports {
            let export = export?;
            match export.kind {
                ExternalKind::Table => {
                    let table: &mut Table = self
                        .tables
                        .get_mut(export.index as usize)
                        .expect("Exporting function that doesn't exist");
                    let name: String = self.settings.renamer.rename_table(export.name);
                    table.field_name =
                        UnqualifiedName::from_string(name).map_err(Error::MalformedName)?;
                    table.origin.exported = true;
                }

                ExternalKind::Memory => {
                    let memory: &mut Memory = self
                        .memories
                        .get_mut(export.index as usize)
                        .expect("Exporting memory that ddoesn't exist");
                    let name: String = self.settings.renamer.rename_memory(export.name);
                    memory.field_name =
                        UnqualifiedName::from_string(name).map_err(Error::MalformedName)?;
                    memory.origin.exported = true;
                }

                ExternalKind::Global => {
                    let global: &mut Global = self
                        .globals
                        .get_mut(export.index as usize)
                        .expect("Exporting global that ddoesn't exist");
                    let name: String = self.settings.renamer.rename_global(export.name);
                    global.field_name =
                        UnqualifiedName::from_string(name).map_err(Error::MalformedName)?;
                    global.origin.exported = true;
                }

                _ => self.exports.push(export),
            }
        }
        Ok(())
    }

    /// Generate members in the outer class corresponding to exports
    fn generate_exports(&mut self, types: &Types) -> Result<(), Error> {
        for export in &self.exports {
            log::trace!("Export {:?}", export);
            match export.kind {
                ExternalKind::Func => {
                    // Exported function
                    let export_descriptor =
                        types.function_idx_type(export.index)?.method_descriptor();

                    // Implementation function
                    let mut underlying_descriptor = export_descriptor.clone();
                    underlying_descriptor.parameters.push(FieldType::object(
                        self.settings.output_full_class_name.clone(),
                    ));

                    let name: String = self.settings.renamer.rename_function(export.name);
                    let mut method_builder = self.class.start_method(
                        MethodAccessFlags::PUBLIC,
                        UnqualifiedName::from_string(name).map_err(Error::MalformedName)?,
                        export_descriptor.clone(),
                    )?;

                    // Push the method arguments onto the stack
                    let mut offset = 1;
                    for parameter in &export_descriptor.parameters {
                        method_builder.code.get_local(offset, parameter)?;
                        offset += parameter.width() as u16;
                    }
                    method_builder.code.get_local(
                        0,
                        &FieldType::object(self.settings.output_full_class_name.clone()),
                    )?;

                    // Call the implementation
                    method_builder.code.invoke_explicit(
                        InvokeType::Static,
                        self.current_part.class.class_name(),
                        &self
                            .settings
                            .wasm_function_name_prefix
                            .concat(&UnqualifiedName::number(export.index as usize)),
                        &underlying_descriptor,
                    )?;
                    method_builder.code.return_(export_descriptor.return_type)?;

                    self.class.finish_method(method_builder)?;
                }
                _ => todo!(),
            }
        }

        Ok(())
    }

    /// Visit the elements section
    fn visit_elements(&mut self, elements: ElementSectionReader<'a>) -> Result<(), Error> {
        self.validator.element_section(&elements)?;
        for element in elements.into_iter() {
            let element = element?;
            let items = element
                .items
                .get_items_reader()?
                .into_iter()
                .collect::<Result<Vec<_>, _>>()?;
            self.elements.push(Element {
                kind: element.kind,
                element_type: TableType::from_general(element.ty)?,
                items,
            });
        }
        Ok(())
    }

    /// Visit the datas section
    fn visit_datas(&mut self, datas: DataSectionReader<'a>) -> Result<(), Error> {
        self.validator.data_section(&datas)?;
        for data in datas.into_iter() {
            self.datas.push(data?);
        }
        Ok(())
    }

    /// Generate a constructor
    pub fn generate_constructor(&mut self, types: &Types) -> Result<(), Error> {
        let mut method_builder = self.class.start_method(
            MethodAccessFlags::PUBLIC,
            UnqualifiedName::INIT,
            MethodDescriptor {
                parameters: vec![],
                return_type: None,
            },
        )?;
        let jvm_code = &mut method_builder.code;

        jvm_code.push_instruction(Instruction::ALoad(0))?;
        jvm_code.invoke(&BinaryName::OBJECT, &UnqualifiedName::INIT)?;

        // Initial table arrays
        for table in &self.tables {
            if let None = table.origin.imported {
                if !table.origin.exported {
                    jvm_code.push_instruction(Instruction::ALoad(0))?;
                    jvm_code.const_int(table.initial as i32)?; // TODO: error if `u32` is too big
                    jvm_code.new_ref_array(&table.table_type.ref_type())?;
                    jvm_code.access_field(
                        &self.settings.output_full_class_name,
                        &table.field_name,
                        AccessMode::Write,
                    )?;
                } else {
                    todo!()
                }
            }
        }

        // Initialize memory
        for memory in &self.memories {
            if memory.origin.is_internal() {
                if memory.memory_type.memory64 {
                    todo!("64-bit memory")
                } else {
                    let initial: u64 = memory.memory_type.initial * 65536;
                    jvm_code.push_instruction(Instruction::ALoad(0))?;
                    jvm_code.const_int(initial as i32)?; // TODO: error if too big
                    jvm_code.invoke(&BinaryName::BYTEBUFFER, &UnqualifiedName::ALLOCATE)?; // TODO: add option for allocate direct
                    jvm_code.access_field(
                        &BinaryName::BYTEORDER,
                        &UnqualifiedName::LITTLEENDIAN,
                        AccessMode::Read,
                    )?;
                    jvm_code.invoke(&BinaryName::BYTEBUFFER, &UnqualifiedName::ORDER)?;
                    jvm_code.access_field(
                        &self.settings.output_full_class_name,
                        &memory.field_name,
                        AccessMode::Write,
                    )?;
                }
            }
        }

        // Initialize globals
        for global in &self.globals {
            if let None = global.origin.imported {
                if !global.origin.exported {
                    if let Some(init_expr) = &global.initial {
                        jvm_code.push_instruction(Instruction::ALoad(0))?;
                        self.translate_init_expr(types, jvm_code, init_expr)?;
                        jvm_code.access_field(
                            &self.settings.output_full_class_name,
                            &global.field_name,
                            AccessMode::Write,
                        )?;
                    }
                } else {
                    todo!()
                }
            }
        }

        // Initialize active elements
        for element in &self.elements {
            if let ElementKind::Active {
                table_index,
                init_expr,
            } = element.kind
            {
                let table = &self.tables[table_index as usize];
                if !table.origin.exported {
                    // Load onto the stack the table array
                    jvm_code.push_instruction(Instruction::ALoad(0))?;
                    jvm_code.access_field(
                        &self.settings.output_full_class_name,
                        &table.field_name,
                        AccessMode::Read,
                    )?;

                    // Store the starting offset in a local variable
                    self.translate_init_expr(types, jvm_code, &init_expr)?;
                    jvm_code.push_instruction(Instruction::IStore(1))?;

                    for item in &element.items {
                        jvm_code.push_instruction(Instruction::Dup)?;
                        jvm_code.push_instruction(Instruction::ILoad(1))?;
                        match item {
                            ElementItem::Func(func_idx) => Self::translate_ref_func(
                                &self.settings,
                                types,
                                *func_idx,
                                jvm_code,
                            )?,
                            ElementItem::Expr(elem_expr) => {
                                self.translate_init_expr(types, jvm_code, &elem_expr)?
                            }
                        }
                        jvm_code.push_instruction(Instruction::AAStore)?;
                        jvm_code.push_instruction(Instruction::IInc(1, 1))?;
                    }

                    // Kill the local variable, drop the array
                    jvm_code.push_instruction(Instruction::Pop)?;
                    jvm_code.push_instruction(Instruction::IKill(1))?;
                } else {
                    todo!()
                }
            }
        }

        // Initialize active data
        for data in &self.datas {
            if let DataKind::Active {
                memory_index,
                init_expr,
            } = data.kind
            {
                let memory = &self.memories[memory_index as usize];
                if memory.origin.is_internal() {
                    // Load onto the stack the memory bytebuffer
                    jvm_code.push_instruction(Instruction::ALoad(0))?;
                    jvm_code.access_field(
                        &self.settings.output_full_class_name,
                        &memory.field_name,
                        AccessMode::Read,
                    )?;

                    // Set the starting offset for the buffer
                    jvm_code.push_instruction(Instruction::Dup)?;
                    self.translate_init_expr(types, jvm_code, &init_expr)?;
                    jvm_code.invoke(&BinaryName::BUFFER, &UnqualifiedName::POSITION)?;
                    jvm_code.push_instruction(Instruction::Pop)?;

                    for chunk in data.data.chunks(u16::MAX as usize) {
                        jvm_code
                            .const_string(chunk.iter().map(|&c| c as char).collect::<String>())?;
                        jvm_code.const_string("ISO-8859-1")?;
                        jvm_code.invoke(&BinaryName::STRING, &UnqualifiedName::GETBYTES)?;
                        jvm_code.invoke_explicit(
                            InvokeType::Virtual,
                            &BinaryName::BYTEBUFFER,
                            &UnqualifiedName::PUT,
                            &MethodDescriptor {
                                parameters: vec![FieldType::array(FieldType::BYTE)],
                                return_type: Some(FieldType::object(BinaryName::BYTEBUFFER)),
                            },
                        )?;
                    }

                    // Kill the local variable, drop the bytebuffer
                    jvm_code.push_instruction(Instruction::Pop)?;
                } else {
                    todo!("Initialize non-internal memory")
                }
            }
        }

        jvm_code.push_branch_instruction(BranchInstruction::Return)?;
        self.class.finish_method(method_builder)?;

        Ok(())
    }

    /// Translate a constant expression
    fn translate_init_expr<B: CodeBuilderExts>(
        &self,
        types: &Types,
        jvm_code: &mut B,
        init_expr: &InitExpr,
    ) -> Result<(), Error> {
        for operator in init_expr.get_operators_reader().into_iter() {
            match operator? {
                Operator::I32Const { value } => jvm_code.const_int(value)?,
                Operator::I64Const { value } => jvm_code.const_long(value)?,
                Operator::F32Const { value } => {
                    jvm_code.const_float(f32::from_bits(value.bits()))?
                }
                Operator::F64Const { value } => {
                    jvm_code.const_double(f64::from_bits(value.bits()))?
                }
                Operator::RefNull { ty } => {
                    let ref_type = ref_type_from_general(ty)?;
                    jvm_code.const_null(ref_type)?;
                }
                Operator::RefFunc { function_index } => {
                    Self::translate_ref_func(&self.settings, types, function_index, jvm_code)?
                }
                Operator::End => (),
                other => todo!(
                    "figure out which other expressions and valid, then rule out the rest {:?}",
                    other
                ),
            }
        }

        Ok(())
    }

    /// Load a method handle for the given function onto the top of the stack
    ///
    /// Note: the method handle will have an "adapted" signature, meaning there is always one final
    /// argument that is the module itself.
    fn translate_ref_func<B: CodeBuilderExts>(
        settings: &Settings,
        types: &Types,
        function_index: u32,
        jvm_code: &mut B,
    ) -> Result<(), Error> {
        let class_name = format!(
            "{}${}0",
            settings.output_full_class_name.as_str(),
            settings.part_short_class_name.as_str(),
        );
        let method_name = format!(
            "{}{}",
            settings.wasm_function_name_prefix.as_str(),
            function_index,
        );
        let mut method_type = types.function_idx_type(function_index)?.method_descriptor();
        method_type
            .parameters
            .push(FieldType::object(settings.output_full_class_name.clone()));
        let method_handle: ConstantIndex = {
            let mut constants = jvm_code.constants();
            let class_name_idx = constants.get_utf8(class_name)?;
            let class_idx = constants.get_class(class_name_idx)?;
            let name_idx = constants.get_utf8(method_name)?;
            let type_idx = constants.get_utf8(method_type.render())?;
            let name_and_type_idx = constants.get_name_and_type(name_idx, type_idx)?;
            let method_ref = constants
                .get_method_ref(class_idx, name_and_type_idx, false)?
                .into();
            constants
                .get_method_handle(HandleKind::InvokeStatic, method_ref)?
                .into()
        };
        jvm_code.push_instruction(Instruction::Ldc(method_handle))?;

        Ok(())
    }

    /// Emit the final classes
    ///
    /// The first element in the output vector is the output class. The rest of the elements are
    /// the "part" inner classes.
    pub fn result(mut self, types: &Types) -> Result<Vec<(BinaryName, ClassFile)>, Error> {
        self.generate_exports(types)?;
        self.generate_constructor(types)?;

        // Assemble all the parts
        let mut parts = self.previous_parts;
        parts.push(self.current_part.result()?);

        // Construct the `NestMembers` and `InnerClasses` attribute
        let (nest_members, inner_classes): (NestMembers, InnerClasses) = {
            let mut nest_members = vec![];
            let mut inner_class_attrs = vec![];
            let mut constants = self.class.constants();
            let outer_class_name =
                constants.get_utf8(self.settings.output_full_class_name.as_str())?;
            let outer_class = constants.get_class(outer_class_name)?;

            // Utilities inner class
            if let UtilitiesStrategy::GenerateNested { inner_class, .. } =
                &self.settings.utilities_strategy
            {
                let utilities_class_name =
                    constants.get_utf8(self.utilities.class_name().as_str())?;
                let utilities_class = constants.get_class(utilities_class_name)?;
                let utilities_name = constants.get_utf8(inner_class.as_str())?;
                nest_members.push(utilities_class);
                inner_class_attrs.push(InnerClass {
                    inner_class: utilities_class,
                    outer_class,
                    inner_name: utilities_name,
                    access_flags: InnerClassAccessFlags::STATIC | InnerClassAccessFlags::PRIVATE,
                });
            }

            // Part inner classes
            for (part_idx, part) in parts.iter().enumerate() {
                let inner_class_name = constants.get_utf8(part.class_name().as_str())?;
                let inner_class = constants.get_class(inner_class_name)?;
                let inner_name = constants.get_utf8(&format!(
                    "{}{}",
                    self.settings.part_short_class_name.as_str(),
                    part_idx
                ))?;
                nest_members.push(inner_class);
                inner_class_attrs.push(InnerClass {
                    inner_class,
                    outer_class,
                    inner_name,
                    access_flags: InnerClassAccessFlags::STATIC | InnerClassAccessFlags::PRIVATE,
                });
            }
            (NestMembers(nest_members), InnerClasses(inner_class_attrs))
        };
        self.class.add_attribute(nest_members)?;
        self.class.add_attribute(inner_classes)?;

        // Final results
        let results: Vec<(BinaryName, ClassFile)> = iter::once(self.class)
            .chain(self.utilities.into_builder().into_iter())
            .chain(parts.into_iter())
            .map(|builder| (builder.class_name().to_owned(), builder.result()))
            .collect();

        Ok(results)
    }
}
