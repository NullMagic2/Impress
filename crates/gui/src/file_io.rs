use model::Document;
use format::FormatRegistry;
use std::{fs::OpenOptions, path::{Path, PathBuf}};

/// Keep Open's aggregate filter separate from Save's concrete format choices.
/// The zero-based indices below always refer to SAVE_FORMATS, never Open's list.
#[derive(Clone, Copy, Debug)]
struct SaveFormat { label: &'static str, extension: &'static str }
const SAVE_FORMATS: [SaveFormat; 5] = [
    SaveFormat { label: "Impress documents", extension: "impress" },
    SaveFormat { label: "Microsoft Word documents", extension: "docx" },
    SaveFormat { label: "Typst documents", extension: "typ" },
    SaveFormat { label: "Rich Text Format", extension: "rtf" },
    SaveFormat { label: "Word 97–2003 documents", extension: "doc" },
];

pub fn document_save_wildcard() -> String {
    SAVE_FORMATS.iter().map(|f| format!("{} (*.{})|*.{}", f.label, f.extension, f.extension))
        .collect::<Vec<_>>().join("|")
}

pub fn document_open_wildcard() -> String {
    let patterns = SAVE_FORMATS.iter().map(|f| format!("*.{}", f.extension))
        .collect::<Vec<_>>().join(";");
    // PDF is read only: opening one converts it into a new, untitled document.
    format!("All supported documents ({patterns};*.pdf)|{patterns};*.pdf|{}|PDF document — converted on opening (*.pdf)|*.pdf|All files (*.*)|*.*", document_save_wildcard())
}

pub fn save_filter_index(path: Option<&Path>) -> i32 {
    let extension = path.and_then(Path::extension).and_then(|e| e.to_str());
    extension.and_then(|ext| SAVE_FORMATS.iter().position(|f| ext.eq_ignore_ascii_case(f.extension)))
        .unwrap_or(0) as i32
}

/// File managers normally pass a native path, but may supply a local file URI.
/// Keep literal percent signs and non-Unicode bytes in native paths intact.
pub fn launch_path(arg: std::ffi::OsString) -> Result<PathBuf, String> {
    if let Some(uri) = arg.to_str().filter(|s| s.get(..7).is_some_and(|p| p.eq_ignore_ascii_case("file://"))) {
        return url::Url::parse(uri).ok().and_then(|url| url.to_file_path().ok())
            .ok_or_else(|| format!("Could not open the file location {uri}"));
    }
    Ok(PathBuf::from(arg))
}

pub fn load_document(path: &Path, formats: &FormatRegistry) -> Result<Document, String> {
    let format = formats.for_path(path)
        .ok_or_else(|| format!("No importer is registered for {}", path.display()))?;
    if !format.capabilities().import {
        return Err(format!("{} cannot be imported", format.display_name()));
    }
    let mut file = OpenOptions::new().read(true).open(path)
        .map_err(|e| format!("Could not open {}: {e}", path.display()))?;
    format.import(&mut file).map_err(|e| e.to_string())
}

pub fn save_document(path: &Path, document: &Document, formats: &FormatRegistry) -> Result<(), String> {
    let format = formats.for_path(path)
        .ok_or_else(|| format!("No exporter is registered for {}", path.display()))?;
    if !format.capabilities().export {
        return Err(format!("{} cannot be exported", format.display_name()));
    }
    format::atomic_write(path, |file| {
        use std::io::{Seek, SeekFrom, Write};
        format.export(document, file).map_err(|e| e.to_string())?;
        file.flush().map_err(|e| e.to_string())?;
        if format.capabilities().import {
            file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
            format.import(file).map_err(|e| format!("Export validation failed: {e}"))?;
        }
        Ok(())
    })
}

/// Font collection belongs to explicit saves, not recovery snapshots.
pub fn save_with_font_embedding(path:&Path,document:&Document,formats:&FormatRegistry,embed:bool)->Result<(),String> {
    let mut snapshot=document.clone();
    if embed {typography::embed_document_fonts(&mut snapshot)?;}else{typography::remove_embedded_fonts(&mut snapshot);}
    save_document(path,&snapshot,formats)
}

/// The explicitly selected Save format owns the suffix. Keeping an old .impress
/// suffix while selecting Word would otherwise dispatch the native exporter.
/// The caller must confirm overwrite of the FINAL path if this changes it.
pub fn ensure_extension_for_filter(path: &Path, filter_index: i32) -> PathBuf {
    let extension = usize::try_from(filter_index).ok().and_then(|i| SAVE_FORMATS.get(i))
        .unwrap_or(&SAVE_FORMATS[0]).extension;
    if path.extension().and_then(|e| e.to_str()).is_some_and(|ext| ext.eq_ignore_ascii_case(extension)) {
        path.to_path_buf()
    } else {
        path.with_extension(extension)
    }
}

#[cfg(test)] #[path = "../tests/unit/file_io.rs"] mod tests;
