use super::*;
#[test]
fn moving_snaps_corresponding_edges_only_within_the_small_tolerance() {
    let margins = Rect::new(100.0, 80.0, 500.0, 700.0);
    let snap = moving(Rect::new(104.0, 83.0, 200.0, 100.0), margins, 6.0, true);
    assert_eq!(snap.rect, Rect::new(100.0, 80.0, 200.0, 100.0));
    assert_eq!(snap.x, Some(100.0));
    assert_eq!(snap.y, Some(80.0));
    let snap = moving(Rect::new(396.0, 677.0, 200.0, 100.0), margins, 6.0, true);
    assert_eq!(snap.rect.max_x(), 600.0);
    assert_eq!(snap.rect.max_y(), 780.0);
    let outside = Rect::new(107.0, 87.0, 200.0, 100.0);
    assert_eq!(moving(outside, margins, 6.0, true).rect, outside);
    assert_eq!(
        moving(Rect::new(104.0, 83.0, 200.0, 100.0), margins, 6.0, false)
            .rect
            .y,
        83.0
    );
}
#[test]
fn resizing_snaps_the_far_edges_and_preserves_shift_proportions() {
    let margins = Rect::new(100.0, 80.0, 500.0, 700.0);
    let rect = Rect::new(100.0, 80.0, 496.0, 695.0);
    let snap = resizing(rect, margins, 6.0, false);
    assert_eq!(snap.rect, margins);
    let snap = resizing(rect, margins, 6.0, true);
    assert_eq!(snap.rect.max_x(), 600.0);
    assert!((snap.rect.width / snap.rect.height - rect.width / rect.height).abs() < 0.0001);
    assert_eq!(snap.rect.x, rect.x);
    assert_eq!(snap.rect.y, rect.y);
    let outside = Rect::new(100.0, 80.0, 490.0, 690.0);
    assert_eq!(resizing(outside, margins, 6.0, false).rect, outside);
}
#[test]
fn ruler_targets_and_screen_tolerance_follow_scroll_zoom_and_display_scaling() {
    use render::{Size, VectorPage};
    let mut page = VectorPage::new(Size::new(600.0, 800.0));
    page.margin_rect = Some(Rect::new(60.0, 50.0, 480.0, 700.0));
    let doc = VectorDocument {
        pages: vec![page],
        ..Default::default()
    };
    for display in [1.0, 2.0] {
        for zoom in [0.5, 1.0, 2.0] {
            let mut viewport = ViewportState::default();
            viewport.dpi = 96.0 * display;
            viewport.zoom = zoom;
            viewport.set_viewport_px(800.0 * display, 600.0 * display);
            viewport.scroll_by(50.0 * display, 100.0 * display, &doc);
            let origin = viewport.page_rect_px(&doc, 0).unwrap();
            let m = margins(&doc, &viewport, Point::new(origin.x + 1.0, origin.y + 1.0)).unwrap();
            let point = viewport
                .page_point_to_view(&doc, 0, Point::new(60.0, 50.0))
                .unwrap();
            assert_eq!(m.x, point.x);
            assert_eq!(m.y, point.y);
            let rect = Rect::new(m.x + 5.0 * display, m.y + 7.0 * display, 100.0, 80.0);
            let snap = moving(rect, m, 6.0 * display, true);
            assert_eq!(snap.rect.x, m.x);
            assert_eq!(snap.rect.y, rect.y);
            assert_eq!(
                edge(m.max_x() - 4.0 * display, [m.x, m.max_x()], 6.0 * display),
                m.max_x()
            );
        }
    }
}
