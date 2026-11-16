use super::*;

#[test]
fn l2_matches_unicode_bidi_reference_example() {
    assert_eq!(
        visual_order(&[0, 0, 0, 1, 1, 1, 2, 2]),
        vec![0, 1, 2, 6, 7, 5, 4, 3],
    );
}
