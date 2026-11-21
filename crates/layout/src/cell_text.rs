//! Shape sideways table-cell text with the ordinary line breaker, then turn
//! the resulting glyphs and hit regions into the cell's writing direction.
use super::{Affine2D, DrawCommand, PreparedCellFragment, Rect};
use model::CellTextDirection;
use render::Point;
use std::sync::Arc;

pub(super) fn rotate(
    mut fragment: PreparedCellFragment,
    direction: CellTextDirection,
    column_width: f32,
    line_width: f32,
) -> Option<PreparedCellFragment> {
    if !fragment.object_regions.is_empty() { return None; }
    let transform = match direction {
        CellTextDirection::BottomToTop => Affine2D { sx: 0.0, kx: 1.0, tx: 0.0, ky: -1.0, sy: 0.0, ty: line_width },
        CellTextDirection::TopToBottom => Affine2D { sx: 0.0, kx: -1.0, tx: column_width, ky: 1.0, sy: 0.0, ty: 0.0 },
    };
    for command in &mut fragment.commands { rotate_command(command, transform)?; }
    for hit in &mut fragment.hit_regions {
        hit.rect = map_rect(hit.rect, transform);
        hit.vertical = true;
        for stop in &mut hit.caret_stops {
            stop.x_pt = transform.map_point(Point::new(stop.x_pt, 0.0)).y;
        }
    }
    fragment.height = line_width;
    Some(fragment)
}

fn rotate_command(command: &mut DrawCommand, transform: Affine2D) -> Option<()> {
    match command {
        DrawCommand::Clipped { rect, command } => {
            *rect = map_rect(*rect, transform);
            rotate_command(Arc::make_mut(command), transform)?;
        }
        DrawCommand::FillRect { rect, .. } => *rect = map_rect(*rect, transform),
        DrawCommand::Path(path) => path.transform = path.transform.then(transform),
        DrawCommand::TextRun(run) => {
            let run = Arc::make_mut(run);
            run.bounds = map_rect(run.bounds, transform);
            for glyph in &mut run.glyphs { glyph.transform = glyph.transform.then(transform); }
        }
        DrawCommand::Image { .. } | DrawCommand::EncodedImage { .. } => return None,
    }
    Some(())
}

fn map_rect(rect: Rect, transform: Affine2D) -> Rect {
    let first = transform.map_point(Point::new(rect.x, rect.y));
    let last = transform.map_point(Point::new(rect.max_x(), rect.max_y()));
    Rect::new(first.x.min(last.x), first.y.min(last.y), (last.x - first.x).abs(), (last.y - first.y).abs())
}
