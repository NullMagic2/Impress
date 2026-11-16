use crate::{Affine2D, Point, Rect};
use std::collections::HashMap;
use thiserror::Error;
use ttf_parser::{Face, GlyphId, OutlineBuilder};

#[derive(Clone, Debug, PartialEq)]
pub enum PathVerb {
    MoveTo(Point),
    LineTo(Point),
    QuadTo { ctrl: Point, to: Point },
    CubicTo { ctrl1: Point, ctrl2: Point, to: Point },
    Close,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VectorPath {
    pub verbs: Vec<PathVerb>,
}

impl VectorPath {
    pub fn transformed(&self, transform: Affine2D) -> Self {
        let verbs = self.verbs.iter().map(|verb| match *verb {
            PathVerb::MoveTo(p) => PathVerb::MoveTo(transform.map_point(p)),
            PathVerb::LineTo(p) => PathVerb::LineTo(transform.map_point(p)),
            PathVerb::QuadTo { ctrl, to } => PathVerb::QuadTo {
                ctrl: transform.map_point(ctrl),
                to: transform.map_point(to),
            },
            PathVerb::CubicTo { ctrl1, ctrl2, to } => PathVerb::CubicTo {
                ctrl1: transform.map_point(ctrl1),
                ctrl2: transform.map_point(ctrl2),
                to: transform.map_point(to),
            },
            PathVerb::Close => PathVerb::Close,
        }).collect();
        Self { verbs }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct GlyphOutline {
    pub glyph_id: u32,
    pub units_per_em: u16,
    pub bounds: Option<Rect>,
    pub path: VectorPath,
}

impl GlyphOutline {
    pub fn scale_to_points(&self, size_pt: f32) -> VectorPath {
        let scale = size_pt / self.units_per_em as f32;
        self.path.transformed(Affine2D::scale(scale, -scale))
    }
}

#[derive(Clone, Debug)]
pub struct GlyphOutlineCache {
    units_per_em: u16,
    outlines: HashMap<u32, GlyphOutline>,
}

impl GlyphOutlineCache {
    pub fn new(units_per_em: u16) -> Self {
        Self { units_per_em, outlines: HashMap::new() }
    }

    pub fn units_per_em(&self) -> u16 { self.units_per_em }

    pub fn get_or_insert_with(
        &mut self,
        glyph_id: u32,
        mut f: impl FnMut() -> Result<GlyphOutline, OutlineError>,
    ) -> Result<&GlyphOutline, OutlineError> {
        if !self.outlines.contains_key(&glyph_id) {
            let outline = f()?;
            self.outlines.insert(glyph_id, outline);
        }
        Ok(self.outlines.get(&glyph_id).expect("glyph outline inserted"))
    }
}

#[derive(Debug, Error)]
pub enum OutlineError {
    #[error("invalid font face")]
    InvalidFont,
    #[error("glyph {0} has no outline")]
    MissingOutline(u32),
}

pub fn extract_glyph_outline(font_data: &[u8], face_index: u32, glyph_id: u32) -> Result<GlyphOutline, OutlineError> {
    let face = Face::parse(font_data, face_index).map_err(|_| OutlineError::InvalidFont)?;
    glyph_outline_from_face(&face, glyph_id)
}

/// Outline extraction from an already parsed face (see `FontCatalog::with_face`).
pub fn glyph_outline_from_face(face: &Face<'_>, glyph_id: u32) -> Result<GlyphOutline, OutlineError> {
    let units_per_em = face.units_per_em();
    let id = GlyphId(glyph_id as u16);
    let mut builder = GlyphPathBuilder::default();
    let bounds = face.outline_glyph(id, &mut builder)
        .ok_or(OutlineError::MissingOutline(glyph_id))
        .map(|r| Rect::new(r.x_min as f32, r.y_min as f32, (r.x_max - r.x_min) as f32, (r.y_max - r.y_min) as f32))?;

    Ok(GlyphOutline {
        glyph_id,
        units_per_em,
        bounds: Some(bounds),
        path: VectorPath { verbs: builder.verbs },
    })
}

#[derive(Default)]
struct GlyphPathBuilder {
    verbs: Vec<PathVerb>,
}

impl OutlineBuilder for GlyphPathBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        self.verbs.push(PathVerb::MoveTo(Point::new(x, y)));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.verbs.push(PathVerb::LineTo(Point::new(x, y)));
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.verbs.push(PathVerb::QuadTo { ctrl: Point::new(x1, y1), to: Point::new(x, y) });
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.verbs.push(PathVerb::CubicTo {
            ctrl1: Point::new(x1, y1),
            ctrl2: Point::new(x2, y2),
            to: Point::new(x, y),
        });
    }

    fn close(&mut self) {
        self.verbs.push(PathVerb::Close);
    }
}
