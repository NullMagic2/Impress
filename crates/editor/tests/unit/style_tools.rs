use super::*;
use model::STYLE_HEADING_1;

fn fixture() -> (EditorState, Vec<NodeId>) {
    let mut document = Document::default();
    let paragraphs = ["First heading", "αβ γδ", "Second heading"].map(Paragraph::plain);
    let ids = paragraphs.iter().map(|p| p.id.clone()).collect();
    document.sections[0].blocks = paragraphs
        .into_iter()
        .map(Block::Paragraph)
        .collect::<Vec<_>>()
        .into();
    (EditorState::new(document), ids)
}

#[test]
fn style_replace_is_disjoint_and_undoable() {
    let (mut editor, ids) = fixture();
    let heading = StyleId::from(STYLE_HEADING_1);
    let body = StyleId::from(STYLE_NORMAL);
    let title = StyleId::from(model::STYLE_TITLE);
    for id in [&ids[0], &ids[2]] {
        editor.set_caret(id.clone(), 0);
        editor.apply_named_style(&heading, false).unwrap();
    }
    let uses = editor.style_matches(&heading);
    assert_eq!(
        uses.iter().map(|m| &m.node).collect::<Vec<_>>(),
        [&ids[0], &ids[2]]
    );
    assert_eq!(
        editor.replace_style_uses(&heading, &title, None).unwrap(),
        2
    );
    assert_eq!(editor.style_matches(&body)[0].node, ids[1]);
    assert!(editor.undo());
    assert_eq!(editor.style_matches(&heading), uses);
    assert_eq!(
        editor
            .replace_style_uses(&heading, &title, Some(&uses[1]))
            .unwrap(),
        1
    );
    assert_eq!(editor.style_matches(&heading)[0].node, ids[0]);
}

#[test]
fn linked_style_applies_to_selected_characters_and_follows_paragraph_definition() {
    let (mut editor, ids) = fixture();
    let heading = StyleId::from(STYLE_HEADING_1);
    let base = editor.document.styles.get(&heading).unwrap().clone();
    let companion = Style {
        id: StyleId::new(),
        name: "Heading companion".into(),
        kind: StyleKind::Character,
        based_on: None,
        next_style: None,
        character: Default::default(),
        paragraph: Default::default(),
        formatting: Default::default(),
    };
    editor.put_style(companion.clone()).unwrap();
    editor.link_style(&heading, &companion.id).unwrap();
    editor.set_caret(ids[1].clone(), 0);
    editor.extend_selection(ids[1].clone(), "αβ".len());
    editor.apply_named_style(&heading, false).unwrap();
    assert_eq!(editor.style_matches(&companion.id)[0].range, 0.."αβ".len());
    assert!(editor.style_matches(&heading).is_empty());
    editor
        .put_style(Style {
            character: CharacterProperties {
                font_size_pt: Some(26.0),
                ..base.character
            },
            ..base
        })
        .unwrap();
    assert_eq!(
        editor
            .inspect_styles()
            .unwrap()
            .inherited_character
            .font_size_pt,
        Some(26.0)
    );
    editor.set_caret(ids[1].clone(), 0);
    editor.apply_named_style(&heading, false).unwrap();
    assert_eq!(editor.style_matches(&heading)[0].node, ids[1]);
}

#[test]
fn importing_a_style_set_keeps_destination_ids_and_remaps_dependencies() {
    let (mut editor, ids) = fixture();
    let heading = StyleId::from(STYLE_HEADING_1);
    let mut source = StyleSheet::default();
    let mut imported = source.styles.remove(&heading).unwrap();
    imported.id = StyleId::from("WordHeading");
    imported.character.font_size_pt = Some(24.0);
    source.styles.insert(imported.id.clone(), imported.clone());
    let name = StyleId::from("NestedHeading");
    source.styles.insert(
        name.clone(),
        Style {
            id: name.clone(),
            name: "Nested heading".into(),
            based_on: Some(imported.id.clone()),
            ..imported
        },
    );
    let id = NodeId::new();
    source.list_styles.insert(
        id.clone(),
        NamedListStyle {
            name: "Report numbering".into(),
            headings: BTreeMap::from([(0, name.clone())]),
            definition: ListDefinition {
                id: id.clone(),
                levels: vec![model::ListLevel {
                    level: 0,
                    format: model::ListFormat::Decimal,
                    text: "%1.".into(),
                    start_at: 1,
                    left_indent_mm: 7.0,
                    hanging_indent_mm: 4.0,
                }],
            },
        },
    );
    editor.set_caret(ids[0].clone(), 0);
    editor.apply_named_style(&heading, false).unwrap();
    editor.import_style_sheet(&source).unwrap();
    assert_eq!(
        editor.paragraph(&ids[0]).unwrap().style.as_ref(),
        Some(&heading)
    );
    assert_eq!(
        editor.document.styles.get(&name).unwrap().based_on.as_ref(),
        Some(&heading)
    );
    assert_eq!(editor.document.styles.linked_list(&name), Some((&id, 0)));
    let encoded = serde_json::to_vec(&editor.document).unwrap();
    let reopened: Document = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(reopened.styles, editor.document.styles);
    assert!(editor.undo());
    assert!(editor.document.styles.get(&name).is_none());
}
