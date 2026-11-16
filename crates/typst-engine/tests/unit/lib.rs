use super::*;
use model::{Equation, EquationNode as N};

#[test]
fn nested_fractions_widen_the_main_bar() {
    let b = |n| Box::new(n);
    let frac = |n: &str, d: &str| N::Fraction { numerator: b(N::ident(n)), denominator: b(N::ident(d)) };
    let nested = Equation::block(N::Fraction { numerator: b(frac("a", "b")), denominator: b(frac("c", "d")) });
    let typesetter = TypstTypesetter::new().with_system_fonts(false);
    let width = |eq: &Equation| typesetter.render_equation_vector(eq, 11.0).expect("renders").size.width;
    let inner = width(&Equation::block(frac("a", "b")));
    assert!(width(&nested) > inner + 11.0 * 0.4, "the main bar overhangs the inner ones");
    // Inline fractions are as tall as display ones: full-size parts.
    let height = |eq: &Equation| typesetter.render_equation_vector(eq, 11.0).expect("renders").size.height;
    // Raw Typst is drawn as written, with Typst's own inline sizes.
    let typst_inline = height(&Equation::inline(N::RawTypst { source: "x / 2".into() }));
    let readable = height(&Equation::inline(frac("x", "2")));
    assert!(readable > typst_inline * 1.15, "readable {readable} vs Typst's inline {typst_inline}");
    // Plain fractions are untouched.
    assert!(typesetter.render_equation_vector(&Equation::inline(frac("x", "2")), 11.0).is_ok());
}

#[test]
fn inline_fraction_baseline_sits_between_numerator_and_denominator() {
    const FONT_SIZES_PT: [f32; 3] = [9.0, 11.0, 24.0];
    const BASELINE_TOLERANCE_PT: f32 = 0.02;
    let fraction = N::Fraction {
        numerator: Box::new(N::number("3")),
        denominator: Box::new(N::number("2")),
    };
    let typesetter = TypstTypesetter::new().with_system_fonts(false);
    let baselines = |fragment: &VectorFragment| -> Vec<f32> {
        fragment.commands.iter().filter_map(|command| {
            let DrawCommand::TextRun(run) = command else { return None; };
            run.glyphs.first().map(|glyph| glyph.transform.ty)
        }).collect()
    };
    for size in FONT_SIZES_PT {
        let fragment = typesetter.render_equation_vector(&Equation::inline(fraction.clone()), size).unwrap();
        let positions = baselines(&fragment);
        assert_eq!(positions.len(), 2);
        assert!(positions[0] < fragment.baseline_pt && fragment.baseline_pt < positions[1],
            "numerator {}, baseline {}, denominator {}", positions[0], fragment.baseline_pt, positions[1]);
        let with_operator = Equation::inline(N::row([fraction.clone(), N::op("+")]));
        let full = typesetter.render_equation_vector(&with_operator, size).unwrap();
        let full_positions = baselines(&full);
        let operator_baseline = *full_positions.last().unwrap();
        assert!((operator_baseline - full.baseline_pt).abs() < BASELINE_TOLERANCE_PT);
        let denominator_offset = positions[1] - fragment.baseline_pt;
        assert!((denominator_offset - (full_positions[1] - operator_baseline)).abs() < BASELINE_TOLERANCE_PT);
    }
}
