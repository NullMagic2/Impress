//! Rulers use exactly the same rounded page rectangles as the canvas.
use render::{Point, Rect, VectorDocument, ViewportState};

#[derive(Clone, Debug, PartialEq)]
pub(super) struct PageScale {
    pub index: usize,
    /// Physical sheet and its margin boundaries in ruler client coordinates.
    pub paper: Rect,
    pub margins: Rect,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct RulerPages {
    pub pages: Vec<PageScale>,
    pub primary: usize,
    pub pixels_per_point: f32,
}

impl RulerPages {
    /// `offset` translates canvas client coordinates into the two ruler strips:
    /// x is relative to the horizontal strip, y to the vertical strip.
    pub fn new(
        document: &VectorDocument,
        viewport: &ViewportState,
        screen: f32,
        offset: Point,
    ) -> Option<Self> {
        let scale = viewport.points_to_pixels();
        let screen = screen.max(0.01);
        let mut largest = 0.0_f32;
        let mut primary = 0;
        let pages: Vec<_> = viewport
            .page_rects_px(document)
            .into_iter()
            .enumerate()
            .filter_map(|(index, rect)| {
                let visible_width = rect.max_x().min(viewport.viewport_px.width) - rect.x.max(0.0);
                let visible_height =
                    rect.max_y().min(viewport.viewport_px.height) - rect.y.max(0.0);
                if visible_width <= 0.0 || visible_height <= 0.0 {
                    return None;
                }
                let area = visible_width * visible_height;
                if area > largest {
                    largest = area;
                    primary = index;
                }
                let page = &document.pages[index];
                let margins = page.margin_rect.unwrap_or(Rect::new(
                    0.0,
                    0.0,
                    page.size.width,
                    page.size.height,
                ));
                Some(PageScale {
                    index,
                    paper: Rect::new(
                        offset.x + rect.x / screen,
                        offset.y + rect.y / screen,
                        rect.width / screen,
                        rect.height / screen,
                    ),
                    margins: Rect::new(
                        offset.x + (rect.x + margins.x * scale) / screen,
                        offset.y + (rect.y + margins.y * scale) / screen,
                        margins.width * scale / screen,
                        margins.height * scale / screen,
                    ),
                })
            })
            .collect();
        (!pages.is_empty()).then_some(Self {
            pages,
            primary,
            pixels_per_point: scale / screen,
        })
    }

    pub fn primary(&self) -> &PageScale {
        self.pages
            .iter()
            .find(|page| page.index == self.primary)
            .expect("primary is visible")
    }
}

#[cfg(test)]
#[path = "../../tests/unit/ruler_geometry.rs"]
mod tests;
