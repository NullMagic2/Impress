//! Render the built-in contents designs with the same paragraphs used by the editor.
use super::*;
use model::{Block, Document, HeaderFooter, Inline, Paragraph};
const TEXT_WIDTH_MM: f32 = 68.0;

fn sample_editor() -> EditorState {
    let mut doc = Document::default();
    let section = &mut doc.sections[0];
    section.properties.page_width_mm = TEXT_WIDTH_MM
        + section.properties.margin_left_mm + section.properties.margin_right_mm;
    section.blocks = [("The landscape", 1), ("Rivers and valleys", 2), ("River crossings", 3), ("Field notes", 1)]
        .into_iter().map(|(label, level)| {
            let mut paragraph = Paragraph::plain(&text(label));
            paragraph.properties.outline_level = Some(level);
            Block::Paragraph(paragraph)
        }).collect();
    let first = model::document_paragraphs(&doc)[0].id.clone();
    let mut editor = EditorState::new(doc);
    editor.set_caret(first, 0);
    editor
}

pub(super) fn sample_fragment(options: ContentsOptions) -> Option<render::VectorFragment> {
    let mut editor = sample_editor();
    let pages: Vec<_> = [(1, 1), (2, 3), (3, 7), (1, 17)].into_iter()
        .filter(|(level, _)| *level <= options.max_heading_level)
        .map(|(_, page)| page).collect();
    editor.update_contents_options(options).ok()?;
    let mut stream = HeaderFooter::default();
    let paragraphs = model::document_paragraphs(&editor.document);
    for (index, id) in editor.document.review.contents_paragraphs.iter().enumerate() {
        let mut paragraph = (*paragraphs.iter().find(|p| &p.id == id)?).clone();
        if let Some(Inline::Hyperlink(link)) = paragraph.content.first_mut() {
            if let Some(Inline::Field(field)) = link.content.last_mut() {
                field.display_text = Some(pages[index - 1].to_string());
            }
        }
        stream.blocks.push(Block::Paragraph(paragraph));
    }
    layout::LayoutEngine::default()
        .layout_header_footer_preview(&stream, TEXT_WIDTH_MM, &editor.document).ok()
}
