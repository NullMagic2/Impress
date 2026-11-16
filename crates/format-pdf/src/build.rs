//! The editable document built from the page structure: styles taken from
//! the PDF's fonts, formatted runs and links, lists, tables, pictures, page
//! size and margins, and running headers and footers as real ones.
use crate::extract::{Line, Span};
use crate::structure::{analyze_structure, Align, Analysis, Grid, Item, Para, RunningLine};
use crate::{ImportOptions, Kind, PdfSource};
use model::{
    Block, BlockImage, Border, BorderSet, BorderStyle, CharacterProperties, Document, Field, FieldKind, HeaderFooter, Hyperlink,
    ImageAlignment, ImageWrap, Inline, ListDefinition, ListFormat, ListLevel, NodeId, Paragraph, ParagraphAlignment, Resource,
    ResourceId, Style, StyleId, StyleKind, Table, TableCell, TableProperties, TableRow, TextRun, STYLE_HEADING_1, STYLE_NORMAL, STYLE_TITLE,
};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

const PT_TO_MM: f64 = 25.4 / 72.0;
/// Page margins are kept within these limits (millimetres).
const MARGIN_RANGE_MM: (f64, f64) = (6.0, 50.0);
/// Indent of each list level, and the hanging indent of its marker (mm).
const LIST_INDENT_MM: f32 = 6.35;
/// Sizes closer than this to the paragraph style's size are the same (points).
const SIZE_TOLERANCE_PT: f64 = 0.6;
const TABLE_BORDER_PT: f32 = 0.5;
/// Most columns a section is given.
const MAX_COLUMNS: u16 = 12;

fn mm(points: f64) -> f32 { (points * PT_TO_MM) as f32 }
/// Width of one column of `count` columns `gap` points apart (points).
fn column_width(text_width: f64, (count, gap): (usize, f64)) -> f64 {
    let count = count.max(1) as f64;
    ((text_width - gap * (count - 1.0)) / count).max(36.0)
}
fn half_points(size: f64) -> f32 { ((size * 2.0).round() / 2.0) as f32 }
fn hex((r, g, b): (u8, u8, u8)) -> String { format!("#{r:02X}{g:02X}{b:02X}") }

/// Builds an editable document from the recovered text.
pub fn to_document(source: &PdfSource, options: &ImportOptions, mut document: Document) -> Document {
    let analysis = analyze_structure(source, options);
    let mut builder = Builder { document: &mut document, options, pictures: HashMap::new() };
    builder.styles(&analysis);
    let text_width = builder.page_setup(&analysis);
    let mut blocks = Vec::new();
    // Finished column sections: their columns and blocks.
    let mut sections: Vec<(usize, f64, Vec<Block>)> = Vec::new();
    let mut columns = (1usize, 0.0f64);
    let mut list: Option<(NodeId, Vec<Option<(ListFormat, i32)>>)> = None;
    for item in &analysis.items {
        let breaks = match item {
            Item::Para(p) => p.page_break_before, Item::Table(t) => t.page_break_before,
            Item::Picture { page_break_before, .. } => *page_break_before, Item::Columns { .. } | Item::ColumnBreak => false,
        };
        if breaks { blocks.push(Block::PageBreak { id: NodeId::new() }); }
        match item {
            Item::Para(para) => {
                let mut paragraph = builder.paragraph(para);
                if let Some(mark) = para.list {
                    // Consecutive items share one list; each level keeps the
                    // format and start of its first item. A different format
                    // at a level starts a new list.
                    let level = usize::from(mark.level.min(2));
                    if list.as_ref().is_some_and(|(_, levels)| levels[level].is_some_and(|(format, _)| format != mark.format)) {
                        if let Some((id, levels)) = list.take() { builder.define_list(id, &levels); }
                    }
                    let (id, levels) = list.get_or_insert_with(|| (NodeId::new(), vec![None; 3]));
                    levels[level].get_or_insert((mark.format, mark.start));
                    paragraph.properties.list_id = Some(id.clone());
                    paragraph.properties.list_level = Some(mark.level.min(2));
                } else if let Some((id, levels)) = list.take() { builder.define_list(id, &levels); }
                blocks.push(Block::Paragraph(paragraph));
            }
            Item::Table(grid) => {
                if let Some((id, levels)) = list.take() { builder.define_list(id, &levels); }
                blocks.push(Block::Table(builder.table(grid, column_width(text_width, columns))));
            }
            Item::Picture { picture, align, .. } => {
                if let Some((id, levels)) = list.take() { builder.define_list(id, &levels); }
                blocks.push(builder.picture(picture, *align, column_width(text_width, columns)));
            }
            Item::Columns { count, gap } => {
                if let Some((id, levels)) = list.take() { builder.define_list(id, &levels); }
                if !blocks.is_empty() { sections.push((columns.0, columns.1, std::mem::take(&mut blocks))); }
                columns = (*count, *gap);
            }
            Item::ColumnBreak => blocks.push(Block::ColumnBreak { id: NodeId::new() }),
        }
    }
    if let Some((id, levels)) = list.take() { builder.define_list(id, &levels); }
    if blocks.is_empty() && sections.is_empty() { blocks.push(Block::Paragraph(Paragraph::plain(""))); }
    if !blocks.is_empty() { sections.push((columns.0, columns.1, blocks)); }
    builder.sections(sections, &analysis);
    for note in &source.notes {
        document.compatibility.push(model::CompatibilityIssue { code: "pdf.conversion".into(), part: "pages".into(), message: note.clone() });
    }
    if document.metadata.title.is_none() { document.metadata.title = source.title.clone(); }
    if document.metadata.author.is_none() { document.metadata.author = source.author.clone(); }
    if document.metadata.subject.is_none() { document.metadata.subject = source.subject.clone(); }
    document
}

struct Builder<'a> {
    document: &'a mut Document,
    options: &'a ImportOptions,
    /// Resources by content hash, so a picture drawn on every page is stored once.
    pictures: HashMap<u64, ResourceId>,
}

/// A run of text before it becomes an inline.
struct Piece { text: String, properties: CharacterProperties, link: Option<Arc<str>> }

impl Builder<'_> {
    fn style_id(&mut self, kind: Kind) -> StyleId {
        match kind {
            Kind::Title => StyleId::from(STYLE_TITLE),
            Kind::Heading(level) => heading_style(self.document, level),
            Kind::Body => StyleId::from(STYLE_NORMAL),
        }
    }

    /// Normal takes the body text's font and size; each heading level takes
    /// the font, size, weight and colour its text has in the PDF.
    fn styles(&mut self, analysis: &Analysis) {
        let mut by_kind: HashMap<Kind, Vec<&Span>> = HashMap::new();
        for item in &analysis.items {
            match item {
                Item::Para(p) => by_kind.entry(p.kind).or_default().extend(p.lines.iter().flat_map(|l| &l.spans)),
                Item::Table(t) => by_kind.entry(Kind::Body).or_default().extend(t.rows.iter().flatten().flatten().flat_map(|l| &l.spans)),
                Item::Picture { .. } | Item::Columns { .. } | Item::ColumnBreak => {}
            }
        }
        for (kind, spans) in by_kind {
            let Some(look) = dominant(&spans) else { continue; };
            let id = self.style_id(kind);
            let Some(style) = self.document.styles.styles.get_mut(&id) else { continue; };
            let character = &mut style.character;
            if !look.family.is_empty() { character.font_family = Some(look.family.clone()); }
            character.font_size_pt = Some(half_points(if kind == Kind::Body { analysis.body_size } else { look.size }));
            if kind != Kind::Body {
                character.bold = Some(look.bold);
                character.italic = Some(look.italic);
                character.color = Some(look.color.map_or_else(|| "#000000".into(), hex));
            } else if let Some(color) = look.color { character.color = Some(hex(color)); }
        }
    }

    /// Page size and margins of the first page; returns the text width (pt).
    fn page_setup(&mut self, analysis: &Analysis) -> f64 {
        let properties = &mut self.document.sections[0].properties;
        let Some((width, height)) = analysis.page_size else {
            return f64::from(properties.page_width_mm - properties.margin_left_mm - properties.margin_right_mm) / PT_TO_MM;
        };
        properties.page_width_mm = mm(width);
        properties.page_height_mm = mm(height);
        if let Some((left, top, right, bottom)) = analysis.text_box {
            let clamp = |points: f64| (points * PT_TO_MM).clamp(MARGIN_RANGE_MM.0, MARGIN_RANGE_MM.1) as f32;
            properties.margin_left_mm = clamp(left);
            properties.margin_right_mm = clamp(width - right);
            properties.margin_top_mm = clamp(top);
            properties.margin_bottom_mm = clamp(height - bottom);
            properties.header_distance_mm = (properties.margin_top_mm * 0.45).max(4.0);
            properties.footer_distance_mm = (properties.margin_bottom_mm * 0.45).max(4.0);
        }
        f64::from(properties.page_width_mm - properties.margin_left_mm - properties.margin_right_mm) / PT_TO_MM
    }

    fn paragraph(&mut self, para: &Para) -> Paragraph {
        let style = self.style_id(para.kind);
        let mut lines = para.lines.clone();
        if para.list.is_some() { strip_marker(&mut lines[0], para.marker_chars); }
        let mut paragraph = Paragraph::plain("");
        paragraph.content = self.inlines(&lines, &style).into();
        paragraph.style = Some(style);
        let properties = &mut paragraph.properties;
        properties.alignment = match para.align {
            Align::Left => None,
            Align::Center => Some(ParagraphAlignment::Center),
            Align::Right => Some(ParagraphAlignment::Right),
            Align::Justify => Some(ParagraphAlignment::Justify),
        };
        if para.left_indent > 0.0 { properties.left_indent_mm = Some(mm(para.left_indent)); }
        if para.first_indent != 0.0 { properties.first_line_indent_mm = Some(mm(para.first_indent)); }
        paragraph
    }

    /// Lines joined into formatted runs: reflowed with spaces (and words
    /// hyphenated at line ends rejoined), or kept as separate lines.
    fn inlines(&self, lines: &[Line], style: &StyleId) -> Vec<Inline> {
        let base = self.document.styles.resolve_character(Some(style));
        let mut pieces: Vec<Piece> = Vec::new();
        let mut breaks: Vec<usize> = Vec::new();
        for (index, line) in lines.iter().enumerate() {
            if index > 0 {
                if self.options.reflow {
                    let hyphenated = self.options.join_hyphens && pieces.last().is_some_and(|p| ends_with_hyphenated_word(&p.text))
                        && line.text.chars().next().is_some_and(char::is_lowercase);
                    if let Some(last) = pieces.last_mut() { if hyphenated { last.text.pop(); } else { last.text.push(' '); } }
                } else {
                    breaks.push(pieces.len());
                }
            }
            for span in &line.spans {
                let properties = run_properties(span, &base);
                let after_break = breaks.contains(&pieces.len());
                match pieces.last_mut() {
                    Some(last) if !after_break && last.properties == properties && last.link == span.link => last.text.push_str(&span.text),
                    _ => pieces.push(Piece { text: span.text.clone(), properties, link: span.link.clone() }),
                }
            }
        }
        let mut out = Vec::new();
        for (index, piece) in pieces.into_iter().enumerate() {
            if breaks.contains(&index) { out.push(Inline::LineBreak { id: NodeId::new() }); }
            let run = Inline::Text(TextRun { properties: piece.properties, ..TextRun::plain(piece.text) });
            match piece.link {
                Some(target) => match out.last_mut() {
                    Some(Inline::Hyperlink(link)) if link.target.as_str() == &*target => link.content.push(run),
                    _ => out.push(Inline::Hyperlink(Hyperlink { id: NodeId::new(), target: target.to_string(), content: vec![run].into() })),
                },
                None => out.push(run),
            }
        }
        if out.is_empty() { out.push(Inline::Text(TextRun::plain(""))); }
        out
    }

    fn define_list(&mut self, id: NodeId, levels: &[Option<(ListFormat, i32)>]) {
        let first = levels.iter().flatten().next().copied().unwrap_or((ListFormat::Bullet, 1));
        let levels = levels.iter().enumerate().map(|(level, entry)| {
            let (format, start_at) = entry.unwrap_or(first);
            let n = level + 1;
            let text = match format {
                ListFormat::Bullet => ["•", "◦", "▪"][level].to_string(),
                ListFormat::Decimal | ListFormat::LowerRoman | ListFormat::UpperRoman => format!("%{n}."),
                ListFormat::LowerLetter | ListFormat::UpperLetter => format!("%{n})"),
            };
            ListLevel { level: level as u8, format, text, start_at, left_indent_mm: LIST_INDENT_MM * n as f32, hanging_indent_mm: LIST_INDENT_MM }
        }).collect();
        self.document.lists.insert(id.clone(), ListDefinition { id, levels });
    }

    fn table(&mut self, grid: &Grid, text_width: f64) -> Table {
        let total: f64 = grid.widths.iter().sum::<f64>().max(1.0);
        let scale = (text_width / total).min(1.0);
        let widths: Vec<f32> = grid.widths.iter().map(|w| mm(w * scale)).collect();
        let normal = StyleId::from(STYLE_NORMAL);
        let rows = grid.rows.iter().map(|row| TableRow {
            id: NodeId::new(),
            cells: row.iter().zip(&widths).map(|(lines, width)| {
                let mut paragraph = Paragraph::plain("");
                paragraph.content = self.inlines(lines, &normal).into();
                TableCell { id: NodeId::new(), blocks: vec![Block::Paragraph(paragraph)].into(), column_span: 1, row_span: 1,
                    width_mm: Some(*width), shading: None, borders: None, vertical_alignment: Default::default() }
            }).collect(),
            repeat_as_header: false, allow_break_across_pages: true, height_mm: None,
        }).collect();
        let border = || Some(Border { width_pt: TABLE_BORDER_PT, color: "#000000".into(), style: BorderStyle::Single });
        Table { id: NodeId::new(), rows, properties: TableProperties {
            preferred_width_mm: Some(widths.iter().sum()),
            borders: Some(BorderSet { top: border(), right: border(), bottom: border(), left: border(), inside_horizontal: border(), inside_vertical: border() }),
            grid_column_widths_mm: widths,
            ..TableProperties::default()
        } }
    }

    fn picture(&mut self, picture: &crate::extract::Picture, align: Align, text_width: f64) -> Block {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        picture.data.hash(&mut hasher);
        let resource = self.pictures.entry(hasher.finish()).or_insert_with(|| {
            let id = ResourceId::new();
            self.document.resources.insert(id.clone(), Resource { font_faces: Vec::new(),  id: id.clone(), media_type: picture.media_type.into(), original_name: None, data: Arc::clone(&picture.data) });
            id
        }).clone();
        let scale = (text_width / picture.area.width).min(1.0);
        Block::Image(BlockImage {
            id: NodeId::new(), resource, width_mm: mm(picture.area.width * scale), height_mm: mm(picture.area.height * scale),
            alt_text: None, effects: Default::default(), wrap: ImageWrap::Inline,
            alignment: match align { Align::Center => ImageAlignment::Center, Align::Right => ImageAlignment::Right, _ => ImageAlignment::Left },
            anchor: None,
        })
    }

    /// One section per run of columns: the first keeps the page setup, the
    /// rest continue on the same page with their own number of columns.
    /// Every section gets its own copy of the header and footer.
    fn sections(&mut self, sections: Vec<(usize, f64, Vec<Block>)>, analysis: &Analysis) {
        let template = self.document.sections[0].clone();
        let mut built = Vec::new();
        for (index, (count, gap, blocks)) in sections.into_iter().enumerate() {
            let mut section = if index == 0 { template.clone() } else { model::Section { id: NodeId::new(), ..template.clone() } };
            section.properties.column_count = count.clamp(1, usize::from(MAX_COLUMNS)) as u16;
            if count > 1 { section.properties.column_gap_mm = mm(gap); }
            section.properties.columns = Vec::new();
            if index > 0 { section.properties.start_on = model::SectionStart::Continuous; }
            section.blocks = blocks.into();
            self.document.sections = vec![section];
            self.running(analysis);
            built.push(self.document.sections.pop().expect("the section just added"));
        }
        self.document.sections = built;
    }

    /// Running headers and footers become the section's header and footer,
    /// with a page number field where the PDF printed page numbers.
    fn running(&mut self, analysis: &Analysis) {
        let normal_size = self.document.styles.resolve_character(Some(&StyleId::from(STYLE_NORMAL))).font_size_pt.unwrap_or(11.0);
        let stream = |lines: &[RunningLine]| -> Option<HeaderFooter> {
            if lines.is_empty() { return None; }
            let blocks = lines.iter().map(|line| {
                let mut properties = CharacterProperties::default();
                if (line.size - f64::from(normal_size)).abs() > SIZE_TOLERANCE_PT { properties.font_size_pt = Some(half_points(line.size)); }
                let text = |t: &str| Inline::Text(TextRun { properties: properties.clone(), ..TextRun::plain(t) });
                let mut content = Vec::new();
                if !line.before.is_empty() { content.push(text(&line.before)); }
                if line.page_number { content.push(Inline::Field(Field { id: NodeId::new(), kind: FieldKind::PageNumber, display_text: Some("1".into()), color: None })); }
                if !line.after.is_empty() { content.push(text(&line.after)); }
                let mut paragraph = Paragraph::plain("");
                paragraph.content = content.into();
                paragraph.properties.alignment = match line.align { Align::Center => Some(ParagraphAlignment::Center), Align::Right => Some(ParagraphAlignment::Right), _ => None };
                Block::Paragraph(paragraph)
            }).collect::<Vec<_>>();
            Some(HeaderFooter { id: NodeId::new(), blocks: blocks.into() })
        };
        let section = &mut self.document.sections[0];
        if let Some(header) = stream(&analysis.headers) { section.headers.default = Some(header); }
        if let Some(footer) = stream(&analysis.footers) { section.footers.default = Some(footer); }
    }
}

/// The look shared by most characters of a set of spans.
struct Look { family: String, size: f64, bold: bool, italic: bool, color: Option<(u8, u8, u8)> }

fn dominant(spans: &[&Span]) -> Option<Look> {
    fn most<K: Eq + Hash + Clone>(spans: &[&Span], key: impl Fn(&Span) -> K) -> Option<K> {
        let mut counts: HashMap<K, usize> = HashMap::new();
        for span in spans { *counts.entry(key(span)).or_default() += span.text.trim().chars().count(); }
        counts.into_iter().max_by_key(|(_, n)| *n).map(|(k, _)| k)
    }
    Some(Look {
        family: most(spans, |s| s.family.clone()).filter(|f| !f.is_empty()).or_else(|| most(&spans.iter().copied().filter(|s| !s.family.is_empty()).collect::<Vec<_>>(), |s| s.family.clone())).unwrap_or_default(),
        size: most(spans, |s| (s.size * 2.0).round() as i64)? as f64 / 2.0,
        bold: most(spans, |s| s.bold)?,
        italic: most(spans, |s| s.italic)?,
        color: most(spans, |s| s.color)?,
    })
}

/// Direct formatting of a span: only what differs from its paragraph style.
fn run_properties(span: &Span, base: &CharacterProperties) -> CharacterProperties {
    let mut properties = CharacterProperties::default();
    if !span.family.is_empty() && base.font_family.as_deref() != Some(span.family.as_str()) { properties.font_family = Some(span.family.clone()); }
    if base.font_size_pt.is_none_or(|size| (f64::from(size) - span.size).abs() > SIZE_TOLERANCE_PT) { properties.font_size_pt = Some(half_points(span.size)); }
    if span.bold != base.bold.unwrap_or(false) { properties.bold = Some(span.bold); }
    if span.italic != base.italic.unwrap_or(false) { properties.italic = Some(span.italic); }
    let base_color = base.color.as_deref().filter(|c| !c.eq_ignore_ascii_case("#000000") && !c.eq_ignore_ascii_case("#auto"));
    match (span.color.map(hex), base_color) {
        (Some(color), base) if base != Some(color.as_str()) => properties.color = Some(color),
        (None, Some(_)) => properties.color = Some("#000000".into()),
        _ => {}
    }
    // Spaces carry no visible formatting; they take their neighbour's.
    if span.text.trim().is_empty() { return CharacterProperties::default(); }
    properties
}

fn ends_with_hyphenated_word(text: &str) -> bool {
    let mut chars = text.chars().rev();
    matches!(chars.next(), Some('-' | '\u{00AD}' | '\u{2010}')) && chars.next().is_some_and(char::is_alphabetic)
}

/// Removes a list item's marker (and the space after it) from its first line.
fn strip_marker(line: &mut Line, mut chars: usize) {
    for span in &mut line.spans {
        if chars == 0 { break; }
        let count = span.text.chars().count();
        if count <= chars { chars -= count; span.text.clear(); }
        else { span.text = span.text.chars().skip(chars).collect(); chars = 0; }
    }
    line.spans.retain(|s| !s.text.is_empty());
    line.text = line.spans.iter().map(|s| s.text.as_str()).collect();
}

pub(crate) fn heading_style(document: &mut Document, level: u8) -> StyleId {
    if level <= 1 { return StyleId::from(STYLE_HEADING_1); }
    let name = format!("Heading {level}");
    if let Some(id) = document.styles.id_by_name(&name) { return id; }
    let id = StyleId::from(format!("pdf.heading{level}").as_str());
    let base = StyleId::from(STYLE_HEADING_1);
    let base_size = document.styles.resolve_character(Some(&base)).font_size_pt.unwrap_or(16.0);
    document.styles.styles.insert(id.clone(), Style {
        id: id.clone(), name, kind: StyleKind::Paragraph, based_on: Some(base), next_style: Some(StyleId::from(STYLE_NORMAL)),
        character: CharacterProperties { font_size_pt: Some((base_size - 2.5 * (level as f32 - 1.0)).max(11.0)), ..CharacterProperties::default() },
        paragraph: Default::default(), formatting: Default::default(),
    });
    id
}
