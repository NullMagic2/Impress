//! Layout-aware caret navigation.
//!
//! The semantic editor deliberately does not know how a paragraph wrapped.
//! This module derives visual rows from page hit regions and resolves Up/Down
//! movement while preserving the user's horizontal column across mixed runs.

use crate::{CaretStop, SourcePosition, TextHitRegion, VectorDocument};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisualMoveDirection {
    Up,
    Down,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VisualCaretTarget {
    pub position: SourcePosition,
    /// Page-space x coordinate to preserve on subsequent vertical moves.
    pub preferred_x_pt: f32,
    /// Exact wrapped line that owns the target caret stop.
    pub visual_line: usize,
}

#[derive(Clone, Debug)]
struct VisualRow<'a> {
    node_id: &'a str,
    visual_line: usize,
    min_offset: usize,
    max_offset: usize,
    stops: Vec<&'a CaretStop>,
}

/// Move a source caret to the nearest stop on the previous/next laid-out line.
///
/// `preferred_x_pt` should be carried across consecutive Up/Down commands. Pass
/// `None` on the first move and the current caret x is captured automatically.
pub fn visual_vertical_target(
    document: &VectorDocument,
    position: &SourcePosition,
    current_visual_line: Option<usize>,
    direction: VisualMoveDirection,
    preferred_x_pt: Option<f32>,
) -> Option<VisualCaretTarget> {
    let rows = collect_rows(document);
    let current_index = current_visual_line
        .and_then(|line| rows.iter().position(|row| {
            row.node_id == position.node_id && row.visual_line == line && row_contains(row, position)
        }))
        .or_else(|| rows.iter().position(|row| row_contains(row, position)))?;
    let current = &rows[current_index];
    let preferred_x = preferred_x_pt.unwrap_or_else(|| x_for_offset(current, position.byte_offset));
    let target_index = match direction {
        VisualMoveDirection::Up => current_index.checked_sub(1)?,
        VisualMoveDirection::Down => {
            let next = current_index + 1;
            (next < rows.len()).then_some(next)?
        }
    };
    let target = &rows[target_index];
    let stop = target.stops.iter().copied().min_by(|a, b| {
        (a.x_pt - preferred_x).abs().total_cmp(&(b.x_pt - preferred_x).abs())
            .then_with(|| a.byte_offset.cmp(&b.byte_offset))
    })?;
    Some(VisualCaretTarget {
        position: SourcePosition {
            node_id: target.node_id.to_string(),
            byte_offset: stop.byte_offset,
        },
        preferred_x_pt: preferred_x,
        visual_line: target.visual_line,
    })
}

/// Return the laid-out x coordinate of a semantic caret, useful when a front
/// end wants to seed or reset vertical-column affinity.
pub fn visual_caret_x(document: &VectorDocument, position: &SourcePosition) -> Option<f32> {
    let rows = collect_rows(document);
    let row = rows.iter().find(|row| row_contains(row, position))?;
    Some(x_for_offset(row, position.byte_offset))
}

fn row_contains(row: &VisualRow<'_>, position: &SourcePosition) -> bool {
    row.node_id == position.node_id
        && position.byte_offset >= row.min_offset
        && position.byte_offset <= row.max_offset
}

fn x_for_offset(row: &VisualRow<'_>, offset: usize) -> f32 {
    row.stops.iter().copied()
        .min_by(|a, b| {
            a.byte_offset.abs_diff(offset).cmp(&b.byte_offset.abs_diff(offset))
                .then_with(|| a.x_pt.total_cmp(&b.x_pt))
        })
        .map(|s| s.x_pt)
        .unwrap_or(0.0)
}

fn collect_rows(document: &VectorDocument) -> Vec<VisualRow<'_>> {
    let mut rows: Vec<VisualRow<'_>> = Vec::new();
    for page in &document.pages {
        for region in &page.hit_regions {
            let same_row = rows.last().is_some_and(|row| {
                row.node_id == region.node_id && row.visual_line == region.visual_line
            });
            if same_row {
                let row = rows.last_mut().expect("checked above");
                row.min_offset = row.min_offset.min(region.byte_range.start);
                row.max_offset = row.max_offset.max(region.byte_range.end);
                row.stops.extend(region.caret_stops.iter());
            } else {
                rows.push(row_from_region(region));
            }
        }
    }
    for row in &mut rows {
        row.stops.sort_by(|a, b| a.x_pt.total_cmp(&b.x_pt).then_with(|| a.byte_offset.cmp(&b.byte_offset)));
        row.stops.dedup_by(|a, b| a.byte_offset == b.byte_offset && (a.x_pt - b.x_pt).abs() < 0.001);
    }
    rows
}

fn row_from_region(region: &TextHitRegion) -> VisualRow<'_> {
    VisualRow {
        node_id: &region.node_id,
        visual_line: region.visual_line,
        min_offset: region.byte_range.start,
        max_offset: region.byte_range.end,
        stops: region.caret_stops.iter().collect(),
    }
}

