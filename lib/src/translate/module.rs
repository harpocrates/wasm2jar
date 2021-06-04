use super::{
    AccessMode, CodeBuilderExts, Element, Error, FunctionTranslator, Global, MemberOrigin,
    Settings, Table, UtilityClass,
};
use crate::jvm::{
    BranchInstruction, ClassAccessFlags, ClassBuilder, ClassFile, ClassGraph, CodeBuilder,
    ConstantIndex, Descriptor, FieldAccessFlags, FieldType, HandleKind, InnerClass,
    InnerClassAccessFlags, InnerClasses, Instruction, InvokeType, MethodAccessFlags,
    MethodDescriptor, NestHost, NestMembers, RefType, Width, BinaryName, UnqualifiedName
};
use crate::wasm::{ref_type_from_general, StackType, TableType, WasmModuleResourcesExt};
use std::cell::RefCell;
use std::rc::Rc;
use std::convert::TryFrom;
use wasmparser::{
    ElementItem, ElementKind, ElementSectionReader, Export, ExportSectionReader, ExternalKind,
    FunctionBody, GlobalSectionReader, Import, ImportSectionReader, InitExpr, Operator, Parser,
    Payload, TableSectionReader, Validator,
};

pub struct ModuleTranslator<'wasm, 'jvm> {
    settings: Settings<'jvm>,
    validator: Validator,
    #[allow(dead_code)]
    class_graph: Rc<RefCell<ClassGraph<'jvm>>>,
    class: ClassBuilder<'jvm>,
    previous_parts: Vec<ClassBuilder<'jvm>>,
    current_part: ClassBuilder<'jvm>,

    /// Utility class (just a carrier for whatever helper methods we may want)
    utilities: UtilityClass<'jvm>,

    /// Populated when we visit exports
    exports: Vec<Export<'wasm>>,

    /// Populated when we visit tables
    tables: Vec<Table<'jvm>>,

    /// Populated when we visit globals
    globals: Vec<Global<'wasm, 'jvm>>,

    /// Populated when we visit elements
    elements: Vec<Element<'wasm>>,

    /// Every time we see a new function, this gets incremented
    current_func_idx: u32,
}

impl<'wasm, 'jvm> ModuleTranslator<'wasm, 'jvm> {
    pub fn new(settings: Settings<'jvm>) -> Result<ModuleTranslator<'wasm, 'jvm>, Error> {
        let mut validator = Validator::new();
        validator.wasm_features(settings.wasm_features);

        let mut class_graph = ClassGraph::new();
        class_graph.insert_lang_types();
        class_graph.insert_error_types();
        class_graph.insert_util_types();
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
            current_part,
            utilities,
            exports: vec![],
            tables: vec![],
            globals: vec![],
            elements: vec![],
            current_func_idx: 0,
        })
    }

    /// Parse a full module
    pub fn parse_module(&mut self, data: &'wasm [u8]) -> Result<(), Error> {
        let parser = Parser::new(0);
        for payload in parser.parse_all(data) {
            let payload = payload?;
            self.process_payload(payload)?;
        }
        Ok(())
    }

    /// Construct a new inner class part
    fn new_part<'x>(
        settings: &'x Settings<'x>,
        class_graph: Rc<RefCell<ClassGraph<'x>>>,
        part_idx: usize,
    ) -> Result<ClassBuilder<'x>, Error> {
        let mut part = ClassBuilder::new(
            ClassAccessFlags::SYNTHETIC | ClassAccessFlags::SUPER,
            BinaryName::try_from(format!(
                "{}${}{}",
                settings.output_full_class_name, settings.part_short_class_name, part_idx
            ).as_str()).unwrap(),
            BinaryName::OBJECT,
            false,
            vec![],
            class_graph.clone(),
        )?;

        // Add the `NestHost` and `InnerClasses` attributes
        let (nest_host, inner_classes): (NestHost, InnerClasses) = {
            let mut constants = part.constants();
            let outer_class_name = constants.get_utf8(settings.output_full_class_name.as_ref())?;
            let outer_class = constants.get_class(outer_class_name)?;
            let inner_class_name = constants.get_utf8(part.class_name().as_ref())?;
            let inner_class = constants.get_class(inner_class_name)?;
            let inner_name =
                constants.get_utf8(&format!("{}{}", settings.part_short_class_name, part_idx))?;
            let inner_class_attr = InnerClass {
                inner_class,
                outer_class,
                inner_name,
                access_flags: InnerClassAccessFlags::STATIC | InnerClassAccessFlags::PRIVATE,
            };
            (NestHost(outer_class), InnerClasses(vec![inner_class_attr]))
        };
        part.add_attribute(nest_host)?;
        part.add_attribute(inner_classes)?;

        Ok(part)
    }

    /// Process one payload
    pub fn process_payload(&mut self, payload: Payload<'wasm>) -> Result<(), Error> {
        log::trace!("Payload {:?}", payload);
        match payload {
            Payload::Version { num, range } => self.validator.version(num, &range)?,
            Payload::TypeSection(section) => self.validator.type_section(&section)?,
            Payload::ImportSection(section) => self.visit_imports(section)?,
            Payload::AliasSection(section) => self.validator.alias_section(&section)?,
            Payload::InstanceSection(section) => self.validator.instance_section(&section)?,
            Payload::TableSection(section) => self.visit_tables(section)?,
            Payload::MemorySection(section) => self.validator.memory_section(&section)?,
            Payload::EventSection(section) => self.validator.event_section(&section)?,
            Payload::GlobalSection(section) => self.visit_globals(section)?,
            Payload::ExportSection(section) => self.visit_exports(section)?,
            Payload::FunctionSection(section) => self.validator.function_section(&section)?,
            Payload::StartSection { func, range } => self.validator.start_section(func, &range)?,
            Payload::ElementSection(section) => self.visit_elements(section)?,
            Payload::DataCountSection { count, range } => {
                self.validator.data_count_section(count, &range)?
            }
            Payload::DataSection(section) => self.validator.data_section(&section)?,
            Payload::CustomSection { .. } => (),
            Payload::CodeSectionStart { count, range, .. } => {
                // TODO: generating table fields here is not quite correct since it means that a
                // module without code won't get tables generated
                self.generate_table_fields()?;
                self.generate_global_fields()?;
                self.validator.code_section_start(count, &range)?
            }
            Payload::CodeSectionEntry(function_body) => self.visit_function_body(function_body)?,
            Payload::ModuleSectionStart { count, range, .. } => {
                self.validator.module_section_start(count, &range)?
            }
            Payload::ModuleSectionEntry { .. } => self.validator.module_section_entry(),
            Payload::UnknownSection { id, range, .. } => {
                self.validator.unknown_section(id, &range)?
            }
            Payload::End => self.validator.end()?,
        }
        Ok(())
    }

    /// Visit a function body
    fn visit_function_body(&mut self, function_body: FunctionBody<'wasm>) -> Result<(), Error> {
        let validator = self.validator.code_section_entry()?;

        // Build up the type and argument
        let typ = validator
            .resources()
            .function_idx_type(self.current_func_idx)?;

        // Build up a method descriptor, which includes a trailing "WASM module" argument
        let mut method_descriptor = typ.method_descriptor();
        method_descriptor.parameters.push(FieldType::Ref(RefType::Object(
            self.settings.output_full_class_name
        )));

        let mut method_builder = self.current_part.start_method(
            MethodAccessFlags::STATIC,
            UnqualifiedName::try_from(format!(
                "{}{}",
                self.settings.wasm_function_name_prefix, self.current_func_idx
            ).as_str()).unwrap(),
            method_descriptor,
        )?;

        let mut function_translator = FunctionTranslator::new(
            typ,
            &self.settings,
            &mut self.utilities,
            &mut method_builder.code,
            &self.tables,
            &self.globals,
            function_body,
            validator,
        )?;
        function_translator.translate()?;

        self.current_part.finish_method(method_builder)?;
        self.current_func_idx += 1;

        Ok(())
    }

    fn visit_globals(&mut self, globals: GlobalSectionReader<'wasm>) -> Result<(), Error> {
        self.validator.global_section(&globals)?;
        for global in globals {
            let wasmparser::Global { ty, init_expr } = global?;
            let origin = MemberOrigin {
                imported: None,
                exported: false,
            };
            let field_name = format!("global_{}", self.globals.len()).as_ref();
            let global = Global {
                origin,
                field_name: UnqualifiedName::try_from(field_name).unwrap(),
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
                todo!()
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
    fn visit_imports(&mut self, imports: ImportSectionReader<'wasm>) -> Result<(), Error> {
        self.validator.import_section(&imports)?;
        for import in imports {
            self.visit_import(import?)?;
        }
        Ok(())
    }

    fn visit_import(&mut self, import: Import<'wasm>) -> Result<(), Error> {
        use wasmparser::ImportSectionEntryType;

        let origin = MemberOrigin {
            imported: Some(Some(import.module.to_owned())),
            exported: false,
        };

        // TODO: this is not the name we want
        let name = match import.field {
            None => unimplemented!(),
            Some(name) => name,
        };

        match import.ty {
            ImportSectionEntryType::Table(table_type) => {
                let name = self.settings.renamer.rename_table(name).as_ref();
                self.tables.push(Table {
                    origin,
                    field_name: UnqualifiedName::try_from(name).map_err(Error::InvalidName)?,
                    table_type: TableType::from_general(table_type.element_type)?,
                    limits: table_type.limits,
                });
            }

            ImportSectionEntryType::Global(global_type) => {
                let name = self.settings.renamer.rename_global(name).as_ref();
                self.globals.push(Global {
                    origin,
                    field_name: UnqualifiedName::try_from(name).map_err(Error::InvalidName)?,
                    global_type: StackType::from_general(global_type.content_type)?,
                    mutable: global_type.mutable,
                    initial: None,
                });
            }

            _ => todo!(),
        }

        Ok(())
    }

    /// Visit the tables section
    fn visit_tables(&mut self, tables: TableSectionReader<'wasm>) -> Result<(), Error> {
        self.validator.table_section(&tables)?;
        for table in tables {
            let table = table?;
            let origin = MemberOrigin {
                imported: None,
                exported: false,
            };
            self.tables.push(Table {
                origin,
                field_name: UnqualifiedName::try_from(format!("table_{}", self.tables.len()).as_str()).unwrap(),
                table_type: TableType::from_general(table.element_type)?,
                limits: table.limits,
            });
        }
        Ok(())
    }

    /// Generate the fields associated with tables
    fn generate_table_fields(&mut self) -> Result<(), Error> {
        for table in &self.tables {
            if !table.origin.is_internal() {
                todo!()
            }

            // TODO: if the limits on the table constrain it to never grow, make the field final
            self.class.add_field(
                FieldAccessFlags::PRIVATE,
                table.field_name.clone(),
                RefType::Array(&table.table_type.field_type()).render(),
            )?;
        }

        Ok(())
    }

    /// Visit the exports
    ///
    /// The actual processing of the exports is in `generate_exports`, since the module resources
    /// aren't ready at this point.
    fn visit_exports(&mut self, exports: ExportSectionReader<'wasm>) -> Result<(), Error> {
        self.validator.export_section(&exports)?;
        for export in exports {
            let export = export?;
            match export.kind {
                ExternalKind::Table => {
                    let table: &mut Table = self
                        .tables
                        .get_mut(export.index as usize)
                        .expect("Exporting function that doesn't exist");
                    let name = self.settings.renamer.rename_table(export.field).as_ref();
                    table.field_name = UnqualifiedName::try_from(name).map_err(Error::InvalidName)?;
                    table.origin.exported = true;
                }

                ExternalKind::Global => {
                    let global: &mut Global = self
                        .globals
                        .get_mut(export.index as usize)
                        .expect("Exporting global that ddoesn't exist");
                    let name = self.settings.renamer.rename_global(export.field).as_ref();
                    global.field_name = UnqualifiedName::try_from(name).map_err(Error::InvalidName)?;
                    global.origin.exported = true;
                }

                _ => self.exports.push(export),
            }
        }
        Ok(())
    }

    /// Generate members in the outer class corresponding to exports
    fn generate_exports(&mut self) -> Result<(), Error> {
        for export in &self.exports {
            log::trace!("Export {:?}", export);
            match export.kind {
                ExternalKind::Function => {
                    // Exported function
                    let export_descriptor = self
                        .validator
                        .function_idx_type(export.index)?
                        .method_descriptor();

                    // Implementation function
                    let mut underlying_descriptor = export_descriptor.clone();
                    underlying_descriptor.parameters.push(FieldType::object(
                        self.settings.output_full_class_name.clone(),
                    ));

                    let name = self.settings.renamer.rename_function(export.field).as_ref();

                    let mut method_builder = self.class.start_method(
                        MethodAccessFlags::PUBLIC,
                        UnqualifiedName::try_from(name).map_err(Error::InvalidName)?,
                        export_descriptor.clone(),
                    )?;

                    // Push the method arguments onto the stack
                    let mut offset = 1;
                    for parameter in &export_descriptor.parameters {
                        method_builder.code.get_local(offset, *parameter)?;
                        offset += parameter.width() as u16;
                    }
                    method_builder.code.get_local(0, self.settings.module_field_type())?;

                    // Call the implementation
                    method_builder.code.invoke_explicit(
                        InvokeType::Static,
                        &self.current_part.class_name(),
                        &UnqualifiedName::try_from(format!(
                            "{}{}",
                            self.settings.wasm_function_name_prefix.as_ref(), export.index
                        ).as_str()).unwrap(),
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
    fn visit_elements(&mut self, elements: ElementSectionReader<'wasm>) -> Result<(), Error> {
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

    /// Generate a constructor
    pub fn generate_constructor(&mut self) -> Result<(), Error> {
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
                    jvm_code.const_int(table.limits.initial as i32)?; // TODO: error if `u32` is too big
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

        // Initialize globals
        for global in &self.globals {
            if let None = global.origin.imported {
                if !global.origin.exported {
                    if let Some(init_expr) = &global.initial {
                        jvm_code.push_instruction(Instruction::ALoad(0))?;
                        self.translate_init_expr(jvm_code, init_expr)?;
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
                    self.translate_init_expr(jvm_code, &init_expr)?;
                    jvm_code.push_instruction(Instruction::IStore(1))?;

                    for item in &element.items {
                        jvm_code.push_instruction(Instruction::Dup)?;
                        jvm_code.push_instruction(Instruction::ILoad(1))?;
                        match item {
                            ElementItem::Null(_) => {
                                jvm_code.push_instruction(Instruction::AConstNull)?
                            }
                            ElementItem::Func(func_idx) => Self::translate_ref_func(
                                &self.settings,
                                &self.validator,
                                *func_idx,
                                jvm_code,
                            )?,
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

        jvm_code.push_branch_instruction(BranchInstruction::Return)?;
        self.class.finish_method(method_builder)?;

        Ok(())
    }

    /// Translate a constant expression
    fn translate_init_expr<B: CodeBuilderExts<'jvm>>(
        &self,
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
                Operator::RefFunc { function_index } => Self::translate_ref_func(
                    &self.settings,
                    &self.validator,
                    function_index,
                    jvm_code,
                )?,
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
    fn translate_ref_func<'x, B: CodeBuilderExts<'x>>(
        settings: &Settings<'x>,
        validator: &Validator,
        function_index: u32,
        jvm_code: &mut B,
    ) -> Result<(), Error> {
        let class_name = format!(
            "{}${}0",
            settings.output_full_class_name, settings.part_short_class_name,
        );
        let method_name = format!("{}{}", settings.wasm_function_name_prefix, function_index,);
        let mut method_type = validator
            .function_idx_type(function_index)?
            .method_descriptor();
        method_type
            .parameters
            .push(settings.module_field_type());
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
    pub fn result(mut self) -> Result<Vec<(BinaryName<'jvm>, ClassFile)>, Error> {
        self.generate_exports()?;
        self.generate_constructor()?;

        // Assemble all the parts
        let mut parts = self.previous_parts;
        parts.push(self.current_part);

        // Construct the `NestMembers` and `InnerClasses` attribute
        let (nest_members, inner_classes): (NestMembers, InnerClasses) = {
            let mut nest_members = vec![];
            let mut inner_class_attrs = vec![];
            let mut constants = self.class.constants();
            let outer_class_name = constants.get_utf8(self.settings.output_full_class_name.as_ref())?;
            let outer_class = constants.get_class(outer_class_name)?;

            // Utilities inner class
            let utilities_class_name = constants.get_utf8(self.utilities.class.class_name().as_ref())?;
            let utilities_class = constants.get_class(utilities_class_name)?;
            let utilities_name = constants.get_utf8(self.settings.utilities_short_class_name.as_ref())?;
            nest_members.push(utilities_class);
            inner_class_attrs.push(InnerClass {
                inner_class: utilities_class,
                outer_class,
                inner_name: utilities_name,
                access_flags: InnerClassAccessFlags::STATIC | InnerClassAccessFlags::PRIVATE,
            });

            // Part inner classes
            for (part_idx, part) in parts.iter().enumerate() {
                let inner_class_name = constants.get_utf8(part.class_name().as_ref())?;
                let inner_class = constants.get_class(inner_class_name)?;
                let inner_name = constants.get_utf8(&format!(
                    "{}{}",
                    self.settings.part_short_class_name, part_idx
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
        let mut results = vec![
            (*self.class.class_name(), self.class.result()),
            (
                *self.utilities.class.class_name(),
                self.utilities.class.result(),
            ),
        ];
        results.extend(
            parts
                .into_iter()
                .map(|part| (*part.class_name(), part.result())),
        );

        Ok(results)
    }
}
