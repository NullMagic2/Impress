mod import;
mod fonts;
mod omml;
mod package;
pub mod compatibility;
mod ligatures;
pub use compatibility::export_warnings;

use model::Document;
use format::{DocumentFormat, FormatCapabilities, FormatResult, ReadSeek, WriteSeek};

#[derive(Default)]
pub struct DocxFormat;

impl DocumentFormat for DocxFormat {
    fn id(&self) -> &'static str { "docx" }
    fn display_name(&self) -> &'static str { "Microsoft Word Document" }
    fn extensions(&self) -> &'static [&'static str] { &["docx"] }
    fn capabilities(&self) -> FormatCapabilities {
        FormatCapabilities { import: true, export: true, preserves_unknown_data: false, native_equations: true }
    }
    fn import(&self, input: &mut dyn ReadSeek) -> FormatResult<Document> { import::import_docx(input) }
    fn export_warnings(&self, document:&Document)->Vec<String>{compatibility::export_warnings(document)}
    fn export(&self, document: &Document, output: &mut dyn WriteSeek) -> FormatResult<()> { package::export_docx(document, output) }
}

/// DrawingML length unit: 12,700 English Metric Units per typographic point.
const EMU_PER_PT: f32 = 12_700.0;
