use super::*;
fn editor()->EditorState{let mut d=Document::default();let p=Paragraph::plain("αβ hello");let id=p.id.clone();d.sections[0].blocks.push(Block::Paragraph(p));let mut e=EditorState::new(d);e.set_caret(id,5);e}
#[test]fn bookmark_and_comment_rebase_and_undo(){let mut e=editor();e.add_bookmark("Place").unwrap();e.add_comment("A","Check this").unwrap();let before=e.document.clone();e.set_caret(e.selection.as_ref().unwrap().focus.node_id.clone(),0);e.insert_text("XYZ ").unwrap();assert_eq!(e.document.review.bookmarks.values().next().unwrap().range.as_ref().unwrap().start.offset,9);assert!(e.undo());assert_eq!(e.document,before);}
#[test]fn tracked_edit_reject_restores_original(){let mut e=editor();e.set_text_tracking(true,"Author").unwrap();let text=e.paragraph_text(&e.selection.as_ref().unwrap().focus.node_id).unwrap();e.insert_text("NEW").unwrap();let id=e.document.review.revisions.keys().next().unwrap().clone();e.resolve_revision(&id,false).unwrap();assert!(e.document.review.revisions.is_empty());assert_eq!(e.paragraph_text(&e.selection.as_ref().unwrap().focus.node_id).unwrap(),text);assert!(e.undo());assert_eq!(e.document.review.revisions.len(),1);}

#[test]
fn rebuilding_contents_preserves_manual_updates() {
    let mut editor = editor();
    editor.apply_paragraph_style(model::StyleId::from(model::STYLE_HEADING_1)).unwrap();
    editor.update_contents(&Default::default()).unwrap();
    assert!(editor.document.review.automatic_contents);
    editor.set_automatic_contents(false).unwrap();
    editor.update_contents(&Default::default()).unwrap();
    assert!(!editor.document.review.automatic_contents);
    let options = model::ContentsOptions { title: "Overview".into(), max_heading_level: 2, ..Default::default() };
    editor.update_contents_options(options.clone()).unwrap();
    assert_eq!(editor.document.review.contents_options, options);
    assert!(!editor.document.review.automatic_contents);
}

#[test]
fn contents_stays_at_its_insertion_point_and_uses_direct_outline_levels() {
    let mut document = Document::default();
    let front = Paragraph::plain("Cover");
    let marker = Paragraph::plain("Preface");
    let heading = Paragraph::plain("Unstyled subsection");
    let (front_id, marker_id, heading_id) = (front.id.clone(), marker.id.clone(), heading.id.clone());
    document.sections[0].blocks = vec![Block::Paragraph(front), Block::Paragraph(marker), Block::Paragraph(heading)].into();
    let mut editor = EditorState::new(document);
    let original_style = editor.paragraph(&heading_id).unwrap().style.clone();
    editor.set_caret(heading_id.clone(), 0);
    editor.set_contents_level(2).unwrap();
    editor.set_caret(marker_id.clone(), 0);
    let mut options = model::ContentsOptions::default();
    options.apply_template(model::ContentsTemplate::Compact);
    editor.update_contents_options(options).unwrap();

    let ids = editor.document.review.contents_paragraphs.clone();
    assert_eq!(editor.document.sections[0].blocks[0], Block::Paragraph(editor.paragraph(&front_id).unwrap().clone()));
    assert_eq!(editor.paragraph(&ids[1]).unwrap().properties.left_indent_mm, Some(3.0));
    assert_eq!(editor.paragraph(&heading_id).unwrap().style, original_style);
    let before = editor.document.clone();

    editor.set_caret(heading_id, 0);
    editor.insert_text("New ").unwrap();
    let body = model::document_paragraphs(&editor.document);
    assert_eq!(body[1].id, ids[0]);
    assert_eq!(body[2].id, ids[1]);
    assert_eq!(body[3].id, marker_id);
    assert!(model::inline_text(&body[2].content).starts_with("New Unstyled"));
    assert!(editor.undo());
    assert_eq!(editor.document, before);

    editor.remove_contents().unwrap();
    assert!(editor.document.review.contents_paragraphs.is_empty());
    assert_eq!(model::document_paragraphs(&editor.document).len(), 3);
    assert!(editor.undo());
    assert_eq!(editor.document, before);
}

#[test]
fn contents_can_be_inserted_before_any_headings_exist() {
    let mut editor = editor();
    let paragraph = editor.focus_position().unwrap().node_id;
    editor.set_caret(paragraph.clone(), 0);
    editor.update_contents(&Default::default()).unwrap();
    assert_eq!(editor.document.review.contents_paragraphs.len(), 1);
    editor.set_caret(paragraph, 0);
    editor.set_contents_level(1).unwrap();
    assert_eq!(editor.document.review.contents_paragraphs.len(), 2);
}
