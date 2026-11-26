use super::*;
use model::{Document, Paragraph, Table, TableCell, TableRow};

fn document(text: &str) -> (EditorState, NodeId) {
    let mut doc = Document::default();
    let p = Paragraph::plain(text);
    let id = p.id.clone();
    doc.sections[0].blocks.push(Block::Paragraph(p));
    (EditorState::new(doc), id)
}

#[test]
fn greek_accents_quotes_and_symbols_are_counted_as_text_not_punctuation() {
    assert_eq!(navigation::count_words("Αγαπητή Μαρία, όλα καλά!"), 4);
    assert_eq!(navigation::count_words("cafe\u{301} ι\u{308}\u{301} \"\" «» — 👩\u{200d}💻"), 2);
    assert_eq!(navigation::count_words("one\u{fffc}two\tthree\nfour"), 4);
}

#[test]
fn live_total_tracks_edits_undo_and_partial_or_reversed_selections() {
    let (mut editor, id) = document("Αγαπητή Μαρία");
    let mut count = State::default();
    count.update(&editor, 0, None);
    assert_eq!(count.label(), "2 words");
    editor.set_caret(id.clone(), "Αγαπητή Μαρία".len());
    editor.insert_text(" κόσμος").unwrap();
    count.update(&editor, 0, None);
    assert_eq!(count.label(), "3 words");
    editor.extend_selection(id.clone(), "Αγαπητή ".len());
    count.update(&editor, 0, None);
    assert_eq!(count.label(), "2 of 3 words");
    editor.set_caret(id.clone(), 0);
    editor.extend_selection(id.clone(), "Αγα".len());
    count.update(&editor, 0, None);
    assert_eq!(count.label(), "1 of 3 words");
    assert!(editor.undo());
    count.update(&editor, 0, None);
    assert_eq!(count.stats.words, 2);
    editor.set_caret(id, 0);
    count.update(&editor, 0, None);
    assert_eq!(count.label(), "2 words");
}

fn cell(text: &str) -> TableCell {
    TableCell { id: NodeId::new(), blocks: vec![Block::Paragraph(Paragraph::plain(text))].into(),
        column_span: 1, row_span: 1, width_mm: None, shading: None, borders: None,
        vertical_alignment: model::CellVerticalAlignment::Top, margins: None, text_direction: None }
}

#[test]
fn table_words_and_selected_cells_are_counted_once() {
    let (mut editor, _) = document("Body text");
    let a = cell("First cell"); let a_id = a.id.clone();
    let b = cell("Second cell words"); let b_id = b.id.clone();
    editor.document.sections[0].blocks.push(Block::Table(Table {
        id: NodeId::new(), properties: Default::default(), rows: vec![TableRow {
            id: NodeId::new(), cells: vec![a, b], repeat_as_header: false,
            allow_break_across_pages: true, height_mm: None,
        }],
    }));
    let mut count = State::default();
    count.update(&editor, 0, Some(&[a_id.clone()]));
    assert_eq!(count.label(), "2 of 7 words");
    count.update(&editor, 0, Some(&[a_id, b_id]));
    assert_eq!(count.label(), "5 of 7 words");
    assert_eq!(count.stats, navigation::count_body(&editor.document));
}

#[test]
fn new_documents_at_the_same_revision_do_not_retain_the_previous_count() {
    let (first, _) = document("One two three");
    let (second, _) = document("");
    let mut count = State::default();
    count.update(&first, 1, None);
    count.update(&second, 2, None);
    assert_eq!(count.label(), "0 words");
    assert_eq!(count.paragraphs.len(), 1);
    assert_eq!(count.stats, navigation::count_body(&second.document));
}

#[test]
fn document_wide_selection_uses_paragraph_order_and_updates_after_deletion() {
    let (mut editor, first) = document("first two");
    let last = Paragraph::plain("last three words"); let last_id = last.id.clone();
    editor.document.sections[0].blocks.extend([
        Block::Paragraph(Paragraph::plain("middle words")), Block::Paragraph(last)]);
    let mut count = State::default();
    editor.set_caret(last_id, "last three ".len());
    editor.extend_selection(first, "first ".len());
    count.update(&editor, 0, None);
    assert_eq!(count.label(), "5 of 7 words");
    editor.delete_selection().unwrap();
    count.update(&editor, 0, None);
    assert_eq!(count.stats, navigation::count_body(&editor.document));
    assert_eq!(count.paragraphs.len(), 1);
}
