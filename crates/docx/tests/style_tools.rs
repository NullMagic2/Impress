use docx::DocxFormat;
use format::DocumentFormat;
use model::*;
use std::{collections::BTreeMap, io::Cursor};

#[test]
fn linked_table_and_list_styles_round_trip_as_ooxml() {
    let mut document = Document::default();
    let heading = StyleId::from(STYLE_HEADING_1);
    let character = StyleId::from("HeadingChar");
    document.styles.styles.insert(
        character.clone(),
        Style {
            id: character.clone(),
            name: "Heading Characters".into(),
            kind: StyleKind::Character,
            based_on: None,
            next_style: None,
            character: Default::default(),
            paragraph: Default::default(),
            formatting: Default::default(),
        },
    );
    document
        .styles
        .linked_styles
        .insert(heading.clone(), character.clone());

    let list_id = NodeId::new();
    document.styles.list_styles.insert(
        list_id.clone(),
        NamedListStyle {
            name: "Treatise numbering".into(),
            headings: BTreeMap::from([(0, heading.clone())]),
            definition: ListDefinition {
                id: list_id.clone(),
                levels: vec![ListLevel {
                    level: 0,
                    format: ListFormat::Decimal,
                    text: "%1.".into(),
                    start_at: 2,
                    left_indent_mm: 12.7,
                    hanging_indent_mm: 6.35,
                }],
            },
        },
    );
    let table_style_id = StyleId::from("ResearchTables");
    document.styles.table_styles.insert(
        table_style_id.clone(),
        TableStyle {
            name: "Research tables".into(),
            body_fill: Some("#EEEEEE".into()),
            header_fill: Some("#CCDDFF".into()),
            band_fill: Some("#DDDDDD".into()),
            borders: None,
        },
    );
    let table = Table {
        id: NodeId::new(),
        properties: TableProperties {
            style: Some(table_style_id.clone()),
            ..Default::default()
        },
        rows: vec![TableRow {
            id: NodeId::new(),
            repeat_as_header: false,
            allow_break_across_pages: true,
            height_mm: None,
            cells: vec![TableCell {
                id: NodeId::new(),
                blocks: vec![Block::Paragraph(Paragraph::plain("cell"))].into(),
                column_span: 1,
                row_span: 1,
                width_mm: None,
                shading: None,
                borders: None,
                vertical_alignment: Default::default(),
                margins: None,
                text_direction: None,
            }],
        }],
    };
    let mut heading_paragraph = Paragraph::plain("Chapter");
    heading_paragraph.style = Some(heading.clone());
    document.sections[0].blocks =
        vec![Block::Paragraph(heading_paragraph), Block::Table(table)].into();
    let mut bytes = Cursor::new(Vec::new());
    DocxFormat.export(&document, &mut bytes).unwrap();
    bytes.set_position(0);
    let reopened = DocxFormat.import(&mut bytes).unwrap();
    assert_eq!(
        reopened.styles.linked_styles.get(&heading),
        Some(&character)
    );
    let Some((list, 0)) = reopened.styles.linked_list(&heading) else {
        panic!("Heading list not linked");
    };
    assert_eq!(list, &list_id);
    assert_eq!(
        reopened.styles.list_styles[list].definition.levels[0].start_at,
        2
    );
    let Block::Table(table) = &reopened.sections[0].blocks[1] else {
        panic!("missing table");
    };
    assert_eq!(table.properties.style.as_ref(), Some(&table_style_id));
    assert_eq!(
        reopened.styles.table_style_fill(table, 0),
        Some("#CCDDFF")
    );
    assert_eq!(
        reopened.styles.table_style_fill(table, 1),
        Some("#DDDDDD")
    );
}
