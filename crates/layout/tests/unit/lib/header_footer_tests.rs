use super::*;
use model::Paragraph;

fn paragraphs(count: usize, text: &str) -> model::Shared<Vec<Block>> {
    (0..count).map(|_| Block::Paragraph(Paragraph::plain(text))).collect::<Vec<_>>().into()
}

fn document_with_header(lines: usize) -> Document {
    let mut document = Document::default();
    document.sections[0].blocks = paragraphs(1, "Body text");
    document.sections[0].headers.default = Some(HeaderFooter { id: NodeId::new(), blocks: paragraphs(lines, "Header line") });
    document
}

fn body_top(document: &Document) -> f32 {
    let page = &LayoutEngine::default().layout_document(document).unwrap().pages[0];
    let body_id = match &document.sections[0].blocks[0] { Block::Paragraph(p) => p.id.0.clone(), _ => unreachable!() };
    page.hit_regions.iter().filter(|region| region.node_id == body_id).map(|region| region.rect.y).fold(f32::MAX, f32::min)
}

#[test]
fn tall_header_pushes_the_body_down_instead_of_overlapping_it() {
    let document = document_with_header(12);
    let extents = HeaderFooterExtents::measure(&document.sections[0], &document).unwrap();
    let header_bottom = header_top(&document.sections[0]) + extents.header;
    assert!(header_bottom > document.sections[0].properties.margin_top_mm * MM_TO_PT);
    assert!(body_top(&document) >= header_bottom + HEADER_FOOTER_BODY_GAP_PT - 0.5);
}

