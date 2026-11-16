//! Expanded ribbon style gallery, using the same preview cards as the ribbon.
use super::*;
use wxdragon::{
    BrushStyle, DeviceContext, PenStyle,
    dc::AutoBufferedPaintDC,
    font::{Font, FontFamily, FontStyle, FontWeight},
    geometry::{Point, Size},
    widgets::panel::Panel,
    window::BackgroundStyle,
    event::window_events::WindowEvents,
};

const COLUMNS: usize = 3;
const CARD_WIDTH: i32 = 145;
const CARD_HEIGHT: i32 = 78;
const GAP: i32 = 8;
const PAD: i32 = 10;
const HEADER: i32 = 32;
const MAX_ROWS: usize = 4;

#[derive(Clone)]
#[derive(PartialEq)]
pub(super) struct Card {
    pub id: model::StyleId,
    pub name: String,
    pub face: String,
    pub points: i32,
    pub bold: bool,
    pub italic: bool,
    /// The style's own text colour; None is automatic (black).
    pub colour: Option<(u8, u8, u8)>,
}

/// The sample text colour of a card: the style's own, else the ink of
/// automatic text.
pub(super) fn sample_colour(card: &Card) -> Colour {
    let (r, g, b) = card.colour.unwrap_or((32, 33, 36));
    Colour::rgb(r, g, b)
}

/// Paragraph styles in Word's gallery order: Normal, headings, Title and
/// Subtitle, then the rest alphabetically.
pub(super) fn cards(ribbon: &Ribbon) -> (Vec<Card>, Option<model::StyleId>) {
    let state = ribbon.state.borrow();
    let sheet = &state.editor.document.styles;
    let selected = state.editor.formatting_summary().style;
    let mut cards = sheet.styles.values()
        .filter(|style| style.kind == model::StyleKind::Paragraph)
        .map(|style| {
            let character = sheet.resolve_character(Some(&style.id));
            Card {
                id: style.id.clone(), name: style.name.clone(),
                face: character.font_family.unwrap_or_else(|| "Segoe UI".into()),
                points: character.font_size_pt.unwrap_or(11.0).clamp(10.0, 18.0).round() as i32,
                bold: character.bold.unwrap_or(false), italic: character.italic.unwrap_or(false),
                colour: character.color.as_deref().and_then(crate::ribbon::styles::hex_colour)
                    .map(|c| (c.r, c.g, c.b)),
            }
        }).collect::<Vec<_>>();
    let rank = |card: &Card| {
        let name = card.name.to_lowercase();
        if card.id.0 == model::STYLE_NORMAL || name == "normal" { (0, 0, name) }
        else if let Some(level) = name.strip_prefix("heading ").and_then(|l| l.trim().parse::<u32>().ok()) { (1, level, name) }
        else if name == "title" { (2, 0, name) }
        else if name == "subtitle" { (2, 1, name) }
        else { (3, 0, name) }
    };
    cards.sort_by_key(rank);
    // Word shows built-in names capitalized ("heading 1" is Heading 1) and
    // each style once.
    for card in &mut cards {
        let mut chars = card.name.chars();
        if let Some(first) = chars.next() { card.name = first.to_uppercase().chain(chars).collect(); }
    }
    let mut seen = std::collections::HashSet::new();
    cards.retain(|card| seen.insert(card.name.to_lowercase()));
    for card in &mut cards {
        if let Some(style)=sheet.get(&card.id) {card.name=crate::i18n::style_name(style);}
    }
    (cards, selected)
}

pub(super) fn show(ribbon: &Ribbon) {
    let (cards, current) = cards(ribbon);
    if cards.is_empty() { return; }
    let rows = cards.len().div_ceil(COLUMNS).min(MAX_ROWS);
    let width = PAD * 2 + COLUMNS as i32 * CARD_WIDTH + (COLUMNS as i32 - 1) * GAP;
    let height = HEADER + PAD + rows as i32 * (CARD_HEIGHT + GAP) + if cards.len() > COLUMNS * MAX_ROWS { 20 } else { 0 };
    let scale = ribbon.ui.main_frame.from_dip(Size::new(width, height));
    let popup = dropdown::create(&ribbon.ui.main_frame, "Styles", scale);
    let surface = Panel::builder(&popup).with_pos(dropdown::origin(&popup))
        .with_size(scale).build();
    surface.set_background_style(BackgroundStyle::Paint);
    let cards = Rc::new(cards);
    let active = cards.iter().position(|card| current.as_ref() == Some(&card.id)).unwrap_or(0);
    let first_row = Rc::new(Cell::new((active / COLUMNS).min(cards.len().div_ceil(COLUMNS).saturating_sub(MAX_ROWS))));
    let hovered = Rc::new(Cell::new(Some(active)));
    {
        let cards=Rc::clone(&cards);let weak=ribbon.owner.clone();let first_row=Rc::clone(&first_row);
        surface.on_mouse_right_up(move |event| {
            let Some(point)=mouse_position(&event) else{return;};
            let Some(index)=hit(surface,point,first_row.get(),cards.len()) else{return;};
            let id=cards[index].id.clone();let at=crate::gtk_native::to_screen(&surface,point);
            dropdown::dismiss(popup);
            if let Some(ribbon)=weak.upgrade() {ribbon.show_style_context(id,at);}
            event.skip(false);
        });
    }
    {
        let cards = Rc::clone(&cards);
        let first_row = Rc::clone(&first_row);
        let hovered = Rc::clone(&hovered);
        surface.on_paint(move |_| paint(surface, &cards, current.as_ref(), first_row.get(), hovered.get()));
    }
    {
        let cards = Rc::clone(&cards);
        let weak = ribbon.owner.clone();
        let first_row = Rc::clone(&first_row);
        surface.on_mouse_left_down(move |event| {
            let Some(point) = mouse_position(&event) else { return; };
            if point.y < surface.from_dip_int(HEADER) && point.x > surface.get_client_size().width - surface.from_dip_int(30) {
                dropdown::dismiss(popup);
                if let Some(ribbon) = weak.upgrade() { ribbon.focus_document(); }
            } else if let Some(index) = hit(surface, point, first_row.get(), cards.len()) {
                dropdown::dismiss(popup);
                if let Some(ribbon) = weak.upgrade() {
                    if let Err(error) = ribbon.edit(|editor| editor.apply_named_style(&cards[index].id, true)) {
                        ribbon.state.borrow_mut().action_status = Some(error);
                    }
                }
            }
        });
    }
    {
        let cards = Rc::clone(&cards);
        let first_row = Rc::clone(&first_row);
        let hovered = Rc::clone(&hovered);
        surface.on_mouse_motion(move |event| {
            let next = mouse_position(&event).and_then(|p| hit(surface, p, first_row.get(), cards.len()));
            if hovered.replace(next) != next { surface.refresh(false, None); }
        });
    }
    {
        let cards = Rc::clone(&cards);
        let first_row = Rc::clone(&first_row);
        surface.on_mouse_wheel(move |event| {
            let (rotation, delta) = match &event {
                WindowEventData::MouseButton(m) => (m.event.get_wheel_rotation(), m.event.get_wheel_delta()),
                WindowEventData::MouseMotion(m) => (m.event.get_wheel_rotation(), m.event.get_wheel_delta()),
                _ => (0, 0),
            };
            if rotation != 0 {
                let max = cards.len().div_ceil(COLUMNS).saturating_sub(MAX_ROWS);
                let steps = (rotation.abs() / delta.max(1)).max(1) as usize;
                let row = if rotation > 0 { first_row.get().saturating_sub(steps) }
                    else { (first_row.get() + steps).min(max) };
                first_row.set(row);
                surface.refresh(false, None);
            }
        });
    }
    {
    let cards = Rc::clone(&cards);
    let weak = ribbon.owner.clone();
    let first_row = Rc::clone(&first_row);
    let hovered = Rc::clone(&hovered);
    dropdown::bind_keys(popup, ribbon.ui.paper_canvas, move |key| {
            use wxdragon::keycode::*;
            let index = hovered.get().unwrap_or(0);
            let next = match key {
                WXK_RETURN => {
                    dropdown::dismiss(popup);
                    if let Some(ribbon) = weak.upgrade() {
                        if let Err(error) = ribbon.edit(|editor| editor.apply_named_style(&cards[index].id, true)) {
                            ribbon.state.borrow_mut().action_status = Some(error);
                        }
                    }
                    return true;
                }
                WXK_LEFT => Some(index.saturating_sub(1)),
                WXK_RIGHT => Some((index + 1).min(cards.len() - 1)),
                WXK_UP => Some(index.saturating_sub(COLUMNS)),
                WXK_DOWN => Some((index + COLUMNS).min(cards.len() - 1)),
                _ => None,
            };
            if let Some(next) = next {
                hovered.set(Some(next));
                let row = next / COLUMNS;
                if row < first_row.get() { first_row.set(row); }
                if row >= first_row.get() + MAX_ROWS { first_row.set(row + 1 - MAX_ROWS); }
                surface.refresh(false, None);
                return true;
            }
        false
    });
    }
    dropdown::show_over(popup, &ribbon.ui.style_gallery, &surface);
}

fn mouse_position(event: &WindowEventData) -> Option<Point> {
    match event {
        WindowEventData::MouseButton(m) => m.event.get_position(),
        WindowEventData::MouseMotion(m) => m.event.get_position(),
        _ => None,
    }
}

fn hit(surface: Panel, point: Point, first_row: usize, count: usize) -> Option<usize> {
    let x = point.x - surface.from_dip_int(PAD);
    let y = point.y - surface.from_dip_int(HEADER + PAD);
    if x < 0 || y < 0 { return None; }
    let col = x / surface.from_dip_int(CARD_WIDTH + GAP);
    let row = y / surface.from_dip_int(CARD_HEIGHT + GAP);
    if col < 0 || col >= COLUMNS as i32 || row < 0 || row >= MAX_ROWS as i32
        || x % surface.from_dip_int(CARD_WIDTH + GAP) >= surface.from_dip_int(CARD_WIDTH)
        || y % surface.from_dip_int(CARD_HEIGHT + GAP) >= surface.from_dip_int(CARD_HEIGHT) { return None; }
    let index = (first_row + row as usize) * COLUMNS + col as usize;
    (index < count).then_some(index)
}

fn paint(surface: Panel, cards: &[Card], current: Option<&model::StyleId>, first_row: usize, hovered: Option<usize>) {
    let dc = AutoBufferedPaintDC::new(&surface);
    let dip = |v: i32| surface.from_dip_int(v);
    let size = surface.get_client_size();
    dc.set_background(Colour::rgb(255, 255, 255)); dc.clear();
    // The header band, shaded as every dropdown's is (dropdown::BAND).
    let band = Colour::rgb(dropdown::BAND.0, dropdown::BAND.1, dropdown::BAND.2);
    dc.set_pen(band, 1, PenStyle::Solid);
    dc.set_brush(band, BrushStyle::Solid);
    dc.draw_rectangle(0, 0, size.width, dip(HEADER));
    if let Some(font) = Font::new_with_details(10, FontFamily::Default as i32, FontStyle::Normal as i32,
        FontWeight::Bold as i32, false, "Segoe UI") { dc.set_font(&font); }
    dc.set_text_foreground(Colour::rgb(36, 57, 94));
    dc.draw_text(&crate::i18n::text(&("Styles")), dip(PAD), dip(8));
    // The panes' red cross, drawn at the screen's resolution.
    let k = crate::display_scale::window_scale(&surface);
    let side = (dip(super::CLOSE_MARK_DIP as i32) as f32 * k).round().max(1.0) as u32;
    if let Some(bitmap) = super::close_mark(side).and_then(|rgba| wxdragon::Bitmap::from_rgba(&rgba, side, side)) {
        let (x, y) = (size.width - dip(26), (dip(HEADER) - dip(super::CLOSE_MARK_DIP as i32)) / 2);
        let reduction = 1.0 / f64::from(k);
        dc.set_user_scale(reduction, reduction);
        dc.draw_bitmap(&bitmap, (x as f32 * k).round() as i32, (y as f32 * k).round() as i32, true);
        dc.set_user_scale(1.0, 1.0);
    }
    for (index, card) in cards.iter().enumerate().skip(first_row * COLUMNS).take(MAX_ROWS * COLUMNS) {
        let local = index - first_row * COLUMNS;
        let x = dip(PAD + (local % COLUMNS) as i32 * (CARD_WIDTH + GAP));
        let y = dip(HEADER + PAD + (local / COLUMNS) as i32 * (CARD_HEIGHT + GAP));
        let active = current == Some(&card.id);
        let hot = hovered == Some(index);
        dc.set_pen(if active { Colour::rgb(58, 104, 194) } else { Colour::rgb(210, 220, 234) },
            dip(if active { 2 } else { 1 }).max(1), PenStyle::Solid);
        dc.set_brush(if hot { Colour::rgb(232, 239, 253) } else { Colour::rgb(255, 255, 255) }, BrushStyle::Solid);
        dc.draw_rounded_rectangle(x, y, dip(CARD_WIDTH), dip(CARD_HEIGHT), dip(5) as f64);
        if let Some(font) = Font::new_with_details(card.points, FontFamily::Default as i32,
            if card.italic { FontStyle::Italic } else { FontStyle::Normal } as i32,
            if card.bold { FontWeight::Bold } else { FontWeight::Normal } as i32,
            false, &card.face) { dc.set_font(&font); }
        dc.set_text_foreground(sample_colour(card));
        let sample = "AaBbCc";
        let sample_width = dc.get_text_extent(&crate::i18n::text(&(sample))).0;
        dc.draw_text(&crate::i18n::text(&(sample)), x + (dip(CARD_WIDTH) - sample_width) / 2, y + dip(9));
        if let Some(font) = Font::new_with_details(10, FontFamily::Default as i32, FontStyle::Normal as i32,
            FontWeight::Normal as i32, false, "Segoe UI") { dc.set_font(&font); }
        dc.set_text_foreground(Colour::rgb(26, 42, 66));
        let mut name = card.name.clone();
        while dc.get_text_extent(&crate::i18n::text(&(&name))).0 > dip(CARD_WIDTH - 20) && name.chars().count() > 2 {
            name.pop();
        }
        if name != card.name { name.pop(); name.push('…'); }
        let name_width = dc.get_text_extent(&crate::i18n::text(&(&name))).0;
        dc.draw_text(&crate::i18n::text(&(&name)), x + (dip(CARD_WIDTH) - name_width) / 2, y + dip(50));
    }
    if cards.len() > COLUMNS * MAX_ROWS {
        dc.set_text_foreground(Colour::rgb(105, 119, 139));
        dc.draw_text(&crate::i18n::text(&(&format!("Scroll for more styles · {}–{} of {}", first_row * COLUMNS + 1,
            ((first_row + MAX_ROWS) * COLUMNS).min(cards.len()), cards.len()))), dip(PAD), size.height - dip(18));
    }
}
