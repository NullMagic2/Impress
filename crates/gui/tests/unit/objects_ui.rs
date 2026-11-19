use super::*;
#[test]fn resize_preserves_ratio_and_opposite_corner(){
    let r=Rect::new(10.0,20.0,80.0,40.0);let n=resized(r,Point::new(170.0,100.0),Handle::BottomRight,false);
    assert!((n.width/n.height-2.0).abs()<0.001);assert_eq!(n.x,r.x);assert_eq!(n.y,r.y);
    let n=resized(r,Point::new(-70.0,-20.0),Handle::TopLeft,false);assert!((n.x+n.width-90.0).abs()<0.001);assert!((n.y+n.height-60.0).abs()<0.001);
}

#[test]
fn cursor_distinguishes_paper_from_workspace_and_page_gaps() {
    use render::{Size, VectorDocument, VectorPage, ViewportState};

    let document = VectorDocument {
        pages: vec![
            VectorPage::new(Size::new(420.0, 300.0)),
            VectorPage::new(Size::new(320.0, 400.0)),
        ],
        ..Default::default()
    };
    let mut viewport = ViewportState { right_inset_px: 120.0, ..Default::default() };
    viewport.zoom_at(Point::new(100.0, 100.0), 0.85, &document);
    viewport.scroll_by(0.0, 180.0, &document);
    let rects = viewport.page_rects_px(&document);
    let [first, second] = rects.as_slice() else { panic!("expected two pages"); };
    let inside = Point::new(first.x + 20.0, first.max_y() - 1.0);
    let workspace = Point::new(first.max_x() + 6.0, inside.y);
    let gap = Point::new(first.x + 20.0, (first.max_y() + second.y) / 2.0);
    let outside_narrower_page = Point::new(second.x - 6.0, second.y + 20.0);

    assert!(paper_pointer(&viewport, &document, inside) == Pointer::Text);
    for point in [workspace, gap, outside_narrower_page] {
        assert!(paper_pointer(&viewport, &document, point) == Pointer::Arrow);
    }
}
