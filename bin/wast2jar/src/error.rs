use std::io;
use std::process;
use wasm2jar::translate;

/// Ways a test can go wrong
#[derive(Debug)]
pub enum TestError {
    Io(io::Error),
    Wast(wast::Error),
    IncompleteHarness(&'static str),
    Translation(translate::Error),
    TranslationPanic(String),
    JavacFailed(process::Output),
    JavaFailed(process::Output),
}

impl From<io::Error> for TestError {
    fn from(err: io::Error) -> TestError {
        TestError::Io(err)
    }
}

impl From<wast::Error> for TestError {
    fn from(err: wast::Error) -> TestError {
        TestError::Wast(err)
    }
}

impl From<translate::Error> for TestError {
    fn from(err: translate::Error) -> TestError {
        TestError::Translation(err)
    }
}

pub enum TestOutcome {
    /// The test passed
    Ok,

    /// The test failed
    Fail(String),

    /// Something failed in the test infrastructure
    Error(String),
}

impl From<TestError> for TestOutcome {
    fn from(err: TestError) -> TestOutcome {
        match err {
            TestError::Io(io_err) => TestOutcome::Error(format!("IO - {:?}", io_err)),
            TestError::Wast(wast_err) => TestOutcome::Error(format!("WAST - {:?}", wast_err)),
            TestError::IncompleteHarness(msg) => TestOutcome::Error(format!("Harness - {}", msg)),
            TestError::Translation(err) => TestOutcome::Fail(format!("Translation - {:?}", err)),
            TestError::TranslationPanic(err) => {
                TestOutcome::Fail(format!("Translation panic - {:?}", err))
            }
            TestError::JavacFailed(output) => {
                let mut message = format!("Failed to compile Java harness ({})", output.status);
                if !output.stdout.is_empty() {
                    message.push('\n');
                    message.push_str(&String::from_utf8_lossy(&output.stdout));
                }
                if !output.stderr.is_empty() {
                    message.push('\n');
                    message.push_str(&String::from_utf8_lossy(&output.stderr));
                }
                TestOutcome::Fail(message)
            }
            TestError::JavaFailed(output) => {
                let mut message = format!("Failed to run Java harness ({})", output.status);
                if !output.stdout.is_empty() {
                    message.push('\n');
                    message.push_str(&String::from_utf8_lossy(&output.stdout));
                }
                if !output.stderr.is_empty() {
                    message.push('\n');
                    message.push_str(&String::from_utf8_lossy(&output.stderr));
                }
                TestOutcome::Fail(message)
            }
        }
    }
}
