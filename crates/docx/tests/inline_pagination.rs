//! Exact inline flow boundaries survive semantic edits and native serialization.
use docx::DocxFormat;
use format::DocumentFormat;
use model::*;
use std::io::{Cursor, Write};

fn package(body: &str) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file(
        "word/document.xml",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    write!(zip, r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}<w:sectPr/></w:body></w:document>"#).unwrap();
    zip.finish().unwrap().into_inner()
}
fn load(bytes: Vec<u8>) -> Document {
    DocxFormat.import(&mut Cursor::new(bytes)).unwrap()
}
#[test]
fn mixed_page_and_column_breaks_keep_one_paragraph_unicode_offsets_and_order() {
    let body = r#"<w:p><w:r><w:t>α😀</w:t><w:br w:type="page"/><w:t>Middle</w:t><w:br w:type="column"/><w:t>End</w:t></w:r></w:p>"#;
    let document = load(package(body));
    let paragraphs = document_paragraphs(&document);
    assert_eq!(paragraphs.len(), 1);
    let paragraph = paragraphs[0];
    assert_eq!(
        inline_text(&paragraph.content),
        "α😀\u{000C}Middle\u{000E}End"
    );
    assert!(matches!(
        paragraph.content[1],
        Inline::PaginationBreak {
            break_kind: PaginationBreakKind::Page,
            ..
        }
    ));
    assert!(matches!(
        paragraph.content[3],
        Inline::PaginationBreak {
            break_kind: PaginationBreakKind::Column,
            ..
        }
    ));
    let (left, right) = split_content(&paragraph.content, "α😀\u{000C}".len()).unwrap();
    assert_eq!(inline_text(&left), "α😀\u{000C}");
    assert_eq!(inline_text(&right), "Middle\u{000E}End");
    let mut bytes = Cursor::new(Vec::new());
    DocxFormat.export(&document, &mut bytes).unwrap();
    let reopened = load(bytes.into_inner());
    assert_eq!(
        inline_text(&document_paragraphs(&reopened)[0].content),
        inline_text(&paragraph.content)
    );
    assert!(!reopened
        .compatibility
        .iter()
        .any(|issue| issue.code == "inline-pagination-break"));
}
