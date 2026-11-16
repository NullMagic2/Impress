//! Executes the ACTUAL generated source through the pinned Typst engine.
//! Run with `cargo test -p typst-engine --test generated_source`.
use model::*;
use format_typst::export_structured;
use typst_engine::TypstTypesetter;

#[test]
fn generated_title_heading_bold_italic_and_escaped_text_compile() {
    let mut document = Document::default();
    document.styles.styles.get_mut(&StyleId::from(STYLE_NORMAL)).unwrap().character.font_family = Some("New Computer Modern".into());
    let mut title = Paragraph::plain("A title"); title.style = Some(StyleId::from(STYLE_TITLE));
    let mut heading = Paragraph::plain("A heading"); heading.style = Some(StyleId::from(STYLE_HEADING_1));
    let mut paragraph = Paragraph::plain("");
    for (text, bold, italic) in [("Regular ", false, false), ("Bold ", true, false), ("Italic ", false, true),
        ("Both [#not-code] _literal_ \"quoted\" ff ffi ffl", true, true)] {
        let mut run = TextRun::plain(text); run.properties.bold = Some(bold); run.properties.italic = Some(italic);
        run.properties.typography = Some(OpenTypeTypography { ligatures: LigatureMode::Discretionary, ..Default::default() });
        paragraph.content.push(Inline::Text(run));
    }
    document.sections[0].blocks = vec![Block::Paragraph(title), Block::Paragraph(heading), Block::Paragraph(paragraph)].into();
    let source = export_structured(&document);
    assert!(source.contains("#title[")); assert!(source.contains("#heading(level: 1"));
    let pages = TypstTypesetter::new().with_system_fonts(false).compile_document_svg_pages(source).unwrap();
    assert!(!pages.is_empty()); assert!(pages.iter().all(|page| page.contains("<svg")));
}
