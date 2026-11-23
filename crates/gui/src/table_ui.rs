//! Table editing on the page and the operations shared with the Table bar.
//!
//! While the caret is in a table, or the pointer is over one, the table shows
//! light controls around its edges:
//! - a handle above every column and left of every row; clicking one selects
//!   that column or row and offers a delete button beside it;
//! - a corner handle that selects the whole table;
//! - a "+" marker when the pointer rests on a border in the handle gutter;
//!   clicking it inserts a column or row at that border;
//! - a guide along the full border while it is hovered or dragged.
//!
//! Everything is computed from the accepted layout's hit regions, in view
//! pixels, so controls keep their size at every zoom.
use crate::*;
use model::CellVerticalAlignment;
use render::{ChromeShape, Color, ObjectKind, Rect, TableCellHit};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableOp {
    InsertRowAbove, InsertRowBelow, InsertColumnLeft, InsertColumnRight, InsertCellsRight, InsertCellsDown,
    DeleteRow, DeleteColumn, DeleteTable, DeleteCellsLeft, DeleteCellsUp,
    MergeRight, MergeDown, Split, Distribute, DistributeRows, FitToMargins,
    /// 0 top, 1 centre, 2 bottom.
    Align(u8),
    HeaderRow,
    /// Change the current column width or row height by this many millimetres.
    Width(i8), Height(i8), HeightAuto,
    /// Free (floating, text wraps) or in line with the text.
    Float(bool),
    /// Floating tables: text flows around (true) or stops above and below.
    WrapAround(bool),
}

/// Whole rows, columns or the table selected through their handles: the
/// first and last column (or row), inclusive.
#[derive(Clone, Debug, PartialEq)]
pub enum Band { Column(usize, usize), Row(usize, usize), Table, /** One cell, by node id. */ Cell(String) }

impl Band {
    fn has_column(&self, k: usize) -> bool { matches!(*self, Band::Column(a, b) if (a..=b).contains(&k)) || *self == Band::Table }
    fn has_row(&self, r: usize) -> bool { matches!(*self, Band::Row(a, b) if (a..=b).contains(&r)) || *self == Band::Table }
}

/// Dragging along the column (or row) handles to select several.
#[derive(Clone, Debug)]
pub struct BandDrag { table: String, page: usize, columns: bool, start: usize, end: usize }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target { Column(usize), Row(usize), Corner, InsertColumn(usize), InsertRow(usize), Delete, Resize }

/// Moving the table by its corner handle, or resizing it by the handle at
/// its bottom-right corner.
#[derive(Clone, Debug)]
pub struct TableDrag {
    table: String,
    resize: bool,
    start: Point,
    current: Point,
    moved: bool,
    /// Table bounds in view pixels on the page where the drag started.
    bounds: (f32, f32, f32, f32),
    indent_mm: f32,
    widths_pt: Vec<f32>,
    heights_pt: Vec<f32>,
    declared: Vec<Option<f32>>,
    horizontal_only: bool,
    proportional: bool,
    /// Floating table: moves freely instead of between paragraphs.
    free: Option<model::TablePosition>,
    snapped: Option<crate::table_snap::Snapped>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Hover { pub table: String, pub page: usize, pub target: Option<Target> }

/// The current cell's sizes and settings, for the Table bar.
#[derive(Clone, Debug, PartialEq)]
pub struct CellFacts { pub width_mm: f32, pub height_mm: Option<f32>, pub actual_height_mm: f32, pub header: bool, pub align: u8, pub merged: bool,
    /// Where a floating table sits; None for an inline table.
    pub position: Option<model::TablePosition>,
    /// A main-text table (only those can float).
    pub movable: bool }

/// Accent shared by handles, guides and the cell selection. The picture
/// selection (picture_chrome.rs) uses the same colours, so every on-page
/// object control looks alike.
pub(crate) const BLUE: Color = Color { r: 37.0 / 255.0, g: 99.0 / 255.0, b: 235.0 / 255.0, a: 1.0 };
pub(crate) const HANDLE: Color = Color { r: 174.0 / 255.0, g: 193.0 / 255.0, b: 228.0 / 255.0, a: 1.0 };
pub(crate) const HANDLE_HOT: Color = Color { r: 104.0 / 255.0, g: 148.0 / 255.0, b: 240.0 / 255.0, a: 1.0 };
pub(crate) const SELECTION_FILL: Color = Color { r: 37.0 / 255.0, g: 99.0 / 255.0, b: 235.0 / 255.0, a: 0.14 };
const RED: Color = Color { r: 214.0 / 255.0, g: 54.0 / 255.0, b: 41.0 / 255.0, a: 1.0 };
const MM_TO_PT: f32 = 72.0 / 25.4;

/// One table's footprint on one page, in view pixels.
struct Geo {
    page: usize,
    left: f32, top: f32, right: f32, bottom: f32,
    /// Logical column borders, left to right.
    cols: Vec<f32>,
    /// (row index, top, bottom) of the rows on this page.
    rows: Vec<(usize, f32, f32)>,
}

/// Distances in view pixels, scaled with the screen DPI.
pub(crate) struct Metrics { pub(crate) s: f32 }
impl Metrics {
    pub(crate) fn of(state: &CanvasState) -> Self { Self { s: (state.viewport.dpi / 96.0).max(1.0) } }
    fn bar_gap(&self) -> f32 { 5.0 * self.s }
    fn bar(&self) -> f32 { 5.0 * self.s }
    fn gutter(&self) -> f32 { 26.0 * self.s }
    fn snap(&self) -> f32 { 7.0 * self.s }
    fn plus(&self) -> f32 { 8.0 * self.s }
    /// Radius of the round × (delete) button.
    pub(crate) fn chip(&self) -> f32 { 10.0 * self.s }
    /// Side of a square resize handle; `hot` when the pointer is on it.
    pub(crate) fn resize_handle(&self, hot: bool) -> f32 { 7.0 * self.s + if hot { 2.0 * self.s } else { 0.0 } }
}

fn cells<'a>(state: &'a CanvasState, page: usize, table: &'a str) -> impl Iterator<Item = (&'a Rect, &'a TableCellHit, &'a str)> + 'a {
    state.vector_document.pages.get(page).into_iter().flat_map(|p| p.object_regions.iter()).filter_map(move |r| match &r.kind {
        ObjectKind::TableCell(info) if info.table_id == table => Some((&r.rect, info, r.node_id.as_str())),
        _ => None,
    })
}

fn geometry(state: &CanvasState, page: usize, table: &str) -> Option<Geo> {
    let origin = state.viewport.page_rect_px(&state.vector_document, page)?;
    let scale = state.viewport.points_to_pixels().max(0.01);
    let (mut left, mut top, mut bottom) = (f32::MAX, f32::MAX, f32::MIN);
    let mut widths: &[f32] = &[];
    let mut first_left = None;
    let mut rows: Vec<(usize, f32, f32)> = Vec::new();
    let mut open: Vec<(usize, f32, f32)> = Vec::new();
    for (rect, info, _) in cells(state, page, table) {
        left = left.min(rect.x); top = top.min(rect.y); bottom = bottom.max(rect.y + rect.height);
        if widths.is_empty() { widths = &info.column_widths_pt; }
        if info.column_index == 0 { first_left = Some(first_left.map_or(rect.x, |x: f32| x.min(rect.x))); }
        let end = if info.row_span == 1 { rect.y + rect.height } else { f32::MIN };
        match rows.iter_mut().find(|r| r.0 == info.row_index) {
            Some(row) => { row.1 = row.1.min(rect.y); row.2 = row.2.max(end); }
            None => rows.push((info.row_index, rect.y, end)),
        }
        open.push((info.row_index, rect.y, info.row_height_pt));
    }
    if rows.is_empty() || widths.is_empty() { return None; }
    rows.sort_by_key(|r| r.0);
    // Rows made only of merged cells end where the next row starts.
    for i in 0..rows.len() {
        if rows[i].2 == f32::MIN {
            rows[i].2 = rows.get(i + 1).map(|n| n.1).unwrap_or_else(|| {
                open.iter().find(|o| o.0 == rows[i].0).map_or(bottom, |o| o.1 + o.2)
            });
        }
    }
    let left = first_left.unwrap_or(left);
    let px = |x: f32| origin.x + x * scale;
    let py = |y: f32| origin.y + y * scale;
    let mut cols = vec![px(left)];
    let mut x = left;
    for w in widths { x += w; cols.push(px(x)); }
    Some(Geo {
        page, left: px(left), top: py(top), right: *cols.last().unwrap_or(&px(left)), bottom: py(bottom),
        cols, rows: rows.into_iter().map(|(i, t, b)| (i, py(t), py(b))).collect(),
    })
}

fn active_table(state: &CanvasState) -> Option<String> {
    state.editor.table_cell_context().map(|c| c.table_id.0)
}

/// The table whose controls are shown: the one under the pointer, else the
/// one holding the caret.
fn shown_table(state: &CanvasState) -> Option<String> {
    state.table_hover.as_ref().map(|h| h.table.clone()).or_else(|| active_table(state))
}

fn geometries(state: &CanvasState, table: &str) -> Vec<Geo> {
    state.viewport.visible_pages(&state.vector_document).into_iter().filter_map(|page| geometry(state, page, table)).collect()
}

fn valid_band(state: &CanvasState) -> Option<(String, Band)> {
    let Some(objects_ui::SelectedObject::Cells { table_id, cell_ids }) = &state.selected_object else { return None; };
    match state.table_band.clone() {
        Some((table, band)) if table_id.0 == table => Some((table, band)),
        // A single selected cell (Select → Cell, Shift+click) can be deleted too.
        _ if cell_ids.len() == 1 => Some((table_id.0.clone(), Band::Cell(cell_ids[0].0.clone()))),
        _ => None,
    }
}

fn chip_center(state: &CanvasState, g: &Geo, table: &str, band: &Band, m: &Metrics) -> Option<Point> {
    let lift = m.bar_gap() + m.bar() + m.chip() + 6.0 * m.s;
    match band {
        Band::Cell(id) => {
            let origin = state.viewport.page_rect_px(&state.vector_document, g.page)?;
            let scale = state.viewport.points_to_pixels();
            let (rect, _, _) = cells(state, g.page, table).find(|(_, _, node)| *node == id)?;
            Some(Point::new(origin.x + (rect.x + rect.width) * scale - m.chip() - 3.0 * m.s, origin.y + rect.y * scale + m.chip() + 3.0 * m.s))
        }
        Band::Column(a, b) => Some(Point::new((g.cols.get(*a)? + g.cols.get(b + 1)?) * 0.5, g.top - lift)),
        Band::Row(a, b) => {
            let rows: Vec<_> = g.rows.iter().filter(|row| (*a..=*b).contains(&row.0)).collect();
            let (top, bottom) = (rows.first()?.1, rows.last()?.2);
            Some(Point::new(g.left - lift, (top + bottom) * 0.5))
        }
        Band::Table => Some(Point::new(g.left - lift, g.top - lift)),
    }
}

fn target_at(state: &CanvasState, g: &Geo, table: &str, p: Point) -> Option<Target> {
    let m = Metrics::of(state);
    if let Some((band_table, band)) = valid_band(state) {
        if band_table == table {
            if let Some(c) = chip_center(state, g, table, &band, &m) {
                let r = m.chip() + 2.0 * m.s;
                if (p.x - c.x).powi(2) + (p.y - c.y).powi(2) <= r * r { return Some(Target::Delete); }
            }
        }
    }
    let gutter = m.gutter();
    let in_top = p.y >= g.top - gutter && p.y < g.top - 1.0;
    let in_left = p.x >= g.left - gutter && p.x < g.left - 1.0;
    let handle = m.bar() + 6.0 * m.s;
    if (p.x - g.right).abs() <= handle * 0.6 && (p.y - g.bottom).abs() <= handle * 0.6 { return Some(Target::Resize); }
    if in_top && in_left { return Some(Target::Corner); }
    if in_top && p.x >= g.left - m.snap() && p.x <= g.right + m.snap() {
        let (k, d) = g.cols.iter().enumerate().map(|(k, x)| (k, (p.x - x).abs())).fold((0, f32::MAX), |a, b| if b.1 < a.1 { b } else { a });
        if d <= m.snap() { return Some(Target::InsertColumn(k)); }
        return (0..g.cols.len().saturating_sub(1)).find(|&k| p.x >= g.cols[k] && p.x < g.cols[k + 1]).map(Target::Column);
    }
    if in_left && p.y >= g.top - m.snap() && p.y <= g.bottom + m.snap() {
        let mut edges: Vec<(usize, f32)> = g.rows.iter().map(|r| (r.0, r.1)).collect();
        if let Some(last) = g.rows.last() { edges.push((last.0 + 1, last.2)); }
        let (edge, d) = edges.iter().map(|&(i, y)| (i, (p.y - y).abs())).fold((0, f32::MAX), |a, b| if b.1 < a.1 { b } else { a });
        if d <= m.snap() { return Some(Target::InsertRow(edge)); }
        return g.rows.iter().find(|r| p.y >= r.1 && p.y < r.2).map(|r| Target::Row(r.0));
    }
    None
}

/// Tracks which table's controls are shown and what the pointer is on.
/// Returns true when the canvas must repaint.
pub fn hover(state: &mut CanvasState, p: Option<Point>) -> bool {
    let next = p.and_then(|p| hover_at(state, p));
    if state.table_hover != next { state.table_hover = next; state.needs_present = true; return true; }
    false
}

fn hover_at(state: &CanvasState, p: Point) -> Option<Hover> {
    if crate::equation_ui::previewing(state) || state.object_drag.is_some() || state.display_generation != Some(state.layout_generation) { return None; }
    let m = Metrics::of(state);
    let reach = m.gutter() + 2.0 * m.chip() + 12.0 * m.s;
    let near = |g: &Geo| p.x >= g.left - reach && p.x <= g.right + 10.0 * m.s && p.y >= g.top - reach && p.y <= g.bottom + 10.0 * m.s;
    // Keep the shown table while the pointer is on it or its gutter.
    if let Some(table) = shown_table(state) {
        for g in geometries(state, &table) {
            if near(&g) {
                let target = target_at(state, &g, &table, p);
                let inside = p.x >= g.left && p.x <= g.right && p.y >= g.top && p.y <= g.bottom;
                if target.is_some() || inside || state.table_hover.as_ref().is_some_and(|h| h.table == table) {
                    return Some(Hover { table, page: g.page, target });
                }
            }
        }
    }
    // Otherwise the innermost table under the pointer.
    let mut best: Option<(f32, String, usize)> = None;
    for page in state.viewport.visible_pages(&state.vector_document) {
        let Some(origin) = state.viewport.page_rect_px(&state.vector_document, page) else { continue; };
        let scale = state.viewport.points_to_pixels().max(0.01);
        let q = Point::new((p.x - origin.x) / scale, (p.y - origin.y) / scale);
        for r in &state.vector_document.pages[page].object_regions {
            let ObjectKind::TableCell(info) = &r.kind else { continue; };
            let rect = r.rect;
            if q.x >= rect.x && q.x <= rect.x + rect.width && q.y >= rect.y && q.y <= rect.y + rect.height {
                let area = rect.width * rect.height;
                if best.as_ref().is_none_or(|b| area < b.0) { best = Some((area, info.table_id.clone(), page)); }
            }
        }
    }
    best.map(|(_, table, page)| Hover { table, page, target: None })
}

pub fn tooltip(state: &CanvasState) -> &'static str {
    let band = valid_band(state).map(|b| b.1);
    match state.table_hover.as_ref().and_then(|h| h.target) {
        Some(Target::Column(_)) => "Select column",
        Some(Target::Row(_)) => "Select row",
        Some(Target::Corner) => "Click to select the table · drag to move it (Shift: sideways only)",
        Some(Target::Resize) => "Drag to resize the table (Shift keeps proportions)",
        Some(Target::InsertColumn(_)) => "Insert column here",
        Some(Target::InsertRow(_)) => "Insert row here",
        Some(Target::Delete) => match band { Some(Band::Column(a, b)) if a != b => "Delete columns", Some(Band::Row(a, b)) if a != b => "Delete rows",
            Some(Band::Column(..)) => "Delete column", Some(Band::Row(..)) => "Delete row",
            Some(Band::Cell(_)) => "Delete cell (the cells to its right move left)", _ => "Delete table" },
        None => "",
    }
}

pub fn over_control(state: &CanvasState) -> bool { state.table_drag.is_some() || state.band_drag.is_some() || state.table_hover.as_ref().is_some_and(|h| h.target.is_some()) }

/// Cursor for the table control under the pointer.
pub fn pointer(state: &CanvasState) -> Option<objects_ui::Pointer> {
    if let Some(drag) = &state.table_drag { return Some(if drag.resize { objects_ui::Pointer::ResizeNwSe } else { objects_ui::Pointer::Move }); }
    match state.table_hover.as_ref()?.target? {
        Target::Corner => Some(objects_ui::Pointer::Move),
        Target::Resize => Some(objects_ui::Pointer::ResizeNwSe),
        _ => Some(objects_ui::Pointer::Hand),
    }
}

fn start_drag(state: &mut CanvasState, table: &str, page: usize, p: Point, resize: bool) -> Result<(), String> {
    let g = geometry(state, page, table).ok_or("The table is not laid out yet.")?;
    let model = state.editor.table_by_id(&NodeId::from(table)).ok_or("Table not found")?;
    let rows = model.rows.len();
    let declared: Vec<Option<f32>> = model.rows.iter().map(|r| r.height_mm).collect();
    let indent_mm = crate::table_snap::margins(&state.vector_document,&state.viewport,Point::new(g.left,g.top))
        .map(|margins|(g.left-margins.x)/state.viewport.points_to_pixels().max(0.01)/MM_TO_PT)
        .unwrap_or(model.properties.left_indent_mm.unwrap_or(0.0));
    let mut widths_pt = Vec::new();
    let mut heights_pt = vec![0.0f32; rows];
    for page in 0..state.vector_document.pages.len() {
        for (_, info, _) in cells(state, page, table) {
            if widths_pt.is_empty() { widths_pt = info.column_widths_pt.clone(); }
            if let Some(h) = heights_pt.get_mut(info.row_index) { *h = h.max(info.row_height_pt); }
        }
    }
    state.table_drag = Some(TableDrag {
        table: table.to_string(), resize, start: p, current: p, moved: false,
        bounds: (g.left, g.top, g.right, g.bottom), indent_mm, widths_pt, heights_pt, declared,
        horizontal_only: false, proportional: false, free: model.properties.position.clone(), snapped: None,
    });
    Ok(())
}

impl TableDrag {
    fn ghost(&self) -> Rect {
        if let Some(snap)=self.snapped {return snap.rect;}
        let (l, t, r, b) = self.bounds;
        let (dx, dy) = (self.current.x - self.start.x, self.current.y - self.start.y);
        if self.resize {
            let (fx, fy) = self.factors();
            Rect::new(l, t, (r - l) * fx, (b - t) * fy)
        } else {
            Rect::new(l + dx, t + if self.horizontal_only { 0.0 } else { dy }, r - l, b - t)
        }
    }
    fn factors(&self) -> (f32, f32) {
        if let Some(snap)=self.snapped {return (snap.rect.width/(self.bounds.2-self.bounds.0).max(1.0),snap.rect.height/(self.bounds.3-self.bounds.1).max(1.0));}
        let (l, t, r, b) = self.bounds;
        let fx = ((r - l + self.current.x - self.start.x) / (r - l).max(1.0)).clamp(0.2, 5.0);
        let fy = ((b - t + self.current.y - self.start.y) / (b - t).max(1.0)).clamp(0.2, 5.0);
        if self.proportional { let f = (fx + fy) * 0.5; (f, f) } else { (fx, fy) }
    }
}

/// Continues a table move or resize. Returns false when none is active.
/// Extends a row or column selection to the handle under the pointer.
fn band_motion(state: &mut CanvasState, p: Point) -> bool {
    let Some(mut drag) = state.band_drag.take() else { return false; };
    if let Some(g) = geometry(state, drag.page, &drag.table) {
        let end = if drag.columns {
            (0..g.cols.len().saturating_sub(1)).rev().find(|&k| p.x >= g.cols[k]).unwrap_or(0)
        } else {
            g.rows.iter().rev().find(|r| p.y >= r.1).or(g.rows.first()).map_or(drag.end, |r| r.0)
        };
        if end != drag.end {
            drag.end = end;
            let (a, b) = (drag.start.min(end), drag.start.max(end));
            let band = if drag.columns { Band::Column(a, b) } else { Band::Row(a, b) };
            if let Err(e) = select_band(state, &drag.table, band) { state.action_status = Some(e); }
            state.needs_present = true;
        }
    }
    state.band_drag = Some(drag);
    true
}

pub fn motion(state: &mut CanvasState, p: Point, shift: bool) -> bool {
    if band_motion(state, p) { return true; }
    let Some(mut drag) = state.table_drag.take() else { return false; };
    drag.current = p;
    drag.snapped = None;
    drag.proportional = drag.resize && shift;
    drag.horizontal_only = !drag.resize && shift;
    let ghost = drag.ghost();
    if let Some(margins)=crate::table_snap::margins(&state.vector_document,&state.viewport,Point::new(ghost.x,ghost.y)) {
        let tolerance=crate::table_snap::TOLERANCE_DIP*Metrics::of(state).s;
        drag.snapped=Some(if drag.resize {crate::table_snap::resizing(ghost,margins,tolerance,drag.proportional)}
            else {crate::table_snap::moving(ghost,margins,tolerance,drag.free.is_some()&&!drag.horizontal_only)});
    }
    drag.moved |= (p.x - drag.start.x).abs() + (p.y - drag.start.y).abs() > 3.0;
    let scale = state.viewport.points_to_pixels().max(0.01);
    if drag.resize {
        drag.proportional = shift;
        let (fx, fy) = drag.factors();
        let width: f32 = drag.widths_pt.iter().sum::<f32>() * fx / MM_TO_PT;
        let height = (drag.bounds.3 - drag.bounds.1) * fy / scale / MM_TO_PT;
        state.action_status = Some(format!("Table: {width:.1} × {height:.1} mm — Shift keeps proportions; Escape cancels"));
    } else if drag.free.is_some() {
        drag.horizontal_only = shift;
        state.action_status = Some("Free position — release to place the table here; text wraps around it (Shift: sideways only)".into());
    } else {
        drag.horizontal_only = shift;
        let indent = new_indent(&drag, scale);
        state.action_status = Some(if shift { format!("Indent {indent:.1} mm — sideways only; release to apply") }
            else { format!("Indent {indent:.1} mm — release to place the table before the paragraph under its top edge (Shift: sideways only)") });
    }
    if drag.moved { state.table_hover = None; }
    state.table_drag = Some(drag);
    state.needs_present = true;
    true
}

fn new_indent(drag: &TableDrag, scale: f32) -> f32 {
    drag.indent_mm + (drag.ghost().x - drag.bounds.0) / scale / MM_TO_PT
}

/// Finishes a table move or resize. Returns false when none is active.
pub fn up(state: &mut CanvasState) -> bool {
    if state.band_drag.take().is_some() { return true; }
    let Some(drag) = state.table_drag.take() else { return false; };
    let table_id = NodeId::from(drag.table.as_str());
    if !drag.moved {
        if !drag.resize { if let Err(e) = select_band(state, &drag.table, Band::Table) { state.action_status = Some(e); } }
        state.needs_present = true;
        return true;
    }
    let revision = state.editor.revision;
    let scale = state.viewport.points_to_pixels().max(0.01);
    let result = if drag.resize {
        let (fx, fy) = drag.factors();
        let widths = drag.widths_pt.iter().map(|w| (w * fx / MM_TO_PT).clamp(2.0, 500.0)).collect();
        let heights = if (fy - 1.0).abs() < 0.01 { drag.declared.clone() }
            else { drag.heights_pt.iter().map(|h| Some((h * fy / MM_TO_PT).clamp(2.0, 500.0))).collect() };
        state.editor.scale_table(&table_id, widths, heights)
    } else if let Some(position) = drag.free.clone() {
        match free_drop(state, &table_id, drag.ghost(), position, drag.horizontal_only) {
            Ok((anchor, position)) => state.editor.set_table_position(&table_id, anchor, Some(position)),
            Err(e) => Err(e),
        }
    } else {
        let ghost = drag.ghost();
        let target = if drag.horizontal_only { None } else {
            drop_target(state, &table_id, Point::new(ghost.x + 4.0, ghost.y))
        };
        let indent = new_indent(&drag, scale);
        state.editor.move_table(&table_id, target, Some(indent))
    };
    if let Err(e) = result { state.action_status = Some(e); } else { state.action_status = None; }
    state.selected_object = None;
    state.table_band = None;
    if state.editor.revision != revision { submit_layout(state); }
    refresh_overlay(state);
    state.needs_present = true;
    true
}

pub fn cancel_drag(state: &mut CanvasState) -> bool {
    if state.band_drag.take().is_some() { return true; }
    if state.table_drag.take().is_none() { return false; }
    state.action_status = Some("Table drag cancelled — document unchanged".into());
    state.needs_present = true;
    true
}

fn paragraph_in_table(state: &CanvasState, table: &NodeId, paragraph: &NodeId) -> bool {
    fn contains(blocks: &[model::Block], id: &NodeId) -> bool {
        blocks.iter().any(|b| match b {
            model::Block::Paragraph(p) => &p.id == id,
            model::Block::Table(t) => t.rows.iter().any(|r| r.cells.iter().any(|c| contains(&c.blocks, id))),
            _ => false,
        })
    }
    state.editor.table_by_id(table).is_some_and(|t| t.rows.iter().any(|r| r.cells.iter().any(|c| contains(&c.blocks, paragraph))))
}

/// Handles a click on a table control. Returns true if one was hit.
pub fn down(state: &mut CanvasState, p: Point) -> bool {
    hover(state, Some(p));
    let Some(Hover { table, page, target: Some(target) }) = state.table_hover.clone() else { return false; };
    let result = match target {
        Target::Corner => start_drag(state, &table, page, p, false),
        Target::Resize => start_drag(state, &table, page, p, true),
        Target::Column(k) | Target::Row(k) => {
            let columns = matches!(target, Target::Column(_));
            state.band_drag = Some(BandDrag { table: table.clone(), page, columns, start: k, end: k });
            select_band(state, &table, if columns { Band::Column(k, k) } else { Band::Row(k, k) })
        }
        Target::InsertColumn(k) => insert_at(state, &table, true, k),
        Target::InsertRow(r) => insert_at(state, &table, false, r),
        Target::Delete => delete_band(state).map(|_| ()),
    };
    if let Err(error) = result { state.action_status = Some(error); }
    hover(state, Some(p));
    state.needs_present = true;
    true
}

/// (row, cell, node id) of a cell covering logical column `column`,
/// preferring unmerged cells; `edge` picks the cell starting (Some(false)) or
/// ending (Some(true)) at that column instead.
fn find_cell(state: &CanvasState, table: &str, want: impl Fn(&TableCellHit) -> bool, rank: impl Fn(&TableCellHit) -> usize) -> Option<(usize, usize, NodeId)> {
    let mut best: Option<(usize, usize, usize, NodeId)> = None;
    for page in 0..state.vector_document.pages.len() {
        for (_, info, id) in cells(state, page, table) {
            if !want(info) { continue; }
            let score = rank(info);
            if best.as_ref().is_none_or(|b| score < b.0) { best = Some((score, info.row_index, info.cell_index, NodeId::from(id))); }
        }
    }
    best.map(|(_, row, cell, id)| (row, cell, id))
}

fn band_cell(state: &CanvasState, table: &str, band: &Band) -> Option<(usize, usize, NodeId)> {
    match *band {
        Band::Column(k, _) => find_cell(state, table, |c| c.column_index <= k && k < c.column_index + c.column_span,
            |c| (c.column_span != 1) as usize * 1000 + c.row_index),
        Band::Row(r, _) => find_cell(state, table, |c| c.row_index == r, |c| c.cell_index),
        Band::Table => find_cell(state, table, |_| true, |c| c.row_index * 1000 + c.cell_index),
        Band::Cell(ref id) => {
            (0..state.vector_document.pages.len()).find_map(|page| cells(state, page, table).find(|(_, _, node)| node == id)
                .map(|(_, info, node)| (info.row_index, info.cell_index, NodeId::from(node))))
        }
    }
}

fn select_band(state: &mut CanvasState, table: &str, band: Band) -> Result<(), String> {
    let (row, cell, cell_id) = band_cell(state, table, &band).ok_or("That part of the table is not laid out yet.")?;
    let table_id = NodeId::from(table);
    state.editor.select_cell(&table_id, row, cell)?;
    let (mode, span) = match band { Band::Cell(_) => (0, None), Band::Row(a, b) => (1, Some((a, b, false))), Band::Column(a, b) => (2, Some((a, b, true))), Band::Table => (3, None) };
    let mut cell_ids = state.editor.table_selection_band(&table_id, &cell_id, mode)?;
    // Several rows or columns: the union of each one's cells.
    if let Some((a, b, columns)) = span {
        for k in a + 1..=b {
            let one = if columns { Band::Column(k, k) } else { Band::Row(k, k) };
            let Some((_, _, id)) = band_cell(state, table, &one) else { continue; };
            for cell in state.editor.table_selection_band(&table_id, &id, mode)? { if !cell_ids.contains(&cell) { cell_ids.push(cell); } }
        }
    }
    state.selected_object = Some(objects_ui::SelectedObject::Cells { table_id, cell_ids });
    state.action_status = Some(match band {
        Band::Column(a, b) if a != b => format!("Columns {}–{} selected — × deletes them; Delete clears their text", a + 1, b + 1),
        Band::Row(a, b) if a != b => format!("Rows {}–{} selected — × deletes them; Delete clears their text", a + 1, b + 1),
        Band::Column(k, _) => format!("Column {} selected — × deletes it; Delete clears its text", k + 1),
        Band::Row(r, _) => format!("Row {} selected — × deletes it; Delete clears its text", r + 1),
        Band::Table => "Table selected — × deletes it; Delete clears its text".into(),
        Band::Cell(_) => "Cell selected — × deletes it; Delete clears its text".into(),
    });
    state.table_band = Some((table.to_string(), band));
    refresh_overlay(state);
    Ok(())
}

fn insert_at(state: &mut CanvasState, table: &str, column: bool, at: usize) -> Result<(), String> {
    let revision = state.editor.revision;
    let (target, after) = if column {
        match find_cell(state, table, |c| c.column_index == at, |c| c.row_index) {
            Some(cell) => (cell, false),
            None => (find_cell(state, table, |c| c.column_index + c.column_span == at, |c| c.row_index).ok_or("No cell borders that column.")?, true),
        }
    } else {
        match find_cell(state, table, |c| c.row_index == at, |c| c.cell_index) {
            Some(cell) => (cell, false),
            None => (find_cell(state, table, |c| c.row_index + 1 == at, |c| c.cell_index).ok_or("No cell borders that row.")?, true),
        }
    };
    let table_id = NodeId::from(table);
    state.editor.select_cell(&table_id, target.0, target.1)?;
    let result = if column { state.editor.insert_table_column(after) } else { state.editor.insert_table_row(after) };
    finish(state, revision);
    result
}

/// Deletes the row, column, table or cell selected (the × button). Returns false
/// when no such selection exists.
pub fn delete_band(state: &mut CanvasState) -> Result<bool, String> {
    let Some((table, band)) = valid_band(state) else { return Ok(false); };
    let revision = state.editor.revision;
    let (row, cell, _) = band_cell(state, &table, &band).ok_or("That part of the table is not laid out yet.")?;
    let table_id = NodeId::from(table.as_str());
    state.editor.select_cell(&table_id, row, cell)?;
    // Every row or every column selected: that is the whole table.
    let columns = (0..state.vector_document.pages.len()).find_map(|page| cells(state, page, &table).next().map(|c| c.1.column_widths_pt.len())).unwrap_or(0);
    let everything = state.editor.table_by_id(&table_id).is_some_and(|t| match band {
        Band::Row(a, b) => a == 0 && b + 1 >= t.rows.len(),
        Band::Column(a, b) => a == 0 && b + 1 >= columns,
        _ => false,
    });
    let result = match band {
        _ if everything => state.editor.delete_table(),
        Band::Column(a, b) => state.editor.delete_table_band(&table_id, true, a, b),
        Band::Row(a, b) => state.editor.delete_table_band(&table_id, false, a, b),
        Band::Table => state.editor.delete_table(),
        Band::Cell(_) => state.editor.shift_table_cells(false, false),
    };
    finish(state, revision);
    result.map(|_| true)
}

fn finish(state: &mut CanvasState, revision: u64) {
    state.selected_object = None;
    state.table_band = None;
    state.table_hover = None;
    if state.editor.revision != revision { submit_layout(state); }
    refresh_overlay(state);
    state.needs_present = true;
}

/// Sizes and settings of the cell holding the caret.
pub fn facts(state: &CanvasState) -> Option<CellFacts> {
    let context = state.editor.table_cell_context()?;
    let table = state.editor.table_by_id(&context.table_id)?;
    let row = table.rows.get(context.row_index)?;
    let cell = row.cells.get(context.cell_index)?;
    let info = state.vector_document.pages.iter().flat_map(|p| p.object_regions.iter()).find_map(|r| match &r.kind {
        ObjectKind::TableCell(info) if r.node_id == context.cell_id.0 => Some(info.clone()),
        _ => None,
    });
    let width_pt = info.as_ref().and_then(|i| i.column_widths_pt.get(i.column_index).copied());
    let width_mm = width_pt.map(|w| w / MM_TO_PT)
        .or_else(|| table.properties.grid_column_widths_mm.get(context.cell_index).copied()).unwrap_or(25.0);
    Some(CellFacts {
        width_mm,
        height_mm: row.height_mm,
        actual_height_mm: info.as_ref().map_or(row.height_mm.unwrap_or(8.0), |i| i.row_height_pt / MM_TO_PT),
        header: row.repeat_as_header,
        align: match cell.vertical_alignment { CellVerticalAlignment::Top => 0, CellVerticalAlignment::Center => 1, _ => 2 },
        merged: cell.column_span > 1 || cell.row_span > 1,
        position: table.properties.position.clone(),
        movable: state.editor.document.sections.iter().any(|s| s.blocks.iter().any(|b| matches!(b, model::Block::Table(t) if t.id == context.table_id))),
    })
}

type Edit = Box<dyn FnOnce(&mut EditorState) -> Result<(), String>>;

/// The editor change a Table bar command performs, resolved against the
/// current layout (for size steps).
pub fn operation(state: &CanvasState, op: TableOp) -> Result<Edit, String> {
    let facts = facts(state).ok_or("Place the caret in a table first")?;
    let step = |value: f32, delta: i8| (value.round() + delta as f32).clamp(2.0, 500.0);
    Ok(match op {
        TableOp::InsertRowAbove => Box::new(|e| e.insert_table_row(false)),
        TableOp::InsertRowBelow => Box::new(|e| e.insert_table_row(true)),
        TableOp::InsertColumnLeft => Box::new(|e| e.insert_table_column(false)),
        TableOp::InsertColumnRight => Box::new(|e| e.insert_table_column(true)),
        TableOp::InsertCellsRight => Box::new(|e| e.shift_table_cells(true, false)),
        TableOp::InsertCellsDown => Box::new(|e| e.shift_table_cells(true, true)),
        TableOp::DeleteRow => Box::new(|e| e.delete_table_row()),
        TableOp::DeleteColumn => Box::new(|e| e.delete_table_column()),
        TableOp::DeleteTable => Box::new(|e| e.delete_table()),
        TableOp::DeleteCellsLeft => Box::new(|e| e.shift_table_cells(false, false)),
        TableOp::DeleteCellsUp => Box::new(|e| e.shift_table_cells(false, true)),
        TableOp::MergeRight => Box::new(|e| e.merge_table_cell_right()),
        TableOp::MergeDown => Box::new(|e| e.merge_table_cell_down()),
        TableOp::Split => Box::new(|e| e.split_table_cell()),
        TableOp::Distribute => Box::new(|e| e.distribute_table_columns_evenly()),
        TableOp::DistributeRows | TableOp::FitToMargins => {
            let context = state.editor.table_cell_context().ok_or("Place the caret in a table first")?;
            let mut table_cells = (0..state.vector_document.pages.len())
                .flat_map(|page| cells(state, page, &context.table_id.0));
            if op == TableOp::DistributeRows {
                let height = table_cells.map(|(_, info, _)| info.row_height_pt).reduce(f32::max)
                    .ok_or("Wait for the table layout to finish.")?;
                Box::new(move |e| e.distribute_table_rows_evenly(height / MM_TO_PT))
            } else {
                let widths = table_cells.next().ok_or("Wait for the table layout to finish.")?.1.column_widths_pt.clone();
                Box::new(move |e| e.fit_table_to_margins(&widths))
            }
        }
        TableOp::Align(a) => {
            let align = match a { 0 => CellVerticalAlignment::Top, 1 => CellVerticalAlignment::Center, _ => CellVerticalAlignment::Bottom };
            Box::new(move |e| e.set_table_cell_vertical_alignment(align))
        }
        TableOp::HeaderRow => { let on = !facts.header; Box::new(move |e| e.set_table_row_repeat_header(on)) }
        TableOp::Width(d) => { let w = step(facts.width_mm, d); Box::new(move |e| e.set_table_column_width(w)) }
        TableOp::Height(d) => { let h = step(facts.height_mm.unwrap_or(facts.actual_height_mm), d); Box::new(move |e| e.set_table_row_height(Some(h))) }
        TableOp::HeightAuto => Box::new(|e| e.set_table_row_height(None)),
        TableOp::Float(free) => {
            if !facts.movable { return Err("Only tables in the main text can float; header and nested tables stay in line.".into()); }
            let id = state.editor.table_cell_context().ok_or("Place the caret in a table first")?.table_id;
            // Start where the table is: at its indent, at the top of the paragraph it precedes.
            let indent = state.editor.table_by_id(&id).and_then(|t| t.properties.left_indent_mm).unwrap_or(0.0);
            let position = free.then(|| facts.position.clone().unwrap_or(model::TablePosition {
                horizontal: model::HorizontalPlacement::Offset(indent), ..Default::default() }));
            Box::new(move |e| e.set_table_position(&id, None, position))
        }
        TableOp::WrapAround(around) => {
            let id = state.editor.table_cell_context().ok_or("Place the caret in a table first")?.table_id;
            let mut position = facts.position.clone().ok_or("Make the table free first")?;
            position.wrap = if around { model::TableWrap::Around } else { model::TableWrap::TopAndBottom };
            Box::new(move |e| e.set_table_position(&id, None, Some(position)))
        }
    })
}

/// Controls of the shown table plus resize guides, for this frame.
pub fn chrome(state: &CanvasState) -> Vec<ChromeShape> {
    let mut out = Vec::new();
    if crate::equation_ui::previewing(state) || state.display_generation != Some(state.layout_generation) { return out; }
    let m = Metrics::of(state);
    selection(state, &mut out, &m);
    if let Some((page, table, column, at)) = objects_ui::resize_guide(state) {
        if let (Some(g), Some(origin)) = (geometry(state, page, &table), state.viewport.page_rect_px(&state.vector_document, page)) {
            let scale = state.viewport.points_to_pixels();
            let ext = 6.0 * m.s;
            let (from, to) = if column {
                let x = origin.x + at * scale;
                (Point::new(x, g.top - ext), Point::new(x, g.bottom + ext))
            } else {
                let y = origin.y + at * scale;
                (Point::new(g.left - ext, y), Point::new(g.right + ext, y))
            };
            out.push(ChromeShape::Line { from, to, width: 2.0 * m.s, color: BLUE });
            for end in [from, to] { out.push(ChromeShape::Circle { center: end, radius: 3.0 * m.s, fill: Some(BLUE), stroke: None }); }
        }
    }
    if state.object_drag.is_some() { return out; }
    if let Some(drag) = &state.table_drag {
        if drag.moved {
            out.push(ChromeShape::RoundRect { rect: drag.ghost(), radius: 2.0 * m.s, fill: Some(SELECTION_FILL), stroke: Some((BLUE, 1.5 * m.s)) });
            if let Some(snap)=drag.snapped {
                if let Some(x)=snap.x {out.push(ChromeShape::Line {from:Point::new(x,snap.margins.y),to:Point::new(x,snap.margins.max_y()),width:m.s,color:BLUE});}
                if let Some(y)=snap.y {out.push(ChromeShape::Line {from:Point::new(snap.margins.x,y),to:Point::new(snap.margins.max_x(),y),width:m.s,color:BLUE});}
            }
            if !drag.resize && !drag.horizontal_only && drag.free.is_none() {
                // Where the table will go: before the paragraph under its top edge.
                let g = drag.ghost();
                out.push(ChromeShape::Line { from: Point::new(g.x - 8.0 * m.s, g.y), to: Point::new(g.x + g.width + 8.0 * m.s, g.y), width: 2.5 * m.s, color: BLUE });
            }
        }
        return out;
    }
    let Some(table) = shown_table(state) else { return out; };
    let band = valid_band(state).filter(|b| b.0 == table).map(|b| b.1);
    let target = state.table_hover.as_ref().filter(|h| h.table == table).and_then(|h| h.target);
    let hot_page = state.table_hover.as_ref().map(|h| h.page);
    for g in geometries(state, &table) {
        let on_page = hot_page.is_none_or(|p| p == g.page);
        let target = if on_page { target } else { None };
        let (bar, gap) = (m.bar(), m.bar_gap());
        let inset = 2.0 * m.s;
        // Column handles.
        for k in 0..g.cols.len().saturating_sub(1) {
            let selected = band.as_ref().is_some_and(|b| b.has_column(k));
            let hot = target == Some(Target::Column(k));
            let grow = if hot || selected { 2.0 * m.s } else { 0.0 };
            let rect = Rect::new(g.cols[k] + inset, g.top - gap - bar - grow, (g.cols[k + 1] - g.cols[k] - 2.0 * inset).max(2.0), bar + grow);
            out.push(ChromeShape::RoundRect { rect, radius: 3.0 * m.s, fill: Some(if selected { BLUE } else if hot { HANDLE_HOT } else { HANDLE }), stroke: None });
            if hot || selected { grip_dots(&mut out, Point::new(rect.x + rect.width * 0.5, rect.y + rect.height * 0.5), true, &m, selected); }
        }
        // Row handles.
        for &(r, top, bottom) in &g.rows {
            let selected = band.as_ref().is_some_and(|b| b.has_row(r));
            let hot = target == Some(Target::Row(r));
            let grow = if hot || selected { 2.0 * m.s } else { 0.0 };
            let rect = Rect::new(g.left - gap - bar - grow, top + inset, bar + grow, (bottom - top - 2.0 * inset).max(2.0));
            out.push(ChromeShape::RoundRect { rect, radius: 3.0 * m.s, fill: Some(if selected { BLUE } else if hot { HANDLE_HOT } else { HANDLE }), stroke: None });
            if hot || selected { grip_dots(&mut out, Point::new(rect.x + rect.width * 0.5, rect.y + rect.height * 0.5), false, &m, selected); }
        }
        // Corner: select the table.
        {
            let selected = band == Some(Band::Table);
            let hot = target == Some(Target::Corner);
            let size = bar + 6.0 * m.s;
            let rect = Rect::new(g.left - gap - size, g.top - gap - size, size, size);
            out.push(ChromeShape::RoundRect { rect, radius: 3.0 * m.s, fill: Some(if selected { BLUE } else if hot { HANDLE_HOT } else { HANDLE }), stroke: None });
            let ink = if selected || hot { Color::WHITE } else { Color::rgb8(72, 98, 150) };
            let (cx, cy, d) = (rect.x + size * 0.5, rect.y + size * 0.5, 2.2 * m.s);
            for (dx, dy) in [(-d, -d), (d, -d), (-d, d), (d, d)] {
                out.push(ChromeShape::Circle { center: Point::new(cx + dx, cy + dy), radius: 1.1 * m.s, fill: Some(ink), stroke: None });
            }
        }
        // Resize handle at the bottom-right corner.
        resize_handle(&mut out, Point::new(g.right, g.bottom), target == Some(Target::Resize), &m);
        // "+" insertion marker and guide.
        match target {
            Some(Target::InsertColumn(k)) => if let Some(&x) = g.cols.get(k) {
                let center = Point::new(x, g.top - gap - bar - m.plus() - 3.0 * m.s);
                out.push(ChromeShape::Line { from: Point::new(x, center.y), to: Point::new(x, g.bottom), width: 2.0 * m.s, color: BLUE });
                plus(&mut out, center, &m);
            },
            Some(Target::InsertRow(r)) => {
                let y = g.rows.iter().find(|row| row.0 == r).map(|row| row.1).or_else(|| g.rows.last().filter(|row| row.0 + 1 == r).map(|row| row.2));
                if let Some(y) = y {
                    let center = Point::new(g.left - gap - bar - m.plus() - 3.0 * m.s, y);
                    out.push(ChromeShape::Line { from: Point::new(center.x, y), to: Point::new(g.right, y), width: 2.0 * m.s, color: BLUE });
                    plus(&mut out, center, &m);
                }
            }
            _ => {}
        }
        // Delete button beside the selected handle.
        if let Some(band) = &band {
            if let Some(c) = chip_center(state, &g, &table, band, &m) {
                delete_chip(&mut out, c, target == Some(Target::Delete), &m);
            }
        }
    }
    out
}

/// A square resize handle centred on `center`: white with a blue rim, filled
/// blue while the pointer is on it.
pub(crate) fn resize_handle(out: &mut Vec<ChromeShape>, center: Point, hot: bool, m: &Metrics) {
    let size = m.resize_handle(hot);
    let rect = Rect::new(center.x - size * 0.5, center.y - size * 0.5, size, size);
    out.push(ChromeShape::RoundRect { rect, radius: 1.5 * m.s, fill: Some(if hot { BLUE } else { Color::WHITE }), stroke: Some((BLUE, 1.2 * m.s)) });
}

/// The round × button that deletes the selected object: a red × on white,
/// white on red while the pointer is on it.
pub(crate) fn delete_chip(out: &mut Vec<ChromeShape>, c: Point, hot: bool, m: &Metrics) {
    let r = m.chip();
    out.push(ChromeShape::Circle { center: Point::new(c.x, c.y + m.s), radius: r + m.s, fill: Some(Color::rgba(0.1, 0.16, 0.28, 0.14)), stroke: None });
    out.push(ChromeShape::Circle { center: c, radius: r, fill: Some(if hot { RED } else { Color::WHITE }),
        stroke: Some((if hot { RED } else { Color::rgb8(214, 220, 230) }, m.s)) });
    let ink = if hot { Color::WHITE } else { RED };
    let d = 3.3 * m.s;
    out.push(ChromeShape::Line { from: Point::new(c.x - d, c.y - d), to: Point::new(c.x + d, c.y + d), width: 1.6 * m.s, color: ink });
    out.push(ChromeShape::Line { from: Point::new(c.x - d, c.y + d), to: Point::new(c.x + d, c.y - d), width: 1.6 * m.s, color: ink });
}

fn plus(out: &mut Vec<ChromeShape>, c: Point, m: &Metrics) {
    let r = m.plus();
    out.push(ChromeShape::Circle { center: Point::new(c.x, c.y + m.s), radius: r + m.s, fill: Some(Color::rgba(0.1, 0.2, 0.45, 0.18)), stroke: None });
    out.push(ChromeShape::Circle { center: c, radius: r, fill: Some(BLUE), stroke: None });
    let d = r * 0.48;
    out.push(ChromeShape::Line { from: Point::new(c.x - d, c.y), to: Point::new(c.x + d, c.y), width: 1.7 * m.s, color: Color::WHITE });
    out.push(ChromeShape::Line { from: Point::new(c.x, c.y - d), to: Point::new(c.x, c.y + d), width: 1.7 * m.s, color: Color::WHITE });
}

fn grip_dots(out: &mut Vec<ChromeShape>, c: Point, horizontal: bool, m: &Metrics, selected: bool) {
    let ink = if selected { Color::rgba(1.0, 1.0, 1.0, 0.9) } else { Color::rgba(1.0, 1.0, 1.0, 0.95) };
    for i in -1..=1 {
        let o = i as f32 * 3.2 * m.s;
        let center = if horizontal { Point::new(c.x + o, c.y) } else { Point::new(c.x, c.y + o) };
        out.push(ChromeShape::Circle { center, radius: 0.95 * m.s, fill: Some(ink), stroke: None });
    }
}

/// Selected cells: a light tint over each cell and one crisp outline around
/// the whole selection on every page.
fn selection(state: &CanvasState, out: &mut Vec<ChromeShape>, m: &Metrics) {
    let Some(objects_ui::SelectedObject::Cells { table_id, cell_ids }) = &state.selected_object else { return; };
    let scale = state.viewport.points_to_pixels();
    for page in state.viewport.visible_pages(&state.vector_document) {
        let Some(origin) = state.viewport.page_rect_px(&state.vector_document, page) else { continue; };
        let mut union: Option<(f32, f32, f32, f32)> = None;
        for (rect, _, id) in cells(state, page, &table_id.0) {
            if !cell_ids.iter().any(|c| c.0 == id) { continue; }
            let r = Rect::new(origin.x + rect.x * scale, origin.y + rect.y * scale, rect.width * scale, rect.height * scale);
            out.push(ChromeShape::RoundRect { rect: r, radius: 0.0, fill: Some(SELECTION_FILL), stroke: None });
            union = Some(match union {
                None => (r.x, r.y, r.x + r.width, r.y + r.height),
                Some(u) => (u.0.min(r.x), u.1.min(r.y), u.2.max(r.x + r.width), u.3.max(r.y + r.height)),
            });
        }
        if let Some((x0, y0, x1, y1)) = union {
            out.push(ChromeShape::RoundRect { rect: Rect::new(x0, y0, x1 - x0, y1 - y0), radius: 1.5 * m.s, fill: None, stroke: Some((BLUE, 1.5 * m.s)) });
        }
    }
}

/// The paragraph line nearest to `at` (outside the moved table): the table
/// goes before it when `at` is above the line's middle, else after it.
fn drop_target(state: &CanvasState, table: &NodeId, at: Point) -> Option<(NodeId, bool)> {
    let scale = state.viewport.points_to_pixels().max(0.01);
    let rects = state.viewport.page_rects_px(&state.vector_document);
    let (page, rect) = rects.iter().enumerate()
        .min_by(|a, b| distance(*a.1, at).total_cmp(&distance(*b.1, at)))?;
    let q = Point::new((at.x - rect.x) / scale, (at.y - rect.y) / scale);
    let region = state.vector_document.pages[page].hit_regions.iter()
        .filter(|r| !paragraph_in_table(state, table, &NodeId::from(r.node_id.as_str())))
        .min_by(|a, b| {
            let d = |r: &render::TextHitRegion| { let c = r.rect.y + r.rect.height * 0.5; (q.y - c).abs() };
            d(a).total_cmp(&d(b))
        })?;
    Some((NodeId::from(region.node_id.as_str()), q.y < region.rect.y + region.rect.height * 0.5))
}

fn distance(r: Rect, p: Point) -> f32 {
    let dx = (r.x - p.x).max(p.x - r.x - r.width).max(0.0);
    let dy = (r.y - p.y).max(p.y - r.y - r.height).max(0.0);
    dx * dx + dy * dy
}

/// Where a floating table dropped at `ghost` (view pixels) goes: the
/// paragraph it will be anchored to (placed before), and its new position
/// in the same reference frames as before, as offsets.
fn free_drop(state: &CanvasState, table: &NodeId, ghost: Rect, mut position: model::TablePosition, sideways: bool) -> Result<(Option<NodeId>, model::TablePosition), String> {
    let scale = state.viewport.points_to_pixels().max(0.01);
    let rects = state.viewport.page_rects_px(&state.vector_document);
    let at = Point::new(ghost.x, ghost.y);
    let (page, origin) = rects.iter().enumerate().min_by(|a, b| distance(*a.1, at).total_cmp(&distance(*b.1, at))).map(|(i, r)| (i, *r)).ok_or("No page")?;
    let q = Point::new((ghost.x - origin.x) / scale, (ghost.y - origin.y) / scale);
    // Top-level paragraphs that start on this page, by the top of their first line.
    let document = &state.editor.document;
    let mut section_of = std::collections::HashMap::new();
    for (index, section) in document.sections.iter().enumerate() {
        for block in section.blocks.iter() { if let model::Block::Paragraph(p) = block { section_of.insert(p.id.0.clone(), index); } }
    }
    let first_line = |page: usize| {
        let mut tops: std::collections::BTreeMap<String, f32> = std::collections::BTreeMap::new();
        if let Some(p) = state.vector_document.pages.get(page) {
            for r in &p.hit_regions { if section_of.contains_key(&r.node_id) { let t = tops.entry(r.node_id.clone()).or_insert(f32::MAX); *t = t.min(r.rect.y); } }
        }
        tops
    };
    let previous = if page > 0 { first_line(page - 1) } else { Default::default() };
    let starts: Vec<(String, f32)> = first_line(page).into_iter().filter(|(id, _)| !previous.contains_key(id) && !paragraph_in_table(state, table, &NodeId::from(id.as_str()))).collect();
    let anchor = starts.iter().filter(|(_, y)| *y <= q.y + 0.5).max_by(|a, b| a.1.total_cmp(&b.1))
        .or_else(|| starts.iter().min_by(|a, b| a.1.total_cmp(&b.1)))
        .ok_or("Drop the table on a page with text to anchor it to.")?;
    let section = &document.sections[section_of[&anchor.0]];
    let (margin_left, margin_top) = (section.properties.margin_left_mm * MM_TO_PT, section.properties.margin_top_mm * MM_TO_PT);
    let mm = |pt: f32| ((pt / MM_TO_PT) * 10.0).round() / 10.0;
    let x = match position.horizontal_from { model::HorizontalFrom::Page => q.x, _ => q.x - margin_left };
    position.horizontal = model::HorizontalPlacement::Offset(mm(x).clamp(-500.0, 500.0));
    // Shift: sideways only, keeping the anchor and vertical placement.
    if sideways { return Ok((None, position)); }
    let y = match position.vertical_from { model::VerticalFrom::Page => q.y, model::VerticalFrom::Margin => q.y - margin_top, model::VerticalFrom::Paragraph => q.y - anchor.1 };
    position.vertical = model::VerticalPlacement::Offset(mm(y).clamp(-500.0, 500.0));
    Ok((Some(NodeId::from(anchor.0.as_str())), position))
}

#[cfg(test)] #[path="../tests/unit/table_drag_snap.rs"] mod snap_tests;
