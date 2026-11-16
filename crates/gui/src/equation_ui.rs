//! Live equation editing. Changes in the equation pane go into the document
//! as they are made, unfinished slots shown as placeholder boxes: each one is
//! laid out first as a debounced, latest-only preview, and committed once
//! Typst has rendered it, so an equation Typst cannot render never reaches
//! the document (it keeps the last good one). The committed document renders
//! exactly as the preview did, so the preview's layout is kept: each change
//! is laid out once.
//! A new equation goes in at the caret where the pane was opened, once it
//! has content: inline, or on its own line for a display equation. Edits of one equation while the pane is open are one undo
//! step. The pane closes with its × or Escape; nothing is left to apply.
use crate::*;
use editor::equation_input;
use model::EquationDisplay;

pub struct EquationDraft {
    /// The equation in the document, once there is one.
    target: Option<NodeId>,
    insertion: Equation,
    selection: Option<Selection>,
    pub text: String,
    pub error: Option<String>,
    /// Inline (`$x$`) or display (`$ x $`), as chosen in the source.
    display: EquationDisplay,
    root: Option<EquationNode>,
    /// What the document holds: the last committed root and display.
    committed: Option<(EquationNode, EquationDisplay)>,
    changed_at: Instant,
    pending: bool,
    pub preview_generation: Option<u64>,
    pub preview_ready: bool,
    /// The document is shown with an uncommitted change in it.
    pub preview_live: bool,
}
impl EquationDraft {
    fn wanted(&self) -> Option<(EquationNode, EquationDisplay)> { self.root.clone().map(|root| (root, self.display)) }
    /// Whether the document holds what the pane shows.
    pub fn up_to_date(&self) -> bool { self.error.is_none() && !self.pending && !self.preview_live && self.wanted() == self.committed }
    pub fn status(&self) -> String {
        if let Some(error) = &self.error { format!("Equation: {error}") }
        else if self.up_to_date() { String::new() }
        else { "Updating the equation…".into() }
    }
}
/// Opens the pane on `requested`, or else for a new equation of the kind `new`.
pub fn open(state: &mut CanvasState, requested: Option<NodeId>, new: EquationDisplay) -> Result<(), String> {
    if state.equation_draft.is_some() { close(state); }
    // Without `requested` the pane is for a new equation, even with the caret
    // beside one: editing one starts from selecting or double-clicking it.
    let original = match requested {
        Some(id) => Some(state.editor.equation_by_id(&id).ok_or("The selected equation no longer exists")?),
        None => None,
    };
    if original.is_none() && state.editor.selection.is_none() { return Err("Place the caret where the equation should be inserted.".into()); }
    let display = original.as_ref().map_or(new, |e| e.display);
    let committed = original.as_ref().map(|e| (e.root.clone(), e.display));
    let root = original.as_ref().map(|e| e.root.clone());
    state.editor.end_equation_edit();
    state.equation_draft = Some(EquationDraft {
        target: original.as_ref().map(|e| e.id.clone()), insertion: Equation::inline(EquationNode::ident("x")),
        selection: state.editor.selection.clone(),
        text: root.as_ref().map(|r| equation_input::format_with_display(r, display)).unwrap_or_default(),
        error: None, display, root, committed, changed_at: Instant::now(), pending: false,
        preview_generation: None, preview_ready: true, preview_live: false,
    });
    state.dragging_selection = false; state.object_drag = None;
    Ok(())
}
/// Updates the draft from the visual editor: `root` is the finished
/// equation, or why it cannot be finished yet.
pub fn set_root(state: &mut CanvasState, text: String, root: Result<EquationNode, String>) {
    let Some(draft) = state.equation_draft.as_mut() else { return; };
    draft.text = text; draft.changed_at = Instant::now(); draft.pending = true;
    match root { Ok(root) => { draft.root = Some(root); draft.error = None; },
        Err(error) => { draft.root = None; draft.error = Some(error); } }
}
pub fn root(state: &CanvasState) -> Option<EquationNode> { state.equation_draft.as_ref()?.root.clone() }
pub fn display(state: &CanvasState) -> EquationDisplay { state.equation_draft.as_ref().map_or(EquationDisplay::Inline, |d| d.display) }
/// Sets inline or display from the source's `$` delimiters.
pub fn set_display(state: &mut CanvasState, display: EquationDisplay) {
    let Some(draft) = state.equation_draft.as_mut() else { return; };
    if draft.display == display { return; }
    draft.display = display; draft.changed_at = Instant::now(); draft.pending = true;
}
/// Typing pauses this long before the page is laid out again; layout runs
/// on a worker and only the latest change is kept, so typing never queues.
const PAUSE: Duration = Duration::from_millis(60);

/// After a pause in editing, lays out the document with the change in it.
pub fn poll_preview(state: &mut CanvasState) {
    let inflight = state.layout_inflight;
    let Some(draft) = state.equation_draft.as_mut() else { return; };
    // A preview whose layout is no longer the one awaited (another layout
    // replaced it) never arrives: lay the change out again. And whatever
    // happened, the document catches up with the pane once it is idle.
    let lost = draft.preview_live && draft.preview_generation.is_some() && draft.preview_generation != inflight;
    let behind = !draft.pending && !draft.preview_live && draft.error.is_none() && draft.wanted().is_some() && draft.wanted() != draft.committed;
    if lost { draft.preview_live = false; draft.preview_generation = None; }
    if lost || behind { draft.pending = true; }
    if !draft.pending || draft.changed_at.elapsed() < PAUSE { return; }
    draft.pending = false;
    let Some(wanted) = draft.wanted() else { return; };
    if Some(&wanted) == draft.committed.as_ref() {
        // Back to what the document holds: show the document itself.
        if draft.preview_live { draft.preview_live = false; draft.preview_ready = true; submit_layout(state); refresh_overlay(state); }
        return;
    }
    let mut preview = EditorState::new(state.editor.document.clone());
    preview.selection = draft.selection.clone().or_else(|| state.editor.selection.clone());
    preview.typing_properties = state.editor.typing_properties.clone();
    if let Err(error) = change(&mut preview, draft, wanted) { draft.error = Some(error); return; }
    draft.preview_live = true; draft.preview_ready = false;
    state.layout_generation = state.layout_generation.wrapping_add(1);
    let generation = state.layout_generation;
    draft.preview_generation = Some(generation);
    state.scheduler.submit(generation, Arc::new(preview.document));
    state.layout_inflight = Some(generation);
    state.typing_layout_pending = false;
}
/// Puts `wanted` into `editor`: updates the target, or inserts the new
/// equation at the draft's caret.
fn change(editor: &mut EditorState, draft: &EquationDraft, (root, display): (EquationNode, EquationDisplay)) -> Result<(), String> {
    let mut equation = draft.insertion.clone();
    equation.root = root; equation.display = display;
    match &draft.target {
        Some(id) => {
            let mut existing = editor.equation_by_id(id).ok_or("The equation is no longer in the document")?;
            existing.root = equation.root; existing.display = equation.display; existing.source_hint = None;
            editor.update_equation_live(id, existing)
        }
        // A display equation goes on its own line, as Word places one.
        None if display == EquationDisplay::Block => editor.insert_display_equation(equation),
        None => editor.insert_inline_equation(equation),
    }
}
/// Called when the preview laid out (and is shown): the change goes into the
/// document. That document is what the preview laid out, so its layout stays
/// current and nothing is laid out again.
pub fn commit_preview(state: &mut CanvasState, generation: u64) {
    let Some(draft) = state.equation_draft.as_ref() else { return; };
    if draft.preview_generation != Some(generation) || !draft.preview_live { return; }
    let Some(wanted) = draft.wanted() else { return; };
    let caret = state.editor.selection.clone();
    let new = draft.target.is_none();
    if new { state.editor.selection = draft.selection.clone().or(caret.clone()); }
    let result = change(&mut state.editor, draft, wanted.clone());
    let draft = state.equation_draft.as_mut().expect("draft checked above");
    match result {
        Ok(()) => {
            if new { draft.target = Some(draft.insertion.id.clone()); }
            draft.committed = Some(wanted);
            state.selected_object = None;
        }
        Err(error) => { draft.error = Some(error); if new { state.editor.selection = caret; } }
    }
    let draft = state.equation_draft.as_mut().expect("draft checked above");
    draft.preview_live = false; draft.preview_ready = true; draft.preview_generation = None;
    let failed = draft.error.is_some();
    // The layout shown is the preview's; after a failed commit the document
    // differs from it and is laid out again.
    if failed { submit_layout(state); }
    refresh_overlay(state);
    state.needs_present = true;
}
/// The preview could not be laid out: the document keeps its equation.
pub fn preview_failed(state: &mut CanvasState, generation: u64, error: String) {
    let Some(draft) = state.equation_draft.as_mut() else { return; };
    if draft.preview_generation != Some(generation) { return; }
    draft.error = Some(error); draft.preview_live = false; draft.preview_ready = true; draft.preview_generation = None;
    submit_layout(state); refresh_overlay(state);
}
/// Whether the document is shown with an uncommitted change in it. Clicks
/// and keys then belong to the preview, not the document.
/// The equation on the page that the pane edits: the document's, or the new
/// one shown in a preview.
pub fn draft_equation(state: &CanvasState) -> Option<NodeId> {
    let draft = state.equation_draft.as_ref()?;
    draft.target.clone().or_else(|| draft.preview_live.then(|| draft.insertion.id.clone()))
}
pub fn previewing(state: &CanvasState) -> bool { state.equation_draft.as_ref().is_some_and(|d| d.preview_live) }
/// The document is being laid out for another reason: an equation preview
/// in flight is dropped, and laid out again after the next pause.
pub fn interrupt_preview(state: &mut CanvasState) {
    let Some(draft) = state.equation_draft.as_mut() else { return; };
    if draft.preview_live { draft.preview_live = false; draft.pending = true; draft.changed_at = Instant::now(); }
    draft.preview_generation = None;
}
/// Shows the document itself again, for editing it while the pane stays open.
pub fn show_document(state: &mut CanvasState) {
    if previewing(state) {
        if let Some(draft) = state.equation_draft.as_mut() { draft.preview_live = false; draft.pending = true; draft.changed_at = Instant::now(); }
        submit_layout(state); refresh_overlay(state);
    }
}
/// Closes the pane. The document already holds the last equation Typst could
/// render; an unfinished edit after it is dropped.
pub fn close(state: &mut CanvasState) {
    let Some(draft) = state.equation_draft.take() else { return; };
    state.equation_pane_slot = None;
    state.editor.end_equation_edit();
    state.layout_error = None;
    if draft.preview_live { submit_layout(state); refresh_overlay(state); }
}
