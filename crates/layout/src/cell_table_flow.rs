//! Positions nested tables in their containing cell and reserves room for cell text.
use super::{MM_TO_PT, Rect};
use model::{HorizontalPlacement, TablePosition, TableWrap, VerticalFrom, VerticalPlacement};

pub(super) struct Flow {
    width: f32,
    minimum_height: f32,
    tables: Vec<PlacedTable>,
}

struct PlacedTable {
    bounds: Rect,
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
        let offset = position.visual_offset.unwrap_or_default();
        let x = (x + offset.x_mm * MM_TO_PT).clamp(0.0, spare);
        let mut bounds = Rect::new(x, (anchor_y + top + offset.y_mm * MM_TO_PT).max(0.0), width, height);
        let exclusion = loop {
            let exclusion = Rect::new(x - left_gap, bounds.y - top_gap,
                width + left_gap + right_gap, height + top_gap + bottom_gap);
            let next = self.tables.iter().filter(|table| {
                !(position.allow_overlap && table.allow_overlap)
                    && !bounds.intersection(table.bounds).is_empty()
            }).map(|table| table.bounds.max_y()).fold(bounds.y, f32::max);
            if next <= bounds.y + 0.001 { break exclusion; }
            bounds.y = next;
        };
        self.tables.push(PlacedTable { bounds, exclusion, allow_overlap: position.allow_overlap, wrap: position.wrap });
        bounds
    }

    pub(super) fn clear(&self, y: f32) -> f32 {
        self.tables.iter().filter(|table| !table.wrap.overlaps_text()).map(|table| table.exclusion.max_y()).fold(y, f32::max)
    }

    /// Fit all floating content inside the owning cell without using it as
    /// a text-flow obstacle. This is separate from clearing wrapped objects.
    pub(super) fn extent(&self, y: f32) -> f32 {
        self.tables.iter().map(|table| table.bounds.max_y()).fold(y, f32::max)
    }

    pub(super) fn paragraph_span(&self, y: &mut f32, height: f32) -> (f32, f32) {
        let minimum_width = self.width.min(24.0);
        loop {
            let mut spans = vec![(0.0, self.width)];
            let mut next = f32::INFINITY;
            // Reserve the whole paragraph, including a table starting partway
            // down it, without reserving tables entirely below that paragraph.
            for table in self.tables.iter().filter(|table| !table.wrap.overlaps_text() && table.exclusion.max_y() > *y && table.exclusion.y < *y + height) {
                next = next.min(table.exclusion.max_y());
                let (blocked_left, blocked_right) = match table.wrap {
                    TableWrap::Around => (table.exclusion.x, table.exclusion.max_x()),
                    TableWrap::TopAndBottom => (0.0, self.width),
                    TableWrap::InFrontOfText | TableWrap::BehindText => unreachable!("overlapping tables do not exclude text"),
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn positioned_tables_share_a_row_when_only_text_clearance_overlaps() {
        let mut flow = Flow::new(400.0, 100.0);
        let mut position = TablePosition::default();
        position.vertical_from = VerticalFrom::Page;
        position.vertical = VerticalPlacement::Offset(4.0);
        position.horizontal = HorizontalPlacement::Right;
        position.allow_overlap = false;
        position.distance_left_mm = 3.175;
        position.distance_right_mm = 3.175;
        let right = flow.place(&position, 180.0, 80.0, 0.0);
        position.horizontal = HorizontalPlacement::Left;
        let left = flow.place(&position, 200.0, 80.0, 100.0);
        assert!((left.y - right.y).abs() < 0.01);
        assert!(left.intersection(right).is_empty());
    }
}

#[cfg(test)]
mod overlap_tests {
    use super::*;
    #[test]
    fn cell_text_can_overlap_a_table_without_being_pushed_down_or_narrowed() {
        for wrap in [TableWrap::InFrontOfText, TableWrap::BehindText] {
        let mut flow = Flow::new(240.0, 180.0);
        let position = TablePosition { wrap, vertical_from: VerticalFrom::Margin, ..Default::default() };
        let bounds = flow.place(&position, 240.0, 90.0, 0.0);
        let mut y = 0.0;
        assert_eq!(flow.paragraph_span(&mut y, 40.0), (0.0, 240.0));
        assert_eq!(y, 0.0);
        assert!(!Rect::new(0.0, y, 240.0, 40.0).intersection(bounds).is_empty());
        assert_eq!(flow.clear(0.0), 0.0, "Inline content must not clear a nonwrapping table");
        assert_eq!(flow.extent(0.0), 90.0, "The containing cell still fits its floating table");
        }
    }
}
