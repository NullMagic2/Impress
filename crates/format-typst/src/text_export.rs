//! Readable Typst for the source pane and .typ export, written as a person
//! would write it. The document's defaults (its Normal style) are set once at
//! the top; each paragraph then states only how it differs from them, and
//! each run only how it differs from its paragraph. Text is escaped Typst
//! markup (never code): bold and italic runs become `*…*` and `_…_` where
//! that reads naturally. Paragraph bodies are indented inside their blocks.
use model::{CharacterProperties, Inline, LigatureMode, Paragraph, ParagraphAlignment,
    ParagraphDirection, ParagraphProperties, StyleId, StyleSheet, STYLE_HEADING_1, STYLE_NORMAL, STYLE_TITLE};
use std::collections::BTreeSet;

/// Named arguments of a Typst call, in the order they are written.
type Args = Vec<(&'static str, String)>;

/// Calls longer than this are written one argument per line.
const LINE_WIDTH: usize = 90;
/// One level of indentation in the generated source: a tab.
const INDENT: &str = "\t";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role { Body, Heading(u8), Title }

fn paragraph_role(styles: &StyleSheet, id: &StyleId) -> Role {
    let mut current = Some(id);
    let mut seen = BTreeSet::new();
    while let Some(id) = current {
        if !seen.insert(id.clone()) { break; }
        let style = styles.get(id);
        let name = style.map(|s| s.name.to_lowercase().split_whitespace().collect::<String>()).unwrap_or_default();
        if id.0 == STYLE_TITLE || name == "title" { return Role::Title; }
        if id.0 == STYLE_HEADING_1 { return Role::Heading(1); }
        if let Some(level) = name.strip_prefix("heading").and_then(|n| n.parse::<u8>().ok()).filter(|n| (1..=9).contains(n)) {
            return Role::Heading(level);
        }
        current = style.and_then(|s| s.based_on.as_ref());
    }
    Role::Body
}

/// What a document's paragraphs inherit unless they say otherwise.
pub(crate) struct Defaults { text: Args, paragraph: Args, align: &'static str, block: (f32, f32) }

impl Defaults {
    /// What most of the document's paragraphs look like, so that the most
    /// paragraphs need no settings of their own.
    pub(crate) fn for_document(document: &model::Document) -> Self {
        Self::for_paragraphs(&model::document_paragraphs(document), &document.styles)
    }

    /// What most of `paragraphs` look like.
    pub(crate) fn for_paragraphs(paragraphs: &[&Paragraph], styles: &StyleSheet) -> Self {
        if paragraphs.is_empty() { return Self::of(styles); }
        let mut text = Vec::new(); let mut paragraph = Vec::new(); let mut align = Vec::new(); let mut block = Vec::new();
        for p in paragraphs {
            let normal = StyleId::from(STYLE_NORMAL);
            let id = p.style.as_ref().unwrap_or(&normal);
            if paragraph_role(styles, id) != Role::Body { continue; }
            let resolved = styles.resolve_paragraph_with_direct(Some(id), &p.properties);
            text.push(text_args(&effective_text(p, styles, id)));
            paragraph.push(paragraph_text_args(&resolved));
            align.push(alignment(&resolved));
            block.push(block_spacing(&resolved));
        }
        if text.is_empty() { return Self::of(styles); }
        Defaults { text: most_common(&text), paragraph: most_common(&paragraph), align: most_common(&align),
            block: most_common(&block) }
    }

    pub(crate) fn of(styles: &StyleSheet) -> Self {
        let normal = StyleId::from(STYLE_NORMAL);
        let p = styles.resolve_paragraph(Some(&normal));
        Defaults { text: text_args(&styles.resolve_character(Some(&normal))), paragraph: paragraph_text_args(&p),
            align: alignment(&p), block: block_spacing(&p) }
    }

    /// The settings these defaults add to `outer`'s, as `#set` lines.
    pub(crate) fn rules_within(&self, outer: &Defaults) -> Vec<String> {
        let mut rules = Vec::new();
        let mut text = differences(&self.text, &outer.text);
        text.extend(differences(&self.paragraph, &outer.paragraph));
        if !text.is_empty() { rules.push(call("#set text", &text)); }
        if self.align != outer.align { rules.push(format!("#set align({})", self.align)); }
        if self.block != outer.block { rules.push(call("#set block", &[("above", points(self.block.0)), ("below", points(self.block.1))])); }
        rules
    }

    /// The document's text, paragraph and spacing settings, set once.
    pub(crate) fn preamble(&self) -> String {
        let mut text = self.text.clone();
        text.extend(self.paragraph.iter().cloned());
        let mut out = vec![call("#set text", &text)];
        out.push(call("#set par", &[("justify", "false".into()), ("first-line-indent", indent_value(0.0))]));
        if self.align != "left" { out.push(format!("#set align({})", self.align)); }
        out.push(call("#set block", &[("above", points(self.block.0)), ("below", points(self.block.1))]));
        out.join("\n")
    }
}

/// The value that occurs most often (the first of equals).
fn most_common<T: Clone + PartialEq>(values: &[T]) -> T {
    let mut best = (0, &values[0]);
    for candidate in values {
        let count = values.iter().filter(|v| *v == candidate).count();
        if count > best.0 { best = (count, candidate); }
    }
    best.1.clone()
}

/// A paragraph's text settings: those of all its text when it is formatted
/// alike throughout, else its style's.
fn effective_text(paragraph: &Paragraph, styles: &StyleSheet, id: &StyleId) -> CharacterProperties {
    let mut runs = Vec::new();
    collect_runs(&paragraph.content, &mut runs);
    let resolved: Vec<CharacterProperties> = runs.iter().map(|r| styles.resolve_run(Some(id), r.style.as_ref(), &r.properties)).collect();
    match resolved.first() {
        Some(first) if resolved.iter().all(|r| r == first) => first.clone(),
        _ => styles.resolve_character(Some(id)),
    }
}

fn collect_runs<'a>(items: &'a [Inline], out: &mut Vec<&'a model::TextRun>) {
    for item in items {
        match item {
            Inline::Text(run) if !run.text.trim().is_empty() => out.push(run),
            Inline::Hyperlink(link) => collect_runs(&link.content, out),
            _ => {}
        }
    }
}

/// A Typst call: `name(a: 1, b: 2)`, or one argument per line when long.
fn call(name: &str, args: &[(&'static str, String)]) -> String {
    let inline = format!("{name}({})", args.iter().map(|(k, v)| format!("{k}: {v}")).collect::<Vec<_>>().join(", "));
    if inline.len() <= LINE_WIDTH { return inline; }
    let lines: String = args.iter().map(|(k, v)| format!("{INDENT}{k}: {v},\n")).collect();
    format!("{name}(\n{lines})")
}

/// The arguments of `args` that `base` does not already give the same value.
fn differences(args: &Args, base: &Args) -> Args {
    args.iter().filter(|(key, value)| base.iter().find(|(k, _)| k == key).is_none_or(|(_, v)| v != value)).cloned().collect()
}

/// Every line of `text` indented one level.
pub(crate) fn indented(text: &str) -> String {
    text.lines().map(|line| if line.is_empty() { String::new() } else { format!("{INDENT}{line}") }).collect::<Vec<_>>().join("\n")
}

fn points(value: f32) -> String { format!("{}pt", (value * 100.0).round() / 100.0) }
fn indent_value(mm: f32) -> String { format!("(amount: {}mm, all: true)", (mm * 100.0).round() / 100.0) }
fn finite(value: Option<f32>, default: f32) -> f32 { value.filter(|n| n.is_finite()).unwrap_or(default) }

fn alignment(p: &ParagraphProperties) -> &'static str {
    match p.alignment.unwrap_or(ParagraphAlignment::Left) {
        ParagraphAlignment::Left | ParagraphAlignment::Justify => "left",
        ParagraphAlignment::Center => "center", ParagraphAlignment::Right => "right",
    }
}

fn block_spacing(p: &ParagraphProperties) -> (f32, f32) {
    (finite(p.space_before_pt, 0.0).max(0.0), finite(p.space_after_pt, 0.0).max(0.0))
}

/// Paragraph settings that Typst keeps on `text`: hyphenation and direction.
fn paragraph_text_args(p: &ParagraphProperties) -> Args {
    let mut args = Args::new();
    if let Some(hyphenate) = p.hyphenation { args.push(("hyphenate", hyphenate.to_string())); }
    if let Some(direction) = p.direction {
        args.push(("dir", match direction { ParagraphDirection::LeftToRight => "ltr", ParagraphDirection::RightToLeft => "rtl", ParagraphDirection::Auto => "auto" }.into()));
    }
    args
}

fn text_args(properties: &CharacterProperties) -> Args {
    let size = finite(properties.font_size_pt, 11.0).clamp(1.0, 400.0);
    let font = properties.font_family.as_deref().filter(|f| !f.trim().is_empty()).unwrap_or("Linux Libertine G");
    let t = properties.typography.clone().unwrap_or_default();
    let ligatures = t.ligatures != LigatureMode::None;
    let mut args: Args = vec![
        ("font", typst_string(font)),
        ("size", points(size)),
        ("weight", if properties.bold.unwrap_or(false) { "\"bold\"" } else { "\"regular\"" }.into()),
        ("style", if properties.italic.unwrap_or(false) { "\"italic\"" } else { "\"normal\"" }.into()),
        ("fill", format!("rgb({})", typst_string(&hex_color(properties.color.as_deref()).unwrap_or("#000000").to_ascii_lowercase()))),
        ("kerning", t.kerning.to_string()),
        ("ligatures", ligatures.to_string()),
        ("discretionary-ligatures", (t.ligatures == LigatureMode::Discretionary).to_string()),
        ("features", format!("(clig: {}, calt: {}, smcp: {}, onum: {}, tnum: {})", u8::from(ligatures && t.contextual_ligatures),
            u8::from(t.contextual_alternates), u8::from(t.small_caps), u8::from(t.oldstyle_figures), u8::from(t.tabular_figures))),
    ];
    if let Some(language) = properties.language.as_deref().filter(|s| !s.trim().is_empty()) {
        // A BCP-47 tag: Typst takes the language and region separately; a
        // script subtag is not a region.
        let parts: Vec<_> = language.split('-').collect();
        args.push(("lang", typst_string(&parts[0].to_ascii_lowercase())));
        if let Some(region) = parts.iter().skip(1).find(|s| s.len() == 2 && s.bytes().all(|c| c.is_ascii_alphabetic())) {
            args.push(("region", typst_string(&region.to_ascii_uppercase())));
        }
    }
    args
}

pub(crate) fn paragraph_to_typst(paragraph: &Paragraph, styles: &StyleSheet) -> String {
    paragraph_with_defaults(paragraph, styles, &Defaults::of(styles))
}

pub(crate) fn paragraph_with_defaults(paragraph: &Paragraph, styles: &StyleSheet, defaults: &Defaults) -> String {
    let normal = StyleId::from(STYLE_NORMAL);
    let id = paragraph.style.as_ref().unwrap_or(&normal);
    let base = effective_text(paragraph, styles, id);
    let base_args = text_args(&base);
    let p = styles.resolve_paragraph_with_direct(Some(id), &paragraph.properties);
    let role = paragraph_role(styles, id);

    // The block: its spacing, and whether it keeps with the next or together.
    let (above, below) = block_spacing(&p);
    let mut block = Args::new();
    if above != defaults.block.0 { block.push(("above", points(above))); }
    if below != defaults.block.1 { block.push(("below", points(below))); }
    if p.keep_with_next.unwrap_or(matches!(role, Role::Heading(_))) { block.push(("sticky", "true".into())); }
    if p.keep_lines_together.unwrap_or(false) { block.push(("breakable", "false".into())); }

    let mut body = Vec::new();
    let mut paragraph_args = differences(&paragraph_text_args(&p), &defaults.paragraph);
    let align = alignment(&p);
    if align != defaults.align { body.push(format!("#set align({align})")); }
    let mut par = Args::new();
    if p.alignment == Some(ParagraphAlignment::Justify) { par.push(("justify", "true".into())); }
    let first_line = finite(p.first_line_indent_mm, 0.0);
    if first_line != 0.0 { par.push(("first-line-indent", indent_value(first_line))); }
    if !par.is_empty() { body.push(call("#set par", &par)); }

    let content = inlines_to_typst(&paragraph.content, styles, id, &base_args);
    match role {
        // Typst's headings and titles set their own size and weight, so those
        // are always given; spacing comes from the paragraph's block.
        Role::Heading(_) | Role::Title => {
            let element = if role == Role::Title { "title" } else { "heading" };
            let mut text = differences(&base_args, &defaults.text);
            for key in ["size", "weight"] {
                if !text.iter().any(|(k, _)| *k == key) { if let Some(arg) = base_args.iter().find(|(k, _)| *k == key) { text.push(arg.clone()); } }
            }
            text.append(&mut paragraph_args);
            body.push(call(&format!("#show {element}: set text"), &text));
            body.push(format!("#show {element}: set block(above: 0pt, below: 0pt)"));
            body.push(match role {
                Role::Heading(level) => format!("#heading(level: {level}, numbering: none)[{content}]"),
                _ => format!("#title[{content}]"),
            });
        }
        Role::Body => {
            let mut text = differences(&base_args, &defaults.text);
            text.append(&mut paragraph_args);
            // Bold or italic throughout reads best as markup around the text.
            let content = match emphasis(&text) {
                Some((open, close)) if !content.is_empty() => { text.clear(); format!("{open}{content}{close}") }
                _ => content,
            };
            // An empty paragraph has no text for settings to apply to.
            if content.is_empty() { body.clear(); }
            else {
                if !text.is_empty() { body.push(call("#set text", &text)); }
                body.push(content);
            }
        }
    }
    let opening = if block.is_empty() { "#block".to_string() } else { call("#block", &block) };
    if body.is_empty() { return format!("{opening}[]"); }
    format!("{opening}[\n{}\n]", indented(&body.join("\n")))
}

fn inlines_to_typst(items: &[Inline], styles: &StyleSheet, paragraph_style: &StyleId, base: &Args) -> String {
    let mut out = String::new();
    let mut index = 0;
    while let Some(item) = items.get(index) {
        index += 1;
        match item {
            Inline::Text(run) => {
                if run.text.is_empty() { continue; }
                let properties = styles.resolve_run(Some(paragraph_style), run.style.as_ref(), &run.properties);
                let mut text = run.text.clone();
                // Runs split by DOCX or typing but formatted alike are one.
                while let Some(Inline::Text(next)) = items.get(index) {
                    if next.text.is_empty() { index += 1; continue; }
                    if properties != styles.resolve_run(Some(paragraph_style), next.style.as_ref(), &next.properties) { break; }
                    text.push_str(&next.text); index += 1;
                }
                let before = out.chars().last();
                let after = items.get(index).and_then(first_char);
                out.push_str(&styled_text(&text, &properties, base, before, after));
            }
            Inline::Hyperlink(link) => {
                out.push_str(&format!("#link({})[{}]", typst_string(&link.target), inlines_to_typst(&link.content, styles, paragraph_style, base)));
            }
            Inline::Equation(eq) => out.push_str(&crate::equation_to_typst(eq)),
            Inline::Image(image) => out.push_str(&escape_markup(&format!("[image:{}]", image.resource), out.is_empty())),
            Inline::EmbeddedObject(object) => out.push_str(&escape_markup(&format!("[embedded object:{}]", object.program.as_deref().unwrap_or("Package")), out.is_empty())),
            Inline::TextBox(boxed) => {
                let mut paragraphs = Vec::new();
                model::visit_paragraphs(&boxed.blocks, &mut paragraphs);
                let text = paragraphs.iter().map(|p| model::inline_text(&p.content)).collect::<Vec<_>>().join("\n");
                out.push_str(&escape_markup(&text, out.is_empty()));
            }
            Inline::Field(field) => match &field.kind {
                model::FieldKind::PageNumber => out.push_str("#context counter(page).display()"),
                model::FieldKind::PageCount => out.push_str("#context counter(page).final().first()"),
                _ => out.push_str(&escape_markup(field.display_text.as_deref().unwrap_or(""), out.is_empty())),
            },
            Inline::PaginationBreak { break_kind, .. } => out.push_str(match break_kind { model::PaginationBreakKind::Page => "#pagebreak()", model::PaginationBreakKind::Column => "#colbreak()" }),
            Inline::LineBreak { .. } => out.push_str("#linebreak()"),
            Inline::Tab { .. } => out.push_str("#h(2em)"),
        }
    }
    out
}

/// The markup for text that differs from its surroundings only by being bold,
/// italic or both.
fn emphasis(changed: &Args) -> Option<(&'static str, &'static str)> {
    let is = |key: &str, value: &str| changed.iter().any(|(k, v)| *k == key && v == value);
    match changed.len() {
        1 if is("weight", "\"bold\"") => Some(("*", "*")),
        1 if is("style", "\"italic\"") => Some(("_", "_")),
        2 if is("weight", "\"bold\"") && is("style", "\"italic\"") => Some(("*_", "_*")),
        _ => None,
    }
}

/// A paragraph that is only its text, without settings: that text alone,
/// for a table cell.
pub(crate) fn bare_content(block: &str) -> Option<&str> {
    if block == "#block[]" { return Some(""); }
    let inner = block.strip_prefix("#block[\n")?.strip_suffix("\n]")?;
    let line = inner.strip_prefix(INDENT)?;
    (!line.contains('\n') && !line.starts_with("#set ") && !line.starts_with("#show ")).then_some(line)
}

fn first_char(item: &Inline) -> Option<char> {
    match item { Inline::Text(run) => run.text.chars().next(), _ => None }
}

/// A run as markup: plain where it looks like its paragraph, `*bold*` and
/// `_italic_` where that is all that differs and reads cleanly, else
/// `#text(…)[…]` with just the differences.
fn styled_text(text: &str, properties: &CharacterProperties, base: &Args, before: Option<char>, after: Option<char>) -> String {
    let changed = differences(&text_args(properties), base);
    if changed.is_empty() && properties.underline != Some(true) && hex_color(properties.highlight.as_deref()).is_none() {
        return escape_markup(text, before.is_none());
    }
    // Spaces at either end stay outside the formatting, as a person types it.
    let core = text.trim_matches(char::is_whitespace);
    if core.is_empty() { return escape_markup(text, before.is_none()); }
    let lead = &text[..text.len() - text.trim_start_matches(char::is_whitespace).len()];
    let trail = &text[text.trim_end_matches(char::is_whitespace).len()..];
    let before = if lead.is_empty() { before } else { Some(' ') };
    let after = if trail.is_empty() { after } else { Some(' ') };
    let markup = escape_markup(core, before.is_none());
    // Delimiters read cleanly only around whole words.
    let clean = !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric);
    let mut out = match emphasis(&changed) {
        _ if changed.is_empty() => markup,
        Some((open, close)) if clean => format!("{open}{markup}{close}"),
        _ => format!("{}[{markup}]", call("#text", &changed)),
    };
    if properties.underline == Some(true) { out = format!("#underline[{out}]"); }
    if let Some(color) = hex_color(properties.highlight.as_deref()) {
        out = format!("#highlight(fill: rgb({}))[{out}]", typst_string(&color.to_ascii_lowercase()));
    }
    format!("{}{out}{}", escape_markup(lead, false), escape_markup(trail, false))
}

/// Text as Typst markup that shows exactly these characters: markup symbols
/// are escaped, and so are those that start a heading or list at the start
/// of a line, and straight quotes (which markup would curl).
fn escape_markup(text: &str, line_start: bool) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut at_start = line_start;
    for (i, &c) in chars.iter().enumerate() {
        let next = chars.get(i + 1).copied();
        match c {
            '\\' | '#' | '*' | '_' | '`' | '$' | '[' | ']' | '<' | '>' | '@' | '~' | '"' | '\'' => { out.push('\\'); out.push(c); }
            '/' if next == Some('/') || at_start => { out.push('\\'); out.push(c); }
            '-' if next == Some('-') || next == Some('?') || at_start => { out.push('\\'); out.push(c); }
            '=' | '+' if at_start => { out.push('\\'); out.push(c); }
            '.' if at_start_number(&chars, i, line_start) => { out.push('\\'); out.push(c); }
            '\n' => out.push_str("#linebreak()"),
            '\r' => {}
            c => out.push(c),
        }
        if !c.is_whitespace() { at_start = false; }
    }
    out
}

/// Whether the `.` at `index` ends a number that opens the line ("1." starts
/// a numbered list in markup).
fn at_start_number(chars: &[char], index: usize, line_start: bool) -> bool {
    line_start && index > 0 && chars[..index].iter().all(|c| c.is_ascii_digit())
}

fn hex_color(value: Option<&str>) -> Option<&str> {
    value.filter(|s| s.len() == 7 && s.starts_with('#') && s.as_bytes()[1..].iter().all(|b| b.is_ascii_hexdigit()))
}

pub(crate) fn typst_string(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"), '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"), '\r' => out.push_str("\\r"), '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{{{:x}}}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"'); out
}

#[cfg(test)] #[path = "../tests/unit/text_export.rs"] mod tests;
