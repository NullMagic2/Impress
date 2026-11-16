//! Footnotes and Endnotes: the document's notes in reading order beside a
//! preview of the selected one, with Go to reference, Edit on page and Delete,
//! and a form that inserts a new footnote or endnote at the caret.
//!
//! The dialog only collects choices; every document change runs through the
//! editor after it closes, one undo step each.
use super::*;
use super::format_dialogs::Chrome;
use crate::NoteDialogUi;
use model::{inline_text, visit_paragraphs, Document, NoteKind};

/// Longest single-line summary shown in the notes list, in characters.
const LIST_PREVIEW_CHARS: usize = 70;
/// Longest note text accepted from the form (the editor's own limit).
const MAX_NOTE_BYTES: usize = 65_536;

/// One note as the list shows it.
#[derive(Clone, Debug, PartialEq)]
struct NoteRow {
    id: NodeId,
    kind: NoteKind,
    /// Number within its own kind: footnotes and endnotes count separately.
    number: usize,
    text: String,
}

impl NoteRow {
    fn label(&self) -> String {
        let kind = if self.kind == NoteKind::Endnote { "Endnote" } else { "Footnote" };
        let mut summary: String = self.text.replace(['\n', '\t'], " ").chars().take(LIST_PREVIEW_CHARS).collect();
        if self.text.chars().count() > LIST_PREVIEW_CHARS { summary.push('…'); }
        if summary.trim().is_empty() { summary = "(empty)".into(); }
        format!("{kind} {} · {summary}", self.number)
    }
}

/// The document's notes in the order their references appear in the body.
fn note_rows(document: &Document) -> Vec<NoteRow> {
    let references = model::footnote_references(document);
    let mut notes: Vec<_> = document.review.footnotes.values().collect();
    notes.sort_by_key(|note| references.iter().position(|(_, id)| id == &note.id).unwrap_or(usize::MAX));
    let (mut footnotes, mut endnotes) = (0, 0);
    notes.into_iter().map(|note| {
        let counter = if note.kind == NoteKind::Endnote { &mut endnotes } else { &mut footnotes };
        *counter += 1;
        let mut paragraphs = Vec::new();
        visit_paragraphs(&note.blocks, &mut paragraphs);
        let text = paragraphs.iter().map(|p| inline_text(&p.content)).collect::<Vec<_>>().join("\n");
        NoteRow { id: note.id.clone(), kind: note.kind, number: *counter, text }
    }).collect()
}

/// What the dialog asks the editor to do once it has closed.
#[derive(Debug, Default)]
struct NoteChoices {
    deleted: Vec<NodeId>,
    follow_up: Option<FollowUp>,
}

#[derive(Debug)]
enum FollowUp {
    Insert { kind: NoteKind, text: String },
    GoToReference(NodeId),
    EditOnPage(NodeId),
}

impl Ribbon {
    pub(super) fn footnotes(&self) -> Result<(), String> {
        let rows = note_rows(&self.state.borrow().editor.document);
        let choices = show_dialog(&self.ui.main_frame, rows);
        for id in &choices.deleted { self.edit(|e| e.delete_footnote(id))?; }
        match choices.follow_up {
            Some(FollowUp::Insert { kind: NoteKind::Footnote, text }) => self.edit(|e| e.insert_footnote(&text)),
            Some(FollowUp::Insert { kind: NoteKind::Endnote, text }) => self.edit(|e| e.insert_endnote(&text)),
            Some(FollowUp::GoToReference(id)) => self.edit(|e| {
                let references = model::footnote_references(&e.document);
                let (anchor, _) = references.into_iter().find(|(_, note)| note == &id).ok_or("This note has no reference in the body")?;
                e.go_to_range(&model::AnchorRange { start: anchor.clone(), end: anchor })
            }),
            Some(FollowUp::EditOnPage(id)) => self.edit(|e| e.enter_footnote(&id)),
            None => Ok(()),
        }
    }
}

fn show_dialog(frame: &impl WxWidget, rows: Vec<NoteRow>) -> NoteChoices {
    let ui = Rc::new(crate::i18n::localized(NoteDialogUi::new(Some(frame), false)));
    let chrome = Chrome::new(ui.note_dialog, ui.note_card, &[], &[], (ui.note_ok, "Insert"), None,
        &[(ui.note_cancel, "Close")],
        &[(ui.note_goto, "Go to reference"), (ui.note_edit, "Edit on page"), (ui.note_delete, "Delete")]);
    let dialog = ui.note_dialog;
    dialog.set_escape_id(ui.note_cancel.get_id());
    ui.note_cancel.on_click(move |_| dialog.end_modal(wxdragon::id::ID_CANCEL));
    ui.note_kind_footnote.set_value(true);

    let rows = Rc::new(RefCell::new(rows));
    let choices = Rc::new(RefCell::new(NoteChoices::default()));
    let selected_id = { let ui = Rc::clone(&ui); let rows = Rc::clone(&rows);
        move || ui.note_list.get_selection().and_then(|i| rows.borrow().get(i as usize).map(|row| row.id.clone())) };

    // Fill the list and keep the preview, actions and empty hint in step.
    let refresh: Rc<dyn Fn()> = { let ui = Rc::clone(&ui); let rows = Rc::clone(&rows);
        Rc::new(move || {
            let rows = rows.borrow();
            let selection = ui.note_list.get_selection().filter(|i| (*i as usize) < rows.len());
            let selected = selection.and_then(|i| rows.get(i as usize));
            ui.note_preview.set_value(selected.map_or("Select a note to see its text.", |row| row.text.as_str()));
            for action in [ui.note_goto, ui.note_edit, ui.note_delete] { action.enable(selected.is_some()); }
            ui.note_empty.show(rows.is_empty());
            ui.note_card.layout();
        }) };
    let fill: Rc<dyn Fn(Option<usize>)> = { let ui = Rc::clone(&ui); let rows = Rc::clone(&rows); let refresh = Rc::clone(&refresh);
        Rc::new(move |select: Option<usize>| {
            ui.note_list.clear();
            for row in rows.borrow().iter() { ui.note_list.append(&row.label()); }
            let count = rows.borrow().len();
            if let Some(index) = select.filter(|_| count > 0) { ui.note_list.set_selection(index.min(count - 1) as u32, true); }
            refresh();
        }) };
    fill(Some(0));

    { let refresh = Rc::clone(&refresh); ui.note_list.on_selection_changed(move |_| refresh()); }
    // Double-clicking a note opens it on the page, like Edit on page.
    { let choices = Rc::clone(&choices); let selected_id = selected_id.clone();
      ui.note_list.on_item_double_clicked(move |_| if let Some(id) = selected_id() {
          choices.borrow_mut().follow_up = Some(FollowUp::EditOnPage(id)); dialog.end_modal(wxdragon::id::ID_CANCEL);
      }); }
    for (button, follow_up) in [(ui.note_goto, FollowUp::GoToReference as fn(NodeId) -> FollowUp), (ui.note_edit, FollowUp::EditOnPage)] {
        let choices = Rc::clone(&choices); let selected_id = selected_id.clone();
        button.on_click(move |_| if let Some(id) = selected_id() {
            choices.borrow_mut().follow_up = Some(follow_up(id)); dialog.end_modal(wxdragon::id::ID_CANCEL);
        });
    }
    // Delete removes the row now; the document changes after the dialog closes.
    { let choices = Rc::clone(&choices); let rows = Rc::clone(&rows); let fill = Rc::clone(&fill); let ui2 = Rc::clone(&ui);
      ui.note_delete.on_click(move |_| {
          let Some(index) = ui2.note_list.get_selection().map(|i| i as usize) else { return; };
          if index >= rows.borrow().len() { return; }
          let removed = rows.borrow_mut().remove(index);
          choices.borrow_mut().deleted.push(removed.id);
          fill(Some(index));
      }); }

    // Insert is available only while there is note text to insert.
    let update_insert = { let ui = Rc::clone(&ui);
        move || { let text = ui.note_text.get_value(); ui.note_ok.enable(!text.trim().is_empty() && text.len() <= MAX_NOTE_BYTES); } };
    update_insert();
    { let update_insert = update_insert.clone(); ui.note_text.on_text_updated(move |_| update_insert()); }

    crate::placement::centre(&ui.note_dialog);
    ui.note_text.set_focus();
    let inserted = chrome.show();
    if inserted {
        let kind = if ui.note_kind_endnote.get_value() { NoteKind::Endnote } else { NoteKind::Footnote };
        choices.borrow_mut().follow_up = Some(FollowUp::Insert { kind, text: ui.note_text.get_value().trim().to_string() });
    }
    ui.note_dialog.destroy();
    choices.take()
}

