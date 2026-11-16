use rustybuzz::{BufferClusterLevel, Direction, Face, Feature, UnicodeBuffer};
use model::{KerningMode, LigatureMode, OpenTypeTypography};
use std::str::FromStr;
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextDirection { LeftToRight, RightToLeft }

#[derive(Clone, Debug)]
pub struct ShapeOptions {
    pub size_pt: f32,
    pub direction: TextDirection,
    pub language: Option<String>,
    pub typography: OpenTypeTypography,
}

impl Default for ShapeOptions {
    fn default() -> Self {
        Self {
            size_pt: 11.0,
            direction: TextDirection::LeftToRight,
            language: None,
            typography: OpenTypeTypography::default(),
        }
    }
}

impl ShapeOptions {
    pub fn from_character_properties(properties: &model::CharacterProperties) -> Self {
        Self::from_character_properties_with_direction(properties, TextDirection::LeftToRight)
    }

    pub fn from_character_properties_with_direction(
        properties: &model::CharacterProperties,
        direction: TextDirection,
    ) -> Self {
        Self {
            size_pt: properties.font_size_pt.unwrap_or(11.0),
            direction,
            language: properties.language.clone(),
            typography: properties.typography.clone().unwrap_or_default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ShapedGlyph {
    /// Glyph ID in the selected font face.
    pub glyph_id: u32,
    /// UTF-8 byte cluster in the input run. Multiple characters may share a
    /// cluster when they form a ligature.
    pub cluster: usize,
    pub x_advance_pt: f32,
    pub y_advance_pt: f32,
    pub x_offset_pt: f32,
    pub y_offset_pt: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ShapedRun {
    pub glyphs: Vec<ShapedGlyph>,
    pub advance_pt: f32,
    pub units_per_em: u16,
}

#[derive(Debug, Error)]
pub enum ShapeError {
    #[error("invalid OpenType font face")]
    InvalidFont,
    #[error("invalid OpenType feature: {0}")]
    InvalidFeature(String),
}

/// Shapes a single already-resolved font run.
///
/// rustybuzz is a pure-Rust HarfBuzz port, so this path gets real GSUB/GPOS
/// behaviour: `fi`/`ff` ligatures when the font provides them, kerning pairs,
/// mark positioning, contextual substitutions, Arabic shaping, and more.
pub fn shape_text(
    font_data: &[u8],
    face_index: u32,
    text: &str,
    options: &ShapeOptions,
) -> Result<ShapedRun, ShapeError> {
    let face = Face::from_slice(font_data, face_index).ok_or(ShapeError::InvalidFont)?;
    shape_with_face(&face, text, options)
}

/// Shapes a run with an already parsed face. Parsing a face builds lookup
/// accelerators for the whole font, so callers should reuse faces
/// (`FontCatalog::shape` does).
pub fn shape_with_face(face: &Face<'_>, text: &str, options: &ShapeOptions) -> Result<ShapedRun, ShapeError> {
    let upem = face.units_per_em();
    let scale = options.size_pt / upem as f32;

    let mut buffer = UnicodeBuffer::new();
    buffer.push_str(text);
    buffer.set_cluster_level(BufferClusterLevel::MonotoneGraphemes);
    buffer.set_direction(match options.direction {
        TextDirection::LeftToRight => Direction::LeftToRight,
        TextDirection::RightToLeft => Direction::RightToLeft,
    });
    if let Some(lang) = options.language.as_deref() {
        if let Ok(lang) = rustybuzz::Language::from_str(lang) {
            buffer.set_language(lang);
        }
    }
    buffer.guess_segment_properties();

    let features = build_features(&options.typography)?;
    let shaped = rustybuzz::shape(face, &features, buffer);
    let mut advance = 0.0_f32;
    let mut glyphs: Vec<ShapedGlyph> = shaped
        .glyph_infos()
        .iter()
        .zip(shaped.glyph_positions())
        .map(|(info, pos)| {
            let x_advance = pos.x_advance as f32 * scale;
            advance += x_advance;
            ShapedGlyph {
                glyph_id: info.glyph_id,
                cluster: info.cluster as usize,
                x_advance_pt: x_advance,
                y_advance_pt: pos.y_advance as f32 * scale,
                x_offset_pt: pos.x_offset as f32 * scale,
                // OpenType uses a y-up convention; Impress's layout space is y-down.
                y_offset_pt: -(pos.y_offset as f32 * scale),
            }
        })
        .collect();

    if options.direction == TextDirection::LeftToRight && options.typography.kerning
        && options.typography.kerning_mode == KerningMode::Optical {
        super::optical_kerning::apply(&mut glyphs, face, text, options.size_pt, upem as u16);
        advance = glyphs.iter().map(|glyph| glyph.x_advance_pt).sum();
    }

    Ok(ShapedRun { glyphs, advance_pt: advance, units_per_em: upem as u16 })
}

/// Results from the SAME shaper and selected font face used by the canvas.
/// A count of one means a full sequence ligature; f + fl is reported as partial,
/// not as an ff/ffl ligature. No presentation characters or substitute fonts are
/// inserted to manufacture a glyph which the font does not provide.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LigatureProbe {
    pub text: &'static str,
    pub off_glyphs: usize,
    pub standard_glyphs: usize,
    pub discretionary_glyphs: usize,
    pub current_glyphs: usize,
}

pub fn probe_ligatures(font_data: &[u8], face_index: u32, options: &ShapeOptions)
    -> Result<Vec<LigatureProbe>, ShapeError>
{
    let mut off = options.clone();
    off.typography.ligatures = LigatureMode::None;
    let mut standard = options.clone();
    standard.typography.ligatures = LigatureMode::Standard;
    standard.typography.contextual_ligatures = true;
    let mut discretionary = standard.clone();
    discretionary.typography.ligatures = LigatureMode::Discretionary;
    ["ff", "fi", "fl", "ffi", "ffl", "st"].into_iter().map(|text| {
        Ok(LigatureProbe {
            text,
            off_glyphs: shape_text(font_data, face_index, text, &off)?.glyphs.len(),
            standard_glyphs: shape_text(font_data, face_index, text, &standard)?.glyphs.len(),
            discretionary_glyphs: shape_text(font_data, face_index, text, &discretionary)?.glyphs.len(),
            current_glyphs: shape_text(font_data, face_index, text, options)?.glyphs.len(),
        })
    }).collect()
}

fn build_features(options: &OpenTypeTypography) -> Result<Vec<Feature>, ShapeError> {
    let mut out = Vec::new();
    push_feature(&mut out, "kern", options.kerning && options.kerning_mode != KerningMode::Off)?;
    push_feature(&mut out, "liga", options.ligatures != LigatureMode::None)?;
    push_feature(&mut out, "clig", options.contextual_ligatures && options.ligatures != LigatureMode::None)?;
    push_feature(&mut out, "dlig", options.ligatures == LigatureMode::Discretionary)?;
    push_feature(&mut out, "calt", options.contextual_alternates)?;
    push_feature(&mut out, "mark", true)?;
    push_feature(&mut out, "mkmk", true)?;
    push_feature(&mut out, "smcp", options.small_caps)?;
    push_feature(&mut out, "onum", options.oldstyle_figures)?;
    push_feature(&mut out, "tnum", options.tabular_figures)?;
    if let Some(set)=options.stylistic_set.filter(|set|(1..=20).contains(set)) {
        push_feature(&mut out, &format!("ss{set:02}"), true)?;
    }
    Ok(out)
}

fn push_feature(out: &mut Vec<Feature>, tag: &str, enabled: bool) -> Result<(), ShapeError> {
    let text = if enabled { format!("+{tag}") } else { format!("-{tag}") };
    let feature = Feature::from_str(&text).map_err(|_| ShapeError::InvalidFeature(tag.into()))?;
    out.push(feature);
    Ok(())
}


#[cfg(test)]
#[path="../../../test-support/ligature_font.rs"]
mod ligature_fixture;
#[cfg(test)] #[path = "../tests/unit/shaping/ligature_tests.rs"] mod ligature_tests;

