//! Picture and equation selection on the page, drawn like the table
//! controls (table_ui.rs) so every on-page object behaves alike:
//! - a picture under the pointer gets a light outline, so it reads as
//!   clickable;
//! - the selected picture gets a crisp blue outline and eight square resize
//!   handles: the corners keep its proportions, the edge middles stretch it
//!   one way. A round rotate handle with a circular arrow sits above its top
//!   edge, the round × button that deletes it beyond its top-right corner,
//!   and below that Word's Layout Options button, which opens the Wrap Text
//!   menu;
//! - a moved or resized picture shows a tinted ghost where it will go, and a
//!   turning picture its turned outline.
//!
//! The selected equation gets the outline only: it is edited, not resized.
//! Geometry is in view pixels, so the controls keep their size at every zoom.
use crate::*;
use crate::table_ui::{delete_chip, resize_handle, Metrics, BLUE, HANDLE, SELECTION_FILL};
use objects_ui::SelectedObject;
use render::{ChromeShape, Color, ObjectKind, Rect};

/// Width of the selection outline, in DPI-scaled view pixels (as the table
/// cell selection's).
const OUTLINE_PX: f32 = 1.5;
/// Width of the outline round a picture under the pointer.
const HOVER_OUTLINE_PX: f32 = 1.5;
/// Corner radius of the outlines and the drag ghost.
const OUTLINE_RADIUS_PX: f32 = 1.5;
/// How far from a handle's centre a press still grabs it, so the small
/// handles are easy to hit.
const HANDLE_REACH_PX: f32 = 6.0;
/// Gap between the picture's top-right corner and the × button beyond it.
const CHIP_GAP_PX: f32 = 6.0;
/// Extra radius round the round buttons that still counts as on them, as for
/// the table's ×.
const BUTTON_REACH_PX: f32 = 2.0;
/// Radius of the rotate handle and how far above the top edge its centre is.
/// The Layout Options button's half side, as a share of the × button's radius.
const LAYOUT_BUTTON_SCALE: f32 = 1.05;
const ROTATE_RADIUS_PX: f32 = 8.0;
const ROTATE_OFFSET_PX: f32 = 26.0;
/// Width of the circular arrow inside the rotate handle, and of the stem
/// joining the handle to the top edge.
const ROTATE_STROKE_PX: f32 = 1.4;
const ROTATE_STEM_PX: f32 = 1.0;
/// The circular arrow spans this much of a turn, drawn as short segments.
const ROTATE_ARC_DEG: f32 = 280.0;
const ROTATE_ARC_SEGMENTS: usize = 14;
/// Shift while turning snaps the angle to multiples of this.
pub const ROTATE_SNAP_DEG: f32 = 15.0;

/// A square resize handle, clockwise from the top-left corner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle { TopLeft, Top, TopRight, Right, BottomRight, Bottom, BottomLeft, Left }

impl Handle {
    pub const ALL: [Handle; 8] = [Handle::TopLeft, Handle::Top, Handle::TopRight, Handle::Right,
        Handle::BottomRight, Handle::Bottom, Handle::BottomLeft, Handle::Left];

    /// The edges the handle moves: -1 the left (top) edge, 1 the right
    /// (bottom) edge, 0 neither, as (horizontal, vertical).
    pub fn sides(self) -> (i8, i8) {
        match self {
            Handle::TopLeft => (-1, -1), Handle::Top => (0, -1), Handle::TopRight => (1, -1), Handle::Right => (1, 0),
            Handle::BottomRight => (1, 1), Handle::Bottom => (0, 1), Handle::BottomLeft => (-1, 1), Handle::Left => (-1, 0),
        }
    }

    /// Corners scale the picture in proportion; edge handles stretch it.
    pub fn is_corner(self) -> bool { let (h, v) = self.sides(); h != 0 && v != 0 }

    /// Where the handle sits on `rect`.
    pub fn point(self, rect: Rect) -> Point {
        let (h, v) = self.sides();
        let along = |side: i8, start: f32, length: f32| start + length * (side as f32 + 1.0) * 0.5;
        Point::new(along(h, rect.x, rect.width), along(v, rect.y, rect.height))
    }
}

/// The selected picture's control under the pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target { Handle(Handle), Rotate, Delete, Layout }

/// What the pointer is on: the picture under it (page, node id), if any,
/// and the selected picture's control.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Hover { pub picture: Option<(usize, String)>, pub target: Option<Target> }

/// `rect` (page points on `page`) in view pixels.
fn view_rect(state: &CanvasState, page: usize, rect: Rect) -> Option<Rect> {
    let origin = state.viewport.page_rect_px(&state.vector_document, page)?;
    let scale = state.viewport.points_to_pixels();
    Some(Rect::new(origin.x + rect.x * scale, origin.y + rect.y * scale, rect.width * scale, rect.height * scale))
}

/// Centre of the × button: diagonally beyond the top-right corner, clear of
/// the corner handle and of the rotate handle at any picture width.
fn chip_center(picture: Rect, m: &Metrics) -> Point {
    let offset = CHIP_GAP_PX * m.s + m.chip();
    Point::new(picture.x + picture.width + offset, picture.y - offset)
}

/// Centre of the Layout Options button, below the × button.
fn layout_center(picture: Rect, m: &Metrics) -> Point {
    let chip = chip_center(picture, m);
    Point::new(chip.x, chip.y + 2.0 * m.chip() + CHIP_GAP_PX * m.s)
}

/// The Layout Options button's rectangle around `center`.
fn layout_rect(center: Point, m: &Metrics) -> Rect {
    let half = m.chip() * LAYOUT_BUTTON_SCALE;
    Rect::new(center.x - half, center.y - half, 2.0 * half, 2.0 * half)
}

/// Centre of the rotate handle, above the middle of the top edge.
fn rotate_center(picture: Rect, m: &Metrics) -> Point {
    Point::new(picture.x + picture.width * 0.5, picture.y - ROTATE_OFFSET_PX * m.s)
}

fn within(p: Point, center: Point, radius: f32) -> bool {
    (p.x - center.x).powi(2) + (p.y - center.y).powi(2) <= radius * radius
}

/// The control of a selected picture shown at `picture` (view pixels) that
/// lies under `p`. The round buttons win over a handle they might touch.
fn target_at(picture: Rect, p: Point, m: &Metrics) -> Option<Target> {
    if within(p, chip_center(picture, m), m.chip() + BUTTON_REACH_PX * m.s) { return Some(Target::Delete); }
    if within(p, rotate_center(picture, m), (ROTATE_RADIUS_PX + BUTTON_REACH_PX) * m.s) { return Some(Target::Rotate); }
    let layout = layout_rect(layout_center(picture, m), m);
    let reach = BUTTON_REACH_PX * m.s;
    if p.x >= layout.x - reach && p.x <= layout.x + layout.width + reach && p.y >= layout.y - reach && p.y <= layout.y + layout.height + reach { return Some(Target::Layout); }
    let reach = HANDLE_REACH_PX * m.s;
    Handle::ALL.into_iter()
        .find(|handle| { let c = handle.point(picture); (c.x - p.x).abs() <= reach && (c.y - p.y).abs() <= reach })
        .map(Target::Handle)
}

/// The turn from `start` to `current` around `center`, in degrees clockwise
/// on screen, in (-180, 180]. `snap` rounds it to `ROTATE_SNAP_DEG`.
pub fn rotation_degrees(center: Point, start: Point, current: Point, snap: bool) -> f32 {
    let angle = |p: Point| (p.y - center.y).atan2(p.x - center.x).to_degrees();
    let turn = editor::normalized_degrees(angle(current) - angle(start));
    if snap { editor::normalized_degrees((turn / ROTATE_SNAP_DEG).round() * ROTATE_SNAP_DEG) } else { turn }
}

/// `p` turned clockwise by `degrees` around `center`.
fn turned(p: Point, center: Point, degrees: f32) -> Point {
    let (sin, cos) = degrees.to_radians().sin_cos();
    let (dx, dy) = (p.x - center.x, p.y - center.y);
    Point::new(center.x + dx * cos - dy * sin, center.y + dx * sin + dy * cos)
}

/// The selected picture in view pixels, on the page where it is shown.
fn selected_rect(state: &CanvasState) -> Option<Rect> {
    let (page, region) = objects_ui::selected_picture(state)?;
    view_rect(state, page, region.rect)
}

/// The selected picture's control under `p` (view pixels). None while a
/// drag or an equation draft is in progress.
pub fn target(state: &CanvasState, p: Point) -> Option<Target> {
    if crate::equation_ui::previewing(state) || state.object_drag.is_some() { return None; }
    target_at(selected_rect(state)?, p, &Metrics::of(state))
}

fn hover_at(state: &CanvasState, p: Point) -> Option<Hover> {
    if crate::equation_ui::previewing(state) || state.object_drag.is_some() { return None; }
    let picture = objects_ui::picture_or_equation(state, p)
        .filter(|(_, region)| matches!(region.kind, ObjectKind::Picture { .. }))
        .map(|(page, region)| (page, region.node_id));
    let hover = Hover { picture, target: target(state, p) };
    (hover != Hover::default()).then_some(hover)
}

/// Tracks the picture and control under the pointer (None: it left the
/// canvas). Returns true when the canvas must repaint.
pub fn hover(state: &mut CanvasState, p: Option<Point>) -> bool {
    let next = p.and_then(|p| hover_at(state, p));
    if state.picture_hover == next { return false; }
    state.picture_hover = next;
    state.needs_present = true;
    true
}

/// Tooltip for the control under the pointer, if any.
pub fn tooltip(state: &CanvasState) -> Option<&'static str> {
    match state.picture_hover.as_ref()?.target? {
        Target::Handle(handle) if handle.is_corner() => Some("Drag to resize the picture (Shift unlocks proportions)"),
        Target::Handle(_) => Some("Drag to stretch the picture"),
        Target::Rotate => Some("Drag to rotate the picture (Shift snaps to 15°)"),
        Target::Delete => Some("Delete picture"),
        Target::Layout => Some("Layout Options"),
    }
}

/// A press on the × button deletes the selected picture. Returns true when
/// the press was on it. Handles are handled by objects_ui::down.
pub fn down(state: &mut CanvasState, p: Point) -> bool {
    if target(state, p) == Some(Target::Layout) {
        state.layout_menu_request = selected_rect(state).map(|picture| { let m = Metrics::of(state); layout_rect(layout_center(picture, &m), &m) });
        return true;
    }
    if target(state, p) != Some(Target::Delete) { return false; }
    match objects_ui::delete_selection(state) {
        Ok(_) => state.action_status = Some("Picture deleted".into()),
        Err(error) => state.action_status = Some(error),
    }
    hover(state, Some(p));
    state.needs_present = true;
    true
}

fn outline(out: &mut Vec<ChromeShape>, rect: Rect, color: Color, width: f32, m: &Metrics) {
    out.push(ChromeShape::RoundRect { rect, radius: OUTLINE_RADIUS_PX * m.s, fill: None, stroke: Some((color, width * m.s)) });
}

fn handles(out: &mut Vec<ChromeShape>, rect: Rect, hot: Option<Handle>, m: &Metrics) {
    for handle in Handle::ALL { resize_handle(out, handle.point(rect), hot == Some(handle), m); }
}

/// The rotate handle: a stem from the top edge to a round button holding a
/// circular arrow; white with a blue rim, blue while the pointer is on it.
fn rotate_handle(out: &mut Vec<ChromeShape>, picture: Rect, hot: bool, m: &Metrics) {
    let center = rotate_center(picture, m);
    let radius = ROTATE_RADIUS_PX * m.s;
    out.push(ChromeShape::Line { from: Point::new(center.x, picture.y), to: Point::new(center.x, center.y + radius), width: ROTATE_STEM_PX * m.s, color: BLUE });
    out.push(ChromeShape::Circle { center, radius, fill: Some(if hot { BLUE } else { Color::WHITE }), stroke: Some((BLUE, 1.2 * m.s)) });
    let ink = if hot { Color::WHITE } else { BLUE };
    let arc = radius * 0.55;
    let point_at = |degrees: f32| { let (sin, cos) = degrees.to_radians().sin_cos(); Point::new(center.x + arc * cos, center.y + arc * sin) };
    // The arc opens at the top right and ends in an arrowhead there.
    let start = -60.0;
    let step = ROTATE_ARC_DEG / ROTATE_ARC_SEGMENTS as f32;
    for segment in 0..ROTATE_ARC_SEGMENTS {
        let from = start + step * segment as f32;
        out.push(ChromeShape::Line { from: point_at(from), to: point_at(from + step), width: ROTATE_STROKE_PX * m.s, color: ink });
    }
    let tip = point_at(start + ROTATE_ARC_DEG);
    let barb = 2.6 * m.s;
    for (dx, dy) in [(barb, 0.0), (0.0, -barb)] {
        out.push(ChromeShape::Line { from: tip, to: Point::new(tip.x + dx, tip.y + dy), width: ROTATE_STROKE_PX * m.s, color: ink });
    }
}

/// A light outline round the picture under the pointer, unless it is the
/// selected one.
fn hover_outline(state: &CanvasState, selected: Option<&NodeId>, out: &mut Vec<ChromeShape>, m: &Metrics) {
    let Some((page, id)) = state.picture_hover.as_ref().and_then(|h| h.picture.as_ref()) else { return; };
    if selected.is_some_and(|s| s.0 == *id) { return; }
    let Some(region) = state.vector_document.pages.get(*page).and_then(|p| p.object_regions.iter().find(|r| r.node_id == *id)) else { return; };
    let Some(rect) = view_rect(state, *page, region.rect) else { return; };
    outline(out, rect, HANDLE, HOVER_OUTLINE_PX, m);
}

/// While a picture moves, resizes or turns, only its ghost is drawn. Returns
/// false when no picture drag is under way.
fn drag_ghost(state: &CanvasState, out: &mut Vec<ChromeShape>, m: &Metrics) -> bool {
    let Some(drag) = &state.object_drag else { return false; };
    if let Some((page, original, degrees)) = drag.rotation_preview() {
        let Some(rect) = view_rect(state, page, original) else { return true; };
        let center = Point::new(rect.x + rect.width * 0.5, rect.y + rect.height * 0.5);
        let corners = objects_ui::corners(rect).map(|c| turned(c, center, degrees));
        for (index, from) in corners.iter().enumerate() {
            out.push(ChromeShape::Line { from: *from, to: corners[(index + 1) % 4], width: OUTLINE_PX * m.s, color: BLUE });
        }
        return true;
    }
    let Some((page, preview, resize)) = drag.picture_preview() else { return false; };
    let Some(ghost) = view_rect(state, page, preview) else { return true; };
    out.push(ChromeShape::RoundRect { rect: ghost, radius: OUTLINE_RADIUS_PX * m.s, fill: Some(SELECTION_FILL), stroke: Some((BLUE, OUTLINE_PX * m.s)) });
    if resize { handles(out, ghost, None, m); }
    true
}

/// The picture and equation selection controls for this frame.
pub fn chrome(state: &CanvasState) -> Vec<ChromeShape> {
    let mut out = Vec::new();
    if crate::equation_ui::previewing(state) { return out; }
    let m = Metrics::of(state);
    if drag_ghost(state, &mut out, &m) { return out; }
    let selected = match &state.selected_object {
        Some(SelectedObject::Picture(id) | SelectedObject::Equation(id)) => Some(id),
        _ => None,
    };
    hover_outline(state, selected, &mut out, &m);
    let Some(id) = selected else { return out; };
    let Some((page, region)) = objects_ui::region_for(state, id) else { return out; };
    let Some(rect) = view_rect(state, page, region.rect) else { return out; };
    outline(&mut out, rect, BLUE, OUTLINE_PX, &m);
    if !matches!(state.selected_object, Some(SelectedObject::Picture(_))) { return out; }
    let target = state.picture_hover.as_ref().and_then(|h| h.target);
    let hot_handle = match target { Some(Target::Handle(handle)) => Some(handle), _ => None };
    rotate_handle(&mut out, rect, target == Some(Target::Rotate), &m);
    handles(&mut out, rect, hot_handle, &m);
    delete_chip(&mut out, chip_center(rect, &m), target == Some(Target::Delete), &m);
    layout_button(&mut out, layout_center(rect, &m), target == Some(Target::Layout), &m);
    out
}

/// Word's Layout Options button: a white rounded square with lines of text
/// beside a picture; blue while the pointer is on it.
fn layout_button(out: &mut Vec<ChromeShape>, center: Point, hot: bool, m: &Metrics) {
    let rect = layout_rect(center, m);
    out.push(ChromeShape::RoundRect { rect: Rect::new(rect.x, rect.y + m.s, rect.width, rect.height), radius: 3.0 * m.s, fill: Some(Color::rgba(0.1, 0.16, 0.28, 0.14)), stroke: None });
    out.push(ChromeShape::RoundRect { rect, radius: 3.0 * m.s, fill: Some(if hot { BLUE } else { Color::WHITE }),
        stroke: Some((if hot { BLUE } else { Color::rgb8(214, 220, 230) }, m.s)) });
    let ink = if hot { Color::WHITE } else { BLUE };
    let half = rect.width * 0.5;
    for row in [-0.3f32, 0.0, 0.3] {
        let y = center.y + row * half;
        out.push(ChromeShape::Line { from: Point::new(center.x - half * 0.55, y), to: Point::new(center.x - half * 0.05, y), width: 1.4 * m.s, color: ink });
    }
    let picture = Rect::new(center.x + half * 0.08, center.y - half * 0.38, half * 0.5, half * 0.76);
    out.push(ChromeShape::RoundRect { rect: picture, radius: m.s, fill: Some(ink), stroke: None });
}

/// Side of the rotate cursor bitmap, in pixels, and the radius of its ring.
pub const ROTATE_CURSOR_PX: usize = 32;
const ROTATE_CURSOR_RING_PX: f32 = 8.0;

/// The rotate cursor as XBM bit planes (least significant bit first, rows
/// packed): `(bits, mask)`. The mask covers a circular arrow and a one-pixel
/// halo round it; `bits` is set on the halo only, so the arrow takes the
/// cursor's background colour and the halo its foreground colour, which
/// keeps it visible on light and dark pages. The hotspot is the centre.
pub fn rotate_cursor_bits() -> (Vec<u8>, Vec<u8>) {
    let size = ROTATE_CURSOR_PX;
    let center = size as f32 * 0.5;
    let start = -60.0f32;
    let end = start + ROTATE_ARC_DEG;
    let tip = { let (sin, cos) = end.to_radians().sin_cos(); Point::new(center + ROTATE_CURSOR_RING_PX * cos, center + ROTATE_CURSOR_RING_PX * sin) };
    let on_arrow = |x: f32, y: f32| {
        let (dx, dy) = (x - center, y - center);
        let angle = dy.atan2(dx).to_degrees();
        let on_arc = (angle - start).rem_euclid(360.0) <= ROTATE_ARC_DEG;
        let ring = ((dx * dx + dy * dy).sqrt() - ROTATE_CURSOR_RING_PX).abs() <= 1.0;
        // Arrowhead: a small square of barbs at the arc's end.
        let head = (x - tip.x).abs() <= 3.0 && (y - tip.y).abs() <= 3.0 && (x - tip.x) + (tip.y - y) >= 0.0;
        (ring && on_arc) || head
    };
    let mut arrow = vec![false; size * size];
    for y in 0..size { for x in 0..size { arrow[y * size + x] = on_arrow(x as f32 + 0.5, y as f32 + 0.5); } }
    let halo = |x: usize, y: usize| (y.saturating_sub(1)..=(y + 1).min(size - 1))
        .any(|ny| (x.saturating_sub(1)..=(x + 1).min(size - 1)).any(|nx| arrow[ny * size + nx]));
    let mut bits = vec![0u8; size * size / 8];
    let mut mask = vec![0u8; size * size / 8];
    for y in 0..size {
        for x in 0..size {
            let index = y * size + x;
            let (byte, bit) = (index / 8, 1u8 << (index % 8));
            if arrow[index] { mask[byte] |= bit; }
            else if halo(x, y) { mask[byte] |= bit; bits[byte] |= bit; }
        }
    }
    (bits, mask)
}

