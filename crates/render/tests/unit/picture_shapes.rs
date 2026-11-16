use super::*;

#[test]
fn all_standard_presets_evaluate_at_square_and_wide_picture_sizes() {
    assert_eq!(preset_names().count(), 187);
    for name in preset_names() {
        for (width, height) in [(100.0, 100.0), (321.0, 123.0), (71.0, 253.0)] {
            let preset = presets::PRESETS.iter().find(|p| p.name == name).unwrap();
            let mut values = guides::Values::new(f64::from(width), f64::from(height));
            for (guide, formula) in preset.adjustments.iter().chain(preset.guides) {
                assert!(values.assign(guide, formula).is_some(), "{name}: {guide} = {formula}");
            }
            let paths = geometry(name, &BTreeMap::new(), width, height).unwrap_or_else(|| panic!("{name} at {width} x {height}"));
            assert!(!paths.is_empty(), "{name}");
            for path in paths {
                for verb in path.path.verbs {
                    let points = match verb {
                        PathVerb::MoveTo(p) | PathVerb::LineTo(p) => vec![p],
                        PathVerb::QuadTo { ctrl, to } => vec![ctrl, to],
                        PathVerb::CubicTo { ctrl1, ctrl2, to } => vec![ctrl1, ctrl2, to],
                        PathVerb::Close => Vec::new(),
                    };
                    assert!(points.iter().all(|p| p.x.is_finite() && p.y.is_finite()), "{name}");
                }
            }
        }
    }
}

#[test]
fn ellipse_and_donut_masks_preserve_transparency_and_holes() {
    let mask = |name| crop_mask(&geometry(name, &BTreeMap::new(), 100.0, 100.0).unwrap(), 100, 100, 100.0, 100.0).unwrap();
    let ellipse = mask("ellipse");
    assert_eq!(ellipse[0], 0);
    assert_eq!(ellipse[50 * 100 + 50], 255);
    assert!(ellipse.iter().any(|a| *a > 0 && *a < 255));
    let donut = mask("donut");
    assert_eq!(donut[50 * 100 + 50], 0);
    assert_eq!(donut[50 * 100 + 10], 255);
}

#[test]
fn adjustment_values_change_the_actual_crop_geometry() {
    let defaults = geometry("roundRect", &BTreeMap::new(), 100.0, 100.0).unwrap();
    let adjusted = geometry("roundRect", &BTreeMap::from([("adj".into(), "val 50000".into())]), 100.0, 100.0).unwrap();
    assert_ne!(defaults[0].path, adjusted[0].path);
    assert!(geometry("ellipse", &BTreeMap::new(), f32::NAN, 100.0).is_none());
    assert!(geometry("notARealShape", &BTreeMap::new(), 100.0, 100.0).is_none());
}
