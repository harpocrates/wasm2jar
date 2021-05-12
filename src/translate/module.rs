use super::{Error, FunctionTranslator, Settings};
use crate::jvm::{
    ClassAccessFlags, ClassBuilder, ClassFile, ClassGraph, FieldType, MethodAccessFlags, NestHost,
    NestMembers, RefType,
};
use crate::wasm::WasmModuleResourcesExt;
use std::cell::RefCell;
use std::rc::Rc;
use wasmparser::{ExportSectionReader, FunctionBody, Payload, Validator};

pub struct ModuleTranslator {
    settings: Settings,
    validator: Validator,
    class_graph: Rc<RefCell<ClassGraph>>,
    class: ClassBuilder,
    previous_parts: Vec<ClassBuilder>,
    current_part: ClassBuilder,

    /// Every time we see a new function, this gets incremented
    current_func_idx: u32,
}

impl ModuleTranslator {
    pub fn new(settings: Settings) -> Result<ModuleTranslator, Error> {
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
        let current_part = Self::new_part(&settings, class_graph.clone())?;

        Ok(ModuleTranslator {
            settings,
            validator,
            class_graph,
            class,
            previous_parts: vec![],
            current_part,
            current_func_idx: 0,
        })
    }

    /// Construct a new inner class part
    fn new_part(
        settings: &Settings,
        class_graph: Rc<RefCell<ClassGraph>>,
    ) -> Result<ClassBuilder, Error> {
        let mut part = ClassBuilder::new(
            ClassAccessFlags::PUBLIC,
            format!(
                "{}${}0",
                settings.output_full_class_name, settings.part_short_class_name
            ),
            RefType::OBJECT_NAME.to_string(),
            false,
            vec![],
            class_graph.clone(),
        )?;

        // Add the nest host attribute early (the nest members on the parent is added at the end)
        let nest_host: NestHost = {
            let mut constants = part.constants();
            let outer_class_name = constants.get_utf8(&settings.output_full_class_name)?;
            let outer_class = constants.get_class(outer_class_name)?;
            NestHost(outer_class)
        };
        part.add_attribute(nest_host)?;

        Ok(part)
    }

    /// Process one payload
    pub fn process_payload<'a>(&mut self, payload: Payload<'a>) -> Result<(), Error> {
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

        let jvm_locals_starting_offset = method_descriptor.parameter_length(false);
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
            &mut method_builder.code,
            jvm_locals_starting_offset,
            function_body,
            validator,
        )?;
        function_translator.translate()?;

        self.current_part.finish_method(method_builder)?;
        self.current_func_idx += 1;

        Ok(())
    }

    /// Visit the exports
    fn visit_exports(&mut self, exports: ExportSectionReader) -> Result<(), Error> {
        self.validator.export_section(&exports)?;

        //   for export in exports {
        //       let export = export?;

        //       match export.kind {
        //           ExternalKind::Function => {

        //               self.class.start_method(
        //                   MethodAccessFlags::PUBLIC,
        //                   export.field.to_string(), // TODO: renamer
        //                   self.validator.

        //           }
        //           _ => todo!(),
        //       }

        //   }

        Ok(())
    }

    /// Emit the final classes
    ///
    /// The first element in the output vector is the output class. The rest of the elements are
    /// the "part" inner classes.
    pub fn result(mut self) -> Result<Vec<ClassFile>, Error> {
        // Assemble all the parts
        let mut parts = self.previous_parts;
        parts.push(self.current_part);

        // Construct the `NestMembers` attribute
        let nest_members: NestMembers = {
            let mut constants = self.class.constants();
            let mut nest_members = vec![];
            for part in &parts {
                let part_name = constants.get_utf8(part.class_name().to_string())?;
                let part_class = constants.get_class(part_name)?;
                nest_members.push(part_class);
            }
            NestMembers(nest_members)
        };
        self.class.add_attribute(nest_members)?;

        // Final results
        let mut results = vec![self.class.result()];
        results.extend(parts.into_iter().map(|part| part.result()));

        Ok(results)
    }
}
