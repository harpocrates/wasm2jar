use super::{
    BootstrapUtilities, Data, Element, Error, ExportName, Function, FunctionTranslator, Global,
    GlobalRepr, ImportName, Memory, MemoryRepr, Settings, Table, TableRepr, UtilityClass,
};
use crate::jvm;
use crate::jvm::class_file;
use crate::jvm::class_graph::{
    AccessMode, ClassData, ClassGraph, ClassId, ConstantData, FieldData, JavaLibrary, MethodData,
    NestedClassData,
};
use crate::jvm::code::{
    BranchInstruction, CodeBuilder, CodeBuilderExts, EqComparison, Instruction, OrdComparison,
};
use crate::jvm::model::{Class, Field, Method};
use crate::jvm::{
    BinaryName, ClassAccessFlags, FieldAccessFlags, FieldType, InnerClassAccessFlags,
    MethodAccessFlags, MethodDescriptor, Name, RefType, UnqualifiedName,
};
use crate::runtime::{
    make_function_class, make_function_table_class, make_global_class, make_memory_class,
    make_reference_table_class, WasmRuntime,
};
use crate::util::Width;
use crate::wasm::{FunctionType, StackType, TableType};
use std::iter;
use wasmparser::types::Types;
use wasmparser::{
    ConstExpr, DataKind, DataSectionReader, ElementKind, ElementSectionReader, ExportSectionReader,
    ExternalKind, FunctionBody, FunctionSectionReader, GlobalSectionReader, Import,
    ImportSectionReader, MemorySectionReader, Parser, Payload, TableSectionReader, Type, TypeRef,
    TypeSectionReader, Validator,
};

/// Main entry point for translating a WASM module
pub struct ModuleTranslator<'a, 'g> {
    settings: Settings,
    validator: Validator,
    class: Class<'g>,
    previous_parts: Vec<Class<'g>>,
    fields_generated: bool,
    class_graph: &'g ClassGraph<'g>,
    java: &'g JavaLibrary<'g>,
    runtime: WasmRuntime<'g>,
    current_part: CurrentPart<'g>,

    /// Populated when visiting the start section with the index of the "start" function. Note that
    /// modules are not required to have a start function (so this may remain empty).
    start_function: Option<usize>,

    /// Utility class (just a carrier for whatever helper methods we may want)
    utilities: UtilityClass<'g>,

    /// Populated as soon as we visit the type section
    types: Vec<FunctionType>,

    /// Populated when we visit functions
    functions: Vec<Function<'a, 'g>>,

    /// Populated when we visit tables
    tables: Vec<Table<'a, 'g>>,

    /// Populated when we visit memories
    memories: Vec<Memory<'a, 'g>>,

    /// Populated when we visit globals
    globals: Vec<Global<'a, 'g>>,

    /// Populated when we visit elements
    elements: Vec<Element<'a, 'g>>,

    /// Populated when we visit datas
    datas: Vec<Data<'a, 'g>>,

    /// Every time we see a new function, this gets incremented
    current_func_idx: u32,
}

struct CurrentPart<'g> {
    class: Class<'g>,
    bootstrap: BootstrapUtilities<'g>,
}
impl<'g> CurrentPart<'g> {
    fn result(self) -> Result<Class<'g>, Error> {
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

        let class_id = class_graph.add_class(ClassData::new(
            settings.output_full_class_name.clone(),
            java.classes.lang.object,
            ClassAccessFlags::PUBLIC | ClassAccessFlags::SUPER,
            None,
        ));
        let current_part = Self::new_part(&settings, class_id, class_graph, java, 0)?;
        let utilities = UtilityClass::new(&settings, class_id, class_graph, java)?;
        let runtime = WasmRuntime::add_to_graph(class_graph, &java.classes);

        Ok(ModuleTranslator {
            settings,
            validator,
            class: Class::new(class_id),
            previous_parts: vec![],
            fields_generated: false,
            class_graph,
            java,
            runtime,
            current_part,
            utilities,
            start_function: None,
            types: vec![],
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
        wasm_module_class: ClassId<'g>,
        class_graph: &'g ClassGraph<'g>,
        java: &'g JavaLibrary<'g>,
        part_idx: usize,
    ) -> Result<CurrentPart<'g>, Error> {
        let name = settings
            .part_short_class_name
            .concat(&UnqualifiedName::number(part_idx));
        let part_id = class_graph.add_class(ClassData::new(
            settings
                .output_full_class_name
                .concat(&UnqualifiedName::DOLLAR)
                .concat(&name),
            java.classes.lang.object,
            ClassAccessFlags::SUPER | ClassAccessFlags::SYNTHETIC | ClassAccessFlags::FINAL,
            Some(NestedClassData {
                access_flags: InnerClassAccessFlags::STATIC | InnerClassAccessFlags::PRIVATE,
                simple_name: Some(name.clone()),
                enclosing_class: wasm_module_class,
            }),
        ));

        Ok(CurrentPart {
            class: Class::new(part_id),
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
            Payload::StartSection { func, range } => {
                self.start_function = Some(func as usize);
                self.validator.start_section(func, &range)?;
            }
            Payload::ElementSection(section) => self.visit_elements(section)?,
            Payload::DataCountSection { count, range } => {
                self.validator.data_count_section(count, &range)?;
                self.visit_data_declarations(count as usize)?;
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
            Payload::CoreTypeSection(section) => self.validator.core_type_section(&section)?,
            Payload::ComponentInstanceSection(section) => {
                self.validator.component_instance_section(&section)?
            }
            Payload::ComponentCanonicalSection(section) => {
                self.validator.component_canonical_section(&section)?
            }
            Payload::ComponentAliasSection(section) => {
                self.validator.component_alias_section(&section)?
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
        log::trace!(
            "Translating function {} (as {:?})",
            self.current_func_idx,
            function.method
        );
        self.current_func_idx += 1;
        let mut code_builder = CodeBuilder::new(self.class_graph, self.java, function.method);

        let mut function_translator = FunctionTranslator::new(
            &function.func_type,
            &self.settings,
            &mut self.utilities,
            &mut self.current_part.bootstrap,
            &mut code_builder,
            self.class.id,
            &self.runtime,
            &self.functions,
            &self.tables,
            &self.memories,
            &self.globals,
            &self.datas,
            &self.elements,
            function_body,
            validator,
        )?;
        function_translator.translate()?;

        self.current_part.class.add_method(Method {
            id: function.method,
            code_impl: Some(code_builder.result()?),
            exceptions: vec![],
            generic_signature: None,
        });

        Ok(())
    }

    fn visit_globals(&mut self, globals: GlobalSectionReader<'a>) -> Result<(), Error> {
        self.validator.global_section(&globals)?;
        for global in globals {
            let wasmparser::Global { ty, init_expr } = global?;
            let global = Global {
                field: None,
                repr: GlobalRepr::UnboxedInternal,
                global_type: StackType::from_general(ty.content_type)?,
                mutable: ty.mutable,
                initial: Some(init_expr),
                import: None,
                export: vec![],
            };
            self.globals.push(global);
        }
        Ok(())
    }

    /// Generate the fields associated with globals
    fn generate_global_fields(&mut self) -> Result<(), Error> {
        for (global_idx, global) in self.globals.iter_mut().enumerate() {
            let access_flags = match global.repr {
                GlobalRepr::BoxedExternal => FieldAccessFlags::FINAL,
                GlobalRepr::UnboxedInternal if global.mutable => FieldAccessFlags::PRIVATE,
                GlobalRepr::UnboxedInternal => FieldAccessFlags::PRIVATE | FieldAccessFlags::FINAL,
            };

            let descriptor = match global.repr {
                GlobalRepr::BoxedExternal => FieldType::object(self.runtime.classes.global),
                GlobalRepr::UnboxedInternal => global.global_type.field_type(&self.java.classes),
            };

            // TODO: this only works for Java 11+. For other Java versions, private fields from
            // outer classes are not visible - getters/setters must be generated (private functions
            // _are_ visible)
            let field_name = self.settings.wasm_global_name(global_idx);
            let field_id = self.class_graph.add_field(FieldData {
                class: self.class.id,
                access_flags,
                name: field_name,
                descriptor,
            });
            self.class.add_field(Field {
                id: field_id,
                generic_signature: None,
                constant_value: None,
            });
            global.field = Some(field_id);
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
                Type::Func(func_type) => {
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
            let mut descriptor = func_type.method_descriptor(&self.java.classes);
            descriptor.parameters.push(FieldType::object(self.class.id));

            let method = self.class_graph.add_method(MethodData {
                class: self.current_part.class.id, // TODO: choose the right part here
                name: self.settings.wasm_function_name(func_idx),
                access_flags: MethodAccessFlags::STATIC,
                descriptor,
            });

            self.functions.push(Function {
                func_type,
                method,
                tailcall_method: None,
                import: None,
                export: vec![],
            });
        }
        Ok(())
    }

    /// Visit data declarations
    fn visit_data_declarations(&mut self, data_count: usize) -> Result<(), Error> {
        for data_idx in 0..data_count {
            let method = self.class_graph.add_method(MethodData {
                class: self.class.id,
                name: self.settings.wasm_data_getter_name(data_idx),
                access_flags: MethodAccessFlags::STATIC,
                descriptor: MethodDescriptor {
                    parameters: vec![FieldType::object(self.class.id)],
                    return_type: Some(FieldType::array(FieldType::byte())),
                },
            });
            let field = self.class_graph.add_field(FieldData {
                class: self.class.id,
                name: self.settings.wasm_data_name(data_idx),
                access_flags: FieldAccessFlags::PRIVATE,
                descriptor: FieldType::array(FieldType::byte()),
            });

            self.datas.push(Data {
                kind: None,
                bytes: None,
                method,
                field,
            });
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
        let class = self.class.id;
        let import_name = ImportName {
            module: import.module,
            name: import.name,
        };

        match import.ty {
            TypeRef::Table(table_type) => {
                self.tables.push(Table {
                    field: None,
                    repr: TableRepr::External,
                    table_type,
                    import: Some(import_name),
                    export: vec![],
                });
            }

            TypeRef::Global(global_type) => {
                self.globals.push(Global {
                    field: None,
                    repr: GlobalRepr::BoxedExternal,
                    global_type: StackType::from_general(global_type.content_type)?,
                    mutable: global_type.mutable,
                    initial: None,
                    import: Some(import_name),
                    export: vec![],
                });
            }

            TypeRef::Func(func_type_idx) => {
                let java = &self.java;

                // This is the index which which the imported function is called
                let func_idx = self.current_func_idx;
                self.current_func_idx += 1;

                // This is the expected descriptor of the imported function
                let func_type = self.types[func_type_idx as usize].clone();
                let imported_descriptor = func_type.method_descriptor(&java.classes);

                // Build up a method descriptor, which includes a trailing "WASM module" argument
                let mut descriptor = imported_descriptor.clone();
                descriptor.parameters.push(FieldType::object(class));

                // Field that will store the `MethodHandle` corresponding to the imported function
                let import_field = self.class_graph.add_field(FieldData {
                    class,
                    access_flags: FieldAccessFlags::PRIVATE | FieldAccessFlags::FINAL,
                    name: self.settings.wasm_import_name(func_idx as usize),
                    descriptor: FieldType::object(java.classes.lang.invoke.method_handle),
                });
                self.class.add_field(Field {
                    id: import_field,
                    generic_signature: None,
                    constant_value: None,
                });

                // Trampoline method, whose sole responsibility is to invoke the method handle
                let method_id = self.class_graph.add_method(MethodData {
                    class: self.current_part.class.id,
                    name: self.settings.wasm_function_name(func_idx as usize),
                    descriptor: descriptor.clone(),
                    access_flags: MethodAccessFlags::STATIC,
                });
                self.functions.push(Function {
                    func_type,
                    method: method_id,
                    tailcall_method: None,
                    import: Some((import_name, import_field)),
                    export: vec![],
                });
                let mut code = CodeBuilder::new(self.class_graph, self.java, method_id);

                // `wasmModule.importedMethodHandle.invokeExact(arg0, ..., argn)`
                code.get_local(
                    imported_descriptor.parameter_length(false) as u16,
                    &FieldType::object(class),
                )?;
                code.access_field(import_field, AccessMode::Read)?;
                let mut offset = 0;
                for parameter in &imported_descriptor.parameters {
                    code.get_local(offset, parameter)?;
                    offset += parameter.width() as u16;
                }
                let return_type = imported_descriptor.return_type;
                code.invoke_invoke_exact(imported_descriptor)?;
                code.return_(return_type)?;

                self.current_part.class.add_method(Method {
                    id: method_id,
                    code_impl: Some(code.result()?),
                    exceptions: vec![],
                    generic_signature: None,
                });
            }

            TypeRef::Memory(memory_type) => {
                self.memories.push(Memory {
                    field: None,
                    repr: MemoryRepr::External,
                    memory_type,
                    import: Some(import_name),
                    export: vec![],
                });
            }

            TypeRef::Tag(_) => todo!("import tag"),
        }

        Ok(())
    }

    /// Visit the tables section
    fn visit_tables(&mut self, tables: TableSectionReader<'a>) -> Result<(), Error> {
        self.validator.table_section(&tables)?;
        for table in tables {
            let table_type = table?;
            let table = Table {
                field: None,
                repr: TableRepr::Internal,
                table_type,
                import: None,
                export: vec![],
            };
            self.tables.push(table);
        }
        Ok(())
    }

    /// Generate the fields associated with tables
    fn generate_table_fields(&mut self) -> Result<(), Error> {
        for (table_idx, table) in self.tables.iter_mut().enumerate() {
            // TODO: if the limits on the table constrain it to never grow, make the field final
            let access_flags = match table.repr {
                TableRepr::External => FieldAccessFlags::FINAL,
                TableRepr::Internal => FieldAccessFlags::PRIVATE,
            };

            let descriptor = match (table.repr, table.table_type.element_type) {
                (TableRepr::External, wasmparser::ValType::FuncRef) => {
                    FieldType::object(self.runtime.classes.function_table)
                }
                (TableRepr::External, wasmparser::ValType::ExternRef) => {
                    FieldType::object(self.runtime.classes.reference_table)
                }
                (TableRepr::Internal, wasmparser::ValType::FuncRef) => FieldType::array(
                    FieldType::object(self.java.classes.lang.invoke.method_handle),
                ),
                (TableRepr::Internal, wasmparser::ValType::ExternRef) => {
                    FieldType::array(FieldType::object(self.java.classes.lang.object))
                }
                _ => panic!(),
            };

            // TODO: this only works for Java 11+. For other Java versions, private fields from
            // outer classes are not visible - getters/setters must be generated (private functions
            // _are_ visible)
            let field_name = self.settings.wasm_table_name(table_idx);
            let field_id = self.class_graph.add_field(FieldData {
                class: self.class.id,
                access_flags,
                name: field_name,
                descriptor,
            });
            self.class.add_field(Field {
                id: field_id,
                generic_signature: None,
                constant_value: None,
            });
            table.field = Some(field_id);
        }

        Ok(())
    }

    /// Visit the memories section
    fn visit_memories(&mut self, memories: MemorySectionReader<'a>) -> Result<(), Error> {
        self.validator.memory_section(&memories)?;
        for memory in memories {
            let memory_type = memory?;
            let memory = Memory {
                field: None,
                repr: MemoryRepr::Internal,
                memory_type,
                import: None,
                export: vec![],
            };
            self.memories.push(memory);
        }

        Ok(())
    }

    /// Generate the fields associated with memories
    fn generate_memory_fields(&mut self) -> Result<(), Error> {
        for (memory_idx, memory) in &mut self.memories.iter_mut().enumerate() {
            let access_flags = match memory.repr {
                MemoryRepr::External => FieldAccessFlags::FINAL,
                MemoryRepr::Internal => FieldAccessFlags::PUBLIC, // empty(), // FieldAccessFlags::PRIVATE,
            };

            let descriptor = match memory.repr {
                MemoryRepr::External => FieldType::object(self.runtime.classes.memory),
                MemoryRepr::Internal => FieldType::object(self.java.classes.nio.byte_buffer),
            };

            // TODO: this only works for Java 11+. For other Java versions, private fields from
            // outer classes are not visible - getters/setters must be generated (private functions
            // _are_ visible)
            let field_name = self.settings.wasm_memory_name(memory_idx);
            let field_id = self.class_graph.add_field(FieldData {
                class: self.class.id,
                access_flags,
                name: field_name,
                descriptor,
            });
            self.class.add_field(Field {
                id: field_id,
                generic_signature: None,
                constant_value: None,
            });
            memory.field = Some(field_id);
        }

        Ok(())
    }

    /// Visit the exports
    ///
    /// The actual processing of the exports is in `generate_constructor` or `generate_exports`,
    /// since the module resources aren't ready at this point.
    fn visit_exports(&mut self, exports: ExportSectionReader<'a>) -> Result<(), Error> {
        self.validator.export_section(&exports)?;
        for export in exports {
            let export = export?;
            let export_name = ExportName { name: export.name };
            match export.kind {
                ExternalKind::Func => {
                    let function: &mut Function = self
                        .functions
                        .get_mut(export.index as usize)
                        .expect("Exporting function that doesn't exist");
                    function
                        .export
                        .push((export_name, self.settings.methods_for_function_exports));
                }

                ExternalKind::Table => {
                    let table: &mut Table = self
                        .tables
                        .get_mut(export.index as usize)
                        .expect("Exporting table that doesn't exist");
                    table.repr = TableRepr::External;
                    table.export.push(export_name);
                }

                ExternalKind::Memory => {
                    let memory: &mut Memory = self
                        .memories
                        .get_mut(export.index as usize)
                        .expect("Exporting memory that doesn't exist");
                    memory.repr = MemoryRepr::External;
                    memory.export.push(export_name);
                }

                ExternalKind::Global => {
                    let global: &mut Global = self
                        .globals
                        .get_mut(export.index as usize)
                        .expect("Exporting global that doesn't exist");
                    global.repr = GlobalRepr::BoxedExternal;
                    global.export.push(export_name);
                }

                _ => unimplemented!(),
            }
        }
        Ok(())
    }

    /// Generate members in the outer class corresponding to exports
    fn generate_exports(&mut self) -> Result<(), Error> {
        let class = self.class.id;
        for function in &self.functions {
            for (ExportName { name }, generate_method) in &function.export {
                if !generate_method {
                    continue;
                }

                let export_descriptor = function.func_type.method_descriptor(&self.java.classes);

                // Implementation function
                let mut underlying_descriptor = export_descriptor.clone();
                underlying_descriptor
                    .parameters
                    .push(FieldType::object(class));

                let name: String = self.settings.renamer.rename_function(name);
                let method_id = self.class_graph.add_method(MethodData {
                    class,
                    name: UnqualifiedName::from_string(name).map_err(Error::MalformedName)?,
                    descriptor: export_descriptor.clone(),
                    access_flags: MethodAccessFlags::PUBLIC,
                });

                let mut code = CodeBuilder::new(self.class_graph, self.java, method_id);

                // Push the method arguments onto the stack
                let mut offset = 1;
                for parameter in &export_descriptor.parameters {
                    code.get_local(offset, parameter)?;
                    offset += parameter.width() as u16;
                }
                code.get_local(0, &FieldType::object(class))?;

                // Call the implementation
                code.invoke(function.method)?;
                code.return_(export_descriptor.return_type)?;

                self.class.add_method(Method {
                    id: method_id,
                    code_impl: Some(code.result()?),
                    exceptions: vec![],
                    generic_signature: None,
                });
            }
        }

        Ok(())
    }

    /// Generate functions for summoning data and elements
    fn generate_constant_segments(&mut self) -> Result<(), Error> {
        for data in &self.datas {
            self.class
                .add_method(data.generate_method(self.class_graph, self.java)?);
            self.class.add_field(Field::new(data.field));
        }
        for element in &self.elements {
            self.class.add_method(element.generate_method(
                self.class_graph,
                self.java,
                &self.runtime,
                &self.functions,
                &self.globals,
            )?);
            self.class.add_field(Field::new(element.field));
        }

        Ok(())
    }

    /// Visit the elements section
    fn visit_elements(&mut self, elements: ElementSectionReader<'a>) -> Result<(), Error> {
        self.validator.element_section(&elements)?;

        for (element_idx, element) in elements.into_iter().enumerate() {
            let element = element?;
            let items = element
                .items
                .get_items_reader()?
                .into_iter()
                .collect::<Result<Vec<_>, _>>()?;
            let element_type = TableType::from_general(element.ty)?;

            let method = self.class_graph.add_method(MethodData {
                class: self.class.id,
                name: self.settings.wasm_element_getter_name(element_idx),
                access_flags: MethodAccessFlags::STATIC,
                descriptor: MethodDescriptor {
                    parameters: vec![FieldType::object(self.class.id)],
                    return_type: Some(FieldType::array(
                        element_type.field_type(&self.java.classes),
                    )),
                },
            });
            let field = self.class_graph.add_field(FieldData {
                class: self.class.id,
                name: self.settings.wasm_element_name(element_idx),
                access_flags: FieldAccessFlags::PRIVATE,
                descriptor: FieldType::array(element_type.field_type(&self.java.classes)),
            });

            self.elements.push(Element {
                kind: element.kind,
                element_type,
                items,
                method,
                field,
            });
        }
        Ok(())
    }

    /// Visit the datas section
    fn visit_datas(&mut self, datas: DataSectionReader<'a>) -> Result<(), Error> {
        self.validator.data_section(&datas)?;

        // This will be non-zero if there was a data declaration section and 0 otherwise
        let expected_datas = datas.get_count() as usize;
        if self.datas.is_empty() && expected_datas != 0 {
            self.visit_data_declarations(expected_datas)?;
        }

        // Fill in data about the segments
        for (segment, data) in datas.into_iter().zip(self.datas.iter_mut()) {
            let segment = segment?;
            data.kind = Some(segment.kind);
            data.bytes = Some(segment.data);
        }
        Ok(())
    }

    /// Generate code to lookup an import
    ///
    /// Assumes that the top of the stack will contain the imports map.
    fn lookup_import(code: &mut CodeBuilder, import: &ImportName) -> Result<(), Error> {
        let module_found = code.fresh_label();
        let entity_found = code.fresh_label();

        // Get the module
        code.const_string(import.module.to_string())?;
        code.invoke(code.java.members.util.map.get)?;
        code.push_instruction(Instruction::Dup)?;
        code.push_branch_instruction(BranchInstruction::IfNull(
            EqComparison::NE,
            module_found,
            (),
        ))?;
        code.new(code.java.classes.lang.illegal_argument_exception)?;
        code.push_instruction(Instruction::Dup)?;
        code.const_string(format!(
            "Could not find module for import {}.{}",
            import.module, import.name
        ))?;
        code.invoke(code.java.members.lang.illegal_argument_exception.init)?;
        code.push_branch_instruction(BranchInstruction::AThrow)?;
        code.place_label(module_found)?;
        code.push_instruction(Instruction::CheckCast(RefType::Object(
            code.java.classes.util.map,
        )))?;

        // Get the member
        code.const_string(import.name.to_string())?;
        code.invoke(code.java.members.util.map.get)?;
        code.push_instruction(Instruction::Dup)?;
        code.push_branch_instruction(BranchInstruction::IfNull(
            EqComparison::NE,
            entity_found,
            (),
        ))?;
        code.new(code.java.classes.lang.illegal_argument_exception)?;
        code.push_instruction(Instruction::Dup)?;
        code.const_string(format!(
            "Could not find {} in module {}",
            import.name, import.module
        ))?;
        code.invoke(code.java.members.lang.illegal_argument_exception.init)?;
        code.push_branch_instruction(BranchInstruction::AThrow)?;
        code.place_label(entity_found)?;

        Ok(())
    }

    /// Generate a constructor
    pub fn generate_constructor(&mut self) -> Result<(), Error> {
        let constructor_id = self.class_graph.add_method(MethodData {
            class: self.class.id,
            name: UnqualifiedName::INIT,
            descriptor: MethodDescriptor {
                parameters: vec![FieldType::object(self.java.classes.util.map)],
                return_type: None,
            },
            access_flags: MethodAccessFlags::PUBLIC,
        });

        let mut jvm_code = CodeBuilder::new(self.class_graph, self.java, constructor_id);

        jvm_code.push_instruction(Instruction::ALoad(0))?;
        jvm_code.invoke(jvm_code.java.members.lang.object.init)?;

        // Read from imports
        jvm_code.push_instruction(Instruction::ALoad(0))?;
        jvm_code.push_instruction(Instruction::ALoad(1))?;
        for function in &self.functions {
            if let Some((import_loc, import_field)) = &function.import {
                jvm_code.push_instruction(Instruction::Dup2)?;

                // Get the imported handle
                Self::lookup_import(&mut jvm_code, &import_loc)?;
                jvm_code.checkcast(self.runtime.classes.function)?;
                jvm_code.access_field(self.runtime.members.function.handle, AccessMode::Read)?;

                // Check that it has the right signature (TODO: this doesn't check multi-return)
                let right_type = jvm_code.fresh_label();
                jvm_code.dup()?;
                jvm_code.invoke(self.java.members.lang.invoke.method_handle.r#type)?;
                let expected_descriptor = function.func_type.method_descriptor(&self.java.classes);
                match expected_descriptor.return_type {
                    Some(ret_class) => jvm_code.const_class(ret_class)?,
                    None => jvm_code
                        .access_field(self.java.members.lang.void.r#type, AccessMode::Read)?,
                }
                let local_idx = 2;
                jvm_code.zero_local(local_idx, FieldType::int())?;
                jvm_code.const_int(expected_descriptor.parameters.len() as i32)?;
                jvm_code.new_ref_array(RefType::Object(self.java.classes.lang.class))?;
                for parameter in expected_descriptor.parameters {
                    jvm_code.dup()?;
                    jvm_code.get_local(local_idx, &FieldType::int())?;
                    jvm_code.const_class(parameter)?;
                    jvm_code.push_instruction(Instruction::AAStore)?;
                    jvm_code.push_instruction(Instruction::IInc(local_idx, 1))?;
                }
                jvm_code.kill_top_local(local_idx, Some(FieldType::int()))?;
                jvm_code.invoke(self.java.members.lang.invoke.method_type.method_type)?;
                jvm_code.invoke(self.java.members.lang.object.equals)?;
                jvm_code.push_branch_instruction(BranchInstruction::If(
                    OrdComparison::NE,
                    right_type,
                    (),
                ))?;
                jvm_code.new(self.java.classes.lang.illegal_argument_exception)?;
                jvm_code.push_instruction(Instruction::Dup)?;
                jvm_code.const_string(format!(
                    "Invalid import type for function import {}.{} (expected {:?})",
                    import_loc.module, import_loc.name, function.func_type,
                ))?;
                jvm_code.invoke(self.java.members.lang.illegal_argument_exception.init)?;
                jvm_code.push_branch_instruction(BranchInstruction::AThrow)?;
                jvm_code.place_label(right_type)?;

                // Assign it to the right field
                jvm_code.access_field(*import_field, AccessMode::Write)?;
            } else {
                break;
            }
        }
        for global in &self.globals {
            if let Some(import_loc) = &global.import {
                jvm_code.push_instruction(Instruction::Dup2)?;

                // Get the imported global
                Self::lookup_import(&mut jvm_code, &import_loc)?;
                jvm_code.checkcast(self.runtime.classes.global)?;

                // Check that it has the right signature
                let right_type = jvm_code.fresh_label();
                jvm_code.dup()?;
                jvm_code.access_field(self.runtime.members.global.value, AccessMode::Read)?;
                jvm_code.invoke(self.java.members.lang.object.get_class)?;
                let expected_class = match global.global_type {
                    StackType::I32 => self.java.classes.lang.integer,
                    StackType::I64 => self.java.classes.lang.long,
                    StackType::F32 => self.java.classes.lang.float,
                    StackType::F64 => self.java.classes.lang.double,
                    StackType::FuncRef => self.java.classes.lang.invoke.method_handle,
                    StackType::ExternRef => self.java.classes.lang.object,
                };
                jvm_code.const_class(FieldType::object(expected_class))?;
                jvm_code.invoke(self.java.members.lang.class.is_assignable_from)?;
                jvm_code.push_branch_instruction(BranchInstruction::If(
                    OrdComparison::NE,
                    right_type,
                    (),
                ))?;
                jvm_code.new(self.java.classes.lang.illegal_argument_exception)?;
                jvm_code.push_instruction(Instruction::Dup)?;
                jvm_code.const_string(format!(
                    "Invalid import type for global import {}.{} (expected {:?})",
                    import_loc.module, import_loc.name, global.global_type,
                ))?;
                jvm_code.invoke(self.java.members.lang.illegal_argument_exception.init)?;
                jvm_code.push_branch_instruction(BranchInstruction::AThrow)?;
                jvm_code.place_label(right_type)?;

                // Assign it to the right field
                jvm_code.access_field(global.field.unwrap(), AccessMode::Write)?;
            } else {
                break;
            }
        }
        for memory in &self.memories {
            if let Some(import_loc) = &memory.import {
                jvm_code.push_instruction(Instruction::Dup2)?;

                // Get the imported memory
                Self::lookup_import(&mut jvm_code, &import_loc)?;
                jvm_code.checkcast(self.runtime.classes.memory)?;

                // Assign it to the right field
                jvm_code.access_field(memory.field.unwrap(), AccessMode::Write)?;
            } else {
                break;
            }
        }
        for table in &self.tables {
            if let Some(import_loc) = &table.import {
                jvm_code.push_instruction(Instruction::Dup2)?;

                // Get the imported memory
                Self::lookup_import(&mut jvm_code, &import_loc)?;
                let table_type = match table.table_type.element_type {
                    wasmparser::ValType::FuncRef => self.runtime.classes.function_table,
                    wasmparser::ValType::ExternRef => self.runtime.classes.reference_table,
                    _ => panic!(),
                };
                jvm_code.checkcast(table_type)?;

                // Assign it to the right field
                jvm_code.access_field(table.field.unwrap(), AccessMode::Write)?;
            } else {
                break;
            }
        }
        jvm_code.push_instruction(Instruction::Pop2)?;

        // Initial table arrays
        for table in &self.tables {
            if table.import.is_none() {
                jvm_code.push_instruction(Instruction::ALoad(0))?;
                jvm_code.const_int(table.table_type.initial as i32)?; // TODO: error if `u32` is too big
                jvm_code.new_ref_array(table.element_type(&jvm_code.java.classes))?;

                if let TableRepr::External = table.repr {
                    let (table_class, table_init) = match table.table_type.element_type {
                        wasmparser::ValType::FuncRef => (
                            self.runtime.classes.function_table,
                            self.runtime.members.function_table.init,
                        ),
                        wasmparser::ValType::ExternRef => (
                            self.runtime.classes.reference_table,
                            self.runtime.members.reference_table.init,
                        ),
                        _ => panic!(),
                    };
                    jvm_code.new(table_class)?;
                    jvm_code.push_instruction(Instruction::DupX1)?;
                    jvm_code.push_instruction(Instruction::Swap)?;
                    jvm_code.invoke(table_init)?;
                }

                jvm_code.access_field(table.field.unwrap(), AccessMode::Write)?;
            }
        }

        // Initialize memory
        for memory in &self.memories {
            if memory.import.is_none() {
                if memory.memory_type.memory64 {
                    todo!("64-bit memory")
                }

                let initial: u64 = memory.memory_type.initial * 65536;
                jvm_code.push_instruction(Instruction::ALoad(0))?;
                jvm_code.const_int(initial as i32)?; // TODO: error if too big
                jvm_code.invoke(jvm_code.java.members.nio.byte_buffer.allocate)?; // TODO: add option for allocate direct
                jvm_code.access_field(
                    jvm_code.java.members.nio.byte_order.little_endian,
                    AccessMode::Read,
                )?;
                jvm_code.invoke(jvm_code.java.members.nio.byte_buffer.order)?;

                if let MemoryRepr::External = memory.repr {
                    jvm_code.new(self.runtime.classes.memory)?;
                    jvm_code.push_instruction(Instruction::DupX1)?;
                    jvm_code.push_instruction(Instruction::Swap)?;
                    jvm_code.invoke(self.runtime.members.memory.init)?;
                }

                jvm_code.access_field(memory.field.unwrap(), AccessMode::Write)?;
            }
        }

        // Initialize globals
        for global in &self.globals {
            if let Some(init_expr) = &global.initial {
                jvm_code.push_instruction(Instruction::ALoad(0))?;

                match global.repr {
                    GlobalRepr::BoxedExternal => {
                        jvm_code.new(self.runtime.classes.global)?;
                        jvm_code.push_instruction(Instruction::Dup)?;

                        self.translate_const_expr(&mut jvm_code, init_expr)?;
                        match global.global_type {
                            StackType::I32 => {
                                jvm_code.invoke(self.java.members.lang.integer.value_of)?
                            }
                            StackType::I64 => {
                                jvm_code.invoke(self.java.members.lang.long.value_of)?
                            }
                            StackType::F32 => {
                                jvm_code.invoke(self.java.members.lang.float.value_of)?
                            }
                            StackType::F64 => {
                                jvm_code.invoke(self.java.members.lang.double.value_of)?
                            }
                            StackType::FuncRef | StackType::ExternRef => (),
                        }
                        jvm_code.invoke(self.runtime.members.global.init)?;
                    }
                    GlobalRepr::UnboxedInternal => {
                        self.translate_const_expr(&mut jvm_code, init_expr)?;
                    }
                }

                jvm_code.access_field(global.field.unwrap(), AccessMode::Write)?;
            }
        }

        // Initialize active elements
        for element in &self.elements {
            if let ElementKind::Active {
                table_index,
                offset_expr,
            } = element.kind
            {
                let table = &self.tables[table_index as usize];

                // Load onto the stack the element array
                jvm_code.push_instruction(Instruction::ALoad(0))?;
                jvm_code.invoke(element.method)?;

                // Offset in the element from which to start copying
                jvm_code.const_int(0)?;

                // Load onto the stack the table array
                jvm_code.push_instruction(Instruction::ALoad(0))?;
                table.load_array(&self.runtime, &mut jvm_code)?;

                // Offset in the table where to begin writing
                self.translate_const_expr(&mut jvm_code, &offset_expr)?;

                // Number of elements to write
                jvm_code.const_int(element.items.len() as i32)?;

                // `System.arraycopy(element, 0, table, start_off, element.length)`
                jvm_code.invoke(jvm_code.java.members.lang.system.arraycopy)?;
            }

            // Drop non-passive elements
            if let ElementKind::Passive = element.kind {
            } else {
                element.drop_element(&mut jvm_code, 0)?;
            }
        }

        // Initialize active data
        for data in &self.datas {
            if let DataKind::Active {
                memory_index,
                offset_expr,
            } = data.kind.unwrap()
            {
                let memory = &self.memories[memory_index as usize];

                // Load onto the stack the memory bytebuffer
                jvm_code.push_instruction(Instruction::ALoad(0))?;
                memory.load_bytebuffer(&self.runtime, &mut jvm_code)?;

                // Set the starting offset for the buffer
                jvm_code.push_instruction(Instruction::Dup)?;
                self.translate_const_expr(&mut jvm_code, &offset_expr)?;
                jvm_code.invoke(jvm_code.java.members.nio.buffer.position)?;
                jvm_code.push_instruction(Instruction::Pop)?;

                // Get the data as a bytebuffer and put that
                jvm_code.push_instruction(Instruction::ALoad(0))?;
                jvm_code.invoke(data.method)?;
                jvm_code.invoke(jvm_code.java.members.nio.byte_buffer.put_bytearray_relative)?;

                // Kill the local variable, drop the bytebuffer
                jvm_code.push_instruction(Instruction::Pop)?;

                data.drop_data(&mut jvm_code, 0)?;
            }

            // Drop non-passive data
            if let DataKind::Passive = data.kind.unwrap() {
            } else {
                data.drop_data(&mut jvm_code, 0)?;
            }
        }

        // Exports object
        // TODO: make unmodifiable
        jvm_code.push_instruction(Instruction::ALoad(0))?;
        let exports_field = self.class_graph.add_field(FieldData {
            class: self.class.id,
            access_flags: FieldAccessFlags::PUBLIC | FieldAccessFlags::FINAL,
            name: UnqualifiedName::EXPORTS,
            descriptor: FieldType::object(jvm_code.java.classes.util.map),
        });
        self.class.add_field(Field {
            id: exports_field,
            generic_signature: Some(String::from(
                "Ljava/util/Map<Ljava/lang/String;Ljava/lang/Object;>;",
            )),
            constant_value: None,
        });

        jvm_code.new(jvm_code.java.classes.util.hash_map)?;
        jvm_code.push_instruction(Instruction::Dup)?;
        jvm_code.invoke(jvm_code.java.members.util.hash_map.init)?;

        // Add function exports to the exports map
        for function in &self.functions {
            for (ExportName { name }, _) in &function.export {
                jvm_code.push_instruction(Instruction::Dup)?;
                jvm_code.const_string(name.to_string())?;

                // Implementation function
                let method = function.method;
                let method_handle = ConstantData::MethodHandle(method);

                // `new org.wasm2jar.Function(handle);`
                jvm_code.new(self.runtime.classes.function)?;
                jvm_code.push_instruction(Instruction::Dup)?;

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
                jvm_code.invoke(self.runtime.members.function.init)?;

                // Put the value in the map
                jvm_code.invoke(jvm_code.java.members.util.map.put)?;
                jvm_code.pop()?;
            }
        }

        // Add global exports to the exports map
        for global in &self.globals {
            for ExportName { name } in &global.export {
                jvm_code.push_instruction(Instruction::Dup)?;
                jvm_code.const_string(name.to_string())?;

                // Get global
                let field = global.field.unwrap();
                jvm_code.push_instruction(Instruction::ALoad(0))?;
                jvm_code.access_field(field, AccessMode::Read)?;

                // Put the value in the map
                jvm_code.invoke(jvm_code.java.members.util.map.put)?;
                jvm_code.pop()?;
            }
        }

        // Add table exports to the exports map
        for table in &self.tables {
            for ExportName { name } in &table.export {
                jvm_code.push_instruction(Instruction::Dup)?;
                jvm_code.const_string(name.to_string())?;

                // Get table
                let field = table.field.unwrap();
                jvm_code.push_instruction(Instruction::ALoad(0))?;
                jvm_code.access_field(field, AccessMode::Read)?;

                // Put the value in the map
                jvm_code.invoke(jvm_code.java.members.util.map.put)?;
                jvm_code.pop()?;
            }
        }

        // Add memory exports to the exports map
        for memory in &self.memories {
            for ExportName { name } in &memory.export {
                jvm_code.push_instruction(Instruction::Dup)?;
                jvm_code.const_string(name.to_string())?;

                // Get table
                let field = memory.field.unwrap();
                jvm_code.push_instruction(Instruction::ALoad(0))?;
                jvm_code.access_field(field, AccessMode::Read)?;

                // Put the value in the map
                jvm_code.invoke(jvm_code.java.members.util.map.put)?;
                jvm_code.pop()?;
            }
        }

        jvm_code.push_instruction(Instruction::PutField(exports_field))?;

        // Main function, if there is one
        if let Some(start_func_idx) = self.start_function {
            jvm_code.push_instruction(Instruction::ALoad(0))?;
            jvm_code.invoke(self.functions[start_func_idx].method)?;
        }

        jvm_code.push_branch_instruction(BranchInstruction::Return)?;

        self.class.add_method(Method {
            id: constructor_id,
            code_impl: Some(jvm_code.result()?),
            exceptions: vec![],
            generic_signature: Some(
                "(Ljava/util/Map<Ljava/lang/String;Ljava/util/Map<Ljava/lang/String;Ljava/lang/Object;>;>;)V".to_string()
            ),
        });

        Ok(())
    }

    /// Translate a constant expression
    ///
    /// Local 0 is the wasm object.
    fn translate_const_expr(
        &self,
        jvm_code: &mut CodeBuilder<'g>,
        init_expr: &ConstExpr,
    ) -> Result<(), Error> {
        super::translate_const_expr(
            &self.functions,
            &self.globals,
            &self.runtime,
            0,
            jvm_code,
            init_expr,
        )
    }

    /// Emit the final classes
    ///
    /// The first element in the output vector is the output class. The rest of the elements are
    /// the "part" inner classes.
    pub fn result(mut self) -> Result<Vec<(BinaryName, class_file::ClassFile)>, Error> {
        self.generate_exports()?;
        self.generate_constant_segments()?;
        self.generate_constructor()?;

        // Prepare runtime libraries
        let runtime_classes = vec![
            make_function_class(self.class_graph, self.java, &self.runtime)?,
            make_global_class(self.class_graph, self.java, &self.runtime)?,
            make_function_table_class(self.class_graph, self.java, &self.runtime)?,
            make_reference_table_class(self.class_graph, self.java, &self.runtime)?,
            make_memory_class(self.class_graph, self.java, &self.runtime)?,
        ];

        // Assemble all the parts
        let mut parts = self.previous_parts;
        parts.push(self.current_part.result()?);

        // Final results
        let results: Vec<(BinaryName, class_file::ClassFile)> = iter::once(self.class)
            .chain(self.utilities.into_builder().into_iter())
            .chain(parts.into_iter())
            .chain(runtime_classes.into_iter())
            .map(|builder| {
                let name = builder.id.name.clone();
                builder
                    .serialize(class_file::Version::JAVA11)
                    .map(|cls| (name, cls))
            })
            .collect::<Result<Vec<_>, jvm::Error>>()?;

        Ok(results)
    }
}
