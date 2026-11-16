use super::*;
use model::{Style,ParagraphAlignment};

fn document()->(EditorState,NodeId,NodeId,NodeId) {
    let mut doc=Document::default();
    let mut first=Paragraph::plain("Γενετική μηχανή");first.style=Some(StyleId::from(STYLE_HEADING_1));
    if let Inline::Text(run)=&mut first.content[0] {run.properties.font_size_pt=Some(12.0);run.properties.italic=Some(true);run.properties.language=Some("el-GR".into());}
    let mut body=Paragraph::plain("Πάντα ευχήθηκαν");
    if let Inline::Text(run)=&mut body.content[0] {run.properties.font_size_pt=Some(12.0);}
    let mut other=Paragraph::plain("Another heading");other.style=Some(StyleId::from(STYLE_HEADING_1));
    if let Inline::Text(run)=&mut other.content[0] {run.properties.font_size_pt=Some(10.0);}
    let ids=(first.id.clone(),body.id.clone(),other.id.clone());
    doc.sections[0].blocks=vec![Block::Paragraph(first),Block::Paragraph(body),Block::Paragraph(other)].into();
    let mut e=EditorState::new(doc);e.set_caret(ids.0.clone(),0);e.extend_selection(ids.0.clone(),"Γενετική μηχανή".len());
    (e,ids.0,ids.1,ids.2)
}

#[test]
fn style_size_edit_refreshes_selected_pasted_heading_and_is_one_undo() {
    let (mut e,first,body,other)=document();let before=e.document.clone();let selection=e.selection.clone();
    let mut style=e.document.styles.get(&StyleId::from(STYLE_HEADING_1)).unwrap().clone();style.character.font_size_pt=Some(14.0);
    e.put_styles_with_selection(vec![style]).unwrap();
    assert_eq!(e.formatting_summary().character.font_size_pt,Some(14.0));
    let Inline::Text(run)=&e.paragraph(&first).unwrap().content[0] else{panic!()};
    assert_eq!(run.properties.font_size_pt,None);assert_eq!(run.properties.italic,Some(true));assert_eq!(run.properties.language.as_deref(),Some("el-GR"));
    for id in [body,other] {assert_eq!(e.paragraph(&id),find_paragraph_in_blocks(&before.sections[0].blocks,&id));}
    assert_eq!(e.selection,selection);let after=e.document.clone();
    assert!(e.undo());assert_eq!(e.document,before);assert_eq!(e.selection,selection);assert!(!e.can_undo());
    assert!(e.redo());assert_eq!(e.document,after);
}

#[test]
fn redefine_copies_selection_format_and_preserves_style_identity_and_membership() {
    let (mut e,first,_,_)=document();let target=StyleId::from(STYLE_TITLE);
    e.apply_character_properties(&CharacterProperties{font_size_pt:Some(18.0),color:Some("#993322".into()),bold:Some(false),..Default::default()}).unwrap();
    e.apply_paragraph_properties(&ParagraphProperties{alignment:Some(ParagraphAlignment::Right),left_indent_mm:Some(5.0),..Default::default()}).unwrap();
    let original=e.document.styles.get(&target).unwrap().clone();let paragraphs=e.document.sections.clone();let selection=e.selection.clone();
    let before=e.document.clone();e.update_style_from_selection(&target).unwrap();
    let updated=e.document.styles.get(&target).unwrap();
    assert_eq!(updated.id,original.id);assert_eq!(updated.name,original.name);assert_eq!(updated.based_on,original.based_on);assert_eq!(updated.next_style,original.next_style);assert_eq!(updated.formatting,original.formatting);
    assert_eq!(updated.character.font_size_pt,Some(18.0));assert_eq!(updated.character.bold,Some(false));assert_eq!(updated.character.color.as_deref(),Some("#993322"));
    assert_eq!(updated.paragraph.alignment,Some(ParagraphAlignment::Right));assert_eq!(updated.paragraph.left_indent_mm,Some(5.0));
    assert_eq!(e.document.sections,paragraphs);assert_eq!(e.selection,selection);
    assert_eq!(e.paragraph(&first).unwrap().style,Some(StyleId::from(STYLE_HEADING_1)));
    assert!(e.undo());assert_eq!(e.document,before);assert!(e.redo());
}

#[test]
fn mixed_selection_preserves_mixed_fields_and_excludes_next_paragraph_boundary() {
    let (mut e,first,body,other)=document();let target=StyleId::from(STYLE_TITLE);
    e.set_caret(first,0);e.extend_selection(body.clone(),"Πάντα ευχήθηκαν".len());
    e.apply_character_properties(&CharacterProperties{bold:Some(false),..Default::default()}).unwrap();
    let mut p=e.paragraph(&body).unwrap().clone();if let Inline::Text(r)=&mut p.content[0] {r.properties.font_size_pt=Some(22.0);}e.replace_paragraph(&p).unwrap();
    e.extend_selection(other,0);
    let old_size=e.document.styles.get(&target).unwrap().character.font_size_pt;
    e.update_style_from_selection(&target).unwrap();
    let style=e.document.styles.get(&target).unwrap();
    assert_eq!(style.character.font_size_pt,old_size);assert_eq!(style.character.bold,Some(false));
}

#[test]
fn character_style_redefinition_uses_typing_format_without_paragraph_settings() {
    let (mut e,_,body,_)=document();let id=StyleId::new();
    let style=Style{id:id.clone(),name:"Emphasis".into(),kind:StyleKind::Character,based_on:None,next_style:None,character:Default::default(),paragraph:Default::default(),formatting:Default::default()};
    e.put_style(style).unwrap();e.set_caret(body,0);e.typing_properties.font_size_pt=Some(16.0);e.typing_properties.underline=Some(true);
    e.update_style_from_selection(&id).unwrap();let updated=e.document.styles.get(&id).unwrap();
    assert_eq!(updated.character.font_size_pt,Some(16.0));assert_eq!(updated.character.underline,Some(true));assert_eq!(updated.paragraph,ParagraphProperties::default());
    assert_eq!(updated.character.language,None,"do not turn the inherited default into an explicit proofing override");
}

#[test]
fn editing_base_style_respects_a_derived_styles_explicit_size() {
    let (mut e,first,body,_)=document();let base=StyleId::from(STYLE_HEADING_1);let id=StyleId::new();
    let mut derived=e.document.styles.get(&base).unwrap().clone();derived.id=id.clone();derived.name="Special heading".into();derived.based_on=Some(base.clone());derived.character.font_size_pt=Some(18.0);e.put_style(derived).unwrap();
    let mut p=e.paragraph(&body).unwrap().clone();p.style=Some(id);e.replace_paragraph(&p).unwrap();
    e.set_caret(first,0);e.extend_selection(body.clone(),"Πάντα ευχήθηκαν".len());
    let untouched=e.paragraph(&body).unwrap().clone();
    let mut style=e.document.styles.get(&base).unwrap().clone();style.character.font_size_pt=Some(14.0);e.put_styles_with_selection(vec![style]).unwrap();
    assert_eq!(e.paragraph(&body).unwrap(),&untouched);
}

#[test]
fn redefining_without_a_selection_or_with_missing_style_is_atomic() {
    let (mut e,_,_,_)=document();e.selection=None;let before=e.document.clone();
    assert!(e.update_style_from_selection(&StyleId::from(STYLE_TITLE)).is_err());assert_eq!(e.document,before);
    let first=model::document_paragraphs(&e.document)[0].id.clone();e.set_caret(first,0);
    assert!(e.update_style_from_selection(&StyleId::new()).is_err());assert_eq!(e.document,before);
}
