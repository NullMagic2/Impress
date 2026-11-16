//! Semantic native Word97 round trips; no Office executable is needed.
use format::DocumentFormat;
use format_doc::DocFormat;
use model::*;
use std::io::Cursor;

const GEOMETRY_TOLERANCE_MM: f32 = 0.04;
fn close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < GEOMETRY_TOLERANCE_MM,
        "{actual} differs from {expected}"
    );
}
fn cycle(document: &Document) -> Document {
    let mut bytes = Cursor::new(Vec::new());
    DocFormat
        .export(document, &mut bytes)
        .expect("native export");
    bytes.set_position(0);
    DocFormat.import(&mut bytes).expect("native import")
}
fn find_paragraph<'a>(document: &'a Document, text: &str) -> &'a Paragraph {
    document_paragraphs(document)
        .into_iter()
        .find(|p| inline_text(&p.content) == text)
        .unwrap_or_else(|| panic!("missing paragraph {text:?}"))
}
fn cell(text: &str) -> TableCell {
    TableCell {
        id: NodeId::new(),
        blocks: vec![Block::Paragraph(Paragraph::plain(text))].into(),
        column_span: 1,
        row_span: 1,
        width_mm: None,
        shading: None,
        borders: None,
        vertical_alignment: CellVerticalAlignment::Top,
    }
}
fn row(cells: Vec<TableCell>) -> TableRow {
    TableRow {
        id: NodeId::new(),
        cells,
        repeat_as_header: false,
        allow_break_across_pages: true,
        height_mm: None,
    }
}
fn table(rows: Vec<TableRow>) -> Table {
    Table {
        id: NodeId::new(),
        properties: TableProperties::default(),
        rows,
    }
}
fn find_table(blocks: &[Block]) -> &Table {
    blocks
        .iter()
        .find_map(|block| {
            if let Block::Table(table) = block {
                Some(table)
            } else {
                None
            }
        })
        .expect("table")
}

#[test]
fn nested_merged_tables_keep_grid_colors_and_floating_geometry() {
    let mut document = Document::default();
    let mut merged = cell("Merged 😎");
    merged.column_span = 2;
    merged.row_span = 2;
    merged.shading = Some("#8ACE72".into());
    merged.vertical_alignment = CellVerticalAlignment::Center;
    merged.borders = Some(BorderSet {
        top: Some(Border {
            width_pt: 1.5,
            color: "#E12060".into(),
            style: BorderStyle::Double,
        }),
        left: Some(Border {
            width_pt: 1.0,
            color: "#1676CB".into(),
            style: BorderStyle::Dashed,
        }),
        right: Some(Border {
            width_pt: 0.0,
            color: "#000000".into(),
            style: BorderStyle::None,
        }),
        ..Default::default()
    });
    let mut nested_cell = cell("Nested container");
    nested_cell.blocks.push(Block::Table(table(vec![row(vec![
        cell("Inner α"),
        cell("Inner β"),
    ])])));
    let mut outer = table(vec![
        row(vec![merged, nested_cell]),
        row(vec![cell("Right continuation")]),
        row(vec![cell("One"), cell("Two"), cell("Three")]),
    ]);
    outer.rows[0].repeat_as_header = true;
    outer.rows[0].allow_break_across_pages = false;
    outer.rows[0].height_mm = Some(9.0);
    outer.properties.borders = Some(BorderSet {
        inside_vertical: Some(Border {
            width_pt: 1.0,
            color: "#000000".into(),
            style: BorderStyle::Single,
        }),
        ..Default::default()
    });
    outer.properties.preferred_width_mm = Some(90.0);
    outer.properties.grid_column_widths_mm = vec![30.0; 3];
    outer.properties.alignment = Some(TableAlignment::Center);
    outer.properties.cell_spacing_mm = Some(1.5);
    outer.properties.position = Some(TablePosition {
        horizontal_from: HorizontalFrom::Page,
        horizontal: HorizontalPlacement::Offset(12.0),
        vertical_from: VerticalFrom::Page,
        vertical: VerticalPlacement::Offset(24.0),
        distance_left_mm: 2.0,
        distance_right_mm: 3.0,
        distance_top_mm: 4.0,
        distance_bottom_mm: 5.0,
        wrap: TableWrap::Around,
        allow_overlap: false,
    });
    document.sections[0].blocks = vec![
        Block::Table(outer),
        Block::Paragraph(Paragraph::plain("Anchor after table")),
    ]
    .into();
    let reopened = cycle(&document);
    let actual = find_table(&reopened.sections[0].blocks);
    assert_eq!(actual.rows.len(), 3);
    let merged = &actual.rows[0].cells[0];
    assert_eq!(merged.column_span, 2);
    assert_eq!(merged.row_span, 2);
    assert_eq!(merged.shading.as_deref(), Some("#8ACE72"));
    assert_eq!(merged.vertical_alignment, CellVerticalAlignment::Center);
    let borders = merged.borders.as_ref().expect("cell borders");
    let top = borders.top.as_ref().expect("top border");
    assert_eq!(top.color, "#E12060");
    assert_eq!(top.style, BorderStyle::Double);
    close(top.width_pt, 1.5);
    assert_eq!(borders.left.as_ref().unwrap().style, BorderStyle::Dashed);
    assert_eq!(
        borders.right.as_ref().expect("explicit border clear").style,
        BorderStyle::None
    );
    assert_eq!(actual.rows[1].cells.len(), 1);
    assert_eq!(actual.rows[2].cells.len(), 3);
    assert!(actual.rows[0].repeat_as_header);
    assert!(!actual.rows[0].allow_break_across_pages);
    close(actual.rows[0].height_mm.unwrap(), 9.0);
    let inner = find_table(&actual.rows[0].cells[1].blocks);
    assert_eq!(inner.rows[0].cells.len(), 2);
    assert_eq!(
        inline_text(
            &document_paragraphs(&reopened)
                .into_iter()
                .find(|p| inline_text(&p.content) == "Inner α")
                .unwrap()
                .content
        ),
        "Inner α"
    );
    close(actual.properties.preferred_width_mm.unwrap(), 90.0);
    close(actual.properties.cell_spacing_mm.unwrap(), 1.5);
    assert_eq!(actual.properties.alignment, Some(TableAlignment::Center));
    let position = actual.properties.position.as_ref().expect("floating table");
    assert_eq!(position.horizontal_from, HorizontalFrom::Page);
    assert_eq!(position.vertical_from, VerticalFrom::Page);
    let HorizontalPlacement::Offset(x) = position.horizontal else {
        panic!("absolute x")
    };
    let VerticalPlacement::Offset(y) = position.vertical else {
        panic!("absolute y")
    };
    close(x, 12.0);
    close(y, 24.0);
    close(position.distance_left_mm, 2.0);
    close(position.distance_right_mm, 3.0);
    close(position.distance_top_mm, 4.0);
    close(position.distance_bottom_mm, 5.0);
    assert!(!position.allow_overlap);
}

fn list(start: i32) -> ListDefinition {
    ListDefinition {
        id: NodeId::new(),
        levels: vec![
            ListLevel {
                level: 0,
                format: ListFormat::Decimal,
                text: "%1.".into(),
                start_at: start,
                left_indent_mm: 12.7,
                hanging_indent_mm: 6.35,
            },
            ListLevel {
                level: 1,
                format: ListFormat::LowerLetter,
                text: "%1.%2)".into(),
                start_at: 1,
                left_indent_mm: 25.4,
                hanging_indent_mm: 6.35,
            },
        ],
    }
}
#[test]
fn automatic_lists_keep_levels_templates_and_independent_restarts() {
    let mut document = Document::default();
    let first = list(7);
    let restart = list(3);
    for (text, id, level) in [
        ("First", &first.id, 0),
        ("Nested", &first.id, 1),
        ("Following", &first.id, 0),
        ("Restart", &restart.id, 0),
    ] {
        let mut p = Paragraph::plain(text);
        p.properties.list_id = Some(id.clone());
        p.properties.list_level = Some(level);
        document.sections[0].blocks.push(Block::Paragraph(p));
    }
    document.lists.insert(first.id.clone(), first);
    document.lists.insert(restart.id.clone(), restart);
    let actual = cycle(&document);
    let first = find_paragraph(&actual, "First");
    let nested = find_paragraph(&actual, "Nested");
    let following = find_paragraph(&actual, "Following");
    let restarted = find_paragraph(&actual, "Restart");
    assert_eq!(first.properties.list_id, following.properties.list_id);
    assert_eq!(nested.properties.list_id, first.properties.list_id);
    assert_ne!(
        first.properties.list_id, restarted.properties.list_id,
        "definitions: {:?}",
        actual.lists
    );
    assert_eq!(nested.properties.list_level, Some(1));
    let definition = &actual.lists[first.properties.list_id.as_ref().unwrap()];
    assert_eq!(definition.levels[0].start_at, 7);
    assert_eq!(definition.levels[1].format, ListFormat::LowerLetter);
    assert_eq!(definition.levels[1].text, "%1.%2)");
    close(definition.levels[1].left_indent_mm, 25.4);
    let restarted = &actual.lists[restarted.properties.list_id.as_ref().unwrap()];
    assert_eq!(restarted.levels[0].start_at, 3);
}

fn field(kind: FieldKind) -> Inline {
    Inline::Field(Field {
        id: NodeId::new(),
        kind,
        display_text: Some("1".into()),
        color: None,
    })
}
#[test]
fn notes_comments_bookmarks_and_page_fields_keep_unicode_anchors() {
    let mut document = Document::default();
    let prefix = "A😎 Ελληνικά";
    let mut body = Paragraph::plain(prefix);
    let body_id = body.id.clone();
    let footnote = NodeId::new();
    let endnote = NodeId::new();
    body.content.push(field(FieldKind::FootnoteReference {
        note: footnote.clone(),
    }));
    body.content.push(field(FieldKind::FootnoteReference {
        note: endnote.clone(),
    }));
    body.content.push(field(FieldKind::PageReference {
        bookmark: "GreekRange".into(),
    }));
    body.content.push(field(FieldKind::PageNumber));
    document.sections[0].blocks.push(Block::Paragraph(body));
    for (id, kind, text) in [
        (footnote, NoteKind::Footnote, "Footnote α"),
        (endnote, NoteKind::Endnote, "Endnote β"),
    ] {
        let mut note = Paragraph::plain(text);
        if let Inline::Text(run) = &mut note.content[0] {
            run.properties.bold = Some(true);
        }
        document.review.footnotes.insert(
            id.clone(),
            Footnote {
                id,
                kind,
                blocks: vec![Block::Paragraph(note)].into(),
            },
        );
    }
    let range = AnchorRange {
        start: TextAnchor {
            paragraph: body_id.clone(),
            offset: "A😎 ".len(),
        },
        end: TextAnchor {
            paragraph: body_id,
            offset: prefix.len(),
        },
    };
    let bookmark = NodeId::new();
    document.review.bookmarks.insert(
        bookmark.clone(),
        Bookmark {
            id: bookmark,
            name: "GreekRange".into(),
            range: Some(range.clone()),
        },
    );
    let comment = NodeId::new();
    document.review.comments.insert(
        comment.clone(),
        ReviewComment {
            id: comment,
            author: "Reviewer Δ".into(),
            text: "Check Ελληνικά".into(),
            range: Some(range),
            resolved: false,
            date: None,
            replies: Vec::new(),
        },
    );
    let actual = cycle(&document);
    assert_eq!(actual.review.footnotes.len(), 2);
    assert_eq!(footnote_references(&actual).len(), 2);
    for (kind, text) in [
        (NoteKind::Footnote, "Footnote α"),
        (NoteKind::Endnote, "Endnote β"),
    ] {
        let note = actual
            .review
            .footnotes
            .values()
            .find(|note| note.kind == kind)
            .unwrap();
        let Block::Paragraph(p) = &note.blocks[0] else {
            panic!("rich note")
        };
        assert_eq!(inline_text(&p.content), text);
        let Inline::Text(run) = &p.content[0] else {
            panic!("note text")
        };
        assert_eq!(run.properties.bold, Some(true));
    }
    let bookmark = actual
        .review
        .bookmarks
        .values()
        .find(|bookmark| bookmark.name == "GreekRange")
        .unwrap();
    let range = bookmark.range.as_ref().unwrap();
    assert!(valid_range(&actual, range));
    assert_eq!(range.start.offset, "A😎 ".len());
    assert_eq!(range.end.offset, prefix.len());
    let comment = actual.review.comments.values().next().unwrap();
    assert_eq!(comment.author, "Reviewer Δ");
    assert_eq!(comment.text, "Check Ελληνικά");
    assert_eq!(comment.range.as_ref(), Some(range));
    let body = document_paragraphs(&actual)
        .into_iter()
        .find(|p| inline_text(&p.content).starts_with(prefix))
        .unwrap();
    assert!(body.content.iter().any(|i|matches!(i,Inline::Field(Field{kind:FieldKind::PageReference{bookmark},..})if bookmark=="GreekRange")));
    assert!(body.content.iter().any(|i| matches!(
        i,
        Inline::Field(Field {
            kind: FieldKind::PageNumber,
            ..
        })
    )));
}

fn resource(document: &mut Document) -> ResourceId {
    let mut bytes = Cursor::new(Vec::new());
    image::DynamicImage::new_rgba8(2, 2)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    let id = ResourceId::new();
    document.resources.insert(
        id.clone(),
        Resource {
            id: id.clone(),
            media_type: "image/png".into(),
            original_name: Some("tiny.png".into()),
            data: bytes.into_inner().into(),
            font_faces: Vec::new(),
        },
    );
    id
}
fn floating_picture(resource: &ResourceId, paragraph: &NodeId) -> BlockImage {
    BlockImage {
        id: NodeId::new(),
        resource: resource.clone(),
        width_mm: 40.0,
        height_mm: 20.0,
        alt_text: Some("Greek α picture".into()),
        effects: ImageEffects {
            crop: Some(ImageCrop {
                left: 0.1,
                top: 0.2,
                right: 0.0,
                bottom: 0.0,
            }),
            ..Default::default()
        },
        wrap: ImageWrap::Square,
        alignment: ImageAlignment::Left,
        anchor: Some(ImageAnchor {
            paragraph: paragraph.clone(),
            reference: AnchorReference::Page,
            x_mm: 20.0,
            y_mm: 30.0,
            gap_mm: 4.0,
            locked: true,
        }),
    }
}
fn assert_picture(document: &Document, blocks: &[Block]) {
    let image = blocks
        .iter()
        .find_map(|block| {
            if let Block::Image(image) = block {
                Some(image)
            } else {
                None
            }
        })
        .unwrap_or_else(|| panic!("missing floating picture in {blocks:?}"));
    let resource = document
        .resources
        .get(&image.resource)
        .expect("embedded raster resource");
    let decoded = image::load_from_memory(&resource.data).expect("valid embedded raster");
    assert_eq!((decoded.width(), decoded.height()), (2, 2));
    close(image.width_mm, 40.0);
    close(image.height_mm, 20.0);
    assert_eq!(image.alt_text.as_deref(), Some("Greek α picture"));
    assert_eq!(image.wrap, ImageWrap::Square);
    let anchor = image.anchor.as_ref().unwrap();
    assert_eq!(anchor.reference, AnchorReference::Page);
    close(anchor.x_mm, 20.0);
    close(anchor.y_mm, 30.0);
    close(anchor.gap_mm, 4.0);
    assert!(anchor.locked);
    let crop = image.effects.crop.as_ref().unwrap();
    assert!((crop.left - 0.1).abs() < 0.001);
    assert!((crop.top - 0.2).abs() < 0.001);
    assert!(blocks
        .iter()
        .any(|block| matches!(block,Block::Paragraph(p) if p.id==anchor.paragraph)));
}
#[test]
fn body_and_header_floating_rasters_and_unequal_columns_keep_geometry() {
    let mut document = Document::default();
    let resource = resource(&mut document);
    let body = Paragraph::plain("Body anchor");
    let header = Paragraph::plain("Header anchor");
    let body_image = floating_picture(&resource, &body.id);
    let header_image = floating_picture(&resource, &header.id);
    document.sections[0].blocks = vec![Block::Paragraph(body), Block::Image(body_image)].into();
    document.sections[0].headers.default = Some(HeaderFooter {
        id: NodeId::new(),
        blocks: vec![Block::Paragraph(header), Block::Image(header_image)].into(),
    });
    document.sections[0].properties.column_count = 2;
    document.sections[0].properties.columns = vec![
        ColumnSpec {
            width_mm: 50.0,
            spacing_mm: 10.0,
        },
        ColumnSpec {
            width_mm: 99.2,
            spacing_mm: 0.0,
        },
    ];
    document.sections[0].properties.column_separator = true;
    let actual = cycle(&document);
    assert_picture(&actual, &actual.sections[0].blocks);
    let header = actual.sections[0].headers.default.as_ref().unwrap();
    assert_picture(&actual, &header.blocks);
    let columns = &actual.sections[0].properties.columns;
    assert_eq!(columns.len(), 2);
    close(columns[0].width_mm, 50.0);
    close(columns[0].spacing_mm, 10.0);
    close(columns[1].width_mm, 99.2);
    assert!(actual.sections[0].properties.column_separator);
}

#[test]
fn inline_raster_keeps_display_size_alt_text_and_crop() {
    let mut document = Document::default();
    let resource = resource(&mut document);
    let mut paragraph = Paragraph::plain("Inline picture: ");
    paragraph.content.push(Inline::Image(InlineImage {
        id: NodeId::new(),
        resource,
        width_mm: 38.0,
        height_mm: 17.0,
        alt_text: Some("Inline α".into()),
        effects: ImageEffects {
            crop: Some(ImageCrop {
                left: 0.125,
                top: 0.25,
                right: 0.0,
                bottom: 0.0,
            }),
            ..Default::default()
        },
    }));
    document.sections[0]
        .blocks
        .push(Block::Paragraph(paragraph));
    let actual = cycle(&document);
    let image = document_paragraphs(&actual)
        .into_iter()
        .flat_map(|paragraph| paragraph.content.iter())
        .find_map(|inline| {
            if let Inline::Image(image) = inline {
                Some(image)
            } else {
                None
            }
        })
        .expect("inline image");
    close(image.width_mm, 38.0);
    close(image.height_mm, 17.0);
    assert_eq!(image.alt_text.as_deref(), Some("Inline α"));
    let crop = image.effects.crop.as_ref().expect("inline crop");
    assert!((crop.left - 0.125).abs() < 0.001);
    assert!((crop.top - 0.25).abs() < 0.001);
    let bytes = &actual.resources[&image.resource].data;
    let decoded = image::load_from_memory(bytes).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (2, 2));
}

#[test]
fn wide_rows_keep_all_cells_and_color_without_losing_data_stream_pictures() {
    const COLUMNS: usize = 63;
    let mut document = Document::default();
    let resource = resource(&mut document);
    let mut rows = Vec::new();
    for row_index in 0..2 {
        let mut cells = Vec::new();
        for column in 0..COLUMNS {
            let mut cell = cell(&format!("Cell {row_index}:{column} α😎"));
            cell.shading = Some("#92B6DC".into());
            let border = Border {
                width_pt: 1.0,
                color: "#A52271".into(),
                style: BorderStyle::Dotted,
            };
            cell.borders = Some(BorderSet {
                top: Some(border.clone()),
                left: Some(border.clone()),
                bottom: Some(border.clone()),
                right: Some(border),
                ..Default::default()
            });
            cells.push(cell);
        }
        rows.push(row(cells));
    }
    let mut table = table(rows);
    table.properties.grid_column_widths_mm = vec![1.5; COLUMNS];
    table.properties.preferred_width_mm = Some(94.5);
    let mut picture = Paragraph::plain("Picture after wide table");
    picture.content.push(Inline::Image(InlineImage {
        id: NodeId::new(),
        resource,
        width_mm: 30.0,
        height_mm: 15.0,
        alt_text: Some("After table".into()),
        effects: Default::default(),
    }));
    document.sections[0].blocks = vec![Block::Table(table), Block::Paragraph(picture)].into();
    let actual = cycle(&document);
    let table = find_table(&actual.sections[0].blocks);
    assert_eq!(table.rows.len(), 2);
    for (row_index, row) in table.rows.iter().enumerate() {
        assert_eq!(row.cells.len(), COLUMNS);
        for (column, cell) in row.cells.iter().enumerate() {
            let Block::Paragraph(paragraph) = &cell.blocks[0] else {
                panic!("cell paragraph")
            };
            assert_eq!(
                inline_text(&paragraph.content),
                format!("Cell {row_index}:{column} α😎")
            );
            assert_eq!(cell.shading.as_deref(), Some("#92B6DC"));
            assert_eq!(
                cell.borders.as_ref().unwrap().right.as_ref().unwrap().color,
                "#A52271"
            );
        }
    }
    let picture = document_paragraphs(&actual)
        .into_iter()
        .flat_map(|paragraph| paragraph.content.iter())
        .find_map(|inline| {
            if let Inline::Image(image) = inline {
                Some(image)
            } else {
                None
            }
        })
        .expect("picture after table");
    close(picture.width_mm, 30.0);
    assert!(image::load_from_memory(&actual.resources[&picture.resource].data).is_ok());
}

#[test]
fn style_inheritance_next_style_and_direct_false_remain_editable() {
    use std::collections::BTreeMap;
    let mut document = Document::default();
    let base = StyleId::from("test.paragraph.base");
    let leaf = StyleId::from("test.paragraph.leaf");
    let character_base = StyleId::from("test.character.base");
    let character_leaf = StyleId::from("test.character.leaf");
    for style in [
        Style {
            id: base.clone(),
            name: "Body Base".into(),
            kind: StyleKind::Paragraph,
            based_on: Some(StyleId::from(STYLE_NORMAL)),
            next_style: None,
            character: CharacterProperties {
                font_family: Some("Liberation Serif".into()),
                font_size_pt: Some(14.0),
                bold: Some(true),
                ..Default::default()
            },
            paragraph: ParagraphProperties {
                left_indent_mm: Some(11.0),
                keep_with_next: Some(true),
                ..Default::default()
            },
            formatting: BTreeMap::new(),
        },
        Style {
            id: leaf.clone(),
            name: "Body Leaf".into(),
            kind: StyleKind::Paragraph,
            based_on: Some(base.clone()),
            next_style: Some(StyleId::from(STYLE_NORMAL)),
            character: CharacterProperties {
                italic: Some(true),
                ..Default::default()
            },
            paragraph: ParagraphProperties {
                space_after_pt: Some(9.0),
                ..Default::default()
            },
            formatting: BTreeMap::new(),
        },
        Style {
            id: character_base.clone(),
            name: "Character Base".into(),
            kind: StyleKind::Character,
            based_on: None,
            next_style: None,
            character: CharacterProperties {
                bold: Some(true),
                color: Some("#234567".into()),
                ..Default::default()
            },
            paragraph: Default::default(),
            formatting: BTreeMap::new(),
        },
        Style {
            id: character_leaf.clone(),
            name: "Character Leaf".into(),
            kind: StyleKind::Character,
            based_on: Some(character_base),
            next_style: None,
            character: CharacterProperties {
                underline: Some(true),
                ..Default::default()
            },
            paragraph: Default::default(),
            formatting: BTreeMap::new(),
        },
    ] {
        document.styles.styles.insert(style.id.clone(), style);
    }
    let mut paragraph = Paragraph::plain("Inherited ");
    paragraph.style = Some(leaf);
    paragraph.properties.borders = Some(BorderSet {
        top: Some(Border {
            width_pt: 0.75,
            color: "#285A9C".into(),
            style: BorderStyle::Dotted,
        }),
        ..Default::default()
    });
    if let Inline::Text(run) = &mut paragraph.content[0] {
        run.properties.highlight = Some("#A0C0E0".into());
    }

    let mut styled = TextRun::plain("Styled");
    styled.style = Some(character_leaf);
    styled.properties.bold = Some(false);
    styled.properties.italic = Some(false);
    paragraph.content.push(Inline::Text(styled));
    document.sections[0]
        .blocks
        .push(Block::Paragraph(paragraph));
    let mut actual = cycle(&document);
    let paragraph = find_paragraph(&actual, "Inherited Styled").clone();
    let border = paragraph
        .properties
        .borders
        .as_ref()
        .expect("paragraph borders")
        .top
        .as_ref()
        .expect("top border");
    assert_eq!(border.color, "#285A9C");
    assert_eq!(border.style, BorderStyle::Dotted);
    close(border.width_pt, 0.75);
    let leaf_id = paragraph.style.as_ref().unwrap().clone();
    let base_id = actual.styles.id_by_name("Body Base").unwrap();
    let char_base_id = actual.styles.id_by_name("Character Base").unwrap();
    let char_leaf_id = actual.styles.id_by_name("Character Leaf").unwrap();
    let leaf = actual.styles.get(&leaf_id).unwrap();
    assert_eq!(leaf.based_on.as_ref(), Some(&base_id));
    assert_eq!(leaf.next_style, actual.styles.id_by_name("Normal"));
    assert_eq!(leaf.character.bold, None);
    assert_eq!(leaf.character.font_size_pt, None);
    assert_eq!(leaf.paragraph.left_indent_mm, None);
    let character_leaf = actual.styles.get(&char_leaf_id).unwrap();
    assert_eq!(character_leaf.kind, StyleKind::Character);
    assert_eq!(character_leaf.based_on.as_ref(), Some(&char_base_id));
    let Inline::Text(inherited) = &paragraph.content[0] else {
        panic!("inherited run")
    };
    let Inline::Text(styled) = &paragraph.content[1] else {
        panic!("styled run")
    };
    assert_eq!(inherited.properties.font_family, None);
    assert_eq!(inherited.properties.highlight.as_deref(), Some("#A0C0E0"));
    assert_eq!(inherited.properties.bold, None);
    assert_eq!(styled.style.as_ref(), Some(&char_leaf_id));
    assert_eq!(styled.properties.bold, Some(false));
    assert_eq!(styled.properties.italic, Some(false));
    let effective = actual.styles.resolve_run(
        paragraph.style.as_ref(),
        inherited.style.as_ref(),
        &inherited.properties,
    );
    assert_eq!(effective.font_size_pt, Some(14.0));
    assert_eq!(effective.bold, Some(true));
    assert_eq!(effective.italic, Some(true));
    close(
        actual
            .styles
            .resolve_paragraph_with_direct(paragraph.style.as_ref(), &paragraph.properties)
            .left_indent_mm
            .unwrap(),
        11.0,
    );
    let base = actual.styles.styles.get_mut(&base_id).unwrap();
    base.character.font_size_pt = Some(18.0);
    base.character.bold = Some(false);
    base.paragraph.left_indent_mm = Some(22.0);
    actual
        .styles
        .styles
        .get_mut(&char_base_id)
        .unwrap()
        .character
        .color = Some("#654321".into());
    let effective = actual.styles.resolve_run(
        paragraph.style.as_ref(),
        inherited.style.as_ref(),
        &inherited.properties,
    );
    assert_eq!(effective.font_size_pt, Some(18.0));
    assert_eq!(effective.bold, Some(false));
    close(
        actual
            .styles
            .resolve_paragraph_with_direct(paragraph.style.as_ref(), &paragraph.properties)
            .left_indent_mm
            .unwrap(),
        22.0,
    );
    let styled = actual.styles.resolve_run(
        paragraph.style.as_ref(),
        styled.style.as_ref(),
        &styled.properties,
    );
    assert_eq!(styled.bold, Some(false));
    assert_eq!(styled.italic, Some(false));
    assert_eq!(styled.underline, Some(true));
    assert_eq!(styled.color.as_deref(), Some("#654321"));
}

#[test]
fn resolved_theme_fonts_materialize_per_script_when_exporting_binary_doc() {
    let mut document = Document::default();
    let style = document
        .styles
        .styles
        .get_mut(&StyleId::from(STYLE_NORMAL))
        .unwrap();
    style.character.font_slots = Some(FontSlots {
        ascii: Some(FontSlot::explicit("Theme Latin")),
        h_ansi: Some(FontSlot::explicit("Theme Greek")),
        east_asia: Some(FontSlot::explicit("Theme CJK")),
        ..Default::default()
    });
    document.sections[0].blocks = vec![Block::Paragraph(Paragraph::plain("Aα漢"))].into();
    let reopened = cycle(&document);
    let paragraph = find_paragraph(&reopened, "Aα漢");
    for (character, family) in [
        ('A', "Theme Latin"),
        ('α', "Theme Greek"),
        ('漢', "Theme CJK"),
    ] {
        let run = paragraph
            .content
            .iter()
            .filter_map(|item| {
                if let Inline::Text(run) = item {
                    Some(run)
                } else {
                    None
                }
            })
            .find(|run| run.text.contains(character))
            .unwrap();
        assert_eq!(
            reopened
                .styles
                .resolve_run(
                    paragraph.style.as_ref(),
                    run.style.as_ref(),
                    &run.properties
                )
                .font_family
                .as_deref(),
            Some(family)
        );
    }
}
