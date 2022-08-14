use wasm2jar::jvm::Name;
use wasm2jar::*;

use clap::{Arg, Command};
use std::fs;
use std::process;

fn main() -> Result<(), translate::Error> {
    env_logger::init();

    let matches = Command::new("WASM to JAR converter")
        .version("0.1.0")
        .author("Alec Theriault <alec.theriault@gmail.com>")
        .about("Convert WASM modules into classes that run on a JVM")
        .arg(
            Arg::new("class")
                .long("output-class")
                .value_name("CLASS_NAME")
                .required(true)
                .takes_value(true)
                .help("Output class name (eg. `foo/bar/Baz`)"),
        )
        .arg(
            Arg::new("jar")
                .allow_invalid_utf8(true)
                .long("jar")
                .required(false)
                .takes_value(true)
                .help("Produce a `jar` output with this name (uses `jar` utility on PATH)"),
        )
        .arg(
            Arg::new("utils")
                .long("utils")
                .required(false)
                .takes_value(true)
                .help("Specify an external utility class to use"),
        )
        .arg(
            Arg::new("INPUT")
                .help("Sets the input WASM module file to use")
                .required(true)
                .index(1),
        )
        .get_matches();

    let settings = translate::Settings::new(
        matches.value_of("class").unwrap(),
        matches.value_of("utils"),
    )?;

    let class_graph_arenas = jvm::class_graph::ClassGraphArenas::new();
    let class_graph = jvm::class_graph::ClassGraph::new(&class_graph_arenas);
    let java = class_graph.insert_java_library_types();

    let wasm_file = matches.value_of("INPUT").unwrap();
    log::info!("Reading and translating '{}'", &wasm_file);
    let wasm_bytes = fs::read(&wasm_file).map_err(jvm::Error::IoError)?;
    let mut translator = translate::ModuleTranslator::new(settings, &class_graph, &java)?;
    let _types = translator.parse_module(&wasm_bytes)?;

    // Write out the results
    let mut output_files = vec![];
    for (class_name, class) in translator.result()? {
        let class_file = format!("{}.class", class_name.as_str());
        log::info!("Writing '{}'", &class_file);
        class
            .save_to_path(&class_file, true)
            .map_err(jvm::Error::IoError)?;
        output_files.push(class_file);
    }

    // Package the results in a JAR
    if let Some(jar_name) = matches.value_of_os("jar") {
        let mut command = process::Command::new("jar");
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
