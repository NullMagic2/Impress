use crate::{Point, Rect, Size, TextHitRegion, VectorDocument};
use std::ops::Range;

const PT_PER_INCH: f32 = 72.0;
const DEFAULT_DPI: f32 = 96.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewportState {
    pub viewport_px: Size,
    pub dpi: f32,
    pub zoom: f32,
    pub scroll_px: Point,
    pub page_gap_pt: f32,
    pub outer_margin_px: f32,
    /// Extra horizontal scrolling room for attached review notes.
    pub right_inset_px: f32,
}

impl Default for ViewportState {
    fn default() -> Self {
        Self {
            viewport_px: Size::new(800.0, 600.0),
            dpi: DEFAULT_DPI,
            zoom: 1.0,
            scroll_px: Point::new(0.0, 0.0),
            page_gap_pt: 22.0,
            outer_margin_px: 32.0,
            right_inset_px: 0.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DocumentHit {
    pub page_index: usize,
    pub node_id: String,
    pub byte_offset: usize,
    /// Visual row selected by the pointer. This disambiguates wrap boundaries
    /// where one semantic offset is a valid caret stop on two adjacent lines.
    pub visual_line: usize,
}

/// Maximum resident page drawings, except when more pages are visible at once.
pub const PAGE_WINDOW_SIZE: usize = 20;

/// The vertical slice on screen. When the viewport is in a page gap, use the
/// following page (or the last one at the bottom).
pub fn pages_in_view(pages: &[Rect], view: Rect) -> Range<usize> {
    if pages.is_empty() { return 0..0; }
    let first = pages.partition_point(|page| page.y + page.height.ceil().max(1.0) <= view.y)
        .min(pages.len() - 1);
    let end = pages.partition_point(|page| page.y < view.max_y()).max(first + 1);
    first..end
}

/// Borrow unused capacity from the other end at the beginning and end.
pub fn page_cache_window(pages: &[Rect], view: Rect) -> Range<usize> {
    if pages.len() <= PAGE_WINDOW_SIZE { return 0..pages.len(); }
    let visible = pages_in_view(pages, view);
    let count = PAGE_WINDOW_SIZE.max(visible.len());
    let before = (count - visible.len()) / 2;
    let start = visible.start.saturating_sub(before).min(pages.len() - count);
    start..start + count
}

impl ViewportState {
    /// Supported document zoom range: 20% through 800%.
    pub const MIN_ZOOM:f32=0.2;
    pub const MAX_ZOOM:f32=8.0;

    pub fn points_to_pixels(&self) -> f32 { (self.dpi / PT_PER_INCH) * self.zoom }

    pub fn set_viewport_px(&mut self, width: f32, height: f32) {
        self.viewport_px = Size::new(width.max(1.0), height.max(1.0));
    }

    pub fn scroll_by(&mut self, dx_px: f32, dy_px: f32, document: &VectorDocument) {
        self.scroll_px.x += dx_px;
        self.scroll_px.y += dy_px;
        self.clamp_scroll(document);
    }

    pub fn zoom_at(&mut self, anchor_px: Point, requested_zoom: f32, document: &VectorDocument) {
        if !requested_zoom.is_finite() { return; }
        let old_scale = self.points_to_pixels();
        let world_x = (self.scroll_px.x + anchor_px.x - self.horizontal_origin_px(document)) / old_scale.max(0.0001);
        let world_y = (self.scroll_px.y + anchor_px.y - self.outer_margin_px) / old_scale.max(0.0001);
        self.zoom = requested_zoom.clamp(Self::MIN_ZOOM, Self::MAX_ZOOM);
        let new_scale = self.points_to_pixels();
        self.scroll_px.x = self.horizontal_origin_px(document) + world_x * new_scale - anchor_px.x;
        self.scroll_px.y = self.outer_margin_px + world_y * new_scale - anchor_px.y;
        self.clamp_scroll(document);
    }

    pub fn canvas_size_px(&self, document: &VectorDocument) -> Size {
        let scale = self.points_to_pixels();
        let mut max_width = 0.0_f32;
        let mut height = self.outer_margin_px * 2.0;
        for (i, page) in document.pages.iter().enumerate() {
            max_width = max_width.max(page.size.width * scale);
            height += page.size.height * scale;
            if i + 1 < document.pages.len() { height += self.page_gap_pt * scale; }
        }
        Size::new(max_width + self.outer_margin_px * 2.0 + self.right_inset_px, height)
    }

    /// Center the whole page stack in spare viewport space. At high zoom the
    /// normal outer margin becomes the scroll origin instead of a negative x.
    fn horizontal_origin_px(&self, document: &VectorDocument) -> f32 {
        let canvas = self.canvas_size_px(document);
        self.outer_margin_px + ((self.viewport_px.width - canvas.width) * 0.5).max(0.0)
    }

    /// View rectangles of every page, in one pass. Page origins are whole
    /// pixels so that cached page rasters line up with the pixel grid; every
    /// consumer (painting, overlays, hit testing) uses these same rectangles.
    pub fn page_rects_px(&self, document: &VectorDocument) -> Vec<Rect> {
        let scale = self.points_to_pixels();
        let max_width = document.pages.iter().map(|p| p.size.width).fold(0.0_f32, f32::max) * scale;
        // Round the scrolled origin and each page's offset separately, so a
        // scroll moves every page by the same whole number of pixels.
        let origin_x = (self.horizontal_origin_px(document) - self.scroll_px.x).round();
        let origin_y = (self.outer_margin_px - self.scroll_px.y).round();
        let mut offset = 0.0_f32;
        document.pages.iter().map(|page| {
            let width = page.size.width * scale;
            let rect = Rect::new(origin_x + ((max_width - width) * 0.5).round(), origin_y + offset.round(),
                width, page.size.height * scale);
            offset += (page.size.height + self.page_gap_pt) * scale;
            rect
        }).collect()
    }

    pub fn page_rect_px(&self, document: &VectorDocument, page_index: usize) -> Option<Rect> {
        document.pages.get(page_index)?;
        self.page_rects_px(document).get(page_index).copied()
    }

    pub fn page_point_to_view(&self, document: &VectorDocument, page_index: usize, p: Point) -> Option<Point> {
        let rect = self.page_rect_px(document, page_index)?;
        let scale = self.points_to_pixels();
        Some(Point::new(rect.x + p.x * scale, rect.y + p.y * scale))
    }

    pub fn visible_pages(&self, document: &VectorDocument) -> Vec<usize> {
        let viewport = Rect::new(0.0, 0.0, self.viewport_px.width, self.viewport_px.height);
        self.page_rects_px(document).into_iter().enumerate().filter(|(_, rect)|
            rect.x < viewport.max_x() && rect.max_x() > viewport.x
                && rect.y < viewport.max_y() && rect.max_y() > viewport.y
        ).map(|(index, _)| index).collect()
    }

    pub fn hit_test(&self, document: &VectorDocument, view_point: Point) -> Option<DocumentHit> {
        let scale = self.points_to_pixels();
        let rects = self.page_rects_px(document);
        let (page_index, page_rect) = rects.iter().copied().enumerate().find(|(_, rect)| contains(*rect, view_point))?;
        let page = &document.pages[page_index];
        let page_point = Point::new((view_point.x - page_rect.x) / scale, (view_point.y - page_rect.y) / scale);
        // Inside a table cell, the nearest text of that cell wins, so a click
        // in a cell's empty space never lands in the paragraph around the table.
        let cell = page.object_regions.iter()
            .filter(|r| matches!(r.kind, crate::ObjectKind::TableCell(_)) && distance_to_rect(r.rect, page_point) == 0.0)
            .min_by(|a, b| (a.rect.width * a.rect.height).total_cmp(&(b.rect.width * b.rect.height)));
        let inside: Vec<TextHitRegion> = cell.map(|c| page.hit_regions.iter().filter(|r| {
            let (x, y) = (r.rect.x + r.rect.width * 0.5, r.rect.y + r.rect.height * 0.5);
            x >= c.rect.x && x <= c.rect.max_x() && y >= c.rect.y && y <= c.rect.max_y()
        }).cloned().collect()).unwrap_or_default();
        let region = if cell.is_some() {
            nearest_region(&inside, page_point)?.clone()
        } else {
            // Blank page space must not reactivate a table by snapping to
            // its nearest cell. Dragging still uses hit_test_nearest below.
            let outside = page.hit_regions.iter().filter(|region| {
                let center = Point::new(region.rect.x + region.rect.width * 0.5, region.rect.y + region.rect.height * 0.5);
                !page.object_regions.iter().any(|object| matches!(object.kind, crate::ObjectKind::TableCell(_)) && contains(object.rect, center))
            });
            nearest_region(outside, page_point)?.clone()
        };
        let region = &region;
        Some(DocumentHit {
            page_index,
            node_id: region.node_id.clone(),
            byte_offset: region.nearest_byte_offset(page_point.x),
            visual_line: region.visual_line,
        })
    }

    /// During a selection drag, the pointer may enter the page margin or the
    /// gap between pages. Continue the selection at the nearest text edge.
    pub fn hit_test_nearest(&self, document: &VectorDocument, view_point: Point) -> Option<DocumentHit> {
        if let Some(hit) = self.hit_test(document, view_point) { return Some(hit); }
        let rects = self.page_rects_px(document);
        let (page_index, page_rect) = rects.iter().copied().enumerate()
            .filter(|(index, _)| !document.pages[*index].hit_regions.is_empty())
            .min_by(|(_, a), (_, b)| distance_to_rect(*a, view_point).total_cmp(&distance_to_rect(*b, view_point)))?;
        let page = &document.pages[page_index];
        let scale = self.points_to_pixels();
        let point = Point::new(
            (view_point.x.clamp(page_rect.x, page_rect.max_x()) - page_rect.x) / scale,
            (view_point.y.clamp(page_rect.y, page_rect.max_y()) - page_rect.y) / scale,
        );
        let region = nearest_region(&page.hit_regions, point)?;
        Some(DocumentHit {
            page_index,
            node_id: region.node_id.clone(),
            byte_offset: region.nearest_byte_offset(point.x),
            visual_line: region.visual_line,
        })
    }

    /// A selection drag that began in table cell `cell_id` keeps to its text.
    /// A cell broken across pages has a part on each page; the part nearest
    /// the pointer is used, so the selection runs on from one page into the
    /// next, and the pointer in a margin or between pages stays in the cell.
    pub fn hit_test_in_cell(&self, document: &VectorDocument, view_point: Point, cell_id: &str) -> Option<DocumentHit> {
        let scale = self.points_to_pixels();
        let rects = self.page_rects_px(document);
        let (page_index, part, view_part) = document.pages.iter().zip(&rects).enumerate()
            .flat_map(|(index, (page, page_rect))| page.object_regions.iter()
                .filter(|r| r.node_id == cell_id && matches!(r.kind, crate::ObjectKind::TableCell(_)))
                .map(move |r| (index, r.rect, Rect::new(page_rect.x + r.rect.x * scale, page_rect.y + r.rect.y * scale, r.rect.width * scale, r.rect.height * scale))))
            .min_by(|a, b| distance_to_rect(a.2, view_point).total_cmp(&distance_to_rect(b.2, view_point)))?;
        let page_rect = rects[page_index];
        let point = Point::new(
            (view_point.x.clamp(view_part.x, view_part.max_x()) - page_rect.x) / scale,
            (view_point.y.clamp(view_part.y, view_part.max_y()) - page_rect.y) / scale,
        );
        let inside: Vec<&TextHitRegion> = document.pages[page_index].hit_regions.iter().filter(|r| {
            let (x, y) = (r.rect.x + r.rect.width * 0.5, r.rect.y + r.rect.height * 0.5);
            x >= part.x && x <= part.max_x() && y >= part.y && y <= part.max_y()
        }).collect();
        let region = nearest_region(inside, point)?;
        Some(DocumentHit {
            page_index,
            node_id: region.node_id.clone(),
            byte_offset: region.nearest_byte_offset(point.x),
            visual_line: region.visual_line,
        })
    }

    pub fn clamp_scroll(&mut self, document: &VectorDocument) {
        let canvas = self.canvas_size_px(document);
        let max_x = (canvas.width - self.viewport_px.width).max(0.0);
        let max_y = (canvas.height - self.viewport_px.height).max(0.0);
        self.scroll_px.x = self.scroll_px.x.clamp(0.0, max_x);
        self.scroll_px.y = self.scroll_px.y.clamp(0.0, max_y);
    }
}

fn contains(rect: Rect, p: Point) -> bool {
    p.x >= rect.x && p.x <= rect.max_x() && p.y >= rect.y && p.y <= rect.max_y()
}

/// The text nearest `p`, picked as a word processor does: first the line at
/// the pointer's height (the nearest one above or below between lines), then
/// the place along it. When several lines share that height (columns, table
/// cells), the one whose paragraph spans the pointer's x wins, then the
/// nearest. Plain distance picked the long line above when the pointer was
/// past the end of a paragraph's short last line.
fn nearest_region<'a>(regions: impl IntoIterator<Item = &'a TextHitRegion>, p: Point) -> Option<&'a TextHitRegion> {
    let regions: Vec<&TextHitRegion> = regions.into_iter().collect();
    let dy = |r: &TextHitRegion| if p.y < r.rect.y { r.rect.y - p.y } else if p.y > r.rect.max_y() { p.y - r.rect.max_y() } else { 0.0 };
    let dx = |r: &TextHitRegion| if p.x < r.rect.x { r.rect.x - p.x } else if p.x > r.rect.max_x() { p.x - r.rect.max_x() } else { 0.0 };
    let nearest_dy = regions.iter().map(|r| dy(r)).fold(f32::INFINITY, f32::min);
    // A paragraph's horizontal extent on the page: the union of its lines.
    let spans_x = |r: &TextHitRegion| {
        let (left, right) = regions.iter().filter(|o| o.node_id == r.node_id)
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(l, h), o| (l.min(o.rect.x), h.max(o.rect.max_x())));
        p.x >= left && p.x <= right
    };
    regions.iter().copied().filter(|r| dy(r) <= nearest_dy + 0.5)
        .min_by(|a, b| spans_x(b).cmp(&spans_x(a))
            .then_with(|| dx(a).total_cmp(&dx(b)))
            .then_with(|| (a.rect.width * a.rect.height).total_cmp(&(b.rect.width * b.rect.height))))
}

fn distance_to_rect(rect: Rect, p: Point) -> f32 {
    let dx = if p.x < rect.x { rect.x - p.x } else if p.x > rect.max_x() { p.x - rect.max_x() } else { 0.0 };
    let dy = if p.y < rect.y { rect.y - p.y } else if p.y > rect.max_y() { p.y - rect.max_y() } else { 0.0 };
    (dx * dx + dy * dy).sqrt()
}

#[cfg(test)] #[path = "../tests/unit/viewport.rs"] mod tests;
