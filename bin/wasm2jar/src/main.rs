use wasm2jar::*;

use clap::{App, Arg};
use std::fs;
use std::process::Command;

fn main() -> Result<(), translate::Error> {
    env_logger::init();

    let matches = App::new("WASM to JAR converter")
        .version("0.1.0")
        .author("Alec Theriault <alec.theriault@gmail.com>")
        .about("Convert WASM modules into classes that run on a JVM")
        .arg(
            Arg::with_name("class")
                .long("output-class")
                .value_name("CLASS_NAME")
                .required(true)
                .takes_value(true)
                .help("Output class name (eg. `foo/bar/Baz`)"),
        )
        .arg(
            Arg::with_name("jar")
                .long("jar")
                .required(false)
                .takes_value(true)
                .help("Produce a `jar` output with this name (uses `jar` utility on PATH)"),
        )
        .arg(
            Arg::with_name("INPUT")
                .help("Sets the input WASM module file to use")
                .required(true)
                .index(1),
        )
        .get_matches();

    let settings = translate::Settings::new(matches.value_of("class").unwrap().to_owned());

    let wasm_file = matches.value_of("INPUT").unwrap();
    log::info!("Reading and translating '{}'", &wasm_file);
    let wasm_bytes = fs::read(&wasm_file).map_err(jvm::Error::IoError)?;
    let mut translator = translate::ModuleTranslator::new(settings)?;
    translator.parse_module(&wasm_bytes)?;

    // Write out the results
    let mut output_files = vec![];
    for (class_name, class) in translator.result()? {
        let class_file = format!("{}.class", class_name);
        log::info!("Writing '{}'", &class_file);
        class
            .save_to_path(&class_file, true)
            .map_err(jvm::Error::IoError)?;
        output_files.push(class_file);
    }

    // Package the results in a JAR
    if let Some(jar_name) = matches.value_of_os("jar") {
        let mut command = Command::new("jar");
        command.arg("cf").arg(&jar_name);
        for output_file in &output_files {
            command.arg(output_file);
        }
        if !command.status().map_err(jvm::Error::IoError)?.success() {
            log::error!("Failed to create JAR {:?}", jar_name);
        } else {
            for output_file in &output_files {
                fs::remove_file(output_file).map_err(jvm::Error::IoError)?;
            }
        }
    }

    Ok(())
}
