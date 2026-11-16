//! Insert > Symbol: a character grid below the button, as the Windows
//! Character Map lays one out. Categories along the top, the characters in a
//! grid that scrolls with the wheel, and the character under the pointer (or
//! chosen with the arrow keys) shown large with its Unicode name and code
//! point. A click or Enter inserts it; Escape closes the grid.
use super::symbol_table::{Category, CATEGORIES};
use super::*;
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap};
use wxdragon::{
    dc::AutoBufferedPaintDC,
    event::window_events::WindowEvents,
    font::{Font, FontFamily, FontStyle, FontWeight},
    geometry::{Point, Size},
    widgets::panel::Panel,
    window::BackgroundStyle,
    DeviceContext,
};

const COLUMNS: usize = 16;
const ROWS: usize = 8;
const CELL: i32 = 30;
const PAD: i32 = 10;
const CHIPS: i32 = 26;
const GRID_TOP: i32 = PAD + CHIPS + 8;
const BAR: i32 = 8;
const FOOTER: i32 = 46;
const WIDTH: i32 = 2 * PAD + COLUMNS as i32 * CELL + BAR;
const HEIGHT: i32 = GRID_TOP + ROWS as i32 * CELL + 8 + FOOTER;

const INK: (u8, u8, u8) = (36, 48, 66);
const MUTED: (u8, u8, u8) = (107, 119, 138);
const ACCENT: (u8, u8, u8) = (43, 111, 232);
const CHOSEN: (u8, u8, u8) = (226, 236, 253);
const HOVER: (u8, u8, u8) = (234, 241, 252);
const LINE: (u8, u8, u8) = (226, 231, 238);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Hit { Chip(usize), Cell(usize) }

#[derive(Default)]
struct Picker {
    category: usize,
    /// First grid row shown.
    first_row: usize,
    /// The character the keyboard is on, and the one under the pointer.
    selected: usize,
    hover: Option<Hit>,
    /// Each chip's left and right edge, as last painted (logical pixels).
    chips: Vec<(i32, i32)>,
}

impl Picker {
    fn symbols(&self) -> &'static [(char, &'static str)] { CATEGORIES[self.category].symbols }
    fn rows(&self) -> usize { self.symbols().len().div_ceil(COLUMNS) }
    fn max_first_row(&self) -> usize { self.rows().saturating_sub(ROWS) }
    fn scroll(&mut self, rows: isize) { self.first_row = self.first_row.saturating_add_signed(rows).min(self.max_first_row()); }
    /// Scrolls just enough to show the selected character.
    fn reveal(&mut self) {
        let row = self.selected / COLUMNS;
        if row < self.first_row { self.first_row = row; }
        if row >= self.first_row + ROWS { self.first_row = row + 1 - ROWS; }
    }
    /// The character to show in the footer: the hovered one, else the selected.
    fn shown(&self) -> Option<(char, &'static str)> {
        let index = match self.hover { Some(Hit::Cell(index)) => index, _ => self.selected };
        self.symbols().get(index).copied()
    }
}

fn hit(surface: Panel, picker: &Picker, p: Point) -> Option<Hit> {
    let d = |v: i32| surface.from_dip_int(v);
    if p.y >= d(PAD) && p.y < d(PAD + CHIPS) {
        return picker.chips.iter().position(|&(left, right)| p.x >= left && p.x < right).map(Hit::Chip);
    }
    let (x, y) = (p.x - d(PAD), p.y - d(GRID_TOP));
    if x < 0 || y < 0 { return None; }
    let (column, row) = ((x / d(CELL)) as usize, (y / d(CELL)) as usize);
    if column >= COLUMNS || row >= ROWS { return None; }
    let index = (picker.first_row + row) * COLUMNS + column;
    (index < picker.symbols().len()).then_some(Hit::Cell(index))
}

fn font(points: i32, weight: FontWeight) -> Option<Font> {
    Font::new_with_details(points, FontFamily::Default as i32, FontStyle::Normal as i32, weight as i32, false, "")
}

fn paint(surface: Panel, picker: &mut Picker) {
    let dc = AutoBufferedPaintDC::new(&surface);
    let size = surface.get_client_size();
    let (w, h) = (size.width.max(1), size.height.max(1));
    let d = |v: i32| surface.from_dip_int(v);
    let f = |v: i32| d(v) as f32;
    let Some((mut pix, canvas)) = crate::crisp::Canvas::new(&surface, w, h) else { return; };
    pix.fill(tiny_skia::Color::WHITE);
    dc.set_background_mode(wxdragon::dc::BackgroundMode::Transparent);
    // Header and footer bands, shaded as every dropdown's are (dropdown::BAND).
    let footer_top = f(GRID_TOP + ROWS as i32 * CELL + 8);
    fill(&mut pix, (0.0, 0.0, w as f32, f(GRID_TOP - 4)), dropdown::BAND);
    fill(&mut pix, (0.0, footer_top, w as f32, h as f32 - footer_top), dropdown::BAND);

    // Category chips: measured with their font, so they are laid out first.
    let chip_font = font(9, FontWeight::Normal);
    if let Some(font) = &chip_font { dc.set_font(font); }
    let mut x = d(PAD);
    picker.chips.clear();
    let labels: Vec<(i32, &str)> = CATEGORIES.iter().map(|Category { name, .. }| {
        let (tw, _) = dc.get_text_extent(&crate::i18n::text(&(name)));
        let width = tw + d(16);
        picker.chips.push((x, x + width));
        let at = x; x += width + d(4);
        (at, *name)
    }).collect();
    for (index, &(left, right)) in picker.chips.iter().enumerate() {
        let rect = (left as f32, f(PAD), (right - left) as f32, f(CHIPS));
        if index == picker.category { fill(&mut pix, rect, CHOSEN); }
        else if picker.hover == Some(Hit::Chip(index)) { fill(&mut pix, rect, (255, 255, 255)); }
    }

    // The grid: the hovered and the selected cells, then the cell lines.
    let symbols = picker.symbols();
    let visible = |index: usize| index / COLUMNS >= picker.first_row && index / COLUMNS < picker.first_row + ROWS;
    let cell_rect = |index: usize| {
        let (row, column) = (index / COLUMNS - picker.first_row, index % COLUMNS);
        (f(PAD) + column as f32 * f(CELL), f(GRID_TOP) + row as f32 * f(CELL), f(CELL), f(CELL))
    };
    if let Some(Hit::Cell(index)) = picker.hover { if visible(index) { fill(&mut pix, cell_rect(index), HOVER); } }
    if visible(picker.selected) && picker.selected < symbols.len() { fill(&mut pix, cell_rect(picker.selected), CHOSEN); }
    let shown_rows = (symbols.len().saturating_sub(picker.first_row * COLUMNS)).div_ceil(COLUMNS).min(ROWS);
    let (grid_w, grid_h) = (COLUMNS as f32 * f(CELL), shown_rows as f32 * f(CELL));
    let px = 1.0_f32;
    for column in 0..=COLUMNS { fill(&mut pix, (f(PAD) + column as f32 * f(CELL), f(GRID_TOP), px, grid_h + px), LINE); }
    for row in 0..=shown_rows { fill(&mut pix, (f(PAD), f(GRID_TOP) + row as f32 * f(CELL), grid_w + px, px), LINE); }
    if visible(picker.selected) && picker.selected < symbols.len() {
        let (x, y, cw, ch) = cell_rect(picker.selected);
        for side in [(x, y, cw + px, px), (x, y + ch, cw + px, px), (x, y, px, ch + px), (x + cw, y, px, ch + px)] { fill(&mut pix, side, ACCENT); }
    }
    // Where the shown rows are in the category.
    let rows = picker.rows();
    if rows > ROWS {
        let track = (f(PAD) + grid_w + f(3), f(GRID_TOP), f(4), f(ROWS as i32 * CELL));
        fill(&mut pix, track, (240, 243, 247));
        let thumb_h = track.3 * ROWS as f32 / rows as f32;
        let thumb_y = track.1 + (track.3 - thumb_h) * picker.first_row as f32 / picker.max_first_row().max(1) as f32;
        fill(&mut pix, (track.0, thumb_y, track.2, thumb_h), (190, 199, 212));
    }

    // Footer: the character large, its name and code point.
    let preview = (f(PAD), footer_top + f(6), f(FOOTER - 10), f(FOOTER - 10));
    fill(&mut pix, preview, (255, 255, 255));
    for side in [(preview.0, preview.1, preview.2, px), (preview.0, preview.1 + preview.3, preview.2 + px, px),
        (preview.0, preview.1, px, preview.3), (preview.0 + preview.2, preview.1, px, preview.3)] { fill(&mut pix, side, LINE); }
    canvas.present(&dc, &pix, 0, 0);

    for (index, &(left, name)) in labels.iter().enumerate() {
        let ink = if index == picker.category { ACCENT } else { INK };
        dc.set_text_foreground(Colour::rgb(ink.0, ink.1, ink.2));
        let (_, th) = dc.get_text_extent(&crate::i18n::text(&(name)));
        dc.draw_text(&crate::i18n::text(&(name)), left + d(8), d(PAD) + (d(CHIPS) - th) / 2);
    }
    if let Some(font) = font(14, FontWeight::Normal) { dc.set_font(&font); }
    dc.set_text_foreground(Colour::rgb(INK.0, INK.1, INK.2));
    let mut text = [0u8; 4];
    for (index, (symbol, _)) in symbols.iter().enumerate().skip(picker.first_row * COLUMNS).take(ROWS * COLUMNS) {
        let label = symbol.encode_utf8(&mut text);
        let (x, y, cw, ch) = cell_rect(index);
        let (tw, th) = dc.get_text_extent(&crate::i18n::text(&(label)));
        dc.draw_text(&crate::i18n::text(&(label)), (x + (cw - tw as f32) * 0.5).round() as i32, (y + (ch - th as f32) * 0.5).round() as i32);
    }
    if let Some((symbol, name)) = picker.shown() {
        if let Some(font) = font(20, FontWeight::Normal) { dc.set_font(&font); }
        let label = symbol.encode_utf8(&mut text);
        let (tw, th) = dc.get_text_extent(&crate::i18n::text(&(label)));
        dc.draw_text(&crate::i18n::text(&(label)), (preview.0 + (preview.2 - tw as f32) * 0.5).round() as i32, (preview.1 + (preview.3 - th as f32) * 0.5).round() as i32);
        let left = (preview.0 + preview.2) as i32 + d(12);
        if let Some(font) = font(9, FontWeight::Bold) { dc.set_font(&font); }
        dc.draw_text(&crate::i18n::text(&(name)), left, preview.1 as i32 + d(2));
        if let Some(font) = &chip_font { dc.set_font(font); }
        dc.set_text_foreground(Colour::rgb(MUTED.0, MUTED.1, MUTED.2));
        dc.draw_text(&crate::i18n::text(&(&format!("U+{:04X}", u32::from(symbol)))), left, preview.1 as i32 + d(19));
    }
}

fn fill(pix: &mut Pixmap, (x, y, w, h): (f32, f32, f32, f32), rgb: (u8, u8, u8)) {
    let mut paint = Paint::default();
    paint.set_color_rgba8(rgb.0, rgb.1, rgb.2, 255);
    if let Some(r) = tiny_skia::Rect::from_xywh(x, y, w.max(1.0), h.max(1.0)) {
        pix.fill_path(&PathBuilder::from_rect(r), &paint, FillRule::Winding, crate::crisp::transform(), None);
    }
}

fn insert(ribbon: &Ribbon, symbol: char) {
    let mut text = [0u8; 4];
    if let Err(error) = ribbon.edit(|editor| editor.insert_text(symbol.encode_utf8(&mut text))) {
        ribbon.state.borrow_mut().action_status = Some(error);
    }
    ribbon.focus_document();
}

pub(super) fn show(ribbon: &Ribbon) {
    let scaled = ribbon.ui.main_frame.from_dip(Size::new(WIDTH, HEIGHT));
    let popup = dropdown::create(&ribbon.ui.main_frame, "Symbol", scaled);
    let surface = Panel::builder(&popup).with_pos(dropdown::origin(&popup)).with_size(scaled).build();
    surface.set_background_style(BackgroundStyle::Paint);
    let picker = Rc::new(RefCell::new(Picker::default()));
    { let picker = Rc::clone(&picker); surface.on_paint(move |_| paint(surface, &mut picker.borrow_mut())); }
    { let picker = Rc::clone(&picker); surface.on_mouse_motion(move |event| {
        let WindowEventData::MouseMotion(m) = &event else { return; };
        let next = m.event.get_position().and_then(|p| hit(surface, &picker.borrow(), p));
        let changed = { let mut picker = picker.borrow_mut(); std::mem::replace(&mut picker.hover, next) != next };
        if changed { surface.refresh(false, None); }
    }); }
    { let picker = Rc::clone(&picker); surface.on_mouse_leave(move |_| {
        if picker.borrow_mut().hover.take().is_some() { surface.refresh(false, None); }
    }); }
    { let picker = Rc::clone(&picker); surface.on_mouse_wheel(move |event| {
        let rotation = match &event {
            WindowEventData::MouseButton(m) => m.event.get_wheel_rotation(),
            WindowEventData::MouseMotion(m) => m.event.get_wheel_rotation(),
            _ => 0,
        };
        if rotation != 0 { picker.borrow_mut().scroll(if rotation > 0 { -3 } else { 3 }); surface.refresh(false, None); }
        event.skip(false);
    }); }
    { let picker = Rc::clone(&picker); let weak = ribbon.owner.clone(); surface.on_mouse_left_up(move |event| {
        let WindowEventData::MouseButton(m) = &event else { return; };
        let at = m.event.get_position().and_then(|p| hit(surface, &picker.borrow(), p));
        match at {
            Some(Hit::Chip(category)) => {
                let mut p = picker.borrow_mut();
                let chips = std::mem::take(&mut p.chips);
                *p = Picker { category, chips, ..Picker::default() };
                drop(p);
                surface.refresh(false, None);
            }
            Some(Hit::Cell(index)) => {
                let symbol = picker.borrow().symbols()[index].0;
                dropdown::dismiss(popup);
                if let Some(ribbon) = weak.upgrade() { insert(&ribbon, symbol); }
            }
            None => {}
        }
    }); }
    { let picker = Rc::clone(&picker); let weak = ribbon.owner.clone();
      dropdown::bind_keys(popup, ribbon.ui.paper_canvas, move |key| {
        use wxdragon::keycode::*;
        let mut p = picker.borrow_mut();
        let last = p.symbols().len().saturating_sub(1);
        let page = ROWS * COLUMNS;
        p.selected = match key {
            WXK_RETURN | WXK_NUMPAD_ENTER => {
                let symbol = p.symbols().get(p.selected).map(|s| s.0);
                drop(p);
                dropdown::dismiss(popup);
                if let (Some(ribbon), Some(symbol)) = (weak.upgrade(), symbol) { insert(&ribbon, symbol); }
                return true;
            }
            WXK_LEFT => p.selected.saturating_sub(1),
            WXK_RIGHT => (p.selected + 1).min(last),
            WXK_UP => p.selected.checked_sub(COLUMNS).unwrap_or(p.selected),
            WXK_DOWN => if p.selected + COLUMNS <= last { p.selected + COLUMNS } else { p.selected },
            WXK_PAGEUP => p.selected.saturating_sub(page),
            WXK_PAGEDOWN => (p.selected + page).min(last),
            WXK_HOME => 0,
            WXK_END => last,
            _ => return false,
        };
        p.hover = None;
        p.reveal();
        drop(p);
        surface.refresh(false, None);
        true
    }); }
    dropdown::show(popup, &ribbon.ui.symbol_button, &surface);
}
