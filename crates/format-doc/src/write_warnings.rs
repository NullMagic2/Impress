//! Report only actual native DOC export losses and Word97 representation limits.
use model::*;
const OBJECT_WARNING:&str="Native DOC export replaces embedded objects, equations and unsupported content with readable placeholders.";
const EFFECT_WARNING:&str="Native DOC export preserves rectangular picture crop; shape crops, outlines, advanced shadow, glow, reflection and soft-edge effects are not preserved.";
pub(super) fn export(document: &Document) -> Vec<String> {
    let mut warnings = Vec::new();
    if !document.content_controls.is_empty() || !document.custom_xml.is_empty() {
        warnings.push("Content controls and XML bindings cannot be represented in binary DOC; keep a DOCX or Impress copy.".into());
    }
    if !document.review.revisions.is_empty()
        || !document.review.history.is_empty()
        || !document.review.tracked_changes.is_empty()
    {
        warnings.push("Native DOC export keeps the final text without revision history.".into());
    }
    if document
        .review
        .comments
        .values()
        .any(|comment| comment.is_closed() || comment.date.is_some() || !comment.replies.is_empty())
    {
        warnings.push("Native DOC comments retain their author, text and range; threaded replies, dates and resolved state are not preserved.".into());
    }
    for section in &document.sections {
        visit_blocks(&section.blocks, &mut warnings);
        for stream in section.header_footers() {
            visit_blocks(&stream.stream.blocks, &mut warnings);
        }
    }
    for note in document.review.footnotes.values() {
        visit_blocks(&note.blocks, &mut warnings);
    }
    if document
        .resources
        .values()
        .any(|resource| !resource.font_faces.is_empty())
    {
        warnings
            .push("Native DOC export does not embed font files; use DOCX to carry fonts.".into());
    }
    warnings.sort();
    warnings.dedup();
    warnings
}
fn effects(value: &ImageEffects, warnings: &mut Vec<String>) {
    if value.outer_shadow.is_some()
        || value.glow.is_some()
        || value.reflection.is_some()
        || value.soft_edge_mm.is_some()
        || value.shape.preset != "rect"
        || value.outline.is_some()
    {
        warnings.push(EFFECT_WARNING.into());
    }
}
fn visit_blocks(blocks: &[Block], warnings: &mut Vec<String>) {
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => visit_inline(&paragraph.content, warnings),
            Block::Table(table) => {
                if table
                    .properties
                    .position
                    .as_ref()
                    .is_some_and(|position| position.wrap == TableWrap::TopAndBottom)
                {
                    warnings.push(
                        "DOC top-and-bottom table wrapping uses side exclusion distances.".into(),
                    );
                }
                for row in &table.rows {
                    for cell in &row.cells {
                        visit_blocks(&cell.blocks, warnings);
                    }
                }
            }
            Block::Image(image) => effects(&image.effects, warnings),
            Block::Equation(_) | Block::Foreign(_) => warnings.push(OBJECT_WARNING.into()),
            _ => {}
        }
    }
}
fn visit_inline(items: &[Inline], warnings: &mut Vec<String>) {
    for item in items {
        match item {
            Inline::TextBox(boxed) => {
                warnings.push("DOC text boxes are exported as inline text.".into());
                visit_blocks(&boxed.blocks, warnings);
            }
            Inline::Image(image) => effects(&image.effects, warnings),
            Inline::EmbeddedObject(_) | Inline::Equation(_) => warnings.push(OBJECT_WARNING.into()),
            Inline::Hyperlink(link) => visit_inline(&link.content, warnings),
            _ => {}
        }
    }
}
