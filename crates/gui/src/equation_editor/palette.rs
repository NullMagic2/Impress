//! Palette contents and the `\name` commands that type the same things.
use super::tree::{AtomKind, Kind};
use std::sync::OnceLock;

#[derive(Clone, Debug, PartialEq)]
pub enum Insert {
    Atom(AtomKind, String),
    Node(Kind),
}

pub struct Entry {
    pub name: &'static str,
    /// How to type it without the palette.
    pub shortcut: String,
    pub insert: Insert,
}

pub struct Tab {
    pub title: &'static str,
    /// Tab face, drawn in the math font.
    pub face: &'static str,
    pub entries: Vec<Entry>,
}

fn node(kind: Kind) -> Insert { Insert::Node(kind) }
fn delimited(left: &str, right: &str) -> Kind { Kind::Delimited { left: left.into(), right: right.into() } }

/// (glyph, command name, kind)
const GREEK: &[(&str, &str)] = &[
    ("α", "alpha"), ("β", "beta"), ("γ", "gamma"), ("δ", "delta"), ("ε", "epsilon"), ("ζ", "zeta"), ("η", "eta"),
    ("θ", "theta"), ("ι", "iota"), ("κ", "kappa"), ("λ", "lambda"), ("μ", "mu"), ("ν", "nu"), ("ξ", "xi"),
    ("π", "pi"), ("ρ", "rho"), ("σ", "sigma"), ("τ", "tau"), ("υ", "upsilon"), ("φ", "phi"), ("χ", "chi"),
    ("ψ", "psi"), ("ω", "omega"), ("ϵ", "varepsilon"), ("ϑ", "vartheta"), ("ϕ", "varphi"), ("ϱ", "varrho"), ("ς", "varsigma"),
    ("Γ", "Gamma"), ("Δ", "Delta"), ("Θ", "Theta"), ("Λ", "Lambda"), ("Ξ", "Xi"), ("Π", "Pi"), ("Σ", "Sigma"),
    ("Υ", "Upsilon"), ("Φ", "Phi"), ("Ψ", "Psi"), ("Ω", "Omega"),
];
const OPERATORS: &[(&str, &str)] = &[
    ("+", "plus"), ("−", "minus"), ("±", "pm"), ("∓", "mp"), ("×", "times"), ("·", "cdot"), ("÷", "div"),
    ("∘", "circ"), ("∗", "ast"), ("⊕", "oplus"), ("⊗", "otimes"), ("∪", "cup"), ("∩", "cap"), ("∖", "setminus"),
    ("∧", "wedge"), ("∨", "vee"), ("¬", "neg"), ("∇", "nabla"), ("∂", "partial"), ("′", "prime"), ("!", "factorial"),
];
const RELATIONS: &[(&str, &str)] = &[
    ("=", "eq"), ("≠", "neq"), ("<", "lt"), (">", "gt"), ("≤", "leq"), ("≥", "geq"), ("≈", "approx"),
    ("≡", "equiv"), ("∼", "sim"), ("≃", "simeq"), ("≅", "cong"), ("∝", "propto"), ("≪", "ll"), ("≫", "gg"),
    ("∈", "in"), ("∉", "notin"), ("∋", "ni"), ("⊂", "subset"), ("⊃", "supset"), ("⊆", "subseteq"), ("⊇", "supseteq"),
    ("⊥", "perp"), ("∥", "parallel"), ("∣", "mid"),
];
const ARROWS: &[(&str, &str)] = &[
    ("→", "to"), ("←", "leftarrow"), ("↔", "leftrightarrow"), ("⇒", "implies"), ("⇐", "impliedby"), ("⇔", "iff"),
    ("↦", "mapsto"), ("↑", "uparrow"), ("↓", "downarrow"), ("⟶", "longrightarrow"), ("⟹", "Longrightarrow"),
    ("↗", "nearrow"), ("↘", "searrow"), ("⇌", "rightleftharpoons"),
];
const MISC: &[(&str, &str)] = &[
    ("∞", "infty"), ("∅", "emptyset"), ("∀", "forall"), ("∃", "exists"), ("∄", "nexists"), ("ℝ", "R"), ("ℕ", "N"),
    ("ℤ", "Z"), ("ℚ", "Q"), ("ℂ", "C"), ("ℓ", "ell"), ("ℏ", "hbar"), ("…", "ldots"), ("⋯", "cdots"), ("⋮", "vdots"),
    ("⋱", "ddots"), ("°", "degree"), ("∠", "angle"), ("△", "triangle"), ("∴", "therefore"), ("∵", "because"),
];

fn symbols(list: &[(&'static str, &'static str)], kind: AtomKind) -> Vec<Entry> {
    list.iter().map(|(glyph, name)| Entry { name, shortcut: format!("\\{name}"), insert: Insert::Atom(kind, (*glyph).into()) }).collect()
}

pub fn tabs() -> &'static [Tab] {
    static TABS: OnceLock<Vec<Tab>> = OnceLock::new();
    TABS.get_or_init(|| {
        let e = |name, shortcut: &str, insert| Entry { name, shortcut: shortcut.into(), insert };
        let structures = vec![
            e("Fraction", "type /", node(Kind::Fraction)),
            e("Square root", "\\sqrt", node(Kind::Sqrt)),
            e("Root", "\\root", node(Kind::Root)),
            e("Superscript", "type ^", node(Kind::Sup)),
            e("Subscript", "type _", node(Kind::Sub)),
            e("Sub- and superscript", "type _ then ^", node(Kind::SubSup)),
            e("Parentheses", "type (", node(delimited("(", ")"))),
            e("Brackets", "type [", node(delimited("[", "]"))),
            e("Braces", "type {", node(delimited("{", "}"))),
            e("Absolute value", "type |", node(delimited("|", "|"))),
            e("Norm", "\\norm", node(delimited("‖", "‖"))),
            e("Matrix", "\\matrix", node(Kind::Matrix { cols: 2 })),
            e("Sum", "\\sum", node(Kind::Nary("sum".into()))),
            e("Product", "\\prod", node(Kind::Nary("product".into()))),
            e("Integral", "\\int", node(Kind::Nary("integral".into()))),
            e("Double integral", "\\iint", node(Kind::Nary("integral.double".into()))),
            e("Contour integral", "\\oint", node(Kind::Nary("integral.cont".into()))),
            e("Limit", "type lim then _", Insert::Atom(AtomKind::Ident, "lim".into())),
            e("Hat", "\\hat", node(Kind::Accent("hat".into()))),
            e("Bar", "\\bar", node(Kind::Accent("overline".into()))),
            e("Vector", "\\vec", node(Kind::Accent("arrow".into()))),
            e("Tilde", "\\tilde", node(Kind::Accent("tilde".into()))),
            e("Dot", "\\dot", node(Kind::Accent("dot".into()))),
            e("Text", "type \"", node(Kind::Text)),
            e("Box", "\\boxed", node(Kind::Boxed)),
            e("Sine", "type sin", Insert::Atom(AtomKind::Ident, "sin".into())),
            e("Cosine", "type cos", Insert::Atom(AtomKind::Ident, "cos".into())),
            e("Tangent", "type tan", Insert::Atom(AtomKind::Ident, "tan".into())),
            e("Natural logarithm", "type ln", Insert::Atom(AtomKind::Ident, "ln".into())),
            e("Logarithm", "type log", Insert::Atom(AtomKind::Ident, "log".into())),
            e("Exponential", "type exp", Insert::Atom(AtomKind::Ident, "exp".into())),
        ];
        vec![
            Tab { title: "Structures", face: "x²", entries: structures },
            Tab { title: "Greek letters", face: "αβ", entries: symbols(GREEK, AtomKind::Ident) },
            Tab { title: "Operators", face: "±×", entries: symbols(OPERATORS, AtomKind::Operator) },
            Tab { title: "Relations", face: "≤≈", entries: symbols(RELATIONS, AtomKind::Relation) },
            Tab { title: "Arrows", face: "⇒", entries: symbols(ARROWS, AtomKind::Relation) },
            Tab { title: "Symbols", face: "∞∂", entries: symbols(MISC, AtomKind::Symbol) },
        ]
    })
}

/// What `\name` inserts.
pub fn command(name: &str) -> Option<Insert> {
    let structure = match name {
        "frac" => Some(Kind::Fraction), "sqrt" => Some(Kind::Sqrt), "root" | "nthroot" => Some(Kind::Root),
        "sum" => Some(Kind::Nary("sum".into())), "prod" | "product" => Some(Kind::Nary("product".into())),
        "int" | "integral" => Some(Kind::Nary("integral".into())), "iint" => Some(Kind::Nary("integral.double".into())),
        "oint" => Some(Kind::Nary("integral.cont".into())),
        "hat" => Some(Kind::Accent("hat".into())), "bar" | "overline" => Some(Kind::Accent("overline".into())),
        "vec" => Some(Kind::Accent("arrow".into())), "tilde" => Some(Kind::Accent("tilde".into())),
        "dot" => Some(Kind::Accent("dot".into())), "ddot" => Some(Kind::Accent("dot.double".into())),
        "matrix" | "pmatrix" | "mat" => Some(Kind::Matrix { cols: 2 }),
        "abs" => Some(delimited("|", "|")), "norm" => Some(delimited("‖", "‖")),
        "text" => Some(Kind::Text), "boxed" | "box" => Some(Kind::Boxed),
        _ => None,
    };
    if let Some(kind) = structure { return Some(Insert::Node(kind)); }
    if super::tree::FUNCTIONS.contains(&name) { return Some(Insert::Atom(AtomKind::Ident, name.into())); }
    let aliases = [("ne", "neq"), ("le", "leq"), ("ge", "geq"), ("rightarrow", "to"), ("Rightarrow", "implies"),
        ("Leftarrow", "impliedby"), ("Leftrightarrow", "iff"), ("land", "wedge"), ("lor", "vee"), ("dots", "ldots"),
        ("infinity", "infty"), ("inf", "infty")];
    let name = aliases.iter().find(|(alias, _)| *alias == name).map_or(name, |(_, target)| target);
    for (list, kind) in [(GREEK, AtomKind::Ident), (OPERATORS, AtomKind::Operator), (RELATIONS, AtomKind::Relation),
        (ARROWS, AtomKind::Relation), (MISC, AtomKind::Symbol)] {
        if let Some((glyph, _)) = list.iter().find(|(_, n)| *n == name) { return Some(Insert::Atom(kind, (*glyph).into())); }
    }
    None
}

/// Commands starting with `prefix`, shortest first, for completion hints.
pub fn completions(prefix: &str) -> Vec<(String, String)> {
    if prefix.is_empty() { return Vec::new(); }
    let mut names: Vec<(String, String)> = Vec::new();
    for tab in tabs() {
        for entry in &tab.entries {
            if let Some(name) = entry.shortcut.strip_prefix('\\') {
                if name.starts_with(prefix) && !names.iter().any(|(n, _)| n == name) {
                    let glyph = match &entry.insert { Insert::Atom(_, text) => text.clone(), Insert::Node(_) => entry.name.to_string() };
                    names.push((name.to_string(), glyph));
                }
            }
        }
    }
    names.sort_by_key(|(n, _)| n.len());
    names.truncate(6);
    names
}

