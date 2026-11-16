use super::*;
use model::{Block,Document,Paragraph,CharacterProperties,Style,StyleId,StyleKind};
fn editor(text:&str)->EditorState {
    let mut document=Document::default();let p=Paragraph::plain(text);let id=p.id.clone();
    document.sections[0].blocks=vec![Block::Paragraph(p)].into();
    let mut editor=EditorState::new(document);editor.set_caret(id,text.len());editor
}
#[test]
fn greek_text_uses_greek_instead_of_the_inherited_english_default() {
    let e=editor("Γενετική μηχανή: ο νέος ορίζοντας στην αναγεννητική ιατρική");
    let input=snapshot(&e,"en-US",false).unwrap();
    assert!(!input.fragments.is_empty());assert!(input.fragments.iter().all(|f|f.language=="el-GR"));
    assert_eq!(selection_language(&e,"en-US").as_deref(),Some("el-GR"));
}
#[test]
fn mixed_greek_english_and_combining_accents_preserve_the_source_offsets() {
    let text="Hello, μηχανή — ἄνθρωπος μηχανη\u{301} world";
    let e=editor(text);let input=snapshot(&e,"en-US",false).unwrap();
    let restored=input.fragments.iter().map(|f|f.text.as_str()).collect::<String>();assert_eq!(restored,text);
    for f in &input.fragments {assert_eq!(&text[f.start..f.start+f.text.len()],f.text);}
    let greek=input.fragments.iter().find(|f|f.text.contains("μηχανή")).unwrap();assert_eq!(greek.language,"el-GR");
    let english=input.fragments.iter().find(|f|f.text.contains("world")).unwrap();assert_eq!(english.language,"en");
}
#[test]
fn direct_language_choice_is_respected_even_for_greek_characters() {
    let mut e=editor("μηχανή");e.select_all();
    e.apply_character_properties(&CharacterProperties{language:Some("en-GB".into()),..Default::default()}).unwrap();
    let input=snapshot(&e,"el-GR",false).unwrap();assert!(input.fragments.iter().all(|f|f.language=="en-GB"));
    assert_eq!(selection_language(&e,"el-GR").as_deref(),Some("en-GB"));
}
#[test]
fn explicit_language_in_a_named_style_is_respected() {
    let mut e=editor("μηχανή");let id=StyleId::from("test.english");
    e.put_style(Style{id:id.clone(),name:"English quote".into(),kind:StyleKind::Paragraph,based_on:Some(StyleId::from(model::STYLE_NORMAL)),next_style:None,
        character:CharacterProperties{language:Some("en-GB".into()),..Default::default()},paragraph:Default::default(),formatting:Default::default()}).unwrap();
    e.apply_paragraph_style(id).unwrap();
    assert!(snapshot(&e,"el-GR",false).unwrap().fragments.iter().all(|f|f.language=="en-GB"));
}
#[test]
fn selection_clipping_and_caret_language_follow_the_greek_word() {
    let text="Hello μηχανή world";let mut e=editor(text);let id=e.focus_position().unwrap().node_id;
    let start=text.find("μηχανή").unwrap();let end=start+"μηχανή".len();
    e.set_caret(id.clone(),start);e.extend_selection(id.clone(),end);
    let input=snapshot(&e,"en-US",true).unwrap();assert_eq!(input.fragments.len(),1);
    assert_eq!(input.fragments[0].start,start);assert_eq!(input.fragments[0].text,"μηχανή");
    assert_eq!(selection_language(&e,"en-US").as_deref(),Some("el-GR"));
    e.set_caret(id,start+"μη".len());assert_eq!(selection_language(&e,"en-US").as_deref(),Some("el-GR"));
}
#[test]
fn missing_greek_dictionary_never_selects_an_english_dictionary() {
    assert_eq!(installed_language("el-GR",&["en-US".into()]),"el-GR");
    assert_eq!(installed_language("el-GR",&["el".into(),"en-US".into()]),"el");
}
#[cfg(target_os="linux")]
#[test]
fn installed_greek_dictionary_accepts_the_reported_word_and_offers_greek_corrections() {
    let installed=spelling::system_languages().unwrap_or_default();
    if spelling::dictionary_language("el-GR",&installed).is_none(){eprintln!("Greek system dictionary unavailable; provider test skipped");return;}
    let e=editor("μηχανή μηχανννή");let input=snapshot(&e,"en-US",false).unwrap();
    let report=check(input,&Dictionary::default()).unwrap();assert!(report.warnings.is_empty(),"{:?}",report.warnings);
    assert!(!report.issues.iter().any(|i|i.word=="μηχανή"));
    let issue=report.issues.iter().find(|i|i.word=="μηχανννή").expect("Greek typo must be checked");
    assert!(greek_language(&issue.language));
    assert!(issue.suggestions.iter().any(|s|s.chars().any(|c|c.script()==Script::Greek)),"{:?}",issue.suggestions);
    assert_eq!(&"μηχανή μηχανννή"[issue.start..issue.end],issue.word);
}

#[test]
fn choosing_the_language_for_future_typing_is_reflected_at_the_caret() {
    let mut e=editor("Hello");
    e.apply_character_properties(&CharacterProperties{language:Some("el-GR".into()),..Default::default()}).unwrap();
    assert_eq!(selection_language(&e,"en-US").as_deref(),Some("el-GR"));
}
