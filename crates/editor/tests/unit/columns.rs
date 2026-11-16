use super::*;

fn editor(sections: usize) -> EditorState {
    let mut document = Document::default();
    let first = Paragraph::plain("First part of the text");
    let caret = first.id.clone();
    document.sections[0].blocks.push(Block::Paragraph(first));
    for _ in 1..sections {
        let mut section = Section::default();
        section.blocks.push(Block::Paragraph(Paragraph::plain("More text")));
        document.sections.push(section);
    }
    let mut editor = EditorState::new(document);
    editor.set_caret(caret, 0);
    editor
}

#[test]
fn whole_document_changes_every_section_in_one_undo_step() {
    let mut e = editor(3);
    let text_width = e.section_properties_at_caret().unwrap().text_width_mm();
    e.apply_column_layout(&ColumnLayout::preset(ColumnPreset::Right, text_width), ColumnScope::WholeDocument).unwrap();
    assert!(e.document.sections.iter().all(|s| s.properties.columns.len() == 2));
    assert!(e.undo());
    assert!(e.document.sections.iter().all(|s| s.properties.columns.is_empty()));
}

