use super::{
    BootstrapUtilities, Element, Error, ExportName, Function, FunctionTranslator, Global,
    ImportName, MemberOrigin, Memory, Settings, Table, UtilityClass, GlobalRepr,
};
use crate::jvm;
use crate::jvm::class_file;
use crate::jvm::class_graph::{
    AccessMode, ClassData, ClassGraph, ClassId, ConstantData, FieldData, JavaLibrary, MethodData,
    NestedClassData,
};
use crate::jvm::code::{BranchInstruction, CodeBuilder, CodeBuilderExts, Instruction};
use crate::jvm::model::{Class, Field, Method};
use crate::jvm::{
    BinaryName, ClassAccessFlags, FieldAccessFlags, FieldType, InnerClassAccessFlags,
    MethodAccessFlags, MethodDescriptor, Name, RefType, UnqualifiedName,
};
use crate::util::Width;
use crate::wasm::{ref_type_from_general, FunctionType, StackType, TableType};
use std::iter;
use wasmparser::types::Types;
use wasmparser::{
    Data, DataKind, DataSectionReader, ElementItem, ElementKind, ElementSectionReader,
    ExportSectionReader, ExternalKind, FunctionBody, FunctionSectionReader, GlobalSectionReader,
    Import, ImportSectionReader, InitExpr, MemorySectionReader, Operator, Parser, Payload,
    TableSectionReader, Type, TypeRef, TypeSectionReader, Validator,
};
use crate::runtime::{WasmRuntime, make_function_class, make_global_class, make_function_table_class, make_reference_table_class, make_memory_class};

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

    /// Utility class (just a carrier for whatever helper methods we may want)
    utilities: UtilityClass<'g>,

    /// Populated as soon as we visit the type section
    types: Vec<FunctionType>,

    /// Populated when we visit functions
    functions: Vec<Function<'a, 'g>>,

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
        self.current_func_idx += 1;
        let mut code_builder = CodeBuilder::new(self.class_graph, self.java, function.method);

        let mut function_translator = FunctionTranslator::new(
            &function.func_type,
            &self.settings,
            &mut self.utilities,
            &mut self.current_part.bootstrap,
            &mut code_builder,
            self.class.id,
            &self.functions,
            &self.tables,
            &self.memories,
            &self.globals,
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
            let origin = MemberOrigin {
                imported: None,
                exported: false,
            };
            let field_name = self
                .settings
                .wasm_global_import_name(self.globals.len());
            let global = Global {
                origin,
                field_name,
                field: None,
                repr: GlobalRepr::UnboxedInternal,
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
            let field_id = self.class_graph.add_field(FieldData {
                class: self.class.id,
                access_flags,
                name: global.field_name.clone(),
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
                export: None,
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
        let origin = MemberOrigin {
            imported: Some(Some(import.module.to_owned())),
            exported: false,
        };

        let name = UnqualifiedName::from_string(import.name.to_owned()).unwrap();
        let class = self.class.id;

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
                    field: None,
                    field_name: self.settings.wasm_global_import_name(self.globals.len()),
                    repr: GlobalRepr::BoxedExternal,
                    global_type: StackType::from_general(global_type.content_type)?,
                    mutable: global_type.mutable,
                    initial: None,
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
                let import_name = ImportName {
                    module: import.module,
                    name: import.name,
                };

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
                    export: None,
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
            let field_id = self.class_graph.add_field(FieldData {
                class: self.class.id,
                access_flags: FieldAccessFlags::PRIVATE,
                name: table.field_name.clone(),
                descriptor: FieldType::array(table.table_type.field_type(&self.java.classes)),
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
                let field_id = self.class_graph.add_field(FieldData {
                    class: self.class.id,
                    access_flags: FieldAccessFlags::PRIVATE,
                    name: memory.field_name.clone(),
                    descriptor: FieldType::object(self.java.classes.nio.byte_buffer),
                });
                self.class.add_field(Field {
                    id: field_id,
                    generic_signature: None,
                    constant_value: None,
                });
                memory.field = Some(field_id);
            }
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
                    function.export =
                        Some((export_name, self.settings.methods_for_function_exports));
                }

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

                _ => unimplemented!(),
            }
        }
        Ok(())
    }

    /// Generate members in the outer class corresponding to exports
    fn generate_exports(&mut self) -> Result<(), Error> {
        let class = self.class.id;
        for function in &self.functions {
            let export_name = if let Some((ExportName { name }, true)) = function.export {
                name
            } else {
                continue;
            };

            let export_descriptor = function.func_type.method_descriptor(&self.java.classes);

            // Implementation function
            let mut underlying_descriptor = export_descriptor.clone();
            underlying_descriptor
                .parameters
                .push(FieldType::object(class));

            let name: String = self.settings.renamer.rename_function(export_name);
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
                        self.translate_init_expr(&mut jvm_code, init_expr)?;
                        global.write(&mut jvm_code)?;
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
                    self.translate_init_expr(&mut jvm_code, &init_expr)?;
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
                                self.translate_init_expr(&mut jvm_code, &elem_expr)?
                            }
                        }
                        jvm_code.push_instruction(Instruction::AAStore)?;
                        jvm_code.push_instruction(Instruction::IInc(offset_var, 1))?;
                    }

                    // Kill the local variable, drop the array
                    jvm_code.push_instruction(Instruction::Pop)?;
                    jvm_code.kill_top_local(offset_var, Some(FieldType::int()))?;
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
                    self.translate_init_expr(&mut jvm_code, &init_expr)?;
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
        for function in &self.functions {
            if let Some((import_loc, import_field)) = &function.import {
                jvm_code.push_instruction(Instruction::Dup2)?;

                /* TODO: error handling for
                 *
                 *   - missing module or function in module
                 *   - method handle that doesn't have the right expected type
                 */

                // Get the module
                jvm_code.const_string(import_loc.module.to_string())?;
                jvm_code.invoke(jvm_code.java.members.util.map.get)?;
                jvm_code.push_instruction(Instruction::CheckCast(RefType::Object(
                    jvm_code.java.classes.util.map,
                )))?;

                // Get the imported handle
                jvm_code.const_string(import_loc.name.to_string())?;
                jvm_code.invoke(jvm_code.java.members.util.map.get)?;
                jvm_code.push_instruction(Instruction::CheckCast(RefType::Object(
                    self.runtime.classes.function,
                )))?;
                jvm_code.access_field(self.runtime.members.function.handle, AccessMode::Read)?;

                // Assign it to the right field
                jvm_code.access_field(*import_field, AccessMode::Write)?;
            } else {
                break;
            }
        }
        jvm_code.push_instruction(Instruction::Pop2)?;

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
            let export_name = if let Some((ExportName { name }, _)) = function.export {
                name
            } else {
                continue;
            };

            jvm_code.push_instruction(Instruction::Dup)?;
            jvm_code.const_string(export_name.to_string())?;

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

        jvm_code.push_instruction(Instruction::PutField(exports_field))?;

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
    fn translate_init_expr(
        &self,
        jvm_code: &mut CodeBuilder<'g>,
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
    pub fn result(mut self) -> Result<Vec<(BinaryName, class_file::ClassFile)>, Error> {
        self.generate_exports()?;
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
