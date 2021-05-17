mod error;
mod harness;
mod java_writer;

use clap::{App, Arg};
use error::TestOutcome;
use harness::*;
use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::exit;
use std::{fs, io};
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};
use walkdir::WalkDir;

fn main() -> io::Result<()> {
    env_logger::init();

    let matches = App::new("WAST tester for WASM to JAR converter")
        .version("0.1.0")
        .author("Alec Theriault <alec.theriault@gmail.com>")
        .about("Run WAST tests on the JVM against converted JARs")
        .arg(
            Arg::with_name("output")
                .long("output-directory")
                .value_name("DIRECTORY")
                .required(false)
                .takes_value(true)
                .help("Sets the output directory")
                .default_value("out"),
        )
        .arg(
            Arg::with_name("java")
                .long("java")
                .value_name("JAVA")
                .required(false)
                .takes_value(true)
                .help("Sets the `java` executable to use"),
        )
        .arg(
            Arg::with_name("javac")
                .long("javac")
                .value_name("JAVA_COMPILER")
                .required(false)
                .takes_value(true)
                .help("Sets the `javac` executable to use"),
        )
        .arg(
            Arg::with_name("INPUT")
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
    let mut _count_ok = 0;
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
        let outcome: TestOutcome = TestHarness::run(&output_subdirectory, &test)
            .map_or_else(TestOutcome::from, |_| TestOutcome::Ok);

        let (color, summary, message) = match outcome {
            TestOutcome::Ok => {
                _count_ok += 1;
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
        s.write(b" - ")?;
        s.set_color(ColorSpec::new().set_bold(true))?;
        s.write(test.to_string_lossy().as_bytes())?;
        s.set_color(ColorSpec::new().set_dimmed(true))?;
        s.write(b" [")?;
        s.set_color(ColorSpec::new().set_fg(Some(color)))?;
        s.write(summary)?;
        s.set_color(ColorSpec::new().set_dimmed(true))?;
        s.write(b"]\n")?;
        s.reset()?;
    }

    // Exit code
    exit(if count_fail > 0 || count_error > 0 {
        1
    } else {
        0
    })
}
