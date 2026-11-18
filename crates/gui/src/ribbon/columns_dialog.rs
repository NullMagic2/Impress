//! Columns dialog (Layout > Columns > More Columns…), as in Word: presets,
//! number of columns, line between, each column's width and spacing, equal
//! column width, "Apply to" with "Start new column", and a live preview.
//!
//! `ColumnsForm` holds the dialog's state and rules without any widgets, so
//! they are testable; `show` wires it to the XRC controls.
use super::*;
use super::format_dialogs::Chrome;
use crate::ColumnsDialogUi;
use editor::{ColumnLayout, ColumnPreset, ColumnScope, MAX_COLUMNS, MIN_COLUMN_WIDTH_MM};
use model::ColumnSpec;
use wxdragon::{dc::AutoBufferedPaintDC, event::window_events::WindowEvents, window::BackgroundStyle, DeviceContext};

const MM_PER_CM: f32 = 10.0;
/// Preset tiles: picture size and the gap between tiles, in DIP.
const TILE: (i32, i32) = (54, 62);
const TILE_STEP: i32 = 70;
const TILE_SELECTED: (u8, u8, u8) = (43, 111, 232);
/// Page picture in the preview, as a share of the panel.
const PREVIEW_PAGE_SHARE: f32 = 0.82;

/// The Columns dialog's state and editing rules.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ColumnsForm {
    pub layout: ColumnLayout,
    pub text_width_mm: f32,
}

impl ColumnsForm {
    pub fn new(layout: ColumnLayout, text_width_mm: f32) -> Self { Self { layout, text_width_mm } }

    pub fn equal(&self) -> bool { self.layout.columns.is_empty() }

    pub fn columns(&self) -> Vec<ColumnSpec> { self.layout.resolved(self.text_width_mm) }

    pub fn set_preset(&mut self, preset: ColumnPreset) {
        let line_between = self.layout.line_between;
        self.layout = ColumnLayout::preset(preset, self.text_width_mm);
        self.layout.line_between = line_between;
    }

    /// A new number of columns starts from equal widths, as in Word.
    pub fn set_count(&mut self, count: u16) {
        let count = count.clamp(1, MAX_COLUMNS);
        if count == self.layout.count { return; }
        let equal = self.equal();
        self.layout.count = count;
        self.layout.columns.clear();
        if !equal && count > 1 { self.layout.columns = self.columns(); }
    }

    pub fn set_equal(&mut self, equal: bool) {
        if equal == self.equal() { return; }
        if equal {
            self.layout.spacing_mm = self.layout.columns.first().map_or(self.layout.spacing_mm, |column| column.spacing_mm);
            self.layout.columns.clear();
        } else if self.layout.count > 1 {
            self.layout.columns = self.columns();
        }
    }

    /// Width of column `index` (0-based). With equal columns the spacing
    /// changes so the columns still fill the text width; otherwise the next
    /// column (the previous one, for the last) gives or takes the difference.
    pub fn set_width(&mut self, index: usize, width_mm: f32) {
        let count = usize::from(self.layout.count);
        if count < 2 || index >= count { return; }
        if self.equal() {
            self.layout.spacing_mm = ((self.text_width_mm - width_mm * count as f32) / (count - 1) as f32).max(0.0);
            return;
        }
        let neighbour = if index + 1 < count { index + 1 } else { index - 1 };
        let change = width_mm - self.layout.columns[index].width_mm;
        let room = self.layout.columns[neighbour].width_mm - MIN_COLUMN_WIDTH_MM;
        let change = change.min(room);
        self.layout.columns[index].width_mm += change;
        self.layout.columns[neighbour].width_mm -= change;
    }

    /// Spacing after column `index`; the next column absorbs the difference.
    pub fn set_spacing(&mut self, index: usize, spacing_mm: f32) {
        let count = usize::from(self.layout.count);
        if count < 2 || index + 1 >= count { return; }
        let spacing_mm = spacing_mm.max(0.0);
        if self.equal() { self.layout.spacing_mm = spacing_mm; return; }
        let change = spacing_mm - self.layout.columns[index].spacing_mm;
        let room = self.layout.columns[index + 1].width_mm - MIN_COLUMN_WIDTH_MM;
        let change = change.min(room);
        self.layout.columns[index].spacing_mm += change;
        self.layout.columns[index + 1].width_mm -= change;
    }
}

/// "4,23", "4.23" or "4.23 cm" in centimetres, as millimetres.
fn parse_cm(text: &str) -> Option<f32> {
    let number = text.trim().trim_end_matches("cm").trim().replace(',', ".");
    number.parse::<f32>().ok().filter(|value| value.is_finite()).map(|value| value * MM_PER_CM)
}

fn format_cm(mm: f32) -> String { format!("{:.2}", mm / MM_PER_CM) }

/// The preset tile under `x` in the presets panel.
fn preset_at(step: i32, x: i32) -> Option<ColumnPreset> {
    let index = usize::try_from(x.div_euclid(step)).ok()?;
    ColumnPreset::ALL.get(index).copied()
}

fn paint_presets(panel: &Panel, step: i32, form: &ColumnsForm) {
    let dc = AutoBufferedPaintDC::new(panel);
    let size = panel.get_client_size();
    let (width, height) = (size.width.max(1), size.height.max(1));
    let d = |v: i32| panel.from_dip_int(v);
    let Some((mut pixmap, canvas)) = crate::crisp::Canvas::new(panel, width as i32, height as i32) else { return; };
    pixmap.fill(tiny_skia::Color::WHITE);
    let selected = form.layout.matching_preset(form.text_width_mm);
    for (index, preset) in ColumnPreset::ALL.into_iter().enumerate() {
        let x = index as i32 * step + (step - d(TILE.0)) / 2;
        if Some(preset) == selected {
            let mut paint = tiny_skia::Paint::default();
            paint.set_color_rgba8(TILE_SELECTED.0, TILE_SELECTED.1, TILE_SELECTED.2, 255);
            if let Some(rect) = tiny_skia::Rect::from_xywh((x - d(3)) as f32, 0.0, (d(TILE.0) + 2 * d(3)) as f32, (d(TILE.1) + 2 * d(3)) as f32) {
                pixmap.fill_path(&tiny_skia::PathBuilder::from_rect(rect), &paint, tiny_skia::FillRule::Winding, crate::crisp::transform(), None);
            }
        }
        let mut layout = ColumnLayout::preset(preset, form.text_width_mm);
        layout.line_between = form.layout.line_between;
        column_art::page(&mut pixmap, (x as f32, d(3) as f32, d(TILE.0) as f32, d(TILE.1) as f32), &layout);
    }
    canvas.present(&dc, &pixmap, 0, 0);
    dc.set_background_mode(wxdragon::dc::BackgroundMode::Transparent);
    dc.set_text_foreground(Colour::rgb(36, 48, 66));
    if let Some(font) = panel.get_font() { dc.set_font(&font); }
    for (index, preset) in ColumnPreset::ALL.into_iter().enumerate() {
        let label = crate::i18n::text(preset.label());
        let (text_width, _) = dc.get_text_extent(&label);
        let x = index as i32 * step + (step - text_width) / 2;
        dc.draw_text(&label, x, d(TILE.1) + d(10));
    }
}

fn paint_preview(panel: &Panel, form: &ColumnsForm) {
    let dc = AutoBufferedPaintDC::new(panel);
    let size = panel.get_client_size();
    let (width, height) = (size.width.max(1), size.height.max(1));
    let Some((mut pixmap, canvas)) = crate::crisp::Canvas::new(panel, width as i32, height as i32) else { return; };
    pixmap.fill(tiny_skia::Color::from_rgba8(238, 242, 247, 255));
    let page_height = height as f32 * PREVIEW_PAGE_SHARE;
    let page_width = (page_height * 0.72).min(width as f32 * PREVIEW_PAGE_SHARE);
    let x = (width as f32 - page_width) / 2.0;
    let y = (height as f32 - page_height) / 2.0;
    column_art::page(&mut pixmap, (x, y, page_width, page_height), &form.layout);
    canvas.present(&dc, &pixmap, 0, 0);
}

/// Opens the dialog and applies the chosen layout.
pub(super) fn show(ribbon: &Ribbon) -> Result<(), String> {
    let (layout, text_width_mm) = {
        let state = ribbon.state.borrow();
        let properties = state.editor.section_properties_at_caret().ok_or("Place the caret in the document first")?;
        (ColumnLayout::of(&properties), properties.text_width_mm())
    };
    let ui = Rc::new(crate::i18n::localized(ColumnsDialogUi::new(Some(&ribbon.ui.main_frame), false)));
    let panel = ui.col_presets;
    let mut step = panel.from_dip_int(TILE_STEP);
    let mut caption_height = 0;
    for preset in ColumnPreset::ALL {
        let size = panel.get_text_extent(&crate::i18n::text(preset.label()));
        step = step.max(size.width + panel.from_dip_int(8));
        caption_height = caption_height.max(size.height);
    }
    panel.set_min_size(Size::new(step * ColumnPreset::ALL.len() as i32,
        panel.from_dip_int(TILE.1) + panel.from_dip_int(10) + caption_height + panel.from_dip_int(4)));
    let chrome = Chrome::new(ui.col_dialog, ui.col_card, &[], &[], (ui.col_ok, "OK"), Some(ui.col_cancel), &[], &[]);
    let form = Rc::new(RefCell::new(ColumnsForm::new(layout, text_width_mm)));
    let labels = [ui.col_label_1, ui.col_label_2, ui.col_label_3, ui.col_label_4, ui.col_label_5, ui.col_label_6,
        ui.col_label_7, ui.col_label_8, ui.col_label_9, ui.col_label_10, ui.col_label_11, ui.col_label_12];
    let widths = [ui.col_width_1, ui.col_width_2, ui.col_width_3, ui.col_width_4, ui.col_width_5, ui.col_width_6,
        ui.col_width_7, ui.col_width_8, ui.col_width_9, ui.col_width_10, ui.col_width_11, ui.col_width_12];
    let spacings = [ui.col_spacing_1, ui.col_spacing_2, ui.col_spacing_3, ui.col_spacing_4, ui.col_spacing_5, ui.col_spacing_6,
        ui.col_spacing_7, ui.col_spacing_8, ui.col_spacing_9, ui.col_spacing_10, ui.col_spacing_11, ui.col_spacing_12];

    // Show the form: one table row per column, editable as Word allows.
    let refresh: Rc<dyn Fn()> = { let ui = Rc::clone(&ui); let form = Rc::clone(&form);
        Rc::new(move || {
            let form = form.borrow();
            let columns = form.columns();
            let count = usize::from(form.layout.count);
            for index in 0..labels.len() {
                let shown = index < count;
                labels[index].show(shown);
                widths[index].show(shown);
                spacings[index].show(index + 1 < count);
                if !shown { continue; }
                widths[index].change_value(&format_cm(columns[index].width_mm));
                spacings[index].change_value(&if index + 1 < count { format_cm(columns[index].spacing_mm) } else { String::new() });
                // Equal columns: only the first row's values can be typed.
                widths[index].enable(count > 1 && (index == 0 || !form.equal()));
                spacings[index].enable(index + 1 < count && (index == 0 || !form.equal()));
            }
            ui.col_count.set_value(f64::from(form.layout.count));
            ui.col_equal.set_value(form.equal() || count < 2);
            ui.col_equal.enable(count > 1);
            ui.col_line.set_value(form.layout.line_between);
            ui.col_line.enable(count > 1);
            // The viewport stays three rows tall as the virtual table grows or shrinks.
            unsafe { wxdragon::ffi::wxd_Window_FitInside(ui.col_table.handle_ptr().cast()) };
            ui.col_presets.refresh(false, None);
            ui.col_preview.refresh(false, None);
            let message = form.layout.validate(form.text_width_mm).err().unwrap_or_default();
            ui.col_error.set_label(&crate::i18n::text(&(&message)));
            ui.col_ok.enable(message.is_empty());
        }) };

    for panel in [ui.col_presets, ui.col_preview] { panel.set_background_style(BackgroundStyle::Paint); }
    { let form = Rc::clone(&form); let panel = ui.col_presets; panel.on_paint(move |_| paint_presets(&panel, step, &form.borrow())); }
    { let form = Rc::clone(&form); let panel = ui.col_preview; panel.on_paint(move |_| paint_preview(&panel, &form.borrow())); }
    { let form = Rc::clone(&form); let refresh = Rc::clone(&refresh); let panel = ui.col_presets;
      panel.on_mouse_left_up(move |event| {
        let WindowEventData::MouseButton(m) = &event else { return; };
        let Some(preset) = m.event.get_position().and_then(|p| preset_at(step, p.x)) else { return; };
        form.borrow_mut().set_preset(preset); refresh();
      }); }
    { let form = Rc::clone(&form); let refresh = Rc::clone(&refresh); let ui2 = Rc::clone(&ui);
      ui.col_count.on_value_changed(move |_| { form.borrow_mut().set_count(ui2.col_count.get_value().round() as u16); refresh(); }); }
    { let form = Rc::clone(&form); let refresh = Rc::clone(&refresh); let ui2 = Rc::clone(&ui);
      ui.col_equal.on_toggled(move |_| { form.borrow_mut().set_equal(ui2.col_equal.get_value()); refresh(); }); }
    { let form = Rc::clone(&form); let refresh = Rc::clone(&refresh); let ui2 = Rc::clone(&ui);
      ui.col_line.on_toggled(move |_| { form.borrow_mut().layout.line_between = ui2.col_line.get_value(); refresh(); }); }
    // A typed value is committed when its field loses focus.
    for (index, (width, spacing)) in widths.iter().zip(spacings.iter()).enumerate() {
        { let form = Rc::clone(&form); let refresh = Rc::clone(&refresh); let field = *width;
          width.on_kill_focus(move |event| {
            if let Some(mm) = parse_cm(&field.get_value()) { form.borrow_mut().set_width(index, mm); }
            refresh(); event.skip(true);
          }); }
        { let form = Rc::clone(&form); let refresh = Rc::clone(&refresh); let field = *spacing;
          spacing.on_kill_focus(move |event| {
            if let Some(mm) = parse_cm(&field.get_value()) { form.borrow_mut().set_spacing(index, mm); }
            refresh(); event.skip(true);
          }); }
    }
    // "Start new column" only means something for "This point forward".
    let sync_apply = { let ui = Rc::clone(&ui); move || {
        let forward = ui.col_apply.get_selection() == Some(2);
        ui.col_new_column.enable(forward);
        if !forward { ui.col_new_column.set_value(false); }
    } };
    ui.col_apply.set_selection(1);
    sync_apply();
    { let sync_apply = sync_apply.clone(); ui.col_apply.on_selection_changed(move |_| sync_apply()); }
    refresh();

    let accepted = chrome.show();
    let scope = match ui.col_apply.get_selection() {
        Some(0) => ColumnScope::WholeDocument,
        Some(2) => ColumnScope::ThisPointForward { new_column: ui.col_new_column.get_value() },
        _ => ColumnScope::ThisSection,
    };
    let layout = form.borrow().layout.clone();
    ui.col_dialog.destroy();
    if !accepted { return Ok(()); }
    ribbon.edit(|editor| editor.apply_column_layout(&layout, scope))
}

