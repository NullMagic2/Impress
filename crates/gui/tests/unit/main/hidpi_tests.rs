use super::*;

/// A click at the same logical position must hit the same character
/// whether the canvas is rendered at 1x, 2x or 3x physical pixels. GTK 3
/// window scales are whole numbers; fractional desktop scaling happens in
/// the compositor, outside the application.
#[test]
fn clicks_hit_the_same_text_at_every_pixel_scale() {
    let mut document = Document::default();
    document.sections[0].blocks.push(Block::Paragraph(Paragraph::plain("The quick brown fox jumps over the lazy dog.")));
    let pages = layout::LayoutEngine::default().layout_document(&document).unwrap();
    let mut hits = 0;
    for x in (100..700).step_by(23) {
        for y in (60..200).step_by(7) {
            let click = wxdragon::geometry::Point::new(x, y);
            let results: Vec<_> = [1.0_f32, 2.0, 3.0].iter().map(|&scale| {
                let mut viewport = ViewportState::default();
                size_viewport(&mut viewport, 900, 700, 96.0, scale);
                viewport.hit_test(&pages, logical_to_view(click, scale)).map(|hit| (hit.node_id, hit.byte_offset))
            }).collect();
            assert_eq!(results[0], results[1], "click ({x}, {y}) at 2x");
            assert_eq!(results[0], results[2], "click ({x}, {y}) at 3x");
            hits += usize::from(results[0].is_some());
        }
    }
    assert!(hits > 0, "the sample clicks must land on text");
}
