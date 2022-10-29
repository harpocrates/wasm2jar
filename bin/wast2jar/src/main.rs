//! This module enables programmatically verifying compliance with WAST files by turning them into
//! runnable Java classes.

mod error;
mod java_harness;
mod java_string_literal;
mod java_writer;
mod wat_translator;

use crate::error::TestError;
use error::TestOutcome;
use java_harness::JavaHarness;
use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::exit;
use std::{fs, io};
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};
use walkdir::WalkDir;
use wat_translator::Wasm2JarTranslator;

fn main() -> io::Result<()> {
    use clap::{Arg, Command};
    env_logger::init();

    let matches = Command::new("WAST tester for WASM to JAR converter")
        .version("0.1.0")
        .author("Alec Theriault <alec.theriault@gmail.com>")
        .about("Run WAST tests on the JVM against converted JARs")
        .arg(
            Arg::new("output")
                .allow_invalid_utf8(true)
                .long("output-directory")
                .value_name("DIRECTORY")
                .required(false)
                .takes_value(true)
                .help("Sets the output directory")
                .default_value("out"),
        )
        .arg(
            Arg::new("java")
                .long("java")
                .value_name("JAVA")
                .required(false)
                .takes_value(true)
                .help("Sets the `java` executable to use"),
        )
        .arg(
            Arg::new("javac")
                .long("javac")
                .value_name("JAVA_COMPILER")
                .required(false)
                .takes_value(true)
                .help("Sets the `javac` executable to use"),
        )
        .arg(
            Arg::new("INPUT")
                .allow_invalid_utf8(true)
                .help("Sets the input file or folder")
                .required(true)
                .index(1),
        )
        .get_matches();

    let input_path: PathBuf = PathBuf::from(matches.value_of_os("INPUT").unwrap());
    let output_path: PathBuf = PathBuf::from(matches.value_of_os("output").unwrap());

    // Find all of the test cases
    let tests: Vec<PathBuf> = if input_path.is_file() {
        vec![input_path]
    } else {
        WalkDir::new(input_path)
            .follow_links(true)
            .into_iter()
            .filter_map(|e| e.ok())
            .map(|e| e.into_path())
            .filter(|e| e.is_file() && e.extension().map_or(false, |ex| ex == "wast"))
            .collect()
    };

    // We'll put test cases in subfolders with helpful names, but those names must not clash
    let mut test_sub_directories: HashSet<String> = HashSet::new();

    // Go through them, one at a time
    let mut count_ok = 0;
    let mut count_fail = 0;
    let mut count_error = 0;
    let stdout = StandardStream::stdout(ColorChoice::Auto);
    for test in tests {
        // Compute a (fresh) output subdirectory
        let simple_test_name: String = match test.file_stem() {
            None => String::from("unnamed"),
            Some(os_str) => os_str.to_string_lossy().into_owned(),
        };
        let output_subdirectory = if !test_sub_directories.contains(&simple_test_name) {
            simple_test_name
        } else {
            let mut counter = 1;
            loop {
                let candidate = format!("{}{}", simple_test_name, counter);
                if !test_sub_directories.contains(&candidate) {
                    break candidate;
                }
                counter += 1
            }
        };
        test_sub_directories.insert(output_subdirectory.clone());
        let output_subdirectory: &Path = &output_path.join(output_subdirectory);
        log::debug!("Test subdirectory is {:?}", output_subdirectory);

        // Make sure that directory is empty and exists
        if output_subdirectory.is_dir() {
            fs::remove_dir_all(output_subdirectory)?;
        } else if output_subdirectory.is_file() {
            fs::remove_file(output_subdirectory)?;
        }
        fs::create_dir_all(output_subdirectory)?;

        // Run the test
        let outcome: TestOutcome = run_test(&test, &output_subdirectory)
            .map_or_else(TestOutcome::from, |_| TestOutcome::Ok);

        let (color, summary, message) = match outcome {
            TestOutcome::Ok => {
                count_ok += 1;
                (Color::Green, b"OK".as_ref(), None)
            }
            TestOutcome::Fail(msg) => {
                count_fail += 1;
                (Color::Red, b"FAILED".as_ref(), Some(msg))
            }
            TestOutcome::Error(msg) => {
                count_error += 1;
                (Color::Yellow, b"ERROR".as_ref(), Some(msg))
            }
        };

        if let Some(message) = message {
            log::error!("{}", message);
        }

        // Print out the test result
        let mut s = stdout.lock();
        s.write_all(b" - ")?;
        s.set_color(ColorSpec::new().set_bold(true))?;
        s.write_all(test.to_string_lossy().as_bytes())?;
        s.set_color(ColorSpec::new().set_dimmed(true))?;
        s.write_all(b" [")?;
        s.set_color(ColorSpec::new().set_fg(Some(color)))?;
        s.write_all(summary)?;
        s.set_color(ColorSpec::new().set_dimmed(true))?;
        s.write_all(b"]\n")?;
        s.reset()?;
        s.flush()?;
    }

    // Only print a summary when there is something to summarize
    if count_ok + count_fail + count_error > 1 {
        let mut s = stdout.lock();
        s.write_all(b"\n")?;
        s.write_all(b"Summary:\n")?;
        for (color, count, message) in [
            (Color::Green, count_ok, "OK"),
            (Color::Yellow, count_error, "ERROR"),
            (Color::Red, count_fail, "FAILED"),
        ] {
            if count > 0 {
                s.write_all(b" - ")?;
                s.write_all(count.to_string().as_bytes())?;
                s.write_all(b" ")?;
                s.set_color(ColorSpec::new().set_fg(Some(color)))?;
                s.write_all(message.as_bytes())?;
                s.reset()?;
                s.write_all(b"\n")?;
                s.flush()?;
            }
        }
    }

    // Exit code
    exit(if count_fail > 0 || count_error > 0 {
        1
    } else {
        0
    })
}

/// Run a single WAST test case in the specified output directory
fn run_test(
    wast_file: impl AsRef<Path>,
    output_directory: impl AsRef<Path>,
) -> Result<(), TestError> {
    use std::process::Command;

    let wast_file = wast_file.as_ref();
    let wast_source = &fs::read_to_string(wast_file)?;
    let output_directory = output_directory.as_ref();
    let java_harness_file = output_directory.join("JavaHarness.java");

    // Construct the Java harness, translate dependent WAT snippets
    log::debug!("Starting fresh Java harness {:?}", &java_harness_file);
    let directives_count = JavaHarness::from_wast(
        wast_file.display(),
        wast_source,
        fs::File::create(&java_harness_file)?,
        Wasm2JarTranslator { output_directory },
    )?;

    // Compile and run the harness (calls out to `javac` and `java`)
    if directives_count > 0 {
        log::debug!("Compiling Java harness");
        let compile_output = Command::new("javac")
            .current_dir(output_directory)
            .arg("JavaHarness.java")
            .output()?;
        if !compile_output.status.success() {
            return Err(TestError::JavacFailed(compile_output));
        };

        log::debug!("Running Java harness");
        let run_output = Command::new("java")
            .current_dir(output_directory)
            .arg("-ea") // enable assertions
            .arg("JavaHarness")
            .output()?;
        if !run_output.status.success() {
            return Err(TestError::JavaFailed(run_output));
        }
    } else {
        log::debug!("Skipping compilation and run since there are no runtime tests");
    }

    Ok(())
}
