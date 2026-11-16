use model::{Equation, EquationDisplay, EquationNode, LimitPosition, VerticalPosition};

pub fn equation_to_typst(eq: &Equation) -> String {
    let body = node_to_typst(&eq.root);
    match eq.display {
        EquationDisplay::Inline => format!("${body}$"),
        EquationDisplay::Block => format!("$ {body} $"),
    }
}

pub fn node_to_typst(node: &EquationNode) -> String {
    match node {
        EquationNode::Row { children } => children.iter().map(node_to_typst).collect::<Vec<_>>().join(" "),
        EquationNode::Identifier { value } => identifier(value),
        EquationNode::Number { value } => value.clone(),
        EquationNode::Symbol { value } | EquationNode::Operator { value } | EquationNode::Relation { value } => escape_math(value),
        EquationNode::Text { value } => format!("\"{}\"", escape_string(value)),
        EquationNode::Fraction { numerator, denominator } => format!("{} / {}", group(numerator), group(denominator)),
        EquationNode::Radical { radicand, index } => match index {
            Some(index) => format!("root({}, {})", node_to_typst(index), node_to_typst(radicand)),
            None => format!("sqrt({})", node_to_typst(radicand)),
        },
        EquationNode::Superscript { base, exponent } => scripts(base, None, Some(exponent)),
        EquationNode::Subscript { base, subscript } => scripts(base, Some(subscript), None),
        EquationNode::Scripts { base, subscript, superscript } => scripts(base, subscript.as_deref(), superscript.as_deref()),
        EquationNode::Matrix { rows } => {
            let rows = rows.iter().map(|row| row.iter().map(node_to_typst).collect::<Vec<_>>().join(", ")).collect::<Vec<_>>().join("; ");
            format!("mat({rows})")
        }
        EquationNode::Delimited { left, body, right } => format!("{} {} {}", left, node_to_typst(body), right),
        EquationNode::Function { name, argument } => format!("{}({})", name, node_to_typst(argument)),
        EquationNode::Accent { accent, body } => format!("{}({})", accent, node_to_typst(body)),
        EquationNode::Nary { operator, body, subscript, superscript } => {
            let mut op = operator.clone();
            if let Some(sub) = subscript { op.push_str(&format!("_{}", group(sub))); }
            if let Some(sup) = superscript { op.push_str(&format!("^{}", group(sup))); }
            format!("{op} {}", node_to_typst(body))
        }
        EquationNode::Limit { base, limit, position } => match position {
            LimitPosition::Lower => format!("attach(limits({}), b: {})", node_to_typst(base), node_to_typst(limit)),
            LimitPosition::Upper => format!("attach(limits({}), t: {})", node_to_typst(base), node_to_typst(limit)),
        },
        EquationNode::PreScripts { base, subscript, superscript } => {
            // Typst has no dedicated OMML-like prescript syntax in Impress's structured
            // subset yet, so preserve the semantics with a tiny helper expression.
            let sub = subscript.as_deref().map(node_to_typst).unwrap_or_default();
            let sup = superscript.as_deref().map(node_to_typst).unwrap_or_default();
            format!("scripts(pre-sub: [{}], pre-sup: [{}], {})", sub, sup, node_to_typst(base))
        }
        EquationNode::Bar { body, position } => match position {
            VerticalPosition::Over => format!("overline({})", node_to_typst(body)),
            VerticalPosition::Under => format!("underline({})", node_to_typst(body)),
        },
        EquationNode::GroupCharacter { body, character, position } => match position {
            VerticalPosition::Over => format!("accent({}, \"{}\")", node_to_typst(body), escape_string(character)),
            VerticalPosition::Under => format!("underbrace({})", node_to_typst(body)),
        },
        EquationNode::EquationArray { rows } => {
            let rows = rows.iter().map(node_to_typst).collect::<Vec<_>>().join(", ");
            format!("cases({rows})")
        }
        EquationNode::Boxed { body } => format!("box({})", node_to_typst(body)),
        EquationNode::Phantom { body } => format!("hide({})", node_to_typst(body)),
        EquationNode::Space { em } => format!("h({em}em)"),
        EquationNode::RawTypst { source } => source.clone(),
    }
}

/// Typst's built-in math operators render upright with proper limits when
/// written bare; other multi-letter names would be looked up as variables.
const MATH_OPERATORS: &[&str] = &["arccos", "arcsin", "arctan", "arg", "cos", "cosh", "cot", "coth", "csc",
    "csch", "ctg", "deg", "det", "dim", "exp", "gcd", "lcm", "hom", "id", "im", "inf", "ker", "lg", "lim",
    "liminf", "limsup", "ln", "log", "max", "min", "mod", "Pr", "sec", "sech", "sin", "sinc", "sinh", "sup",
    "tan", "tanh", "tg", "tr"];

pub fn is_math_operator(name: &str) -> bool { MATH_OPERATORS.contains(&name) }

fn identifier(value: &str) -> String {
    if value.chars().count() <= 1 { escape_math(value) }
    else if is_math_operator(value) { value.to_string() }
    else { format!("italic(\"{}\")", escape_string(value)) }
}

/// Characters with syntactic meaning in Typst math are escaped, so an
/// operator such as `,` or `/` never splits arguments or builds a fraction.
fn escape_math(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        if matches!(c, ',' | ';' | '/' | '_' | '^' | '&' | '#' | '$' | '\\' | '"' | '@') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Scripts on a single token attach directly (`x^(2)`). Other bases use
/// `attach`, which, unlike `(base)^(e)`, draws no parentheses.
fn scripts(base: &EquationNode, sub: Option<&EquationNode>, sup: Option<&EquationNode>) -> String {
    let simple = matches!(base, EquationNode::Identifier { .. } | EquationNode::Number { .. } | EquationNode::Symbol { .. }
        | EquationNode::Operator { .. } | EquationNode::Relation { .. } | EquationNode::Delimited { .. } | EquationNode::Text { .. });
    if simple {
        let mut out = node_to_typst(base);
        if let Some(sub) = sub { out.push_str(&format!("_{}", group(sub))); }
        if let Some(sup) = sup { out.push_str(&format!("^{}", group(sup))); }
        return out;
    }
    let mut out = format!("attach({}", node_to_typst(base));
    if let Some(sub) = sub { out.push_str(&format!(", b: {}", node_to_typst(sub))); }
    if let Some(sup) = sup { out.push_str(&format!(", t: {}", node_to_typst(sup))); }
    out.push(')');
    out
}

/// A fraction part or script: parenthesized unless it is one token or call,
/// so `a/b`, `x^2` and `(x + 1) / 2` read as typed. Typst drops these
/// parentheses, so a `Delimited` part keeps its own inside them.
fn group(node: &EquationNode) -> String {
    let token = match node {
        EquationNode::Identifier { value } => (value.chars().count() == 1 && escape_math(value) == *value) || is_math_operator(value),
        EquationNode::Number { value } => value.chars().all(|c| c.is_ascii_digit() || c == '.') && !value.starts_with('.'),
        EquationNode::Symbol { value } => value.chars().count() == 1 && escape_math(value) == *value
            && !matches!(value.as_str(), "(" | ")" | "[" | "]" | "{" | "}" | "|" | "'"),
        EquationNode::Radical { .. } | EquationNode::Matrix { .. } | EquationNode::Accent { .. } | EquationNode::Text { .. } => true,
        _ => false,
    };
    if token { node_to_typst(node) } else { format!("({})", node_to_typst(node)) }
}

fn escape_string(value: &str) -> String { value.replace('\\', "\\\\").replace('"', "\\\"") }


