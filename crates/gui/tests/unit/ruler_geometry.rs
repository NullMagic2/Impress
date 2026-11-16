use super::*;
use render::{Size, VectorPage};

fn document() -> VectorDocument {
    let mut first = VectorPage::new(Size::new(600.0, 800.0));
    first.margin_rect = Some(Rect::new(70.0, 60.0, 460.0, 670.0));
    let mut second = VectorPage::new(Size::new(800.0, 600.0));
    second.margin_rect = Some(Rect::new(40.0, 30.0, 700.0, 520.0));
    VectorDocument {
        pages: vec![first, second],
        ..Default::default()
    }
}

#[test]
fn ruler_margins_match_canvas_coordinates_across_zoom_dpi_and_scroll() {
    let doc = document();
    for screen in [1.0, 2.0] {
        for dpi in [96.0, 144.0] {
            for zoom in [0.5, 1.0, 2.0] {
                let mut viewport = ViewportState::default();
                viewport.dpi = dpi * screen;
                viewport.zoom = zoom;
                viewport.set_viewport_px(1200.0 * screen, 900.0 * screen);
                viewport.scroll_by(200.0 * screen, 300.0 * screen, &doc);
                let offset = Point::new(34.0, 7.0);
                let rulers = RulerPages::new(&doc, &viewport, screen, offset).unwrap();
                for page in &rulers.pages {
                    let margin = doc.pages[page.index].margin_rect.unwrap();
                    let point = viewport
                        .page_point_to_view(&doc, page.index, Point::new(margin.x, margin.y))
                        .unwrap();
                    assert!((page.margins.x - (offset.x + point.x / screen)).abs() < 0.001);
                    assert!((page.margins.y - (offset.y + point.y / screen)).abs() < 0.001);
                    let right = viewport
                        .page_point_to_view(
                            &doc,
                            page.index,
                            Point::new(margin.max_x(), margin.max_y()),
                        )
                        .unwrap();
                    assert!((page.margins.max_x() - (offset.x + right.x / screen)).abs() < 0.001);
                    assert!((page.margins.max_y() - (offset.y + right.y / screen)).abs() < 0.001);
                }
            }
        }
    }
}

#[test]
fn scrolling_uses_visible_sheet_with_its_own_size_and_margins() {
    let doc = document();
    let mut viewport = ViewportState::default();
    viewport.set_viewport_px(1200.0, 700.0);
    viewport.scroll_by(0.0, 1120.0, &doc);
    let rulers = RulerPages::new(&doc, &viewport, 1.0, Point::new(34.0, 0.0)).unwrap();
    assert_eq!(rulers.primary, 1);
    assert_eq!(rulers.pages.len(), 1);
    let page = rulers.primary();
    assert!((page.margins.x - page.paper.x - 40.0 * rulers.pixels_per_point).abs() < 0.001);
    assert!((page.margins.y - page.paper.y - 30.0 * rulers.pixels_per_point).abs() < 0.001);
    // Scale increases downward and restarts at this sheet, independent of any caret.
    assert!((page.margins.y - 48.0).abs() < 0.1);
    assert!(page.margins.max_y() > page.margins.y);
}

#[test]
fn two_visible_sheets_have_separate_vertical_origins_and_a_gap() {
    let doc = document();
    let mut viewport = ViewportState::default();
    viewport.set_viewport_px(1200.0, 700.0);
    viewport.scroll_by(0.0, 750.0, &doc);
    let rulers = RulerPages::new(&doc, &viewport, 1.0, Point::new(34.0, 0.0)).unwrap();
    assert_eq!(rulers.pages.len(), 2);
    assert_eq!(rulers.primary, 1);
    let a = &rulers.pages[0];
    let b = &rulers.pages[1];
    assert!(a.paper.max_y() < b.paper.y);
    assert!(a.margins.y < 0.0 && b.margins.y > 0.0);
    assert!(
        (b.paper.y - a.paper.max_y() - viewport.page_gap_pt * viewport.points_to_pixels()).abs()
            < 1.0
    );
}
