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

/// Word can render the linked entries and PAGEREF fields as normal OOXML.
/// The content-control tag preserves Impress-specific settings on reopening.
fn contents_tag(review: &model::ReviewState) -> String {
    let options = &review.contents_options;
    let template = match options.template {
        model::ContentsTemplate::Classic => "classic",
        model::ContentsTemplate::Compact => "compact",
        model::ContentsTemplate::Modern => "modern",
        model::ContentsTemplate::ReadingOutline => "reading_outline",
        model::ContentsTemplate::Formal => "formal",
        model::ContentsTemplate::Technical => "technical",
        model::ContentsTemplate::Atlas => "atlas",
        model::ContentsTemplate::Book => "book",
        model::ContentsTemplate::InlinePages => "inline_pages",
        model::ContentsTemplate::ChapterGuide => "chapter_guide",
    };
    let leader = match options.leader {
        model::TabLeader::None => "none", model::TabLeader::Dots => "dots",
        model::TabLeader::Dashes => "dashes", model::TabLeader::Line => "line",
        model::TabLeader::Heavy => "heavy", model::TabLeader::MiddleDots => "middle",
    };
    let settings = format!("{template}:{}:{}:{leader}:{}", options.max_heading_level,
        u8::from(options.show_page_numbers), u8::from(review.automatic_contents));
    if options.right_align_page_numbers && options.max_numbered_level == 9 {
        format!("Impress.Contents.v2:{settings}")
    } else {
        format!("Impress.Contents.v3:{settings}:{}:{}", u8::from(options.right_align_page_numbers), options.max_numbered_level)
    }
}

fn is_contents_tag(tag: &str) -> bool {
    tag == "Impress.Contents.v1" || parse_contents_tag(tag).is_some()
}

fn parse_contents_tag(tag: &str) -> Option<(model::ContentsOptions, bool)> {
    let (version, settings) = tag.strip_prefix("Impress.Contents.")?.split_once(':')?;
    let mut fields = settings.split(':');
    let template = match fields.next()? {
        "classic" => model::ContentsTemplate::Classic,
        "compact" => model::ContentsTemplate::Compact,
        "modern" => model::ContentsTemplate::Modern,
        "reading_outline" => model::ContentsTemplate::ReadingOutline,
        "formal" => model::ContentsTemplate::Formal,
        "technical" => model::ContentsTemplate::Technical,
        "atlas" => model::ContentsTemplate::Atlas,
        "book" => model::ContentsTemplate::Book,
        "inline_pages" => model::ContentsTemplate::InlinePages,
        "chapter_guide" => model::ContentsTemplate::ChapterGuide,
        _ => return None,
    };
    let max_heading_level = fields.next()?.parse::<u8>().ok()?;
    if !(1..=9).contains(&max_heading_level) { return None; }
    let binary = |s: &str| match s { "0" => Some(false), "1" => Some(true), _ => None };
    let show_page_numbers = binary(fields.next()?)?;
    let leader = match fields.next()? {
        "none" => model::TabLeader::None, "dots" => model::TabLeader::Dots,
        "dashes" => model::TabLeader::Dashes, "line" => model::TabLeader::Line,
        "heavy" => model::TabLeader::Heavy, "middle" => model::TabLeader::MiddleDots,
        _ => return None,
    };
    let automatic = binary(fields.next()?)?;
    let (right_align_page_numbers, max_numbered_level) = match version {
        "v2" => (true, 9),
        "v3" => (binary(fields.next()?)?, fields.next()?.parse::<u8>().ok()?),
        _ => return None,
    };
    if !(1..=9).contains(&max_numbered_level) || fields.next().is_some() { return None; }
    Some((model::ContentsOptions { max_heading_level, template, show_page_numbers, leader, right_align_page_numbers, max_numbered_level, ..Default::default() }, automatic))
}
