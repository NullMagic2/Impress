use super::*;
#[test]
fn snapped_preview_is_the_move_and_resize_that_will_be_committed() {
    let mut drag = TableDrag {
        table: "t".into(),
        resize: false,
        start: Point::new(100.0, 80.0),
        current: Point::new(104.0, 80.0),
        moved: true,
        bounds: (100.0, 80.0, 300.0, 180.0),
        indent_mm: 0.0,
        widths_pt: vec![100.0, 100.0],
        heights_pt: vec![100.0],
        declared: vec![None],
        horizontal_only: true,
        proportional: false,
        free: None,
        snapped: None,
    };
    drag.snapped = Some(crate::table_snap::moving(
        drag.ghost(),
        Rect::new(100.0, 80.0, 500.0, 700.0),
        6.0,
        false,
    ));
    assert_eq!(new_indent(&drag, 1.0), 0.0);
    assert_eq!(drag.ghost().x, 100.0);
    drag.resize = true;
    drag.snapped = Some(crate::table_snap::Snapped {
        rect: Rect::new(100.0, 80.0, 500.0, 150.0),
        x: Some(600.0),
        y: None,
        margins: Rect::new(100.0, 80.0, 500.0, 700.0),
    });
    assert_eq!(drag.factors(), (2.5, 1.5));
    assert_eq!(drag.ghost().width, 500.0);
}
