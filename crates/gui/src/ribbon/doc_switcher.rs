//! The document name in the title bar is the document switcher: the name of
//! the document shown, a dot when it has unsaved changes and a small arrow.
//! Clicking it lists the documents open in this window in the order they
//! were opened; a row
//! shows its document, × closes it.
use super::*;
use super::bar_paint::position;
use crate::documents::{self, Row};
use wxdragon::{
    dc::AutoBufferedPaintDC,
    event::window_events::WindowEvents,
    font::{Font, FontFamily, FontStyle, FontWeight},
    geometry::{Point, Rect, Size},
    widgets::panel::Panel,
    window::BackgroundStyle,
    DeviceContext,
};

/// Sizes in device-independent pixels.
const HEIGHT: i32 = 28;
/// Space kept clear beside the quick-access buttons and the buttons at the right.
const GAP: i32 = 16;
/// Inner padding, around the name and after the arrow.
const PAD: i32 = 8;
const MENU_WIDTH: i32 = 360;
const MENU_PAD: i32 = 6;
const HEADER_HEIGHT: i32 = 24;
const ROW_HEIGHT: i32 = 44;
const CLOSE_SIZE: i32 = 22;

const INK: (u8, u8, u8) = (31, 42, 58);
const MUTED: (u8, u8, u8) = (104, 117, 138);
const ACCENT: (u8, u8, u8) = (47, 98, 201);

fn rgb(c: (u8, u8, u8)) -> Colour { Colour::rgb(c.0, c.1, c.2) }

fn font(points: i32, bold: bool) -> Option<Font> {
    Font::new_with_details(points, FontFamily::Default as i32, FontStyle::Normal as i32,
        if bold { FontWeight::Bold } else { FontWeight::Normal } as i32, false, "Segoe UI")
}

/// Cuts `text` with an ellipsis to fit `width` pixels.
fn fitted(dc: &impl DeviceContext, text: &str, width: i32) -> String {
    if dc.get_text_extent(text).0 <= width { return text.to_string(); }
    let mut chars: Vec<char> = text.chars().collect();
    while !chars.is_empty() && dc.get_text_extent(&format!("{}…", chars.iter().collect::<String>())).0 > width { chars.pop(); }
    format!("{}…", chars.into_iter().collect::<String>())
}

/// A small page with two text lines, the document icon.
fn draw_page_icon(dc: &impl DeviceContext, x: i32, y: i32, size: i32, colour: Colour, px: i32) {
    let (w, h) = (size * 10 / 14, size);
    let fold = size * 4 / 14;
    dc.set_pen(colour, px.max(1), PenStyle::Solid);
    dc.set_brush(Colour::rgb(255, 255, 255), BrushStyle::Solid);
    let p = wxdragon::dc::Point::new;
    let points = [p(x, y), p(x + w - fold, y), p(x + w, y + fold), p(x + w, y + h), p(x, y + h)];
    dc.draw_polygon(&points, 0, 0, wxdragon::dc::PolygonFillMode::OddEven);
    for row in [h * 7 / 14, h * 10 / 14] { dc.draw_line(x + w / 4, y + row, x + w * 3 / 4, y + row); }
}

pub(super) struct DocSwitcher {
    ribbon: std::rc::Weak<Ribbon>,
    panel: Panel,
    /// What is painted now: the rows and the name's left edge and width.
    shown: RefCell<Option<(Vec<Row>, i32, i32)>>,
    /// The inputs the switcher was last placed from, so an unchanged title
    /// bar costs a comparison per tick instead of measuring text with Pango.
    inputs: RefCell<Option<(Vec<Row>, i32, i32, i32)>>,
    /// The active document's name and its measured width.
    measured: RefCell<Option<(String, i32)>>,
    hover: Cell<bool>,
}

pub(super) fn install(ribbon: &Rc<Ribbon>) -> Rc<DocSwitcher> {
    // The switcher takes the title label's place.
    ribbon.ui.document_name_label.hide();
    let strip = ribbon.ui.title_strip;
    let bg = super::title_bar();
    strip.set_background_color(Colour::rgb(bg[0], bg[1], bg[2]));
    strip.refresh(true, None);
    let panel = Panel::builder(&strip).with_size(strip.from_dip(Size::new(200, HEIGHT))).build();
    panel.set_background_style(BackgroundStyle::Paint);
    panel.on_erase_background(|_| {});
    if let Some(f) = strip.get_font() { panel.set_font(&f); }
    let switcher = Rc::new(DocSwitcher { ribbon: Rc::downgrade(ribbon), panel, shown: RefCell::new(None), inputs: RefCell::new(None), measured: RefCell::new(None), hover: Cell::new(false) });
    let s = Rc::downgrade(&switcher);
    panel.on_paint(move |_| { if let Some(s) = s.upgrade() { s.paint(); } });
    let s = Rc::downgrade(&switcher);
    panel.on_mouse_motion(move |_| { if let Some(s) = s.upgrade() { if !s.hover.replace(true) { s.panel.refresh(false, None); } } });
    let s = Rc::downgrade(&switcher);
    panel.on_mouse_leave(move |_| { if let Some(s) = s.upgrade() { if s.hover.replace(false) { s.panel.refresh(false, None); } } });
    let s = Rc::downgrade(&switcher);
    panel.on_mouse_left_down(move |_| { if let Some(s) = s.upgrade() { s.show_list(); } });
    let s = Rc::downgrade(&switcher);
    strip.on_size(move |event| { if let Some(s) = s.upgrade() { s.shown.replace(None); s.inputs.replace(None); s.measured.replace(None); s.refresh(); } event.skip(true); });
    switcher.refresh();
    switcher
}

impl DocSwitcher {
    /// Paints again, for new theme colours.
    pub(super) fn repaint(&self) { self.panel.refresh(false, None); }

    /// Follows the open documents; resizes and repaints only on change.
    pub(super) fn refresh(&self) {
        let Some(ribbon) = self.ribbon.upgrade() else { return; };
        let ui = &ribbon.ui;
        let strip = ui.title_strip;
        let rows = documents::rows(&ribbon.state.borrow());
        let dip = |v: i32| strip.from_dip_int(v);
        // Centred in the title bar, within the span between the quick-access
        // buttons and the buttons at the right.
        let redo = ui.redo_button;
        let low = redo.get_position().x + redo.get_size().width + dip(GAP);
        let high = ui.collapse_ribbon_button.get_position().x - dip(GAP);
        let strip_width = strip.get_client_size().width;
        // Runs every tick: nothing to do while the documents and title bar are as they were.
        let inputs = (rows.clone(), low, high, strip_width);
        if self.inputs.borrow().as_ref() == Some(&inputs) { return; }
        self.inputs.replace(Some(inputs));
        let name = rows.iter().find(|r| r.active).map(|r| r.name.clone()).unwrap_or_default();
        let dirty = rows.iter().any(|r| r.active && r.dirty);
        let cached = self.measured.borrow().as_ref().filter(|(n, _)| *n == name).map(|(_, w)| *w);
        let text = cached.unwrap_or_else(|| {
            let width = self.panel.get_text_extent(&name).width;
            self.measured.replace(Some((name.clone(), width)));
            width
        });
        let width = (dip(PAD) + text + if dirty { dip(15) } else { 0 } + dip(18) + dip(PAD)).min((high - low).max(dip(60)));
        let centre = strip_width / 2;
        let left = (centre - width / 2).clamp(low, (high - width).max(low));
        let key = (rows, left, width);
        if self.shown.borrow().as_ref() == Some(&key) { return; }
        let height = dip(HEIGHT);
        let top = (strip.get_client_size().height - height) / 2;
        self.panel.set_size(Size::new(width, height));
        self.panel.move_window(left, top.max(0));
        self.shown.replace(Some(key));
        self.panel.refresh(false, None);
    }

    fn paint(&self) {
        let dc = AutoBufferedPaintDC::new(&self.panel);
        let size = self.panel.get_client_size();
        let dip = |v: i32| self.panel.from_dip_int(v);
        let px = dip(1).max(1);
        let (bg, ink) = (super::title_bar(), super::title_ink());
        let colour = |c: [u8; 3]| Colour::rgb(c[0], c[1], c[2]);
        dc.set_background(colour(bg)); dc.clear();
        let shown = self.shown.borrow();
        let Some((rows, _, _)) = shown.as_ref() else { return; };
        let Some(active) = rows.iter().find(|r| r.active) else { return; };
        if self.hover.get() {
            let hover = colour(super::mix(bg, ink, 0.1));
            dc.set_pen(hover, px, PenStyle::Solid);
            dc.set_brush(hover, BrushStyle::Solid);
            dc.draw_rounded_rectangle(0, 0, size.width, size.height, dip(6) as f64);
        }
        let cy = size.height / 2;
        // From the right: the arrow, then the unsaved dot; the name gets the rest.
        let mut right = size.width - dip(PAD);
        dc.set_pen(colour(super::mix(ink, bg, 0.3)), (px * 3 / 2).max(1), PenStyle::Solid);
        dc.draw_line(right - dip(8), cy - dip(2), right - dip(4), cy + dip(2));
        dc.draw_line(right - dip(4), cy + dip(2), right, cy - dip(2));
        right -= dip(18);
        if active.dirty {
            let dot = if super::title_is_dark() { ink } else { super::accent() };
            dc.set_pen(colour(dot), px, PenStyle::Solid);
            dc.set_brush(colour(dot), BrushStyle::Solid);
            dc.draw_circle(right - dip(4), cy, dip(3).max(2));
            right -= dip(15);
        }
        if let Some(f) = self.panel.get_font() { dc.set_font(&f); }
        let name = fitted(&dc, &active.name, right - dip(PAD));
        let (_, nh) = dc.get_text_extent(&name);
        dc.set_text_foreground(colour(ink));
        dc.draw_text(&name, dip(PAD), cy - nh / 2);
    }

    /// The list of documents, centred below the name.
    pub(super) fn show_list(&self) {
        let Some(ribbon) = self.ribbon.upgrade() else { return; };
        let rows = Rc::new(documents::rows(&ribbon.state.borrow()));
        let frame = ribbon.ui.main_frame;
        let client = frame.from_dip(Size::new(MENU_WIDTH, 2 * MENU_PAD + HEADER_HEIGHT + rows.len() as i32 * ROW_HEIGHT));
        let popup = dropdown::create(&frame, "Documents", client);
        let surface = Panel::builder(&popup).with_pos(dropdown::origin(&popup)).with_size(client).build();
        surface.set_background_style(BackgroundStyle::Paint);
        surface.on_erase_background(|_| {});
        let selected = Rc::new(Cell::new(rows.iter().position(|r| r.active).unwrap_or(0)));
        let hover_close = Rc::new(Cell::new(false));
        {
            let (rows, selected, hover_close) = (Rc::clone(&rows), Rc::clone(&selected), Rc::clone(&hover_close));
            surface.on_paint(move |_| paint_list(surface, &rows, selected.get(), hover_close.get()));
        }
        {
            let (rows, selected, hover_close) = (Rc::clone(&rows), Rc::clone(&selected), Rc::clone(&hover_close));
            surface.on_mouse_motion(move |event| {
                let Some((x, y)) = position(&event) else { return; };
                if let Some((index, on_close)) = list_hit(surface, rows.len(), x as i32, y as i32) {
                    if selected.replace(index) != index || hover_close.replace(on_close) != on_close { surface.refresh(false, None); }
                }
            });
        }
        {
            let rows = Rc::clone(&rows);
            let weak = ribbon.owner.clone();
            surface.on_mouse_left_down(move |event| {
                let Some((x, y)) = position(&event) else { return; };
                let Some((index, on_close)) = list_hit(surface, rows.len(), x as i32, y as i32) else { return; };
                dropdown::dismiss(popup);
                let Some(ribbon) = weak.upgrade() else { return; };
                let id = rows[index].id;
                let result = if on_close && rows.len() > 1 { ribbon.close_document(id) }
                    else { documents::switch_to(&mut ribbon.state.borrow_mut(), id) };
                if let Err(error) = result { ribbon.state.borrow_mut().action_status = Some(error); }
                ribbon.focus_document();
            });
        }
        {
            let rows = Rc::clone(&rows);
            let selected = Rc::clone(&selected);
            let weak = ribbon.owner.clone();
            dropdown::bind_keys(popup, ribbon.ui.paper_canvas, move |key| {
                let count = rows.len();
                match key {
                    WXK_UP => { selected.set((selected.get() + count - 1) % count); surface.refresh(false, None); true }
                    WXK_DOWN => { selected.set((selected.get() + 1) % count); surface.refresh(false, None); true }
                    WXK_RETURN | WXK_NUMPAD_ENTER => {
                        dropdown::dismiss(popup);
                        if let Some(ribbon) = weak.upgrade() {
                            if let Err(error) = documents::switch_to(&mut ribbon.state.borrow_mut(), rows[selected.get()].id) {
                                ribbon.state.borrow_mut().action_status = Some(error);
                            }
                            ribbon.focus_document();
                        }
                        true
                    }
                    _ => false,
                }
            });
        }
        // Centred below the name; the dropdown keeps it on the screen.
        let origin = crate::gtk_native::to_screen(&self.panel, Point::new(0, 0));
        let bar = self.panel.get_size();
        let width = popup.get_size().width;
        let x = origin.x + bar.width / 2 - width / 2;
        dropdown::show_below_rect(popup, Rect::new(x, origin.y + frame.from_dip_int(3), width, bar.height), &surface);
    }
}

/// The row under a point, and whether it is over the row's × button.
fn list_hit(surface: Panel, count: usize, x: i32, y: i32) -> Option<(usize, bool)> {
    let dip = |v: i32| surface.from_dip_int(v);
    let top = dip(MENU_PAD + HEADER_HEIGHT);
    if y < top { return None; }
    let index = ((y - top) / dip(ROW_HEIGHT)) as usize;
    if index >= count { return None; }
    let width = surface.get_client_size().width;
    let on_close = count > 1 && x >= width - dip(MENU_PAD + 10 + CLOSE_SIZE) && x <= width - dip(MENU_PAD + 10);
    Some((index, on_close))
}

fn paint_list(surface: Panel, rows: &[Row], selected: usize, hover_close: bool) {
    let dc = AutoBufferedPaintDC::new(&surface);
    let dip = |v: i32| surface.from_dip_int(v);
    let px = dip(1).max(1);
    let size = surface.get_client_size();
    dc.set_background(Colour::rgb(255, 255, 255)); dc.clear();
    // The header band, shaded as every dropdown's is (dropdown::BAND).
    dc.set_pen(rgb(dropdown::BAND), 1, PenStyle::Solid);
    dc.set_brush(rgb(dropdown::BAND), BrushStyle::Solid);
    dc.draw_rectangle(0, 0, size.width, dip(MENU_PAD + HEADER_HEIGHT) - dip(2));
    if let Some(f) = font(8, false) { dc.set_font(&f); }
    dc.set_text_foreground(rgb(MUTED));
    let (_, hh) = dc.get_text_extent("Open");
    dc.draw_text(&crate::i18n::text("Open in this window"), dip(MENU_PAD + 10), dip(MENU_PAD) + (dip(HEADER_HEIGHT) - hh) / 2);
    for (index, row) in rows.iter().enumerate() {
        let y = dip(MENU_PAD + HEADER_HEIGHT) + index as i32 * dip(ROW_HEIGHT);
        let (x0, w) = (dip(MENU_PAD), size.width - 2 * dip(MENU_PAD));
        if row.active || index == selected {
            let fill = if row.active { Colour::rgb(231, 238, 252) } else { Colour::rgb(242, 245, 250) };
            dc.set_pen(fill, px, PenStyle::Solid);
            dc.set_brush(fill, BrushStyle::Solid);
            dc.draw_rounded_rectangle(x0, y, w, dip(ROW_HEIGHT), dip(6) as f64);
        }
        let icon = dip(16);
        draw_page_icon(&dc, x0 + dip(10), y + (dip(ROW_HEIGHT) - icon) / 2, icon, if row.active { rgb(ACCENT) } else { rgb((122, 132, 150)) }, px);
        let text_left = x0 + dip(10) + icon + dip(10);
        // Right side: the switch key, then the × of the row under the pointer.
        let mut right = x0 + w - dip(10);
        let can_close = rows.len() > 1;
        if can_close && index == selected {
            let (cx, cy, half) = (right - dip(CLOSE_SIZE) / 2, y + dip(ROW_HEIGHT) / 2, dip(4));
            if hover_close {
                dc.set_pen(Colour::rgb(222, 227, 236), px, PenStyle::Solid);
                dc.set_brush(Colour::rgb(222, 227, 236), BrushStyle::Solid);
                dc.draw_rounded_rectangle(cx - dip(CLOSE_SIZE) / 2, cy - dip(CLOSE_SIZE) / 2, dip(CLOSE_SIZE), dip(CLOSE_SIZE), dip(5) as f64);
            }
            dc.set_pen(rgb((74, 85, 104)), (px * 3 / 2).max(1), PenStyle::Solid);
            dc.draw_line(cx - half, cy - half, cx + half, cy + half);
            dc.draw_line(cx + half, cy - half, cx - half, cy + half);
        }
        if can_close { right -= dip(CLOSE_SIZE) + dip(6); }
        if let Some(f) = font(8, false) { dc.set_font(&f); }
        if index < 9 {
            let key = format!("Alt+{}", index + 1);
            let (kw, kh) = dc.get_text_extent(&key);
            dc.set_text_foreground(rgb(MUTED));
            dc.draw_text(&key, right - kw, y + (dip(ROW_HEIGHT) - kh) / 2);
            right -= kw + dip(10);
        }
        if row.dirty {
            dc.set_pen(rgb(ACCENT), px, PenStyle::Solid);
            dc.set_brush(rgb(ACCENT), BrushStyle::Solid);
            dc.draw_circle(right - dip(4), y + dip(ROW_HEIGHT) / 2, dip(3).max(2));
            right -= dip(8) + dip(8);
        }
        let avail = right - text_left;
        if let Some(f) = font(9, false) { dc.set_font(&f); }
        let name = fitted(&dc, &row.name, avail);
        let (_, nh) = dc.get_text_extent(&name);
        if let Some(f) = font(8, false) { dc.set_font(&f); }
        let folder = fitted(&dc, &row.folder, avail);
        let (_, fh) = dc.get_text_extent(&folder);
        let block = nh + fh;
        let top = y + (dip(ROW_HEIGHT) - block) / 2;
        dc.set_text_foreground(rgb(MUTED));
        dc.draw_text(&folder, text_left, top + nh);
        if let Some(f) = font(9, false) { dc.set_font(&f); }
        dc.set_text_foreground(rgb(INK));
        dc.draw_text(&name, text_left, top);
    }
}
