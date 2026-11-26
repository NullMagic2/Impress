//! Moving and reanchoring a table must preserve its cells and DOCX properties.
use docx::DocxFormat;
use editor::EditorState;
use format::DocumentFormat;
use model::*;
use std::io::{Cursor, Write};
fn document() -> Document {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file(
        "word/document.xml",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    zip.write_all(br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Before</w:t></w:r></w:p><w:tbl><w:tblPr><w:tblW w:w="6000" w:type="dxa"/></w:tblPr><w:tblGrid><w:gridCol w:w="6000"/></w:tblGrid><w:tr><w:tc><w:tcPr><w:shd w:fill="DDEEFF"/></w:tcPr><w:p><w:r><w:rPr><w:b/><w:color w:val="336699"/></w:rPr><w:t>Cell text</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:p><w:r><w:t>Anchor</w:t></w:r></w:p><w:p><w:r><w:t>End</w:t></w:r></w:p></w:body></w:document>"#).unwrap();
    DocxFormat
        .import(&mut Cursor::new(zip.finish().unwrap().into_inner()))
        .unwrap()
}
fn table(document: &Document) -> &Table {
    document.sections[0]
        .blocks
        .iter()
        .find_map(|b| {
            if let Block::Table(t) = b {
                Some(t)
            } else {
                None
            }
        })
        .unwrap()
}
#[test]
fn inline_table_move_preserves_cells_and_is_one_undo_step() {
    let document = document();
    let original = document.clone();
    let t = table(&document).clone();
    let end = document_paragraphs(&document)
        .into_iter()
        .find(|p| inline_text(&p.content) == "End")
        .unwrap()
        .id
        .clone();
    let mut editor = EditorState::new(document);
    editor.move_table(&t.id, Some((end, false)), None).unwrap();
    assert!(
        matches!(editor.document.sections[0].blocks.last(),Some(Block::Table(moved)) if moved==&t)
    );
    assert!(editor.undo());
    assert_eq!(editor.document, original);
    assert!(editor.redo());
    assert_eq!(table(&editor.document), &t);
}
#[test]
fn floating_table_position_round_trips_without_losing_its_format() {
    let document = document();
    let t = table(&document).clone();
    let mut editor = EditorState::new(document);
    let position = TablePosition {
        horizontal: HorizontalPlacement::Offset(12.0),
        vertical: VerticalPlacement::Offset(8.0),
        ..Default::default()
    };
    editor
        .set_table_position(&t.id, Some(position.clone()))
        .unwrap();
    assert_eq!(table(&editor.document).rows, t.rows);
    let mut out = Cursor::new(Vec::new());
    DocxFormat.export(&editor.document, &mut out).unwrap();
    out.set_position(0);
    let reopened = DocxFormat.import(&mut out).unwrap();
    let moved = table(&reopened);
    let saved = moved.properties.position.as_ref().unwrap();
    assert_eq!(saved.horizontal_from, position.horizontal_from);
    assert_eq!(saved.vertical_from, position.vertical_from);
    assert_eq!(saved.wrap, position.wrap);
    assert_eq!(saved.allow_overlap, position.allow_overlap);
    // DOCX stores whole twips (one twip is about 0.01764 mm).
    assert!(matches!(saved.horizontal,HorizontalPlacement::Offset(x) if (x-12.0).abs()<0.018));
    assert!(matches!(saved.vertical,VerticalPlacement::Offset(y) if (y-8.0).abs()<0.018));
    assert!((saved.distance_left_mm - 3.2).abs() < 0.018);
    assert_eq!(moved.rows[0].cells[0].shading, t.rows[0].cells[0].shading);
    let p = match &moved.rows[0].cells[0].blocks[0] {
        Block::Paragraph(p) => p,
        _ => panic!(),
    };
    assert!(
        matches!(&p.content[0],Inline::Text(run) if run.properties.bold==Some(true)&&run.properties.color.as_deref()==Some("#336699"))
    );
}

// Word worksheets often put a whole exercise inside a cell of a layout table.
fn nested_document() -> (Document, NodeId, NodeId, NodeId, NodeId) {
    let mut document = document();
    let inner = table(&document).clone();
    let mut outer = inner.clone();
    outer.id = NodeId::new();
    outer.properties.preferred_width_mm = Some(140.0);
    outer.rows.truncate(1);
    outer.rows[0].id = NodeId::new();
    outer.rows[0].cells.truncate(1);
    let cell = &mut outer.rows[0].cells[0];
    cell.id = NodeId::new();
    let first = Paragraph::plain("Cell anchor");
    let last = Paragraph::plain("Cell end");
    let first_id = first.id.clone();
    let last_id = last.id.clone();
    cell.blocks = vec![Block::Table(inner.clone()), Block::Paragraph(first), Block::Paragraph(last)].into();
    let outer_id = outer.id.clone();
    document.sections[0].blocks[1] = Block::Table(outer);
    (document, inner.id, outer_id, first_id, last_id)
}

#[test]
fn nested_table_moves_inside_its_cell_and_undo_preserves_the_whole_worksheet() {
    let (document, inner, outer, first, last) = nested_document();
    let original = document.clone();
    let mut editor = EditorState::new(document);
    assert!(editor.table_can_move(&inner));
    assert_eq!(editor.table_drag_paragraphs(&inner), [first, last.clone()]);
    let rows = editor.table_by_id(&inner).unwrap().rows.clone();
    editor.move_table(&inner, Some((last, false)), Some(3.0)).unwrap();
    let parent = &editor.table_by_id(&outer).unwrap().rows[0].cells[0];
    assert!(matches!(&parent.blocks[parent.blocks.len()-2], Block::Table(t) if t.id == inner));
    assert!(matches!(parent.blocks.last(), Some(Block::Paragraph(p)) if inline_text(&p.content).is_empty()));
    assert_eq!(editor.table_by_id(&inner).unwrap().rows, rows);
    let moved = editor.document.clone();
    assert!(editor.undo());
    assert_eq!(editor.document, original);
    assert!(!editor.can_undo());
    assert!(editor.redo());
    assert_eq!(editor.document, moved);
}

#[test]
fn nested_positioning_round_trips_and_rejects_an_anchor_outside_its_cell() {
    let (document, inner, outer, _, _) = nested_document();
    let mut editor = EditorState::new(document);
    let rows = editor.table_by_id(&inner).unwrap().rows.clone();
    let outer_properties = editor.table_by_id(&outer).unwrap().properties.clone();
    let position = TablePosition { horizontal: HorizontalPlacement::Offset(5.0), vertical: VerticalPlacement::Offset(4.0),
        vertical_from: VerticalFrom::Margin, ..Default::default() };
    editor.set_table_position(&inner, Some(position.clone())).unwrap();
    assert_eq!(editor.table_by_id(&inner).unwrap().rows, rows);
    assert_eq!(editor.table_by_id(&outer).unwrap().properties, outer_properties);
    let before_rejected_move = editor.document.clone();
    let outside = match &editor.document.sections[0].blocks[0] { Block::Paragraph(p) => p.id.clone(), _ => panic!() };
    assert!(editor.move_table(&inner, Some((outside, false)), None).is_err());
    assert_eq!(editor.document, before_rejected_move);
    let mut output = Cursor::new(Vec::new());
    DocxFormat.export(&editor.document, &mut output).unwrap();
    let imported = DocxFormat.import(&mut Cursor::new(output.into_inner())).unwrap();
    let saved_outer = table(&imported);
    let saved_inner = saved_outer.rows[0].cells[0].blocks.iter().find_map(|b| match b { Block::Table(t) => Some(t), _ => None }).unwrap();
    let saved = saved_inner.properties.position.as_ref().unwrap();
    assert!(matches!(saved.horizontal, HorizontalPlacement::Offset(x) if (x-5.0).abs()<0.018));
    assert!(matches!(saved.vertical, VerticalPlacement::Offset(y) if (y-4.0).abs()<0.018));
    assert_eq!(saved_inner.rows[0].cells[0].shading, rows[0].cells[0].shading);
}

#[test]
fn selecting_a_containing_cell_does_not_select_its_first_nested_table() {
    let (document, inner, outer, _, _) = nested_document();
    let mut editor = EditorState::new(document);
    editor.select_cell(&inner, 0, 0).unwrap();
    assert_eq!(editor.table_cell_context().unwrap().table_id, inner);
    editor.select_cell(&outer, 0, 0).unwrap();
    assert_eq!(editor.table_cell_context().unwrap().table_id, outer);
}

#[test]
fn every_table_wrapping_mode_survives_docx_and_undo_redo() {
    for wrap in [TableWrap::Around, TableWrap::TopAndBottom, TableWrap::InFrontOfText, TableWrap::BehindText] {
        let doc = document();
        let id = table(&doc).id.clone();
        let rows = table(&doc).rows.clone();
        let mut editor = EditorState::new(doc);
        let position = TablePosition { wrap, vertical: VerticalPlacement::Offset(14.0), ..Default::default() };
        editor.set_table_position(&id, Some(position)).unwrap();
        assert!(editor.undo()); assert!(table(&editor.document).properties.position.is_none());
        assert!(editor.redo()); assert_eq!(table(&editor.document).properties.position.as_ref().unwrap().wrap, wrap);
        let mut bytes = Cursor::new(Vec::new());
        DocxFormat.export(&editor.document, &mut bytes).unwrap(); bytes.set_position(0);
        let loaded = DocxFormat.import(&mut bytes).unwrap();
        let loaded_table = table(&loaded);
        assert_eq!(loaded_table.properties.position.as_ref().unwrap().wrap, wrap);
        assert_eq!(inline_text(&document_paragraphs(&loaded)[1].content), "Cell text");
        assert_eq!(loaded_table.rows[0].cells[0].shading, rows[0].cells[0].shading);
    }
}

#[test]
fn previous_overlap_documents_open_as_in_front_of_text() {
    let wrap: TableWrap = serde_json::from_str("\"overlap\"").unwrap();
    assert_eq!(wrap, TableWrap::InFrontOfText);
    let mut doc = document();
    let Block::Table(t) = &mut doc.sections[0].blocks[1] else { panic!("table") };
    t.properties.position = Some(TablePosition { wrap, ..Default::default() });
    let mut bytes = Cursor::new(Vec::new()); DocxFormat.export(&doc, &mut bytes).unwrap();
    let mut input = zip::ZipArchive::new(Cursor::new(bytes.into_inner())).unwrap();
    let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..input.len() {
        use std::io::Read;
        let mut entry = input.by_index(index).unwrap(); let mut data = Vec::new(); entry.read_to_end(&mut data).unwrap();
        if entry.name() == "word/document.xml" { data = String::from_utf8(data).unwrap().replace("impress:wrap=\"in_front_of_text\"", "impress:wrap=\"overlap\"").into_bytes(); }
        output.start_file(entry.name(), zip::write::SimpleFileOptions::default()).unwrap(); output.write_all(&data).unwrap();
    }
    let loaded = DocxFormat.import(&mut Cursor::new(output.finish().unwrap().into_inner())).unwrap();
    assert_eq!(table(&loaded).properties.position.as_ref().unwrap().wrap, TableWrap::InFrontOfText);
}

#[test]
fn overlap_switch_preserves_the_layout_slot_until_an_explicit_move() {
    for nested in [false,true] {
        for positioned in [false,true] {
            let (doc,id)=if nested {let(d,id,_,_,_)=nested_document();(d,id)} else {let d=document();let id=table(&d).id.clone();(d,id)};
            let mut editor=EditorState::new(doc);
            if positioned { editor.set_table_position(&id,Some(TablePosition {horizontal:HorizontalPlacement::Offset(12.0),vertical:VerticalPlacement::Offset(8.0),..Default::default()})).unwrap(); }
            let original=editor.table_by_id(&id).unwrap().clone();
            editor.set_table_wrap(&id,TableWrap::InFrontOfText).unwrap();
            let first=editor.table_by_id(&id).unwrap().properties.position.clone().unwrap();
            assert_eq!(first.flow_position(),original.properties.position.as_ref());
            editor.set_table_wrap(&id,TableWrap::BehindText).unwrap();
            let second=editor.table_by_id(&id).unwrap().properties.position.clone().unwrap();
            assert_eq!(first.preserved_text_flow,second.preserved_text_flow);
            assert_eq!(editor.table_by_id(&id).unwrap().rows,original.rows);
            let mut bytes=Cursor::new(Vec::new());DocxFormat.export(&editor.document,&mut bytes).unwrap();bytes.set_position(0);
            let loaded=DocxFormat.import(&mut bytes).unwrap();let loaded=EditorState::new(loaded);
            // Native IDs are regenerated by DOCX; find the semantic counterpart.
            let current=if nested {
                let outer=table(&loaded.document);outer.rows[0].cells[0].blocks.iter().find_map(|b|match b{Block::Table(t)=>Some(t),_=>None}).unwrap()
            }else{table(&loaded.document)};
            assert_eq!(current.properties.position.as_ref().unwrap().preserved_text_flow,second.preserved_text_flow);
            assert!(editor.undo());assert_eq!(editor.table_by_id(&id).unwrap().properties.position.as_ref().unwrap().wrap,TableWrap::InFrontOfText);
            assert!(editor.redo());
            let mut explicit=second.clone();explicit.vertical=VerticalPlacement::Offset(20.0);
            editor.set_table_position(&id,Some(explicit)).unwrap();
            assert!(editor.table_by_id(&id).unwrap().properties.position.as_ref().unwrap().preserved_text_flow.is_none());
            assert!(editor.undo());assert_eq!(editor.table_by_id(&id).unwrap().properties.position,Some(second));
        }
    }
}

#[test]
fn dragging_each_wrap_mode_keeps_source_order_and_nested_settings_and_undoes_exactly() {
    for wrap in [TableWrap::Around, TableWrap::TopAndBottom, TableWrap::InFrontOfText, TableWrap::BehindText] {
        let (doc, inner, outer, _, _) = nested_document();
        let mut editor = EditorState::new(doc);
        editor.set_table_wrap(&outer, wrap).unwrap();
        let before = editor.document.clone();
        let inner_before = editor.table_by_id(&inner).unwrap().clone();
        editor.offset_table(&outer, TableOffset { x_mm: 7.0, y_mm: 24.0, page_delta: 1 }).unwrap();
        let after = editor.document.clone();
        assert_eq!(editor.table_by_id(&inner).unwrap(), &inner_before, "{wrap:?} modified a nested table");
        assert_eq!(editor.table_by_id(&outer).unwrap().properties.position.as_ref().unwrap().wrap, wrap);
        assert!(editor.undo()); assert_eq!(editor.document, before);
        assert!(editor.redo()); assert_eq!(editor.document, after);
        assert!(editor.undo()); assert!(editor.undo());
        assert!(editor.table_by_id(&outer).unwrap().properties.position.is_none());
        assert!(editor.redo()); assert!(editor.redo()); assert_eq!(editor.document, after);
        let mut bytes = Cursor::new(Vec::new()); DocxFormat.export(&editor.document, &mut bytes).unwrap(); bytes.set_position(0);
        let loaded = DocxFormat.import(&mut bytes).unwrap();
        let saved = table(&loaded).properties.position.as_ref().unwrap();
        let expected = editor.table_by_id(&outer).unwrap().properties.position.as_ref().unwrap();
        assert_eq!(saved.wrap, expected.wrap); assert_eq!(saved.visual_offset, expected.visual_offset);
        assert_eq!(saved.preserved_text_flow, expected.preserved_text_flow);
        assert!((saved.distance_left_mm - expected.distance_left_mm).abs() < 0.018);
    }
}
