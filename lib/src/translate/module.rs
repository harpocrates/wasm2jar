use super::{CodeBuilderExts, Error, FunctionTranslator, Settings};
use crate::jvm::{
    BranchInstruction, ClassAccessFlags, ClassBuilder, ClassFile, ClassGraph, CodeBuilder,
    FieldType, InnerClass, InnerClassAccessFlags, InnerClasses, Instruction, InvokeType,
    MethodAccessFlags, MethodDescriptor, RefType, Width,
};
use crate::wasm::WasmModuleResourcesExt;
use std::cell::RefCell;
use std::rc::Rc;
use wasmparser::{
    Export, ExportSectionReader, ExternalKind, FunctionBody, Parser, Payload, Validator,
};

pub struct ModuleTranslator<'a> {
    settings: Settings,
    validator: Validator,
    #[allow(dead_code)]
    class_graph: Rc<RefCell<ClassGraph>>,
    class: ClassBuilder,
    previous_parts: Vec<ClassBuilder>,
    current_part: ClassBuilder,

    /// Populated when we visit exports
    exports: Vec<Export<'a>>,

    /// Every time we see a new function, this gets incremented
    current_func_idx: u32,
}

impl<'a> ModuleTranslator<'a> {
    pub fn new(settings: Settings) -> Result<ModuleTranslator<'a>, Error> {
        let mut validator = Validator::new();
        validator.wasm_features(settings.wasm_features);

        let mut class_graph = ClassGraph::new();
        class_graph.insert_lang_types();
        let class_graph = Rc::new(RefCell::new(class_graph));

        let class = ClassBuilder::new(
            ClassAccessFlags::PUBLIC,
            settings.output_full_class_name.clone(),
            RefType::OBJECT_NAME.to_string(),
            false,
            vec![],
            class_graph.clone(),
        )?;
        let current_part = Self::new_part(&settings, class_graph.clone(), 0)?;

        Ok(ModuleTranslator {
            settings,
            validator,
            class_graph,
            class,
            previous_parts: vec![],
            current_part,
            exports: vec![],
            current_func_idx: 0,
        })
    }

    /// Parse a full module
    pub fn parse_module(&mut self, data: &'a [u8]) -> Result<(), Error> {
        let parser = Parser::new(0);
        for payload in parser.parse_all(data) {
            let payload = payload?;
            self.process_payload(payload)?;
        }
        Ok(())
    }

    /// Construct a new inner class part
    fn new_part(
        settings: &Settings,
        class_graph: Rc<RefCell<ClassGraph>>,
        part_idx: usize,
    ) -> Result<ClassBuilder, Error> {
        let mut part = ClassBuilder::new(
            ClassAccessFlags::PUBLIC,
            format!(
                "{}${}{}",
                settings.output_full_class_name, settings.part_short_class_name, part_idx
            ),
            RefType::OBJECT_NAME.to_string(),
            false,
            vec![],
            class_graph.clone(),
        )?;

        // Add the `InnerClasses` attribute early (the piece on the parent is added at the end)
        let inner_classes: InnerClasses = {
            let mut constants = part.constants();
            let outer_class_name = constants.get_utf8(&settings.output_full_class_name)?;
            let outer_class = constants.get_class(outer_class_name)?;
            let inner_class_name = constants.get_utf8(part.class_name())?;
            let inner_class = constants.get_class(inner_class_name)?;
            let inner_name =
                constants.get_utf8(&format!("{}{}", settings.part_short_class_name, part_idx))?;
            let inner_class_attr = InnerClass {
                inner_class,
                outer_class,
                inner_name,
                access_flags: InnerClassAccessFlags::STATIC,
            };
            InnerClasses(vec![inner_class_attr])
        };
        part.add_attribute(inner_classes)?;

        Ok(part)
    }

    /// Process one payload
    pub fn process_payload(&mut self, payload: Payload<'a>) -> Result<(), Error> {
        log::trace!("Payload {:?}", payload);
        match payload {
            Payload::Version { num, range } => self.validator.version(num, &range)?,
            Payload::TypeSection(section) => self.validator.type_section(&section)?,
            Payload::ImportSection(section) => self.validator.import_section(&section)?,
            Payload::AliasSection(section) => self.validator.alias_section(&section)?,
            Payload::InstanceSection(section) => self.validator.instance_section(&section)?,
            Payload::TableSection(section) => self.validator.table_section(&section)?,
            Payload::MemorySection(section) => self.validator.memory_section(&section)?,
            Payload::EventSection(section) => self.validator.event_section(&section)?,
            Payload::GlobalSection(section) => self.validator.global_section(&section)?,
            Payload::ExportSection(section) => self.visit_exports(section)?,
            Payload::FunctionSection(section) => self.validator.function_section(&section)?,
            Payload::StartSection { func, range } => self.validator.start_section(func, &range)?,
            Payload::ElementSection(section) => self.validator.element_section(&section)?,
            Payload::DataCountSection { count, range } => {
                self.validator.data_count_section(count, &range)?
            }
            Payload::DataSection(section) => self.validator.data_section(&section)?,
            Payload::CustomSection { .. } => (),
            Payload::CodeSectionStart { count, range, .. } => {
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
    fn visit_function_body(&mut self, function_body: FunctionBody) -> Result<(), Error> {
        let validator = self.validator.code_section_entry()?;

        // Build up the type and argument
        let typ = validator
            .resources()
            .function_idx_type(self.current_func_idx)?;

        // Build up a method descriptor, which includes a trailing "WASM module" argument
        let mut method_descriptor = typ.method_descriptor();
        method_descriptor.parameters.push(FieldType::object(
            self.settings.output_full_class_name.clone(),
        ));

        let mut method_builder = self.current_part.start_method(
            MethodAccessFlags::STATIC,
            format!(
                "{}{}",
                self.settings.wasm_function_name_prefix, self.current_func_idx
            ),
            method_descriptor,
        )?;

        let mut function_translator = FunctionTranslator::new(
            typ,
            RefType::object(self.settings.output_full_class_name.clone()),
            &mut method_builder.code,
            function_body,
            validator,
        )?;
        function_translator.translate()?;

        self.current_part.finish_method(method_builder)?;
        self.current_func_idx += 1;

        Ok(())
    }

    /// Visit the exports
    ///
    /// The actual processing of the exports is in `generate_exports`, since the module resources
    /// aren't ready at this point.
    fn visit_exports(&mut self, exports: ExportSectionReader<'a>) -> Result<(), Error> {
        self.validator.export_section(&exports)?;
        for export in exports {
            self.exports.push(export?);
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

                    let mut method_builder = self.class.start_method(
                        MethodAccessFlags::PUBLIC,
                        export.field.to_string(), // TODO: renamer
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
                        self.current_part.class_name().to_owned(),
                        format!(
                            "{}{}",
                            self.settings.wasm_function_name_prefix, export.index
                        ),
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

    /// Generate a constructor
    pub fn generate_constructor(&mut self) -> Result<(), Error> {
        let mut method_builder = self.class.start_method(
            MethodAccessFlags::PUBLIC,
            String::from("<init>"),
            MethodDescriptor {
                parameters: vec![],
                return_type: None,
            },
        )?;
        method_builder
            .code
            .push_instruction(Instruction::ALoad(0))?;
        method_builder.code.invoke(RefType::OBJECT_NAME, "<init>")?;
        method_builder
            .code
            .push_branch_instruction(BranchInstruction::Return)?;

        self.class.finish_method(method_builder)?;

        Ok(())
    }

    /// Emit the final classes
    ///
    /// The first element in the output vector is the output class. The rest of the elements are
    /// the "part" inner classes.
    pub fn result(mut self) -> Result<Vec<(String, ClassFile)>, Error> {
        self.generate_exports()?;
        self.generate_constructor()?;

        // Assemble all the parts
        let mut parts = self.previous_parts;
        parts.push(self.current_part);

        // Construct the `InnerClasses` attribute
        let inner_classes: InnerClasses = {
            let mut inner_class_attrs = vec![];
            let mut constants = self.class.constants();
            let outer_class_name = constants.get_utf8(&self.settings.output_full_class_name)?;
            let outer_class = constants.get_class(outer_class_name)?;

            for (part_idx, part) in parts.iter().enumerate() {
                let inner_class_name = constants.get_utf8(part.class_name())?;
                let inner_class = constants.get_class(inner_class_name)?;
                let inner_name = constants.get_utf8(&format!(
                    "{}{}",
                    self.settings.part_short_class_name, part_idx
                ))?;
                inner_class_attrs.push(InnerClass {
                    inner_class,
                    outer_class,
                    inner_name,
                    access_flags: InnerClassAccessFlags::STATIC,
                })
            }
            InnerClasses(inner_class_attrs)
        };
        self.class.add_attribute(inner_classes)?;

        // Final results
        let mut results = vec![(self.class.class_name().to_owned(), self.class.result())];
        results.extend(
            parts
                .into_iter()
                .map(|part| (part.class_name().to_owned(), part.result())),
        );

        Ok(results)
    }
}
