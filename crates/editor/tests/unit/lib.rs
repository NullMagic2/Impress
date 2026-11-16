use super::*;
use model::{Equation, EquationNode};

fn editor_with(text: &str) -> EditorState {
    let mut doc = Document::default();
    let p = Paragraph::plain(text);
    let id = p.id.clone();
    doc.sections[0].blocks.push(Block::Paragraph(p));
    let mut editor = EditorState::new(doc);
    editor.set_caret(id, text.len());
    editor
}

#[test]
fn control_arrows_move_by_unicode_words_and_shift_extends_from_anchor() {
    let mut editor = editor_with("alpha beta café");
    let id = editor.focus_position().unwrap().node_id;
    editor.set_caret(id, 0);
    editor.move_word_right(false);
    assert_eq!(editor.focus_position().unwrap().byte_offset, 6);
    editor.move_word_right(true);
    let selection = editor.selection.as_ref().unwrap();
    assert_eq!(selection.anchor.byte_offset, 6);
    assert_eq!(selection.focus.byte_offset, 11);
    editor.move_word_right(true);
    assert_eq!(editor.selection.as_ref().unwrap().focus.byte_offset, "alpha beta café".len());
    editor.move_word_left(true);
    assert_eq!(editor.selection.as_ref().unwrap().focus.byte_offset, 11);
    editor.move_word_left(false);
    assert_eq!(editor.focus_position().unwrap().byte_offset, 6);
}

#[test]
fn typing_is_undoable_without_document_snapshots() {
    let mut e = editor_with("Hi");
    e.insert_text("!").unwrap();
    assert_eq!(e.paragraph_text(&e.focus_position().unwrap().node_id).unwrap(), "Hi!");
    assert!(e.undo());
    assert_eq!(e.paragraph_text(&e.focus_position().unwrap().node_id).unwrap(), "Hi");
}

#[test]
fn undo_back_to_saved_history_is_clean() {
    let mut e = editor_with("Hi");
    e.insert_text("!").unwrap();
    e.mark_saved();
    assert!(!e.dirty);
    e.insert_text("?").unwrap();
    assert!(e.dirty);
    assert!(e.undo());
    assert!(!e.dirty);
    assert!(e.redo());
    assert!(e.dirty);
}

#[test]
fn split_and_merge_paragraph() {
    let mut e = editor_with("abcd");
    let id = e.focus_position().unwrap().node_id;
    e.set_caret(id, 2);
    e.split_paragraph().unwrap();
    assert_eq!(paragraph_positions(&e.document).len(), 2);
    e.backspace().unwrap();
    assert_eq!(paragraph_positions(&e.document).len(), 1);
}

#[test]
fn inline_equation_is_atomic_in_flat_stream() {
    let mut e = editor_with("ab");
    let id = e.focus_position().unwrap().node_id;
    e.set_caret(id.clone(), 1);
    e.insert_inline_equation(Equation::inline(EquationNode::ident("x"))).unwrap();
    assert_eq!(e.paragraph_text(&id).unwrap(), format!("a{}b", INLINE_OBJECT_MARKER));
    e.backspace().unwrap();
    assert_eq!(e.paragraph_text(&id).unwrap(), "ab");
}

#[test]
fn typed_typst_math_becomes_an_equation() {
    let mut e = editor_with("");
    let id = e.focus_position().unwrap().node_id;
    for c in "a $x/2$".chars() { e.insert_text(&c.to_string()).unwrap(); e.convert_typed_math().unwrap(); }
    assert_eq!(e.paragraph_text(&id).unwrap(), format!("a {}", INLINE_OBJECT_MARKER));
    let Inline::Equation(eq) = &e.paragraph(&id).unwrap().content[1] else { panic!("no equation") };
    assert!(matches!(eq.root, EquationNode::Fraction { .. }));
    assert_eq!(eq.display, model::EquationDisplay::Inline);
    assert!(e.undo());
    assert_eq!(e.paragraph_text(&id).unwrap(), "a $x/2$", "undo gives the typed source back");

    let mut e = editor_with("");
    let id = e.focus_position().unwrap().node_id;
    for c in "$ sum_(k=1)^n k $".chars() { e.insert_text(&c.to_string()).unwrap(); e.convert_typed_math().unwrap(); }
    let Inline::Equation(eq) = &e.paragraph(&id).unwrap().content[0] else { panic!("no equation") };
    assert_eq!(eq.display, model::EquationDisplay::Block);

    // Prices and escaped dollars stay text.
    let mut e = editor_with("");
    let id = e.focus_position().unwrap().node_id;
    for c in "costs $5 and $10, \\$x\\$".chars() { e.insert_text(&c.to_string()).unwrap(); e.convert_typed_math().unwrap(); }
    assert_eq!(e.paragraph_text(&id).unwrap(), "costs $5 and $10, \\$x\\$");
}

#[test]
fn composition_commits_as_one_undo_transaction() {
    let mut e = editor_with("a");
    e.begin_composition().unwrap();
    e.update_composition("α").unwrap();
    e.update_composition("ά").unwrap();
    assert_eq!(e.paragraph_text(&e.focus_position().unwrap().node_id).unwrap(), "a");
    e.commit_composition().unwrap();
    assert_eq!(e.paragraph_text(&e.focus_position().unwrap().node_id).unwrap(), "aά");
    assert!(e.undo());
    assert_eq!(e.paragraph_text(&e.focus_position().unwrap().node_id).unwrap(), "a");
}

#[test]
fn nested_cell_selection_cannot_leak_into_neighbour_cell() {
    let mut document = Document::default();
    let left = Paragraph::plain("left"); let left_id = left.id.clone();
    let right = Paragraph::plain("right"); let right_id = right.id.clone();
    document.sections[0].blocks.push(Block::Table(model::Table {
        id: NodeId::new(), properties: Default::default(), rows: vec![model::TableRow {
            id: NodeId::new(), repeat_as_header: false, allow_break_across_pages: true, height_mm: None,
            cells: vec![
                model::TableCell { id: NodeId::new(), blocks: (vec![Block::Paragraph(left)]).into(), column_span: 1, row_span: 1, width_mm: None, shading: None, borders: None, vertical_alignment: Default::default() },
                model::TableCell { id: NodeId::new(), blocks: (vec![Block::Paragraph(right)]).into(), column_span: 1, row_span: 1, width_mm: None, shading: None, borders: None, vertical_alignment: Default::default() },
            ],
        }],
    }));
    let mut editor = EditorState::new(document);
    editor.selection = Some(Selection { anchor: DocumentPosition::new(left_id, 0), focus: DocumentPosition::new(right_id, 5) });
    assert!(editor.selected_text().is_none());
    assert!(editor.delete_selection().is_err());
}

#[test]
fn enter_on_empty_list_item_exits_list_without_creating_paragraph() {
    let mut document = Document::default();
    let list_id = NodeId::new();
    document.lists.insert(list_id.clone(), ListDefinition {
        id: list_id.clone(),
        levels: vec![model::ListLevel {
            level: 0, format: model::ListFormat::Bullet, text: "•".into(), start_at: 1,
            left_indent_mm: 6.0, hanging_indent_mm: 3.0,
        }],
    });
    let mut paragraph = Paragraph::plain("");
    paragraph.properties.list_id = Some(list_id);
    paragraph.properties.list_level = Some(0);
    let id = paragraph.id.clone();
    document.sections[0].blocks.push(Block::Paragraph(paragraph));
    let mut editor = EditorState::new(document);
    editor.set_caret(id.clone(), 0);
    editor.split_paragraph().unwrap();
    assert_eq!(paragraph_positions(&editor.document).len(), 1);
    let paragraph = editor.paragraph(&id).unwrap();
    assert!(paragraph.properties.list_id.is_none());
    assert!(paragraph.properties.list_level.is_none());
    assert!(editor.undo());
    assert!(editor.paragraph(&id).unwrap().properties.list_id.is_some());
}

#[test]
fn double_click_word_selection_handles_greek_and_combining_accents_without_editing() {
    let text="Hello, κόσμος! cafe\u{301} next";
    let mut editor=editor_with(text);
    let id=editor.focus_position().unwrap().node_id;
    let revision=editor.revision;
    assert!(editor.select_word_at(id.clone(),text.find("κόσμος").unwrap()+"κό".len()));
    assert_eq!(editor.selected_text().as_deref(),Some("κόσμος"));
    assert!(editor.select_word_at(id.clone(),text.find("cafe").unwrap()+2));
    assert_eq!(editor.selected_text().as_deref(),Some("cafe\u{301}"));
    assert!(editor.select_word_at(id.clone(),"Hello".len()));
    assert_eq!(editor.selected_text().as_deref(),Some("Hello"));
    assert_eq!(editor.revision,revision);
    assert!(!editor.can_undo());
}

#[test]
fn batched_deletion_preserves_graphemes_and_undo_and_matches_individual_keys() {
    let text="start β e\u{301} 👩\u{200d}💻 end";
    for backward in [false,true] {
        for offset in [0,"start β ".len(),text.len()] {
            let mut reference=editor_with(text);
            let id=reference.focus_position().unwrap().node_id;
            reference.set_caret(id.clone(),offset);
            let mut batched=EditorState::new(reference.document.clone());
            batched.set_caret(id,offset);
            for _ in 0..4 {if backward {reference.backspace().unwrap();}else{reference.delete_forward().unwrap();}}
            batched.delete_repeated(backward,4).unwrap();
            let compare_text = |e: &EditorState| e.paragraph_text(&e.focus_position().unwrap().node_id);
            assert_eq!(compare_text(&batched),compare_text(&reference));
            assert_eq!(batched.selection,reference.selection);
            if batched.can_undo() {
                assert!(batched.undo());
                assert_eq!(batched.paragraph_text(&batched.focus_position().unwrap().node_id).as_deref(),Some(text));
                assert!(batched.redo());
                assert_eq!(compare_text(&batched),compare_text(&reference));
            }
        }
    }
}

#[test]
fn one_repeat_tick_deletes_long_paragraph_with_one_revision_and_restores_all_text() {
    let text="Greek Ελληνικά e\u{301} 👩\u{200d}💻 ".repeat(2000);
    let mut editor=editor_with(&text);
    let id=editor.focus_position().unwrap().node_id;
    let revision=editor.revision;
    editor.delete_repeated(true,4).unwrap();
    assert_eq!(editor.revision,revision+1);
    assert!(editor.undo());
    assert_eq!(editor.paragraph_text(&id).as_deref(),Some(text.as_str()));
}
