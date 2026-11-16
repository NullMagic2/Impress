//! A slot of an equation selected on the page (a number, an empty box, an
//! exponent…), shown with a blue box as a selected table cell is. Typing
//! replaces what the slot holds, and the equation follows each key (typed as
//! in the equation pane, so `1/2` makes a fraction); Backspace erases. Enter,
//! a click elsewhere or another key finishes; Tab finishes and selects the
//! next empty box; Escape puts the slot back as it was. One undo step per slot.
use crate::*;
use crate::equation_editor::slots;
use model::EquationNode;
use render::{ChromeShape, Rect};

pub struct SlotSelection {
    id: NodeId,
    path: Vec<(usize, usize)>,
    /// The equation before this slot was edited, for Escape.
    original: EquationNode,
    /// What has been typed into the slot; None while it shows what it held.
    typed: Option<String>,
}

/// Selects the slot under a click at (`fx`, `fy`), fractions of the
/// equation's width and height. False when the click is not on a slot.
pub fn select_at(state: &mut CanvasState, id: NodeId, fx: f32, fy: f32) -> bool {
    finish(state);
    let Some(equation) = state.editor.equation_by_id(&id) else { return false; };
    let Some(slot) = slots::slot_at(&equation.root, fx, fy) else { return false; };
    state.equation_slot = Some(SlotSelection { id, path: slot.path, original: equation.root, typed: None });
    state.selected_object = None;
    true
}

fn select_path(state: &mut CanvasState, id: NodeId, path: Vec<(usize, usize)>) {
    let Some(equation) = state.editor.equation_by_id(&id) else { return; };
    state.equation_slot = Some(SlotSelection { id, path, original: equation.root, typed: None });
}

/// Ends the edit: the equation keeps what was typed.
pub fn finish(state: &mut CanvasState) {
    if state.equation_slot.take().is_some() { state.editor.end_equation_edit(); refresh_overlay(state); }
}

/// Puts the slot back as it was and ends the edit.
fn cancel(state: &mut CanvasState) {
    let Some(selection) = state.equation_slot.take() else { return; };
    if selection.typed.is_some() { set_root(state, &selection.id, selection.original); }
    state.editor.end_equation_edit();
    refresh_overlay(state);
}

fn set_root(state: &mut CanvasState, id: &NodeId, root: EquationNode) {
    let Some(mut equation) = state.editor.equation_by_id(id) else { return; };
    if equation.root == root { return; }
    equation.root = root;
    match state.editor.update_equation_live(id, equation) {
        Ok(()) => submit_layout(state),
        Err(error) => state.action_status = Some(format!("Equation not changed: {error}")),
    }
}

/// The equation with what has been typed in the slot.
fn apply(state: &mut CanvasState) {
    let Some(selection) = state.equation_slot.as_ref() else { return; };
    let text = selection.typed.clone().unwrap_or_default();
    match slots::fill(&selection.original, &selection.path, &text) {
        Ok(root) => { let id = selection.id.clone(); set_root(state, &id, root); }
        Err(error) => state.action_status = Some(format!("Equation: {error}")),
    }
}

/// A typed character; true when it went into the selected slot.
pub fn typed(state: &mut CanvasState, ch: char) -> bool {
    let Some(selection) = state.equation_slot.as_mut() else { return false; };
    selection.typed.get_or_insert_with(String::new).push(ch);
    apply(state);
    true
}

/// A key while a slot is selected; true when it was handled here. Keys that
/// move the caret, and shortcuts, finish the edit and go on.
pub fn key_down(state: &mut CanvasState, code: i32, command: bool) -> bool {
    use wxdragon::keycode::*;
    let Some(selection) = state.equation_slot.as_mut() else { return false; };
    if command { finish(state); return false; }
    match code {
        WXK_BACK => {
            if selection.typed.as_ref().map_or_else(
                || slots::is_empty(&selection.original, &selection.path), String::is_empty,
            ) {
                let (id, path) = (selection.id.clone(), selection.path.clone());
                let previous = state.editor.equation_by_id(&id).and_then(|equation| slots::previous(&equation.root, &path));
                finish(state);
                if let Some(previous) = previous {
                    select_path(state, id, previous);
                } else if let Some((_, region)) = objects_ui::region_for(state, &id) {
                    match region.kind {
                        render::ObjectKind::Equation { source: Some((paragraph, offset)) } => {
                            state.editor.set_caret(NodeId::from(paragraph.as_str()), offset);
                        }
                        _ => state.selected_object = Some(objects_ui::SelectedObject::Equation(id)),
                    }
                }
                refresh_overlay(state);
            } else {
                let text = selection.typed.get_or_insert_with(String::new);
                text.pop();
                apply(state);
            }
            true
        }
        WXK_DELETE => { selection.typed = Some(String::new()); apply(state); true }
        WXK_RETURN | WXK_NUMPAD_ENTER => { finish(state); true }
        WXK_ESCAPE => { cancel(state); true }
        WXK_TAB => {
            let (id, path) = (selection.id.clone(), selection.path.clone());
            finish(state);
            let next = state.editor.equation_by_id(&id).and_then(|e| slots::next_empty(&e.root, &path));
            if let Some(next) = next { select_path(state, id, next); }
            true
        }
        // Moving the caret away ends the edit; other keys arrive as typed
        // characters (`typed`).
        WXK_LEFT | WXK_RIGHT | WXK_UP | WXK_DOWN | WXK_HOME | WXK_END | WXK_PAGEUP | WXK_PAGEDOWN => { finish(state); false }
        _ => false,
    }
}

/// The selected slot's area in view pixels.
fn view_rect(state: &CanvasState) -> Option<Rect> {
    // Typst sets each equation with this margin around it.
    const MARGIN_PT: f32 = 2.0;
    // A slot selected on the page, else the one the equation pane's cursor is in.
    let (id, path) = match &state.equation_slot {
        Some(selection) => (&selection.id, &selection.path),
        None => { let (id, path) = state.equation_pane_slot.as_ref().filter(|_| state.equation_draft.is_some())?; (id, path) }
    };
    let equation = state.editor.equation_by_id(id)?;
    let slot = slots::slot_by_path(&equation.root, path)?;
    let (page, region) = objects_ui::region_for(state, id)?;
    let page_rect = state.viewport.page_rect_px(&state.vector_document, page)?;
    let scale = state.viewport.points_to_pixels();
    let r = region.rect;
    let (iw, ih) = ((r.width - 2.0 * MARGIN_PT).max(1.0), (r.height - 2.0 * MARGIN_PT).max(1.0));
    let (ax, ay, aw, ah) = slot.area;
    // Where the slot falls by the editor's own layout of the formula, then
    // fitted to the glyphs Typst drew there: the two lay a formula out a
    // little differently (a fraction's gaps, a script's raise).
    let estimate = Rect::new(r.x + MARGIN_PT + ax * iw, r.y + MARGIN_PT + ay * ih, aw * iw, ah * ih);
    let area = snapped(state, page, r, estimate).unwrap_or(estimate);
    Some(Rect::new(page_rect.x + area.x * scale, page_rect.y + area.y * scale, area.width * scale, area.height * scale))
}

/// The box around the glyphs of the equation in `region` (page points)
/// whose centres lie in or near `estimate`; else around the nearest one.
fn snapped(state: &CanvasState, page: usize, region: Rect, estimate: Rect) -> Option<Rect> {
    use render::{DrawCommand, PathVerb};
    let mut glyphs = Vec::new();
    for command in &state.vector_document.pages.get(page)?.commands {
        let DrawCommand::TextRun(run) = command.unclipped().0 else { continue; };
        if run.source.is_some() { continue; } // text of the paragraph, not the equation
        for glyph in &run.glyphs {
            let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
            for verb in &glyph.outline.verbs {
                let points: &[render::Point] = match verb {
                    PathVerb::MoveTo(p) | PathVerb::LineTo(p) => std::slice::from_ref(p),
                    PathVerb::QuadTo { to, .. } | PathVerb::CubicTo { to, .. } => std::slice::from_ref(to),
                    PathVerb::Close => &[],
                };
                for p in points {
                    let q = glyph.transform.map_point(*p);
                    x0 = x0.min(q.x); y0 = y0.min(q.y); x1 = x1.max(q.x); y1 = y1.max(q.y);
                }
            }
            if x0 > x1 { continue; }
            let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
            let inside = |r: Rect, x: f32, y: f32| x >= r.x && x <= r.x + r.width && y >= r.y && y <= r.y + r.height;
            if inside(region, cx, cy) { glyphs.push((Rect::new(x0, y0, x1 - x0, y1 - y0), cx, cy)); }
        }
    }
    // A margin from the slot's height only: a wide slot (a long numerator)
    // must not reach down to the denominator.
    let reach = 0.25 * estimate.height + 0.5;
    let near = Rect::new(estimate.x - reach, estimate.y - reach, estimate.width + 2.0 * reach, estimate.height + 2.0 * reach);
    let (ex, ey) = (estimate.x + estimate.width / 2.0, estimate.y + estimate.height / 2.0);
    let mut chosen: Vec<Rect> = glyphs.iter().filter(|(_, cx, cy)| *cx >= near.x && *cx <= near.x + near.width && *cy >= near.y && *cy <= near.y + near.height)
        .map(|(r, _, _)| *r).collect();
    if chosen.is_empty() {
        let nearest = glyphs.iter().min_by(|a, b| ((a.1 - ex).hypot(a.2 - ey)).total_cmp(&(b.1 - ex).hypot(b.2 - ey)))?;
        if (nearest.1 - ex).hypot(nearest.2 - ey) > 1.5 * estimate.height.max(estimate.width) { return None; }
        chosen.push(nearest.0);
    }
    let (x0, y0) = chosen.iter().fold((f32::MAX, f32::MAX), |(x, y), r| (x.min(r.x), y.min(r.y)));
    let (x1, y1) = chosen.iter().fold((f32::MIN, f32::MIN), |(x, y), r| (x.max(r.x + r.width), y.max(r.y + r.height)));
    Some(Rect::new(x0, y0, x1 - x0, y1 - y0))
}

/// The blue box around the selected slot (on the page, or in the equation
/// pane), as a selected table cell has.
pub fn chrome(state: &CanvasState) -> Vec<ChromeShape> {
    let Some(rect) = view_rect(state) else { return Vec::new(); };
    let s = state.pixel_scale.max(1.0);
    let pad = 2.0 * s;
    let rect = Rect::new(rect.x - pad, rect.y - pad, rect.width + 2.0 * pad, rect.height + 2.0 * pad);
    vec![ChromeShape::RoundRect { rect, radius: 2.0 * s, fill: Some(table_ui::SELECTION_FILL), stroke: Some((table_ui::BLUE, 1.5 * s)) }]
}
