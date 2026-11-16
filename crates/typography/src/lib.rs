//! High-quality text composition for Impress.
//!
//! The design deliberately separates four stages:
//! 1. font resolution/fallback,
//! 2. OpenType shaping (GSUB/GPOS, ligatures, kerning, marks),
//! 3. paragraph composition (Knuth–Plass inspired line breaking),
//! 4. rendering (a GUI/print backend consumes positioned glyphs).
//!
//! Equations do not pass through this module: they are first-class semantic
//! nodes rendered by the Typst equation engine.

mod break_lines;
mod fonts;
mod embedding;
pub use embedding::*;
mod portable_embedding;
pub use portable_embedding::compatible_truetype_font;
mod microtype;
mod shaping;
mod optical_kerning;
mod bidi;
mod hyphen;
mod render;

pub use break_lines::*;
pub use fonts::*;
pub use microtype::*;
pub use shaping::*;
pub use optical_kerning::{font_pair_kerning,optical_pair_adjustment};
pub use bidi::*;
pub use hyphen::*;
pub use render::*;
/// Parsed font face lent out by `FontCatalog::with_face`; dereferences to `ttf_parser::Face`.
pub use rustybuzz::Face as ShapingFace;
