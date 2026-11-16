//! Drawing shared by the painted Ribbon pages (Table and Picture tools):
//! rounded rectangles, lines, the large-button geometry and mouse positions.
use super::*;
use tiny_skia::{FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke};
use wxdragon::widgets::panel::Panel;

pub(super) fn position(event: &WindowEventData) -> Option<(f32, f32)> {
    let p = match event {
        WindowEventData::MouseButton(m) => m.event.get_position(),
        WindowEventData::MouseMotion(m) => m.event.get_position(),
        _ => None,
    }?;
    Some((p.x as f32, p.y as f32))
}

pub(super) fn paint(rgb: (u8, u8, u8)) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color_rgba8(rgb.0, rgb.1, rgb.2, 255);
    paint.anti_alias = true;
    paint
}

pub(super) fn round_rect(r: (f32, f32, f32, f32), radius: f32) -> Option<tiny_skia::Path> {
    let (x, y, w, h) = r;
    if w <= 0.0 || h <= 0.0 { return None; }
    let radius = radius.min(w * 0.5).min(h * 0.5).max(0.0);
    let c = radius * 0.4477;
    let (x1, y1) = (x + w, y + h);
    let mut p = PathBuilder::new();
    p.move_to(x + radius, y);
    p.line_to(x1 - radius, y); p.cubic_to(x1 - c, y, x1, y + c, x1, y + radius);
    p.line_to(x1, y1 - radius); p.cubic_to(x1, y1 - c, x1 - c, y1, x1 - radius, y1);
    p.line_to(x + radius, y1); p.cubic_to(x + c, y1, x, y1 - c, x, y1 - radius);
    p.line_to(x, y + radius); p.cubic_to(x, y + c, x + c, y, x + radius, y);
    p.close();
    p.finish()
}

pub(super) fn fill_round(pixmap: &mut Pixmap, r: (f32, f32, f32, f32), radius: f32, rgb: (u8, u8, u8)) {
    if let Some(path) = round_rect(r, radius) { pixmap.fill_path(&path, &paint(rgb), FillRule::Winding, crate::crisp::transform(), None); }
}

pub(super) fn fill_alpha(pixmap: &mut Pixmap, r: (f32, f32, f32, f32), rgb: (u8, u8, u8), alpha: u8) {
    let mut p = paint(rgb);
    p.set_color_rgba8(rgb.0, rgb.1, rgb.2, alpha);
    if let Some(path) = round_rect(r, 0.0) { pixmap.fill_path(&path, &p, FillRule::Winding, crate::crisp::transform(), None); }
}

pub(super) fn stroke_round(pixmap: &mut Pixmap, r: (f32, f32, f32, f32), radius: f32, rgb: (u8, u8, u8), width: f32) {
    if let Some(path) = round_rect(r, radius) {
        pixmap.stroke_path(&path, &paint(rgb), &Stroke { width, ..Stroke::default() }, crate::crisp::transform(), None);
    }
}

pub(super) fn line(pixmap: &mut Pixmap, points: &[(f32, f32)], rgb: (u8, u8, u8), width: f32) {
    let mut p = PathBuilder::new();
    for (i, (x, y)) in points.iter().enumerate() { if i == 0 { p.move_to(*x, *y) } else { p.line_to(*x, *y) } }
    if let Some(path) = p.finish() {
        let stroke = Stroke { width, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Stroke::default() };
        pixmap.stroke_path(&path, &paint(rgb), &stroke, crate::crisp::transform(), None);
    }
}

/// Top of a large button's icon, so icon and two caption lines sit centred.
pub(super) fn large_top(height: f32, scale: f32) -> f32 { ((height - 66.0 * scale) * 0.5).max(4.0 * scale) }
pub(super) fn scale_of(panel: &Panel) -> f32 { panel.from_dip_int(1000) as f32 / 1000.0 }
