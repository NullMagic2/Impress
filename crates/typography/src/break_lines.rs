//! Knuth–Plass-inspired paragraph composition with conservative pdfTeX-style
//! glyph expansion.

use crate::{choose_line_adjustment, MicrotypeOptions};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LineItem {
    Box { width: f32 },
    Glue { width: f32, stretch: f32, shrink: f32 },
    Penalty { width: f32, penalty: i32, flagged: bool },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BreakPoint {
    /// Item index after which the line ends.
    pub item_index: usize,
    /// TeX-like glue adjustment ratio. Positive stretches spaces; negative shrinks.
    pub ratio: f32,
    /// Horizontal glyph scale chosen by microtypography (normally 0.985–1.015).
    pub glyph_scale: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct ComposeOptions {
    pub line_width: f32,
    pub tolerance: f32,
    pub consecutive_flagged_penalty: f64,
    pub microtype: MicrotypeOptions,
}

impl Default for ComposeOptions {
    fn default() -> Self {
        Self {
            line_width: 420.0,
            tolerance: 3.0,
            consecutive_flagged_penalty: 100.0,
            microtype: MicrotypeOptions::default(),
        }
    }
}

#[derive(Clone, Debug)]
struct Candidate {
    previous: Option<usize>,
    at: usize,
    ratio: f32,
    glyph_scale: f32,
    demerits: f64,
    flagged: bool,
}

pub fn compose(items: &[LineItem], options: ComposeOptions) -> Vec<BreakPoint> {
    if items.is_empty() || options.line_width <= 0.0 {
        return Vec::new();
    }

    // Prefix measurements make each candidate O(1), instead of repeatedly
    // walking a paragraph slice inside a quadratic pair of loops.
    let prefix=MetricPrefix::new(items);
    let mut break_positions = vec![0usize];
    for (i, item) in items.iter().enumerate() {
        match item {
            LineItem::Glue { .. } => break_positions.push(i + 1),
            LineItem::Penalty { penalty, .. } if *penalty < 10_000 => break_positions.push(i + 1),
            _ => {}
        }
    }
    if *break_positions.last().unwrap_or(&0) != items.len() {
        break_positions.push(items.len());
    }
    break_positions.sort_unstable();
    break_positions.dedup();

    let mut candidates: Vec<Option<Candidate>> = vec![None; break_positions.len()];
    candidates[0] = Some(Candidate {
        previous: None,
        at: 0,
        ratio: 0.0,
        glyph_scale: 1.0,
        demerits: 0.0,
        flagged: false,
    });

    for target_idx in 1..break_positions.len() {
        let end = break_positions[target_idx];
        let mut best: Option<Candidate> = None;

        for start_idx in 0..target_idx {
            let Some(prev) = candidates[start_idx].as_ref() else { continue; };
            let start = break_positions[start_idx];
            let metrics = prefix.range(items,start,end);
            // End-of-paragraph is not permission to paint an overfull line.
            let terminal=end==items.len();
            let (ratio, glyph_scale, line_badness) = if terminal && metrics.width<=options.line_width {
                (0.0,1.0,0.0)
            } else { choose_line_adjustment(
                options.line_width,
                metrics.width,
                metrics.box_width,
                metrics.stretch,
                metrics.shrink,
                options.microtype.max_glyph_expansion,
                options.microtype.glyph_expansion_penalty,
            ) };

            let forced = end == items.len();
            if (!ratio.is_finite() || ratio < -1.0 || ratio > options.tolerance) && !(forced && metrics.width<=options.line_width) {
                continue;
            }

            let (penalty, flagged) = terminal_penalty(&items[start..end]);
            let penalty_cost = if penalty >= 0 {
                f64::from(penalty).powi(2)
            } else {
                -f64::from(penalty).powi(2)
            };
            let mut demerits = prev.demerits + (1.0 + line_badness).powi(2) + penalty_cost;
            if prev.flagged && flagged {
                demerits += options.consecutive_flagged_penalty;
            }

            let candidate = Candidate {
                previous: Some(start_idx),
                at: target_idx,
                ratio,
                glyph_scale,
                demerits,
                flagged,
            };
            if best.as_ref().is_none_or(|current| candidate.demerits < current.demerits) {
                best = Some(candidate);
            }
        }
        candidates[target_idx] = best;
    }

    let mut result = Vec::new();
    let mut cursor = candidates.len().checked_sub(1);
    while let Some(index) = cursor {
        let Some(candidate) = candidates[index].as_ref() else { break; };
        if candidate.at == 0 {
            break;
        }
        result.push(BreakPoint {
            item_index: break_positions[candidate.at],
            ratio: candidate.ratio,
            glyph_scale: candidate.glyph_scale,
        });
        cursor = candidate.previous;
    }
    result.reverse();
    result
}

#[derive(Clone, Copy, Default)]
struct Metrics {
    width: f32,
    box_width: f32,
    stretch: f32,
    shrink: f32,
}

struct MetricPrefix { values:Vec<Metrics> }
impl MetricPrefix {
    fn new(items:&[LineItem])->Self {
        let mut values=Vec::with_capacity(items.len()+1);let mut m=Metrics::default();values.push(m);
        for item in items {match *item {
            LineItem::Box{width}=>{m.width+=width;m.box_width+=width;},
            LineItem::Glue{width,stretch,shrink}=>{m.width+=width;m.stretch+=stretch;m.shrink+=shrink;},
            LineItem::Penalty{..}=>{},
        }values.push(m);}
        Self{values}
    }
    fn range(&self,items:&[LineItem],start:usize,end:usize)->Metrics {
        let a=self.values[start];let b=self.values[end];
        let mut m=Metrics{width:b.width-a.width,box_width:b.box_width-a.box_width,
            stretch:b.stretch-a.stretch,shrink:b.shrink-a.shrink};
        if let Some(LineItem::Penalty{width,..})=items.get(end.saturating_sub(1)){m.width+=width;m.box_width+=width;}
        m
    }
}

fn terminal_penalty(items: &[LineItem]) -> (i32, bool) {
    match items.last() {
        Some(LineItem::Penalty { penalty, flagged, .. }) => (*penalty, *flagged),
        _ => (0, false),
    }
}

#[cfg(test)] #[path = "../tests/unit/break_lines.rs"] mod tests;

