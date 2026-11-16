//! Standalone native conversion between legacy DOC, DOCX and Impress documents.
use format::{FormatRegistry, atomic_write};
use std::{
    fs::File,
    io::{Seek, SeekFrom, Write},
    path::PathBuf,
};

const USAGE: &str = "impress-convert [--overwrite] INPUT OUTPUT\n\nSupported extensions: .doc, .docx, .impress\nConversion runs in-process. No office installation is required.\nThe output format is selected by its extension. Existing files require --overwrite.\nUnsupported features are reported on stderr; keep the source document.";

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut overwrite = false;
    let mut paths = Vec::new();
    for argument in std::env::args_os().skip(1) {
        match argument.to_str() {
            Some("--help" | "-h") => {
                println!("{USAGE}");
                return Ok(());
            }
            Some("--version") => {
                println!("impress-convert {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            Some("--overwrite") => overwrite = true,
            Some(value) if value.starts_with('-') => {
                return Err(format!("Unknown option: {value}\n{USAGE}"));
            }
            _ => paths.push(PathBuf::from(argument)),
        }
    }
    if paths.len() != 2 {
        return Err(USAGE.into());
    }
    let input = &paths[0];
    let output = &paths[1];
    if output.exists() && !overwrite {
        return Err("Output already exists. Choose another path or use --overwrite.".into());
    }
    let mut formats = FormatRegistry::default();
    formats.register(format_doc::DocFormat);
    formats.register(docx::DocxFormat);
    formats.register(format_native::NativeImpressFormat);
    let reader = formats
        .for_path(input)
        .ok_or("Unsupported input extension")?;
    let writer = formats
        .for_path(output)
        .ok_or("Unsupported output extension")?;
    let mut input_file = File::open(input).map_err(|error| error.to_string())?;
    let document = reader
        .import(&mut input_file)
        .map_err(|error| error.to_string())?;
    for issue in &document.compatibility {
        eprintln!("{}: {}", issue.code, issue.message);
    }
    for warning in writer.export_warnings(&document) {
        eprintln!("{warning}");
    }
    atomic_write(output, |file| {
        writer
            .export(&document, file)
            .map_err(|error| error.to_string())?;
        file.flush().map_err(|error| error.to_string())?;
        file.seek(SeekFrom::Start(0))
            .map_err(|error| error.to_string())?;
        writer
            .import(file)
            .map_err(|error| format!("Output validation failed: {error}"))?;
        Ok(())
    })?;
    println!("Converted {} to {}", input.display(), output.display());
    Ok(())
}
