//! Retain independent row/cell construction state for every nested table level.
use super::*;

#[derive(Default)]
pub(super) struct SavedTableState {
    cur_rows: Vec<RowBuild>,
    cur_row_pagination: Vec<TableRowPaginationHint>,
    cur_table_bidi_visual: Option<bool>,
    cur_row_cells: Vec<CellBuild>,
    cell_blocks: Vec<Block>,
    cell_pagination: Vec<Option<PaginationHint>>,
    cell_line_spacing: Vec<Option<LineSpacingHint>>,
    #[cfg(feature = "docx")]
    cell_column_break_offsets: Vec<Vec<usize>>,
    #[cfg(any(feature = "docx", feature = "render"))]
    cell_tab_stops: Vec<Vec<TabStop>>,
}

impl Asm<'_, '_> {
    pub(super) fn change_table_depth(&mut self, target: u32) {
        while self.table_depth > target && self.table_depth > 1 {
            self.close_nested_table();
        }
        if target == 0 && self.table_depth == 1 {
            self.flush_table();
            self.table_depth = 0;
        }
        if target > 0 && self.table_depth == 0 {
            self.table_depth = 1;
        }
        while self.table_depth < target {
            let saved = SavedTableState {
                cur_rows: std::mem::take(&mut self.cur_rows),
                cur_row_pagination: std::mem::take(&mut self.cur_row_pagination),
                cur_table_bidi_visual: std::mem::take(&mut self.cur_table_bidi_visual),
                cur_row_cells: std::mem::take(&mut self.cur_row_cells),
                cell_blocks: std::mem::take(&mut self.cell_blocks),
                cell_pagination: std::mem::take(&mut self.cell_pagination),
                cell_line_spacing: std::mem::take(&mut self.cell_line_spacing),
                #[cfg(feature = "docx")]
                cell_column_break_offsets: std::mem::take(&mut self.cell_column_break_offsets),
                #[cfg(any(feature = "docx", feature = "render"))]
                cell_tab_stops: std::mem::take(&mut self.cell_tab_stops),
            };
            self.parent_tables.push(saved);
            self.table_depth += 1;
        }
    }

    fn close_nested_table(&mut self) {
        let output_start = self.blocks.len();
        self.flush_table();
        let nested = if self.blocks.len() > output_start {
            self.pagination_hints.pop();
            self.line_spacing_hints.pop();
            #[cfg(any(feature = "docx", feature = "render"))]
            self.tab_stops.pop();
            #[cfg(feature = "docx")]
            self.note_reference_anchors.pop();
            self.column_break_offsets.pop();
            self.page_break_offsets.pop();
            #[cfg(feature = "docx")]
            self.table_cell_column_break_offsets.pop();
            self.table_row_pagination.pop();
            self.table_cell_pagination.pop();
            self.table_cell_line_spacing.pop();
            #[cfg(any(feature = "docx", feature = "render"))]
            self.table_cell_tab_stops.pop();
            self.blocks.pop()
        } else {
            None
        };
        let Some(parent) = self.parent_tables.pop() else {
            self.table_depth = 0;
            return;
        };
        self.cur_rows = parent.cur_rows;
        self.cur_row_pagination = parent.cur_row_pagination;
        self.cur_table_bidi_visual = parent.cur_table_bidi_visual;
        self.cur_row_cells = parent.cur_row_cells;
        self.cell_blocks = parent.cell_blocks;
        self.cell_pagination = parent.cell_pagination;
        self.cell_line_spacing = parent.cell_line_spacing;
        #[cfg(feature = "docx")]
        {
            self.cell_column_break_offsets = parent.cell_column_break_offsets;
        }
        #[cfg(any(feature = "docx", feature = "render"))]
        {
            self.cell_tab_stops = parent.cell_tab_stops;
        }
        self.table_depth -= 1;
        if let Some(nested) = nested {
            self.cell_blocks.push(nested);
            self.cell_pagination.push(None);
            self.cell_line_spacing.push(None);
            #[cfg(feature = "docx")]
            self.cell_column_break_offsets.push(Vec::new());
            #[cfg(any(feature = "docx", feature = "render"))]
            self.cell_tab_stops.push(Vec::new());
        }
    }
}
