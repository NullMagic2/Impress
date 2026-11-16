//! Equation source as Typst math. The source is parsed into `EquationNode`
//! values (see `format_typst::parse_math_source`); embedded Typst code is
//! never evaluated.
use model::{EquationDisplay, EquationNode as N};
const LIMIT:usize=65536;

pub const TEMPLATES:&[(&str,&str)]=&[
    ("Fraction","a/b"),("Square root","sqrt(x)"),("Root","root(3, x)"),
    ("Power","x^2"),("Subscript","x_i"),("Matrix","mat(a, b; c, d)"),
    ("Integral","integral_a^b f(x) dif x"),("Sum","sum_(i=1)^n x_i"),
];

pub fn parse(input:&str)->Result<N,String> { parse_with_display(input).map(|(node,_)|node) }

/// Parses equation source. `$x$` asks for an inline equation and `$ x $`
/// (with spaces inside, as in Typst) for a display one; bare source keeps
/// the equation's current display.
pub fn parse_with_display(input:&str)->Result<(N,Option<EquationDisplay>),String> {
    if input.len()>LIMIT{return Err("Equation input exceeds 64 KiB".into());}
    let input=input.trim();if input.is_empty(){return Err("Enter an equation".into());}
    if let Some(json)=input.strip_prefix("@ast ") {
        let root: N = serde_json::from_str(json).map_err(|e|format!("Invalid semantic equation: {e}"))?;
        validate(&root)?; return Ok((root,None));
    }
    let (body,display)=match input.strip_prefix('$').and_then(|s|s.strip_suffix('$')) {
        Some(body) if !body.trim().is_empty() => {
            let block=body.starts_with(char::is_whitespace)&&body.ends_with(char::is_whitespace);
            (body,Some(if block{EquationDisplay::Block}else{EquationDisplay::Inline}))
        }
        _ => (input,None),
    };
    let result=format_typst::parse_math_source(body)?;
    validate(&result)?; Ok((result,display))
}

/// The JSON fallback is a semantic representation, not an escape hatch to
/// executable Typst. Validate the final tree too: several individually bounded
/// script chains can otherwise combine into an excessively deep tree.
fn validate(root: &N) -> Result<(), String> {
    let mut pending = vec![(root, 0usize)]; let mut count = 0usize;
    while let Some((node, depth)) = pending.pop() {
        count += 1;
        if depth > 64 || count > 10000 { return Err("Equation exceeds the depth or node limit".into()); }
        let mut push = |node| pending.push((node, depth + 1));
        match node {
            N::RawTypst { source } if !format_typst::is_safe_raw(source) => return Err("This Typst cannot be used in an equation: it runs code or calls a function that is not a math function".into()),
            N::Row { children } | N::EquationArray { rows: children } => { for child in children { push(child); } },
            N::Fraction { numerator, denominator } => { push(numerator.as_ref()); push(denominator.as_ref()); },
            N::Radical { radicand, index } => { push(radicand.as_ref()); if let Some(index) = index { push(index.as_ref()); } },
            N::Superscript { base, exponent } => { push(base.as_ref()); push(exponent.as_ref()); },
            N::Subscript { base, subscript } => { push(base.as_ref()); push(subscript.as_ref()); },
            N::Scripts { base, subscript, superscript } | N::PreScripts { base, subscript, superscript } => {
                push(base.as_ref()); if let Some(n) = subscript { push(n.as_ref()); } if let Some(n) = superscript { push(n.as_ref()); }
            },
            N::Matrix { rows } => {
                if rows.is_empty() || rows.len() > 64 || rows[0].is_empty() || rows[0].len() > 64 || rows.iter().any(|r| r.len() != rows[0].len()) { return Err("Matrix dimensions must be rectangular and between 1 and 64".into()); }
                for row in rows { for cell in row { push(cell); } }
            },
            N::Nary { body, subscript, superscript, .. } => {
                push(body.as_ref()); if let Some(n) = subscript { push(n.as_ref()); } if let Some(n) = superscript { push(n.as_ref()); }
            },
            N::Limit { base, limit, .. } => { push(base.as_ref()); push(limit.as_ref()); },
            N::Function { argument, .. } => push(argument.as_ref()),
            N::Delimited { body, .. } | N::Accent { body, .. } | N::Bar { body, .. } |
            N::GroupCharacter { body, .. } | N::Boxed { body } | N::Phantom { body } => push(body.as_ref()),
            N::Space { em } if !em.is_finite() || em.abs() > 100.0 => return Err("Invalid equation spacing".into()),
            _ => {}
        }
    }
    Ok(())
}

/// The equation as Typst math source.
pub fn format(node:&N)->String { format_typst::node_to_typst(node) }

/// The equation as Typst source with its delimiters: `$x$` inline, `$ x $` display.
pub fn format_with_display(node:&N,display:EquationDisplay)->String {
    let body=format(node);
    match display { EquationDisplay::Inline=>format!("${body}$"), EquationDisplay::Block=>format!("$ {body} $") }
}

#[cfg(test)] #[path = "../tests/unit/equation_input.rs"] mod tests;
