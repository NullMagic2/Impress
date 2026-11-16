//! Language-aware Knuth/Liang-style hyphenation using embedded patterns.

use hypher::{hyphenate_bounded, Lang};

/// Return legal UTF-8 byte offsets inside `word` where a discretionary hyphen
/// may be inserted. Unsupported/unknown language tags simply return no breaks.
pub fn hyphenation_points(word: &str, language_tag: Option<&str>) -> Vec<usize> {
    if word.chars().count() < 5 || !word.chars().all(char::is_alphabetic) { return Vec::new(); }
    let Some(tag) = language_tag else { return Vec::new(); };
    let primary = tag.split(['-', '_']).next().unwrap_or(tag).to_ascii_lowercase();
    let bytes = primary.as_bytes();
    if bytes.len() != 2 { return Vec::new(); }
    let Some(language) = Lang::from_iso([bytes[0], bytes[1]]) else { return Vec::new(); };

    let mut points = Vec::new();
    let mut offset = 0usize;
    let mut syllables = hyphenate_bounded(word, language, 2, 2).peekable();
    while let Some(syllable) = syllables.next() {
        offset += syllable.len();
        if syllables.peek().is_some() && offset > 0 && offset < word.len() && word.is_char_boundary(offset) {
            points.push(offset);
        }
    }
    points
}

#[cfg(test)] #[path = "../tests/unit/hyphen.rs"] mod tests;
