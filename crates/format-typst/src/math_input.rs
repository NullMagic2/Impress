//! Typst math source to `EquationNode`.
//!
//! Equations are typed as Typst math (`x^2 + 1/2`, `sum_(k=1)^n k`,
//! `alpha -> beta`). The source is parsed with Typst's own parser and mapped
//! onto the semantic AST. Calls of Typst math functions the AST has no node
//! for (`binom`, `cases`, `cancel`, …) are kept verbatim as `RawTypst`, so
//! they still render. Embedded code (`#…`) and calls of anything but math
//! functions are refused: an equation never runs Typst code.
use crate::equation::is_math_operator;
use model::{EquationNode as N, VerticalPosition};
use typst_syntax::ast::{self, Arg, AstNode, Expr, MathTextKind};
use typst_syntax::{parse_math, SyntaxKind, SyntaxNode};

const LIMIT: usize = 65536;
const MAX_DEPTH: usize = 64;
const MAX_NODES: usize = 10000;

/// Typst math functions an equation may call. Anything else that looks like
/// a call must be a symbol (`arrow.r(x)` is the arrow followed by `(x)`).
const FUNCTIONS: &[&str] = &[
    "abs", "accent", "acute", "arrow", "attach", "bb", "binom", "bold", "box", "breve", "cal", "cancel",
    "caron", "cases", "ceil", "circle", "class", "diaer", "display", "dot", "floor", "frac", "frak",
    "grave", "h", "harpoon", "hat", "hide", "inline", "italic", "limits", "lr", "macron", "mat", "mid",
    "mono", "norm", "op", "overbrace", "overbracket", "overline", "overparen", "overshell", "primes",
    "root", "round", "sans", "scr", "script", "scripts", "serif", "sqrt", "sscript", "stretch", "text",
    "tilde", "underbrace", "underbracket", "underline", "underparen", "undershell", "upright", "vec",
];

/// Accents the visual editor draws, by their Typst names.
const ACCENTS: &[&str] = &["hat", "tilde", "dot", "dot.double", "arrow", "overline", "macron"];

/// Large operators whose following terms become their body.
const LARGE: &[(&str, &str)] = &[
    ("sum", "∑"), ("product", "∏"), ("product.co", "∐"), ("integral", "∫"), ("integral.double", "∬"),
    ("integral.triple", "∭"), ("integral.cont", "∮"), ("union.big", "⋃"), ("inter.big", "⋂"),
];

/// Parses Typst math (without the surrounding `$`).
pub fn parse_math_source(source: &str) -> Result<N, String> {
    if source.len() > LIMIT { return Err("Equation input exceeds 64 KiB".into()); }
    if source.trim().is_empty() { return Err("Enter an equation".into()); }
    let root = parse_math(source);
    check(&root)?;
    let math = root.cast::<ast::Math>().ok_or("Not Typst math")?;
    let mut cx = Cx { depth: 0, nodes: 0 };
    let row = cx.row(math.exprs().collect())?;
    if row.is_empty() { return Err("Enter an equation".into()); }
    Ok(collapse(row))
}

/// Whether `source` may be kept as raw Typst inside an equation: it parses
/// as math, embeds no code and calls only math functions.
pub fn is_safe_raw(source: &str) -> bool {
    source.len() <= LIMIT && check(&parse_math(source)).is_ok()
}

/// Syntax errors, embedded code and calls of non-math functions.
fn check(root: &SyntaxNode) -> Result<(), String> {
    let (errors, _) = root.errors_and_warnings();
    if let Some(error) = errors.first() { return Err(format!("Typst: {}", error.message)); }
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        match node.kind() {
            SyntaxKind::Hash => return Err("Typst code (#…) is not allowed in an equation".into()),
            SyntaxKind::Dollar => return Err("Write the equation without $ … $ inside it".into()),
            SyntaxKind::MathCall => {
                let callee = node.children().next().map(SyntaxNode::full_text).unwrap_or_default();
                if !FUNCTIONS.contains(&callee.as_str()) && symbol(&callee).is_none() && !is_math_operator(&callee) {
                    return Err(format!("Unknown function \"{callee}\""));
                }
            }
            _ => {}
        }
        pending.extend(node.children());
    }
    Ok(())
}

struct Cx { depth: usize, nodes: usize }

fn collapse(mut row: Vec<N>) -> N { if row.len() == 1 { row.remove(0) } else { N::Row { children: row } } }
fn raw(node: &SyntaxNode) -> N { N::RawTypst { source: node.full_text().trim().to_string() } }
fn text_of<'a>(expr: Expr<'a>) -> String { expr.to_untyped().full_text().to_string() }

impl Cx {
    fn enter(&mut self) -> Result<(), String> {
        self.depth += 1;
        if self.depth > MAX_DEPTH { return Err("Equation nesting is too deep".into()); }
        Ok(())
    }

    /// A row of expressions. Large operators take the following terms, up to
    /// a relation or line break, as their body.
    fn row(&mut self, exprs: Vec<Expr<'_>>) -> Result<Vec<N>, String> {
        self.enter()?;
        let mut out = Vec::new();
        let mut i = 0;
        while i < exprs.len() {
            let expr = exprs[i];
            i += 1;
            if let Some((operator, glyph, subscript, superscript)) = self.large(expr)? {
                let start = i;
                while i < exprs.len() && !self.ends_body(exprs[i]) { i += 1; }
                let mut body = Vec::new();
                for &e in &exprs[start..i] { self.expr(e, &mut body)?; }
                if body.is_empty() {
                    // Nothing to integrate: keep the bare operator with its limits.
                    let base = Box::new(N::Operator { value: glyph.into() });
                    out.push(match (subscript, superscript) {
                        (None, None) => *base,
                        (subscript, superscript) => N::Scripts { base, subscript, superscript },
                    });
                } else {
                    out.push(N::Nary { operator: operator.into(), body: Box::new(collapse(body)), subscript, superscript });
                }
                continue;
            }
            self.expr(expr, &mut out)?;
        }
        self.depth -= 1;
        Ok(out)
    }

    /// `sum`, `integral_0^1`, … as (name, glyph, lower, upper).
    #[allow(clippy::type_complexity)]
    fn large(&mut self, expr: Expr<'_>) -> Result<Option<(&'static str, &'static str, Option<Box<N>>, Option<Box<N>>)>, String> {
        let find = |e: Expr<'_>| match e {
            Expr::MathIdent(_) | Expr::MathFieldAccess(_) => { let name = text_of(e); LARGE.iter().find(|(n, _)| *n == name).copied() }
            _ => None,
        };
        match expr {
            Expr::MathAttach(attach) if attach.primes().is_none() => {
                let Some((name, glyph)) = find(attach.base()) else { return Ok(None); };
                let lower = attach.bottom().map(|e| self.group(e)).transpose()?.map(Box::new);
                let upper = attach.top().map(|e| self.group(e)).transpose()?.map(Box::new);
                Ok(Some((name, glyph, lower, upper)))
            }
            e => Ok(find(e).map(|(name, glyph)| (name, glyph, None, None))),
        }
    }

    fn ends_body(&self, expr: Expr<'_>) -> bool {
        match expr {
            Expr::Linebreak(_) | Expr::MathAlignPoint(_) => true,
            Expr::MathShorthand(s) => matches!(classify(&s.get().to_string()), N::Relation { .. }),
            Expr::MathText(t) => matches!(t.get(), MathTextKind::Grapheme(g) if matches!(classify(g), N::Relation { .. })),
            Expr::MathIdent(_) | Expr::MathFieldAccess(_) => symbol(&text_of(expr)).is_some_and(|s| matches!(classify(&s), N::Relation { .. })),
            _ => false,
        }
    }

    /// A fraction part, script or argument: parentheses only group.
    fn group(&mut self, expr: Expr<'_>) -> Result<N, String> {
        let mut out = Vec::new();
        match expr {
            Expr::Math(math) => out = self.row(math.exprs().collect())?,
            e => self.expr(e, &mut out)?,
        }
        Ok(collapse(out))
    }

    fn expr(&mut self, expr: Expr<'_>, out: &mut Vec<N>) -> Result<(), String> {
        self.nodes += 1;
        if self.nodes > MAX_NODES { return Err("Too many equation nodes".into()); }
        let node = match expr {
            Expr::Space(_) => return Ok(()),
            Expr::Linebreak(_) => N::RawTypst { source: "\\".into() },
            Expr::MathAlignPoint(_) => N::RawTypst { source: "&".into() },
            Expr::Escape(escape) => classify(&escape.get().to_string()),
            Expr::MathShorthand(shorthand) => classify(&shorthand.get().to_string()),
            Expr::Str(text) => N::Text { value: text.get().to_string() },
            Expr::MathText(text) => match text.get() {
                MathTextKind::Number(number) => N::Number { value: number.to_string() },
                MathTextKind::Grapheme(grapheme) => classify(grapheme),
            },
            Expr::MathIdent(_) | Expr::MathFieldAccess(_) => ident(&text_of(expr))?,
            Expr::MathPrimes(primes) => N::Operator { value: "′".repeat(primes.count()) },
            Expr::MathFrac(frac) => N::Fraction { numerator: Box::new(self.group(frac.num())?), denominator: Box::new(self.group(frac.denom())?) },
            Expr::MathRoot(root) => N::Radical {
                radicand: Box::new(self.group(root.radicand())?),
                index: root.index().map(|n| Box::new(N::Number { value: n.to_string() })),
            },
            Expr::MathDelimited(delimited) => {
                let body = collapse(self.row(delimited.body().exprs().collect())?);
                N::Delimited { left: delimiter(delimited.open()), body: Box::new(body), right: delimiter(delimited.close()) }
            }
            Expr::MathAttach(attach) => {
                let mut base = Vec::new();
                self.expr(attach.base(), &mut base)?;
                if let Some(primes) = attach.primes() { base.push(N::Operator { value: "′".repeat(primes.count()) }); }
                let base = Box::new(collapse(base));
                let lower = attach.bottom().map(|e| self.group(e)).transpose()?.map(Box::new);
                let upper = attach.top().map(|e| self.group(e)).transpose()?.map(Box::new);
                match (lower, upper) {
                    (None, None) => *base,
                    (Some(subscript), None) => N::Subscript { base, subscript },
                    (None, Some(exponent)) => N::Superscript { base, exponent },
                    (subscript, superscript) => N::Scripts { base, subscript, superscript },
                }
            }
            Expr::Math(math) => { out.extend(self.row(math.exprs().collect())?); return Ok(()); }
            Expr::MathCall(call) => return self.call(call, out),
            other => return Err(format!("\"{}\" is not supported in an equation", text_of(other).trim())),
        };
        out.push(node);
        Ok(())
    }

    fn call(&mut self, call: ast::MathCall<'_>, out: &mut Vec<N>) -> Result<(), String> {
        let name = call.callee().to_untyped().full_text().to_string();
        if !FUNCTIONS.contains(&name.as_str()) {
            // Not a function: Typst shows the name followed by its arguments.
            out.push(ident(&name)?);
            let mut inner = Vec::new();
            for item in call.args().content_items() {
                match item {
                    ast::MathArgItem::Arg(Arg::Pos(e)) => match e { Expr::Math(m) => inner.extend(self.row(m.exprs().collect())?), e => self.expr(e, &mut inner)? },
                    ast::MathArgItem::Arg(_) => return Err(format!("\"{name}\" takes no named arguments")),
                    ast::MathArgItem::Comma(..) => inner.push(N::Operator { value: ",".into() }),
                    ast::MathArgItem::Semicolon(..) => inner.push(N::Operator { value: ";".into() }),
                    _ => {}
                }
            }
            out.push(N::Delimited { left: "(".into(), body: Box::new(collapse(inner)), right: ")".into() });
            return Ok(());
        }
        let mut positional = Vec::new();
        let mut named = Vec::new();
        let mut rows = vec![Vec::new()];
        let mut spread = false;
        for arg in call.args().arg_items() {
            match arg.arg {
                Arg::Pos(e) => { positional.push(e); rows.last_mut().expect("a row").push(e); }
                Arg::Named(n) => named.push((n.name().as_str().to_string(), n.expr())),
                Arg::Spread(_) => spread = true,
            }
            if arg.ends_in_semicolon { rows.push(Vec::new()); }
        }
        if rows.last().is_some_and(Vec::is_empty) && rows.len() > 1 { rows.pop(); }
        let plain = named.is_empty() && !spread;
        let one = positional.len() == 1;
        let b = Box::new;
        let node = match name.as_str() {
            "frac" if plain && positional.len() == 2 => N::Fraction { numerator: b(self.group(positional[0])?), denominator: b(self.group(positional[1])?) },
            "sqrt" if plain && one => N::Radical { radicand: b(self.group(positional[0])?), index: None },
            "root" if plain && positional.len() == 2 => N::Radical { index: Some(b(self.group(positional[0])?)), radicand: b(self.group(positional[1])?) },
            "mat" if plain && !positional.is_empty() && rows.iter().all(|r| r.len() == rows[0].len()) && rows.len() <= 64 && rows[0].len() <= 64 => {
                let mut matrix = Vec::new();
                for row in rows { matrix.push(row.into_iter().map(|e| self.group(e)).collect::<Result<Vec<_>, _>>()?); }
                N::Matrix { rows: matrix }
            }
            "abs" | "norm" | "floor" | "ceil" if plain && one => {
                let (left, right) = match name.as_str() { "abs" => ("|", "|"), "norm" => ("‖", "‖"), "floor" => ("⌊", "⌋"), _ => ("⌈", "⌉") };
                N::Delimited { left: left.into(), body: b(self.group(positional[0])?), right: right.into() }
            }
            accent if plain && one && ACCENTS.contains(&accent) => N::Accent { accent: accent.into(), body: b(self.group(positional[0])?) },
            "underline" if plain && one => N::Bar { body: b(self.group(positional[0])?), position: VerticalPosition::Under },
            "box" if plain && one => N::Boxed { body: b(self.group(positional[0])?) },
            "italic" if plain && one && string_arg(positional[0]).is_some() => N::Identifier { value: string_arg(positional[0]).expect("checked") },
            "upright" if plain && one && string_arg(positional[0]).is_some() => N::Text { value: string_arg(positional[0]).expect("checked") },
            "attach" if !spread && one && named.iter().all(|(n, _)| n == "t" || n == "b") => {
                let base = b(self.group(positional[0])?);
                let get = |cx: &mut Self, key: &str| named.iter().find(|(n, _)| n == key).map(|(_, e)| cx.group(*e)).transpose();
                match (get(self, "b")?.map(Box::new), get(self, "t")?.map(Box::new)) {
                    (None, None) => *base,
                    (Some(subscript), None) => N::Subscript { base, subscript },
                    (None, Some(exponent)) => N::Superscript { base, exponent },
                    (subscript, superscript) => N::Scripts { base, subscript, superscript },
                }
            }
            _ => raw(call.to_untyped()),
        };
        out.push(node);
        Ok(())
    }
}

/// `"abc"` as a call argument.
fn string_arg(expr: Expr<'_>) -> Option<String> {
    match expr {
        Expr::Str(s) => Some(s.get().to_string()),
        Expr::Math(m) => { let mut exprs = m.exprs().filter(|e| !matches!(e, Expr::Space(_))); match (exprs.next(), exprs.next()) { (Some(Expr::Str(s)), None) => Some(s.get().to_string()), _ => None } }
        _ => None,
    }
}

fn delimiter(expr: Expr<'_>) -> String {
    match expr {
        Expr::MathShorthand(s) => s.get().to_string(),
        Expr::Escape(e) => e.get().to_string(),
        e => text_of(e).trim().to_string(),
    }
}

/// A name in math: an operator such as `sin`, or a symbol such as `alpha`
/// or `arrow.r.double`.
fn ident(name: &str) -> Result<N, String> {
    if is_math_operator(name) { return Ok(N::Identifier { value: name.into() }); }
    // Differentials and spacing render through Typst as written.
    if matches!(name, "dif" | "Dif" | "quad" | "wide" | "thin" | "med" | "thick" | "oo") { return Ok(N::RawTypst { source: name.into() }); }
    if let Some(&(_, glyph)) = LARGE.iter().find(|(n, _)| *n == name) { return Ok(N::Operator { value: glyph.into() }); }
    match symbol(name) {
        Some(value) => Ok(classify(&value)),
        None => Err(format!("Unknown name \"{name}\" (write text in quotes: \"{name}\")")),
    }
}

/// The character Typst's `sym` module gives `name` (`arrow.r` → `→`).
fn symbol(name: &str) -> Option<String> {
    let (base, modifiers) = name.split_once('.').unwrap_or((name, ""));
    match codex::SYM.get(base)?.def {
        codex::Def::Symbol(symbol) => symbol.get(codex::ModifierSet::from_raw_dotted(modifiers)).map(|(s, _)| s.to_string()),
        codex::Def::Module(_) => None,
    }
}

/// Greek and Latin letters are identifiers; relations, arrows and binary
/// operators keep their role; anything else is a symbol.
fn classify(value: &str) -> N {
    let mut chars = value.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else { return N::Symbol { value: value.into() } };
    let code = c as u32;
    if c.is_alphabetic() && !matches!(c, 'ℝ' | 'ℕ' | 'ℤ' | 'ℚ' | 'ℂ' | 'ℓ' | 'ℏ') {
        return N::Identifier { value: value.into() };
    }
    let relation = "=<>≤≥≠≈≡∼≃≅∝≪≫⋘⋙∈∉∋∌⊂⊃⊆⊇⊄⊅⊊⊋⊥∥∦∣∤≔≕⩴≺≻⪯⪰⊢⊨≐≜≍";
    if relation.contains(c) || (0x2190..=0x21FF).contains(&code) || (0x27F0..=0x27FF).contains(&code)
        || (0x2900..=0x297F).contains(&code) || (0x2B30..=0x2B4F).contains(&code) {
        return N::Relation { value: value.into() };
    }
    if "+−-±∓×·÷∘∗⊕⊖⊗⊘⊙∪∩∖∧∨¬!,;:′/⋅⋆".contains(c) { return N::Operator { value: value.into() }; }
    N::Symbol { value: value.into() }
}

#[cfg(test)] #[path = "../tests/unit/math_input.rs"] mod tests;
