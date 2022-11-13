use wasm2jar::jvm::Name;
use wasm2jar::*;

use clap::{Arg, ArgAction, Command};
use std::fs;
use std::path::PathBuf;
use std::process;

fn main() -> Result<(), translate::Error> {
    env_logger::init();

    let matches = Command::new(clap::crate_name!())
        .version(clap::crate_version!())
        .author(clap::crate_authors!())
        .about(clap::crate_description!())
        .arg(
            Arg::new("class")
                .long("output-class")
                .value_name("CLASS_NAME")
                .required(true)
                .action(ArgAction::Set)
                .help("Output class name (eg. `foo/bar/Baz`)"),
        )
        .arg(
            Arg::new("jar")
                .value_parser(clap::value_parser!(PathBuf))
                .long("jar")
                .required(false)
                .action(ArgAction::Set)
                .help("Produce a `jar` output with this name (uses `jar` utility on PATH)"),
        )
        .arg(
            Arg::new("utils")
                .long("utils")
                .required(false)
                .action(ArgAction::Set)
                .help("Specify an external utility class to use"),
        )
        .arg(
            Arg::new("INPUT")
                .value_parser(clap::value_parser!(PathBuf))
                .help("Sets the input WASM module file to use")
                .action(ArgAction::Set)
                .index(1),
        )
        .get_matches();

    let settings = translate::Settings::new(
        matches.get_one::<String>("class").unwrap(),
        matches.get_one::<String>("utils").map(|x| &**x),
    )?;

    let class_graph_arenas = jvm::class_graph::ClassGraphArenas::new();
    let class_graph = jvm::class_graph::ClassGraph::new(&class_graph_arenas);
    let java = class_graph.insert_java_library_types();

    let wasm_file: &PathBuf = matches.get_one::<PathBuf>("INPUT").unwrap();
    log::info!("Reading and translating '{}'", wasm_file.to_string_lossy());
    let wasm_bytes = fs::read(wasm_file).map_err(jvm::Error::IoError)?;
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
    if let Some(jar_name) = matches.get_one::<PathBuf>("jar") {
        let mut command = process::Command::new("jar");
        command.arg("cf").arg(jar_name);
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
