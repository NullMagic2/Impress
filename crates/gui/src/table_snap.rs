//! Gentle screen-distance snapping to the same margins used by the rulers.
use render::{Point, Rect, VectorDocument, ViewportState};
pub const TOLERANCE_DIP: f32 = 6.0;
#[derive(Clone, Copy, Debug)]
pub struct Snapped {
    pub rect: Rect,
    pub x: Option<f32>,
    pub y: Option<f32>,
    pub margins: Rect,
}

pub fn margins(document: &VectorDocument, viewport: &ViewportState, at: Point) -> Option<Rect> {
    let distance = |rect: Rect| {
        let dx = (rect.x - at.x).max(at.x - rect.max_x()).max(0.0);
        let dy = (rect.y - at.y).max(at.y - rect.max_y()).max(0.0);
        dx * dx + dy * dy
    };
    let (index, page) = viewport
        .page_rects_px(document)
        .into_iter()
        .enumerate()
        .min_by(|a, b| distance(a.1).total_cmp(&distance(b.1)))?;
    let m = document.pages[index].margin_rect?;
    let scale = viewport.points_to_pixels();
    Some(Rect::new(
        page.x + m.x * scale,
        page.y + m.y * scale,
        m.width * scale,
        m.height * scale,
    ))
}
fn correction(candidates: [(f32, f32); 2], tolerance: f32) -> Option<(f32, f32)> {
    candidates
        .into_iter()
        .filter(|(delta, _)| delta.abs() <= tolerance)
        .min_by(|a, b| a.0.abs().total_cmp(&b.0.abs()))
}
pub fn edge(value: f32, targets: [f32; 2], tolerance: f32) -> f32 {
    correction(targets.map(|target| (target - value, target)), tolerance)
        .map_or(value, |(_, target)| target)
}
pub fn moving(mut rect: Rect, margins: Rect, tolerance: f32, vertical: bool) -> Snapped {
    let x = correction(
        [
            (margins.x - rect.x, margins.x),
            (margins.max_x() - rect.max_x(), margins.max_x()),
        ],
        tolerance,
    );
    if let Some((delta, _)) = x {
        rect.x += delta;
    }
    let y = vertical
        .then(|| {
            correction(
                [
                    (margins.y - rect.y, margins.y),
                    (margins.max_y() - rect.max_y(), margins.max_y()),
                ],
                tolerance,
            )
        })
        .flatten();
    if let Some((delta, _)) = y {
        rect.y += delta;
    }
    Snapped {
        rect,
        x: x.map(|(_, target)| target),
        y: y.map(|(_, target)| target),
        margins,
    }
}
pub fn resizing(mut rect: Rect, margins: Rect, tolerance: f32, proportional: bool) -> Snapped {
    let dx = margins.max_x() - rect.max_x();
    let dy = margins.max_y() - rect.max_y();
    let mut x = None;
    let mut y = None;
    if proportional {
        // Choose the closer edge, keeping both dimensions in one ratio.
        let choice = [(dx, true), (dy, false)]
            .into_iter()
            .filter(|(delta, _)| delta.abs() <= tolerance)
            .filter_map(|(delta, horizontal)| {
                let factor = if horizontal {
                    (rect.width + delta) / rect.width
                } else {
                    (rect.height + delta) / rect.height
                };
                (factor.is_finite() && factor > 0.0).then_some((delta, horizontal, factor))
            })
            .min_by(|a, b| a.0.abs().total_cmp(&b.0.abs()));
        if let Some((_, horizontal, factor)) = choice {
            rect.width *= factor;
            rect.height *= factor;
            if horizontal {
                x = Some(margins.max_x());
            } else {
                y = Some(margins.max_y());
            }
        }
    } else {
        if dx.abs() <= tolerance && rect.width + dx > 0.0 {
            rect.width += dx;
            x = Some(margins.max_x());
        }
        if dy.abs() <= tolerance && rect.height + dy > 0.0 {
            rect.height += dy;
            y = Some(margins.max_y());
        }
    }
    Snapped {
        rect,
        x,
        y,
        margins,
    }
}
#[cfg(test)]
#[path = "../tests/unit/table_snap.rs"]
mod tests;
