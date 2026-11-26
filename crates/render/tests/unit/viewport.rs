use super::*;
use crate::{Size, VectorPage};

fn two_page_document() -> VectorDocument {
    VectorDocument {
        pages: vec![
            VectorPage::new(Size::new(595.0, 842.0)),
            VectorPage::new(Size::new(595.0, 842.0)),
        ], ..Default::default()
    }
}

#[test]
fn blank_body_clicks_never_select_nearby_header_or_footer_text() {
    use crate::{ObjectHitRegion, ObjectKind, TextHitRegion};
    let mut document = two_page_document();
    let page = &mut document.pages[0];
    for (id, y) in [("header", 35.0), ("body", 400.0), ("footer", 805.0)] {
        page.hit_regions.push(TextHitRegion { node_id: id.into(), visual_line: 0,
            byte_range: 0..6, rect: Rect::new(60.0, y, 80.0, 12.0), vertical: false, caret_stops: vec![] });
    }
    for (header, y, height) in [(true, 0.0, 70.0), (false, 780.0, 62.0)] {
        page.object_regions.push(ObjectHitRegion { node_id: "section".into(),
            rect: Rect::new(50.0, y, 495.0, height),
            kind: ObjectKind::HeaderFooter { section_id: "section".into(), header, variant: 0 } });
    }
    for dpi in [96.0, 192.0] { for zoom in [0.5, 1.0, 2.0] {
        let mut viewport = ViewportState { dpi, zoom, ..Default::default() };
        viewport.set_viewport_px(2000.0, 2400.0);
        let at = |x, y| viewport.page_point_to_view(&document, 0, Point::new(x, y)).unwrap();
        for y in [71.0, 200.0, 650.0, 779.0] {
            let point = at(250.0, y);
            assert_eq!(viewport.hit_test(&document, point).unwrap().node_id, "body");
            assert_eq!(viewport.hit_test_character(&document, point).unwrap().node_id, "body");
        }
        assert_eq!(viewport.hit_test(&document, at(250.0, 40.0)).unwrap().node_id, "header");
        assert_eq!(viewport.hit_test(&document, at(250.0, 810.0)).unwrap().node_id, "footer");
    } }
    // A table-only body can have no eligible text outside its cells. That
    // empty space must remain empty rather than borrowing a footer caret.
    document.pages[0].hit_regions.retain(|r| r.node_id != "body");
    let viewport = ViewportState::default();
    let point = viewport.page_point_to_view(&document, 0, Point::new(250.0, 760.0)).unwrap();
    assert!(viewport.hit_test(&document, point).is_none());
    assert!(viewport.hit_test_character(&document, point).is_none());
}

#[test]
fn a_click_in_a_cells_empty_space_hits_that_cells_text() {
    use crate::{ObjectHitRegion, ObjectKind, TableCellHit, TextHitRegion};
    let mut document = two_page_document();
    let line = |id: &str, rect: Rect| TextHitRegion { node_id: id.into(), visual_line: 0, byte_range: 0..1, rect, vertical: false, caret_stops: Vec::new() };
    let page = &mut document.pages[0];
    // A wide paragraph line just above the table, and short text in the cell.
    page.hit_regions.push(line("above", Rect::new(50.0, 90.0, 400.0, 12.0)));
    page.hit_regions.push(line("cell", Rect::new(52.0, 106.0, 20.0, 12.0)));
    page.object_regions.push(ObjectHitRegion { node_id: "c".into(), rect: Rect::new(50.0, 104.0, 150.0, 40.0),
        kind: ObjectKind::TableCell(TableCellHit { table_id: "t".into(), row_index: 0, cell_index: 0, column_index: 0, column_span: 1,
            row_span: 1, column_widths_pt: vec![150.0], row_height_pt: 40.0, can_resize_row: true }) });
    let viewport = ViewportState::default();
    let origin = viewport.page_rect_px(&document, 0).unwrap();
    let scale = viewport.points_to_pixels();
    let at = |x: f32, y: f32| Point::new(origin.x + x * scale, origin.y + y * scale);
    assert_eq!(viewport.hit_test(&document, at(180.0, 110.0)).unwrap().node_id, "cell");
    assert_eq!(viewport.hit_test(&document, at(180.0, 95.0)).unwrap().node_id, "above");
    document.pages[0].hit_regions.retain(|region| region.node_id == "cell");
    assert!(viewport.hit_test(&document, at(300.0, 300.0)).is_none());
    assert_eq!(viewport.hit_test_nearest(&document, at(300.0, 300.0)).unwrap().node_id, "cell");
}

#[test]
fn anchored_zoom_keeps_world_position_stable_before_clamping() {
    let document = two_page_document();
    let mut viewport = ViewportState::default();
    viewport.set_viewport_px(320.0, 240.0);
    viewport.scroll_px = Point::new(120.0, 220.0);
    let anchor = Point::new(100.0, 80.0);
    let old_scale = viewport.points_to_pixels();
    let world = Point::new(
        (viewport.scroll_px.x + anchor.x - viewport.horizontal_origin_px(&document)) / old_scale,
        (viewport.scroll_px.y + anchor.y - viewport.outer_margin_px) / old_scale,
    );
    viewport.zoom_at(anchor, 1.5, &document);
    let new_scale = viewport.points_to_pixels();
    let world_after = Point::new(
        (viewport.scroll_px.x + anchor.x - viewport.horizontal_origin_px(&document)) / new_scale,
        (viewport.scroll_px.y + anchor.y - viewport.outer_margin_px) / new_scale,
    );
    assert!((world.x - world_after.x).abs() < 0.01);
    assert!((world.y - world_after.y).abs() < 0.01);
}

#[test]
fn invalid_zoom_cannot_poison_viewport_coordinates() {
    let document = two_page_document(); let mut viewport = ViewportState::default();
    let before = viewport;
    viewport.zoom_at(Point::new(100.0,100.0), f32::NAN, &document);
    assert_eq!(viewport, before);
}

#[test]
fn past_the_end_of_a_paragraphs_short_last_line_the_pointer_stays_on_that_line() {
    use crate::TextHitRegion;
    let mut document = two_page_document();
    let line = |visual_line: usize, rect: Rect, bytes: std::ops::Range<usize>| TextHitRegion { node_id: "p".into(), visual_line, byte_range: bytes, rect, vertical: false, caret_stops: Vec::new() };
    let page = &mut document.pages[0];
    // A long line, then the paragraph's short last line just below it.
    page.hit_regions.push(line(0, Rect::new(50.0, 100.0, 400.0, 12.0), 0..80));
    page.hit_regions.push(line(1, Rect::new(50.0, 114.0, 120.0, 12.0), 80..100));
    let viewport = ViewportState::default();
    let origin = viewport.page_rect_px(&document, 0).unwrap();
    let scale = viewport.points_to_pixels();
    let at = |x: f32, y: f32| Point::new(origin.x + x * scale, origin.y + y * scale);
    for x in [60.0, 200.0, 300.0, 440.0, 560.0] {
        assert_eq!(viewport.hit_test(&document, at(x, 120.0)).unwrap().visual_line, 1, "click at x {x}");
        assert_eq!(viewport.hit_test_nearest(&document, at(x, 120.0)).unwrap().visual_line, 1, "drag at x {x}");
    }
    // The long line is still hit on its own height, and between the lines the nearer one wins.
    assert_eq!(viewport.hit_test(&document, at(300.0, 105.0)).unwrap().visual_line, 0);
    assert_eq!(viewport.hit_test(&document, at(300.0, 113.4)).unwrap().visual_line, 1);
}

