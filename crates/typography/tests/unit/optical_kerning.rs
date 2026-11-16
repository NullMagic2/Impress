use super::*;
fn rectangle(x0:f32, x1:f32, y0:f32, y1:f32)->Outline {
    let mut o=Outline::default();o.move_to(x0,y0);o.line_to(x1,y0);o.line_to(x1,y1);o.line_to(x0,y1);o.close();o
}
#[test] fn close_shapes_are_not_forced_to_overlap() {
    let stem=rectangle(0.0,500.0,0.0,700.0);
    let delta=pair_adjustment(&stem,&stem,5.1,0.0,0.0,0.01,10.0).unwrap();
    assert!(delta >= 0.0);
}
