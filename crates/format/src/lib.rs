mod atomic;
mod limits;
pub use limits::*;
pub use atomic::atomic_write;

use model::Document;
use std::{collections::BTreeMap, io::{Read, Seek, Write}, path::Path, sync::Arc};
use thiserror::Error;

pub trait ReadSeek: Read + Seek {}
impl<T: Read + Seek> ReadSeek for T {}
pub trait WriteSeek: Write + Seek {}
impl<T: Write + Seek> WriteSeek for T {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormatCapabilities {
    pub import: bool,
    pub export: bool,
    pub preserves_unknown_data: bool,
    pub native_equations: bool,
}

#[derive(Debug, Error)]
pub enum FormatError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid {format} document: {message}")]
    Invalid { format: &'static str, message: String },
    #[error("unsupported {format} feature: {feature}")]
    Unsupported { format: &'static str, feature: String },
    #[error("serialization error: {0}")]
    Serialization(String),
}

pub type FormatResult<T> = Result<T, FormatError>;

pub trait DocumentFormat: Send + Sync {
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn extensions(&self) -> &'static [&'static str];
    fn capabilities(&self) -> FormatCapabilities;
    fn import(&self, input: &mut dyn ReadSeek) -> FormatResult<Document>;
    /// Warnings displayed before exporting a lossy representation.
    fn export_warnings(&self, _document: &Document) -> Vec<String> { Vec::new() }
    fn export(&self, document: &Document, output: &mut dyn WriteSeek) -> FormatResult<()>;
}

#[derive(Default)]
pub struct FormatRegistry {
    by_id: BTreeMap<String, Arc<dyn DocumentFormat>>,
    by_extension: BTreeMap<String, Arc<dyn DocumentFormat>>,
}

impl FormatRegistry {
    pub fn register<F: DocumentFormat + 'static>(&mut self, format: F) {
        let format: Arc<dyn DocumentFormat> = Arc::new(format);
        self.by_id.insert(format.id().to_owned(), format.clone());
        for ext in format.extensions() { self.by_extension.insert((*ext).to_owned(), format.clone()); }
    }

    pub fn by_id(&self, id: &str) -> Option<Arc<dyn DocumentFormat>> { self.by_id.get(id).cloned() }
    pub fn by_extension(&self, extension: &str) -> Option<Arc<dyn DocumentFormat>> {
        let extension = extension.trim_start_matches('.').to_ascii_lowercase();
        self.by_extension.get(&extension).cloned()
    }

    pub fn for_path(&self, path: &Path) -> Option<Arc<dyn DocumentFormat>> {
        path.extension().and_then(|s| s.to_str()).and_then(|ext| self.by_extension(ext))
    }
}
