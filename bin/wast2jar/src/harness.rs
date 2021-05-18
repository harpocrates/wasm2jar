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
use wasm2jar::{jvm, translate};
use wast::parser::{self, ParseBuffer};
use wast::{Float32, Float64, Id, Module, QuoteModule, Span, Wast, WastDirective, Wat};

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

    /// Current in-process harness file (classname, file)
    latest_java_harness: Option<JavaHarness>,

    /// Java class name of latest module (this is in `foo/baz/Bar` format)
    latest_module: Option<String>,

    /// Defined Java classes
    translated_modules: HashMap<Id<'a>, String>,

    /// Last seen span
    pub latest_span: Span,
}

impl<'a> TestHarness<'a> {
    /// Run a test harness for a WAST file, operating entirely in the specified output directory
    pub fn run<P, Q>(output_directory: P, wast_file: Q) -> Result<(), TestError>
    where
        P: AsRef<Path>,
        Q: AsRef<Path>,
    {
        let wast_file = wast_file.as_ref();
        let wast_source = &fs::read_to_string(wast_file)?;
        let buf = ParseBuffer::new(wast_source)?;
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
        };

        for directive in directives {
            harness.visit_directive(directive)?;
        }
        harness.end_java_harness()?;

        Ok(())
    }

    /// Pretty-print a span
    fn pretty_span(&self, span: Span) -> String {
        let (line, col) = span.linecol_in(self.wast_source);
        format!("{}:{}:{}", &self.wast_path, line + 1, col + 1)
    }

    fn visit_directive(&mut self, directive: WastDirective<'a>) -> Result<(), TestError> {
        match directive {
            WastDirective::Module(module) => {
                self.end_java_harness()?;
                let module = QuoteModule::Module(module);
                for (class_name, class) in self.visit_module(module)? {
                    let class_file = self.output_directory.join(format!("{}.class", class_name));
                    class
                        .save_to_path(&class_file, true)
                        .map_err(|err| translate::Error::BytecodeGen(jvm::Error::IoError(err)))?;
                }
            }

            WastDirective::QuoteModule { source, .. } => {
                self.end_java_harness()?;
                let module = QuoteModule::Quote(source);
                for (class_name, class) in self.visit_module(module)? {
                    let class_file = self.output_directory.join(format!("{}.class", class_name));
                    class
                        .save_to_path(&class_file, true)
                        .map_err(|err| translate::Error::BytecodeGen(jvm::Error::IoError(err)))?;
                }
            }

            WastDirective::AssertMalformed {
                module,
                message,
                span,
            } => {
                self.end_java_harness()?;
                self.visit_module_expecting_error(module, span, message, false)?;
            }

            WastDirective::AssertInvalid {
                module,
                message,
                span,
            } => {
                self.end_java_harness()?;
                let module = QuoteModule::Module(module);
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

                if results.len() == 1 {
                    let result = &results[0];

                    let typ = Self::java_assert_type(result);
                    harness.inline_code_fmt(format_args!("{} result = ", typ))?;
                    Self::java_execute(&exec, harness)?;
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
                } else if results.len() == 0 {
                    Self::java_execute(&exec, harness)?;
                    harness.inline_code(";")?;
                } else {
                    todo!("multiple returns")
                }

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

            _ => todo!(),
        }

        Ok(())
    }

    /// Try to translate a module
    fn visit_module(
        &mut self,
        module: QuoteModule<'a>,
    ) -> Result<Vec<(String, jvm::ClassFile)>, TestError> {
        let id: Option<Id<'a>> = match &module {
            QuoteModule::Module(Module { id, .. }) => *id,
            QuoteModule::Quote(_) => None,
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
        let settings = translate::Settings::new(name, String::from(""));
        let wasm_bytes: Vec<u8> = match module {
            QuoteModule::Module(mut module) => module.encode()?,
            QuoteModule::Quote(wat_bytes) => {
                let mut wat_str = String::new();
                for wat_byte_line in wat_bytes {
                    wat_str.push_str(&String::from_utf8_lossy(wat_byte_line));
                    wat_str.push('\n');
                }
                let wat_buf = ParseBuffer::new(&wat_str)?;
                parser::parse::<Wat>(&wat_buf)?.module.encode()?
            }
        };

        let translation_result = || -> Result<Vec<(String, jvm::ClassFile)>, translate::Error> {
            let mut translator = translate::ModuleTranslator::new(settings)?;
            translator.parse_module(&wasm_bytes)?;
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

    /// Try to translate a module, expecting to get a certain error message
    fn visit_module_expecting_error(
        &mut self,
        module: QuoteModule<'a>,
        span: Span,
        expecting_message: &str,
        expecting_invalid: bool, // otherwise it` is expecting malformed
    ) -> Result<(), TestError> {
        match self.visit_module(module) {
            Err(TestError::Translation(translate::Error::WasmParser(err))) if expecting_invalid => {
                let message = err.message();
                if !message.starts_with(expecting_message) {
                    log::error!(
                        "{}: Expected invalid message {:?} but got {:?}",
                        self.pretty_span(span),
                        expecting_message,
                        message
                    );
                    return Err(TestError::InvalidMessage(message.to_owned()));
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

            let mut modules_in_scope = vec![];
            for name in self.translated_modules.values() {
                modules_in_scope.push((name.to_owned(), format!("mod_{}", name)));
            }
            if let Some(name) = self.latest_module.as_ref() {
                modules_in_scope.push((name.to_owned(), String::from("current")));
            }

            self.java_harness_idx += 1;
            log::debug!("Starting fresh Java harness {:?}", &java_harness_file);

            self.latest_java_harness = Some(JavaHarness::new(
                class_name,
                java_harness_file,
                modules_in_scope,
            )?);
        }

        Ok(self.latest_java_harness.as_mut().unwrap().writer()?)
    }

    /// Close off the latest Java harness (if there is one) and compile + run it
    fn end_java_harness(&mut self) -> Result<(), TestError> {
        if let Some(harness) = self.latest_java_harness.take() {
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

        Ok(())
    }

    /// Print a WAST execute into a Java expression
    pub fn java_execute<W: io::Write>(
        execute: &wast::WastExecute,
        java_writer: &mut JavaWriter<W>,
    ) -> io::Result<()> {
        use wast::WastExecute;

        match execute {
            WastExecute::Invoke(invoke) => Self::java_invoke(invoke, java_writer),
            WastExecute::Module(_) => todo!(),
            WastExecute::Get { module, global } => {
                let name = match module {
                    None => String::from("current"),
                    Some(id) => format!("mod_{}", id.name()),
                };
                java_writer.inline_code_fmt(format_args!(
                    "{name}.{field}",
                    name = name,
                    field = global
                ))
            }
        }
    }

    /// Print a WAST invoke into a Java method call
    pub fn java_invoke<W: io::Write>(
        invoke: &wast::WastInvoke,
        java_writer: &mut JavaWriter<W>,
    ) -> io::Result<()> {
        let name = match invoke.module {
            None => String::from("current"),
            Some(id) => format!("mod_{}", id.name()),
        };

        java_writer.inline_code_fmt(format_args!(
            "{name}.{method}(",
            name = name,
            method = invoke.name,
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
        expr: &wast::Expression,
        java_writer: &mut JavaWriter<W>,
    ) -> io::Result<()> {
        use std::num::FpCategory;
        use wast::Instruction;

        let instrs = &expr.instrs;
        assert_eq!(
            instrs.len(),
            1,
            "WAST expression has more than 1 instruction {:?}",
            instrs
        );
        match &instrs[0] {
            Instruction::I32Const(integer) => {
                java_writer.inline_code_fmt(format_args!("{}", integer))
            }
            Instruction::I64Const(long) => java_writer.inline_code_fmt(format_args!("{}L", long)),
            Instruction::F32Const(Float32 { bits }) => {
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
            Instruction::F64Const(Float64 { bits }) => {
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
            Instruction::RefNull(_) => java_writer.inline_code("null"),
            other => panic!("Unsupported WAST expression instruction {:?}", other),
        }
    }

    /// Infer the Java type of the expression being checked
    pub fn java_assert_type(assert_expr: &wast::AssertExpression) -> &'static str {
        match assert_expr {
            wast::AssertExpression::I32(_) => "int",
            wast::AssertExpression::I64(_) => "long",
            wast::AssertExpression::F32(_) => "float",
            wast::AssertExpression::F64(_) => "double",
            wast::AssertExpression::RefNull(_) => "Object",
            _ => unimplemented!(),
        }
    }

    /// Print a WAST assert prefix into a Java expression and return any closing part
    pub fn java_assert_expr<W: io::Write>(
        assert_expr: &wast::AssertExpression,
        java_writer: &mut JavaWriter<W>,
    ) -> io::Result<&'static str> {
        use wast::{AssertExpression, NanPattern};

        match assert_expr {
            AssertExpression::I32(integer) => {
                java_writer.inline_code_fmt(format_args!("{} == ", integer))?;
                Ok("")
            }
            AssertExpression::I64(long) => {
                java_writer.inline_code_fmt(format_args!("{}L == ", long))?;
                Ok("")
            }
            AssertExpression::F32(NanPattern::CanonicalNan)
            | AssertExpression::F32(NanPattern::ArithmeticNan) => {
                java_writer.inline_code("Float.isNaN(")?;
                Ok(")")
            }
            AssertExpression::F32(NanPattern::Value(Float32 { bits })) => {
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
            AssertExpression::F64(NanPattern::CanonicalNan)
            | AssertExpression::F64(NanPattern::ArithmeticNan) => {
                java_writer.inline_code("Double.isNaN(")?;
                Ok(")")
            }
            AssertExpression::F64(NanPattern::Value(Float64 { bits })) => {
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
            AssertExpression::RefNull(_) => {
                java_writer.inline_code("null == ")?;
                Ok("")
            }
            _ => todo!(),
        }
    }
}