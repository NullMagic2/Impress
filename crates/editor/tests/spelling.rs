use editor::EditorState;
use model::*;

fn run(text: &str, color: &str) -> TextRun {
    let mut run = TextRun::plain(text);
    run.properties = CharacterProperties {
        color: Some(color.into()), bold: Some(true), italic: Some(true),
        underline: Some(true), highlight: Some("#FFFF00".into()),
        font_family: Some("Linux Libertine G".into()), font_size_pt: Some(19.0),
        language: Some("el-GR".into()), ..Default::default()
    };
    run.style = Some(StyleId::from("Answer"));
    run
}
fn editor(content: Vec<Inline>) -> (EditorState, NodeId) {
    let mut document = Document::default();
    let mut paragraph = Paragraph::plain("");
    let id = paragraph.id.clone();
    paragraph.content = content.into();
    document.sections[0].blocks = vec![Block::Paragraph(paragraph)].into();
    let mut editor = EditorState::new(document);
    editor.set_caret(id.clone(), 0);
    (editor, id)
}
fn format_at(editor: &EditorState, id: &NodeId, offset: usize) -> TextRun {
    fn at(items: &[Inline], mut offset: usize) -> TextRun {
        for item in items {
            if offset < inline_flat_len(item) {
                return match item { Inline::Text(run) => run.clone(), Inline::Hyperlink(h) => at(&h.content, offset), _=> panic!("not text") };
            }
            offset -= inline_flat_len(item);
        }
        panic!("missing text")
    }
    let p = document_paragraphs(&editor.document).into_iter().find(|p|p.id==*id).unwrap();
    at(&p.content, offset)
}
fn assert_format(actual: &TextRun, expected: &TextRun) {
    assert_eq!(actual.properties, expected.properties);
    assert_eq!(actual.style, expected.style);
}
#[test]
fn correction_preserves_all_word_formatting_instead_of_preceding_typing_format() {
    let word = run("wrng", "#C02040");
    let (mut e,id) = editor(vec![Inline::Text(TextRun::plain("before ")), Inline::Text(word.clone()), Inline::Text(TextRun::plain(" after"))]);
    let original=e.document.clone();
    e.replace_spelling_word(&id,7..11,"wrng","wrong",e.revision).unwrap();
    assert_eq!(e.paragraph_text(&id).unwrap(),"before wrong after");
    for offset in 7..12 {assert_format(&format_at(&e,&id,offset),&word);}
    assert_eq!(format_at(&e,&id,6).properties, CharacterProperties::default());
    assert_eq!(format_at(&e,&id,12).properties, CharacterProperties::default());
    let corrected=e.document.clone();
    assert!(e.undo());assert_eq!(e.document,original);
    assert!(e.redo());assert_eq!(e.document,corrected);
}
#[test]
fn mixed_runs_survive_greek_accent_correction_and_insertions_at_word_edges() {
    let a=run("μη", "#AA0000");let b=run("χανη", "#008855");
    let(mut e,id)=editor(vec![Inline::Text(a.clone()),Inline::Text(b.clone())]);
    e.replace_spelling_word(&id,0.."μηχανη".len(),"μηχανη","μηχανή",e.revision).unwrap();
    assert_format(&format_at(&e,&id,0),&a);
    assert_format(&format_at(&e,&id,"μηχαν".len()),&b);
    let word=run("ello", "#4433AA");
    let(mut e,id)=editor(vec![Inline::Text(TextRun::plain("x ")),Inline::Text(word.clone())]);
    e.replace_spelling_word(&id,2..6,"ello","hello",e.revision).unwrap();
    assert_format(&format_at(&e,&id,2),&word);
    e.replace_spelling_word(&id,2..7,"hello","hellos",e.revision).unwrap();
    assert_format(&format_at(&e,&id,7),&word);
}
#[test]
fn decomposed_accents_remain_whole_and_correction_keeps_hyperlink() {
    let a=run("cafe\u{301}","#336699");
    let target="https://example.com";
    let(mut e,id)=editor(vec![Inline::Hyperlink(Hyperlink{id:NodeId::new(),target:target.into(),content:vec![Inline::Text(a.clone())].into()})]);
    e.replace_spelling_word(&id,0..a.text.len(),&a.text,"café",e.revision).unwrap();
    assert_eq!(e.paragraph_text(&id).unwrap(),"café");
    assert_format(&format_at(&e,&id,3),&a);
    let p=document_paragraphs(&e.document)[0];
    assert!(matches!(&p.content[0],Inline::Hyperlink(h) if h.target==target));
    let word=run("ello", "#336699");
    let(mut e,id)=editor(vec![Inline::Text(TextRun::plain("Before ")),Inline::Hyperlink(Hyperlink{id:NodeId::new(),target:target.into(),content:vec![Inline::Text(word)].into()})]);
    e.replace_spelling_word(&id,7..11,"ello","hello",e.revision).unwrap();
    let p=document_paragraphs(&e.document)[0];
    assert!(matches!(&p.content[1],Inline::Hyperlink(h) if inline_text(&h.content)=="hello"));
}
#[test]
fn repeated_corrections_preserve_each_occurrences_own_format() {
    let red=run("teh", "#CC0000");let blue=run("teh", "#0000CC");
    let(mut e,id)=editor(vec![Inline::Text(red.clone()),Inline::Text(TextRun::plain(" ")),Inline::Text(blue.clone())]);
    for range in [0..3,4..7] {e.replace_spelling_word(&id,range,"teh","the",e.revision).unwrap();}
    assert_format(&format_at(&e,&id,0),&red);assert_format(&format_at(&e,&id,4),&blue);
    let unchanged=e.document.clone();
    assert!(e.replace_spelling_word(&id,0..3,"teh","the",e.revision).is_err());
    assert!(e.replace_spelling_word(&id,0..3,"the","teh",e.revision-1).is_err());
    assert_eq!(e.document,unchanged);
}
#[test]
fn table_cell_correction_preserves_character_formatting_and_undo() {
    let(mut e,_)=editor(vec![Inline::Text(TextRun::plain(""))]);
    e.insert_table(1,2).unwrap();
    let id=e.selection.as_ref().unwrap().focus.node_id.clone();
    e.apply_character_properties(&run("", "#7733AA").properties).unwrap();
    e.insert_text("wrng").unwrap();
    let before=e.document.clone();
    let expected=format_at(&e,&id,0);
    e.replace_spelling_word(&id,0..4,"wrng","wrong",e.revision).unwrap();
    assert_format(&format_at(&e,&id,2),&expected);
    assert!(e.undo());assert_eq!(e.document,before);
}

#[test]
fn corrections_are_recorded_when_track_changes_is_enabled() {
    let word=run("wrng", "#AA2255");
    let(mut e,id)=editor(vec![Inline::Text(word.clone())]);
    e.set_track_changes(true,"Proofreader").unwrap();
    let before=e.document.clone();
    e.replace_spelling_word(&id,0..4,"wrng","wrong",e.revision).unwrap();
    assert!(!e.document.review.tracked_changes.is_empty());
    assert_format(&format_at(&e,&id,2),&word);
    assert!(e.undo());assert_eq!(e.document,before);
}
