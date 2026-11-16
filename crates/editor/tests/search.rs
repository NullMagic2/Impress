//! Search and replace.
use model::*;
use editor::EditorState;
fn editor(text:&str)->(EditorState,NodeId){let mut d=Document::default();let p=Paragraph::plain(text);let id=p.id.clone();d.sections[0].blocks=(vec![Block::Paragraph(p)]).into();let mut e=EditorState::new(d);e.set_caret(id.clone(),0);(e,id)}
#[test]fn replacement_is_one_undo_and_preserves_unaffected_formatting(){let(mut e,id)=editor("before αβαβ after");e.set_caret(id.clone(),0);e.extend_selection(id.clone(),7);e.apply_character_properties(&CharacterProperties{bold:Some(true),..Default::default()}).unwrap();let before=e.document.clone();let report=e.search("αβ",Default::default()).unwrap();assert_eq!(e.replace_all_search(&report,"γ").unwrap(),2);assert_eq!(e.paragraph_text(&id).unwrap(),"before γγ after");let p=document_paragraphs(&e.document)[0];assert!(matches!(&p.content[0],Inline::Text(t)if t.properties.bold==Some(true)));assert!(e.undo());assert_eq!(e.document,before);assert!(e.redo());assert_eq!(e.paragraph_text(&id).unwrap(),"before γγ after");}

