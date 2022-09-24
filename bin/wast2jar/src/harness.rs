use super::java_harness::JavaHarness;
use super::java_writer::JavaWriter;
use crate::error::TestError;
use std::boxed::Box;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::panic::catch_unwind;
use std::path::Path;
use std::process::Command;
use wasm2jar::jvm::Name;
use wasm2jar::{jvm, translate};
use wast::component::Component;
use wast::core::Module;
use wast::lexer::Lexer;
use wast::parser::{self, ParseBuffer};
use wast::token::{Float32, Float64, Id, Span};
use wast::{QuoteWat, Wast, WastDirective, Wat};

pub struct TestHarness<'a> {
    /// Path of the initial WAST source (re-interpreted into a string as best as possible)
    wast_path: String,

    /// Source of the WAST to translate
    wast_source: &'a str,

    /// Directory into which we write `.java` and `.class` files
    output_directory: Box<Path>,

    /// Number of anonymous modules we've created so far
    anonymous_module_idx: usize,

    /// Number of Java harnesses we've created so far
    java_harness_idx: usize,

    /// Current in-process harness file and whether the modules in scope have changed since the
    /// last access
    latest_java_harness: Option<(JavaHarness, bool)>,

    /// Java class name of latest module (this is in `foo/baz/Bar` format)
    latest_module: Option<String>,

    /// Defined Java classes
    translated_modules: HashMap<Id<'a>, String>,

    /// Last seen span
    pub latest_span: Span,

    /// If set, each harness file gets compiled then run sequentiallly (slower, easier to look at
    /// the output for debugging)
    compile_and_run_incrementally: bool,
}

impl<'a> TestHarness<'a> {
    /// Run a test harness for a WAST file, operating entirely in the specified output directory
    pub fn run<P, Q>(
        output_directory: P,
        wast_file: Q,
        compile_and_run_incrementally: bool,
    ) -> Result<(), TestError>
    where
        P: AsRef<Path>,
        Q: AsRef<Path>,
    {
        let wast_file = wast_file.as_ref();
        let wast_source = &fs::read_to_string(wast_file)?;
        let mut lexer = Lexer::new(wast_source);
        lexer.allow_confusing_unicode(true); // For `names.wast` in the spec tests
        let buf = ParseBuffer::new_with_lexer(lexer)?;
        let Wast { directives } = parser::parse::<Wast>(&buf)?;

        let mut harness = TestHarness {
            wast_path: format!("{}", wast_file.display()),
            wast_source,
            output_directory: output_directory.as_ref().into(),
            anonymous_module_idx: 0,
            latest_module: None,
            java_harness_idx: 0,
            latest_java_harness: None,
            translated_modules: HashMap::new(),
            latest_span: Span::from_offset(0),
            compile_and_run_incrementally,
        };

        for directive in directives {
            harness.visit_directive(directive)?;
            if let Some((harness, _)) = harness.latest_java_harness.as_mut() {
                harness.finish_test()?;
            }
        }
        harness.end_java_harness(true)?;

        Ok(())
    }

    /// Pretty-print a span
    fn pretty_span(&self, span: Span) -> String {
        let (line, col) = span.linecol_in(self.wast_source);
        format!("{}:{}:{}", &self.wast_path, line + 1, col + 1)
    }

    fn visit_directive(&mut self, directive: WastDirective<'a>) -> Result<(), TestError> {
        match directive {
            WastDirective::Wat(module) => {
                self.end_java_harness(false)?;
                self.visit_module_and_save_classfiles(module)?;
            }

            WastDirective::AssertMalformed {
                module,
                message,
                span,
            } => {
                self.end_java_harness(false)?;
                self.visit_module_expecting_error(module, span, message, false)?;
            }

            WastDirective::AssertInvalid {
                module,
                message,
                span,
            } => {
                self.end_java_harness(false)?;
                self.visit_module_expecting_error(module, span, message, true)?;
            }

            WastDirective::AssertReturn {
                span,
                exec,
                results,
            } => {
                let span_str = self.pretty_span(span);

                let harness = self.get_java_writer()?;
                harness.newline()?;
                harness.inline_code("try")?;
                harness.open_curly_block()?;

                match results.as_slice() {
                    [] => {
                        self.java_execute(exec)?;
                        let harness = self.get_java_writer()?;
                        harness.inline_code(";")?;
                    }
                    [result] => {
                        let (typ, _boxed_ty, _unbox) = Self::java_assert_type(result);
                        let harness = self.get_java_writer()?;
                        harness
                            .inline_code_fmt(format_args!("{typ} result = ({typ})", typ = typ))?;
                        self.java_execute(exec)?;
                        let harness = self.get_java_writer()?;
                        harness.inline_code(";")?;
                        harness.newline()?;

                        // Check the arguments match
                        harness.inline_code("if (!(")?;
                        let closing = Self::java_assert_expr(result, harness)?;
                        harness.inline_code_fmt(format_args!("result{}))", closing))?;
                        harness.open_curly_block()?;
                        harness.inline_code_fmt(format_args!(
                            "System.out.println(\"Incorrect return at {}: found \" + result);",
                            &span_str
                        ))?;
                        harness.newline()?;
                        harness.inline_code_fmt(format_args!(
                            "{} = true;",
                            JavaHarness::FAILURE_VAR_NAME
                        ))?;
                        harness.close_curly_block()?;
                    }
                    _ => {
                        harness.inline_code("Object[] result = (Object[])")?;
                        self.java_execute(exec)?;
                        let harness = self.get_java_writer()?;
                        harness.inline_code(";")?;
                        harness.newline()?;

                        // Check the arguments match
                        for (i, result) in results.iter().enumerate() {
                            let (typ, boxed_ty, unbox) = Self::java_assert_type(result);

                            // Define a temp variable
                            harness.inline_code_fmt(format_args!(
                                "{} result{} = (({}) result[{}]){};",
                                typ, i, boxed_ty, i, unbox,
                            ))?;
                            harness.newline()?;

                            harness.inline_code("if (!(")?;
                            let closing = Self::java_assert_expr(result, harness)?;
                            harness.inline_code_fmt(format_args!("result{}{}))", i, closing))?;
                            harness.open_curly_block()?;
                            harness.inline_code_fmt(format_args!(
                                "System.out.println(\"Incorrect return #{} at {}: found \" + result{});",
                                i,
                                &span_str,
                                i,
                            ))?;
                            harness.newline()?;
                            harness.inline_code_fmt(format_args!(
                                "{} = true;",
                                JavaHarness::FAILURE_VAR_NAME
                            ))?;
                            harness.close_curly_block()?;
                        }
                    }
                }

                let harness = self.get_java_writer()?;
                harness.close_curly_block()?;
                harness.inline_code("catch (Throwable e)")?;
                harness.open_curly_block()?;
                harness
                    .inline_code_fmt(format_args!("{} = true;", JavaHarness::FAILURE_VAR_NAME))?;
                harness.newline()?;
                harness.inline_code_fmt(format_args!(
                    "System.out.println(\"Unexpected error at {}: \" + e.toString());",
                    &span_str
                ))?;
                harness.newline()?;
                harness.close_curly_block()?;
            }

            WastDirective::AssertTrap {
                span,
                exec,
                message: _,
            } => {
                let span_str = self.pretty_span(span);

                let harness = self.get_java_writer()?;
                harness.newline()?;
                harness.inline_code("try")?;
                harness.open_curly_block()?;

                self.java_execute(exec)?;
                let harness = self.get_java_writer()?;
                harness.inline_code(";")?;
                harness.newline()?;
                harness
                    .inline_code_fmt(format_args!("{} = true;", JavaHarness::FAILURE_VAR_NAME))?;
                harness.newline()?;
                harness.inline_code_fmt(format_args!(
                    "System.out.println(\"Unexpected success at {}\");",
                    &span_str
                ))?;

                harness.close_curly_block()?;

                // TODO: check message?
                harness.inline_code("catch (Throwable e)")?;
                harness.open_curly_block()?;
                harness.close_curly_block()?;
            }

            WastDirective::AssertExhaustion {
                span,
                call,
                message: _,
            } => {
                let span_str = self.pretty_span(span);

                let harness = self.get_java_writer()?;
                harness.newline()?;
                harness.inline_code("try")?;
                harness.open_curly_block()?;

                Self::java_invoke(&call, harness)?;
                harness.inline_code(";")?;
                harness.newline()?;
                harness
                    .inline_code_fmt(format_args!("{} = true;", JavaHarness::FAILURE_VAR_NAME))?;
                harness.newline()?;
                harness.inline_code_fmt(format_args!(
                    "System.out.println(\"Unexpected success at {}\");",
                    &span_str
                ))?;

                harness.close_curly_block()?;

                // TODO: check message?
                harness.inline_code("catch (StackOverflowError e)")?;
                harness.open_curly_block()?;
                harness.close_curly_block()?;

                harness.inline_code("catch (Throwable e)")?;
                harness.open_curly_block()?;
                harness
                    .inline_code_fmt(format_args!("{} = true;", JavaHarness::FAILURE_VAR_NAME))?;
                harness.newline()?;
                harness.inline_code_fmt(format_args!(
                    "System.out.println(\"Unexpected error at {}: \" + e.toString());",
                    &span_str
                ))?;
                harness.newline()?;
                harness.close_curly_block()?;
            }

            WastDirective::Invoke(invoke) => {
                let span_str = self.pretty_span(invoke.span);

                let harness = self.get_java_writer()?;
                harness.newline()?;
                harness.inline_code("try")?;
                harness.open_curly_block()?;

                Self::java_invoke(&invoke, harness)?;
                harness.inline_code(";")?;
                harness.newline()?;

                harness.close_curly_block()?;
                harness.inline_code("catch (Throwable e)")?;
                harness.open_curly_block()?;
                harness
                    .inline_code_fmt(format_args!("{} = true;", JavaHarness::FAILURE_VAR_NAME))?;
                harness.newline()?;
                harness.inline_code_fmt(format_args!(
                    "System.out.println(\"Unexpected error at {}: \" + e.toString());",
                    &span_str
                ))?;
                harness.newline()?;
                harness.close_curly_block()?;
            }

            WastDirective::AssertUnlinkable {
                span,
                module,
                message: _,
            } => {
                let span_str = self.pretty_span(span);

                let harness = self.get_java_writer()?;
                harness.newline()?;
                harness.inline_code("try")?;
                harness.open_curly_block()?;

                self.java_execute(wast::WastExecute::Wat(module))?;
                let harness = self.get_java_writer()?;
                harness.inline_code(";")?;
                harness.newline()?;
                harness
                    .inline_code_fmt(format_args!("{} = true;", JavaHarness::FAILURE_VAR_NAME))?;
                harness.newline()?;
                harness.inline_code_fmt(format_args!(
                    "System.out.println(\"Unexpected success at {}\");",
                    &span_str
                ))?;

                harness.close_curly_block()?;

                // TODO: check message?
                harness.inline_code("catch (Throwable e)")?;
                harness.open_curly_block()?;
                harness.close_curly_block()?;
            }

            WastDirective::Register { name, module, .. } => {
                let harness = self.get_java_writer()?;
                let module = match module {
                    None => String::from("current"),
                    Some(id) => format!("mod_{}", id.name()),
                };
                harness.inline_code_fmt(format_args!(
                    "{imports}.put(\"{name}\", {module}.exports);",
                    imports = JavaHarness::IMPORTS_VAR_NAME,
                    name = name,
                    module = module,
                ))?;
                harness.newline()?;
            }
            WastDirective::AssertException { .. } => {
                return Err(TestError::IncompleteHarness("assert_exception"))
            }
        }

        Ok(())
    }

    /// Try to translate a module
    fn visit_module(
        &mut self,
        module: QuoteWat<'a>,
    ) -> Result<Vec<(jvm::BinaryName, jvm::class_file::ClassFile)>, TestError> {
        let id: Option<Id<'a>> = match &module {
            QuoteWat::Wat(Wat::Module(Module { id, .. })) => *id,
            QuoteWat::Wat(Wat::Component(Component { id, .. })) => *id,
            QuoteWat::QuoteModule(_, _) => None,
            QuoteWat::QuoteComponent(_, _) => None,
        };
        let name = if let Some(id) = id {
            let name = id.name().to_owned();
            self.translated_modules.insert(id, name.clone());
            name
        } else {
            self.anonymous_module_idx += 1;
            format!("Anonymous{}", self.anonymous_module_idx)
        };
        self.latest_module = Some(name.clone());

        // Translate the module
        let mut settings = translate::Settings::new(&name, None)?;
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
        match catch_unwind(translation_result) {
            Ok(res) => Ok(res?),
            Err(e) => {
                let message: String = if let Some(e) = e.downcast_ref::<&'static str>() {
                    String::from(*e)
                } else if let Some(e) = e.downcast_ref::<String>() {
                    String::from(e)
                } else {
                    String::from("unknown error")
                };
                Err(TestError::TranslationPanic(message))
            }
        }
    }

    fn visit_module_and_save_classfiles(&mut self, module: QuoteWat<'a>) -> Result<(), TestError> {
        for (class_name, class) in self.visit_module(module)? {
            let class_file = self
                .output_directory
                .join(format!("{}.class", class_name.as_str()));
            class
                .save_to_path(&class_file, true)
                .map_err(|err| translate::Error::BytecodeGen(jvm::Error::IoError(err)))?;
        }
        Ok(())
    }

    /// Try to translate a module, expecting to get a certain error message
    fn visit_module_expecting_error(
        &mut self,
        module: QuoteWat<'a>,
        span: Span,
        expecting_message: &str,
        expecting_invalid: bool, // otherwise it` is expecting malformed
    ) -> Result<(), TestError> {
        match self.visit_module(module) {
            Err(TestError::Translation(translate::Error::WasmParser(err))) => {
                let message = err.message();
                if !message.starts_with(expecting_message) {
                    log::warn!(
                        "{}: Expected invalid message {:?} but got {:?}",
                        self.pretty_span(span),
                        expecting_message,
                        message
                    );
                }
            }
            Err(TestError::Wast(_)) if !expecting_invalid => (),
            other => {
                let _ = other?;
            }
        }

        Ok(())
    }

    /// Get (or create, insert, and return) the latest Java harness
    fn get_java_writer(&mut self) -> Result<&mut JavaWriter<fs::File>, TestError> {
        // Ensure `latest_java_harness` is populated
        if self.latest_java_harness.is_none() {
            let class_name = format!("JavaHarness{}", self.java_harness_idx);
            let java_harness_file = self.output_directory.join(format!("{}.java", class_name));

            let modules_in_scope = Self::modules_in_scope(
                self.translated_modules.values(),
                self.latest_module.as_ref(),
            );

            self.java_harness_idx += 1;
            log::debug!("Starting fresh Java harness {:?}", &java_harness_file);

            let harness = JavaHarness::new(class_name, java_harness_file, modules_in_scope)?;
            self.latest_java_harness = Some((harness, false));
        }

        let (harness, scope_has_changed) = self.latest_java_harness.as_mut().unwrap();
        if *scope_has_changed {
            let modules_in_scope = Self::modules_in_scope(
                self.translated_modules.values(),
                self.latest_module.as_ref(),
            );
            harness.change_modules_in_scope(modules_in_scope)?;
            *scope_has_changed = false;
        }
        Ok(harness.writer()?)
    }

    fn modules_in_scope<'b>(
        past: impl Iterator<Item = &'b String>,
        current: Option<&'b String>,
    ) -> Vec<(String, String)> {
        let mut modules_in_scope = vec![];
        for name in past {
            modules_in_scope.push((name.to_owned(), format!("mod_{}", name)));
        }
        if let Some(name) = current {
            modules_in_scope.push((name.to_owned(), String::from("current")));
        }
        modules_in_scope
    }

    /// Close off the latest Java harness (if there is one) and compile + run it
    fn end_java_harness(&mut self, end_of_harness: bool) -> Result<(), TestError> {
        if self.compile_and_run_incrementally || end_of_harness {
            if let Some((harness, _)) = self.latest_java_harness.take() {
                let harness_name = harness.close()?;

                log::debug!("Compiling Java harness {:?}", &harness_name);
                let compile_output = Command::new("javac")
                    .current_dir(&self.output_directory)
                    .arg(&format!("{}.java", harness_name))
                    .output()?;
                if !compile_output.status.success() {
                    return Err(TestError::JavacFailed(compile_output));
                }

                log::debug!("Running Java harness {:?}", &harness_name);
                let run_output = Command::new("java")
                    .current_dir(&self.output_directory)
                    .arg("-ea") // enable assertions
                    .arg(&harness_name)
                    .output()?;
                if !run_output.status.success() {
                    return Err(TestError::JavaFailed(run_output));
                }
            }
        } else if let Some((_, scope_has_changed)) = self.latest_java_harness.as_mut() {
            *scope_has_changed = true;
        }

        Ok(())
    }

    /// Print a WAST execute into a Java expression
    pub fn java_execute(&mut self, execute: wast::WastExecute<'a>) -> Result<(), TestError> {
        use wast::WastExecute;

        match execute {
            WastExecute::Invoke(invoke) => {
                let java_writer = self.get_java_writer()?;
                Self::java_invoke(&invoke, java_writer)?;
            }
            WastExecute::Wat(module) => {
                self.visit_module_and_save_classfiles(QuoteWat::Wat(module))?;
                let name = self.latest_module.clone().unwrap();
                self.get_java_writer()?.inline_code_fmt(format_args!(
                    "new {name}({imports})",
                    name = name,
                    imports = JavaHarness::IMPORTS_VAR_NAME,
                ))?;
            }
            WastExecute::Get { module, global } => {
                let java_writer = self.get_java_writer()?;
                let name = match module {
                    None => String::from("current"),
                    Some(id) => format!("mod_{}", id.name()),
                };
                java_writer.inline_code_fmt(format_args!(
                    "getGlobal({name}.exports, {global})",
                    name = name,
                    global = JavaHarness::escape_str(global),
                ))?;
            }
        }

        Ok(())
    }

    /// WAST invoke is transformed into invoking the method handle
    pub fn java_invoke<W: io::Write>(
        invoke: &wast::WastInvoke,
        java_writer: &mut JavaWriter<W>,
    ) -> io::Result<()> {
        let name = match invoke.module {
            None => String::from("current"),
            Some(id) => format!("mod_{}", id.name()),
        };

        java_writer.inline_code_fmt(format_args!(
            "getFunc({name}.exports, {func}).invoke(",
            name = name,
            func = JavaHarness::escape_str(invoke.name),
        ))?;
        let mut needs_comma = false;
        for arg in &invoke.args {
            if needs_comma {
                java_writer.inline_code(", ")?;
            } else {
                needs_comma = true;
            }
            Self::java_expr(arg, java_writer)?;
        }
        java_writer.inline_code(")")?;

        Ok(())
    }

    /// Print a WAST expression into a Java expression
    pub fn java_expr<W: io::Write>(
        expr: &wast::WastArg,
        java_writer: &mut JavaWriter<W>,
    ) -> io::Result<()> {
        use std::num::FpCategory;
        use wast::core::WastArgCore;
        use wast::WastArg;

        match expr {
            WastArg::Core(WastArgCore::I32(integer)) => {
                java_writer.inline_code_fmt(format_args!("{}", integer))
            }
            WastArg::Core(WastArgCore::I64(long)) => {
                java_writer.inline_code_fmt(format_args!("{}L", long))
            }
            WastArg::Core(WastArgCore::F32(Float32 { bits })) => {
                let float = f32::from_bits(*bits);
                match float.classify() {
                    FpCategory::Normal => java_writer.inline_code_fmt(format_args!("{}f", float)),
                    FpCategory::Zero => {
                        let z = if float.is_sign_negative() {
                            "-0.0f"
                        } else {
                            "0.0f"
                        };
                        java_writer.inline_code(z)
                    }
                    _ => java_writer
                        .inline_code_fmt(format_args!("Float.intBitsToFloat({:#08x})", bits)),
                }
            }
            WastArg::Core(WastArgCore::F64(Float64 { bits })) => {
                let double = f64::from_bits(*bits);
                match double.classify() {
                    FpCategory::Normal => java_writer.inline_code_fmt(format_args!("{}d", double)),
                    FpCategory::Zero => {
                        let z = if double.is_sign_negative() {
                            "-0.0d"
                        } else {
                            "0.0d"
                        };
                        java_writer.inline_code(z)
                    }
                    _ => java_writer
                        .inline_code_fmt(format_args!("Double.longBitsToDouble({:#016x}L)", bits)),
                }
            }
            WastArg::Core(WastArgCore::RefNull(_)) => java_writer.inline_code("null"),
            WastArg::Core(WastArgCore::RefExtern(idx)) => {
                java_writer.inline_code_fmt(format_args!("java.lang.Integer.valueOf({})", idx))
            }
            other => panic!("Unsupported WAST expression instruction {:?}", other),
        }
    }

    /// Infer from the assertion expression the
    ///
    ///   * expected Java type
    ///   * expected boxed variant of the type
    ///   * method to go from the boxed variant to the unboxed one
    ///
    pub fn java_assert_type(
        assert_expr: &wast::WastRet,
    ) -> (&'static str, &'static str, &'static str) {
        use wast::core::WastRetCore;
        use wast::WastRet;

        match assert_expr {
            WastRet::Core(WastRetCore::I32(_)) => ("int", "Integer", ".intValue()"),
            WastRet::Core(WastRetCore::I64(_)) => ("long", "Long", ".longValue()"),
            WastRet::Core(WastRetCore::F32(_)) => ("float", "Float", ".floatValue()"),
            WastRet::Core(WastRetCore::F64(_)) => ("double", "Double", ".doubleValue()"),
            WastRet::Core(WastRetCore::RefNull(_) | WastRetCore::RefExtern(_)) => {
                ("Object", "Object", "")
            }
            _ => unimplemented!(),
        }
    }

    /// Print a WAST assert prefix into a Java expression and return any closing part
    pub fn java_assert_expr<W: io::Write>(
        assert_expr: &wast::WastRet,
        java_writer: &mut JavaWriter<W>,
    ) -> io::Result<&'static str> {
        use wast::core::{NanPattern, WastRetCore};
        use wast::WastRet;

        match assert_expr {
            WastRet::Core(WastRetCore::I32(integer)) => {
                java_writer.inline_code_fmt(format_args!("{} == ", integer))?;
                Ok("")
            }
            WastRet::Core(WastRetCore::I64(long)) => {
                java_writer.inline_code_fmt(format_args!("{}L == ", long))?;
                Ok("")
            }
            WastRet::Core(WastRetCore::F32(
                NanPattern::CanonicalNan | NanPattern::ArithmeticNan,
            )) => {
                java_writer.inline_code("Float.isNaN(")?;
                Ok(")")
            }
            WastRet::Core(WastRetCore::F32(NanPattern::Value(Float32 { bits }))) => {
                let float = f32::from_bits(*bits);
                if float.is_normal() {
                    java_writer.inline_code_fmt(format_args!("{}f == ", float))?;
                    Ok("")
                } else {
                    java_writer.inline_code_fmt(format_args!(
                        "{:#08x} == Float.floatToRawIntBits(",
                        bits
                    ))?;
                    Ok(")")
                }
            }
            WastRet::Core(WastRetCore::F64(
                NanPattern::CanonicalNan | NanPattern::ArithmeticNan,
            )) => {
                java_writer.inline_code("Double.isNaN(")?;
                Ok(")")
            }
            WastRet::Core(WastRetCore::F64(NanPattern::Value(Float64 { bits }))) => {
                let double = f64::from_bits(*bits);
                if double.is_normal() {
                    java_writer.inline_code_fmt(format_args!("{}d == ", double))?;
                    Ok("")
                } else {
                    java_writer.inline_code_fmt(format_args!(
                        "{:#016x}L == Double.doubleToRawLongBits(",
                        bits
                    ))?;
                    Ok(")")
                }
            }
            WastRet::Core(WastRetCore::RefNull(_)) => {
                java_writer.inline_code("null == ")?;
                Ok("")
            }
            WastRet::Core(WastRetCore::RefExtern(idx)) => {
                java_writer
                    .inline_code_fmt(format_args!("java.lang.Integer.valueOf({}).equals(", idx))?;
                Ok(")")
            }
            _ => todo!(),
        }
    }
}
