//! Table Position: exact placement of a free (floating) table, as in Word's
//! Table Positioning dialog, with a live sketch of the page.
use super::*;
use super::format_dialogs::Chrome;
use crate::TablePositionDialogUi;
use model::{HorizontalFrom, HorizontalPlacement, TablePosition, TableWrap, VerticalFrom, VerticalPlacement};
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap};
use wxdragon::{dc::AutoBufferedPaintDC, event::window_events::WindowEvents, window::BackgroundStyle};

const H_MODES: [&str; 6] = ["Offset", "Left", "Center", "Right", "Inside", "Outside"];
const V_MODES: [&str; 4] = ["Offset", "Top", "Center", "Bottom"];

/// What the sketch needs: page and margins, and the table's size (mm).
#[derive(Clone, Copy)]
struct Sheet { page: (f32, f32), margins: (f32, f32, f32, f32), table: (f32, f32) }

fn read(ui: &TablePositionDialogUi) -> TablePosition {
    let h = ui.tpos_h_value.get_value() as f32;
    let v = ui.tpos_v_value.get_value() as f32;
    TablePosition {
        horizontal: match ui.tpos_h_mode.get_selection().unwrap_or(0) {
            1 => HorizontalPlacement::Left, 2 => HorizontalPlacement::Center, 3 => HorizontalPlacement::Right,
            4 => HorizontalPlacement::Inside, 5 => HorizontalPlacement::Outside, _ => HorizontalPlacement::Offset(h),
        },
        horizontal_from: match ui.tpos_h_from.get_selection().unwrap_or(0) { 1 => HorizontalFrom::Page, 2 => HorizontalFrom::Column, _ => HorizontalFrom::Margin },
        vertical: match ui.tpos_v_mode.get_selection().unwrap_or(0) {
            1 => VerticalPlacement::Top, 2 => VerticalPlacement::Center, 3 => VerticalPlacement::Bottom, _ => VerticalPlacement::Offset(v),
        },
        vertical_from: match ui.tpos_v_from.get_selection().unwrap_or(0) { 1 => VerticalFrom::Margin, 2 => VerticalFrom::Page, _ => VerticalFrom::Paragraph },
        distance_top_mm: ui.tpos_top.get_value() as f32, distance_bottom_mm: ui.tpos_bottom.get_value() as f32,
        distance_left_mm: ui.tpos_left.get_value() as f32, distance_right_mm: ui.tpos_right.get_value() as f32,
        wrap: if ui.tpos_topbottom.get_value() { TableWrap::TopAndBottom } else { TableWrap::Around },
        allow_overlap: ui.tpos_overlap.get_value(),
    }
}

fn write(ui: &TablePositionDialogUi, p: &TablePosition) {
    let (h_mode, h) = match p.horizontal {
        HorizontalPlacement::Offset(v) => (0, v), HorizontalPlacement::Left => (1, 0.0), HorizontalPlacement::Center => (2, 0.0),
        HorizontalPlacement::Right => (3, 0.0), HorizontalPlacement::Inside => (4, 0.0), HorizontalPlacement::Outside => (5, 0.0),
    };
    let (v_mode, v) = match p.vertical {
        VerticalPlacement::Offset(v) => (0, v), VerticalPlacement::Top => (1, 0.0), VerticalPlacement::Center => (2, 0.0), VerticalPlacement::Bottom => (3, 0.0),
    };
    ui.tpos_h_mode.set_selection(h_mode); ui.tpos_h_value.set_value(h as f64);
    ui.tpos_h_from.set_selection(match p.horizontal_from { HorizontalFrom::Margin => 0, HorizontalFrom::Page => 1, HorizontalFrom::Column => 2 });
    ui.tpos_v_mode.set_selection(v_mode); ui.tpos_v_value.set_value(v as f64);
    ui.tpos_v_from.set_selection(match p.vertical_from { VerticalFrom::Paragraph => 0, VerticalFrom::Margin => 1, VerticalFrom::Page => 2 });
    ui.tpos_top.set_value(p.distance_top_mm as f64); ui.tpos_bottom.set_value(p.distance_bottom_mm as f64);
    ui.tpos_left.set_value(p.distance_left_mm as f64); ui.tpos_right.set_value(p.distance_right_mm as f64);
    ui.tpos_around.set_value(p.wrap == TableWrap::Around); ui.tpos_topbottom.set_value(p.wrap == TableWrap::TopAndBottom);
    ui.tpos_overlap.set_value(p.allow_overlap);
}

/// The table's rectangle on the sketch page (mm), mirroring the layout
/// engine's placement; the anchor paragraph sits a third of the way down.
fn place(p: &TablePosition, s: Sheet) -> (f32, f32, f32, f32) {
    let (pw, ph) = s.page; let (ml, mt, mr, mb) = s.margins; let (w, h) = s.table;
    let (left, span) = match p.horizontal_from { HorizontalFrom::Page => (0.0, pw), _ => (ml, pw - ml - mr) };
    let x = match p.horizontal {
        HorizontalPlacement::Offset(v) => left + v, HorizontalPlacement::Left | HorizontalPlacement::Inside => left,
        HorizontalPlacement::Center => left + (span - w) * 0.5, HorizontalPlacement::Right | HorizontalPlacement::Outside => left + span - w,
    };
    let paragraph = mt + (ph - mt - mb) / 3.0;
    let (top, extent) = match p.vertical_from { VerticalFrom::Page => (0.0, ph), VerticalFrom::Margin => (mt, ph - mt - mb), VerticalFrom::Paragraph => (paragraph, ph - mb - paragraph) };
    let y = match p.vertical { VerticalPlacement::Offset(v) => top + v, VerticalPlacement::Top => top, VerticalPlacement::Center => top + (extent - h) * 0.5, VerticalPlacement::Bottom => top + extent - h };
    (x.min(pw - w).max(0.0), y.min(ph - mb - h).max(0.0), w, h)
}

fn paint_sketch(panel: &wxdragon::widgets::panel::Panel, p: &TablePosition, s: Sheet) {
    let dc = AutoBufferedPaintDC::new(panel);
    let size = panel.get_client_size();
    let (w, h) = (size.width.max(1), size.height.max(1));
    let Some((mut pix, canvas)) = crate::crisp::Canvas::new(panel, w as i32, h as i32) else { return; };
    pix.fill(tiny_skia::Color::from_rgba8(238, 242, 247, 255));
    let pad = panel.from_dip_int(14) as f32;
    let k = ((w as f32 - 2.0 * pad) / s.page.0).min((h as f32 - 2.0 * pad) / s.page.1);
    let (ox, oy) = ((w as f32 - s.page.0 * k) * 0.5, (h as f32 - s.page.1 * k) * 0.5);
    let rect = |pix: &mut Pixmap, x: f32, y: f32, rw: f32, rh: f32, rgba: (u8, u8, u8, u8)| {
        let mut paint = Paint::default(); paint.set_color_rgba8(rgba.0, rgba.1, rgba.2, rgba.3); paint.anti_alias = true;
        if let Some(r) = tiny_skia::Rect::from_xywh(ox + x * k, oy + y * k, (rw * k).max(0.5), (rh * k).max(0.5)) {
            pix.fill_path(&PathBuilder::from_rect(r), &paint, FillRule::Winding, crate::crisp::transform(), None);
        }
    };
    for (grow, alpha) in [(3.0f32, 20u8), (1.5, 35)] { rect(&mut pix, -grow / k, (-grow + 2.0) / k, s.page.0 + 2.0 * grow / k, s.page.1 + 2.0 * grow / k, (40, 60, 90, alpha)); }
    rect(&mut pix, 0.0, 0.0, s.page.0, s.page.1, (255, 255, 255, 255));
    let (ml, mt, mr, mb) = s.margins;
    let (tx, ty, tw, th) = place(p, s);
    let (dl, dr, dt, db) = (p.distance_left_mm, p.distance_right_mm, p.distance_top_mm, p.distance_bottom_mm);
    // Text lines, wrapping around (or stopping above and below) the table.
    let line_h = 4.2;
    let mut y = mt;
    while y + 1.6 <= s.page.1 - mb {
        let blocked = y + 1.6 > ty - dt && y < ty + th + db;
        let spans: Vec<(f32, f32)> = if !blocked { vec![(ml, s.page.0 - mr)] }
            else if p.wrap == TableWrap::TopAndBottom { Vec::new() }
            else { vec![(ml, (tx - dl).min(s.page.0 - mr)), ((tx + tw + dr).max(ml), s.page.0 - mr)] };
        for (a, b) in spans { if b - a > 6.0 { rect(&mut pix, a, y, b - a, 1.6, (196, 204, 216, 255)); } }
        y += line_h;
    }
    // Distance from text, then the table.
    rect(&mut pix, tx - dl, ty - dt, tw + dl + dr, th + dt + db, (37, 99, 235, 28));
    rect(&mut pix, tx, ty, tw, th, (37, 99, 235, 80));
    for i in 1..3 { rect(&mut pix, tx + tw * i as f32 / 3.0, ty, 0.3, th, (37, 99, 235, 200)); rect(&mut pix, tx, ty + th * i as f32 / 3.0, tw, 0.3, (37, 99, 235, 200)); }
    for (x, y, rw, rh) in [(tx, ty, tw, 0.4), (tx, ty + th - 0.4, tw, 0.4), (tx, ty, 0.4, th), (tx + tw - 0.4, ty, 0.4, th)] { rect(&mut pix, x, y, rw, rh, (37, 99, 235, 255)); }
    canvas.present(&dc, &pix, 0, 0);
}

pub(super) fn table_position(ribbon: &Ribbon) -> Result<(), String> {
    let (id, current, sheet) = {
        let state = ribbon.state.borrow();
        let context = state.editor.table_cell_context().ok_or("Place the caret in a table first")?;
        let facts = crate::table_ui::facts(&state).ok_or("Place the caret in a table first")?;
        if !facts.movable { return Err("Only tables in the main text can be positioned freely.".into()); }
        let table = state.editor.table_by_id(&context.table_id).ok_or("Table not found")?;
        let indent = table.properties.left_indent_mm.unwrap_or(0.0);
        let current = table.properties.position.clone().unwrap_or(TablePosition { horizontal: HorizontalPlacement::Offset(indent), ..Default::default() });
        let props = &state.editor.document.sections[context.section_index].properties;
        // The table's laid-out size, from its cells.
        let (mut width, mut height) = (0.0f32, 0.0f32);
        let mut rows = std::collections::BTreeMap::new();
        for page in &state.vector_document.pages { for r in &page.object_regions {
            if let render::ObjectKind::TableCell(c) = &r.kind { if c.table_id == context.table_id.0 {
                width = width.max(c.column_widths_pt.iter().sum::<f32>());
                rows.entry(c.row_index).or_insert(c.row_height_pt);
            } }
        } }
        height = height.max(rows.values().sum::<f32>());
        let pt = 72.0 / 25.4;
        (context.table_id, current, Sheet {
            page: (props.page_width_mm, props.page_height_mm),
            margins: (props.margin_left_mm, props.margin_top_mm, props.margin_right_mm, props.margin_bottom_mm),
            table: ((width / pt).max(10.0), (height / pt).max(6.0)),
        })
    };
    let ui = Rc::new(crate::i18n::localized(TablePositionDialogUi::new(Some(&ribbon.ui.main_frame), false)));
    let chrome = Chrome::new(ui.tpos_dialog, ui.tpos_card, &[], &[], (ui.tpos_ok, "OK"), Some(ui.tpos_cancel), &[], &[]);
    let _ = (H_MODES, V_MODES);
    write(&ui, &current);
    let preview = ui.tpos_preview;
    preview.set_background_style(BackgroundStyle::Paint);
    preview.on_erase_background(|_| {});
    { let ui2 = Rc::clone(&ui); preview.on_paint(move |_| paint_sketch(&ui2.tpos_preview, &read(&ui2), sheet)); }
    let refresh: Rc<dyn Fn()> = { let ui = Rc::clone(&ui); Rc::new(move || {
        ui.tpos_h_value.enable(ui.tpos_h_mode.get_selection().unwrap_or(0) == 0);
        ui.tpos_v_value.enable(ui.tpos_v_mode.get_selection().unwrap_or(0) == 0);
        ui.tpos_preview.refresh(false, None);
    }) };
    for combo in [ui.tpos_h_mode, ui.tpos_h_from, ui.tpos_v_mode, ui.tpos_v_from] { let r = Rc::clone(&refresh); combo.on_selection_changed(move |_| r()); }
    for spin in [ui.tpos_h_value, ui.tpos_v_value, ui.tpos_top, ui.tpos_bottom, ui.tpos_left, ui.tpos_right] { let r = Rc::clone(&refresh); spin.on_value_changed(move |_| r()); }
    for radio in [ui.tpos_around, ui.tpos_topbottom] { let r = Rc::clone(&refresh); radio.on_selected(move |_| r()); }
    refresh();
    let accepted = chrome.show();
    let position = read(&ui);
    ui.tpos_dialog.destroy();
    if !accepted { return Ok(()); }
    ribbon.edit(|e| e.set_table_position(&id, None, Some(position)))
}
