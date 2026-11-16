//! A physical table-row break need not intersect every cell at the same
//! text coordinate. Different fonts and paragraph spacing give each cell
//! its own legal line breaks; their continuations still share row borders.
use super::*;

pub(super) fn split_unaligned_row(row: &PreparedTableRow, available: f32)
    -> Option<(PreparedTableRow, PreparedTableRow)> {
    let limit = available - TABLE_CELL_PADDING;
    let mut top_cells = Vec::with_capacity(row.cells.len());
    let mut bottom_cells = Vec::with_capacity(row.cells.len());
    let mut last_boundary = TABLE_CELL_PADDING;
    let mut consumed = false;
    for cell in &row.cells {
        // Retain the existing protection for nested tables, images, equations
        // and paths whose split bounds cannot be established safely.
        if !cell.object_regions.is_empty() { return None; }
        let bounds = cell.commands.iter().map(command_vertical_bounds).collect::<Option<Vec<_>>>()?;
        let used_bottom = bounds.iter().map(|(_, b)| *b)
            .chain(cell.hit_regions.iter().map(|h| h.rect.max_y()))
            .fold(TABLE_CELL_PADDING, f32::max);
        let boundary = if used_bottom <= limit {
            // Short/finished cells must not contribute the full painted row
            // height to every continuation, otherwise blank tails grow.
            cell.content_bottom.min(limit).max(used_bottom)
        } else if let Some(cut) = cell.hit_regions.iter().map(|h| h.rect.max_y())
            .filter(|y| *y >= TABLE_CELL_PADDING && *y <= limit && cell_can_split_at(cell, *y))
            .max_by(f32::total_cmp) { cut }
        else {
            // A cell may start below this page's break. Carry its entire
            // content forward while other cells make progress on this page.
            let first = bounds.iter().map(|(t, _)| *t)
                .chain(cell.hit_regions.iter().map(|h| h.rect.y))
                .fold(f32::INFINITY, f32::min);
            let cut = first.min(limit);
            if cut < TABLE_CELL_PADDING || !cell_can_split_at(cell, cut) { return None; }
            cut
        };
        let (top, bottom) = split_prepared_cell(cell, boundary)?;
        consumed |= !top.commands.is_empty() || !top.hit_regions.is_empty();
        last_boundary = last_boundary.max(boundary);
        top_cells.push(top); bottom_cells.push(bottom);
    }
    // Do not emit empty pages indefinitely for an indivisible item.
    if !consumed { return None; }
    let top_height = last_boundary + TABLE_CELL_PADDING;
    let bottom_height = bottom_cells.iter().map(|c| c.content_bottom + TABLE_CELL_PADDING)
        .fold((row.height - last_boundary).max(0.0) + TABLE_CELL_PADDING, f32::max);
    if top_height > available + 0.01 || bottom_height >= row.height - 0.01 { return None; }
    for cell in &mut top_cells { set_height(cell, top_height); }
    for cell in &mut bottom_cells { set_height(cell, bottom_height); }
    Some((
        PreparedTableRow { height: top_height, cells: top_cells,
            allow_break_across_pages: row.allow_break_across_pages, span_group_end: 0 },
        PreparedTableRow { height: bottom_height, cells: bottom_cells,
            allow_break_across_pages: row.allow_break_across_pages, span_group_end: 0 },
    ))
}

fn set_height(cell: &mut PreparedTableCell, height: f32) {
    cell.height = height;
    cell.own_region.rect.height = height;
}
