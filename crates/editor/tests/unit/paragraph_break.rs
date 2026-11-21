use super::*;

fn sample(size: Option<f32>) -> (EditorState, NodeId) {
    let mut doc = Document::default();
    let mut p = Paragraph::plain("Body αβ");
    if let Inline::Text(run) = &mut p.content[0] {
        run.properties = CharacterProperties { font_size_pt: size, font_family: Some("Neo Aegean".into()),
            italic: Some(true), language: Some("el-GR".into()), ..Default::default() };
    }
    let id = p.id.clone();doc.sections[0].blocks.push(Block::Paragraph(p));
    let mut e = EditorState::new(doc);e.set_caret(id.clone(), "Body αβ".len());(e, id)
}
fn assert_size(e: &EditorState, size: f32) { assert_eq!(e.formatting_summary().character.font_size_pt, Some(size)); }

#[test]
fn enter_keeps_direct_format_on_blank_lines_after_movement_undo_and_reload() {
    let (mut e, first) = sample(Some(18.0));let original = e.document.clone();
    e.split_paragraph().unwrap();let second = e.focus_position().unwrap().node_id;
    let split = e.document.clone();assert_size(&e,18.0);
    e.set_caret(first.clone(), 0);e.set_caret(second.clone(),0);assert_size(&e,18.0);
    assert_eq!(e.typing_properties.font_family.as_deref(),Some("Neo Aegean"));
    assert_eq!(e.typing_properties.language.as_deref(),Some("el-GR"));
    assert!(e.undo());assert_eq!(e.document,original);assert!(!e.can_undo());
    assert!(e.redo());assert_eq!(e.document,split);assert_size(&e,18.0);
    let mut reopened = EditorState::new(e.document.clone());reopened.set_caret(second.clone(),0);assert_size(&reopened,18.0);
    reopened.split_paragraph().unwrap();let third=reopened.focus_position().unwrap().node_id;
    reopened.set_caret(third,0);assert_size(&reopened,18.0);reopened.insert_text("Next").unwrap();assert_size(&reopened,18.0);
    assert_eq!(reopened.paragraph(&first),e.paragraph(&first));
}

#[test]
fn enter_retains_pending_size_and_character_style_without_flattening_paragraph_style() {
    let (mut e, _) = sample(None);
    let mut style = e.document.styles.get(&StyleId::from(STYLE_NORMAL)).unwrap().clone();
    style.character.font_size_pt=Some(16.0);e.put_style(style).unwrap();
    e.split_paragraph().unwrap();assert_size(&e,16.0);
    let id=e.focus_position().unwrap().node_id;e.set_caret(id,0);
    assert_eq!(e.typing_properties.font_size_pt,None,"named size stays linked to the style");
    let mut style=e.document.styles.get(&StyleId::from(STYLE_NORMAL)).unwrap().clone();style.character.font_size_pt=Some(20.0);e.put_style(style).unwrap();assert_size(&e,20.0);
    let character=StyleId::new();let style=model::Style{id:character.clone(),name:"Custom character".into(),kind:model::StyleKind::Character,
        based_on:None,next_style:None,character:CharacterProperties{bold:Some(true),..Default::default()},paragraph:Default::default(),formatting:Default::default()};
    e.put_style(style).unwrap();e.apply_named_style(&character,false).unwrap();
    e.apply_character_properties(&CharacterProperties{font_size_pt:Some(24.0),..Default::default()}).unwrap();
    e.split_paragraph().unwrap();let id=e.focus_position().unwrap().node_id;e.set_caret(id,0);
    assert_size(&e,24.0);assert_eq!(e.typing_style,Some(character));
}

#[test]
fn heading_split_keeps_style_in_the_middle_and_honors_configured_following_style_at_end() {
    let (mut e, first) = sample(Some(14.0));
    let mut p=e.paragraph(&first).unwrap().clone();p.style=Some(StyleId::from(STYLE_HEADING_1));p.properties.left_indent_mm=Some(8.0);e.replace_paragraph(&p).unwrap();
    e.set_caret(first.clone(),5);e.split_paragraph().unwrap();let tail=e.focus_position().unwrap().node_id;
    assert_eq!(e.paragraph(&tail).unwrap().style,p.style);assert_eq!(e.paragraph(&tail).unwrap().properties,p.properties);assert_size(&e,14.0);
    e.set_caret(tail,"αβ".len());e.split_paragraph().unwrap();let body=e.focus_position().unwrap().node_id;
    assert_eq!(e.paragraph(&body).unwrap().style,Some(StyleId::from(STYLE_NORMAL)));assert_size(&e,11.0);
    assert_eq!(e.typing_properties.font_size_pt,None);assert_eq!(e.typing_style,None);
}

#[test]
fn enter_replacing_selected_text_preserves_format_and_is_one_undo() {
    let (mut e,first)=sample(Some(17.0));e.set_caret(first.clone(),5);e.extend_selection(first.clone(),"Body αβ".len());
    let before=e.document.clone();e.split_paragraph().unwrap();let tail=e.focus_position().unwrap().node_id;
    e.set_caret(tail,0);assert_size(&e,17.0);assert_eq!(e.paragraph_text(&first).as_deref(),Some("Body "));
    assert!(e.undo());assert_eq!(e.document,before);assert!(!e.can_undo());
}

#[test]
fn enter_in_tables_and_notes_retains_the_same_blank_paragraph_format() {
    let (mut e,first)=sample(Some(19.0));let p=e.paragraph(&first).unwrap().clone();
    let table=model::Table{id:NodeId::new(),properties:Default::default(),rows:vec![model::TableRow{id:NodeId::new(),repeat_as_header:false,allow_break_across_pages:true,height_mm:None,
        cells:vec![model::TableCell{id:NodeId::new(),blocks:vec![Block::Paragraph(p)].into(),column_span:1,row_span:1,width_mm:None,shading:None,borders:None,vertical_alignment:Default::default(), margins: None, text_direction: None}]}]};
    e.document.sections[0].blocks=vec![Block::Table(table)].into();e.set_caret(first,"Body αβ".len());e.split_paragraph().unwrap();
    let id=e.focus_position().unwrap().node_id;e.set_caret(id,0);assert_size(&e,19.0);
    let (mut e,_) = sample(Some(19.0));e.insert_footnote("note").unwrap();let note=e.document.review.footnotes.keys().next().unwrap().clone();e.enter_footnote(&note).unwrap();
    e.move_end(false);e.apply_character_properties(&CharacterProperties{font_size_pt:Some(21.0),..Default::default()}).unwrap();e.split_paragraph().unwrap();
    let id=e.focus_position().unwrap().node_id;e.set_caret(id,0);assert_size(&e,21.0);assert!(e.undo());assert!(e.redo());assert_size(&e,21.0);
}
