//! Concrete theme fonts participate in style inheritance, shaping and editable export.
use docx::DocxFormat;
use format::DocumentFormat;
use model::*;
use std::io::{Cursor, Read, Write};
const WORD: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const DRAWING: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
fn fixture(body: &str, styles: &str, settings: &str, theme: bool) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let mut add = |name: &str, xml: String| {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(xml.as_bytes()).unwrap();
    };
    add(
        "word/document.xml",
        format!(r#"<w:document xmlns:w="{WORD}"><w:body>{body}<w:sectPr/></w:body></w:document>"#),
    );
    add(
        "word/styles.xml",
        format!(r#"<w:styles xmlns:w="{WORD}">{styles}</w:styles>"#),
    );
    add(
        "word/settings.xml",
        format!(r#"<w:settings xmlns:w="{WORD}">{settings}</w:settings>"#),
    );
    add("word/_rels/document.xml.rels",r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rTheme" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="theme/custom.xml"/></Relationships>"#.into());
    if theme {
        add(
            "word/theme/custom.xml",
            format!(
                r#"<a:theme xmlns:a="{DRAWING}"><a:themeElements><a:fontScheme name="Custom"><a:majorFont><a:latin typeface="Heading Latin"/><a:ea typeface=""/><a:cs typeface=""/><a:font script="Jpan" typeface="Heading Japanese"/><a:font script="Hans" typeface="Heading Chinese"/><a:font script="Arab" typeface="Heading Arabic"/></a:majorFont><a:minorFont><a:latin typeface="Body Latin"/><a:ea typeface=""/><a:cs typeface=""/><a:font script="Jpan" typeface="Body Japanese"/><a:font script="Hans" typeface="Body Chinese"/><a:font script="Hant" typeface="Body Traditional"/><a:font script="Arab" typeface="Body Arabic"/></a:minorFont></a:fontScheme></a:themeElements></a:theme>"#
            ),
        );
    }
    zip.finish().unwrap().into_inner()
}
fn load(bytes: Vec<u8>) -> Document {
    DocxFormat.import(&mut Cursor::new(bytes)).unwrap()
}
fn export(document: &Document) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    DocxFormat.export(document, &mut bytes).unwrap();
    bytes.into_inner()
}
fn props(document: &Document, index: usize) -> CharacterProperties {
    let p = document_paragraphs(document)[index];
    let Inline::Text(run) = &p.content[0] else {
        panic!("text")
    };
    document
        .styles
        .resolve_run(p.style.as_ref(), run.style.as_ref(), &run.properties)
}
fn xml(bytes: &[u8], name: &str) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = String::new();
    zip.by_name(name).unwrap().read_to_string(&mut out).unwrap();
    out
}
const DEFAULTS: &str = r#"<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:asciiTheme="minorAscii" w:hAnsiTheme="minorHAnsi" w:eastAsiaTheme="minorEastAsia" w:cstheme="minorBidi"/></w:rPr></w:rPrDefault></w:docDefaults><w:style w:type="paragraph" w:styleId="Normal" w:default="1"><w:name w:val="Normal"/></w:style><w:style w:type="paragraph" w:styleId="Heading"><w:name w:val="Heading"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:rPr><w:rFonts w:ascii="Ignored" w:asciiTheme="majorAscii" w:hAnsiTheme="majorHAnsi"/><w:emboss/><w:noProof/><w:vertAlign w:val="superscript"/><w:bCs w:val="0"/></w:rPr><w:uiPriority w:val="8"/></w:style>"#;
#[test]
fn defaults_styles_regional_scripts_and_direct_overrides_resolve_actual_fonts() {
    let body = r#"<w:p><w:pPr><w:pStyle w:val="Heading"/></w:pPr><w:r><w:t>Aα中</w:t></w:r></w:p><w:p><w:pPr><w:pStyle w:val="Heading"/></w:pPr><w:r><w:rPr><w:rFonts w:ascii="Direct"/></w:rPr><w:t>Aα中</w:t></w:r></w:p><w:p><w:r><w:rPr><w:lang w:eastAsia="ja-JP"/></w:rPr><w:t>中</w:t></w:r></w:p><w:p><w:r><w:rPr><w:rtl/><w:lang w:bidi="ar-SA"/></w:rPr><w:t>ع</w:t></w:r></w:p>"#;
    let document = load(fixture(body, DEFAULTS, "", true));
    assert_eq!(
        props(&document, 0).font_family_for('A'),
        Some("Heading Latin")
    );
    assert_eq!(
        props(&document, 0).font_family_for('α'),
        Some("Heading Latin")
    );
    assert_eq!(
        props(&document, 0).font_family_for('中'),
        Some("Body Chinese")
    );
    assert_eq!(props(&document, 1).font_family_for('A'), Some("Direct"));
    assert_eq!(
        props(&document, 1).font_family_for('α'),
        Some("Heading Latin")
    );
    assert_eq!(
        props(&document, 2).font_family_for('中'),
        Some("Body Japanese")
    );
    assert_eq!(
        props(&document, 3).font_family_for('ع'),
        Some("Body Arabic")
    );
    assert!(!document
        .compatibility
        .iter()
        .any(|issue| issue.code == "theme-fonts"));
}
#[test]
fn theme_languages_and_theme_selectors_survive_export_and_imported_style_edits() {
    let body = r#"<w:p><w:pPr><w:pStyle w:val="Heading"/></w:pPr><w:r><w:t>A中</w:t></w:r></w:p>"#;
    let mut document = load(fixture(
        body,
        DEFAULTS,
        r#"<w:themeFontLang w:val="ja-JP" w:eastAsia="zh-TW" w:bidi="ar-SA"/>"#,
        true,
    ));
    assert_eq!(
        props(&document, 0).font_family_for('A'),
        Some("Heading Japanese")
    );
    assert_eq!(
        props(&document, 0).font_family_for('中'),
        Some("Body Traditional")
    );
    let bytes = export(&document);
    let reopened = load(bytes.clone());
    assert_eq!(
        props(&reopened, 0).font_family_for('A'),
        Some("Heading Japanese")
    );
    assert_eq!(
        props(&reopened, 0).font_family_for('中'),
        Some("Body Traditional")
    );
    assert!(xml(&bytes, "word/_rels/document.xml.rels").contains("theme/custom.xml"));
    let normal = document
        .styles
        .styles
        .get_mut(&StyleId("Normal".into()))
        .unwrap();
    normal.paragraph.keep_with_next = Some(true);
    normal.paragraph.keep_lines_together = Some(true);
    normal.paragraph.page_break_before = Some(true);
    normal.paragraph.widow_control = Some(false);
    normal.paragraph.direction = Some(ParagraphDirection::RightToLeft);
    let heading = document
        .styles
        .styles
        .get_mut(&StyleId("Heading".into()))
        .unwrap();
    heading.character.set_font_family(Some("Edited".into()));
    heading.paragraph.keep_with_next = Some(false);
    heading.paragraph.keep_lines_together = Some(false);
    heading.paragraph.page_break_before = Some(false);
    heading.paragraph.widow_control = Some(true);
    heading.paragraph.direction = Some(ParagraphDirection::LeftToRight);
    heading.name = "Edited heading".into();
    let bytes = export(&document);
    let reopened = load(bytes.clone());
    assert_eq!(props(&reopened, 0).font_family_for('A'), Some("Edited"));
    assert_eq!(props(&reopened, 0).font_family_for('中'), Some("Edited"));
    let styles = xml(&bytes, "word/styles.xml");
    assert!(styles.contains("w:emboss"));
    assert!(styles.contains("w:noProof"));
    assert!(styles.contains("w:vertAlign"));
    assert!(styles.contains("w:bCs"));
    let paragraph = reopened
        .styles
        .resolve_paragraph(Some(&StyleId("Heading".into())));
    assert_eq!(
        (
            paragraph.keep_with_next,
            paragraph.keep_lines_together,
            paragraph.page_break_before,
            paragraph.widow_control,
            paragraph.direction
        ),
        (
            Some(false),
            Some(false),
            Some(false),
            Some(true),
            Some(ParagraphDirection::LeftToRight)
        )
    );
    assert!(styles.contains("w:uiPriority"));
    assert!(styles.contains("Edited heading"));
}
#[test]
fn missing_theme_reports_fallback_but_direct_font_remains_usable() {
    let body = r#"<w:p><w:r><w:rPr><w:rFonts w:asciiTheme="minorAscii" w:ascii="Explicit fallback"/></w:rPr><w:t>Text</w:t></w:r></w:p>"#;
    let document = load(fixture(body, "", "", false));
    assert_eq!(
        props(&document, 0).font_family_for('A'),
        Some("Explicit fallback")
    );
    assert_eq!(
        document
            .compatibility
            .iter()
            .filter(|issue| issue.code == "theme-fonts")
            .count(),
        1
    );
}
#[test]
fn aliased_style_namespaces_and_local_extension_declarations_survive_semantic_updates() {
    let styles = DEFAULTS.replace(
        "<w:emboss/>",
        "<w:emboss/><extra:keep extra:flag=\"yes\">Literal preserved</extra:keep>",
    );
    let styles = styles.replace("<w:rPr>", "<w:rPr xmlns:extra=\"urn:local-style\">");
    let bytes = fixture(
        "<w:p><w:pPr><w:pStyle w:val=\"Heading\"/></w:pPr><w:r><w:t>Heading</w:t></w:r></w:p>",
        &styles,
        "",
        true,
    );
    let mut source = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..source.len() {
        let mut member = source.by_index(index).unwrap();
        let name = member.name().to_owned();
        let mut data = Vec::new();
        member.read_to_end(&mut data).unwrap();
        if name == "word/styles.xml" {
            data = String::from_utf8(data)
                .unwrap()
                .replace("w:", "x:")
                .replace("xmlns:w=", "xmlns:x=")
                .into_bytes();
        }
        output
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        output.write_all(&data).unwrap();
    }
    let mut document = load(output.finish().unwrap().into_inner());
    document
        .styles
        .styles
        .get_mut(&StyleId("Heading".into()))
        .unwrap()
        .character
        .set_font_family(Some("Edited".into()));
    let bytes = export(&document);
    let styles = xml(&bytes, "word/styles.xml");
    let mut reader = quick_xml::NsReader::from_str(&styles);
    let mut extension = false;
    loop {
        match reader.read_event().unwrap() {
            quick_xml::events::Event::Start(event) | quick_xml::events::Event::Empty(event) => {
                let (resolved, local) = reader.resolver().resolve_element(event.name());
                assert!(
                    !matches!(resolved, quick_xml::name::ResolveResult::Unknown(_)),
                    "unbound prefix in {styles}"
                );
                if local.as_ref() == "keep" {
                    assert!(
                        matches!(resolved,quick_xml::name::ResolveResult::Bound(uri) if uri.as_ref()=="urn:local-style")
                    );
                    extension = true;
                }
            }
            quick_xml::events::Event::Eof => break,
            _ => {}
        }
    }
    assert!(extension);
    assert!(styles.contains("Literal preserved"));
    assert_eq!(props(&load(bytes), 0).font_family_for('A'), Some("Edited"));
}
