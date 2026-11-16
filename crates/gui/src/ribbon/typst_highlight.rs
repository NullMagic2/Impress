//! Syntax highlighting for the Typst source view: a small tokenizer for the
//! code Impress generates (and ordinary Typst), and its colours applied to the
//! read-only text control as styled ranges.
use wxdragon::{widgets::textctrl::{TextAttr, TextCtrl}, window::WxWidget, Colour};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind { Comment, Text, Number, Keyword, Function, Argument, Constant }

/// Colours of a light code editor.
fn colour(kind: Kind) -> Colour {
    match kind {
        Kind::Comment => Colour::rgb(0x3a, 0x80, 0x3a),
        Kind::Text => Colour::rgb(0xa3, 0x15, 0x15),
        Kind::Number => Colour::rgb(0x09, 0x86, 0x58),
        Kind::Keyword => Colour::rgb(0x8f, 0x2b, 0xb8),
        Kind::Function => Colour::rgb(0x79, 0x5e, 0x26),
        Kind::Argument => Colour::rgb(0x00, 0x56, 0x9e),
        Kind::Constant => Colour::rgb(0x00, 0x3b, 0xd6),
    }
}

const KEYWORDS: [&str; 12] = ["set", "show", "let", "import", "include", "if", "else", "for", "while", "return", "context", "in"];
const CONSTANTS: [&str; 4] = ["true", "false", "none", "auto"];
/// More tokens than this are left plain, so a very long source stays quick.
const MAX_TOKENS: usize = 80_000;

/// Where the tokenizer is, as Typst reads it: text (markup), or code after a
/// `#`. Code started by a rule (`#set`, `#show`, `#let`…) runs to the end of
/// its line; code started by a call ends with the call.
#[derive(Clone, Copy, PartialEq)]
enum Mode { Markup, Code { depth: u32, line: bool } }

/// The coloured spans of `source`: (start, end, kind) in characters, as text
/// controls count positions. Only code is coloured; the document's own text
/// (markup) stays plain, whatever words it contains.
pub(super) fn tokens(source: &str) -> Vec<(usize, usize, Kind)> {
    let chars: Vec<char> = source.chars().collect();
    let mut out = Vec::new();
    let mut stack = vec![Mode::Markup];
    let mut i = 0;
    let ident = |c: char| c.is_alphanumeric() || c == '_' || c == '-';
    // After a call's `)` or `]`: the call goes on with `(` or `[`, else ends.
    let ends_call = |stack: &mut Vec<Mode>, next: Option<char>| {
        if let Some(&Mode::Code { depth: 0, line: false }) = stack.last() {
            if !matches!(next, Some('(' | '[')) { stack.pop(); }
        }
    };
    while i < chars.len() && out.len() < MAX_TOKENS {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        match *stack.last().unwrap_or(&Mode::Markup) {
            Mode::Markup => match c {
                '\\' => i += 2,
                '/' if next == Some('/') => {
                    let start = i;
                    while i < chars.len() && chars[i] != '\n' { i += 1; }
                    out.push((start, i, Kind::Comment));
                }
                '$' => { i += 1; while i < chars.len() && chars[i] != '$' { i += 1; } i += 1; }
                '#' if next.is_some_and(|n| n.is_alphabetic() || n == '_') => {
                    let start = i;
                    i += 1;
                    let word_start = i;
                    while i < chars.len() && (ident(chars[i]) || chars[i] == '.') { i += 1; }
                    let word: String = chars[word_start..i].iter().collect();
                    let keyword = KEYWORDS.contains(&word.as_str());
                    out.push((start, i, if keyword { Kind::Keyword } else { Kind::Function }));
                    if keyword { stack.push(Mode::Code { depth: 0, line: true }); }
                    else if matches!(chars.get(i), Some('(' | '[')) { stack.push(Mode::Code { depth: 0, line: false }); }
                }
                '#' if next == Some('[') => { stack.push(Mode::Code { depth: 0, line: false }); i += 1; }
                ']' if stack.len() > 1 => { stack.pop(); i += 1; ends_call(&mut stack, chars.get(i).copied()); }
                _ => i += 1,
            },
            Mode::Code { depth, line } => match c {
                '/' if next == Some('/') => {
                    let start = i;
                    while i < chars.len() && chars[i] != '\n' { i += 1; }
                    out.push((start, i, Kind::Comment));
                }
                '"' => {
                    let start = i;
                    i += 1;
                    while i < chars.len() && chars[i] != '"' && chars[i] != '\n' { if chars[i] == '\\' { i += 1; } i += 1; }
                    i = (i + 1).min(chars.len());
                    out.push((start, i, Kind::Text));
                }
                '(' => { if let Some(Mode::Code { depth, .. }) = stack.last_mut() { *depth += 1; } i += 1; }
                ')' => {
                    if let Some(Mode::Code { depth, .. }) = stack.last_mut() { *depth = depth.saturating_sub(1); }
                    i += 1;
                    ends_call(&mut stack, chars.get(i).copied());
                }
                '[' => { stack.push(Mode::Markup); i += 1; }
                '\n' if line && depth == 0 => { stack.pop(); i += 1; }
                _ if c.is_ascii_digit() && (i == 0 || !ident(chars[i - 1])) => {
                    let start = i;
                    while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') { i += 1; }
                    // A unit: pt, mm, em, %, fr…
                    while i < chars.len() && (chars[i].is_ascii_alphabetic() || chars[i] == '%') { i += 1; }
                    out.push((start, i, Kind::Number));
                }
                _ if (c.is_alphabetic() || c == '_') && (i == 0 || !ident(chars[i - 1])) => {
                    let start = i;
                    while i < chars.len() && ident(chars[i]) { i += 1; }
                    let word: String = chars[start..i].iter().collect();
                    let after = chars[i..].iter().find(|c| **c != ' ').copied();
                    if CONSTANTS.contains(&word.as_str()) { out.push((start, i, Kind::Constant)); }
                    else if KEYWORDS.contains(&word.as_str()) { out.push((start, i, Kind::Keyword)); }
                    else if matches!(chars.get(i), Some('(' | '[')) { out.push((start, i, Kind::Function)); }
                    else if after == Some(':') { out.push((start, i, Kind::Argument)); }
                }
                _ => i += 1,
            },
        }
    }
    out
}

/// Coloured spans applied per tick: setting many styles at once froze the
/// window (a third of a second for a long document).
const BATCH: usize = 3_000;

thread_local! {
    /// Spans still to colour, from the top so the visible part comes first.
    static PENDING: std::cell::RefCell<Option<(Vec<(usize, usize, Kind)>, usize)>> = const { std::cell::RefCell::new(None) };
}

/// The system's monospaced font, so indentation and alignment read as code.
/// (The layout file names Consolas, which Linux does not have.)
pub(super) fn use_code_font(control: TextCtrl) {
    use wxdragon::font::{Font, FontFamily, FontStyle, FontWeight};
    if let Some(font) = Font::new_with_details(10, FontFamily::Teletype as i32, FontStyle::Normal as i32, FontWeight::Normal as i32, false, "Monospace") {
        control.set_font(&font);
    }
    // The generated source is indented with tabs: show each as 4 characters.
    let width = control.get_text_extent("0000").width;
    crate::gtk_native::set_tab_width(&control, width);
}

/// Shows `source` in the control at once; its colours follow on the next ticks.
pub(super) fn show(control: TextCtrl, source: &str) {
    control.change_value(source);
    PENDING.with(|pending| pending.replace(Some((tokens(source), 0))));
    poll(control);
}

/// Colours the next batch of spans.
pub(super) fn poll(control: TextCtrl) {
    PENDING.with(|pending| {
        let mut pending = pending.borrow_mut();
        let Some((spans, next)) = pending.as_mut() else { return; };
        let styles: Vec<(Kind, TextAttr)> = [Kind::Comment, Kind::Text, Kind::Number, Kind::Keyword, Kind::Function, Kind::Argument, Kind::Constant]
            .into_iter().map(|kind| { let mut attr = TextAttr::new(); attr.set_text_colour(colour(kind)); (kind, attr) }).collect();
        let end = (*next + BATCH).min(spans.len());
        control.freeze();
        for &(start, stop, kind) in &spans[*next..end] {
            if let Some((_, attr)) = styles.iter().find(|(k, _)| *k == kind) { control.set_style(start as i64, stop as i64, attr); }
        }
        control.thaw();
        *next = end;
        if *next >= spans.len() { *pending = None; }
    });
}

