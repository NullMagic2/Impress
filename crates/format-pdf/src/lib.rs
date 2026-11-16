//! PDF import, as Word's PDF conversion: the PDF becomes an editable
//! document that keeps as much of the original as it can.
//!
//! A PDF stores positioned glyphs, not paragraphs, so the importer:
//! - reads each glyph's font, size and colour, the pictures and web links
//!   (`extract`, on a patched copy of pdf-extract in `vendor/`);
//! - finds page columns and reads them in order, recognises tables from
//!   aligned cells, and rebuilds paragraphs with their headings, alignment,
//!   indents and bulleted or numbered lists (`structure`);
//! - builds the document: styles from the PDF's fonts, formatted runs,
//!   lists, tables, pictures, the page size and margins, and repeated
//!   headers, footers and page numbers as a real header and footer with a
//!   page number field (`build`).
//!
//! Words hyphenated at line ends are rejoined. Scanned pages without a text
//! layer are read by OCR (`ocr`: Tesseract when installed, else the built-in
//! ocrs models, never stripping accents); vector drawings become pictures
//! (`drawings`). Exact positions are not kept.
mod build;
mod drawings;
mod extract;
mod ocr;
mod render;
mod scan;
mod structure;

use format::{DocumentFormat, FormatCapabilities, FormatError, FormatResult, ReadSeek, WriteSeek};
use model::Document;
use std::io::Read;

pub use build::to_document;
pub use extract::{Area, Line, Link, PageText, Picture, Span};

const MAX_INPUT: u64 = 256 * 1024 * 1024;

/// Text recovered from a PDF, before it is shaped into a document.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PdfSource {
    pub pages: Vec<PageText>, pub title: Option<String>, pub author: Option<String>, pub subject: Option<String>,
    /// What the conversion could not do, for the user (for example scans left
    /// as pictures because OCR would have stripped their accents).
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ImportOptions {
    /// Zero-based pages to import; None imports every page.
    pub pages: Option<Vec<usize>>,
    /// Join the lines of a paragraph (true) or keep the PDF's line breaks.
    pub reflow: bool,
    pub headings: bool,
    /// Take repeated headers, footers and page numbers out of the text and
    /// make them the document's header and footer.
    pub strip_running: bool,
    pub join_hyphens: bool,
    pub page_breaks: bool,
    /// Keep page columns as column sections (true) or flow them as one column.
    pub keep_columns: bool,
}

impl Default for ImportOptions {
    fn default() -> Self { Self { pages: None, reflow: true, headings: true, strip_running: true, join_hyphens: true, page_breaks: false, keep_columns: true } }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind { Title, Heading(u8), Body }

/// A rebuilt paragraph as plain lines, for previews.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece { pub kind: Kind, pub lines: Vec<String>, pub page_break_before: bool }

/// Reads every page, scans included (OCR). Fails for encrypted or damaged files.
pub fn analyze(bytes: &[u8]) -> Result<PdfSource, String> {
    let mut source = analyze_without_ocr(bytes)?;
    read_scans(bytes, &mut source);
    Ok(source)
}

/// Reads scanned pages of `source` by OCR, which can take seconds a page;
/// `analyze_without_ocr` leaves them as pictures for a quick preview.
pub fn read_scans(bytes: &[u8], source: &mut PdfSource) {
    let notes = ocr::recognise(bytes, &mut source.pages);
    source.notes.extend(notes);
}

/// Reads the text, pictures, drawings and links of every page; scans stay pictures.
pub fn analyze_without_ocr(bytes: &[u8]) -> Result<PdfSource, String> {
    let doc = pdf_extract::Document::load_mem(bytes).map_err(|e| format!("This file is not a readable PDF ({e})."))?;
    if doc.is_encrypted() { return Err("This PDF is password-protected. Remove the protection and try again.".into()); }
    let mut collector = extract::Collector::default();
    // The extractor asserts on some malformed fonts; treat that as a read error.
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| pdf_extract::output_doc(&doc, &mut collector)));
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(e)) => return Err(format!("The PDF could not be read ({e:?}).")),
        Err(_) => return Err("The PDF uses a font or structure the importer cannot read.".into()),
    }
    let tops = std::mem::take(&mut collector.tops);
    let mut pages = std::mem::take(&mut collector.pages);
    extract::attach_links(&doc, &mut pages, &tops);
    let marks = std::mem::take(&mut collector.marks);
    drawings::convert(bytes, &mut pages, &marks);
    scan::render_blank_pages(bytes, &mut pages);
    let info = |key: &[u8]| -> Option<String> {
        let dict = doc.trailer.get(b"Info").ok()
            .and_then(|o| o.as_reference().ok()).and_then(|id| doc.get_dictionary(id).ok())
            .or_else(|| doc.trailer.get(b"Info").ok().and_then(|o| o.as_dict().ok()))?;
        let bytes = dict.get(key).ok()?.as_str().ok()?;
        let text = decode_text_string(bytes);
        let text = text.trim();
        (!text.is_empty()).then(|| text.to_string())
    };
    Ok(PdfSource { pages, title: info(b"Title"), author: info(b"Author"), subject: info(b"Subject"), notes: Vec::new() })
}

/// PDF text strings are UTF-16BE with a byte-order mark, or PDFDocEncoding
/// (Latin-1 for the printable range).
fn decode_text_string(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let units: Vec<u16> = bytes[2..].chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
        return String::from_utf16_lossy(&units);
    }
    if let Ok(text) = std::str::from_utf8(bytes) { return text.to_string(); }
    bytes.iter().map(|&b| b as char).collect()
}

/// The import result as plain paragraphs: one per paragraph, one per table
/// row (cells separated by bars) and a marker for each picture.
pub fn pieces(source: &PdfSource, options: &ImportOptions) -> Vec<Piece> {
    use structure::Item;
    let mut out = Vec::new();
    for item in structure::analyze_structure(source, options).items {
        match item {
            Item::Para(para) => out.push(Piece { kind: para.kind, lines: para.lines.into_iter().map(|l| l.text).collect(), page_break_before: para.page_break_before }),
            Item::Table(grid) => for (index, row) in grid.rows.into_iter().enumerate() {
                let cells: Vec<String> = row.into_iter().map(|cell| cell.into_iter().map(|l| l.text).collect::<Vec<_>>().join(" ")).collect();
                out.push(Piece { kind: Kind::Body, lines: vec![cells.join("  │  ")], page_break_before: index == 0 && grid.page_break_before });
            },
            Item::Picture { picture, page_break_before, .. } => out.push(Piece {
                kind: Kind::Body, lines: vec![format!("[Picture {:.0} × {:.0} pt]", picture.area.width, picture.area.height)], page_break_before,
            }),
            Item::Columns { .. } | Item::ColumnBreak => {}
        }
    }
    out
}

pub(crate) fn joined(lines: &[String], join_hyphens: bool) -> String {
    let mut text = String::new();
    for line in lines {
        if text.is_empty() { text.push_str(line); continue; }
        let hyphenated = join_hyphens && text.ends_with('-') && text.chars().rev().nth(1).is_some_and(char::is_alphabetic)
            && line.chars().next().is_some_and(char::is_lowercase);
        if hyphenated { text.pop(); } else { text.push(' '); }
        text.push_str(line);
    }
    text
}

/// Plain-text rendition of the import result, for previews.
pub fn preview(source: &PdfSource, options: &ImportOptions, max_pieces: usize) -> String {
    let mut out = String::new();
    for piece in pieces(source, options).into_iter().take(max_pieces) {
        if piece.page_break_before { out.push_str("──────── page break ────────\n\n"); }
        let text = if options.reflow || piece.kind != Kind::Body { joined(&piece.lines, options.join_hyphens) } else { piece.lines.join("\n") };
        match piece.kind {
            Kind::Title => out.push_str(&format!("TITLE  {text}\n\n")),
            Kind::Heading(level) => out.push_str(&format!("H{level}  {text}\n\n")),
            Kind::Body => out.push_str(&format!("{text}\n\n")),
        }
    }
    out
}

/// Opens PDFs by converting them. Converted documents start from the
/// template's styles and settings.
pub struct PdfFormat { template: fn() -> Document }

impl PdfFormat {
    pub fn new(template: fn() -> Document) -> Self { Self { template } }
}

impl Default for PdfFormat {
    fn default() -> Self { Self::new(Document::default) }
}

impl DocumentFormat for PdfFormat {
    fn id(&self) -> &'static str { "pdf" }
    fn display_name(&self) -> &'static str { "PDF document" }
    fn extensions(&self) -> &'static [&'static str] { &["pdf"] }
    fn capabilities(&self) -> FormatCapabilities {
        FormatCapabilities { import: true, export: false, preserves_unknown_data: false, native_equations: false }
    }
    fn import(&self, input: &mut dyn ReadSeek) -> FormatResult<Document> {
        let mut data = Vec::new();
        input.take(MAX_INPUT + 1).read_to_end(&mut data)?;
        if data.len() as u64 > MAX_INPUT { return Err(FormatError::Invalid { format: "PDF", message: "file is larger than 256 MiB".into() }); }
        let source = analyze(&data).map_err(|message| FormatError::Invalid { format: "PDF", message })?;
        Ok(to_document(&source, &ImportOptions::default(), (self.template)()))
    }
    fn export(&self, _document: &Document, _output: &mut dyn WriteSeek) -> FormatResult<()> {
        Err(FormatError::Unsupported { format: "PDF", feature: "saving; use Export PDF".into() })
    }
}

#[cfg(test)] #[path = "../tests/unit/lib.rs"] mod tests;
