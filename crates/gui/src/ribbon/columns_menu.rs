//! Layout > Columns, as in Word: One, Two, Three, Left and Right, each with a
//! picture of the layout, then "More Columns…" for the Columns dialog. A
//! preset applies to the section at the caret; the current one is marked.
use super::*;
use editor::{ColumnLayout, ColumnPreset, ColumnScope};
use tiny_skia::Pixmap;
use wxdragon::{
    dc::AutoBufferedPaintDC,
    event::window_events::WindowEvents,
    font::{Font, FontFamily, FontStyle, FontWeight},
    geometry::{Point, Size},
    widgets::panel::Panel,
    window::BackgroundStyle,
    DeviceContext,
};

// Sizes in device-independent pixels.
const WIDTH: i32 = 168;
const ROW: i32 = 46;
const MORE_ROW: i32 = 30;
const PAD: i32 = 4;
const PICTURE: (i32, i32) = (28, 36);
const SEPARATOR: i32 = 9;
const HOVER: (u8, u8, u8) = (226, 236, 253);
const CURRENT_EDGE: (u8, u8, u8) = (43, 111, 232);
const INK: (u8, u8, u8) = (36, 48, 66);
const RULE: (u8, u8, u8) = (223, 228, 236);

/// A row of the menu: a preset, or "More Columns…".
#[derive(Clone, Copy, PartialEq)]
enum Item { Preset(ColumnPreset), More }

fn items() -> Vec<Item> {
    ColumnPreset::ALL.into_iter().map(Item::Preset).chain([Item::More]).collect()
}

/// Top edge and height of each row, in physical pixels.
fn rows(surface: Panel) -> Vec<(Item, i32, i32)> {
    let d = |v: i32| surface.from_dip_int(v);
    let mut top = d(PAD);
    items().into_iter().map(|item| {
        if item == Item::More { top += d(SEPARATOR); }
        let height = d(if item == Item::More { MORE_ROW } else { ROW });
        let row = (item, top, height);
        top += height;
        row
    }).collect()
}

fn item_at(surface: Panel, point: Point) -> Option<Item> {
    rows(surface).into_iter().find(|&(_, top, height)| point.y >= top && point.y < top + height).map(|(item, ..)| item)
}

fn paint(surface: Panel, hovered: Option<Item>, current: Option<ColumnPreset>, text_width_mm: f32) {
    let dc = AutoBufferedPaintDC::new(&surface);
    let size = surface.get_client_size();
    let (width, height) = (size.width.max(1), size.height.max(1));
    let d = |v: i32| surface.from_dip_int(v);
    let Some((mut pixmap, canvas)) = crate::crisp::Canvas::new(&surface, width as i32, height as i32) else { return; };
    pixmap.fill(tiny_skia::Color::WHITE);
    let rows = rows(surface);
    for &(item, top, row_height) in &rows {
        let marked = matches!(item, Item::Preset(preset) if Some(preset) == current);
        if hovered == Some(item) || marked {
            let fill = if hovered == Some(item) { HOVER } else { (240, 245, 253) };
            rect(&mut pixmap, d(PAD), top, width - 2 * d(PAD), row_height, fill);
            if marked { outline(&mut pixmap, d(PAD), top, width - 2 * d(PAD), row_height, CURRENT_EDGE); }
        }
        if let Item::Preset(preset) = item {
            let (picture_width, picture_height) = (d(PICTURE.0) as f32, d(PICTURE.1) as f32);
            let x = (d(PAD) * 3) as f32;
            let y = top as f32 + (row_height as f32 - picture_height) / 2.0;
            column_art::page(&mut pixmap, (x, y, picture_width, picture_height), &ColumnLayout::preset(preset, text_width_mm));
        }
    }
    if let Some(&(_, more_top, _)) = rows.iter().find(|(item, ..)| *item == Item::More) {
        rect(&mut pixmap, d(PAD) * 2, more_top - d(SEPARATOR) / 2 - 1, width - 4 * d(PAD), 1, RULE);
    }
    canvas.present(&dc, &pixmap, 0, 0);
    dc.set_background_mode(wxdragon::dc::BackgroundMode::Transparent);
    if let Some(font) = Font::new_with_details(10, FontFamily::Default as i32, FontStyle::Normal as i32, FontWeight::Normal as i32, false, "") { dc.set_font(&font); }
    dc.set_text_foreground(Colour::rgb(INK.0, INK.1, INK.2));
    for &(item, top, row_height) in &rows {
        let label = match item { Item::Preset(preset) => preset.label(), Item::More => "More Columns…" };
        let (_, text_height) = dc.get_text_extent(&crate::i18n::text(&(label)));
        let x = if item == Item::More { d(PAD) * 3 } else { d(PAD) * 6 + d(PICTURE.0) };
        dc.draw_text(&crate::i18n::text(&(label)), x, top + (row_height - text_height) / 2);
    }
}

fn rect(pixmap: &mut Pixmap, x: i32, y: i32, width: i32, height: i32, (r, g, b): (u8, u8, u8)) {
    let mut paint = tiny_skia::Paint::default();
    paint.set_color_rgba8(r, g, b, 255);
    if let Some(area) = tiny_skia::Rect::from_xywh(x as f32, y as f32, width.max(1) as f32, height.max(1) as f32) {
        pixmap.fill_path(&tiny_skia::PathBuilder::from_rect(area), &paint, tiny_skia::FillRule::Winding, crate::crisp::transform(), None);
    }
}

fn outline(pixmap: &mut Pixmap, x: i32, y: i32, width: i32, height: i32, color: (u8, u8, u8)) {
    rect(pixmap, x, y, width, 1, color);
    rect(pixmap, x, y + height - 1, width, 1, color);
    rect(pixmap, x, y, 1, height, color);
    rect(pixmap, x + width - 1, y, 1, height, color);
}

fn choose(ribbon: &Ribbon, item: Item, text_width_mm: f32) {
    let result = match item {
        Item::Preset(preset) => ribbon.edit(|editor| {
            // A preset keeps the section's "Line between" setting.
            let mut layout = ColumnLayout::preset(preset, text_width_mm);
            layout.line_between = editor.column_layout_at_caret().is_some_and(|current| current.line_between);
            editor.apply_column_layout(&layout, ColumnScope::ThisSection)
        }),
        Item::More => columns_dialog::show(ribbon),
    };
    if let Err(error) = result { ribbon.state.borrow_mut().action_status = Some(error); }
}

pub(super) fn show(ribbon: &Ribbon, anchor: &impl WxWidget) {
    let (current, text_width_mm) = {
        let state = ribbon.state.borrow();
        let Some(properties) = state.editor.section_properties_at_caret() else { return; };
        (ColumnLayout::of(&properties).matching_preset(properties.text_width_mm()), properties.text_width_mm())
    };
    let height = 2 * PAD + ColumnPreset::ALL.len() as i32 * ROW + SEPARATOR + MORE_ROW;
    let scaled = ribbon.ui.main_frame.from_dip(Size::new(WIDTH, height));
    let popup = dropdown::create(&ribbon.ui.main_frame, "Columns", scaled);
    let surface = Panel::builder(&popup).with_pos(dropdown::origin(&popup)).with_size(scaled).build();
    surface.set_background_style(BackgroundStyle::Paint);
    let hovered = Rc::new(Cell::new(current.map(Item::Preset)));
    { let hovered = Rc::clone(&hovered); surface.on_paint(move |_| paint(surface, hovered.get(), current, text_width_mm)); }
    { let hovered = Rc::clone(&hovered); surface.on_mouse_motion(move |event| {
        let WindowEventData::MouseMotion(m) = &event else { return; };
        let next = m.event.get_position().and_then(|p| item_at(surface, p));
        if hovered.replace(next) != next { surface.refresh(false, None); }
    }); }
    { let weak = ribbon.owner.clone(); surface.on_mouse_left_up(move |event| {
        let WindowEventData::MouseButton(m) = &event else { return; };
        let Some(item) = m.event.get_position().and_then(|p| item_at(surface, p)) else { return; };
        dropdown::dismiss(popup);
        if let Some(ribbon) = weak.upgrade() { choose(&ribbon, item, text_width_mm); }
    }); }
    { let hovered = Rc::clone(&hovered); let weak = ribbon.owner.clone();
      dropdown::bind_keys(popup, ribbon.ui.paper_canvas, move |key| {
        use wxdragon::keycode::*;
        let all = items();
        let index = hovered.get().and_then(|item| all.iter().position(|i| *i == item));
        let next = match key {
            WXK_RETURN | WXK_NUMPAD_ENTER => {
                let Some(item) = hovered.get() else { return true; };
                dropdown::dismiss(popup);
                if let Some(ribbon) = weak.upgrade() { choose(&ribbon, item, text_width_mm); }
                return true;
            }
            WXK_UP => index.map_or(all.len() - 1, |i| i.saturating_sub(1)),
            WXK_DOWN => index.map_or(0, |i| (i + 1).min(all.len() - 1)),
            _ => return false,
        };
        hovered.set(Some(all[next])); surface.refresh(false, None);
        true
    }); }
    dropdown::show(popup, anchor, &surface);
}
