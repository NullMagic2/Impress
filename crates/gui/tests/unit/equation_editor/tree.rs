use super::*;

fn typed(text: &str) -> Editor {
    let mut editor = Editor::default();
    for c in text.chars() { editor.type_char(c); }
    editor
}
fn typst(editor: &Editor) -> String { format_typst::node_to_typst(&editor.to_node().unwrap()) }

#[test]
fn typing_builds_structures() {
    assert_eq!(typst(&typed("x^2")), "x^2");
    assert_eq!(typst(&typed("a/b")), "a / b");
    assert_eq!(typst(&typed("x_i^2")), "x_i^2");
    assert_eq!(typst(&typed("12+3")), "12 + 3");
    assert_eq!(typst(&typed("a<=b")), "a ≤ b");
    assert_eq!(typst(&typed("x->y")), "x → y");
    assert_eq!(typst(&typed("sin x")), "sin x");
}

#[test]
fn document_equations_round_trip() {
    let b = |n| Box::new(n);
    let nodes = [
        N::Superscript { base: b(N::ident("x")), exponent: b(N::number("2")) },
        N::Superscript { base: b(N::row([N::ident("a"), N::op("+"), N::ident("b")])), exponent: b(N::number("10")) },
        N::Fraction { numerator: b(N::row([N::ident("x"), N::op("+"), N::number("1")])), denominator: b(N::Radical { radicand: b(N::ident("x")), index: Some(b(N::number("3"))) }) },
        N::Nary { operator: "sum".into(), body: b(N::ident("k")), subscript: Some(b(N::row([N::ident("k"), N::Relation { value: "=".into() }, N::number("1")]))), superscript: None },
        N::Matrix { rows: vec![vec![N::number("1"), N::number("0")], vec![N::number("0"), N::number("1")]] },
        N::Text { value: "if x".into() },
        N::Bar { body: b(N::ident("z")), position: model::VerticalPosition::Over },
    ];
    for node in nodes {
        assert_eq!(Editor::from_node(&node).to_node().unwrap(), node);
    }
}

#[test]
fn backspace_reaches_nested_previous_content_and_removes_empty_fraction() {
    let mut editor = typed("a/b/c");
    editor.set_cursor(Vec::new(), editor.root.len());
    editor.backspace();
    assert_eq!(editor.cursor.path, vec![(0, 1), (0, 1)]);
    editor.backspace();
    editor.backspace();
    assert_eq!(editor.cursor.path, vec![(0, 1), (0, 0)]);
    let mut empty = Editor::default();
    empty.insert_node(Kind::Fraction);
    empty.next_slot(false);
    empty.backspace();
    assert!(empty.root.is_empty());
    assert!(empty.cursor.path.is_empty());
}

#[test]
fn backspace_deletes_selected_text_before_a_pending_command() {
    let mut editor = typed("x\\alpha");
    editor.cursor.anchor = Some(0);
    editor.backspace();
    assert!(editor.root.is_empty());
    assert_eq!(editor.cursor.offset, 0);
}

#[test]
fn empty_page_slot_can_move_to_previous_slot_then_leave_equation() {
    let fraction = N::Fraction {
        numerator: Box::new(N::ident("a")),
        denominator: Box::new(N::Symbol { value: PLACEHOLDER.into() }),
    };
    let denominator = vec![(0, 1)];
    let numerator = vec![(0, 0)];
    assert!(super::super::slots::is_empty(&fraction, &denominator));
    assert_eq!(super::super::slots::previous(&fraction, &denominator), Some(numerator.clone()));
    assert_eq!(super::super::slots::previous(&fraction, &numerator), None);
}

#[test]
fn palette_operators_leave_deactivated_fraction_slots_but_keep_active_slots() {
    let fraction = N::Fraction {
        numerator: Box::new(N::number("3")),
        denominator: Box::new(N::number("4")),
    };
    for slot in [0, 1] {
        for entry in &super::super::palette::tabs()[2].entries {
            let super::super::palette::Insert::Atom(kind, text) = &entry.insert else { unreachable!() };
            let mut inactive = Editor::from_node(&fraction);
            inactive.select(vec![(0, slot)], 0, 1);
            inactive.deactivate_slot();
            assert!(inactive.cursor.path.is_empty());
            assert_eq!(inactive.selection(), None);
            assert_eq!(inactive.to_node().unwrap(), fraction);
            inactive.insert_atom(Atom::new(*kind, text.clone()));
            assert_eq!(inactive.to_node().unwrap(), N::row([fraction.clone(), N::op(text)]));

            let mut active = Editor::from_node(&fraction);
            active.set_cursor(vec![(0, slot)], 1);
            active.insert_atom(Atom::new(*kind, text.clone()));
            assert_eq!(active.cursor.path, vec![(0, slot)]);
            assert_eq!(active.root.len(), 1);
            assert_eq!(active.current_row().len(), 2);
        }
    }
}

#[test]
fn deactivated_nested_fraction_returns_only_to_its_enclosing_level() {
    let mut editor = typed("a/b/c");
    assert_eq!(editor.cursor.path, vec![(0, 1), (0, 1)]);
    editor.deactivate_slot();
    assert_eq!(editor.cursor.path, vec![(0, 1)]);
    editor.insert_atom(Atom::new(AtomKind::Operator, "±"));
    assert_eq!(typst(&editor), "a / (b / c ±)");
}
