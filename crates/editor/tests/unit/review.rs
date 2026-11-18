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
    let options = model::ContentsOptions { title: "Overview".into(), max_heading_level: 2 };
    editor.update_contents_options(options.clone()).unwrap();
    assert_eq!(editor.document.review.contents_options, options);
    assert!(!editor.document.review.automatic_contents);
}
