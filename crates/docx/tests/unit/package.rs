use super::*;
use model::{
    Field, FieldKind, HeaderFooter, InlineImage, ListDefinition, ListLevel, ParagraphProperties,
    Resource, SectionProperties, TableCell, TableProperties, TableRow,
};
use std::io::{Cursor, Read};
use zip::ZipArchive;

fn sample_structured_document() -> Document {
    let mut document=Document::default();
    let image_id=ResourceId::new();
    document.resources.insert(image_id.clone(),Resource { font_faces: Vec::new(), 
        id:image_id.clone(), media_type:"image/png".into(), original_name:Some("sample.png".into()),
        data:vec![0x89,b'P',b'N',b'G'].into(),
    });
    let list_id=NodeId::new();
    document.lists.insert(list_id.clone(),ListDefinition { id:list_id.clone(), levels:vec![ListLevel {
        level:0, format:ListFormat::Decimal, text:"%1.".into(), start_at:1, left_indent_mm:6.35, hanging_indent_mm:3.18,
    }]});
    let mut list_paragraph=Paragraph::plain("First");
    list_paragraph.properties.list_id=Some(list_id); list_paragraph.properties.list_level=Some(0);
    let mut image_paragraph=Paragraph::plain("");
    image_paragraph.content=(vec![Inline::Image(InlineImage { id:NodeId::new(), resource:image_id, width_mm:20.0, height_mm:10.0, alt_text:Some("sample".into()), effects:Default::default() })]).into();
    let table=Table { id:NodeId::new(), properties:TableProperties::default(), rows:vec![TableRow {
        id:NodeId::new(), repeat_as_header:true, allow_break_across_pages: true, height_mm:None, cells:vec![TableCell {
            id:NodeId::new(), blocks:(vec![Block::Paragraph(Paragraph::plain("Cell"))]).into(), column_span:1, row_span:1,
            width_mm:None, shading:Some("#EEEEEE".into()), borders:None, vertical_alignment:Default::default(), margins: None, text_direction: None,
        }],
    }]};
    document.sections[0].properties=SectionProperties { column_count:2, ..Default::default() };
    document.sections[0].blocks=(vec![Block::Paragraph(list_paragraph),Block::Paragraph(image_paragraph),Block::Table(table)]).into();
    document.sections[0].footers.default=Some(HeaderFooter { id:NodeId::new(), blocks:(vec![Block::Paragraph(Paragraph {
        id:NodeId::new(), style:None, properties:ParagraphProperties { alignment:Some(ParagraphAlignment::Center), ..Default::default() },
        content:(vec![Inline::Field(Field { id:NodeId::new(), kind:FieldKind::PageNumber, display_text:None, color:None })]).into(),
    })]).into()});
    document
}

fn zip_text(zip: &mut ZipArchive<Cursor<Vec<u8>>>, name: &str) -> String {
    let mut text=String::new(); zip.by_name(name).unwrap().read_to_string(&mut text).unwrap(); text
}

#[test]
fn structured_export_writes_real_word_parts_and_relationships() {
    let document=sample_structured_document();
    let mut output=Cursor::new(Vec::new()); export_docx(&document,&mut output).unwrap();
    output.set_position(0); let mut zip=ZipArchive::new(output).unwrap();
    let document_xml=zip_text(&mut zip,"word/document.xml");
    assert!(document_xml.contains("<w:tbl>"));
    assert!(document_xml.contains("<w:numPr>"));
    assert!(document_xml.contains("<w:cols w:num=\"2\""));
    assert!(document_xml.contains("<w:drawing>"));
    assert!(document_xml.contains("<w:footerReference"));
    let rels=zip_text(&mut zip,"word/_rels/document.xml.rels");
    assert!(rels.contains("relationships/image")); assert!(rels.contains("relationships/footer"));
    assert!(zip.by_name("word/numbering.xml").is_ok());
    let footer=zip_text(&mut zip,"word/footer1.xml"); assert!(footer.contains("PAGE"));
    assert!(zip.by_name("word/media/image1.png").is_ok());
}

#[test]
fn table_cell_margins_and_writing_direction_round_trip() {
    let mut doc = sample_structured_document();
    let Block::Table(table) = &mut doc.sections[0].blocks[2] else { panic!("expected table") };
    let mm = |twips: f32| twips / (1440.0 / 25.4);
    let all = model::TableCellMargins { top_mm: Some(0.0), right_mm: Some(mm(45.0)),
        bottom_mm: Some(mm(30.0)), left_mm: Some(mm(55.0)) };
    let override_margins = model::TableCellMargins { left_mm: Some(mm(100.0)), ..Default::default() };
    table.properties.cell_margins = Some(all);
    table.rows[0].cells[0].margins = Some(override_margins);
    table.rows[0].cells[0].text_direction = Some(model::CellTextDirection::BottomToTop);

    let mut bytes = Cursor::new(Vec::new());
    export_docx(&doc, &mut bytes).unwrap();
    bytes.set_position(0);
    let imported = crate::import::import_docx(&mut bytes).unwrap();
    let table = imported.sections[0].blocks.iter().find_map(|block| {
        if let Block::Table(table) = block { Some(table) } else { None }
    }).unwrap();
    assert_eq!(table.properties.cell_margins, Some(all));
    let cell = &table.rows[0].cells[0];
    assert_eq!(cell.margins, Some(override_margins));
    assert_eq!(cell.text_direction, Some(model::CellTextDirection::BottomToTop));
}
#[test]
fn vertical_merge_exports_vmerge_and_round_trips_to_row_span() {
    let mut document=Document::default();
    document.sections[0].blocks=(vec![Block::Table(Table {
        id:NodeId::new(), properties:TableProperties::default(), rows:vec![
            TableRow { id:NodeId::new(), repeat_as_header:false, allow_break_across_pages:true, height_mm:None, cells:vec![
                TableCell { id:NodeId::new(), blocks:(vec![Block::Paragraph(Paragraph::plain("A"))]).into(), column_span:1, row_span:2, width_mm:None, shading:None, borders:None, vertical_alignment:Default::default(), margins: None, text_direction: None },
                TableCell { id:NodeId::new(), blocks:(vec![Block::Paragraph(Paragraph::plain("B1"))]).into(), column_span:1, row_span:1, width_mm:None, shading:None, borders:None, vertical_alignment:Default::default(), margins: None, text_direction: None },
            ]},
            TableRow { id:NodeId::new(), repeat_as_header:false, allow_break_across_pages:true, height_mm:None, cells:vec![
                TableCell { id:NodeId::new(), blocks:(vec![Block::Paragraph(Paragraph::plain("B2"))]).into(), column_span:1, row_span:1, width_mm:None, shading:None, borders:None, vertical_alignment:Default::default(), margins: None, text_direction: None },
            ]},
        ],
    })]).into();
    let mut output=Cursor::new(Vec::new()); export_docx(&document,&mut output).unwrap();
    let bytes=output.into_inner();
    let mut zip=ZipArchive::new(Cursor::new(bytes.clone())).unwrap();
    let document_xml=zip_text(&mut zip,"word/document.xml");
    assert!(document_xml.contains("<w:vMerge w:val=\"restart\"/>"));
    assert!(document_xml.contains("<w:vMerge/>"));
    drop(zip);
    let mut input=Cursor::new(bytes);
    let imported=crate::import::import_docx(&mut input).unwrap();
    let table=imported.sections[0].blocks.iter().find_map(|block| if let Block::Table(table)=block{Some(table)}else{None}).unwrap();
    assert_eq!(table.rows[0].cells[0].row_span,2);
    assert_eq!(table.rows[1].cells.len(),1);
}

