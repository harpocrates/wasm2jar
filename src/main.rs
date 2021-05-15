use wasm2jar::*;

use clap::{App, Arg};
use std::fs;

fn main() -> Result<(), translate::Error> {
    env_logger::init();

    let matches = App::new("WASM to JAR converter")
        .version("0.1.0")
        .author("Alec Theriault <alec.theriault@gmail.com>")
        .about("Converts WebAssembly modules into classes that run on a JVM")
        .arg(
            Arg::with_name("output class")
                .long("output-class")
                .value_name("CLASS_NAME")
                .required(true)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("INPUT")
                .help("Sets the input WASM module file to use")
                .required(true)
                .index(1),
        )
        .get_matches();

    let settings = translate::Settings::new(
        matches.value_of("output class").unwrap().to_owned(),
        String::from(""),
    );

    let wasm_file = matches.value_of("INPUT").unwrap();
    log::info!("Reading and translating '{}'", &wasm_file);
    let wasm_bytes = fs::read(&wasm_file).map_err(jvm::Error::IoError)?;
    let mut translator = translate::ModuleTranslator::new(settings)?;
    translator.parse_module(&wasm_bytes)?;

    // Write out the results
    for (class_name, class) in translator.result()? {
        let class_file = format!("{}.class", class_name);
        log::info!("Writing '{}'", class_file);
        class
            .save_to_path(&class_file, true)
            .map_err(jvm::Error::IoError)?;
    }

    Ok(())
}
