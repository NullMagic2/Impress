use super::*;
use crate::equation::node_to_typst;

fn parse(s: &str) -> N { parse_math_source(s).unwrap_or_else(|e| panic!("{s}: {e}")) }

#[test]
fn fractions_stack() {
    assert!(matches!(parse("a/b"), N::Fraction { .. }));
    let N::Fraction { numerator, .. } = parse("(x+1)/(x-1)") else { panic!() };
    assert!(matches!(*numerator, N::Row { .. }), "the parentheses only group");
    assert!(matches!(parse("frac(1, 2)"), N::Fraction { .. }));
    let nested = parse("(a/b)/(c/d)");
    let N::Fraction { numerator, denominator } = nested else { panic!() };
    assert!(matches!(*numerator, N::Fraction { .. }) && matches!(*denominator, N::Fraction { .. }));
}

#[test]
fn symbols_and_large_operators() {
    assert_eq!(parse("alpha"), N::Identifier { value: "α".into() });
    assert_eq!(parse("arrow.r"), N::Relation { value: "→".into() });
    assert_eq!(parse("a <= b"), N::row([N::ident("a"), N::Relation { value: "≤".into() }, N::ident("b")]));
    let N::Nary { operator, body, subscript, superscript } = parse("integral_0^1 x dif x") else { panic!() };
    assert_eq!(operator, "integral");
    assert!(subscript.is_some() && superscript.is_some());
    assert!(matches!(*body, N::Row { .. }));
    assert!(matches!(parse("integral 4"), N::Nary { .. }));
    assert!(matches!(parse("sum_(k=1)^n k = 1"), N::Row { .. }), "the relation ends the body");
    assert_eq!(parse("sin x"), N::row([N::ident("sin"), N::ident("x")]));
}

#[test]
fn code_and_unknown_functions_are_refused() {
    for s in ["#read(\"x\")", "read(\"x\")", "x + #{1}", "abc", "frac(a,"] {
        assert!(parse_math_source(s).is_err(), "{s}");
    }
    assert!(is_safe_raw("binom(n, k)"));
    assert!(!is_safe_raw("#read(\"secret\")"));
}

#[test]
fn serialized_equations_parse_back() {
    let b = |n| Box::new(n);
    let nodes = [
        N::Fraction { numerator: b(N::row([N::ident("x"), N::op("+"), N::number("1")])), denominator: b(N::Radical { radicand: b(N::ident("x")), index: Some(b(N::number("3"))) }) },
        N::Superscript { base: b(N::ident("x")), exponent: b(N::number("2")) },
        N::Superscript { base: b(N::row([N::ident("a"), N::op("+"), N::ident("b")])), exponent: b(N::number("10")) },
        N::Scripts { base: b(N::ident("x")), subscript: Some(b(N::ident("i"))), superscript: Some(b(N::number("2"))) },
        N::Matrix { rows: vec![vec![N::number("1"), N::number("0")], vec![N::number("0"), N::number("1")]] },
        N::Text { value: "if x".into() },
        N::Identifier { value: "abc".into() },
        N::Accent { accent: "hat".into(), body: b(N::ident("x")) },
        N::Delimited { left: "(".into(), body: b(N::row([N::ident("a"), N::op("+"), N::ident("b")])), right: ")".into() },
        N::Nary { operator: "sum".into(), body: b(N::ident("k")), subscript: Some(b(N::ident("k"))), superscript: Some(b(N::ident("n"))) },
        N::Boxed { body: b(N::ident("x")) },
    ];
    for node in nodes {
        let source = node_to_typst(&node);
        assert_eq!(parse(&source), node, "{source}");
    }
}
