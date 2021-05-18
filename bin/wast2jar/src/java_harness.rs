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
}

impl JavaHarness {
    /// Name of the boolean variable used to track if some test has failed
    pub const FAILURE_VAR_NAME: &'static str = "somethingFailed";

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
        };

        // Class pre-amble
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

    /// Get a mutable reference to the Java code writer in order to write in another test
    ///
    /// Every time this gets called, the test counter is incremented
    pub fn writer(&mut self) -> io::Result<&mut JavaWriter<fs::File>> {
        self.tests_in_latest_method += 1;

        // Switch to a new test method if we've exceeded the max number of tests in this method
        if self.tests_in_latest_method > Self::MAX_TESTS_PER_METHOD {
            self.finish_current_test_method()?;
            self.writer.newline()?;
            self.start_new_test_method()?;
            self.tests_in_latest_method = 0;
        }

        Ok(&mut self.writer)
    }

    fn start_new_test_method(&mut self) -> io::Result<()> {
        let method_name = Self::test_method_name(self.methods_generated.len());
        self.writer
            .inline_code_fmt(format_args!("static boolean {}(", &method_name))?;
        self.methods_generated.push(method_name);

        // Copy over the arguments
        let mut needs_comma = false;
        for (mod_typ, mod_name) in &self.modules_in_scope {
            if needs_comma {
                self.writer.inline_code(", ")?;
            } else {
                needs_comma = true;
            }
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

    /// Close off the class and the undrlying writer and return the name of the harness
    pub fn close(mut self) -> io::Result<String> {
        self.finish_current_test_method()?;

        // Make the main method
        self.writer.newline()?;
        self.writer
            .inline_code("public static void main(String[] args)")?;
        self.writer.open_curly_block()?;
        self.writer
            .inline_code_fmt(format_args!("boolean {} = false;", Self::FAILURE_VAR_NAME))?;
        self.writer.newline()?;

        // Instantiate all of the modules in scope
        self.writer.newline()?;
        for (mod_typ, mod_name) in &self.modules_in_scope {
            self.writer.inline_code_fmt(format_args!(
                "{typ} {name} = new {typ}();",
                name = mod_name,
                typ = mod_typ,
            ))?;
            self.writer.newline()?;
        }

        // Call all of the test methods in order
        self.writer.newline()?;
        for test_method in self.methods_generated {
            self.writer.inline_code_fmt(format_args!(
                "{} = {}(",
                Self::FAILURE_VAR_NAME,
                test_method
            ))?;

            // Pass in all the modules as arguments
            let mut needs_comma = false;
            for (_, mod_name) in &self.modules_in_scope {
                if needs_comma {
                    self.writer.inline_code(", ")?;
                } else {
                    needs_comma = true;
                }
                self.writer.inline_code(mod_name)?;
            }

            self.writer
                .inline_code_fmt(format_args!(") || {};", Self::FAILURE_VAR_NAME))?;
            self.writer.newline()?;
        }

        // Exit code based on whether we saw any errors
        self.writer.newline()?;
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
