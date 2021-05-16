use std::boxed::Box;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;
use wasm2jar::{jvm, translate};
use wast::parser::{self, ParseBuffer};
use wast::{Id, Instruction, Module, QuoteModule, Wast, WastDirective};

pub struct TestHarness<'a> {
    /// Directory into which we write Java files
    java_directory: Box<Path>,

    /// Number of anonymous modules we've created so far
    anonymous_module_idx: usize,

    /// Java class name of latest module (this is in `foo/baz/Bar` format)
    latest_module: Option<String>,

    /// Defined Java classes
    translated_modules: HashMap<Id<'a>, String>,
}

impl<'a> TestHarness<'a> {
    pub fn new<P: AsRef<Path>>(java_directory: P) -> TestHarness<'a> {
        TestHarness {
            java_directory: java_directory.as_ref().into(),
            anonymous_module_idx: 0,
            latest_module: None,
            translated_modules: HashMap::new(),
        }
    }

    /// Visit a WAST file
    pub fn visit_wast<P: AsRef<Path>>(&mut self, wast_file: P) -> Result<(), TestError> {
        let wast_src: &'a str = Box::leak::<'a>(fs::read_to_string(wast_file)?.into_boxed_str());
        let buf: &'a ParseBuffer<'a> = Box::leak::<'a>(Box::new(ParseBuffer::new(wast_src)?));
        let Wast { directives } = parser::parse::<Wast>(buf)?;

        for directive in directives {
            self.visit_directive(directive)?;
        }

        Ok(())
    }

    fn visit_directive(&mut self, directive: WastDirective<'a>) -> Result<(), TestError> {
        match directive {
            WastDirective::Module(module) => {
                let module = QuoteModule::Module(module);
                for (class_name, class) in self.visit_module(module)? {
                    let class_file = self.java_directory.join(format!("{}.class", class_name));
                    class
                        .save_to_path(&class_file, true)
                        .map_err(|err| translate::Error::BytecodeGen(jvm::Error::IoError(err)))?;
                }
            }

            WastDirective::QuoteModule { source, .. } => {
                let module = QuoteModule::Quote(source);
                for (class_name, class) in self.visit_module(module)? {
                    let class_file = self.java_directory.join(format!("{}.class", class_name));
                    class
                        .save_to_path(&class_file, true)
                        .map_err(|err| translate::Error::BytecodeGen(jvm::Error::IoError(err)))?;
                }
            }

            WastDirective::AssertMalformed {
                module, message, ..
            } => match self.visit_module(module) {
                Err(TestError::Translation(translate::Error::WasmParser(err))) => {
                    assert_eq!(
                        message,
                        err.message(),
                        "incorrect error message for malformed module"
                    );
                }
                other => {
                    let _ = other?;
                }
            },

            WastDirective::AssertInvalid {
                module, message, ..
            } => match self.visit_module(QuoteModule::Module(module)) {
                Err(TestError::Translation(translate::Error::WasmParser(err))) => {
                    assert_eq!(
                        message,
                        err.message(),
                        "incorrect error message for invalid module"
                    );
                }
                other => {
                    let _ = other?;
                }
            },

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
            QuoteModule::Quote(bytes) => bytes.into_iter().flatten().cloned().collect(),
        };
        let mut translator = translate::ModuleTranslator::new(settings)?;
        translator.parse_module(&wasm_bytes)?;

        Ok(translator.result()?)
    }

    /// Turn a literal into a Java expression evaluating to that literal
    fn java_const(expr: &wast::Expression) -> Result<String, TestError> {
        let instrs = &expr.instrs;
        if instrs.len() != 1 {
            Err(TestError::UnexpectedWast)
        } else {
            Ok(match &instrs[0] {
                Instruction::I32Const(integer) => integer.to_string(),
                Instruction::I64Const(long) => {
                    let mut output = long.to_string();
                    output.push('L');
                    output
                }
                Instruction::F32Const(float) => f32::from_bits(float.bits).to_string(),
                Instruction::F64Const(double) => {
                    let mut output = f64::from_bits(double.bits).to_string();
                    output.push('D');
                    output
                }
                Instruction::RefNull(_) => String::from("null"),
                _ => return Err(TestError::UnexpectedWast),
            })
        }
    }
}

/// Ways a test can go wrong
pub enum TestError {
    Io(io::Error),
    Wast(wast::Error),
    Translation(translate::Error),
    UnexpectedWast,
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
