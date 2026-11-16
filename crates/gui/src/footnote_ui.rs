//! Word-style footnote interaction on the page. Hovering over a reference
//! mark shows its note as a tooltip; double-clicking a reference mark jumps
//! to the note, and double-clicking the start of a note jumps back.
use crate::*;

/// Longest note text shown in a tooltip, in characters.
const TOOLTIP_CHARS: usize = 240;

/// The note whose reference mark is under `view_point`.
pub fn reference_under(state: &CanvasState, view_point: Point) -> Option<NodeId> {
    let hit = state.viewport.hit_test(&state.vector_document, view_point)?;
    state.editor.note_reference_at(&NodeId::from(hit.node_id.as_str()), hit.byte_offset)
}

/// Tooltip text for the reference mark under `view_point`, if any.
pub fn tooltip(state: &CanvasState, view_point: Point) -> Option<String> {
    let note = reference_under(state, view_point)?;
    let text = state.editor.note_text(&note)?.replace('\n', " ");
    let mut shown: String = text.chars().take(TOOLTIP_CHARS).collect();
    if text.chars().count() > TOOLTIP_CHARS { shown.push('…'); }
    Some(if shown.trim().is_empty() { "(Empty note)".into() } else { shown })
}

/// Handles a double-click on a reference mark or at the start of a note.
/// Returns true when it did.
pub fn double_click(state: &mut CanvasState, view_point: Point) -> bool {
    let jumped = if let Some(note) = reference_under(state, view_point) {
        state.editor.enter_footnote(&note)
    } else {
        let Some(hit) = state.viewport.hit_test(&state.vector_document, view_point) else { return false; };
        let paragraph = NodeId::from(hit.node_id.as_str());
        // The note number sits just before the note's first character.
        if hit.byte_offset != 0 || state.editor.note_for_paragraph(&paragraph).is_none() { return false; }
        state.editor.set_caret(paragraph, 0);
        state.editor.leave_footnote()
    };
    match jumped {
        Ok(()) => {
            state.selected_object = None;
            state.preferred_caret_x_pt = None;
            state.caret_visual_line = None;
            refresh_overlay(state);
            reveal_review_caret(state);
            wake_caret(state);
        }
        Err(error) => state.action_status = Some(error),
    }
    true
}
