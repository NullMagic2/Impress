//! Conservative outline-based spacing for Latin text. Font kerning remains the
//! baseline; outline corrections address conspicuously loose or tight pairs.
use crate::shaping::ShapedGlyph;
use std::collections::HashMap;
use ttf_parser::{Face, GlyphId, OutlineBuilder};
use unicode_script::{Script, UnicodeScript};

type Segment = (f32, f32, f32, f32);

#[derive(Default)]
struct Outline {
    segments: Vec<Segment>,
    first: (f32, f32),
    current: (f32, f32),
    bounds: Option<(f32, f32)>,
}

impl Outline {
    fn line(&mut self, x: f32, y: f32) {
        let (px, py) = self.current;
        if (px - x).abs() + (py - y).abs() > 0.001 {
            self.segments.push((px, py, x, y));
            self.bounds = Some(match self.bounds {
                Some((lo, hi)) => (lo.min(py).min(y), hi.max(py).max(y)),
                None => (py.min(y), py.max(y)),
            });
        }
        self.current = (x, y);
    }
}

impl OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) { self.first = (x, y); self.current = (x, y); }
    fn line_to(&mut self, x: f32, y: f32) { self.line(x, y); }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (x0, y0) = self.current;
        for step in 1..=12 {
            let t = step as f32 / 12.0;
            let u = 1.0 - t;
            self.line(u*u*x0 + 2.0*u*t*x1 + t*t*x, u*u*y0 + 2.0*u*t*y1 + t*t*y);
        }
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (x0, y0) = self.current;
        for step in 1..=16 {
            let t = step as f32 / 16.0;
            let u = 1.0 - t;
            self.line(u*u*u*x0 + 3.0*u*u*t*x1 + 3.0*u*t*t*x2 + t*t*t*x,
                u*u*u*y0 + 3.0*u*u*t*y1 + 3.0*u*t*t*y2 + t*t*t*y);
        }
    }
    fn close(&mut self) { let (x, y) = self.first; self.line(x, y); }
}

fn edge_at(outline: &Outline, y: f32, right: bool) -> Option<f32> {
    outline.segments.iter().filter_map(|&(x0, y0, x1, y1)| {
        if (y0 <= y && y < y1) || (y1 <= y && y < y0) {
            Some(x0 + (y - y0) * (x1 - x0) / (y1 - y0))
        } else { None }
    }).reduce(|a, b| if right { a.max(b) } else { a.min(b) })
}

fn latin_or_punctuation(ch: char) -> bool {
    !ch.is_whitespace() && !ch.is_control()
        && matches!(ch.script(), Script::Latin | Script::Common)
}

fn pair_adjustment(left: &Outline, right: &Outline, advance: f32,
    left_offset: f32, right_offset: f32, scale: f32, size_pt: f32) -> Option<f32> {
    pair_adjustment_mixed(left,right,advance,left_offset,right_offset,scale,scale,size_pt,size_pt,true)
}

fn pair_adjustment_mixed(left: &Outline, right: &Outline, advance: f32,
    left_offset: f32, right_offset: f32, left_scale: f32, right_scale: f32,
    left_size_pt: f32, right_size_pt: f32, font_pair: bool) -> Option<f32> {
    let (l0, l1) = left.bounds?;
    let (r0, r1) = right.bounds?;
    let lo = (l0 * left_scale).max(r0 * right_scale);
    let hi = (l1 * left_scale).min(r1 * right_scale);
    let size_pt = (left_size_pt + right_size_pt) * 0.5;
    if hi - lo < size_pt * 0.04 { return None; }
    let mut gaps = Vec::with_capacity(24);
    for i in 0..24 {
        let y = lo + (i as f32 + 0.5) * (hi - lo) / 24.0;
        if let (Some(a), Some(b)) = (edge_at(left, y / left_scale, true), edge_at(right, y / right_scale, false)) {
            gaps.push(advance + right_offset - left_offset + b * right_scale - a * left_scale);
        }
    }
    if gaps.len() < 8 { return None; }
    gaps.sort_by(f32::total_cmp);
    let trim = gaps.len() / 6;
    let central = &gaps[trim..gaps.len()-trim];
    let mean = central.iter().sum::<f32>() / central.len() as f32;
    // Serif sidebearings and visible strokes are not interchangeable. Preserve
    // the designer's spacing through a broad neutral band, then move only a
    // fraction of unusually open or closed pairs.
    let lower = size_pt * 0.05;
    let upper = size_pt * if font_pair {0.22} else {0.18};
    let correction = if mean < lower { (lower-mean)*0.45 }
        else if mean > upper { (upper-mean)*if font_pair {0.35} else {0.45} }
        else { 0.0 };
    let max_pull=size_pt*if font_pair {0.025} else {0.04};
    let correction = correction.clamp(-max_pull,size_pt*0.035);
    // Preserve a small minimum gap when an outline extremum is near its mate.
    let minimum = gaps[0];
    Some(correction.max(size_pt*0.01-minimum).clamp(-max_pull,size_pt*0.035))
}

/// Pair adjustment normally supplied by the font's GPOS/kern tables. Used at
/// a style boundary, where the shaper had to process each side separately.
pub fn font_pair_kerning(face:&rustybuzz::Face<'_>,left:char,right:char,
    size_pt:f32,typography:&model::OpenTypeTypography)->Option<f32> {
    use crate::shaping::{shape_with_face,ShapeOptions,TextDirection};
    if !size_pt.is_finite() || size_pt<=0.0 {return None;}
    let mut typography=typography.clone();typography.kerning_mode=model::KerningMode::Metrics;
    let options=ShapeOptions{size_pt,direction:TextDirection::LeftToRight,language:None,typography};
    let pair=shape_with_face(face,&format!("{left}{right}"),&options).ok()?;
    let a=shape_with_face(face,&left.to_string(),&options).ok()?;
    let b=shape_with_face(face,&right.to_string(),&options).ok()?;
    if pair.glyphs.len()!=2 || a.glyphs.len()!=1 || b.glyphs.len()!=1
        || pair.glyphs[0].glyph_id!=a.glyphs[0].glyph_id
        || pair.glyphs[1].glyph_id!=b.glyphs[0].glyph_id {return None;}
    Some(pair.advance_pt-a.advance_pt-b.advance_pt)
}

/// Optical adjustment for the boundary between two separately styled runs.
/// The caller adds this to the final advance of the left run, before line fitting.
#[allow(clippy::too_many_arguments)]
pub fn optical_pair_adjustment(
    left_face: &Face<'_>, left_glyph: &ShapedGlyph, left_size_pt: f32,
    right_face: &Face<'_>, right_glyph: &ShapedGlyph, right_size_pt: f32,
    left_char: char, right_char: char, font_kern_delta: Option<f32>,
) -> Option<f32> {
    if !left_size_pt.is_finite() || !right_size_pt.is_finite()
        || left_size_pt <= 0.0 || right_size_pt <= 0.0
        || !latin_or_punctuation(left_char) || !latin_or_punctuation(right_char)
        || left_glyph.x_advance_pt <= 0.0 || right_glyph.x_advance_pt <= 0.0 { return None; }
    let baseline=font_kern_delta.unwrap_or(0.0);
    let optical=(|| {
        let mut left=Outline::default();
        let mut right=Outline::default();
        left_face.outline_glyph(GlyphId(left_glyph.glyph_id as u16),&mut left)?;
        right_face.outline_glyph(GlyphId(right_glyph.glyph_id as u16),&mut right)?;
        pair_adjustment_mixed(&left,&right,left_glyph.x_advance_pt+baseline,
            left_glyph.x_offset_pt,right_glyph.x_offset_pt,
            left_size_pt/left_face.units_per_em() as f32,
            right_size_pt/right_face.units_per_em() as f32,left_size_pt,right_size_pt,
            font_kern_delta.is_some())
    })();
    optical.map(|delta|baseline+delta).or(font_kern_delta)
}

pub(super) fn apply(glyphs: &mut [ShapedGlyph], face: &Face<'_>,
    text: &str, size_pt: f32, upem: u16) {
    if glyphs.len() < 2 || upem == 0 || !size_pt.is_finite() || size_pt <= 0.0 { return; }
    let scale = size_pt / upem as f32;
    let mut cache = HashMap::<u32, Option<Outline>>::new();
    for index in 0..glyphs.len()-1 {
        let left = &glyphs[index];
        let right = &glyphs[index+1];
        if left.cluster == right.cluster || left.glyph_id == 0 || right.glyph_id == 0
            || left.x_advance_pt <= 0.0 || right.x_advance_pt <= 0.0 { continue; }
        let Some(a) = text.get(left.cluster..).and_then(|s|s.chars().next()) else { continue; };
        let Some(b) = text.get(right.cluster..).and_then(|s|s.chars().next()) else { continue; };
        if !latin_or_punctuation(a) || !latin_or_punctuation(b) { continue; }
        for id in [left.glyph_id, right.glyph_id] {
            cache.entry(id).or_insert_with(|| {
                let mut outline = Outline::default();
                face.outline_glyph(GlyphId(id as u16), &mut outline)?;
                outline.bounds.map(|_| outline)
            });
        }
        let (Some(Some(a)), Some(Some(b))) = (cache.get(&left.glyph_id), cache.get(&right.glyph_id)) else { continue; };
        if let Some(delta) = pair_adjustment(a, b, left.x_advance_pt,
            left.x_offset_pt, right.x_offset_pt, scale, size_pt) {
            glyphs[index].x_advance_pt += delta;
        }
    }
}

#[cfg(test)] #[path = "../tests/unit/optical_kerning.rs"] mod tests;
