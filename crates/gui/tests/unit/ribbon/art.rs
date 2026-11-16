use super::*;
#[test]
fn alpha_resampling_does_not_dark_fringe() {
    let rgba = resize_rgba(&[0, 0, 0, 0, 255, 0, 0, 255], 2, 1, 1, 1);
    assert_eq!(&rgba[..3], &[255, 0, 0]);
    assert!((i32::from(rgba[3]) - 128).abs() <= 1);
}
