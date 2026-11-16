//! Write genuine Word97 compound documents without an external converter.
#[path = "write_annotations.rs"]
mod annotations;
#[path = "write_content.rs"]
mod content;
#[path = "write_digest.rs"]
mod digest;
#[path = "write_drawings.rs"]
mod drawings;
#[path = "write_fkp.rs"]
mod fkp;
#[path = "write_lists.rs"]
mod lists;
#[path = "write_pictures.rs"]
mod pictures;
#[path = "write_properties.rs"]
mod properties;
#[path = "write_styles.rs"]
mod styles;
#[path = "write_warnings.rs"]
mod warnings;
use format::{FormatResult, WriteSeek};
use model::Document;

pub(crate) fn export_doc(document: &Document, output: &mut dyn WriteSeek) -> FormatResult<()> {
    content::export(document, output)
}
pub(crate) fn export_warnings(document: &Document) -> Vec<String> {
    warnings::export(document)
}
