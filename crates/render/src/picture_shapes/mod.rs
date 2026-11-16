//! Evaluate standard DrawingML picture geometry and produce shared crop masks.
mod presets;
mod guides;
use crate::{Affine2D, PathVerb, Point, VectorPath};
use std::collections::BTreeMap;

/// DrawingML's angular unit is one sixty-thousandth of a degree.
const ANGLE_TO_RAD: f64 = std::f64::consts::PI / 10_800_000.0;
/// Bound malformed adjustment coordinates before rasterization.
const MAX_PATH_COORDINATE_PT: f32 = 10_000_000.0;
/// Cubic arcs span at most a quarter turn to keep their approximation accurate.
const MAX_ARC_SEGMENT_RAD: f64 = std::f64::consts::FRAC_PI_2;

struct Preset {
    name: &'static str,
    adjustments: &'static [(&'static str, &'static str)],
    guides: &'static [(&'static str, &'static str)],
    paths: &'static [ShapePath],
}
struct ShapePath {
    width: Option<&'static str>,
    height: Option<&'static str>,
    fill: bool,
    stroke: bool,
    commands: &'static [Command],
}
enum Command {
    Move(&'static str, &'static str), Line(&'static str, &'static str),
    Quad(&'static str, &'static str, &'static str, &'static str),
    Cubic(&'static str, &'static str, &'static str, &'static str, &'static str, &'static str),
    Arc(&'static str, &'static str, &'static str, &'static str), Close,
}

/// One evaluated path; line-only decoration is excluded from the crop mask.
pub struct GeometryPath { pub path: VectorPath, pub fill: bool, pub stroke: bool }

pub fn preset_names() -> impl Iterator<Item = &'static str> { presets::PRESETS.iter().map(|p| p.name) }

pub fn is_crop_shape(name: &str) -> bool {
    presets::PRESETS.iter().find(|p| p.name == name).is_some_and(|p| p.paths.iter()
        .any(|path| path.fill && path.commands.iter().any(|c| matches!(c, Command::Close))))
}

/// Resolve preset adjustments and guides at the final picture's physical size.
pub fn geometry(name: &str, adjustments: &BTreeMap<String, String>, width: f32, height: f32) -> Option<Vec<GeometryPath>> {
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 { return None; }
    let preset = presets::PRESETS.iter().find(|p| p.name == name)?;
    let mut values = guides::Values::new(f64::from(width), f64::from(height));
    for (name, default) in preset.adjustments {
        values.assign(name, adjustments.get(*name).map(String::as_str).unwrap_or(default))?;
    }
    for (name, formula) in preset.guides { values.assign(name, formula)?; }
    preset.paths.iter().map(|path| evaluate_path(path, &values, width, height)).collect()
}

fn evaluate_path(path: &ShapePath, values: &guides::Values, width: f32, height: f32) -> Option<GeometryPath> {
    let path_width = match path.width { Some(value) => values.get(value)?, None => f64::from(width) };
    let sx = width / path_width as f32;
    let path_height = match path.height { Some(value) => values.get(value)?, None => f64::from(height) };
    let sy = height / path_height as f32;
    if !sx.is_finite() || !sy.is_finite() { return None; }
    let point = |x, y| Some(Point::new(values.get(x)? as f32, values.get(y)? as f32));
    let mut verbs = Vec::new();
    let mut current = Point::new(0.0, 0.0);
    let mut start = current;
    for command in path.commands {
        match *command {
            Command::Move(x, y) => { current = point(x, y)?; start = current; verbs.push(PathVerb::MoveTo(current)); }
            Command::Line(x, y) => { current = point(x, y)?; verbs.push(PathVerb::LineTo(current)); }
            Command::Quad(x1, y1, x, y) => {
                current = point(x, y)?;
                verbs.push(PathVerb::QuadTo { ctrl: point(x1, y1)?, to: current });
            }
            Command::Cubic(x1, y1, x2, y2, x, y) => {
                current = point(x, y)?;
                verbs.push(PathVerb::CubicTo { ctrl1: point(x1, y1)?, ctrl2: point(x2, y2)?, to: current });
            }
            Command::Arc(rx, ry, start, sweep) => {
                current = arc(&mut verbs, current, values.get(rx)?.abs(), values.get(ry)?.abs(), values.get(start)? * ANGLE_TO_RAD, values.get(sweep)? * ANGLE_TO_RAD);
            }
            Command::Close => { verbs.push(PathVerb::Close); current = start; }
        }
    }
    let geometry = VectorPath { verbs }.transformed(Affine2D::scale(sx, sy));
    let valid = |p: Point| p.x.is_finite() && p.y.is_finite() && p.x.abs() <= MAX_PATH_COORDINATE_PT && p.y.abs() <= MAX_PATH_COORDINATE_PT;
    if !geometry.verbs.iter().all(|verb| match *verb {
        PathVerb::MoveTo(p) | PathVerb::LineTo(p) => valid(p),
        PathVerb::QuadTo { ctrl, to } => valid(ctrl) && valid(to),
        PathVerb::CubicTo { ctrl1, ctrl2, to } => valid(ctrl1) && valid(ctrl2) && valid(to),
        PathVerb::Close => true,
    }) { return None; }
    Some(GeometryPath { path: geometry, fill: path.fill, stroke: path.stroke })
}

fn arc(verbs: &mut Vec<PathVerb>, current: Point, rx: f64, ry: f64, start: f64, sweep: f64) -> Point {
    if rx <= f64::EPSILON || ry <= f64::EPSILON || sweep.abs() <= f64::EPSILON { return current; }
    let sweep = sweep.clamp(-std::f64::consts::TAU, std::f64::consts::TAU);
    // DrawingML uses radial angles; ellipse parameter angles depend on aspect.
    let parameter = |angle: f64| (rx * angle.sin()).atan2(ry * angle.cos()).rem_euclid(std::f64::consts::TAU);
    let first = parameter(start);
    let mut last = parameter(start + sweep);
    if sweep > 0.0 && last <= first { last += std::f64::consts::TAU; }
    if sweep < 0.0 && last >= first { last -= std::f64::consts::TAU; }
    if sweep.abs() >= std::f64::consts::TAU - f64::EPSILON { last = first + sweep; }
    let cx = f64::from(current.x) - rx * first.cos();
    let cy = f64::from(current.y) - ry * first.sin();
    let count = ((last - first).abs() / MAX_ARC_SEGMENT_RAD).ceil().max(1.0) as usize;
    let step = (last - first) / count as f64;
    let at = |angle: f64| Point::new((cx + rx * angle.cos()) as f32, (cy + ry * angle.sin()) as f32);
    let mut end = current;
    for index in 0..count {
        let a = first + step * index as f64;
        let b = a + step;
        let factor = (step / 4.0).tan() * 4.0 / 3.0;
        end = at(b);
        verbs.push(PathVerb::CubicTo {
            ctrl1: Point::new((cx + rx * (a.cos() - factor * a.sin())) as f32, (cy + ry * (a.sin() + factor * a.cos())) as f32),
            ctrl2: Point::new((cx + rx * (b.cos() + factor * b.sin())) as f32, (cy + ry * (b.sin() - factor * b.cos())) as f32), to: end,
        });
    }
    end
}

/// Antialiased picture silhouette, retaining holes and independent filled paths.
pub fn crop_mask(paths: &[GeometryPath], width: u32, height: u32, physical_width: f32, physical_height: f32) -> Option<Vec<u8>> {
    if !physical_width.is_finite() || !physical_height.is_finite() || physical_width <= 0.0 || physical_height <= 0.0 { return None; }
    let pixels = (width as usize).checked_mul(height as usize)?;
    if pixels > crate::MAX_RASTER_BYTES { return None; }
    let mut mask = tiny_skia::Mask::new(width, height)?;
    let transform = tiny_skia::Transform::from_scale(width as f32 / physical_width, height as f32 / physical_height);
    for geometry in paths.iter().filter(|p| p.fill) {
        if let Some(path) = skia_path(&geometry.path) {
            mask.fill_path(&path, tiny_skia::FillRule::Winding, true, transform);
        }
    }
    Some(mask.data().to_vec())
}

pub fn skia_path(path: &VectorPath) -> Option<tiny_skia::Path> {
    let mut builder = tiny_skia::PathBuilder::new();
    for verb in &path.verbs {
        match *verb {
            PathVerb::MoveTo(p) => builder.move_to(p.x, p.y),
            PathVerb::LineTo(p) => builder.line_to(p.x, p.y),
            PathVerb::QuadTo { ctrl, to } => builder.quad_to(ctrl.x, ctrl.y, to.x, to.y),
            PathVerb::CubicTo { ctrl1, ctrl2, to } => builder.cubic_to(ctrl1.x, ctrl1.y, ctrl2.x, ctrl2.y, to.x, to.y),
            PathVerb::Close => builder.close(),
        }
    }
    builder.finish()
}

#[cfg(test)] #[path = "../../tests/unit/picture_shapes.rs"] mod tests;
