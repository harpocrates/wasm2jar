use super::{
    AccessMode, BootstrapUtilities, CodeBuilderExts, Element, Error, Function, FunctionTranslator,
    Global, MemberOrigin, Memory, Settings, Table, UtilitiesStrategy, UtilityClass, WasmImport,
};
use crate::jvm;
use crate::jvm::{
    BinaryName, BranchInstruction, BytecodeBuilder, ClassAccessFlags, ClassBuilder, ClassFile,
    ClassGraph, ConstantData, FieldAccessFlags, FieldType, InnerClass, InnerClassAccessFlags,
    InnerClasses, Instruction, JavaLibrary, MethodAccessFlags, MethodData, MethodDescriptor, Name,
    NestHost, NestMembers, RefType, UnqualifiedName, Width,
};
use crate::wasm::{
    ref_type_from_general, FunctionType, StackType, TableType, WasmModuleResourcesExt,
};
use std::iter;
use wasmparser::types::Types;
use wasmparser::{
    Data, DataKind, DataSectionReader, ElementItem, ElementKind, ElementSectionReader, Export,
    ExportSectionReader, ExternalKind, FunctionBody, FunctionSectionReader, GlobalSectionReader,
    Import, ImportSectionReader, InitExpr, MemorySectionReader, Operator, Parser, Payload,
    TableSectionReader, TypeDef, TypeRef, TypeSectionReader, Validator,
};

/// Main entry point for translating a WASM module
pub struct ModuleTranslator<'a, 'g> {
    settings: Settings,
    validator: Validator,
    class: ClassBuilder<'g>,
    previous_parts: Vec<ClassBuilder<'g>>,
    fields_generated: bool,
    class_graph: &'g ClassGraph<'g>,

    current_part: CurrentPart<'g>,

    /// Utility class (just a carrier for whatever helper methods we may want)
    utilities: UtilityClass<'g>,

    /// Populated when we visit exports
    exports: Vec<Export<'a>>,

    /// Populated when we visit imports
    imports: Vec<WasmImport<'a, 'g>>,

    /// Populated as soon as we visit the type section
    types: Vec<FunctionType>,

    /// Populated when we visit functions
    functions: Vec<Function<'g>>,

    /// Populated when we visit tables
    tables: Vec<Table<'g>>,

    /// Populated when we visit memories
    memories: Vec<Memory<'g>>,

    /// Populated when we visit globals
    globals: Vec<Global<'a, 'g>>,

    /// Populated when we visit elements
    elements: Vec<Element<'a>>,

    /// Populated when we visit datas
    datas: Vec<Data<'a>>,

    /// Every time we see a new function, this gets incremented
    current_func_idx: u32,
}

struct CurrentPart<'g> {
    class: ClassBuilder<'g>,
    bootstrap: BootstrapUtilities<'g>,
}
impl<'g> CurrentPart<'g> {
    fn result(self) -> Result<ClassBuilder<'g>, Error> {
        Ok(self.class)
    }
}

impl<'a, 'g> ModuleTranslator<'a, 'g> {
    pub fn new(
        settings: Settings,
        class_graph: &'g ClassGraph<'g>,
        java: &'g JavaLibrary<'g>,
    ) -> Result<Self, Error> {
        let validator = Validator::new_with_features(settings.wasm_features);

        let class = ClassBuilder::new(
            ClassAccessFlags::PUBLIC | ClassAccessFlags::SUPER,
            settings.output_full_class_name.clone(),
            java.classes.lang.object,
            false,
            vec![],
            class_graph,
            java,
        )?;
        let current_part = Self::new_part(&settings, class_graph, java, 0)?;
        let utilities = UtilityClass::new(&settings, class_graph, java)?;

        Ok(ModuleTranslator {
            settings,
            validator,
            class,
            previous_parts: vec![],
            fields_generated: false,
            class_graph,
            current_part,
            utilities,
            types: vec![],
            exports: vec![],
            imports: vec![],
            functions: vec![],
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
        class_graph: &'g ClassGraph<'g>,
        java: &'g JavaLibrary<'g>,
        part_idx: usize,
    ) -> Result<CurrentPart<'g>, Error> {
        let name = settings
            .part_short_class_name
            .concat(&UnqualifiedName::number(part_idx));
        let class = ClassBuilder::new(
            ClassAccessFlags::SYNTHETIC | ClassAccessFlags::SUPER,
            settings
                .output_full_class_name
                .concat(&UnqualifiedName::DOLLAR)
                .concat(&name),
            java.classes.lang.object,
            false,
            vec![],
            class_graph,
            java,
        )?;

        // Add the `NestHost` and `InnerClasses` attributes
        let (nest_host, inner_classes): (NestHost, InnerClasses) = {
            let constants = &class.constants_pool;
            let outer_class_name = constants.get_utf8(settings.output_full_class_name.as_str())?;
            let outer_class = constants.get_class(outer_class_name)?;
            let inner_class_name = constants.get_utf8(class.class.name.as_str())?;
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
            bootstrap: BootstrapUtilities::new(),
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
            Payload::TypeSection(section) => self.visit_types(section)?,
            Payload::ImportSection(section) => self.visit_imports(section)?,
            Payload::AliasSection(section) => self.validator.alias_section(&section)?,
            Payload::InstanceSection(section) => self.validator.instance_section(&section)?,
            Payload::TableSection(section) => self.visit_tables(section)?,
            Payload::MemorySection(section) => self.visit_memories(section)?,
            Payload::TagSection(section) => self.validator.tag_section(&section)?,
            Payload::GlobalSection(section) => self.visit_globals(section)?,
            Payload::ExportSection(section) => self.visit_exports(section)?,
            Payload::FunctionSection(section) => self.visit_function_declarations(section)?,
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

        // Look up the previously declared method and start implementing it
        let function = &self.functions[self.current_func_idx as usize];
        self.current_func_idx += 1;
        let mut method_builder = self
            .current_part
            .class
            .implement_method(MethodAccessFlags::STATIC, function.method)?;

        let mut function_translator = FunctionTranslator::new(
            &function.func_type,
            &self.settings,
            &mut self.utilities,
            &mut self.current_part.bootstrap,
            &mut method_builder.code,
            self.class.class,
            &self.functions,
            &self.tables,
            &self.memories,
            &self.globals,
            function_body,
            validator,
        )?;
        function_translator.translate()?;

        method_builder.finish()?;

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
                field: None,
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
        for global in &mut self.globals {
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
            let field = self.class.add_field(
                FieldAccessFlags::PRIVATE | mutable_flag,
                global.field_name.clone(),
                global.global_type.field_type(&self.class.java.classes),
            )?;
            global.field = Some(field);
        }

        Ok(())
    }

    /// Visit type section
    ///
    /// Ideally, we wouldn't need to manually track this since `wasmparser` already needs to track
    /// this information. However, we only get access in a function validator, which is too late.
    fn visit_types(&mut self, types: TypeSectionReader<'a>) -> Result<(), Error> {
        self.validator.type_section(&types)?;
        for ty in types {
            match ty? {
                TypeDef::Func(func_type) => {
                    self.types.push(FunctionType::from_general(&func_type)?);
                }
            }
        }
        Ok(())
    }

    /// Visit function section
    fn visit_function_declarations(
        &mut self,
        functions: FunctionSectionReader<'a>,
    ) -> Result<(), Error> {
        self.validator.function_section(&functions)?;
        for (func_idx, func_type_idx) in functions.into_iter().enumerate() {
            // Offset by imported functions
            let func_idx = func_idx + self.current_func_idx as usize;

            // Build up a method descriptor, which includes a trailing "WASM module" argument
            let func_type = self.types[func_type_idx? as usize].clone();
            let mut descriptor = func_type.method_descriptor(&self.class.java.classes);
            descriptor
                .parameters
                .push(FieldType::object(self.class.class));

            let method = self.class_graph.add_method(MethodData {
                class: self.current_part.class.class, // TODO: choose the right part here
                name: self.settings.wasm_function_name(func_idx),
                descriptor,
                is_static: true,
            });

            self.functions.push(Function { func_type, method });
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
                field: None,
                table_type: TableType::from_general(table_type.element_type)?,
                initial: table_type.initial,
                maximum: table_type.maximum,
            }),

            TypeRef::Global(global_type) => {
                self.globals.push(Global {
                    origin,
                    field_name: name,
                    field: None,
                    global_type: StackType::from_general(global_type.content_type)?,
                    mutable: global_type.mutable,
                    initial: None,
                });
            }

            TypeRef::Func(func_type_idx) => {
                let java = &self.class.java;

                // This is the index which which the imported function is called
                let func_idx = self.current_func_idx;
                self.current_func_idx += 1;

                // This is the expected descriptor of the imported function
                let func_type = self.types[func_type_idx as usize].clone();
                let imported_descriptor = func_type.method_descriptor(&java.classes);

                // Build up a method descriptor, which includes a trailing "WASM module" argument
                let mut descriptor = imported_descriptor.clone();
                descriptor
                    .parameters
                    .push(FieldType::object(self.class.class));

                // Field that will store the `MethodHandle` corresponding to the imported function
                let field = self.class.add_field(
                    FieldAccessFlags::PRIVATE | FieldAccessFlags::FINAL,
                    self.settings.wasm_import_name(func_idx as usize),
                    FieldType::object(java.classes.lang.invoke.method_handle),
                )?;
                self.imports.push(WasmImport::Function {
                    field,
                    module: import.module,
                    name: import.name,
                });

                // Trampoline method, whose sole responsibility is to invoke the method handle
                let mut method_builder = self.current_part.class.start_method(
                    MethodAccessFlags::STATIC,
                    self.settings.wasm_function_name(func_idx as usize),
                    descriptor.clone(),
                )?;
                self.functions.push(Function {
                    func_type,
                    method: method_builder.method,
                });
                let code = &mut method_builder.code;

                // `wasmModule.importedMethodHandle.invokeExact(arg0, ..., argn)`
                code.get_local(
                    imported_descriptor.parameter_length(false) as u16,
                    &FieldType::object(self.class.class),
                )?;
                code.access_field(field, AccessMode::Read)?;
                let mut offset = 0;
                for parameter in &imported_descriptor.parameters {
                    code.get_local(offset, parameter)?;
                    offset += parameter.width() as u16;
                }
                let return_type = imported_descriptor.return_type;
                code.invoke_invoke_exact(imported_descriptor)?;
                code.return_(return_type)?;

                method_builder.finish()?;
            }
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
                field: None,
                table_type: TableType::from_general(table.element_type)?,
                initial: table.initial,
                maximum: table.maximum,
            });
        }
        Ok(())
    }

    /// Generate the fields associated with tables
    fn generate_table_fields(&mut self) -> Result<(), Error> {
        for table in &mut self.tables {
            if !table.origin.is_internal() {
                todo!("exported/imported table")
            }

            // TODO: if the limits on the table constrain it to never grow, make the field final
            let field = self.class.add_field(
                FieldAccessFlags::PRIVATE,
                table.field_name.clone(),
                FieldType::array(table.table_type.field_type(&self.class.java.classes)),
            )?;
            table.field = Some(field);
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
                field: None,
                memory_type: memory,
            });
        }

        Ok(())
    }

    /// Generate the fields associated with memories
    fn generate_memory_fields(&mut self) -> Result<(), Error> {
        for memory in &mut self.memories {
            if !memory.origin.is_internal() {
                todo!("exported/imported memories")
            } else if memory.memory_type.shared {
                todo!("shared memory")
            } else if memory.memory_type.memory64 {
                todo!("64-bit memory")
            } else {
                // TODO: if the limits on the memory constrain it to never grow, make the field final
                let field = self.class.add_field(
                    FieldAccessFlags::PRIVATE,
                    memory.field_name.clone(),
                    FieldType::object(self.class.java.classes.nio.byte_buffer),
                )?;
                memory.field = Some(field);
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
                        .expect("Exporting table that doesn't exist");
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
                    let export_descriptor = types
                        .function_idx_type(export.index)?
                        .method_descriptor(&self.class.java.classes);

                    // Implementation function
                    let mut underlying_descriptor = export_descriptor.clone();
                    underlying_descriptor
                        .parameters
                        .push(FieldType::object(self.class.class));

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
                    method_builder
                        .code
                        .get_local(0, &FieldType::object(self.class.class))?;

                    // Call the implementation
                    let method = self.functions[export.index as usize].method;
                    method_builder.code.invoke(method)?;
                    method_builder.code.return_(export_descriptor.return_type)?;

                    method_builder.finish()?;
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
    pub fn generate_constructor(&mut self) -> Result<(), Error> {
        let mut method_builder = self.class.start_method(
            MethodAccessFlags::PUBLIC,
            UnqualifiedName::INIT,
            MethodDescriptor {
                parameters: vec![FieldType::object(self.class.java.classes.util.map)],
                return_type: None,
            },
        )?;
        method_builder.add_generic_signature(
            "(Ljava/util/Map<Ljava/lang/String;Ljava/util/Map<Ljava/lang/String;Ljava/lang/Object;>;>;)V"
        )?;
        let jvm_code = &mut method_builder.code;

        jvm_code.push_instruction(Instruction::ALoad(0))?;
        jvm_code.invoke(jvm_code.java.members.lang.object.init)?;

        // Initial table arrays
        for table in &self.tables {
            if let None = table.origin.imported {
                if !table.origin.exported {
                    jvm_code.push_instruction(Instruction::ALoad(0))?;
                    jvm_code.const_int(table.initial as i32)?; // TODO: error if `u32` is too big
                    jvm_code.new_ref_array(table.table_type.ref_type(&jvm_code.java.classes))?;
                    jvm_code.access_field(table.field.unwrap(), AccessMode::Write)?;
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
                    jvm_code.invoke(jvm_code.java.members.nio.byte_buffer.allocate)?; // TODO: add option for allocate direct
                    jvm_code.access_field(
                        jvm_code.java.members.nio.byte_order.little_endian,
                        AccessMode::Read,
                    )?;
                    jvm_code.invoke(jvm_code.java.members.nio.byte_buffer.order)?;
                    jvm_code.access_field(memory.field.unwrap(), AccessMode::Write)?;
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
                        jvm_code.access_field(global.field.unwrap(), AccessMode::Write)?;
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
                let offset_var = 2;
                let table = &self.tables[table_index as usize];
                if !table.origin.exported {
                    // Load onto the stack the table array
                    jvm_code.push_instruction(Instruction::ALoad(0))?;
                    jvm_code.access_field(table.field.unwrap(), AccessMode::Read)?;

                    // Store the starting offset in a local variable
                    self.translate_init_expr(jvm_code, &init_expr)?;
                    jvm_code.push_instruction(Instruction::IStore(offset_var))?;

                    for item in &element.items {
                        jvm_code.push_instruction(Instruction::Dup)?;
                        jvm_code.push_instruction(Instruction::ILoad(offset_var))?;
                        match item {
                            ElementItem::Func(func_idx) => {
                                let method = self.functions[*func_idx as usize].method;
                                let method_handle = ConstantData::MethodHandle(method);
                                jvm_code.push_instruction(Instruction::Ldc(method_handle))?;
                            }
                            ElementItem::Expr(elem_expr) => {
                                self.translate_init_expr(jvm_code, &elem_expr)?
                            }
                        }
                        jvm_code.push_instruction(Instruction::AAStore)?;
                        jvm_code.push_instruction(Instruction::IInc(offset_var, 1))?;
                    }

                    // Kill the local variable, drop the array
                    jvm_code.push_instruction(Instruction::Pop)?;
                    jvm_code.push_instruction(Instruction::IKill(offset_var))?;
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
                    jvm_code.access_field(memory.field.unwrap(), AccessMode::Read)?;

                    // Set the starting offset for the buffer
                    jvm_code.push_instruction(Instruction::Dup)?;
                    self.translate_init_expr(jvm_code, &init_expr)?;
                    jvm_code.invoke(jvm_code.java.members.nio.buffer.position)?;
                    jvm_code.push_instruction(Instruction::Pop)?;

                    for chunk in data.data.chunks(u16::MAX as usize) {
                        jvm_code
                            .const_string(chunk.iter().map(|&c| c as char).collect::<String>())?;
                        jvm_code.const_string("ISO-8859-1")?;
                        jvm_code.invoke(jvm_code.java.members.lang.string.get_bytes)?;
                        jvm_code.invoke(jvm_code.java.members.nio.byte_buffer.put_bytearray)?;
                    }

                    // Kill the local variable, drop the bytebuffer
                    jvm_code.push_instruction(Instruction::Pop)?;
                } else {
                    todo!("Initialize non-internal memory")
                }
            }
        }

        // Read from imports
        jvm_code.push_instruction(Instruction::ALoad(0))?;
        jvm_code.push_instruction(Instruction::ALoad(1))?;
        for import in &self.imports {
            jvm_code.push_instruction(Instruction::Dup2)?;
            match import {
                WasmImport::Function {
                    module,
                    name,
                    field,
                } => {
                    /* TODO: error handling for
                     *
                     *   - missing module or function in module
                     *   - method handle that doesn't have the right expected type
                     */

                    // Get the module
                    jvm_code.const_string(module.to_string())?;
                    jvm_code.invoke(jvm_code.java.members.util.map.get)?;
                    jvm_code.push_instruction(Instruction::CheckCast(RefType::Object(
                        jvm_code.java.classes.util.map,
                    )))?;

                    // Get the imported handle
                    jvm_code.const_string(name.to_string())?;
                    jvm_code.invoke(jvm_code.java.members.util.map.get)?;
                    jvm_code.push_instruction(Instruction::CheckCast(RefType::Object(
                        jvm_code.java.classes.lang.invoke.method_handle,
                    )))?;

                    // Assign it to the right field
                    jvm_code.access_field(field, AccessMode::Write)?;
                }
            }
        }
        jvm_code.push_instruction(Instruction::Pop2)?;

        // Exports object
        // TODO: make unmodifiable
        jvm_code.push_instruction(Instruction::ALoad(0))?;
        let exports_field = self.class.add_field_with_signature(
            FieldAccessFlags::PUBLIC | FieldAccessFlags::FINAL,
            UnqualifiedName::EXPORTS,
            FieldType::object(jvm_code.java.classes.util.map),
            Some(String::from(
                "Ljava/util/Map<Ljava/lang/String;Ljava/lang/Object;>;",
            )),
        )?;

        jvm_code.new(jvm_code.java.classes.util.hash_map)?;
        jvm_code.push_instruction(Instruction::Dup)?;
        jvm_code.invoke(jvm_code.java.members.util.hash_map.init)?;

        // Add exports to the exports map
        for export in &self.exports {
            jvm_code.push_instruction(Instruction::Dup)?;
            match export.kind {
                ExternalKind::Func => {
                    jvm_code.const_string(export.name.to_string())?;

                    // Implementation function
                    let method = self.functions[export.index as usize].method;
                    let method_handle = ConstantData::MethodHandle(method);

                    // `MethodHandles.insertArguments(hdl, n - 1, new Object[1] { this })`
                    jvm_code.push_instruction(Instruction::Ldc(method_handle))?;
                    jvm_code.const_int((method.descriptor.parameters.len() - 1) as i32)?;
                    jvm_code.const_int(1)?;
                    jvm_code.new_ref_array(RefType::Object(jvm_code.java.classes.lang.object))?;
                    jvm_code.dup()?;
                    jvm_code.const_int(0)?;
                    jvm_code.push_instruction(Instruction::ALoad(0))?;
                    jvm_code.push_instruction(Instruction::AAStore)?;
                    jvm_code.invoke(
                        jvm_code
                            .java
                            .members
                            .lang
                            .invoke
                            .method_handles
                            .insert_arguments,
                    )?;

                    // Put the value in the map
                    jvm_code.invoke(jvm_code.java.members.util.map.put)?;
                    jvm_code.pop()?;
                }

                _ => unimplemented!("non-function export"),
            }
        }

        jvm_code.push_instruction(Instruction::PutField(exports_field))?;

        jvm_code.push_branch_instruction(BranchInstruction::Return)?;
        method_builder.finish()?;

        Ok(())
    }

    /// Translate a constant expression
    fn translate_init_expr(
        &self,
        jvm_code: &mut BytecodeBuilder<'a, 'g>,
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
                    let ref_type = ref_type_from_general(ty, &jvm_code.java.classes)?;
                    jvm_code.const_null(ref_type)?;
                }
                Operator::RefFunc { function_index } => {
                    let method = self.functions[function_index as usize].method;
                    let method_handle = ConstantData::MethodHandle(method);
                    jvm_code.push_instruction(Instruction::Ldc(method_handle))?;
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

    /// Emit the final classes
    ///
    /// The first element in the output vector is the output class. The rest of the elements are
    /// the "part" inner classes.
    pub fn result(mut self, types: &Types) -> Result<Vec<(BinaryName, ClassFile)>, Error> {
        self.generate_exports(types)?;
        self.generate_constructor()?;

        // Assemble all the parts
        let mut parts = self.previous_parts;
        parts.push(self.current_part.result()?);

        // Construct the `NestMembers` and `InnerClasses` attribute
        let (nest_members, inner_classes): (NestMembers, InnerClasses) = {
            let mut nest_members = vec![];
            let mut inner_class_attrs = vec![];
            let constants = &self.class.constants_pool;
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
                let inner_class_name = constants.get_utf8(part.class.name.as_str())?;
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
            .map(|builder| {
                let name = builder.class.name.clone();
                builder.result().map(|cls| (name, cls))
            })
            .collect::<Result<Vec<_>, jvm::Error>>()?;

        Ok(results)
    }
}
