//! Attach exact inline-object hit boxes after pagination and header/footer layout.
//! The text-marker region already contains the final baseline and BiDi position.
use super::*;

pub(super) fn annotate_inline_objects(document: &Document, pages: &mut [VectorPage]) {
    let mut objects: HashMap<(String, usize), (String, ObjectKind)> = HashMap::new();
    fn inline(content: &[Inline], paragraph: &str, offset: &mut usize, out: &mut HashMap<(String, usize), (String, ObjectKind)>) {
        for item in content {
            match item {
                Inline::Text(run) => *offset += run.text.len(),
                Inline::Hyperlink(link) => inline(&link.content, paragraph, offset, out),
                Inline::Equation(eq) => {
                    out.insert((paragraph.to_owned(), *offset), (eq.id.0.clone(), ObjectKind::Equation { source: Some((paragraph.to_owned(), *offset)) }));
                    *offset += INLINE_OBJECT_MARKER_LEN;
                }
                Inline::Image(image) => {
                    out.insert((paragraph.to_owned(), *offset), (image.id.0.clone(), ObjectKind::Picture { source: Some((paragraph.to_owned(), *offset)) }));
                    *offset += INLINE_OBJECT_MARKER_LEN;
                }
                Inline::TextBox(boxed) => {
                    out.insert((paragraph.to_owned(), *offset), (boxed.id.0.clone(), ObjectKind::TextBox { source: Some((paragraph.to_owned(), *offset)) }));
                    *offset += INLINE_OBJECT_MARKER_LEN;
                }
                Inline::EmbeddedObject(object) => {
                    out.insert((paragraph.to_owned(), *offset), (object.id.0.clone(), ObjectKind::EmbeddedObject { source: Some((paragraph.to_owned(), *offset)) }));
                    *offset += INLINE_OBJECT_MARKER_LEN;
                }
                Inline::Field(_) => *offset += INLINE_OBJECT_MARKER_LEN,
                Inline::LineBreak { .. } | Inline::PaginationBreak { .. } | Inline::Tab { .. } => *offset += 1,
            }
        }
    }
    fn blocks(items: &[Block], out: &mut HashMap<(String, usize), (String, ObjectKind)>) {
        for block in items { match block {
            Block::Paragraph(p) => inline(&p.content, &p.id.0, &mut 0, out),
            Block::Table(t) => for row in &t.rows { for cell in &row.cells { blocks(&cell.blocks, out); } }, _ => {}
        }}
    }
    for section in &document.sections {
        blocks(&section.blocks, &mut objects);
        for hf in section.header_footers() {
            blocks(&hf.stream.blocks, &mut objects);
        }
    }
    for page in pages {
        for region in &page.hit_regions {
            if region.byte_range.end != region.byte_range.start + INLINE_OBJECT_MARKER_LEN { continue; }
            if let Some((id, kind)) = objects.get(&(region.node_id.clone(), region.byte_range.start)) {
                page.object_regions.push(ObjectHitRegion { node_id: id.clone(), rect: region.rect, kind: kind.clone() });
            }
        }
    }
}

