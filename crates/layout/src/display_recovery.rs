//! Last-resort editing view for a section that failed normal pagination.
//! All substitution happens in the scene. The document being edited and saved
//! retains its original paragraphs, images, equations and table structure.
use super::*;

pub(super) fn section(
    document: &Document,
    section: &Section,
    geometry: &SectionGeometry,
    initial: Option<(usize, f32)>,
    background: bool,
    fields: &FieldContext,
    cancel: Option<(&AtomicU64, u64)>,
) -> Result<(Vec<VectorPage>, FlowCursor), LayoutError> {
    let mut pages = vec![new_page(
        geometry.page_width,
        geometry.page_height,
        background,
    )];
    let (column_index, y) = initial.unwrap_or((0, geometry.content_top));
    let mut cursor = FlowCursor {
        page_index: 0,
        column_index,
        y,
    };
    let markers = compute_list_markers(section, &document.lists, &document.styles);
    let fallback_styles = StyleSheet::default();
    for block in &section.blocks {
        check_cancel(cancel)?;
        let before = incremental::Boundary::capture(&pages, cursor);
        let marker = block_node_id(block).and_then(|id| markers.get(id));
        let width = geometry.width_of(cursor.column_index);
        let placed = preparation_cache::prepare(
            block,
            width,
            &document.styles,
            &document.resources,
            marker,
            fields,
        )
        .and_then(|block| {
            paginate_block(&[block], 0, &mut pages, &mut cursor, geometry, background)
        });
        if placed.is_ok() {
            continue;
        }
        // A table or paragraph may already have emitted part of a page.
        before.restore(&mut pages, &mut cursor);

        let mut paragraphs = Vec::new();
        model::visit_paragraphs(std::slice::from_ref(block), &mut paragraphs);
        if paragraphs.is_empty() {
            let width = width.min(72.0);
            let height = 18.0;
            ensure_vertical_space(&mut pages, &mut cursor, geometry, height, background);
            let x = geometry.column_left(cursor.column_index);
            let page = &mut pages[cursor.page_index];
            page.commands.extend(
                image_frame_commands(width, height)
                    .iter()
                    .map(|c| c.translated(x, cursor.y)),
            );
            if let Block::Equation(equation) = block {
                page.object_regions.push(ObjectHitRegion {
                    node_id: equation.id.0.clone(),
                    rect: Rect::new(x, cursor.y, width, height),
                    kind: ObjectKind::Equation { source: None },
                });
            }
            cursor.y += height;
        }
        for source in paragraphs {
            check_cancel(cancel)?;
            // One three-byte marker per inline object keeps the editor's byte
            // offsets (including the text following a broken object) intact.
            let mut plain = Paragraph::plain(model::inline_text(&source.content));
            plain.id = source.id.clone();
            let block = preparation_cache::prepare(
                &Block::Paragraph(plain),
                geometry.width_of(cursor.column_index),
                &fallback_styles,
                &document.resources,
                None,
                fields,
            )?;
            paginate_block(&[block], 0, &mut pages, &mut cursor, geometry, background)?;
        }
    }
    Ok((pages, cursor))
}
