//! Encode physical Word table grids, row properties, cell merges and nesting.
use super::*;

const MAX_CELLS: usize = 63;
const CELL_BYTES: usize = 20;
const IN_TABLE: u16 = 0x2416;
const TABLE_DEPTH: u16 = 0x6649;
const INNER_CELL: u16 = 0x244B;
const INNER_ROW: u16 = 0x244C;

#[derive(Clone, Copy)]
struct Slot<'a> {
    cell: &'a TableCell,
    horizontal: usize,
    continuation: bool,
}

pub(super) fn paragraph_context(depth: u32) -> Vec<u8> {
    let mut out = Vec::new();
    if depth != 0 {
        byte_property(&mut out, IN_TABLE, 1);
        word(&mut out, TABLE_DEPTH);
        long(&mut out, depth);
    }
    out
}

fn grid(table: &Table) -> FormatResult<Vec<Vec<Option<Slot<'_>>>>> {
    let mut active = vec![None::<(usize, Slot<'_>)>; MAX_CELLS];
    let mut result = Vec::new();
    let mut maximum = table.properties.grid_column_widths_mm.len();
    for row in &table.rows {
        let mut slots = vec![None; MAX_CELLS];
        for (column, pending) in active.iter_mut().enumerate() {
            if let Some((remaining, slot)) = pending {
                slots[column] = Some(Slot {
                    continuation: true,
                    ..*slot
                });
                *remaining -= 1;
                if *remaining == 0 {
                    *pending = None;
                }
            }
        }
        let mut column = 0;
        for cell in &row.cells {
            while column < MAX_CELLS && slots[column].is_some() {
                column += 1;
            }
            let span = usize::from(cell.column_span.max(1));
            if column + span > MAX_CELLS || slots[column..column + span].iter().any(Option::is_some)
            {
                return Err(unsupported("A table row exceeds 63 native DOC grid columns or has overlapping merged cells"));
            }
            for offset in 0..span {
                let slot = Slot {
                    cell,
                    horizontal: offset,
                    continuation: false,
                };
                slots[column + offset] = Some(slot);
                if cell.row_span > 1 {
                    active[column + offset] = Some((usize::from(cell.row_span - 1), slot));
                }
            }
            column += span;
        }
        let columns = slots
            .iter()
            .rposition(Option::is_some)
            .map_or(0, |index| index + 1);
        maximum = maximum.max(columns);
        slots.truncate(columns);
        result.push(slots);
    }
    if maximum > MAX_CELLS || active.iter().any(Option::is_some) {
        return Err(unsupported(
            "A table merge extends beyond the native DOC table grid",
        ));
    }
    for row in &mut result {
        row.resize(maximum, None);
    }
    Ok(result)
}

fn column_widths(table: &Table, rows: &[Vec<Option<Slot<'_>>>], width: f32) -> Vec<f32> {
    let columns = rows.first().map_or(0, Vec::len);
    let fallback = table.properties.preferred_width_mm.unwrap_or(width) / columns.max(1) as f32;
    let mut result = (0..columns)
        .map(|column| {
            table
                .properties
                .grid_column_widths_mm
                .get(column)
                .copied()
                .or_else(|| {
                    rows.iter().filter_map(|row| row[column]).find_map(|slot| {
                        slot.cell
                            .width_mm
                            .map(|value| value / f32::from(slot.cell.column_span.max(1)))
                    })
                })
                .filter(|value| value.is_finite() && *value > 0.0)
                .unwrap_or(fallback.max(0.1))
        })
        .collect::<Vec<_>>();
    let grid_complete = table.properties.grid_column_widths_mm.len() == columns
        && table
            .properties
            .grid_column_widths_mm
            .iter()
            .all(|value| value.is_finite() && *value > 0.0);
    let total = result.iter().sum::<f32>();
    let requested = table
        .properties
        .preferred_width_mm
        .unwrap_or(if grid_complete { total } else { width });
    if total > 0.0 {
        for value in &mut result {
            *value *= requested / total;
        }
    }
    result
}

impl Story<'_> {
    pub(super) fn table(&mut self, table: &Table, width: f32) -> FormatResult<()> {
        let rows = grid(table)?;
        let widths = column_widths(table, &rows, width);
        let depth = self
            .table_depth
            .checked_add(1)
            .ok_or_else(|| unsupported("Table nesting exceeds the native DOC limit"))?;
        if depth > 64 {
            return Err(unsupported("Table nesting exceeds the native DOC limit"));
        }
        let previous = self.table_depth;
        self.table_depth = depth;
        for (row_index, (row, slots)) in table.rows.iter().zip(&rows).enumerate() {
            if slots.is_empty() {
                continue;
            }
            let physical = slots
                .iter()
                .enumerate()
                .filter(|(_, slot)| slot.is_none_or(|slot| slot.horizontal == 0))
                .collect::<Vec<_>>();
            let row_widths = physical
                .iter()
                .map(|(column, slot)| {
                    widths[*column
                        ..(*column
                            + slot.map_or(1, |slot| usize::from(slot.cell.column_span.max(1))))]
                        .iter()
                        .sum()
                })
                .collect::<Vec<f32>>();
            let row_slots = physical.iter().map(|(_, slot)| **slot).collect::<Vec<_>>();
            for (slot, cell_width) in row_slots.iter().zip(&row_widths) {
                let content = slot
                    .filter(|slot| !slot.continuation && slot.horizontal == 0)
                    .map(|slot| slot.cell.blocks.as_slice())
                    .unwrap_or(&[]);
                self.table_cell(content, *cell_width, depth)?;
            }
            let mut properties = paragraph_context(depth);
            byte_property(
                &mut properties,
                if depth == 1 { 0x2417 } else { INNER_ROW },
                1,
            );
            byte_property(&mut properties, 0x3404, u8::from(row.repeat_as_header));
            byte_property(
                &mut properties,
                0x3403,
                u8::from(!row.allow_break_across_pages),
            );
            if let Some(height) = row.height_mm {
                word_property(&mut properties, 0x9407, twips(height));
            }
            word_property(
                &mut properties,
                0x5400,
                match table.properties.alignment.unwrap_or(TableAlignment::Left) {
                    TableAlignment::Left => 0,
                    TableAlignment::Center => 1,
                    TableAlignment::Right => 2,
                },
            );
            row_definition(&mut properties, table, &row_slots, &row_widths);
            row_decoration(&mut properties, table, &row_slots, row_index, rows.len());
            floating(&mut properties, table.properties.position.as_ref());
            self.paragraph(
                &Paragraph::plain(""),
                &properties,
                if depth == 1 { 7 } else { 13 },
            );
        }
        self.table_depth = previous;
        Ok(())
    }

    fn table_cell(&mut self, blocks: &[Block], width: f32, depth: u32) -> FormatResult<()> {
        let mut properties = paragraph_context(depth);
        if depth > 1 {
            byte_property(&mut properties, INNER_CELL, 1);
        }
        let terminator = if depth == 1 { 7 } else { 13 };
        for (index, block) in blocks.iter().enumerate() {
            if index + 1 == blocks.len() {
                if let Block::Paragraph(paragraph) = block {
                    self.paragraph(paragraph, &properties, terminator);
                    return Ok(());
                }
            }
            self.blocks(std::slice::from_ref(block), width)?;
        }
        self.paragraph(&Paragraph::plain(""), &properties, terminator);
        Ok(())
    }
}

fn row_definition(out: &mut Vec<u8>, table: &Table, slots: &[Option<Slot<'_>>], widths: &[f32]) {
    word(out, 0xD608);
    word(
        out,
        (2 + 2 * (slots.len() + 1) + CELL_BYTES * slots.len()) as u16,
    );
    out.push(slots.len() as u8);
    let mut position = table.properties.left_indent_mm.unwrap_or(0.0);
    word(out, twips(position));
    for width in widths {
        position += width;
        word(out, twips(position));
    }
    for slot in slots {
        let mut bytes = [0u8; CELL_BYTES];
        if let Some(slot) = slot {
            let horizontal = 0u16;
            let vertical = if slot.continuation {
                0x20
            } else if slot.cell.row_span > 1 {
                0x60
            } else {
                0
            };
            let alignment = match slot.cell.vertical_alignment {
                CellVerticalAlignment::Top => 0,
                CellVerticalAlignment::Center => 1,
                CellVerticalAlignment::Bottom => 2,
            };
            bytes[..2]
                .copy_from_slice(&(horizontal | vertical | (alignment << 7) as u16).to_le_bytes());
        }
        out.extend(bytes);
    }
    for (column, slot) in slots.iter().enumerate() {
        if let Some(slot) = slot {
            if slot.cell.row_span > 1 {
                variable(
                    out,
                    0xD62B,
                    &[column as u8, if slot.continuation { 1 } else { 3 }],
                );
            }
        }
    }
    for (index, chunk) in slots.chunks(22).enumerate() {
        let mut shading = Vec::new();
        for slot in chunk {
            let color = slot
                .and_then(|slot| slot.cell.shading.as_deref())
                .and_then(rgb);
            shading.extend_from_slice(&[0, 0, 0, 0xFF]);
            shading.extend_from_slice(
                &color
                    .map(|value| [value[0], value[1], value[2], 0])
                    .unwrap_or([0, 0, 0, 0xFF]),
            );
            word(&mut shading, 0);
        }
        variable(out, [0xD612, 0xD616, 0xD60C][index], &shading);
    }
    word(out, 0xF614);
    out.push(3);
    word(out, twips(widths.iter().sum()));
    if let Some(spacing) = table.properties.cell_spacing_mm {
        variable(
            out,
            0xD633,
            &[
                0,
                1,
                15,
                3,
                twips(spacing) as u8,
                (twips(spacing) >> 8) as u8,
            ],
        );
    }
}

fn variable(out: &mut Vec<u8>, code: u16, operand: &[u8]) {
    word(out, code);
    out.push(operand.len() as u8);
    out.extend_from_slice(operand);
}

fn row_decoration(
    out: &mut Vec<u8>,
    table: &Table,
    slots: &[Option<Slot<'_>>],
    row: usize,
    row_count: usize,
) {
    if let Some(borders) = &table.properties.borders {
        let mut bytes = Vec::new();
        for border in [
            &borders.top,
            &borders.left,
            &borders.bottom,
            &borders.right,
            &borders.inside_horizontal,
            &borders.inside_vertical,
        ] {
            bytes.extend(border.as_ref().map(border_bytes).unwrap_or([0; 8]));
        }
        variable(out, 0xD613, &bytes);
    }
    for (column, slot) in slots.iter().enumerate() {
        let Some(slot) = slot else {
            continue;
        };
        if let Some(color) = slot.cell.shading.as_deref().and_then(rgb) {
            let mut bytes = vec![
                column as u8,
                (column + 1) as u8,
                0,
                0,
                0,
                0xFF,
                color[0],
                color[1],
                color[2],
                0,
                0,
                0,
            ];
            variable(out, 0xD62D, &bytes);
            bytes.clear();
        }
        let cell = slot.cell.borders.as_ref();
        let inherited = table.properties.borders.as_ref();
        let borders = [
            (
                1,
                cell.and_then(|v| v.top.as_ref()).or_else(|| {
                    inherited.and_then(|v| {
                        if row == 0 {
                            v.top.as_ref()
                        } else {
                            v.inside_horizontal.as_ref()
                        }
                    })
                }),
            ),
            (
                2,
                cell.and_then(|v| v.left.as_ref()).or_else(|| {
                    inherited.and_then(|v| {
                        if column == 0 {
                            v.left.as_ref()
                        } else {
                            v.inside_vertical.as_ref()
                        }
                    })
                }),
            ),
            (
                4,
                cell.and_then(|v| v.bottom.as_ref()).or_else(|| {
                    inherited.and_then(|v| {
                        if row + 1 == row_count {
                            v.bottom.as_ref()
                        } else {
                            v.inside_horizontal.as_ref()
                        }
                    })
                }),
            ),
            (
                8,
                cell.and_then(|v| v.right.as_ref()).or_else(|| {
                    inherited.and_then(|v| {
                        if column + 1 == slots.len() {
                            v.right.as_ref()
                        } else {
                            v.inside_vertical.as_ref()
                        }
                    })
                }),
            ),
        ];
        for (mask, border) in borders {
            if let Some(border) = border {
                let mut bytes = vec![column as u8, (column + 1) as u8, mask];
                bytes.extend(border_bytes(border));
                variable(out, 0xD62F, &bytes);
            }
        }
    }
}

fn floating(out: &mut Vec<u8>, position: Option<&TablePosition>) {
    let Some(position) = position else {
        return;
    };
    let horizontal = match position.horizontal_from {
        HorizontalFrom::Column => 0,
        HorizontalFrom::Margin => 1,
        HorizontalFrom::Page => 2,
    };
    let vertical = match position.vertical_from {
        VerticalFrom::Margin => 0,
        VerticalFrom::Page => 1,
        VerticalFrom::Paragraph => 2,
    };
    byte_property(out, 0x360D, (horizontal << 6) | (vertical << 4));
    let x = match position.horizontal {
        HorizontalPlacement::Offset(value) => twips(value),
        HorizontalPlacement::Left => 0,
        HorizontalPlacement::Center => (-4i16) as u16,
        HorizontalPlacement::Right => (-8i16) as u16,
        HorizontalPlacement::Inside => (-12i16) as u16,
        HorizontalPlacement::Outside => (-16i16) as u16,
    };
    let y = match position.vertical {
        VerticalPlacement::Offset(value) => twips(value),
        VerticalPlacement::Top => (-4i16) as u16,
        VerticalPlacement::Center => (-8i16) as u16,
        VerticalPlacement::Bottom => (-12i16) as u16,
    };
    for (code, value) in [
        (0x940E, x),
        (0x940F, y),
        (0x9410, twips(position.distance_left_mm)),
        (0x941E, twips(position.distance_right_mm)),
        (0x9411, twips(position.distance_top_mm)),
        (0x941F, twips(position.distance_bottom_mm)),
    ] {
        word_property(out, code, value);
    }
    byte_property(out, 0x3465, u8::from(!position.allow_overlap));
    // Word's table wrap mode uses absolute placement; zero side distances do not disable it.
    if position.wrap == TableWrap::TopAndBottom {
        word_property(out, 0x9410, twips(1000.0));
        word_property(out, 0x941E, twips(1000.0));
    }
}
