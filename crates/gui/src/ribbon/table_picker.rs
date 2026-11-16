//! Insert > Table: Word's size grid. Moving over the grid highlights the
//! rows and columns the new table will have; a click inserts it and opens the
//! Table tools. Arrow keys and Enter work too; Escape closes it.
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

const COLUMNS: usize = 10;
const ROWS: usize = 8;
const CELL: i32 = 18;
const GAP: i32 = 3;
const PAD: i32 = 10;
const HEADER: i32 = 30;

fn size_at(surface: Panel, p: Point) -> Option<(usize, usize)> {
    let d = |v: i32| surface.from_dip_int(v);
    let (x, y) = (p.x - d(PAD), p.y - d(HEADER));
    if x < 0 || y < 0 { return None; }
    let (column, row) = ((x / d(CELL + GAP)) as usize, (y / d(CELL + GAP)) as usize);
    (column < COLUMNS && row < ROWS).then_some((row + 1, column + 1))
}

fn paint(surface: Panel, chosen: Option<(usize, usize)>) {
    let dc = AutoBufferedPaintDC::new(&surface);
    let size = surface.get_client_size();
    let (w, h) = (size.width.max(1), size.height.max(1));
    let d = |v: i32| surface.from_dip_int(v) as f32;
    let Some((mut pix, canvas)) = crate::crisp::Canvas::new(&surface, w as i32, h as i32) else { return; };
    pix.fill(tiny_skia::Color::WHITE);
    // The header band, shaded as every dropdown's is (dropdown::BAND).
    square_band(&mut pix, w as f32, d(HEADER) - d(GAP) - d(2));
    let (rows, columns) = chosen.unwrap_or((0, 0));
    for row in 0..ROWS {
        for column in 0..COLUMNS {
            let on = row < rows && column < columns;
            let (x, y) = (d(PAD) + column as f32 * d(CELL + GAP), d(HEADER) + row as f32 * d(CELL + GAP));
            let (fill, line) = if on { ((226, 236, 253), (43, 111, 232)) } else { ((255, 255, 255), (196, 205, 218)) };
            square(&mut pix, x, y, d(CELL), line);
            square(&mut pix, x + d(1), y + d(1), d(CELL) - 2.0 * d(1), fill);
        }
    }
    canvas.present(&dc, &pix, 0, 0);
    dc.set_background_mode(wxdragon::dc::BackgroundMode::Transparent);
    if let Some(font) = Font::new_with_details(9, FontFamily::Default as i32, FontStyle::Normal as i32, FontWeight::Bold as i32, false, "Segoe UI") { dc.set_font(&font); }
    dc.set_text_foreground(Colour::rgb(36, 48, 66));
    let caption = match chosen { Some((r, c)) => format!("{c} × {r} Table"), None => "Insert Table".into() };
    let (_, th) = dc.get_text_extent(&crate::i18n::text(&(&caption)));
    dc.draw_text(&crate::i18n::text(&(&caption)), d(PAD) as i32, (d(HEADER) as i32 - th) / 2);
}

fn square_band(pix: &mut Pixmap, width: f32, height: f32) {
    let mut paint = Paint::default();
    paint.set_color_rgba8(dropdown::BAND.0, dropdown::BAND.1, dropdown::BAND.2, 255);
    if let Some(r) = tiny_skia::Rect::from_xywh(0.0, 0.0, width, height) {
        pix.fill_path(&PathBuilder::from_rect(r), &paint, FillRule::Winding, crate::crisp::transform(), None);
    }
}

fn square(pix: &mut Pixmap, x: f32, y: f32, side: f32, rgb: (u8, u8, u8)) {
    let mut paint = Paint::default();
    paint.set_color_rgba8(rgb.0, rgb.1, rgb.2, 255);
    if let Some(r) = tiny_skia::Rect::from_xywh(x, y, side.max(1.0), side.max(1.0)) {
        pix.fill_path(&PathBuilder::from_rect(r), &paint, FillRule::Winding, crate::crisp::transform(), None);
    }
}

fn insert(ribbon: &Ribbon, (rows, columns): (usize, usize)) {
    match ribbon.edit(|editor| editor.insert_table(rows, columns)) {
        Ok(()) => ribbon.open_table_tools(),
        Err(error) => ribbon.state.borrow_mut().action_status = Some(error),
    }
}

pub(super) fn show(ribbon: &Ribbon) {
    let width = 2 * PAD + COLUMNS as i32 * CELL + (COLUMNS as i32 - 1) * GAP;
    let height = HEADER + ROWS as i32 * (CELL + GAP) - GAP + PAD;
    let scaled = ribbon.ui.main_frame.from_dip(Size::new(width, height));
    let popup = dropdown::create(&ribbon.ui.main_frame, "Insert Table", scaled);
    let surface = Panel::builder(&popup).with_pos(dropdown::origin(&popup)).with_size(scaled).build();
    surface.set_background_style(BackgroundStyle::Paint);
    let chosen = Rc::new(Cell::new(Some((3, 3))));
    { let chosen = Rc::clone(&chosen); surface.on_paint(move |_| paint(surface, chosen.get())); }
    { let chosen = Rc::clone(&chosen); surface.on_mouse_motion(move |event| {
        let WindowEventData::MouseMotion(m) = &event else { return; };
        let next = m.event.get_position().and_then(|p| size_at(surface, p));
        if next.is_some() && chosen.replace(next) != next { surface.refresh(false, None); }
    }); }
    { let chosen = Rc::clone(&chosen); let weak = ribbon.owner.clone(); surface.on_mouse_left_up(move |event| {
        let WindowEventData::MouseButton(m) = &event else { return; };
        let Some(size) = m.event.get_position().and_then(|p| size_at(surface, p)).or(chosen.get()) else { return; };
        dropdown::dismiss(popup);
        if let Some(ribbon) = weak.upgrade() { insert(&ribbon, size); }
    }); }
    { let chosen = Rc::clone(&chosen); let weak = ribbon.owner.clone();
      dropdown::bind_keys(popup, ribbon.ui.paper_canvas, move |key| {
        use wxdragon::keycode::*;
        let (rows, columns) = chosen.get().unwrap_or((1, 1));
        let next = match key {
            WXK_RETURN | WXK_NUMPAD_ENTER => {
                dropdown::dismiss(popup);
                if let Some(ribbon) = weak.upgrade() { insert(&ribbon, (rows, columns)); }
                return true;
            }
            WXK_LEFT => (rows, columns.saturating_sub(1).max(1)),
            WXK_RIGHT => (rows, (columns + 1).min(COLUMNS)),
            WXK_UP => (rows.saturating_sub(1).max(1), columns),
            WXK_DOWN => ((rows + 1).min(ROWS), columns),
            _ => return false,
        };
        chosen.set(Some(next)); surface.refresh(false, None);
        true
    }); }
    dropdown::show(popup, &ribbon.ui.insert_table_button, &surface);
}
