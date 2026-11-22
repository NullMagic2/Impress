//! The documents open in this window. One is shown; the others are parked
//! with everything that belongs to them (editor and undo, file path,
//! recovery snapshot, last layout, zoom and scroll). Open, New, Import PDF
//! and Recover add a document to the window's list instead of replacing the
//! one shown.
//! Launching Impress again opens a separate window with its own stack.
use super::*;

/// What a document keeps while another is shown.
pub(crate) struct Parked {
    editor: EditorState,
    snapshot: Option<(u64, Arc<Document>)>,
    current_path: Option<PathBuf>,
    recovery: recovery::Recovery,
    vector_document: Arc<VectorDocument>,
    zoom: f32,
    scroll: Point,
    search_marks: Option<(u64, u64, Vec<render::ColoredOverlayRect>)>,
    inline_spelling: inline_spelling::InlineSpelling,
    revisions: ribbon::revision_tools::ReviewView,
    caret_page_hint: Option<usize>,
    preferred_caret_x_pt: Option<f32>,
    caret_visual_line: Option<usize>,
    layout_error: Option<String>,
}

struct Entry {
    id: u64,
    /// Untitled documents are numbered in the order they were made.
    untitled: u32,
    /// None for the document shown.
    parked: Option<Box<Parked>>,
}

/// A command the Ribbon carries out on its next poll, because it may need a
/// dialog (keyboard shortcuts arrive in the canvas).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Request { Close(u64) }

/// One row of the document switcher.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Row {
    pub id: u64,
    pub name: String,
    pub folder: String,
    pub dirty: bool,
    pub active: bool,
}

pub(crate) struct Stack {
    /// In the order they were opened; exactly one entry (the document shown) is not parked.
    entries: Vec<Entry>,
    next_id: u64,
    next_untitled: u32,
    /// Changes whenever another document is shown, so views that cache per
    /// document (Ribbon panes, search) start again.
    pub epoch: u64,
    pub request: Option<Request>,
    /// Close this document once its save has finished.
    close_after_save: Option<u64>,
    /// Impress was started with a file to open: the blank document it starts
    /// with stands in for that file and is replaced by it, not kept beside it.
    pub startup_placeholder: bool,
}

impl Default for Stack {
    fn default() -> Self {
        Self { entries: vec![Entry { id: 1, untitled: 1, parked: None }], next_id: 2, next_untitled: 2,
            epoch: 0, request: None, close_after_save: None, startup_placeholder: false }
    }
}

impl Stack {
    pub fn len(&self) -> usize { self.entries.len() }
    pub fn active_id(&self) -> u64 { self.entries.iter().find(|e| e.parked.is_none()).map_or(0, |e| e.id) }
    fn active_index(&self) -> usize { self.entries.iter().position(|e| e.parked.is_none()).unwrap_or(0) }
    fn index_of(&self, id: u64) -> Option<usize> { self.entries.iter().position(|e| e.id == id) }
    /// The id of the document at a position in the switcher (Alt+1 is 0).
    pub fn id_at(&self, index: usize) -> Option<u64> { self.entries.get(index).map(|e| e.id) }
}

fn untitled_name(number: u32) -> String {
    if number <= 1 { format!("{}.impress",crate::i18n::text("Untitled")) } else { format!("{} {number}.impress",crate::i18n::text("Untitled")) }
}

/// The folder as people say it: `~/Documents` rather than `/home/me/Documents`.
fn folder_label(path: Option<&Path>) -> String {
    let Some(parent) = path.and_then(Path::parent) else { return "Not saved yet".into(); };
    let text = parent.display().to_string();
    match std::env::var_os("HOME").map(PathBuf::from) {
        Some(home) if parent.starts_with(&home) => {
            let rest = parent.strip_prefix(&home).map(|p| p.display().to_string()).unwrap_or_default();
            if rest.is_empty() { "~".into() } else { format!("~/{rest}") }
        }
        _ => text,
    }
}

fn file_name(path: Option<&Path>, untitled: u32) -> String {
    path.and_then(Path::file_name).map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| untitled_name(untitled))
}

/// The name of the document shown, for the window title.
pub(crate) fn active_name(s: &CanvasState) -> String {
    let untitled = s.documents.entries.get(s.documents.active_index()).map_or(1, |e| e.untitled);
    file_name(s.current_path.as_deref(), untitled)
}

/// The switcher's rows, in the order the documents were opened.
pub(crate) fn rows(s: &CanvasState) -> Vec<Row> {
    s.documents.entries.iter().map(|entry| match &entry.parked {
        None => Row { id: entry.id, name: file_name(s.current_path.as_deref(), entry.untitled),
            folder: folder_label(s.current_path.as_deref()), dirty: s.editor.dirty, active: true },
        Some(p) => Row { id: entry.id, name: file_name(p.current_path.as_deref(), entry.untitled),
            folder: folder_label(p.current_path.as_deref()), dirty: p.editor.dirty, active: false },
    }).collect()
}

/// The document already open from `path`, if any.
pub(crate) fn find_path(s: &CanvasState, path: &Path) -> Option<u64> {
    let same = |candidate: Option<&Path>| candidate.is_some_and(|c| c == path
        || std::fs::canonicalize(c).ok().zip(std::fs::canonicalize(path).ok()).is_some_and(|(a, b)| a == b));
    s.documents.entries.iter().find(|entry| match &entry.parked {
        None => same(s.current_path.as_deref()),
        Some(p) => same(p.current_path.as_deref()),
    }).map(|e| e.id)
}

/// Why another document cannot be shown right now, if it cannot.
pub(crate) fn busy(s: &CanvasState) -> Option<&'static str> {
    if s.file_job.is_some() || s.clipboard_job.is_some() || s.output_job.is_some() {
        Some("Wait for the current file, clipboard or print operation to finish")
    } else if s.pending_close {
        Some("Impress is closing")
    } else if s.object_drag.is_some() || s.input.active() {
        Some("Finish the current edit first")
    } else { None }
}

/// The first document with unsaved changes, the one shown first.
pub(crate) fn first_dirty(s: &CanvasState) -> Option<u64> {
    if s.editor.dirty { return Some(s.documents.active_id()); }
    s.documents.entries.iter().find(|e| e.parked.as_ref().is_some_and(|p| p.editor.dirty)).map(|e| e.id)
}

/// Swaps the document shown with a parked one.
fn exchange(s: &mut CanvasState, p: &mut Parked) {
    std::mem::swap(&mut s.editor, &mut p.editor);
    std::mem::swap(&mut s.snapshot, &mut p.snapshot);
    std::mem::swap(&mut s.current_path, &mut p.current_path);
    std::mem::swap(&mut s.recovery, &mut p.recovery);
    std::mem::swap(&mut s.vector_document, &mut p.vector_document);
    std::mem::swap(&mut s.viewport.zoom, &mut p.zoom);
    std::mem::swap(&mut s.viewport.scroll_px, &mut p.scroll);
    std::mem::swap(&mut s.search_marks, &mut p.search_marks);
    std::mem::swap(&mut s.inline_spelling, &mut p.inline_spelling);
    std::mem::swap(&mut s.revisions, &mut p.revisions);
    std::mem::swap(&mut s.caret_page_hint, &mut p.caret_page_hint);
    std::mem::swap(&mut s.preferred_caret_x_pt, &mut p.preferred_caret_x_pt);
    std::mem::swap(&mut s.caret_visual_line, &mut p.caret_visual_line);
    std::mem::swap(&mut s.layout_error, &mut p.layout_error);
}

/// Forgets what the pointer and selection tools were doing in the previous
/// document and lays out the one now shown.
fn shown(s: &mut CanvasState) {
    s.selected_object = None; s.object_drag = None; s.layout_menu_request = None;
    s.table_pointer = None; s.table_hover = None; s.picture_hover = None; s.table_band = None;
    s.table_drag = None; s.band_drag = None; s.cell_drag_anchor = None;
    s.key_repeat = None; s.dragging_selection = false; s.wheel_glide = (0.0, 0.0);
    s.pending_equation = None; s.pending_comment = None; s.comment_selected = None; s.pending_new_comment = false; s.comment_leaders.clear();
    s.overlay = CanvasOverlay::default(); s.display_generation = None; s.typing_layout_pending = false;
    s.documents.epoch = s.documents.epoch.wrapping_add(1);
    submit_layout(s); refresh_overlay(s); wake_caret(s);
    s.needs_present = true;
}

/// Shows another open document.
pub(crate) fn switch_to(s: &mut CanvasState, id: u64) -> Result<(), String> {
    let target = s.documents.index_of(id).ok_or("That document is no longer open")?;
    let active = s.documents.active_index();
    if target == active { return Ok(()); }
    if let Some(reason) = busy(s) { return Err(reason.into()); }
    // The equation pane edits the document shown; its changes are already in it.
    crate::equation_ui::close(s);
    let mut parked = s.documents.entries[target].parked.take().expect("parked document");
    exchange(s, &mut parked);
    s.documents.entries[active].parked = Some(parked);
    s.action_status = None;
    shown(s);
    Ok(())
}

/// Shows `document`, added at the end of the window's list of documents.
pub(crate) fn add(s: &mut CanvasState, document: Document, path: Option<PathBuf>) -> Result<(), String> {
    if let Some(reason) = busy(s) { return Err(reason.into()); }
    // The equation pane edits the document shown; its changes are already in it.
    crate::equation_ui::close(s);
    if std::mem::take(&mut s.documents.startup_placeholder) && s.documents.entries.len() == 1
        && !s.editor.dirty && !s.editor.can_undo() && s.current_path.is_none() {
        s.recovery.mark_clean(); s.editor.replace_document(document); s.snapshot = None; s.current_path = path;
        s.vector_document = Arc::new(VectorDocument::default());
        s.viewport.scroll_px = Point::new(0.0, 0.0);
        shown(s);
        return Ok(());
    }
    let mut editor = EditorState::new(Document::default());
    editor.replace_document(document);
    let untitled = if path.is_none() { let n = s.documents.next_untitled; s.documents.next_untitled += 1; n } else { 0 };
    let parked = Parked {
        editor, snapshot: None, current_path: path, recovery: recovery::Recovery::new(),
        vector_document: Arc::new(VectorDocument::default()), zoom: s.viewport.zoom, scroll: Point::new(0.0, 0.0),
        search_marks: None, inline_spelling: Default::default(), revisions: Default::default(), caret_page_hint: None,
        preferred_caret_x_pt: None, caret_visual_line: None, layout_error: None,
    };
    let id = s.documents.next_id; s.documents.next_id += 1;
    s.documents.entries.push(Entry { id, untitled, parked: Some(Box::new(parked)) });
    switch_to(s, id)
}

/// A new blank document, as File > New makes it.
pub(crate) fn blank() -> Document {
    let mut document = document_defaults::new_document();
    document.sections[0].blocks.push(Block::Paragraph(Paragraph::plain("")));
    document
}

/// Closes a document whose changes were saved or deliberately discarded.
/// Closing the last one leaves a blank document, as the window stays open.
pub(crate) fn close(s: &mut CanvasState, id: u64) -> Result<(), String> {
    let index = s.documents.index_of(id).ok_or("That document is no longer open")?;
    if s.documents.entries.len() == 1 {
        if let Some(reason) = busy(s) { return Err(reason.into()); }
    // The equation pane edits the document shown; its changes are already in it.
    crate::equation_ui::close(s);
        s.recovery.mark_clean();
        s.editor.replace_document(blank()); s.current_path = None; s.snapshot = None;
        s.vector_document = Arc::new(VectorDocument::default());
        s.viewport.scroll_px = Point::new(0.0, 0.0);
        s.documents.entries[0].untitled = 1; s.documents.next_untitled = 2;
        shown(s);
        return Ok(());
    }
    if s.documents.entries[index].parked.is_none() {
        // Show the next document down the stack, else the one above.
        let next = s.documents.entries.get(index + 1).or_else(|| s.documents.entries.get(index - 1)).map(|e| e.id).expect("another document");
        switch_to(s, next)?;
    }
    let mut entry = s.documents.entries.remove(s.documents.index_of(id).expect("still open"));
    if let Some(parked) = entry.parked.as_mut() { parked.recovery.mark_clean(); }
    if s.documents.close_after_save == Some(id) { s.documents.close_after_save = None; }
    s.documents.epoch = s.documents.epoch.wrapping_add(1);
    s.needs_present = true;
    Ok(())
}

/// Close the document shown once the save just started has finished.
pub(crate) fn close_when_saved(s: &mut CanvasState) {
    s.documents.close_after_save = s.file_job.is_some().then(|| s.documents.active_id());
}

/// A save finished: close its document if that was asked and nothing was
/// typed meanwhile.
pub(crate) fn saved(s: &mut CanvasState, revision: u64) {
    let Some(id) = s.documents.close_after_save.take() else { return; };
    if id != s.documents.active_id() { return; }
    if s.editor.revision == revision && !s.editor.dirty {
        if let Err(error) = close(s, id) { s.action_status = Some(error); }
    } else {
        s.action_status = Some("The document changed while it was saved, so it stays open".into());
    }
}

/// Keeps recovery snapshots of parked documents current (each writes at
/// most once per period, and only when its document changed).
pub(crate) fn poll_parked_recovery(s: &mut CanvasState) {
    let formats = Arc::clone(&s.formats);
    for entry in &mut s.documents.entries {
        if let Some(p) = entry.parked.as_mut() {
            let snapshot = p.snapshot.as_ref().filter(|(r, _)| *r == p.editor.revision).map(|(_, d)| Arc::clone(d));
            p.recovery.poll_snapshot(&p.editor, p.current_path.as_deref(), &formats, snapshot);
        }
    }
}

/// During a forced shutdown, snapshots every parked document with changes.
pub(crate) fn emergency_snapshots(s: &CanvasState) {
    for p in s.documents.entries.iter().filter_map(|e| e.parked.as_ref()) {
        let _ = p.recovery.emergency_snapshot(&p.editor, p.current_path.as_deref(), &s.formats);
    }
}

