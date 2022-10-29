use crate::error::TestError;
use wasm2jar::jvm::Name;
use wasm2jar::{jvm, translate};
use wast::QuoteWat;

use std::path::Path;

/// Functionality for translating a WAT snippet
///
/// Pulled out into a trait to facilitate mocking in unit tests
pub trait WatTranslator {
    /// Translate a module
    fn translate_module(
        &mut self,
        name: &str,
        dry_run: bool,
        module: QuoteWat,
    ) -> Result<(), TestError>;
}

pub struct Wasm2JarTranslator<P: AsRef<Path>> {
    pub output_directory: P,
}

impl<P: AsRef<Path>> WatTranslator for Wasm2JarTranslator<P> {
    fn translate_module(
        &mut self,
        name: &str,
        dry_run: bool,
        module: QuoteWat,
    ) -> Result<(), TestError> {
        // Translate the module
        let mut settings = translate::Settings::new(name, None)?;
        settings.methods_for_function_exports = false;
        let mut module = module;
        let wasm_bytes: Vec<u8> = module.encode()?;

        let translation_result =
            || -> Result<Vec<(jvm::BinaryName, jvm::class_file::ClassFile)>, translate::Error> {
                let class_graph_arenas = jvm::class_graph::ClassGraphArenas::new();
                let class_graph = jvm::class_graph::ClassGraph::new(&class_graph_arenas);
                let java = class_graph.insert_java_library_types();

                let mut translator =
                    translate::ModuleTranslator::new(settings, &class_graph, &java)?;
                let _types = translator.parse_module(&wasm_bytes)?;
                translator.result()
            };

        // TODO: catch should be removed once `wasm2jar` doesn't use `todo`
        let classes = match std::panic::catch_unwind(translation_result) {
            Ok(res) => res?,
            Err(e) => {
                let message: String = if let Some(e) = e.downcast_ref::<&'static str>() {
                    String::from(*e)
                } else if let Some(e) = e.downcast_ref::<String>() {
                    String::from(e)
                } else {
                    String::from("unknown error")
                };
                return Err(TestError::TranslationPanic(message));
            }
        };

        // Save classfiles
        if !dry_run {
            for (class_name, class) in classes {
                let class_file = self
                    .output_directory
                    .as_ref()
                    .join(format!("{}.class", class_name.as_str()));
                class
                    .save_to_path(&class_file, true)
                    .map_err(|err| translate::Error::BytecodeGen(jvm::Error::IoError(err)))?;
            }
        }

        Ok(())
    }
}
