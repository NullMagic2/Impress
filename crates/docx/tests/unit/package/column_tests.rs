use super::*;
use model::{ColumnSpec, SectionStart};

#[test]
fn unequal_columns_line_between_and_new_column_round_trip() {
    let mut document = Document::default();
    let properties = &mut document.sections[0].properties;
    properties.column_count = 2;
    properties.columns = vec![ColumnSpec { width_mm: 50.0, spacing_mm: 12.5 }, ColumnSpec { width_mm: 100.0, spacing_mm: 0.0 }];
    properties.column_separator = true;
    properties.start_on = SectionStart::NewColumn;
    document.sections[0].blocks.push(Block::Paragraph(Paragraph::plain("Columns")));
    let mut bytes = std::io::Cursor::new(Vec::new());
    export_docx(&document, &mut bytes).unwrap();
    let back = crate::import::import_docx(&mut std::io::Cursor::new(bytes.into_inner())).unwrap();
    let properties = &back.sections[0].properties;
    assert!(properties.column_separator);
    assert_eq!(properties.start_on, SectionStart::NewColumn);
    assert_eq!(properties.columns.len(), 2);
    assert!((properties.columns[0].width_mm - 50.0).abs() < 0.1);
    assert!((properties.columns[0].spacing_mm - 12.5).abs() < 0.1);
    assert!((properties.columns[1].width_mm - 100.0).abs() < 0.1);
}
