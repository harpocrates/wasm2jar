use super::java_writer::JavaWriter;
use std::path::Path;
use std::{fs, io};

pub struct JavaHarness {
    /// Name of the Java harness (eg. `JavaHarness5`)
    class_name: String,

    /// Writer for emitting Java code
    writer: JavaWriter<fs::File>,

    /// Modules that are in scope for this harness, as a list of the Java class type of the module
    /// and the Java variable name for the module
    modules_in_scope: Vec<(String, String)>,

    /// Test methods generated so far. We split the harness into multiple methods to avoid
    /// exceeding the maximum method code size.
    methods_generated: Vec<String>,

    /// Number of tests in the latest method
    tests_in_latest_method: usize,

    /// Number of test methods
    test_methods_so_far: usize,

    /// Number of run methods
    run_methods_so_far: usize,
}

impl JavaHarness {
    /// Name of the boolean variable used to track if some test has failed
    pub const FAILURE_VAR_NAME: &'static str = "somethingFailed";

    /// Name of the variable used for tracking current imports
    pub const IMPORTS_VAR_NAME: &'static str = "currentImports";

    const MAX_TESTS_PER_METHOD: usize = 512;

    /// Create a new Java harness class
    pub fn new<P: AsRef<Path>>(
        class_name: String,
        class_file: P,
        modules_in_scope: Vec<(String, String)>,
    ) -> io::Result<JavaHarness> {
        let mut harness = JavaHarness {
            class_name,
            writer: JavaWriter::new(fs::File::create(&class_file)?),
            modules_in_scope,
            methods_generated: vec![],
            tests_in_latest_method: 0,
            test_methods_so_far: 0,
            run_methods_so_far: 0,
        };

        // Class pre-amble
        for import in &[
            "org.wasm2jar.*",
            "java.lang.invoke.MethodHandle",
            "java.lang.invoke.MethodHandles",
            "java.lang.invoke.MethodType",
            "java.util.Map",
        ] {
            harness
                .writer
                .inline_code_fmt(format_args!("import {import};", import = import))?;
            harness.writer.newline()?;
        }

        harness
            .writer
            .inline_code_fmt(format_args!("public class {}", &harness.class_name))?;
        harness.writer.open_curly_block()?;

        harness.writer.newline()?;
        harness.start_new_test_method()?;

        Ok(harness)
    }

    /// Name given to a test method
    fn test_method_name(idx: usize) -> String {
        format!("testMethod{}", idx)
    }

    pub fn finish_test(&mut self) -> io::Result<()> {
        self.tests_in_latest_method += 1;

        // Switch to a new test method if we've exceeded the max number of tests in this method
        if self.tests_in_latest_method > Self::MAX_TESTS_PER_METHOD {
            self.finish_current_test_method()?;
            self.writer.newline()?;
            self.start_new_test_method()?;
            self.tests_in_latest_method = 0;
        }

        Ok(())
    }

    /// Get a mutable reference to the Java code writer in order to write in another test
    ///
    /// Every time this gets called, the test counter is incremented
    pub fn writer(&mut self) -> io::Result<&mut JavaWriter<fs::File>> {
        Ok(&mut self.writer)
    }

    fn start_new_test_method(&mut self) -> io::Result<()> {
        let method_name = Self::test_method_name(self.test_methods_so_far);
        self.test_methods_so_far += 1;
        self.writer
            .inline_code_fmt(format_args!("static boolean {}(", &method_name))?;
        self.methods_generated.push(method_name);

        // Copy over the arguments
        self.writer
            .inline_code("Map<String, Map<String, Object>> ")?;
        self.writer.inline_code(Self::IMPORTS_VAR_NAME)?;
        for (mod_typ, mod_name) in &self.modules_in_scope {
            self.writer.inline_code(", ")?;
            self.writer.inline_code(mod_typ)?;
            self.writer.inline_code(" ")?;
            self.writer.inline_code(mod_name)?;
        }

        self.writer.inline_code(")")?;
        self.writer.open_curly_block()?;

        // Define the variable used to detect if there was _any_ failure
        self.writer
            .inline_code_fmt(format_args!("boolean {} = false;", Self::FAILURE_VAR_NAME))?;
        self.writer.newline()?;

        Ok(())
    }

    fn finish_current_test_method(&mut self) -> io::Result<()> {
        // Return whether there was _any_ failure
        self.writer.newline()?;
        self.writer
            .inline_code_fmt(format_args!("return {};", Self::FAILURE_VAR_NAME))?;

        self.writer.close_curly_block()?;

        Ok(())
    }

    /// Alters the list of modules in scope
    pub fn change_modules_in_scope(
        &mut self,
        modules_in_scope: Vec<(String, String)>,
    ) -> io::Result<()> {
        self.create_next_run_method()?;
        self.modules_in_scope = modules_in_scope;
        self.start_new_test_method()?;

        Ok(())
    }

    /// Create a `run{N}` method that call all of the test methods buffered up so far
    fn create_next_run_method(&mut self) -> io::Result<()> {
        self.finish_current_test_method()?;

        // Make the `run` method
        self.writer.newline()?;
        self.writer.inline_code_fmt(format_args!(
            "public static boolean run{}(Map<String, Map<String, Object>> {})",
            self.run_methods_so_far,
            Self::IMPORTS_VAR_NAME,
        ))?;
        self.writer.open_curly_block()?;
        self.writer
            .inline_code_fmt(format_args!("boolean {} = false;", Self::FAILURE_VAR_NAME))?;
        self.writer.newline()?;
        self.run_methods_so_far += 1;

        // Instantiate all of the modules in scope
        self.writer.newline()?;
        for (mod_typ, mod_name) in &self.modules_in_scope {
            self.writer.inline_code_fmt(format_args!(
                "{typ} {name} = new {typ}({imports});",
                name = mod_name,
                typ = mod_typ,
                imports = Self::IMPORTS_VAR_NAME,
            ))?;
            self.writer.newline()?;
        }

        // Call all of the test methods in order
        self.writer.newline()?;
        for test_method in std::mem::take(&mut self.methods_generated) {
            self.writer.inline_code_fmt(format_args!(
                "{} = {}(",
                Self::FAILURE_VAR_NAME,
                test_method
            ))?;

            // Pass in all the modules as arguments
            self.writer.inline_code(Self::IMPORTS_VAR_NAME)?;
            for (_, mod_name) in &self.modules_in_scope {
                self.writer.inline_code(", ")?;
                self.writer.inline_code(mod_name)?;
            }

            self.writer
                .inline_code_fmt(format_args!(") || {};", Self::FAILURE_VAR_NAME))?;
            self.writer.newline()?;
        }

        // Return whether something failed
        self.writer.newline()?;
        self.writer
            .inline_code_fmt(format_args!("return {};", Self::FAILURE_VAR_NAME))?;
        self.writer.close_curly_block()?;

        Ok(())
    }

    /// Close off the class and the undrlying writer and return the name of the harness
    pub fn close(mut self) -> io::Result<String> {
        self.create_next_run_method()?;

        // Make a main method that has an exit code corresponding to the output of `run`
        self.writer.newline()?;
        self.writer
            .inline_code("public static void main(String[] args) throws Throwable")?;
        self.writer.open_curly_block()?;

        self.writer
            .inline_code_fmt(format_args!("boolean {} = false;", Self::FAILURE_VAR_NAME))?;
        self.writer.newline()?;

        self.writer.inline_code_fmt(format_args!(
            "Map<String, Map<String, Object>> {} = new java.util.HashMap<>();",
            Self::IMPORTS_VAR_NAME
        ))?;
        self.writer.newline()?;

        // Populate spectest
        self.writer
            .inline_code("final var lookup = java.lang.invoke.MethodHandles.lookup();")?;
        self.writer.newline()?;
        self.writer
            .inline_code("final var spectest = new java.util.HashMap<String, Object>();")?;
        self.writer.newline()?;
        self.writer.inline_code(
            "spectest.put(\"print_i32\", new org.wasm2jar.Function(lookup.findVirtual(java.io.PrintStream.class, \"print\", MethodType.methodType(void.class, int.class)).bindTo(System.out)));",
        )?;
        self.writer.newline()?;
        self.writer.inline_code(
            "spectest.put(\"print_f32\", new org.wasm2jar.Function(lookup.findVirtual(java.io.PrintStream.class, \"print\", MethodType.methodType(void.class, float.class)).bindTo(System.out)));",
        )?;
        self.writer.newline()?;
        self.writer.inline_code(
            "spectest.put(\"print_f64\", new org.wasm2jar.Function(lookup.findVirtual(java.io.PrintStream.class, \"print\", MethodType.methodType(void.class, double.class)).bindTo(System.out)));",
        )?;
        self.writer.newline()?;
        self.writer.inline_code(
            "spectest.put(\"print\", new org.wasm2jar.Function(lookup.findVirtual(java.io.PrintStream.class, \"print\", MethodType.methodType(void.class, String.class)).bindTo(System.out).bindTo(\"\")));",
        )?;
        self.writer.newline()?;
        self.writer
            .inline_code("spectest.put(\"global_i32\", new org.wasm2jar.Global(666));")?;
        self.writer.newline()?;
        self.writer
            .inline_code("spectest.put(\"global_i64\", new org.wasm2jar.Global(666L));")?;
        self.writer.newline()?;
        self.writer
            .inline_code("spectest.put(\"global_f64\", new org.wasm2jar.Global(666d));")?;
        self.writer.newline()?;
        self.writer.inline_code("spectest.put(\"memory\", new org.wasm2jar.Memory(java.nio.ByteBuffer.allocate(65536)));")?;
        self.writer.newline()?;
        self.writer.inline_code(
            "spectest.put(\"table\", new org.wasm2jar.FunctionTable(new MethodHandle[10]));",
        )?;
        self.writer.newline()?;
        self.writer.inline_code_fmt(format_args!(
            "{}.put(\"spectest\", spectest);",
            Self::IMPORTS_VAR_NAME
        ))?;
        self.writer.newline()?;

        //  "print_i32_f32": console.log.bind(console),
        //  "print_f64_f64": console.log.bind(console),

        for idx in 0..self.run_methods_so_far {
            self.writer.inline_code_fmt(format_args!(
                "{failVar} = {failVar} || run{idx}({imports});",
                failVar = Self::FAILURE_VAR_NAME,
                idx = idx,
                imports = Self::IMPORTS_VAR_NAME,
            ))?;
            self.writer.newline()?;
        }

        self.writer.inline_code_fmt(format_args!(
            "System.exit({} ? 1 : 0);",
            Self::FAILURE_VAR_NAME
        ))?;
        self.writer.close_curly_block()?;

        self.writer.close_curly_block()?;
        self.writer.close()?;

        Ok(self.class_name)
    }
}
