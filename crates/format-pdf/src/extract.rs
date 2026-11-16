//! Reading a PDF: styled glyphs grouped into chunks of text on one baseline,
//! the pictures drawn on each page and the areas of its web links.
use pdf_extract::{ColorSpace, MediaBox, Object, OutputDev, OutputError, Path, PathOp, PdfImage, TextStyle, Transform};
use std::rc::Rc;
use std::sync::Arc;

/// A gap wider than this many ems ends a chunk: it separates table cells,
/// page columns or a list marker from its text, never two words.
const CHUNK_GAP_EM: f64 = 1.0;
/// A gap wider than this many ems between glyphs is a word space.
const WORD_GAP_EM: f64 = 0.18;
/// Pictures smaller than this (points, either side) are rules or bullets.
const MIN_PICTURE_PT: f64 = 8.0;
/// Pages with more shapes than this (maps, plots of many points) keep only
/// the first ones for finding drawings.
const MAX_MARKS_PER_PAGE: usize = 20_000;

/// A rectangle in PDF points, origin at the page's top-left corner.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Area { pub x: f64, pub y: f64, pub width: f64, pub height: f64 }

impl Area {
    pub fn right(&self) -> f64 { self.x + self.width }
    pub fn bottom(&self) -> f64 { self.y + self.height }
    pub fn contains(&self, x: f64, y: f64) -> bool { x >= self.x && x <= self.right() && y >= self.y && y <= self.bottom() }
}

/// Text of one font, size and colour within a line.
#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    pub text: String,
    pub size: f64,
    /// Font family, cleaned of subset tags and style suffixes ("" when unknown).
    pub family: String,
    pub bold: bool,
    pub italic: bool,
    /// Text colour, None for black.
    pub color: Option<(u8, u8, u8)>,
    /// Web address the text links to.
    pub link: Option<Arc<str>>,
}

/// Text on one baseline without wide gaps, in PDF points (y grows downward,
/// at the baseline).
#[derive(Clone, Debug, PartialEq)]
pub struct Line { pub text: String, pub x0: f64, pub x1: f64, pub y: f64, pub size: f64, pub spans: Vec<Span> }

impl Line {
    /// A line of plain text in one unnamed font.
    pub fn plain(text: &str, x0: f64, x1: f64, y: f64, size: f64) -> Self {
        let span = Span { text: text.into(), size, family: String::new(), bold: false, italic: false, color: None, link: None };
        Self { text: text.into(), x0, x1, y, size, spans: vec![span] }
    }
    pub fn all_bold(&self) -> bool { self.spans.iter().filter(|s| !s.text.trim().is_empty()).all(|s| s.bold) }
}

/// A picture drawn on a page, as an image file.
#[derive(Clone, Debug, PartialEq)]
pub struct Picture { pub area: Area, pub media_type: &'static str, pub data: Arc<Vec<u8>> }

/// A web link area on a page.
#[derive(Clone, Debug, PartialEq)]
pub struct Link { pub area: Area, pub uri: String }

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PageText { pub width: f64, pub height: f64, pub lines: Vec<Line>, pub pictures: Vec<Picture>, pub links: Vec<Link> }

/// A painted vector shape: its bounding box and what it is made of.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Mark {
    pub(crate) area: Area,
    /// The outline has curves (pie slices, diagrams, logos).
    pub(crate) curved: bool,
    /// A white fill, usually a background or knock-out.
    pub(crate) white: bool,
}

/// A shown glyph with its style.
struct Glyph { x: f64, y: f64, end: f64, size: f64, text: String, style: Rc<SpanStyle> }

#[derive(Clone, Debug, PartialEq)]
struct SpanStyle { family: String, bold: bool, italic: bool, color: Option<(u8, u8, u8)> }

#[derive(Default)]
pub(crate) struct Collector {
    pub(crate) pages: Vec<PageText>,
    /// Top edge of each page's media box, to convert link rectangles.
    pub(crate) tops: Vec<f64>,
    /// Painted shapes of each page.
    pub(crate) marks: Vec<Vec<Mark>>,
    page_marks: Vec<Mark>,
    glyphs: Vec<Glyph>,
    pictures: Vec<Picture>,
    style: Option<Rc<SpanStyle>>,
    top: f64,
    width: f64,
    height: f64,
}

impl OutputDev for Collector {
    fn begin_page(&mut self, _page: u32, media: &MediaBox, _art: Option<(f64, f64, f64, f64)>) -> Result<(), OutputError> {
        self.glyphs.clear();
        self.pictures.clear();
        self.page_marks.clear();
        self.top = media.ury;
        self.width = (media.urx - media.llx).abs();
        self.height = (media.ury - media.lly).abs();
        Ok(())
    }
    fn end_page(&mut self) -> Result<(), OutputError> {
        let lines = chunks(std::mem::take(&mut self.glyphs));
        self.pages.push(PageText { width: self.width, height: self.height, lines, pictures: std::mem::take(&mut self.pictures), links: Vec::new() });
        self.tops.push(self.top);
        self.marks.push(std::mem::take(&mut self.page_marks));
        Ok(())
    }
    fn fill(&mut self, ctm: &Transform, space: &ColorSpace, color: &[f64], path: &Path) -> Result<(), OutputError> {
        let white = pdf_extract::device_rgb(space, color).is_some_and(|(r, g, b)| r.min(g).min(b) > 245);
        self.mark(ctm, path, white);
        Ok(())
    }
    fn stroke(&mut self, ctm: &Transform, _space: &ColorSpace, _color: &[f64], path: &Path) -> Result<(), OutputError> {
        self.mark(ctm, path, false);
        Ok(())
    }
    fn output_character(&mut self, trm: &Transform, width: f64, _spacing: f64, font_size: f64, text: &str) -> Result<(), OutputError> {
        // Upright text only: rotated captions and margin stamps would break lines apart.
        if trm.m12.abs() > trm.m11.abs() * 0.05 || trm.m21.abs() > trm.m22.abs() * 0.05 || trm.m11 <= 0.0 || trm.m22 <= 0.0 { return Ok(()); }
        let v = trm.transform_vector(euclid::vec2(font_size, font_size));
        let size = (v.x * v.y).abs().sqrt();
        if !size.is_finite() || size <= 0.0 || text.is_empty() { return Ok(()); }
        let style = self.style.clone().unwrap_or_else(|| Rc::new(SpanStyle { family: String::new(), bold: false, italic: false, color: None }));
        let (x, y) = (trm.m31, self.top - trm.m32);
        self.glyphs.push(Glyph { x, y, end: x + width * size, size, text: normalize(text), style });
        Ok(())
    }
    fn output_styled_character(&mut self, trm: &Transform, width: f64, spacing: f64, font_size: f64, text: &str, style: &TextStyle) -> Result<(), OutputError> {
        let next = span_style(style);
        if self.style.as_deref() != Some(&next) { self.style = Some(Rc::new(next)); }
        self.output_character(trm, width, spacing, font_size, text)
    }
    fn image(&mut self, ctm: &Transform, image: &PdfImage) -> Result<(), OutputError> {
        let corners = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)].map(|(x, y)| ctm.transform_point(euclid::point2(x, y)));
        let (left, right) = corners.iter().fold((f64::MAX, f64::MIN), |(l, r), p| (l.min(p.x), r.max(p.x)));
        let (low, high) = corners.iter().fold((f64::MAX, f64::MIN), |(l, h), p| (l.min(p.y), h.max(p.y)));
        let area = Area { x: left, y: self.top - high, width: right - left, height: high - low };
        if area.width < MIN_PICTURE_PT || area.height < MIN_PICTURE_PT { return Ok(()); }
        let (media_type, data) = match image {
            PdfImage::Jpeg(bytes) => ("image/jpeg", bytes.clone()),
            PdfImage::Rgba { width, height, pixels } => {
                let Some(buffer) = image::RgbaImage::from_raw(*width, *height, pixels.clone()) else { return Ok(()); };
                let mut png = std::io::Cursor::new(Vec::new());
                if buffer.write_to(&mut png, image::ImageFormat::Png).is_err() { return Ok(()); }
                ("image/png", png.into_inner())
            }
        };
        self.pictures.push(Picture { area, media_type, data: Arc::new(data) });
        Ok(())
    }
    fn begin_word(&mut self) -> Result<(), OutputError> { Ok(()) }
    fn end_word(&mut self) -> Result<(), OutputError> { Ok(()) }
    fn end_line(&mut self) -> Result<(), OutputError> { Ok(()) }
}

impl Collector {
    /// Records a painted path's box in page coordinates.
    fn mark(&mut self, ctm: &Transform, path: &Path, white: bool) {
        if self.page_marks.len() >= MAX_MARKS_PER_PAGE { return; }
        let mut points = Vec::new();
        let mut curved = false;
        for op in &path.ops {
            match *op {
                PathOp::MoveTo(x, y) | PathOp::LineTo(x, y) => points.push((x, y)),
                PathOp::CurveTo(x1, y1, x2, y2, x, y) => { curved = true; points.extend([(x1, y1), (x2, y2), (x, y)]); }
                PathOp::Rect(x, y, w, h) => points.extend([(x, y), (x + w, y + h)]),
                PathOp::Close => {}
            }
        }
        if points.is_empty() { return; }
        let mapped: Vec<_> = points.iter().map(|&(x, y)| ctm.transform_point(euclid::point2(x, y))).collect();
        let (left, right) = mapped.iter().fold((f64::MAX, f64::MIN), |(l, r), p| (l.min(p.x), r.max(p.x)));
        let (low, high) = mapped.iter().fold((f64::MAX, f64::MIN), |(l, h), p| (l.min(p.y), h.max(p.y)));
        let area = Area { x: left, y: self.top - high, width: right - left, height: high - low };
        if area.width.is_finite() && area.height.is_finite() { self.page_marks.push(Mark { area, curved, white }); }
    }
}

/// Ligatures become their letters and symbol-font bullets their Unicode
/// characters, so the text can be searched, spell-checked and edited.
fn normalize(text: &str) -> String {
    let text = model::replace_symbol_font_chars(text);
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\u{FB00}' => out.push_str("ff"),
            '\u{FB01}' => out.push_str("fi"),
            '\u{FB02}' => out.push_str("fl"),
            '\u{FB03}' => out.push_str("ffi"),
            '\u{FB04}' => out.push_str("ffl"),
            '\u{FB05}' | '\u{FB06}' => out.push_str("st"),
            '\u{00A0}' => out.push(' '),
            _ => out.push(c),
        }
    }
    out
}

fn span_style(style: &TextStyle) -> SpanStyle {
    let (family, bold, italic) = font_parts(style.font_name);
    const ITALIC_FLAG: i64 = 1 << 6;
    const FORCE_BOLD_FLAG: i64 = 1 << 18;
    let color = style.fill_rgb.filter(|&(r, g, b)| r.max(g).max(b) > 40);
    SpanStyle { family, bold: bold || style.font_flags & FORCE_BOLD_FLAG != 0, italic: italic || style.font_flags & ITALIC_FLAG != 0, color }
}

/// Family, bold and italic from a PDF font name such as
/// "ABCDEF+Arial-BoldItalicMT" or "TimesNewRomanPS-ItalicMT".
pub(crate) fn font_parts(name: &str) -> (String, bool, bool) {
    let name = match name.split_once('+') { Some((tag, rest)) if tag.len() == 6 && tag.chars().all(|c| c.is_ascii_uppercase()) => rest, _ => name };
    let lower = name.to_ascii_lowercase();
    let bold = ["bold", "black", "heavy", "semibold", "demi", "extrabold"].iter().any(|w| lower.contains(w));
    let (base, style_part) = match name.split_once([',', '-']) { Some((b, s)) => (b, s.to_ascii_lowercase()), None => (name, String::new()) };
    let italic = lower.contains("italic") || lower.contains("oblique") || style_part == "it" || style_part.ends_with("it") && style_part.len() <= 6;
    let mut base = base.trim_end_matches("PSMT").trim_end_matches("MT").trim_end_matches("PS").to_string();
    if base.contains(' ') { return (standard_family(&base), bold, italic); }
    // "TimesNewRoman" → "Times New Roman"; runs of capitals ("ITCAvant") stay together.
    let chars: Vec<char> = base.chars().collect();
    let mut spaced = String::new();
    for (i, &c) in chars.iter().enumerate() {
        let split = i > 0 && c.is_uppercase() && chars[i - 1].is_lowercase();
        if split { spaced.push(' '); }
        spaced.push(c);
    }
    base = spaced;
    (standard_family(&base), bold, italic)
}

/// The PDF standard fonts under the names of their usual installed
/// equivalents.
fn standard_family(family: &str) -> String {
    match family {
        "Helvetica" | "Arial Unicode" => "Arial".into(),
        "Times" | "Times Roman" | "Times-Roman" => "Times New Roman".into(),
        "Courier" => "Courier New".into(),
        "Zapf Dingbats" | "Symbol" => String::new(),
        _ => family.trim().to_string(),
    }
}

/// Glyphs become chunks: text on one baseline, split at wide gaps. Chunks
/// on one baseline that the content stream drew out of order are joined
/// afterwards, so the result does not depend on drawing order.
fn chunks(glyphs: Vec<Glyph>) -> Vec<Line> {
    struct Open { line: Line, end: f64, count: usize }
    let mut out: Vec<Line> = Vec::new();
    let mut current: Option<Open> = None;
    for g in glyphs {
        let whitespace = g.text.chars().all(char::is_whitespace);
        if let Some(open) = current.as_mut() {
            let size = open.line.size.max(g.size);
            let same_baseline = (g.y - open.line.y).abs() <= size * 0.45;
            let continues = same_baseline && g.x >= open.end - size * 0.8 && g.x <= open.end + size * CHUNK_GAP_EM;
            if continues {
                if whitespace {
                    push_text(&mut open.line, " ", &g.style, g.size);
                } else {
                    if g.x > open.end + g.size.min(open.line.size) * WORD_GAP_EM { push_text(&mut open.line, " ", &g.style, g.size); }
                    push_text(&mut open.line, &g.text, &g.style, g.size);
                    open.line.x1 = open.line.x1.max(g.end);
                    open.line.size = (open.line.size * open.count as f64 + g.size) / (open.count as f64 + 1.0);
                    open.count += 1;
                }
                open.end = g.end.max(open.end);
                continue;
            }
        }
        if let Some(open) = current.take() { finish(&mut out, open.line); }
        if !whitespace {
            let mut line = Line { text: String::new(), x0: g.x, x1: g.end, y: g.y, size: g.size, spans: Vec::new() };
            push_text(&mut line, &g.text, &g.style, g.size);
            current = Some(Open { line, end: g.end, count: 1 });
        }
    }
    if let Some(open) = current { finish(&mut out, open.line); }
    join_neighbours(out)
}

fn push_text(line: &mut Line, text: &str, style: &Rc<SpanStyle>, size: f64) {
    line.text.push_str(text);
    if let Some(last) = line.spans.last_mut() {
        let same = last.family == style.family && last.bold == style.bold && last.italic == style.italic
            && last.color == style.color && (last.size - size).abs() < 0.3;
        // Spaces join the current span whatever their font.
        if same || text == " " { last.text.push_str(text); return; }
    }
    line.spans.push(Span { text: text.into(), size, family: style.family.clone(), bold: style.bold, italic: style.italic, color: style.color, link: None });
}

fn finish(out: &mut Vec<Line>, mut line: Line) {
    collapse_spaces(&mut line);
    if !line.text.is_empty() { out.push(line); }
}

/// Trims the line and collapses runs of spaces, in the text and its spans alike.
fn collapse_spaces(line: &mut Line) {
    let mut previous_space = true;
    for span in &mut line.spans {
        let mut text = String::with_capacity(span.text.len());
        for c in span.text.chars() {
            let space = c.is_whitespace();
            if space && previous_space { continue; }
            text.push(if space { ' ' } else { c });
            previous_space = space;
        }
        span.text = text;
    }
    if let Some(last) = line.spans.iter_mut().rev().find(|s| !s.text.is_empty()) { last.text.truncate(last.text.trim_end().len()); }
    line.spans.retain(|s| !s.text.is_empty());
    line.text = line.spans.iter().map(|s| s.text.as_str()).collect();
}

/// Joins chunks on one baseline separated by less than a chunk gap.
fn join_neighbours(mut lines: Vec<Line>) -> Vec<Line> {
    lines.sort_by(|a, b| a.y.total_cmp(&b.y).then(a.x0.total_cmp(&b.x0)));
    let mut out: Vec<Line> = Vec::new();
    for line in lines {
        let near = out.iter_mut().rev().take(8).find(|o| {
            let size = o.size.max(line.size);
            (o.y - line.y).abs() <= size * 0.3 && line.x0 >= o.x1 - size * 0.3 && line.x0 <= o.x1 + size * CHUNK_GAP_EM
        });
        match near {
            Some(open) => {
                if line.x0 > open.x1 + open.size.min(line.size) * WORD_GAP_EM {
                    if let Some(last) = open.spans.last_mut() { last.text.push(' '); }
                }
                open.spans.extend(line.spans);
                open.x1 = open.x1.max(line.x1);
                open.text = open.spans.iter().map(|s| s.text.as_str()).collect();
            }
            None => out.push(line),
        }
    }
    out.sort_by(|a, b| a.y.total_cmp(&b.y).then(a.x0.total_cmp(&b.x0)));
    out
}

fn number(object: &Object) -> Option<f64> {
    match object { Object::Integer(i) => Some(*i as f64), Object::Real(r) => Some(*r as f64), _ => None }
}

/// The web links of each page, and the spans each covers.
pub(crate) fn attach_links(doc: &pdf_extract::Document, pages: &mut [PageText], tops: &[f64]) {
    let deref = |object: &Object| -> Option<Object> {
        match object { Object::Reference(id) => doc.get_object(*id).ok().cloned(), other => Some(other.clone()) }
    };
    for (index, (_, page_id)) in doc.get_pages().into_iter().enumerate() {
        let (Some(page), Some(&top)) = (pages.get_mut(index), tops.get(index)) else { break; };
        let Ok(dict) = doc.get_dictionary(page_id) else { continue; };
        let Some(Object::Array(annotations)) = dict.get(b"Annots").ok().and_then(deref) else { continue; };
        for annotation in annotations.iter().filter_map(deref) {
            let Ok(annotation) = annotation.as_dict() else { continue; };
            if annotation.get(b"Subtype").ok().and_then(|s| s.as_name().ok()) != Some(b"Link") { continue; }
            let Some(Object::Dictionary(action)) = annotation.get(b"A").ok().and_then(deref) else { continue; };
            if action.get(b"S").ok().and_then(|s| s.as_name().ok()) != Some(b"URI") { continue; }
            let Some(uri) = action.get(b"URI").ok().and_then(deref).and_then(|u| u.as_str().ok().map(|b| String::from_utf8_lossy(b).trim().to_string())) else { continue; };
            let Some(Object::Array(rect)) = annotation.get(b"Rect").ok().and_then(deref) else { continue; };
            let values: Vec<f64> = rect.iter().filter_map(|o| deref(o).as_ref().and_then(number)).collect();
            let [x1, y1, x2, y2] = values[..] else { continue; };
            if uri.is_empty() { continue; }
            page.links.push(Link { area: Area { x: x1.min(x2), y: top - y1.max(y2), width: (x2 - x1).abs(), height: (y2 - y1).abs() }, uri });
        }
        // A span belongs to a link when its middle lies inside the link area.
        for line in &mut page.lines {
            let total: usize = line.spans.iter().map(|s| s.text.chars().count()).sum::<usize>().max(1);
            let mut done = 0usize;
            for span in &mut line.spans {
                let count = span.text.chars().count();
                let middle = line.x0 + (line.x1 - line.x0) * (done as f64 + count as f64 / 2.0) / total as f64;
                span.link = page.links.iter().find(|l| l.area.contains(middle, line.y - line.size * 0.3)).map(|l| Arc::from(l.uri.as_str()));
                done += count;
            }
        }
    }
}

