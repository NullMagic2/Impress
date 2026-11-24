use docx::DocxFormat;
use format::DocumentFormat;
use model::Block;
use std::io::{Cursor, Read};

const ORIGINAL: &[u8] = include_bytes!("fixtures/word2013-table-review.docx");

fn roundtrip() -> (model::Document, Vec<u8>) {
    let document = DocxFormat.import(&mut Cursor::new(ORIGINAL)).unwrap();
    let mut output = Cursor::new(Vec::new());
    DocxFormat.export(&document, &mut output).unwrap();
    (document, output.into_inner())
}

fn xml(bytes: &[u8], name: &str) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut text = String::new();
    zip.by_name(name).unwrap().read_to_string(&mut text).unwrap();
    text
}

#[test]
fn word_comment_retains_visible_text_without_page_instructions() {
    let (document, saved) = roundtrip();
    let comments = xml(&saved, "word/comments.xml");
    assert!(comments.contains("Compare this cell in both editors."));
    assert!(!comments.contains("PAGE"));
    assert_eq!(document.review.comments.len(), 1);
}

#[test]
fn authored_fixed_table_algorithm_survives_native_and_docx_saves() {
    let (document, saved) = roundtrip();
    let table = document.sections[0].blocks.iter().find_map(|block| {
        if let Block::Table(table) = block { Some(table) } else { None }
    }).unwrap();
    assert_eq!(table.properties.fixed_layout, Some(true));
    assert!(xml(&saved, "word/document.xml").contains("<w:tblLayout w:type=\"fixed\"/>"));
    let reimported = DocxFormat.import(&mut Cursor::new(saved)).unwrap();
    let Block::Table(table) = &reimported.sections[0].blocks[2] else { panic!("missing table") };
    assert_eq!(table.properties.fixed_layout, Some(true));
}

#[test]
fn saving_does_not_request_unsolicited_field_updates() {
    let (_, saved) = roundtrip();
    assert!(!xml(ORIGINAL, "word/settings.xml").contains("updateFields"));
    assert!(!xml(&saved, "word/settings.xml").contains("updateFields"));
}

#[test]
fn absent_note_parts_do_not_leave_separator_references_in_settings() {
    use std::io::Write;
    let mut input = zip::ZipArchive::new(Cursor::new(ORIGINAL)).unwrap();
    let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..input.len() {
        let mut file = input.by_index(index).unwrap();
        let name = file.name().to_owned();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        if name == "word/settings.xml" {
            let text = String::from_utf8(bytes).unwrap();
            bytes = text.replace("</w:settings>",
                r#"<w:footnotePr><w:footnote w:id="-1"/><w:footnote w:id="0"/></w:footnotePr><w:endnotePr><w:endnote w:id="-1"/><w:endnote w:id="0"/></w:endnotePr></w:settings>"#).into_bytes();
        }
        output.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        output.write_all(&bytes).unwrap();
    }
    let document = DocxFormat.import(&mut Cursor::new(output.finish().unwrap().into_inner())).unwrap();
    let mut saved = Cursor::new(Vec::new());
    DocxFormat.export(&document, &mut saved).unwrap();
    let settings = xml(saved.get_ref(), "word/settings.xml");
    assert!(!settings.contains("<w:footnote "));
    assert!(!settings.contains("<w:endnote "));
}

#[test]
fn the_native_normal_fallback_cannot_replace_words_normal_style() {
    let (_, saved) = roundtrip();
    let styles = xml(&saved, "word/styles.xml");
    assert_eq!(styles.split("<w:name").skip(1).filter(|element| element.split("/>").next().unwrap().contains("w:val=\"Normal\"")).count(), 1);
    assert!(styles.contains("w:val=\"Normal (builtin.normal)\""));
    let document = DocxFormat.import(&mut Cursor::new(saved)).unwrap();
    assert_eq!(document.styles.resolve_character(Some(&model::StyleId("Normal".into()))).font_family.as_deref(), Some("Arial"));
}
