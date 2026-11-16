use super::*;
fn editor(text: &str) -> EditorState {
    let mut document = Document::default();
    let p = Paragraph::plain(text);
    let id = p.id.clone();
    document.sections[0].blocks.push(Block::Paragraph(p));
    let mut e = EditorState::new(document); e.set_caret(id, 0); e
}
#[test]
fn ribbon_font_change_preserves_unselected_text_and_undo() {
    let mut e = editor("abcdef"); let id = e.focus_position().unwrap().node_id;
    let before = e.document.clone(); e.set_caret(id.clone(), 1); e.extend_selection(id, 4);
    e.apply_character_properties(&CharacterProperties { font_size_pt: Some(18.0), ..Default::default() }).unwrap();
    assert_eq!(e.formatting_summary().character.font_size_pt, Some(18.0));
    assert_eq!(e.selected_text().as_deref(), Some("bcd"));
    assert!(e.undo()); assert_eq!(e.document, before);
    assert!(e.redo()); assert_eq!(e.formatting_summary().character.font_size_pt, Some(18.0));
}
#[test]
fn list_toggle_creates_definition_and_is_one_undo_record() {
    let mut e=editor("item"); let before=e.document.clone();
    e.toggle_list(ListFormat::Bullet).unwrap();
    assert_eq!(e.document.lists.len(),1);
    let p=e.paragraph(&e.focus_position().unwrap().node_id).unwrap();
    assert!(p.properties.list_id.is_some());
    assert!(e.undo()); assert_eq!(e.document,before); assert!(!e.can_undo());
    assert!(e.redo()); assert_eq!(e.document.lists.len(),1);
}

#[test]
fn table_row_column_merge_split_commands_are_undoable() {
    let mut e=editor("");
    e.insert_table(2,2).unwrap();
    let before=e.document.clone();
    e.insert_table_row(true).unwrap();
    assert!(e.undo()); assert_eq!(e.document,before);
    e.insert_table_column(true).unwrap();
    assert!(e.undo()); assert_eq!(e.document,before);
    e.merge_table_cell_right().unwrap();
    let context=e.table_cell_context().unwrap();
    let table=find_table_in_blocks(&e.document.sections[context.section_index].blocks,&context.table_id).unwrap();
    assert_eq!(table.rows[context.row_index].cells[context.cell_index].column_span,2);
    e.split_table_cell().unwrap();
    let context=e.table_cell_context().unwrap();
    let table=find_table_in_blocks(&e.document.sections[context.section_index].blocks,&context.table_id).unwrap();
    assert_eq!(table.rows[context.row_index].cells.len(),2);
}

#[test]
fn header_footer_paragraphs_use_normal_typing_and_escape_target() {
    let mut e=editor("body");
    e.enter_header(HeaderFooterVariant::Default).unwrap();
    assert!(e.is_editing_header_footer());
    e.insert_text("Header").unwrap();
    let header=e.document.sections[0].headers.default.as_ref().unwrap();
    let id=first_paragraph_id(&header.blocks).unwrap();
    assert_eq!(e.paragraph_text(&id).as_deref(),Some("Header"));
    e.exit_header_footer().unwrap();
    assert!(!e.is_editing_header_footer());
}

#[test]
fn section_break_splits_body_and_undo_restores_one_section() {
    let mut e=editor("abcd");
    let id=e.focus_position().unwrap().node_id;
    e.set_caret(id,2);
    let before=e.document.clone();
    e.insert_section_break(SectionStart::Continuous).unwrap();
    assert_eq!(e.document.sections.len(),2);
    assert_eq!(e.document.sections[1].properties.start_on,SectionStart::Continuous);
    let first=first_paragraph_id(&e.document.sections[0].blocks).unwrap();
    let second=first_paragraph_id(&e.document.sections[1].blocks).unwrap();
    assert_eq!(e.paragraph_text(&first).as_deref(),Some("ab"));
    assert_eq!(e.paragraph_text(&second).as_deref(),Some("cd"));
    assert!(e.undo()); assert_eq!(e.document,before);
    assert!(e.redo()); assert_eq!(e.document.sections.len(),2);
}


fn paragraph_style_boundary_editor(reverse: bool, selected_second_bytes: usize) -> (EditorState, NodeId, NodeId) {
    let mut document=Document::default();
    let title=Paragraph::plain("Γενετική μηχανή: ο νέος ορίζοντας");
    let mut body=Paragraph::plain("Πάντα ευχήθηκαν οι άνθρωποι να θεραπεύονται");
    body.properties.space_after_pt=Some(7.0);
    if let Inline::Text(run)=&mut body.content[0] {run.properties.font_size_pt=Some(12.0);}
    let (first,second)=(title.id.clone(),body.id.clone());
    document.sections[0].blocks=vec![Block::Paragraph(title),Block::Paragraph(body)].into();
    let mut editor=EditorState::new(document);
    if reverse {editor.set_caret(second.clone(),selected_second_bytes);editor.extend_selection(first.clone(),0);}
    else {editor.set_caret(first.clone(),0);editor.extend_selection(second.clone(),selected_second_bytes);}
    (editor,first,second)
}

#[test]
fn applying_title_excludes_the_unselected_next_paragraph_in_both_selection_directions() {
    for reverse in [false,true] {
        let (mut editor,first,second)=paragraph_style_boundary_editor(reverse,0);
        let before=editor.document.clone();
        let untouched=editor.paragraph(&second).unwrap().clone();
        let selected=editor.selected_text().unwrap();
        assert!(selected.ends_with('\n'),"the selected paragraph separator must still copy");
        editor.apply_paragraph_style(StyleId::from(STYLE_TITLE)).unwrap();
        assert_eq!(editor.paragraph(&first).unwrap().style,Some(StyleId::from(STYLE_TITLE)));
        assert_eq!(editor.paragraph(&second).unwrap(),&untouched,"the exclusive end paragraph is unselected");
        assert_eq!(editor.formatting_summary().style,Some(StyleId::from(STYLE_TITLE)));
        assert_eq!(editor.document.styles,before.styles,"applying a style must not modify shared definitions");
        assert_eq!(editor.selected_text().unwrap(),selected);
        assert_eq!(editor.copy_fragment().unwrap().plain_text,selected);
        let after=editor.document.clone();
        assert!(editor.undo());assert_eq!(editor.document,before);
        assert!(editor.redo());assert_eq!(editor.document,after);
    }
}

#[test]
fn paragraph_formatting_stops_at_an_exclusive_paragraph_boundary() {
    for reverse in [false,true] {
        let (mut editor,first,second)=paragraph_style_boundary_editor(reverse,0);
        let untouched=editor.paragraph(&second).unwrap().clone();
        editor.apply_paragraph_properties(&ParagraphProperties{alignment:Some(ParagraphAlignment::Center),..Default::default()}).unwrap();
        assert_eq!(editor.paragraph(&first).unwrap().properties.alignment,Some(ParagraphAlignment::Center));
        assert_eq!(editor.paragraph(&second).unwrap(),&untouched);
    }
}

#[test]
fn a_selected_character_in_the_next_paragraph_includes_that_paragraph_in_style_application() {
    for reverse in [false,true] {
        let (mut editor,first,second)=paragraph_style_boundary_editor(reverse,"Π".len());
        editor.apply_paragraph_style(StyleId::from(STYLE_TITLE)).unwrap();
        for id in [first,second] {assert_eq!(editor.paragraph(&id).unwrap().style,Some(StyleId::from(STYLE_TITLE)));}
    }
}

#[test]
fn a_collapsed_caret_at_paragraph_start_applies_style_to_that_paragraph_only() {
    let (mut editor,first,second)=paragraph_style_boundary_editor(false,0);
    editor.set_caret(second.clone(),0);
    let untouched=editor.paragraph(&first).unwrap().clone();
    editor.apply_paragraph_style(StyleId::from(STYLE_TITLE)).unwrap();
    assert_eq!(editor.paragraph(&first).unwrap(),&untouched);
    assert_eq!(editor.paragraph(&second).unwrap().style,Some(StyleId::from(STYLE_TITLE)));
}

#[test]
fn selected_empty_paragraphs_are_styled_but_the_exclusive_end_paragraph_is_not() {
    let (mut editor,first,second)=paragraph_style_boundary_editor(false,0);
    let empty=Paragraph::plain("");let empty_id=empty.id.clone();
    editor.document.sections[0].blocks.insert(1,Block::Paragraph(empty));
    let untouched=editor.paragraph(&second).unwrap().clone();
    let before_text=editor.selected_text().unwrap();
    editor.apply_paragraph_style(StyleId::from(STYLE_TITLE)).unwrap();
    for id in [first,empty_id] {assert_eq!(editor.paragraph(&id).unwrap().style,Some(StyleId::from(STYLE_TITLE)));}
    assert_eq!(editor.paragraph(&second).unwrap(),&untouched);
    assert_eq!(editor.selected_text().unwrap(),before_text);
}

#[test]
fn legacy_multiline_text_has_independent_style_targets_when_opened() {
    let mut document=Document::default();
    let paragraph=Paragraph::plain("Γενετική μηχανή\r\n\nΠάντα ευχήθηκαν\nΟι πρώτοι προσπάθειες");
    let first=paragraph.id.clone();
    document.sections[0].blocks=vec![Block::Paragraph(paragraph)].into();
    for replace in [false,true] {
        let mut e=EditorState::new(if replace {Document::default()} else {document.clone()});
        if replace {e.replace_document(document.clone());}
        let paragraphs=model::document_paragraphs(&e.document);
        assert_eq!(paragraphs.len(),4,"literal newlines must become independent paragraphs");
        assert_eq!(paragraphs[0].id,first);
        let untouched=paragraphs[1..].iter().map(|p|(*p).clone()).collect::<Vec<_>>();
        e.set_caret(first.clone(),0);e.extend_selection(first.clone(),"Γενετική μηχανή".len());
        let before=e.document.clone();let selection=e.selection.clone();
        e.apply_named_style(&StyleId::from(STYLE_HEADING_1),true).unwrap();
        assert_eq!(e.paragraph(&first).unwrap().style,Some(StyleId::from(STYLE_HEADING_1)));
        assert_eq!(model::document_paragraphs(&e.document)[1..].iter().map(|p|(*p).clone()).collect::<Vec<_>>(),untouched);
        assert_eq!(e.document.styles,before.styles);
        assert_eq!(e.selection,selection);
        assert!(e.undo());assert_eq!(e.document,before);assert_eq!(e.selection,selection);
        assert!(e.redo());
    }
}

#[test]
fn multiline_insertion_is_one_undoable_edit_with_independent_paragraph_styles() {
    let mut e=editor("prefix suffix");let first=e.focus_position().unwrap().node_id;
    e.set_caret(first.clone(),"prefix ".len());
    let before=e.document.clone();let selection=e.selection.clone();
    e.insert_text("Γενετική\r\n\r\nΠάντα\rΟι πρώτοι\n").unwrap();
    let paragraphs=model::document_paragraphs(&e.document);
    assert_eq!(paragraphs.iter().map(|p|model::inline_text(&p.content)).collect::<Vec<_>>(),
        ["prefix Γενετική","","Πάντα","Οι πρώτοι","suffix"]);
    assert_eq!(e.focus_position().unwrap().node_id,paragraphs[4].id);
    assert_eq!(e.focus_position().unwrap().byte_offset,0);
    let inserted=e.document.clone();
    assert!(e.undo());assert_eq!(e.document,before);assert_eq!(e.selection,selection);
    assert!(!e.can_undo());assert!(e.redo());assert_eq!(e.document,inserted);
    let untouched=model::document_paragraphs(&e.document)[1..].iter().map(|p|(*p).clone()).collect::<Vec<_>>();
    e.set_caret(first.clone(),0);e.extend_selection(first.clone(),"prefix Γενετική".len());
    e.apply_named_style(&StyleId::from(STYLE_HEADING_1),true).unwrap();
    assert_eq!(model::document_paragraphs(&e.document)[1..].iter().map(|p|(*p).clone()).collect::<Vec<_>>(),untouched);
}
