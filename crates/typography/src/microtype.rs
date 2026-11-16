//! TeX/pdfTeX-inspired microtypography.
//!
//! This module operates only on derived layout values. Nothing here is part of
//! the native document format, DOCX mapping, or Typst parser.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MicrotypeOptions {
    /// Enable optical margin alignment (punctuation protrusion).
    pub optical_margin_alignment: bool,
    /// Symmetric glyph expansion/compression limit. `0.015` means ±1.5%.
    pub max_glyph_expansion: f32,
    /// Penalty used when choosing expansion over ordinary inter-word glue.
    pub glyph_expansion_penalty: f64,
}

impl Default for MicrotypeOptions {
    fn default() -> Self {
        Self {
            optical_margin_alignment: true,
            max_glyph_expansion: 0.015,
            glyph_expansion_penalty: 35.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct OpticalMargins {
    /// Amount by which the first glyph may protrude into the left margin.
    pub left_pt: f32,
    /// Amount by which the last glyph may protrude into the right margin.
    pub right_pt: f32,
}

/// Approximate punctuation protrusion in points.
///
/// TeX microtype packages usually tune these values per font. Impress starts
/// with deliberately conservative script-independent defaults and can later
/// replace them with per-font profiles without touching document files.
pub fn optical_margins(first: Option<char>, last: Option<char>, font_size_pt: f32) -> OpticalMargins {
    let left = first.map(left_protrusion_em).unwrap_or(0.0) * font_size_pt;
    let right = last.map(right_protrusion_em).unwrap_or(0.0) * font_size_pt;
    OpticalMargins { left_pt: left, right_pt: right }
}

/// Keep protrusion proportional to the actual edge glyph. A fixed em fraction
/// can put a narrow period entirely outside the margin in some typefaces.
pub fn optical_margins_with_advances(
    first: Option<char>, last: Option<char>, font_size_pt: f32,
    first_advance_pt: Option<f32>, last_advance_pt: Option<f32>,
) -> OpticalMargins {
    let mut margins = optical_margins(first, last, font_size_pt);
    if let Some(width) = first_advance_pt.filter(|v| v.is_finite() && *v >= 0.0) {
        margins.left_pt = margins.left_pt.min(width * 0.7);
    }
    if let Some(width) = last_advance_pt.filter(|v| v.is_finite() && *v >= 0.0) {
        margins.right_pt = margins.right_pt.min(width * 0.7);
    }
    margins
}

fn left_protrusion_em(c: char) -> f32 {
    match c {
        '“' | '‘' | '"' | '\'' => 0.18,
        '(' | '[' | '{' => 0.08,
        '«' | '‹' => 0.12,
        _ => 0.0,
    }
}

fn right_protrusion_em(c: char) -> f32 {
    match c {
        '.' | ',' => 0.42,
        ':' | ';' => 0.24,
        '!' | '?' => 0.20,
        '”' | '’' | '"' | '\'' => 0.18,
        ')' | ']' | '}' => 0.08,
        '»' | '›' => 0.12,
        _ => 0.0,
    }
}

/// Chooses the least-obtrusive combination of inter-word glue adjustment and
/// glyph expansion for one candidate line.
///
/// We sample the tiny expansion range because it is bounded to about ±1–2%.
/// This keeps the algorithm easy to audit while still providing the important
/// pdfTeX behaviour: small font-width changes can rescue an otherwise visibly
/// loose/tight justified line.
pub fn choose_line_adjustment(
    goal: f32,
    natural: f32,
    box_width: f32,
    stretch: f32,
    shrink: f32,
    max_expansion: f32,
    expansion_penalty: f64,
) -> (f32, f32, f64) {
    if (goal - natural).abs() < f32::EPSILON {
        return (0.0, 1.0, 0.0);
    }

    let max_expansion = max_expansion.clamp(0.0, 0.03);
    let steps = if max_expansion > 0.0 { 12 } else { 0 };
    let mut best = None::<(f32, f32, f64)>;

    for step in -steps..=steps {
        let expansion = if steps == 0 {
            0.0
        } else {
            max_expansion * (step as f32 / steps as f32)
        };
        let adjusted = natural + box_width * expansion;
        let ratio = adjustment_ratio(goal, adjusted, stretch, shrink);
        if !ratio.is_finite() {
            continue;
        }

        let glue_badness = 100.0 * f64::from(ratio.abs()).powi(3);
        let normalized_expansion = if max_expansion > 0.0 {
            f64::from((expansion / max_expansion).abs())
        } else {
            0.0
        };
        let expansion_cost = expansion_penalty * normalized_expansion.powi(2);
        let cost = glue_badness + expansion_cost;

        if best.as_ref().is_none_or(|(_, _, old_cost)| cost < *old_cost) {
            best = Some((ratio, 1.0 + expansion, cost));
        }
    }

    best.unwrap_or_else(|| {
        let ratio = adjustment_ratio(goal, natural, stretch, shrink);
        (ratio, 1.0, if ratio.is_finite() { 100.0 * f64::from(ratio.abs()).powi(3) } else { 10_000.0 })
    })
}

fn adjustment_ratio(goal: f32, natural: f32, stretch: f32, shrink: f32) -> f32 {
    let delta = goal - natural;
    if delta > 0.0 {
        if stretch > 0.0 { delta / stretch } else { f32::INFINITY }
    } else if delta < 0.0 {
        if shrink > 0.0 { delta / shrink } else { f32::NEG_INFINITY }
    } else {
        0.0
    }
}

