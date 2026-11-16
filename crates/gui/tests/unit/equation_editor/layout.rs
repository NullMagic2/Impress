//! Click targets must not trap the caret inside a fraction.
use super::*;

fn fraction_geometry() -> MathBox {
    MathBox {
        w: 100.0, asc: 30.0, desc: 30.0,
        rows: vec![
            RowGeometry { path: vec![(1, 0)], rect: Rect { x: 43.0, y: -25.0, w: 14.0, h: 12.0 }, carets: vec![43.0, 57.0], caret_y: (-25.0, -13.0) },
            RowGeometry { path: vec![(1, 1)], rect: Rect { x: 43.0, y: 18.0, w: 14.0, h: 12.0 }, carets: vec![43.0, 57.0], caret_y: (18.0, 30.0) },
            RowGeometry { path: vec![], rect: Rect { x: 0.0, y: -30.0, w: 100.0, h: 60.0 }, carets: vec![0.0, 40.0, 60.0, 100.0], caret_y: (-20.0, 8.0) },
        ], ..Default::default()
    }
}

#[test]
fn clicks_outside_fraction_choose_outer_row_and_inside_keeps_slots() {
    let formula = fraction_geometry();
    assert_eq!(formula.hit(62.0, 24.0), Some((vec![], 2)));
    assert_eq!(formula.hit(50.0, 70.0), Some((vec![], 1)));
    assert_eq!(formula.hit(50.0, 24.0), Some((vec![(1, 1)], 0)));
    assert_eq!(formula.hit(50.0, -20.0), Some((vec![(1, 0)], 0)));
}
