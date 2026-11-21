//! Positions nested tables in their containing cell and reserves room for cell text.
use super::{MM_TO_PT, Rect};
use model::{HorizontalPlacement, TablePosition, TableWrap, VerticalFrom, VerticalPlacement};

pub(super) struct Flow {
    width: f32,
    minimum_height: f32,
    tables: Vec<PlacedTable>,
}

struct PlacedTable {
    exclusion: Rect,
    allow_overlap: bool,
    wrap: TableWrap,
}

impl Flow {
    pub(super) fn new(width: f32, minimum_height: f32) -> Self {
        Self { width, minimum_height, tables: Vec::new() }
    }

    pub(super) fn place(&mut self, position: &TablePosition, width: f32, height: f32, y: f32) -> Rect {
        // A nested table's page/margin offsets are local to the cell. Confine
        // its edges to that cell instead of clipping parts of the nested grid.
        let spare = (self.width - width).max(0.0);
        let x = match position.horizontal {
            HorizontalPlacement::Offset(offset) => offset * MM_TO_PT,
            HorizontalPlacement::Center => spare * 0.5,
            HorizontalPlacement::Right | HorizontalPlacement::Outside => spare,
            HorizontalPlacement::Left | HorizontalPlacement::Inside => 0.0,
        }.clamp(0.0, spare);
        let anchor_y = if position.vertical_from == VerticalFrom::Paragraph { y } else { 0.0 };
        let spare_height = (self.minimum_height - anchor_y - height).max(0.0);
        let top = match position.vertical {
            VerticalPlacement::Offset(offset) => offset * MM_TO_PT,
            VerticalPlacement::Top => 0.0,
            VerticalPlacement::Center => spare_height * 0.5,
            VerticalPlacement::Bottom => spare_height,
        };
        let left_gap = position.distance_left_mm * MM_TO_PT;
        let right_gap = position.distance_right_mm * MM_TO_PT;
        let top_gap = position.distance_top_mm * MM_TO_PT;
        let bottom_gap = position.distance_bottom_mm * MM_TO_PT;
        let mut bounds = Rect::new(x, (anchor_y + top).max(y), width, height);
        let exclusion = loop {
            let exclusion = Rect::new(x - left_gap, bounds.y - top_gap,
                width + left_gap + right_gap, height + top_gap + bottom_gap);
            let next = self.tables.iter().filter(|table| {
                !(position.allow_overlap && table.allow_overlap)
                    && !exclusion.intersection(table.exclusion).is_empty()
            }).map(|table| table.exclusion.max_y() + top_gap).fold(bounds.y, f32::max);
            if next <= bounds.y + 0.001 { break exclusion; }
            bounds.y = next;
        };
        self.tables.push(PlacedTable { exclusion, allow_overlap: position.allow_overlap, wrap: position.wrap });
        bounds
    }

    pub(super) fn clear(&self, y: f32) -> f32 {
        self.tables.iter().map(|table| table.exclusion.max_y()).fold(y, f32::max)
    }

    pub(super) fn paragraph_span(&self, y: &mut f32) -> (f32, f32) {
        let minimum_width = self.width.min(24.0);
        loop {
            let mut spans = vec![(0.0, self.width)];
            let mut next = f32::INFINITY;
            // Reserve a single horizontal span for the paragraph. A table
            // starting partway down it must also be excluded.
            for table in self.tables.iter().filter(|table| table.exclusion.max_y() > *y) {
                next = next.min(table.exclusion.max_y());
                let (blocked_left, blocked_right) = match table.wrap {
                    TableWrap::Around => (table.exclusion.x, table.exclusion.max_x()),
                    TableWrap::TopAndBottom => (0.0, self.width),
                };
                spans = spans.into_iter().flat_map(|(left, right)| {
                    if blocked_right <= left || blocked_left >= right {
                        [Some((left, right)), None]
                    } else {
                        [(left < blocked_left).then_some((left, blocked_left)),
                         (right > blocked_right).then_some((blocked_right, right))]
                    }
                }).flatten().collect();
            }
            if let Some((left, right)) = spans.into_iter().filter(|(left, right)| right - left >= minimum_width)
                .max_by(|a, b| (a.1 - a.0).total_cmp(&(b.1 - b.0))) {
                return (left, right - left);
            }
            *y = next;
        }
    }
}
