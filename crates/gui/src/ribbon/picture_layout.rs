//! Picture layout as in Word: the wrapping styles and their pictures, the
//! rules that apply a style or a place without moving the picture, the
//! Position gallery (in line with the text, or one of nine places on the
//! page) and the Wrap Text menu that the Layout Options button beside a
//! selected picture opens. The Picture tools page (picture_bar.rs) shows the
//! same choices on the Ribbon.
use super::*;
use model::{AnchorReference, ImageAnchor, ImageWrap};
use tiny_skia::Pixmap;
use wxdragon::{
    dc::AutoBufferedPaintDC,
    event::window_events::WindowEvents,
    font::{Font, FontFamily, FontStyle, FontWeight},
    geometry::{Point, Rect as WxRect, Size},
    widgets::panel::Panel,
    window::BackgroundStyle,
    DeviceContext,
};

const MM_TO_PT: f32 = 72.0 / 25.4;
/// Distance from text given to a picture that starts to float.
pub(super) const DEFAULT_GAP_MM: f32 = 3.2;

/// The wrapping styles, in Word's order, with their names and tips.
pub(super) const WRAPS: [(ImageWrap, &str, &str); 6] = [
    (ImageWrap::Inline, "In Line with Text", "The picture sits in the line like a large character"),
    (ImageWrap::Square, "Square", "Text wraps around the picture's box"),
    (ImageWrap::Tight, "Tight", "Text wraps close around the picture's outline"),
    (ImageWrap::TopAndBottom, "Top and Bottom", "Text stays above and below the picture"),
    (ImageWrap::BehindText, "Behind Text", "The picture lies behind the text"),
    (ImageWrap::InFrontOfText, "In Front of Text", "The picture covers the text"),
];

// Menu sizes, in device-independent pixels.
const MENU_WIDTH: i32 = 236;
const ROW: i32 = 34;
const CHECK_ROW: i32 = 28;
const PAD: i32 = 4;
const SEPARATOR: i32 = 9;
const ICON: (i32, i32) = (26, 22);
// Position gallery sizes.
const TILE: (i32, i32) = (44, 54);
const TILE_GAP: i32 = 8;
const CAPTION: i32 = 22;
// Colours.
const HOVER: (u8, u8, u8) = (226, 236, 253);
const CURRENT: (u8, u8, u8) = (240, 245, 253);
const CURRENT_EDGE: (u8, u8, u8) = (43, 111, 232);
const INK: (u8, u8, u8) = (36, 48, 66);
const MUTED: (u8, u8, u8) = (120, 130, 145);
const RULE: (u8, u8, u8) = (223, 228, 236);
const PAPER: (u8, u8, u8) = (255, 255, 255);
const PAGE_EDGE: (u8, u8, u8) = (160, 170, 184);
const TEXT_LINE: (u8, u8, u8) = (150, 160, 175);
const PICTURE: (u8, u8, u8) = (70, 130, 220);
const PICTURE_PALE: (u8, u8, u8) = (190, 212, 245);

/// The selected picture's layout, as the menus show it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Current { pub wrap: ImageWrap, pub fixed: bool }

/// Where a picture goes when a menu entry is chosen.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Choice { Wrap(ImageWrap), MoveWithText, FixOnPage, MoreOptions, InLine, Place(u8, u8) }

pub(super) fn selected_picture(ribbon: &Ribbon) -> Option<NodeId> {
    match ribbon.state.borrow().selected_object.as_ref() { Some(objects_ui::SelectedObject::Picture(id)) => Some(id.clone()), _ => None }
}

pub(super) fn current(ribbon: &Ribbon, id: &NodeId) -> Current {
    let state = ribbon.state.borrow();
    Current { wrap: state.editor.picture_wrap(id), fixed: state.editor.picture_anchor(id).is_some_and(|a| a.reference == AnchorReference::Page) }
}

// Pictures --------------------------------------------------------------------------------------

fn fill(pixmap: &mut Pixmap, (x, y, w, h): (f32, f32, f32, f32), (r, g, b): (u8, u8, u8)) {
    let mut paint = tiny_skia::Paint::default();
    paint.set_color_rgba8(r, g, b, 255);
    if let Some(rect) = tiny_skia::Rect::from_xywh(x, y, w.max(0.5), h.max(0.5)) {
        pixmap.fill_path(&tiny_skia::PathBuilder::from_rect(rect), &paint, tiny_skia::FillRule::Winding, crate::crisp::transform(), None);
    }
}

fn ellipse(pixmap: &mut Pixmap, (x, y, w, h): (f32, f32, f32, f32), (r, g, b): (u8, u8, u8)) {
    let mut paint = tiny_skia::Paint::default();
    paint.set_color_rgba8(r, g, b, 255);
    paint.anti_alias = true;
    if let Some(path) = tiny_skia::Rect::from_xywh(x, y, w, h).and_then(tiny_skia::PathBuilder::from_oval) {
        pixmap.fill_path(&path, &paint, tiny_skia::FillRule::Winding, crate::crisp::transform(), None);
    }
}

/// A small picture of a wrapping style: text lines and a picture placed as
/// the style places them, in the rectangle `(x, y, w, h)`.
pub(super) fn paint_wrap(pixmap: &mut Pixmap, (x, y, w, h): (f32, f32, f32, f32), wrap: ImageWrap) {
    let rows = 5;
    let step = h / rows as f32;
    let line = (step * 0.42).max(1.0);
    let picture = (x + w * 0.3, y + step * 1.0, w * 0.4, step * 3.0 - (step - line));
    let lines = |pixmap: &mut Pixmap, skip: &dyn Fn(usize) -> Option<(f32, f32)>| {
        for row in 0..rows {
            let top = y + row as f32 * step + (step - line) / 2.0;
            match skip(row) {
                Some((gap_left, gap_right)) => {
                    if gap_left > x { fill(pixmap, (x, top, gap_left - x, line), TEXT_LINE); }
                    if gap_right < x + w { fill(pixmap, (gap_right, top, x + w - gap_right, line), TEXT_LINE); }
                }
                None => fill(pixmap, (x, top, w, line), TEXT_LINE),
            }
        }
    };
    let beside = |row: usize| (1..=3).contains(&row);
    match wrap {
        ImageWrap::Inline => {
            // The picture sits on the third line, like a large character.
            lines(pixmap, &|row| (row == 2 || row == 3).then_some((x + w * 0.42, x + w * 0.72)));
            fill(pixmap, (x + w * 0.45, y + step * 1.9, w * 0.24, step * 1.9), PICTURE);
        }
        ImageWrap::Square => {
            lines(pixmap, &|row| beside(row).then_some((picture.0 - w * 0.08, picture.0 + picture.2 + w * 0.08)));
            fill(pixmap, picture, PICTURE);
        }
        ImageWrap::Tight => {
            let narrow = |row: usize| if row == 2 { w * 0.04 } else { w * 0.12 };
            lines(pixmap, &|row| beside(row).then(|| (picture.0 + narrow(row) - w * 0.04, picture.0 + picture.2 - narrow(row) + w * 0.04)));
            ellipse(pixmap, picture, PICTURE);
        }
        ImageWrap::TopAndBottom => {
            lines(pixmap, &|row| beside(row).then_some((x, x + w)));
            fill(pixmap, picture, PICTURE);
        }
        ImageWrap::BehindText => {
            fill(pixmap, picture, PICTURE_PALE);
            lines(pixmap, &|_| None);
        }
        ImageWrap::InFrontOfText => {
            lines(pixmap, &|_| None);
            fill(pixmap, picture, PICTURE);
        }
    }
}

/// A page with text and, unless `place` is None (in line), a picture at
/// column `place.0`, row `place.1` of a 3 × 3 grid with the text around it.
fn paint_place(pixmap: &mut Pixmap, (x, y, w, h): (f32, f32, f32, f32), place: Option<(u8, u8)>) {
    fill(pixmap, (x, y, w, h), PAGE_EDGE);
    fill(pixmap, (x + 1.0, y + 1.0, w - 2.0, h - 2.0), PAPER);
    let margin = w * 0.14;
    let (left, top, width, height) = (x + margin, y + margin, w - 2.0 * margin, h - 2.0 * margin);
    let picture = place.map(|(column, row)| {
        let (pw, ph) = (width * 0.36, height * 0.26);
        (left + (width - pw) * f32::from(column) / 2.0, top + (height - ph) * f32::from(row) / 2.0, pw, ph)
    });
    let step = (height / 9.0).max(2.0);
    let line = (step * 0.45).max(1.0);
    let mut line_top = top;
    while line_top + line <= top + height {
        match picture {
            Some((px, py, pw, ph)) if line_top + line > py - 1.0 && line_top < py + ph + 1.0 => {
                if px - 2.0 > left { fill(pixmap, (left, line_top, px - 2.0 - left, line), TEXT_LINE); }
                if px + pw + 2.0 < left + width { fill(pixmap, (px + pw + 2.0, line_top, left + width - px - pw - 2.0, line), TEXT_LINE); }
            }
            _ => fill(pixmap, (left, line_top, width, line), TEXT_LINE),
        }
        line_top += step;
    }
    match picture {
        Some(rect) => fill(pixmap, rect, PICTURE),
        None => fill(pixmap, (left + width * 0.35, top + height * 0.38, width * 0.3, height * 0.22), PICTURE),
    }
}

// Placing pictures ------------------------------------------------------------------------------

/// The picture's box on its page, in points, as last laid out.
fn shown_rect(ribbon: &Ribbon, id: &NodeId) -> Option<(usize, render::Rect)> {
    objects_ui::region_for(&ribbon.state.borrow(), id).map(|(page, region)| (page, region.rect))
}

/// Where layout measures a "move with text" picture from: the anchor
/// paragraph's left text edge and top on `page`, in points.
pub(super) fn paragraph_origin(ribbon: &Ribbon, page: usize, paragraph: &NodeId) -> Option<(f32, f32)> {
    let state = ribbon.state.borrow();
    let top = state.vector_document.pages.get(page)?.hit_regions.iter().filter(|r| r.node_id == paragraph.0).map(|r| r.rect.y).reduce(f32::min)?;
    let section = state.editor.document.sections.iter().find(|s| s.blocks.iter().any(|b| matches!(b, model::Block::Paragraph(p) if &p.id == paragraph)))?;
    Some((section.properties.margin_left_mm * MM_TO_PT, top))
}

/// An anchor that keeps the picture at `rect` (points on `page`), measured
/// from the page or, `fixed` false, from its anchor paragraph.
fn anchor_at(ribbon: &Ribbon, id: &NodeId, page: usize, rect: render::Rect, fixed: bool) -> Result<ImageAnchor, String> {
    let (paragraph, gap, locked) = {
        let state = ribbon.state.borrow();
        let old = state.editor.picture_anchor(id).cloned();
        // A picture that starts to float is anchored to the last paragraph
        // starting above it: anchored to one below, wrapping would push that
        // paragraph down and the picture with it, and layout would not settle.
        state.editor.picture_body_paragraph(id).ok_or("Only pictures in the body text can float; pictures in tables, headers or notes stay in line")?;
        let paragraph = old.as_ref().map(|a| a.paragraph.clone())
            .or_else(|| objects_ui::anchor_paragraph(&state, page, rect.y))
            .or_else(|| state.editor.picture_body_paragraph(id))
            .ok_or("Only pictures in the body text can float")?;
        (paragraph, old.as_ref().map_or(DEFAULT_GAP_MM, |a| a.gap_mm), old.is_some_and(|a| a.locked))
    };
    // A paragraph not shown on the picture's page cannot be measured from.
    let origin = if fixed { None } else { paragraph_origin(ribbon, page, &paragraph) };
    let (reference, (origin_x, origin_y)) = match origin {
        Some(origin) => (AnchorReference::Paragraph, origin),
        None => (AnchorReference::Page, (0.0, 0.0)),
    };
    Ok(ImageAnchor { paragraph, reference, x_mm: (rect.x - origin_x) / MM_TO_PT, y_mm: (rect.y - origin_y) / MM_TO_PT, gap_mm: gap, locked })
}

/// Applies a wrapping style. A picture that starts to float stays where it
/// is and moves with its text, as in Word.
pub(super) fn set_wrap(ribbon: &Ribbon, id: &NodeId, wrap: ImageWrap) -> Result<(), String> {
    let now = current(ribbon, id);
    if wrap == now.wrap { return Ok(()); }
    if wrap == ImageWrap::Inline { return ribbon.edit(|e| e.make_picture_inline(id)); }
    let anchor = match ribbon.state.borrow().editor.picture_anchor(id).cloned() {
        Some(anchor) => anchor,
        None => { let (page, rect) = shown_rect(ribbon, id).ok_or("Show the picture on a page first")?; anchor_at(ribbon, id, page, rect, false)? }
    };
    ribbon.edit(|e| e.position_picture(id, anchor, wrap))
}

/// Move with Text (false) or Fix Position on Page (true), keeping the
/// picture where it is now.
pub(super) fn set_fixed(ribbon: &Ribbon, id: &NodeId, fixed: bool) -> Result<(), String> {
    let now = current(ribbon, id);
    if now.wrap == ImageWrap::Inline || now.fixed == fixed { return Ok(()); }
    let (page, rect) = shown_rect(ribbon, id).ok_or("Show the picture on a page first")?;
    let anchor = anchor_at(ribbon, id, page, rect, fixed)?;
    ribbon.edit(|e| e.position_picture(id, anchor, now.wrap))
}

/// The picture's top-left corner (points) for column and row 0–2 of the
/// page's text area, given the section's page setup.
pub(super) fn place_origin(section: &model::SectionProperties, size_mm: (f32, f32), column: u8, row: u8) -> (f32, f32) {
    let (left, right) = (section.margin_left_mm, section.page_width_mm - section.margin_right_mm);
    let (top, bottom) = (section.margin_top_mm, section.page_height_mm - section.margin_bottom_mm);
    let x = left + (right - left - size_mm.0).max(0.0) * f32::from(column.min(2)) / 2.0;
    let y = top + (bottom - top - size_mm.1).max(0.0) * f32::from(row.min(2)) / 2.0;
    (x * MM_TO_PT, y * MM_TO_PT)
}

/// Puts the picture at one of nine places of the page's text area, fixed on
/// the page with text wrapping around it (Square unless it already wraps).
fn place(ribbon: &Ribbon, id: &NodeId, column: u8, row: u8) -> Result<(), String> {
    let (section, size) = {
        let state = ribbon.state.borrow();
        (state.editor.picture_section(id).cloned().ok_or("Only pictures in the body text can be placed on the page")?,
         state.editor.picture_size(id).ok_or("Picture not found")?)
    };
    let (page, _) = shown_rect(ribbon, id).ok_or("Show the picture on a page first")?;
    let (x, y) = place_origin(&section, size, column, row);
    let rect = render::Rect::new(x, y, size.0 * MM_TO_PT, size.1 * MM_TO_PT);
    let anchor = anchor_at(ribbon, id, page, rect, true)?;
    let wrap = match current(ribbon, id).wrap { ImageWrap::Inline => ImageWrap::Square, wrap => wrap };
    ribbon.edit(|e| e.position_picture(id, anchor, wrap))
}

fn choose(ribbon: &Ribbon, id: &NodeId, choice: Choice) {
    let result = match choice {
        Choice::Wrap(wrap) => set_wrap(ribbon, id, wrap),
        Choice::MoveWithText => set_fixed(ribbon, id, false),
        Choice::FixOnPage => set_fixed(ribbon, id, true),
        Choice::MoreOptions => layout_dialog::show(ribbon, id),
        Choice::InLine => set_wrap(ribbon, id, ImageWrap::Inline),
        Choice::Place(column, row) => place(ribbon, id, column, row),
    };
    if let Err(error) = result { ribbon.state.borrow_mut().action_status = Some(error); }
}

// Wrap Text menu --------------------------------------------------------------------------------

/// Rows of the Wrap Text menu: entry, top and height in physical pixels.
fn wrap_rows(surface: Panel) -> Vec<(Choice, i32, i32)> {
    let d = |v: i32| surface.from_dip_int(v);
    let mut top = d(PAD);
    let mut out = Vec::new();
    for (wrap, ..) in WRAPS { out.push((Choice::Wrap(wrap), top, d(ROW))); top += d(ROW); }
    top += d(SEPARATOR);
    for choice in [Choice::MoveWithText, Choice::FixOnPage] { out.push((choice, top, d(CHECK_ROW))); top += d(CHECK_ROW); }
    top += d(SEPARATOR);
    out.push((Choice::MoreOptions, top, d(CHECK_ROW)));
    out
}

fn wrap_menu_height() -> i32 { 2 * PAD + WRAPS.len() as i32 * ROW + 2 * SEPARATOR + 3 * CHECK_ROW }

fn label(choice: Choice) -> &'static str {
    match choice {
        Choice::Wrap(wrap) => WRAPS.iter().find(|w| w.0 == wrap).map_or("", |w| w.1),
        Choice::MoveWithText => "Move with Text",
        Choice::FixOnPage => "Fix Position on Page",
        Choice::MoreOptions => "More Layout Options…",
        Choice::InLine => "In Line with Text",
        Choice::Place(..) => "",
    }
}

fn enabled(choice: Choice, now: Current) -> bool {
    !(matches!(choice, Choice::MoveWithText | Choice::FixOnPage) && now.wrap == ImageWrap::Inline)
}

fn marked(choice: Choice, now: Current) -> bool {
    match choice {
        Choice::Wrap(wrap) => wrap == now.wrap,
        Choice::MoveWithText => now.wrap != ImageWrap::Inline && !now.fixed,
        Choice::FixOnPage => now.wrap != ImageWrap::Inline && now.fixed,
        _ => false,
    }
}

fn set_font(dc: &impl DeviceContext) {
    if let Some(font) = Font::new_with_details(10, FontFamily::Default as i32, FontStyle::Normal as i32, FontWeight::Normal as i32, false, "") { dc.set_font(&font); }
}

fn rect(pixmap: &mut Pixmap, x: i32, y: i32, w: i32, h: i32, colour: (u8, u8, u8)) { fill(pixmap, (x as f32, y as f32, w as f32, h as f32), colour); }

fn outline(pixmap: &mut Pixmap, x: i32, y: i32, w: i32, h: i32, colour: (u8, u8, u8)) {
    rect(pixmap, x, y, w, 1, colour); rect(pixmap, x, y + h - 1, w, 1, colour);
    rect(pixmap, x, y, 1, h, colour); rect(pixmap, x + w - 1, y, 1, h, colour);
}

fn paint_wrap_menu(surface: Panel, hovered: Option<Choice>, now: Current) {
    let dc = AutoBufferedPaintDC::new(&surface);
    let size = surface.get_client_size();
    let (width, height) = (size.width.max(1), size.height.max(1));
    let d = |v: i32| surface.from_dip_int(v);
    let Some((mut pixmap, canvas)) = crate::crisp::Canvas::new(&surface, width as i32, height as i32) else { return; };
    pixmap.fill(tiny_skia::Color::WHITE);
    let rows = wrap_rows(surface);
    for &(choice, top, row) in &rows {
        let on = enabled(choice, now);
        if on && (hovered == Some(choice) || marked(choice, now) && matches!(choice, Choice::Wrap(_))) {
            rect(&mut pixmap, d(PAD), top, width - 2 * d(PAD), row, if hovered == Some(choice) { HOVER } else { CURRENT });
            if marked(choice, now) { outline(&mut pixmap, d(PAD), top, width - 2 * d(PAD), row, CURRENT_EDGE); }
        }
        if let Choice::Wrap(wrap) = choice {
            let (w, h) = (d(ICON.0) as f32, d(ICON.1) as f32);
            paint_wrap(&mut pixmap, ((d(PAD) * 3) as f32, top as f32 + (row as f32 - h) / 2.0, w, h), wrap);
        }
    }
    for separator in [WRAPS.len(), WRAPS.len() + 2] {
        if let Some(&(_, top, _)) = rows.get(separator) { rect(&mut pixmap, d(PAD) * 2, top - d(SEPARATOR) / 2 - 1, width - 4 * d(PAD), 1, RULE); }
    }
    canvas.present(&dc, &pixmap, 0, 0);
    dc.set_background_mode(wxdragon::dc::BackgroundMode::Transparent);
    set_font(&dc);
    for &(choice, top, row) in &rows {
        let ink = if enabled(choice, now) { INK } else { MUTED };
        dc.set_text_foreground(Colour::rgb(ink.0, ink.1, ink.2));
        let text = label(choice);
        let (_, text_height) = dc.get_text_extent(&crate::i18n::text(&(text)));
        let x = d(PAD) * 5 + d(ICON.0);
        dc.draw_text(&crate::i18n::text(&(text)), x, top + (row - text_height) / 2);
        if matches!(choice, Choice::MoveWithText | Choice::FixOnPage) && marked(choice, now) {
            dc.draw_text(&crate::i18n::text(&("✓")), d(PAD) * 3 + d(ICON.0) / 2 - 4, top + (row - text_height) / 2);
        }
    }
}

/// Opens the Wrap Text menu below `screen` (the Layout Options button on
/// the page, in screen coordinates).
pub(super) fn show_wrap_menu(ribbon: &Ribbon, screen: WxRect) {
    let Some(id) = selected_picture(ribbon) else { ribbon.state.borrow_mut().action_status = Some("Select a picture first".into()); return; };
    let now = current(ribbon, &id);
    let scaled = ribbon.ui.main_frame.from_dip(Size::new(MENU_WIDTH, wrap_menu_height()));
    let popup = dropdown::create(&ribbon.ui.main_frame, "Wrap Text", scaled);
    let surface = Panel::builder(&popup).with_pos(dropdown::origin(&popup)).with_size(scaled).build();
    surface.set_background_style(BackgroundStyle::Paint);
    let hovered = Rc::new(Cell::new(None::<Choice>));
    { let hovered = Rc::clone(&hovered); surface.on_paint(move |_| paint_wrap_menu(surface, hovered.get(), now)); }
    let at = move |p: Point| wrap_rows(surface).into_iter().find(|&(_, top, h)| p.y >= top && p.y < top + h).map(|(c, ..)| c).filter(|c| enabled(*c, now));
    { let hovered = Rc::clone(&hovered); surface.on_mouse_motion(move |event| {
        let WindowEventData::MouseMotion(m) = &event else { return; };
        let next = m.event.get_position().and_then(at);
        if hovered.replace(next) != next { surface.refresh(false, None); }
    }); }
    { let weak = ribbon.owner.clone(); let id = id.clone(); surface.on_mouse_left_up(move |event| {
        let WindowEventData::MouseButton(m) = &event else { return; };
        let Some(choice) = m.event.get_position().and_then(at) else { return; };
        dropdown::dismiss(popup);
        if let Some(ribbon) = weak.upgrade() { choose(&ribbon, &id, choice); }
    }); }
    bind_menu_keys(ribbon, popup, surface, Rc::clone(&hovered), wrap_rows(surface).into_iter().map(|r| r.0).filter(|c| enabled(*c, now)).collect(), id);
    dropdown::show_below_rect(popup, screen, &surface);
}

fn bind_menu_keys(ribbon: &Ribbon, popup: Panel, surface: Panel, hovered: Rc<Cell<Option<Choice>>>, items: Vec<Choice>, id: NodeId) {
    let weak = ribbon.owner.clone();
    dropdown::bind_keys(popup, ribbon.ui.paper_canvas, move |key| {
        use wxdragon::keycode::*;
        let index = hovered.get().and_then(|item| items.iter().position(|i| *i == item));
        let next = match key {
            WXK_RETURN | WXK_NUMPAD_ENTER => {
                let Some(choice) = hovered.get() else { return true; };
                dropdown::dismiss(popup);
                if let Some(ribbon) = weak.upgrade() { choose(&ribbon, &id, choice); }
                return true;
            }
            WXK_UP | WXK_LEFT => index.map_or(items.len().saturating_sub(1), |i| i.saturating_sub(1)),
            WXK_DOWN | WXK_RIGHT => index.map_or(0, |i| (i + 1).min(items.len().saturating_sub(1))),
            _ => return false,
        };
        if let Some(&item) = items.get(next) { hovered.set(Some(item)); surface.refresh(false, None); }
        true
    });
}

// Position gallery ------------------------------------------------------------------------------

/// Tiles of the Position gallery: entry and rectangle in physical pixels.
fn place_tiles(surface: Panel) -> Vec<(Choice, i32, i32, i32, i32)> {
    let d = |v: i32| surface.from_dip_int(v);
    let (w, h, gap) = (d(TILE.0), d(TILE.1), d(TILE_GAP));
    let left = d(PAD * 3);
    let mut out = vec![(Choice::InLine, left, d(PAD) + d(CAPTION), w, h)];
    let top = d(PAD) + d(CAPTION) + h + d(CAPTION) + d(SEPARATOR);
    for row in 0..3u8 {
        for column in 0..3u8 {
            out.push((Choice::Place(column, row), left + i32::from(column) * (w + gap), top + i32::from(row) * (h + gap), w, h));
        }
    }
    out
}

fn place_gallery_size() -> (i32, i32) { (PAD * 6 + 3 * TILE.0 + 2 * TILE_GAP, PAD * 2 + 2 * CAPTION + SEPARATOR + 4 * TILE.1 + 2 * TILE_GAP + PAD) }

fn paint_place_gallery(surface: Panel, hovered: Option<Choice>) {
    let dc = AutoBufferedPaintDC::new(&surface);
    let size = surface.get_client_size();
    let (width, height) = (size.width.max(1), size.height.max(1));
    let d = |v: i32| surface.from_dip_int(v);
    let Some((mut pixmap, canvas)) = crate::crisp::Canvas::new(&surface, width as i32, height as i32) else { return; };
    pixmap.fill(tiny_skia::Color::WHITE);
    let tiles = place_tiles(surface);
    for &(choice, x, y, w, h) in &tiles {
        if hovered == Some(choice) { rect(&mut pixmap, x - d(3), y - d(3), w + d(6), h + d(6), HOVER); outline(&mut pixmap, x - d(3), y - d(3), w + d(6), h + d(6), CURRENT_EDGE); }
        let place = match choice { Choice::Place(column, row) => Some((column, row)), _ => None };
        paint_place(&mut pixmap, (x as f32, y as f32, w as f32, h as f32), place);
    }
    let rule_y = tiles[0].2 + tiles[0].4 + d(SEPARATOR) / 2 + d(2);
    rect(&mut pixmap, d(PAD) * 2, rule_y, width - 4 * d(PAD), 1, RULE);
    canvas.present(&dc, &pixmap, 0, 0);
    dc.set_background_mode(wxdragon::dc::BackgroundMode::Transparent);
    set_font(&dc);
    dc.set_text_foreground(Colour::rgb(MUTED.0, MUTED.1, MUTED.2));
    dc.draw_text(&crate::i18n::text(&("In Line with Text")), d(PAD * 3), d(PAD) + d(4));
    dc.draw_text(&crate::i18n::text(&("With Text Wrapping")), d(PAD * 3), rule_y + d(6));
}

const PLACE_NAMES: [[&str; 3]; 3] = [
    ["Top Left", "Top Center", "Top Right"],
    ["Middle Left", "Middle Center", "Middle Right"],
    ["Bottom Left", "Bottom Center", "Bottom Right"],
];

fn place_tip(choice: Choice) -> String {
    match choice {
        Choice::Place(column, row) => format!("Position in {} with Square Text Wrapping", PLACE_NAMES[usize::from(row)][usize::from(column)]),
        _ => "In Line with Text".into(),
    }
}

/// Opens the Position gallery below `screen`, its button on the Picture
/// tools page (screen coordinates).
pub(super) fn show_place_gallery(ribbon: &Ribbon, screen: WxRect) {
    let Some(id) = selected_picture(ribbon) else { ribbon.state.borrow_mut().action_status = Some("Select a picture first".into()); return; };
    let (w, h) = place_gallery_size();
    let scaled = ribbon.ui.main_frame.from_dip(Size::new(w, h));
    let popup = dropdown::create(&ribbon.ui.main_frame, "Position", scaled);
    let surface = Panel::builder(&popup).with_pos(dropdown::origin(&popup)).with_size(scaled).build();
    surface.set_background_style(BackgroundStyle::Paint);
    let hovered = Rc::new(Cell::new(None::<Choice>));
    { let hovered = Rc::clone(&hovered); surface.on_paint(move |_| paint_place_gallery(surface, hovered.get())); }
    let at = move |p: Point| place_tiles(surface).into_iter().find(|&(_, x, y, w, h)| p.x >= x && p.x < x + w && p.y >= y && p.y < y + h).map(|t| t.0);
    { let hovered = Rc::clone(&hovered); surface.on_mouse_motion(move |event| {
        let WindowEventData::MouseMotion(m) = &event else { return; };
        let next = m.event.get_position().and_then(at);
        if hovered.replace(next) != next {
            surface.set_tooltip(&crate::i18n::text(&(&next.map(place_tip).unwrap_or_default())));
            surface.refresh(false, None);
        }
    }); }
    { let weak = ribbon.owner.clone(); let id = id.clone(); surface.on_mouse_left_up(move |event| {
        let WindowEventData::MouseButton(m) = &event else { return; };
        let Some(choice) = m.event.get_position().and_then(at) else { return; };
        dropdown::dismiss(popup);
        if let Some(ribbon) = weak.upgrade() { choose(&ribbon, &id, choice); }
    }); }
    bind_menu_keys(ribbon, popup, surface, hovered, place_tiles(surface).into_iter().map(|t| t.0).collect(), id);
    dropdown::show_below_rect(popup, screen, &surface);
}

