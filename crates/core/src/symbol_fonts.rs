//! Word writes bullets and many symbols as private-use characters
//! (U+F020–U+F0FF) that only look right in the Symbol and Wingdings fonts. On
//! systems without those fonts they show as empty boxes, so they are replaced
//! by the Unicode characters they depict. Every replacement keeps the UTF-8
//! length of the original, so text offsets (caret, selection) do not move.
use std::borrow::Cow;

/// Private-use range where Word places symbol-font characters.
const SYMBOL_FONT_RANGE: std::ops::RangeInclusive<u32> = 0xF020..=0xF0FF;
/// Shown for symbol-font characters without a known equivalent.
const UNKNOWN_SYMBOL: char = '•';

/// Word's common bullet characters (Symbol and Wingdings) and what they show.
const SYMBOL_FONT_CHARACTERS: &[(u32, char)] = &[
    (0xF0B7, '•'), // Symbol: bullet
    (0xF0A7, '▪'), // Wingdings: small square (Word's third list level)
    (0xF0D8, '➢'), // Wingdings: arrowhead
    (0xF0FC, '✓'), // Wingdings: check mark
    (0xF076, '❖'), // Wingdings: diamond with dots
    (0xF06E, '■'), // Wingdings: black square
    (0xF06C, '●'), // Wingdings: black circle
    (0xF071, '❑'), // Wingdings: shadowed square
    (0xF075, '◆'), // Wingdings: black diamond
    (0xF0E0, '➔'), // Wingdings: arrow
    (0xF0AE, '→'), // Symbol: right arrow
    (0xF02D, '−'), // Symbol: minus
];

/// The Unicode character a symbol-font character depicts, or `ch` itself.
pub fn symbol_font_fallback(ch: char) -> char {
    let code = u32::from(ch);
    if !SYMBOL_FONT_RANGE.contains(&code) { return ch; }
    SYMBOL_FONT_CHARACTERS.iter().find(|(private, _)| *private == code).map_or(UNKNOWN_SYMBOL, |(_, shown)| *shown)
}

/// `text` with every symbol-font character replaced by what it depicts.
pub fn replace_symbol_font_chars(text: &str) -> Cow<'_, str> {
    if !text.chars().any(|ch| SYMBOL_FONT_RANGE.contains(&u32::from(ch))) { return Cow::Borrowed(text); }
    Cow::Owned(text.chars().map(symbol_font_fallback).collect())
}

#[cfg(test)] #[path = "../tests/unit/symbol_fonts.rs"] mod tests;
