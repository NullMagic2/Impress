//! Selection and caret geometry derived from vector text hit regions.
//!
//! The editor owns semantic selections. This module converts a compact source
//! selection description into page-space rectangles that any renderer can draw.

use crate::{CanvasOverlay, OverlayRect, Rect, VectorDocument};
use std::{collections::HashMap, ops::Range};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourcePosition {
    pub node_id: String,
    pub byte_offset: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceSelection {
    pub anchor: SourcePosition,
    pub focus: SourcePosition,
}

impl SourceSelection {
    pub fn collapsed(position: SourcePosition) -> Self {
        Self { anchor: position.clone(), focus: position }
    }

    pub fn is_collapsed(&self) -> bool { self.anchor == self.focus }
}

pub fn overlay_for_selection(
    document: &VectorDocument,
    selection: Option<&SourceSelection>,
    caret_width_pt: f32,
) -> CanvasOverlay {
    overlay_for_selection_with_caret_line(document, selection, caret_width_pt, None)
}

/// Convert a semantic selection to page overlay geometry while preserving an
/// optional wrapped-line affinity for a collapsed caret. The affinity matters
/// only at soft-wrap boundaries, where the same byte offset belongs to both
/// the previous line end and the next line start.
pub fn overlay_for_selection_with_caret_line(
    document: &VectorDocument,
    selection: Option<&SourceSelection>,
    caret_width_pt: f32,
    caret_visual_line: Option<usize>,
) -> CanvasOverlay {
    let Some(selection) = selection else { return CanvasOverlay::default(); };

    if selection.is_collapsed() {
        return caret_overlay(document, &selection.focus, caret_width_pt, caret_visual_line);
    }

    if selection.anchor.node_id == selection.focus.node_id {
        let start = selection.anchor.byte_offset.min(selection.focus.byte_offset);
        let end = selection.anchor.byte_offset.max(selection.focus.byte_offset);
        return CanvasOverlay {
            selections: document.pages.iter().enumerate().flat_map(|(page, content)| {
                content.hit_regions.iter().filter(|r| r.node_id == selection.anchor.node_id)
                    .filter_map(move |r| text_range_rect(page, r, start..end))
            }).collect(),
            ..Default::default()
        };
    }

    let order = node_orders(document);
    let Some(&anchor_order) = order.get(selection.anchor.node_id.as_str()) else {
        return CanvasOverlay::default();
    };
    let Some(&focus_order) = order.get(selection.focus.node_id.as_str()) else {
        return CanvasOverlay::default();
    };

    let anchor_key = (anchor_order, selection.anchor.byte_offset);
    let focus_key = (focus_order, selection.focus.byte_offset);
    let (start, end) = if anchor_key <= focus_key {
        (&selection.anchor, &selection.focus)
    } else {
        (&selection.focus, &selection.anchor)
    };
    let start_order = *order.get(start.node_id.as_str()).unwrap_or(&anchor_order);
    let end_order = *order.get(end.node_id.as_str()).unwrap_or(&focus_order);

    let mut overlay = CanvasOverlay::default();
    for (page_index, page) in document.pages.iter().enumerate() {
        for region in &page.hit_regions {
            let Some(&region_order) = order.get(region.node_id.as_str()) else { continue; };
            if region_order < start_order || region_order > end_order { continue; }

            let region_start = if region.node_id == start.node_id {
                start.byte_offset.max(region.byte_range.start)
            } else {
                region.byte_range.start
            };
            let region_end = if region.node_id == end.node_id {
                end.byte_offset.min(region.byte_range.end)
            } else {
                region.byte_range.end
            };
            overlay.selections.extend(text_range_rect(page_index, region, region_start..region_end));
        }
    }
    overlay
}

/// Clip a byte range to one shaped run and return its highlight in page points.
pub fn text_range_rect(page_index: usize, region: &crate::TextHitRegion, range: Range<usize>) -> Option<OverlayRect> {
    let start = range.start.max(region.byte_range.start);
    let end = range.end.min(region.byte_range.end);
    if start >= end { return None; }
    let x0 = x_for_offset(region, start);
    let x1 = x_for_offset(region, end);
    Some(OverlayRect {
        page_index,
        rect: Rect::new(x0.min(x1), region.rect.y, (x1 - x0).abs().max(0.5), region.rect.height),
    })
}

fn node_orders(document: &VectorDocument) -> HashMap<&str, (usize, usize)> {
    // Paragraphs the document lists keep its order; anything else (generated
    // text) follows, in page order.
    let mut orders = HashMap::new();
    let mut next = 0usize;
    let mut previous: Option<&str> = None;
    for page in &document.pages {
        for region in &page.hit_regions {
            if previous != Some(region.node_id.as_str()) {
                let key = document.text_order.get(&region.node_id).map_or((1, next), |&i| (0, i));
                orders.entry(region.node_id.as_str()).or_insert(key);
                next += 1;
                previous = Some(region.node_id.as_str());
            }
        }
    }
    orders
}

fn caret_overlay(
    document: &VectorDocument,
    position: &SourcePosition,
    caret_width_pt: f32,
    preferred_visual_line: Option<usize>,
) -> CanvasOverlay {
    if let Some(line) = preferred_visual_line {
        if let Some(overlay) = caret_overlay_matching(document, position, caret_width_pt, Some(line)) {
            return overlay;
        }
    }
    caret_overlay_matching(document, position, caret_width_pt, None).unwrap_or_default()
}

fn caret_overlay_matching(
    document: &VectorDocument,
    position: &SourcePosition,
    caret_width_pt: f32,
    preferred_visual_line: Option<usize>,
) -> Option<CanvasOverlay> {
    for (page_index, page) in document.pages.iter().enumerate() {
        for region in &page.hit_regions {
            if region.node_id != position.node_id { continue; }
            if preferred_visual_line.is_some_and(|line| region.visual_line != line) { continue; }
            if position.byte_offset < region.byte_range.start || position.byte_offset > region.byte_range.end { continue; }
            let x = x_for_offset(region, position.byte_offset);
            return Some(CanvasOverlay {
                selections: Vec::new(),
                adornments: Vec::new(),
                review_marks: Vec::new(),
                chrome: Vec::new(),
                caret: Some(OverlayRect {
                    page_index,
                    rect: Rect::new(x, region.rect.y, caret_width_pt.max(0.5), region.rect.height),
                }),
            });
        }
    }
    None
}

fn x_for_offset(region: &crate::TextHitRegion, offset: usize) -> f32 {
    region.caret_stops.iter()
        .min_by_key(|stop| stop.byte_offset.abs_diff(offset))
        .map(|stop| stop.x_pt)
        .unwrap_or(region.rect.x)
}

#[cfg(test)] #[path = "../tests/unit/overlay.rs"] mod tests;

/// A physical-page affinity for repeating header/footer source streams.
pub fn caret_on_page(document: &VectorDocument, position: &SourcePosition, page_index: usize, line: Option<usize>) -> Option<OverlayRect> {
    let page = document.pages.get(page_index)?;
    let matches = |r: &&crate::TextHitRegion| r.node_id == position.node_id
        && (r.byte_range.start..=r.byte_range.end).contains(&position.byte_offset);
    let region = page.hit_regions.iter().filter(matches).find(|r| line.is_some_and(|l| r.visual_line == l))
        .or_else(|| page.hit_regions.iter().find(matches))?;
    Some(OverlayRect { page_index, rect: Rect::new(x_for_offset(region, position.byte_offset), region.rect.y, 0.85, region.rect.height) })
}
