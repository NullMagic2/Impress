use super::*;

#[test]
fn english_has_internal_breaks() {
    let points = hyphenation_points("extensive", Some("en-US"));
    assert!(!points.is_empty());
    assert!(points.iter().all(|&p| p > 1 && p < "extensive".len() - 1));
}

