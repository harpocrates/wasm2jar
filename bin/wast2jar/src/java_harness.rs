//! Generate Java code to drive assertions about converted modules
//!
//! # Structure of Java harness output
//!
//! Each WAST file is translated into Java source code for invoking the translated module code and
//! making assertions about that output. The Java harness source code has the following structure:
//!
//! ```java,ignore,no_run
//! public class JavaHarness {
//!
//!   /** Every new module instantiated will use this as its import scope. This
//!     * starts out containing only the `spectest` import and then a new entry
//!     * is added to the map every time a `register` directive runs (eg.
//!     * `(register "foo" $mod)` adds `$mod.exports` to the map under the key
//!     * `"foo"`)..
//!     */
//!   final static Map<String, Map<String, Object>> imports = initialImports();
//!
//!   /** Every new module instantiated with an explicit name has its exports
//!     * added to this map. This maps the name of the module (eg. `MyMod` in
//!     * `(module $MyMod ...)`) in the WAST file to its exports.
//!     */
//!   final static Map<String, Map<String, Object>> exports = new HashMap<>();
//!
//!   /** Tracks the exports of the last module */
//!   static Map<String, Object> latestExports = new HashMap();
//!
//!   /** Has anything failed so far? */
//!   static boolean somethingFailed = 0;
//!
//!   // `testsPart1`, `testsPart2`, ... static methods defined here
//!
//!   public static void main(String[] args) throws Throwable {
//!
//!     // Test directives are chunked to not overflow the maximum method size
//!     testsPart1();
//!     testsPart2();
//!     ...
//!
//!     System.exit(somethingFailed ? 1 : 0);
//!   }
//! }
//! ```

use crate::error::TestError;
use crate::java_string_literal::JavaStringLiteral;
use crate::java_writer::JavaWriter;
use crate::wat_translator::WatTranslator;
use wasm2jar::translate;
use wast::component::Component;
use wast::core::{Module, NanPattern, WastArgCore, WastRetCore};
use wast::lexer::Lexer;
use wast::parser::{parse, ParseBuffer};
use wast::token::{Float32, Float64, Id, Span};
use wast::{QuoteWat, Wast, WastArg, WastDirective, WastExecute, WastInvoke, WastRet, Wat};

use std::fmt::Display;
use std::io;
use std::io::Write;
use std::num::FpCategory;

pub struct JavaHarness<'a, P: Display, W: Write, T: WatTranslator> {
    /// Path of the initial WAST source (re-interpreted into a string as best as possible), used
    /// for generating error messages in the harness
    wast_path: P,

    /// Source of the WAST to translate (used to pretty-print line/col offsets)
    wast_source: &'a str,

    /// Number of anonymous modules created so far
    anonymous_module_idx: usize,

    /// Writer for emitting Java code
    writer: JavaWriter<W>,

    /// Module translator
    translator: T,
}

impl<'a, P: Display, W: Write, T: WatTranslator> JavaHarness<'a, P, W, T> {
    /// Artificially constrain the number of tests in any one test method `testPartN` so as to
    /// reduce the likelihood of overflowing the maximum method size
    const MAX_TESTS_PER_METHOD: usize = 512;

    /// Construct a Java harness from a WAST file and return the number of directives that will run
    pub fn from_wast(
        wast_path: P,
        wast_source: &'a str,
        writer: W,
        translator: T,
    ) -> Result<usize, TestError> {
        let mut lexer = Lexer::new(wast_source);
        lexer.allow_confusing_unicode(true); // For `names.wast` in the spec tests
        let buf = ParseBuffer::new_with_lexer(lexer)?;
        let Wast { directives } = parse::<Wast>(&buf)?;

        let mut harness = JavaHarness {
            wast_path,
            wast_source,
            anonymous_module_idx: 0,
            writer: JavaWriter::new(writer),
            translator,
        };

        // Open the class
        harness.class_setup()?;

        // Visit the directives, packing `MAX_TESTS_PER_METHOD` directives per test method
        let mut test_methods_count = 1;
        let mut directives_count = 0;

        write!(harness.writer, "static void testMethod0()")?;
        harness.writer.open_curly_block()?;
        for directive in directives.into_iter() {
            writeln!(harness.writer)?;
            writeln!(
                harness.writer,
                "// {}",
                harness.pretty_span(directive.span())
            )?;

            if harness.visit_wast_directive(directive)? {
                directives_count += 1;

                // Close off preceding test method and open new one
                if directives_count % Self::MAX_TESTS_PER_METHOD == 0 {
                    harness.writer.close_curly_block()?;
                    harness.writer.newline()?;
                    write!(
                        harness.writer,
                        "static void testMethod{}()",
                        test_methods_count
                    )?;
                    harness.writer.open_curly_block()?;
                    test_methods_count += 1;
                }
            }
        }
        harness.writer.close_curly_block()?;

        // Write the main method and finish the class
        harness.class_main_and_close(test_methods_count)?;

        Ok(directives_count)
    }

    /// Create a new Java harness class
    fn class_setup(&mut self) -> io::Result<()> {
        // Imports
        for import in &[
            "org.wasm2jar.*",
            "java.lang.invoke.MethodHandle",
            "java.lang.invoke.MethodHandles",
            "java.lang.invoke.MethodType",
            "java.io.PrintWriter",
            "java.util.Map",
            "java.util.HashMap",
        ] {
            writeln!(self.writer, "import {import};", import = import)?;
        }

        // Start the class
        write!(self.writer, "\npublic class JavaHarness")?;
        self.writer.open_curly_block()?;
        self.writer.newline()?;

        // Static members
        for static_decl in [
            "boolean somethingFailed = false",
            "Map<String, Map<String, Object>> imports = new HashMap<>()",
            "Map<String, Map<String, Object>> exports = new HashMap<>()",
            "Map<String, Object> latestExports = new HashMap<>()",
            "PrintWriter spectestWriter",
        ] {
            writeln!(self.writer, "static {};", static_decl)?;
        }
        self.writer.newline()?;

        Ok(())
    }

    /// Close off the class and the undrlying writer and return the name of the harness
    pub fn class_main_and_close(mut self, test_methods_count: usize) -> io::Result<()> {
        // Make a main method that has an exit code corresponding to `somethingFailed`
        self.writer.newline()?;
        write!(
            self.writer,
            "public static void main(String[] args) throws Throwable"
        )?;
        self.writer.open_curly_block()?;

        // Populate spectest
        writeln!(self.writer, "final var lookup = MethodHandles.lookup();")?;
        writeln!(
            self.writer,
            "final var spectest = new HashMap<String, Object>();"
        )?;
        writeln!(
            self.writer,
            "spectestWriter = new PrintWriter(\"spectest.log\");"
        )?;
        for (name, value) in [
            ("print", "new Function(lookup.findStatic(JavaHarness.class, \"print\", MethodType.methodType(void.class)))"),
            ("print_i32", "new Function(lookup.findStatic(JavaHarness.class, \"printI32\", MethodType.methodType(void.class, int.class)))"),
            ("print_f32", "new Function(lookup.findStatic(JavaHarness.class, \"printF32\", MethodType.methodType(void.class, float.class)))"),
            ("print_f64", "new Function(lookup.findStatic(JavaHarness.class, \"printF64\", MethodType.methodType(void.class, double.class)))"),
            ("print_i32_f32", "new Function(lookup.findStatic(JavaHarness.class, \"printI32F32\", MethodType.methodType(void.class, int.class, float.class)))"),
            ("print_f64_f64", "new Function(lookup.findStatic(JavaHarness.class, \"printF64F64\", MethodType.methodType(void.class, double.class, double.class)))"),
            ("global_i32", "new org.wasm2jar.Global(666, false)"),
            ("global_i64", "new org.wasm2jar.Global(666L, false)"),
            ("global_f32", "new org.wasm2jar.Global(666f, false)"),
            ("global_f64", "new org.wasm2jar.Global(666d, false)"),
            ("memory", "new Memory(java.nio.ByteBuffer.allocate(65536))"),
            ("table", "new FunctionTable(new MethodHandle[10])"),
        ] {
            writeln!(self.writer, "spectest.put(\"{}\", {});", name, value)?;
        }
        self.writer.newline()?;
        writeln!(self.writer, "imports.put(\"spectest\", spectest);")?;
        self.writer.newline()?;

        for idx in 0..test_methods_count {
            writeln!(self.writer, "testMethod{}();", idx)?;
        }

        writeln!(self.writer, "spectestWriter.close();")?;
        writeln!(self.writer, "System.exit(somethingFailed ? 1 : 0);")?;
        self.writer.close_curly_block()?;

        // Helper method: `getFunc`
        write!(
            self.writer,
            "static MethodHandle getFunc(String module, String name)"
        )?;
        self.writer.open_curly_block()?;
        writeln!(
            self.writer,
            "Map<String, Object> exps = (module == null) ? latestExports : exports.get(module);",
        )?;
        writeln!(self.writer, "return ((Function)exps.get(name)).handle;")?;
        self.writer.close_curly_block()?;
        self.writer.newline()?;

        // Helper method: `getGlobal`
        write!(
            self.writer,
            "static Object getGlobal(String module, String name)"
        )?;
        self.writer.open_curly_block()?;
        writeln!(
            self.writer,
            "Map<String, Object> exps = (module == null) ? latestExports : exports.get(module);",
        )?;
        writeln!(
            self.writer,
            "return ((org.wasm2jar.Global)exps.get(name)).value;"
        )?;
        self.writer.close_curly_block()?;
        self.writer.newline()?;

        // Helper method: `print`
        writeln!(self.writer, "public static void print()")?;
        self.writer.open_curly_block()?;
        writeln!(self.writer, "spectestWriter.println(\"print: <no-args>\");")?;
        self.writer.close_curly_block()?;
        self.writer.newline()?;

        // Helper method: `printI32`
        writeln!(self.writer, "public static void printI32(int i)")?;
        self.writer.open_curly_block()?;
        writeln!(self.writer, "spectestWriter.println(\"print_i32: \" + i);")?;
        self.writer.close_curly_block()?;
        self.writer.newline()?;

        // Helper method: `printF32`
        writeln!(self.writer, "public static void printF32(float f)")?;
        self.writer.open_curly_block()?;
        writeln!(self.writer, "spectestWriter.println(\"print_f32: \" + f);")?;
        self.writer.close_curly_block()?;
        self.writer.newline()?;

        // Helper method: `printF64`
        writeln!(self.writer, "public static void printF64(double d)")?;
        self.writer.open_curly_block()?;
        writeln!(self.writer, "spectestWriter.println(\"print_f64: \" + d);")?;
        self.writer.close_curly_block()?;
        self.writer.newline()?;

        // Helper method: `printI32F32`
        writeln!(
            self.writer,
            "public static void printI32F32(int i, float f)"
        )?;
        self.writer.open_curly_block()?;
        writeln!(
            self.writer,
            "spectestWriter.println(\"print_i32_f32: \" + i + \" \" + f);"
        )?;
        self.writer.close_curly_block()?;
        self.writer.newline()?;

        // Helper method: `printF64F64`
        writeln!(
            self.writer,
            "public static void printF64F64(double d1, double d2)"
        )?;
        self.writer.open_curly_block()?;
        writeln!(
            self.writer,
            "spectestWriter.println(\"print_f64_f64: \" + d1 + \" \" + d2);"
        )?;
        self.writer.close_curly_block()?;
        self.writer.newline()?;

        self.writer.close_curly_block()?;
        self.writer.close()?;

        Ok(())
    }

    /// Visit a single directive and return whether it turned into anything in the Java harness
    ///
    /// This should be called when the Java writer is in a test method
    fn visit_wast_directive(&mut self, directive: WastDirective<'a>) -> Result<bool, TestError> {
        Ok(match directive {
            WastDirective::Wat(module) => {
                let (name, is_anon) = self.generate_module_name(&module);
                self.translator.translate_module(&name, false, module)?;
                writeln!(
                    self.writer,
                    "latestExports = new {name}(imports).exports;",
                    name = name,
                )?;
                if !is_anon {
                    writeln!(
                        self.writer,
                        "exports.put({name}, latestExports);",
                        name = JavaStringLiteral(&name),
                    )?;
                }
                true
            }

            WastDirective::AssertMalformed {
                module,
                message,
                span,
            } => {
                self.visit_module_expecting_error(module, span, message, false)?;
                false
            }

            WastDirective::AssertInvalid {
                module,
                message,
                span,
            } => {
                self.visit_module_expecting_error(module, span, message, true)?;
                false
            }

            WastDirective::AssertReturn {
                span,
                exec,
                results,
            } => {
                let span_str = self.pretty_span(span);

                write!(self.writer, "try")?;
                self.writer.open_curly_block()?;

                match results.as_slice() {
                    [] => {
                        self.visit_wast_execute(exec)?;
                        write!(self.writer, ";")?;
                    }
                    [result] => {
                        let (typ, _boxed_ty, _unbox) = Self::java_assert_type(result)?;
                        write!(self.writer, "{typ} result = ({typ})", typ = typ)?;
                        self.visit_wast_execute(exec)?;
                        writeln!(self.writer, ";")?;

                        // Check the arguments match
                        write!(self.writer, "if (!(")?;
                        let closing = self.visit_wast_ret(result)?;
                        write!(self.writer, "result{}))", closing)?;
                        self.writer.open_curly_block()?;
                        writeln!(
                            self.writer,
                            "System.out.println(\"Incorrect return at {}: found \" + result);",
                            &span_str
                        )?;
                        writeln!(self.writer, "somethingFailed = true;")?;
                        self.writer.close_curly_block()?;
                    }
                    _ => {
                        write!(self.writer, "Object[] result = (Object[])")?;
                        self.visit_wast_execute(exec)?;
                        writeln!(self.writer, ";")?;

                        // Check the arguments match
                        for (i, result) in results.iter().enumerate() {
                            let (typ, boxed_ty, unbox) = Self::java_assert_type(result)?;

                            // Define a temp variable
                            writeln!(
                                self.writer,
                                "{} result{} = (({}) result[{}]){};",
                                typ, i, boxed_ty, i, unbox,
                            )?;

                            write!(self.writer, "if (!(")?;
                            let closing = self.visit_wast_ret(result)?;
                            write!(self.writer, "result{}{}))", i, closing)?;
                            self.writer.open_curly_block()?;
                            writeln!(
                                self.writer,
                                "System.out.println(\"Incorrect return #{} at {}: found \" + result{});",
                                i,
                                &span_str,
                                i,
                            )?;
                            writeln!(self.writer, "somethingFailed = true;")?;
                            self.writer.close_curly_block()?;
                        }
                    }
                }

                self.writer.close_curly_block()?;
                write!(self.writer, "catch (Throwable e)")?;
                self.writer.open_curly_block()?;
                writeln!(self.writer, "somethingFailed = true;")?;
                writeln!(
                    self.writer,
                    "System.out.println(\"Unexpected error at {}: \" + e.toString());",
                    &span_str
                )?;
                self.writer.close_curly_block()?;

                true
            }

            WastDirective::AssertTrap {
                span,
                exec,
                message: _,
            } => {
                let span_str = self.pretty_span(span);

                write!(self.writer, "try")?;
                self.writer.open_curly_block()?;

                self.visit_wast_execute(exec)?;
                writeln!(self.writer, ";")?;
                writeln!(self.writer, "somethingFailed = true;")?;
                writeln!(
                    self.writer,
                    "System.out.println(\"Unexpected success at {}\");",
                    &span_str
                )?;

                self.writer.close_curly_block()?;

                // TODO: check message?
                write!(self.writer, "catch (Throwable e)")?;
                self.writer.open_curly_block()?;
                self.writer.close_curly_block()?;

                true
            }

            WastDirective::AssertExhaustion {
                span,
                call,
                message: _,
            } => {
                let span_str = self.pretty_span(span);

                write!(self.writer, "try")?;
                self.writer.open_curly_block()?;

                self.visit_wast_invoke(&call)?;
                writeln!(self.writer, ";")?;
                writeln!(self.writer, "somethingFailed = true;")?;
                writeln!(
                    self.writer,
                    "System.out.println(\"Unexpected success at {}\");",
                    &span_str
                )?;

                self.writer.close_curly_block()?;

                // TODO: check message?
                write!(self.writer, "catch (StackOverflowError e)")?;
                self.writer.open_curly_block()?;
                self.writer.close_curly_block()?;

                write!(self.writer, "catch (Throwable e)")?;
                self.writer.open_curly_block()?;
                writeln!(self.writer, "somethingFailed = true;")?;
                writeln!(
                    self.writer,
                    "System.out.println(\"Unexpected error at {}: \" + e.toString());",
                    &span_str
                )?;
                self.writer.close_curly_block()?;

                true
            }

            WastDirective::Invoke(invoke) => {
                let span_str = self.pretty_span(invoke.span);

                write!(self.writer, "try")?;
                self.writer.open_curly_block()?;

                self.visit_wast_invoke(&invoke)?;
                writeln!(self.writer, ";")?;

                self.writer.close_curly_block()?;

                write!(self.writer, "catch (Throwable e)")?;
                self.writer.open_curly_block()?;
                writeln!(self.writer, "somethingFailed = true;")?;
                writeln!(
                    self.writer,
                    "System.out.println(\"Unexpected error at {}: \" + e.toString());",
                    &span_str
                )?;
                self.writer.close_curly_block()?;

                true
            }

            WastDirective::AssertUnlinkable {
                span,
                module,
                message: _,
            } => {
                let span_str = self.pretty_span(span);

                write!(self.writer, "try")?;
                self.writer.open_curly_block()?;

                self.visit_wast_execute(WastExecute::Wat(module))?;
                writeln!(self.writer, ";")?;
                writeln!(self.writer, "somethingFailed = true;")?;
                writeln!(
                    self.writer,
                    "System.out.println(\"Unexpected success at {}\");",
                    &span_str
                )?;

                self.writer.close_curly_block()?;

                // TODO: check message?
                write!(self.writer, "catch (Throwable e)")?;
                self.writer.open_curly_block()?;
                self.writer.close_curly_block()?;

                true
            }

            WastDirective::Register { name, module, .. } => {
                let module_exports = match module {
                    None => "latestExports".to_string(),
                    Some(id) => format!(
                        "exports.get({module_name})",
                        module_name = JavaStringLiteral(id.name()),
                    ),
                };
                writeln!(
                    self.writer,
                    "imports.put({name}, {module_exports});",
                    name = JavaStringLiteral(name),
                    module_exports = module_exports,
                )?;

                true
            }
            WastDirective::AssertException { .. } => {
                return Err(TestError::IncompleteHarness("assert_exception"))
            }
        })
    }

    /// Extract the module name, if there is any
    fn wat_module_id(module: &QuoteWat<'a>) -> Option<Id<'a>> {
        match module {
            QuoteWat::Wat(Wat::Module(Module { id, .. })) => *id,
            QuoteWat::Wat(Wat::Component(Component { id, .. })) => *id,
            QuoteWat::QuoteModule(_, _) => None,
            QuoteWat::QuoteComponent(_, _) => None,
        }
    }

    /// Generate a name for the input module, return the name along with whether
    /// the name is anonymous
    fn generate_module_name(&mut self, module: &QuoteWat<'a>) -> (String, bool) {
        if let Some(id) = Self::wat_module_id(module) {
            let name = id.name().to_owned();
            (name, false)
        } else {
            self.anonymous_module_idx += 1;
            (format!("Anonymous{}", self.anonymous_module_idx), true)
        }
    }

    /// Try to translate a module, expecting to get a certain error message
    fn visit_module_expecting_error(
        &mut self,
        module: QuoteWat<'a>,
        span: Span,
        expecting_message: &str,
        expecting_invalid: bool, // otherwise it` is expecting malformed
    ) -> Result<(), TestError> {
        match self.translator.translate_module("Module", true, module) {
            Err(TestError::Translation(translate::Error::WasmParser(err))) => {
                let actual_message = err.message();
                if !actual_message.contains(expecting_message) {
                    log::warn!(
                        "{}: Expected invalid message {:?} but got {:?}",
                        self.pretty_span(span),
                        expecting_message,
                        actual_message
                    );
                }
            }
            Err(TestError::Wast(_)) if !expecting_invalid => (),
            Ok(_) => {
                return Err(TestError::TranslationPanic(format!(
                    "{}: Expected failure \"{}\" but got succeeded",
                    self.pretty_span(span),
                    expecting_message
                )))
            }
            other => {
                other?;
            }
        }

        Ok(())
    }

    /// Print a WAST execute call into an inline Java expression
    fn visit_wast_execute(&mut self, execute: WastExecute<'a>) -> Result<(), TestError> {
        match execute {
            WastExecute::Invoke(invoke) => self.visit_wast_invoke(&invoke)?,
            WastExecute::Wat(module) => {
                let module = QuoteWat::Wat(module);
                let name = self.generate_module_name(&module).0;
                self.translator.translate_module(&name, false, module)?;
                write!(self.writer, "new {name}(imports)", name = name)?;
            }
            WastExecute::Get { module, global } => {
                if let Some(id) = module {
                    write!(
                        self.writer,
                        "getGlobal({name}, {global})",
                        name = JavaStringLiteral(id.name()),
                        global = JavaStringLiteral(global),
                    )?;
                } else {
                    write!(
                        self.writer,
                        "getGlobal(null, {global})",
                        global = JavaStringLiteral(global),
                    )?;
                }
            }
        }

        Ok(())
    }

    /// Print a WAST function call into a manual `invoke` call on the Java method handle
    pub fn visit_wast_invoke(&mut self, invoke: &WastInvoke) -> Result<(), TestError> {
        if let Some(id) = invoke.module {
            write!(
                self.writer,
                "getFunc({name}, {func}).invoke(",
                name = JavaStringLiteral(id.name()),
                func = JavaStringLiteral(invoke.name),
            )?;
        } else {
            write!(
                self.writer,
                "getFunc(null, {func}).invoke(",
                func = JavaStringLiteral(invoke.name),
            )?;
        }
        let mut needs_comma = false;
        for arg in &invoke.args {
            if needs_comma {
                write!(self.writer, ", ")?;
            } else {
                needs_comma = true;
            }
            self.visit_wast_arg(arg)?;
        }
        write!(self.writer, ")")?;

        Ok(())
    }

    /// Print a WAST argument into an inline Java expression
    pub fn visit_wast_arg(&mut self, arg: &WastArg) -> Result<(), TestError> {
        Ok(match arg {
            WastArg::Core(WastArgCore::I32(integer)) => {
                write!(self.writer, "{}", integer)
            }
            WastArg::Core(WastArgCore::I64(long)) => {
                write!(self.writer, "{}L", long)
            }
            WastArg::Core(WastArgCore::F32(Float32 { bits })) => {
                let float = f32::from_bits(*bits);
                match float.classify() {
                    FpCategory::Normal => write!(self.writer, "{}f", float),
                    FpCategory::Zero => {
                        if float.is_sign_negative() {
                            write!(self.writer, "-")?;
                        }
                        write!(self.writer, "0.0f")
                    }
                    _ => write!(self.writer, "Float.intBitsToFloat({:#08x})", bits),
                }
            }
            WastArg::Core(WastArgCore::F64(Float64 { bits })) => {
                let double = f64::from_bits(*bits);
                match double.classify() {
                    FpCategory::Normal => write!(self.writer, "{}d", double),
                    FpCategory::Zero => {
                        if double.is_sign_negative() {
                            write!(self.writer, "-")?;
                        }
                        write!(self.writer, "0.0d")
                    }
                    _ => write!(self.writer, "Double.longBitsToDouble({:#016x}L)", bits),
                }
            }
            WastArg::Core(WastArgCore::RefNull(_)) => write!(self.writer, "null"),
            WastArg::Core(WastArgCore::RefExtern(idx)) => {
                write!(self.writer, "Integer.valueOf({})", idx)
            }
            WastArg::Core(WastArgCore::V128(_)) => {
                return Err(TestError::IncompleteHarness("visit_wast_arg: V128"))
            }
            WastArg::Component(_) => {
                return Err(TestError::IncompleteHarness("visit_wast_arg: Component"))
            }
        }?)
    }

    /// Print a WAST assert prefix into a Java expression and return any closing part
    fn visit_wast_ret(&mut self, ret: &WastRet) -> Result<&'static str, TestError> {
        match ret {
            WastRet::Core(WastRetCore::I32(integer)) => {
                write!(self.writer, "{} == ", integer)?;
                Ok("")
            }
            WastRet::Core(WastRetCore::I64(long)) => {
                write!(self.writer, "{}L == ", long)?;
                Ok("")
            }
            WastRet::Core(WastRetCore::F32(
                NanPattern::CanonicalNan | NanPattern::ArithmeticNan,
            )) => {
                write!(self.writer, "Float.isNaN(")?;
                Ok(")")
            }
            WastRet::Core(WastRetCore::F32(NanPattern::Value(Float32 { bits }))) => {
                let float = f32::from_bits(*bits);
                if float.is_normal() {
                    write!(self.writer, "{}f == ", float)?;
                    Ok("")
                } else {
                    write!(self.writer, "{:#x} == Float.floatToRawIntBits(", bits)?;
                    Ok(")")
                }
            }
            WastRet::Core(WastRetCore::F64(
                NanPattern::CanonicalNan | NanPattern::ArithmeticNan,
            )) => {
                write!(self.writer, "Double.isNaN(")?;
                Ok(")")
            }
            WastRet::Core(WastRetCore::F64(NanPattern::Value(Float64 { bits }))) => {
                let double = f64::from_bits(*bits);
                if double.is_normal() {
                    write!(self.writer, "{}d == ", double)?;
                    Ok("")
                } else {
                    write!(self.writer, "{:#x}L == Double.doubleToRawLongBits(", bits)?;
                    Ok(")")
                }
            }
            WastRet::Core(WastRetCore::RefNull(_)) => {
                write!(self.writer, "null == ")?;
                Ok("")
            }
            WastRet::Core(WastRetCore::RefExtern(idx)) => {
                write!(self.writer, "Integer.valueOf({}).equals(", idx)?;
                Ok(")")
            }
            WastRet::Core(WastRetCore::RefFunc(_)) => {
                Err(TestError::IncompleteHarness("visit_wast_ret: RefFunc"))
            }
            WastRet::Core(WastRetCore::V128(_)) => {
                Err(TestError::IncompleteHarness("visit_wast_ret: V128"))
            }
            WastRet::Component(_) => Err(TestError::IncompleteHarness("visit_wast_ret: Component")),
        }
    }

    /// Infer from the assertion expression the
    ///
    ///   * expected Java type
    ///   * expected boxed variant of the type
    ///   * method to go from the boxed variant to the unboxed one
    ///
    fn java_assert_type(
        assert_expr: &WastRet,
    ) -> Result<(&'static str, &'static str, &'static str), TestError> {
        match assert_expr {
            WastRet::Core(WastRetCore::I32(_)) => Ok(("int", "Integer", ".intValue()")),
            WastRet::Core(WastRetCore::I64(_)) => Ok(("long", "Long", ".longValue()")),
            WastRet::Core(WastRetCore::F32(_)) => Ok(("float", "Float", ".floatValue()")),
            WastRet::Core(WastRetCore::F64(_)) => Ok(("double", "Double", ".doubleValue()")),
            WastRet::Core(WastRetCore::RefNull(_) | WastRetCore::RefExtern(_)) => {
                Ok(("Object", "Object", ""))
            }
            WastRet::Core(WastRetCore::RefFunc(_)) => {
                Err(TestError::IncompleteHarness("java_assert_type: RefFunc"))
            }
            WastRet::Core(WastRetCore::V128(_)) => {
                Err(TestError::IncompleteHarness("java_assert_type: V128"))
            }
            WastRet::Component(_) => {
                Err(TestError::IncompleteHarness("java_assert_type: Component"))
            }
        }
    }

    /// Pretty-print a span
    fn pretty_span(&self, span: Span) -> String {
        let (line, col) = span.linecol_in(self.wast_source);
        format!("{}:{}:{}", &self.wast_path, line + 1, col + 1)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::boxed::Box;
    use wast::parser::Parse;

    struct NopTranslator;

    impl WatTranslator for NopTranslator {
        fn translate_module(
            &mut self,
            _name: &str,
            _dry_run: bool,
            _module: QuoteWat,
        ) -> Result<(), TestError> {
            Ok(())
        }
    }

    /// Generates a "renderer" for a parseable type
    ///
    /// This uses [`Box::leak`] to work around the [`Parse`] API, so be sure only to use it in
    /// tests
    fn render_helper<'a, A: Parse<'a>>(
        func: impl Fn(
            &mut JavaHarness<&'static str, &mut Vec<u8>, NopTranslator>,
            A,
        ) -> Result<(), TestError>,
    ) -> impl (Fn(&'static str) -> Result<String, TestError>) {
        move |wast_source: &'static str| {
            let mut output = vec![];
            let buf = ParseBuffer::new(wast_source)?;
            let arg = parse::<A>(Box::leak(Box::new(buf)))?;
            let mut harness = JavaHarness {
                wast_path: "test",
                wast_source,
                anonymous_module_idx: 0,
                writer: JavaWriter::new(&mut output),
                translator: NopTranslator,
            };
            func(&mut harness, arg)?;
            Ok(std::str::from_utf8(&output).unwrap().to_string())
        }
    }

    #[test]
    fn wast_directive() -> Result<(), TestError> {
        let render_wast_directive = render_helper(|harness, x| {
            harness.visit_wast_directive(x)?;
            Ok(())
        });

        assert_eq!(
            render_wast_directive(r#"module (func (export "foo") (param i32))"#)?
                .lines()
                .collect::<Vec<_>>(),
            vec![r#"latestExports = new Anonymous1(imports).exports;"#],
        );
        assert_eq!(
            render_wast_directive(r#"module $Mg (func (export "foo") (param i32))"#)?
                .lines()
                .collect::<Vec<_>>(),
            vec![
                r#"latestExports = new Mg(imports).exports;"#,
                r#"exports.put("Mg", latestExports);"#
            ],
        );
        assert_eq!(
            render_wast_directive("assert_return (invoke \"bar\" (i32.const 10) (f32.const 4.5))")?
                .lines()
                .collect::<Vec<_>>(),
            vec![
                r#"try {"#,
                r#"    getFunc(null, "bar").invoke(10, 4.5f);"#,
                r#"}"#,
                r#"catch (Throwable e) {"#,
                r#"    somethingFailed = true;"#,
                r#"    System.out.println("Unexpected error at test:1:1: " + e.toString());"#,
                r#"}"#,
            ]
        );
        assert_eq!(
            render_wast_directive(
                "assert_return (invoke \"baz\" (i32.const 10)) (i64.const 20) (f32.const 2.3)"
            )?
            .lines()
            .collect::<Vec<_>>(),
            vec![
                r#"try {"#,
                r#"    Object[] result = (Object[])getFunc(null, "baz").invoke(10);"#,
                r#"    long result0 = ((Long) result[0]).longValue();"#,
                r#"    if (!(20L == result0)) {"#,
                r#"        System.out.println("Incorrect return #0 at test:1:1: found " + result0);"#,
                r#"        somethingFailed = true;"#,
                r#"    }"#,
                r#"    float result1 = ((Float) result[1]).floatValue();"#,
                r#"    if (!(2.3f == result1)) {"#,
                r#"        System.out.println("Incorrect return #1 at test:1:1: found " + result1);"#,
                r#"        somethingFailed = true;"#,
                r#"    }"#,
                r#"}"#,
                r#"catch (Throwable e) {"#,
                r#"    somethingFailed = true;"#,
                r#"    System.out.println("Unexpected error at test:1:1: " + e.toString());"#,
                r#"}"#,
            ],
        );
        assert_eq!(
            render_wast_directive(r#"assert_trap (invoke "trapping") "trapped""#)?
                .lines()
                .collect::<Vec<_>>(),
            vec![
                r#"try {"#,
                r#"    getFunc(null, "trapping").invoke();"#,
                r#"    somethingFailed = true;"#,
                r#"    System.out.println("Unexpected success at test:1:1");"#,
                r#"}"#,
                r#"catch (Throwable e) {"#,
                r#"}"#,
            ]
        );
        assert_eq!(
            render_wast_directive(
                r#"assert_exhaustion (invoke "exhausting") "call stack exhausted""#
            )?
            .lines()
            .collect::<Vec<_>>(),
            vec![
                r#"try {"#,
                r#"    getFunc(null, "exhausting").invoke();"#,
                r#"    somethingFailed = true;"#,
                r#"    System.out.println("Unexpected success at test:1:1");"#,
                r#"}"#,
                r#"catch (StackOverflowError e) {"#,
                r#"}"#,
                r#"catch (Throwable e) {"#,
                r#"    somethingFailed = true;"#,
                r#"    System.out.println("Unexpected error at test:1:1: " + e.toString());"#,
                r#"}"#,
            ]
        );
        assert_eq!(
            render_wast_directive(r#"invoke "init" (i32.const 22)"#)?
                .lines()
                .collect::<Vec<_>>(),
            vec![
                r#"try {"#,
                r#"    getFunc(null, "init").invoke(22);"#,
                r#"}"#,
                r#"catch (Throwable e) {"#,
                r#"    somethingFailed = true;"#,
                r#"    System.out.println("Unexpected error at test:1:1: " + e.toString());"#,
                r#"}"#,
            ]
        );
        assert_eq!(
            render_wast_directive(
                r#"assert_unlinkable (module (import "m" "bar" (func (param i32)))) "unlinkable""#,
            )?
            .lines()
            .collect::<Vec<_>>(),
            vec![
                r#"try {"#,
                r#"    new Anonymous1(imports);"#,
                r#"    somethingFailed = true;"#,
                r#"    System.out.println("Unexpected success at test:1:1");"#,
                r#"}"#,
                r#"catch (Throwable e) {"#,
                r#"}"#,
            ]
        );
        assert_eq!(
            render_wast_directive(r#"register "m" $Mf"#)?
                .lines()
                .collect::<Vec<_>>(),
            vec![r#"imports.put("m", exports.get("Mf"));"#]
        );
        assert_eq!(
            render_wast_directive(r#"register "bar""#)?
                .lines()
                .collect::<Vec<_>>(),
            vec![r#"imports.put("bar", latestExports);"#]
        );

        Ok(())
    }

    #[test]
    fn wast_execute() -> Result<(), TestError> {
        let render_wast_execute = render_helper(|harness, x| harness.visit_wast_execute(x));

        assert_eq!(
            render_wast_execute(r#"invoke "foo""#)?,
            r#"getFunc(null, "foo").invoke()"#
        );
        assert_eq!(
            render_wast_execute(r#"invoke $MyMod "baz" (f32.const 42.1) (i64.const 7)"#)?,
            r#"getFunc("MyMod", "baz").invoke(42.1f, 7L)"#
        );
        assert_eq!(
            render_wast_execute(r#"module (func $main (unreachable)) (start $main)"#)?,
            r#"new Anonymous1(imports)"#
        );
        assert_eq!(
            render_wast_execute(r#"module $Mg (func $main (unreachable)) (start $main)"#)?,
            r#"new Mg(imports)"#
        );
        assert_eq!(
            render_wast_execute(r#"get "foo""#)?,
            r#"getGlobal(null, "foo")"#
        );
        assert_eq!(
            render_wast_execute(r#"get $Mg "foo""#)?,
            r#"getGlobal("Mg", "foo")"#
        );

        Ok(())
    }

    #[test]
    fn wast_invoke() -> Result<(), TestError> {
        let render_wast_invoke = render_helper(|harness, x| harness.visit_wast_invoke(&x));

        assert_eq!(
            render_wast_invoke(r#"invoke "foo""#)?,
            r#"getFunc(null, "foo").invoke()"#
        );
        assert_eq!(
            render_wast_invoke(r#"invoke "bar" (i32.const 42)"#)?,
            r#"getFunc(null, "bar").invoke(42)"#
        );
        assert_eq!(
            render_wast_invoke(r#"invoke "baz" (f32.const 42.1) (i64.const 7)"#)?,
            r#"getFunc(null, "baz").invoke(42.1f, 7L)"#
        );
        assert_eq!(
            render_wast_invoke(r#"invoke $MyMod "foo""#)?,
            r#"getFunc("MyMod", "foo").invoke()"#
        );
        assert_eq!(
            render_wast_invoke(r#"invoke $MyMod "bar" (i32.const 42)"#)?,
            r#"getFunc("MyMod", "bar").invoke(42)"#
        );
        assert_eq!(
            render_wast_invoke(r#"invoke $MyMod "baz" (f32.const 42.1) (i64.const 7)"#)?,
            r#"getFunc("MyMod", "baz").invoke(42.1f, 7L)"#
        );

        Ok(())
    }

    #[test]
    fn wast_arg() -> Result<(), TestError> {
        let render_wast_arg = render_helper(|harness, x| harness.visit_wast_arg(&x));

        assert_eq!(render_wast_arg("i32.const 42")?, "42");
        assert_eq!(render_wast_arg("i64.const 42")?, "42L");
        assert_eq!(render_wast_arg("f32.const 42.1")?, "42.1f");
        assert_eq!(render_wast_arg("f64.const 42.1")?, "42.1d");
        assert_eq!(render_wast_arg("f32.const -42.1")?, "-42.1f");
        assert_eq!(render_wast_arg("f64.const -42.1")?, "-42.1d");

        assert_eq!(render_wast_arg("f32.const 0.0")?, "0.0f");
        assert_eq!(render_wast_arg("f32.const -0.0")?, "-0.0f");
        assert_eq!(render_wast_arg("f64.const 0.0")?, "0.0d");
        assert_eq!(render_wast_arg("f64.const -0.0")?, "-0.0d");

        assert_eq!(
            render_wast_arg("f32.const inf")?,
            "Float.intBitsToFloat(0x7f800000)"
        );
        assert_eq!(
            render_wast_arg("f32.const -inf")?,
            "Float.intBitsToFloat(0xff800000)"
        );
        assert_eq!(
            render_wast_arg("f64.const inf")?,
            "Double.longBitsToDouble(0x7ff0000000000000L)"
        );
        assert_eq!(
            render_wast_arg("f64.const -inf")?,
            "Double.longBitsToDouble(0xfff0000000000000L)"
        );

        assert_eq!(
            render_wast_arg("f32.const nan:0x200000")?,
            "Float.intBitsToFloat(0x7fa00000)"
        );
        assert_eq!(
            render_wast_arg("f64.const nan:0x4000000000000")?,
            "Double.longBitsToDouble(0x7ff4000000000000L)",
        );

        assert_eq!(render_wast_arg("ref.null func")?, "null");
        assert_eq!(render_wast_arg("ref.null extern")?, "null");

        assert_eq!(render_wast_arg("ref.extern 42")?, "Integer.valueOf(42)");
        assert_eq!(render_wast_arg("ref.extern 21")?, "Integer.valueOf(21)");

        Ok(())
    }

    #[test]
    fn wast_ret() -> Result<(), TestError> {
        let render_wast_ret = render_helper(|harness, x| -> Result<(), TestError> {
            let closing = harness.visit_wast_ret(&x)?;
            write!(harness.writer, "x{}", closing)?;
            Ok(())
        });

        assert_eq!(render_wast_ret("i32.const 42")?, "42 == x");
        assert_eq!(render_wast_ret("i64.const 42")?, "42L == x");
        assert_eq!(render_wast_ret("f32.const 42.1")?, "42.1f == x");
        assert_eq!(render_wast_ret("f64.const 42.1")?, "42.1d == x");
        assert_eq!(render_wast_ret("f32.const -42.1")?, "-42.1f == x");
        assert_eq!(render_wast_ret("f64.const -42.1")?, "-42.1d == x");

        assert_eq!(
            render_wast_ret("f32.const 0.0")?,
            "0x0 == Float.floatToRawIntBits(x)"
        );
        assert_eq!(
            render_wast_ret("f32.const -0.0")?,
            "0x80000000 == Float.floatToRawIntBits(x)"
        );
        assert_eq!(
            render_wast_ret("f64.const 0.0")?,
            "0x0L == Double.doubleToRawLongBits(x)"
        );
        assert_eq!(
            render_wast_ret("f64.const -0.0")?,
            "0x8000000000000000L == Double.doubleToRawLongBits(x)"
        );

        assert_eq!(
            render_wast_ret("f32.const inf")?,
            "0x7f800000 == Float.floatToRawIntBits(x)"
        );
        assert_eq!(
            render_wast_ret("f32.const -inf")?,
            "0xff800000 == Float.floatToRawIntBits(x)"
        );
        assert_eq!(
            render_wast_ret("f64.const inf")?,
            "0x7ff0000000000000L == Double.doubleToRawLongBits(x)"
        );
        assert_eq!(
            render_wast_ret("f64.const -inf")?,
            "0xfff0000000000000L == Double.doubleToRawLongBits(x)"
        );

        assert_eq!(
            render_wast_ret("f32.const nan:canonical")?,
            "Float.isNaN(x)"
        );
        assert_eq!(
            render_wast_ret("f32.const nan:arithmetic")?,
            "Float.isNaN(x)"
        );
        assert_eq!(
            render_wast_ret("f64.const nan:canonical")?,
            "Double.isNaN(x)"
        );
        assert_eq!(
            render_wast_ret("f64.const nan:arithmetic")?,
            "Double.isNaN(x)"
        );

        assert_eq!(
            render_wast_ret("f32.const nan:0x200000")?,
            "0x7fa00000 == Float.floatToRawIntBits(x)"
        );
        assert_eq!(
            render_wast_ret("f64.const nan:0x4000000000000")?,
            "0x7ff4000000000000L == Double.doubleToRawLongBits(x)",
        );

        assert_eq!(render_wast_ret("ref.null func")?, "null == x");
        assert_eq!(render_wast_ret("ref.null extern")?, "null == x");

        assert_eq!(
            render_wast_ret("ref.extern 42")?,
            "Integer.valueOf(42).equals(x)"
        );
        assert_eq!(
            render_wast_ret("ref.extern 21")?,
            "Integer.valueOf(21).equals(x)"
        );

        Ok(())
    }
}
