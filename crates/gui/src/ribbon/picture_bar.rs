//! The Picture tools page, shown in place of the Insert page while a picture
//! is selected, as Word's Picture Format tab and like the Table tools: the
//! wrapping styles as large buttons, Position (gallery, move with text or
//! fix on the page, more options), Size (width and height steppers, lock
//! ratio), Arrange (align, rotate, delete) and Close Picture Tools. One
//! painted surface, drawn the same on every platform.
use super::*;
use super::bar_paint::{fill_round, large_top, line, position, scale_of, stroke_round};
use super::picture_layout::WRAPS;
use super::table_bar::{draw_icon, Icon};
use model::{ImageAlignment, ImageWrap};
use tiny_skia::Pixmap;
use wxdragon::{
    dc::AutoBufferedPaintDC,
    event::window_events::WindowEvents,
    font::{Font, FontFamily, FontStyle, FontWeight},
    widgets::panel::Panel,
    window::BackgroundStyle,
    DeviceContext,
};

/// Size change of one press of a width or height stepper, in millimetres.
const STEP_MM: f32 = 1.0;
const MIN_SIZE_MM: f32 = 1.0;
const MENU_FIRST: i32 = 32200;
const GROUP_PAD: i32 = 6;
const CAPTION: i32 = 17;
const SMALL_H: i32 = 22;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Cmd { CropShape, Wrap(ImageWrap), Place, MoveWithText, FixOnPage, MoreOptions, Width(i8), Height(i8), LockRatio, Align(u8), Rotate, Delete, Close, Label, Value }

/// A command's picture: a Yaru icon by its Impress name, or one of the Table
/// bar's drawn icons (the steppers and the red close button).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Glyph { Table(Icon), Yaru(&'static str) }

#[derive(Clone, Copy, PartialEq)]
enum Style { Large, Small, IconOnly, Value, Caption }

#[derive(Clone)]
struct Button { cmd: Cmd, glyph: Option<Glyph>, label: String, tip: &'static str, style: Style, rect: (i32, i32, i32, i32) }

struct Layout { buttons: Vec<Button>, groups: Vec<(i32, i32, &'static str)> }

/// What the page shows about the selected picture.
#[derive(Clone, Debug, PartialEq)]
struct Facts { id: NodeId, wrap: ImageWrap, fixed: bool, size_mm: (f32, f32), alignment: Option<ImageAlignment>, in_body: bool }

pub(super) struct PictureBar {
    panel: Panel,
    ribbon: std::rc::Weak<Ribbon>,
    hover: Cell<Option<Cmd>>,
    pressed: Cell<Option<Cmd>>,
    lock_ratio: Cell<bool>,
    font: Option<Font>,
    shown: RefCell<Option<Option<Facts>>>,
}

pub(super) fn install(ribbon: &Rc<Ribbon>) -> Rc<PictureBar> {
    let panel = ribbon.ui.picture_context_tools;
    let font = Font::new_with_details(9, FontFamily::Default as i32, FontStyle::Normal as i32, FontWeight::Normal as i32, false, "Segoe UI");
    if let Some(font) = &font { panel.set_font(font); }
    let bar = Rc::new(PictureBar { panel, ribbon: Rc::downgrade(ribbon), hover: Cell::new(None), pressed: Cell::new(None), lock_ratio: Cell::new(true), font, shown: RefCell::new(None) });
    let b = Rc::downgrade(&bar);
    panel.on_menu_selected(move |event| {
        let Some(b) = b.upgrade() else { return; };
        let turn = match event.get_id() - MENU_FIRST { 0 => 90.0, 1 => -90.0, 2 => 180.0, _ => { event.skip(true); return; } };
        b.rotate(turn);
    });
    panel.set_background_style(BackgroundStyle::Paint);
    panel.on_erase_background(|_| {});
    let b = Rc::downgrade(&bar);
    panel.on_paint(move |_| { if let Some(b) = b.upgrade() { b.paint(); } });
    panel.on_size(move |event| { panel.refresh(false, None); event.skip(true); });
    let b = Rc::downgrade(&bar);
    panel.on_mouse_motion(move |event| {
        let (Some(b), Some((x, y))) = (b.upgrade(), position(&event)) else { return; };
        let hit = b.hit(x, y);
        if b.hover.get() != hit.as_ref().map(|h| h.cmd) {
            b.hover.set(hit.as_ref().map(|h| h.cmd));
            panel.set_tooltip(&crate::i18n::text(&(hit.as_ref().map_or("", |h| h.tip))));
            panel.refresh(false, None);
        }
    });
    let b = Rc::downgrade(&bar);
    panel.on_mouse_leave(move |_| { if let Some(b) = b.upgrade() { b.pressed.set(None); if b.hover.replace(None).is_some() { panel.refresh(false, None); } } });
    let b = Rc::downgrade(&bar);
    panel.on_mouse_left_down(move |event| {
        let (Some(b), Some((x, y))) = (b.upgrade(), position(&event)) else { return; };
        let facts = b.facts();
        b.pressed.set(b.hit(x, y).filter(|h| enabled(h.cmd, &facts)).map(|h| h.cmd));
        panel.refresh(false, None);
    });
    let b = Rc::downgrade(&bar);
    panel.on_mouse_left_up(move |event| {
        let (Some(b), Some((x, y))) = (b.upgrade(), position(&event)) else { return; };
        let pressed = b.pressed.take();
        panel.refresh(false, None);
        let hit = b.hit(x, y);
        if let (Some(cmd), Some(hit)) = (pressed, hit) { if hit.cmd == cmd { b.activate(cmd, hit.rect); } }
    });
    bar
}

fn enabled(cmd: Cmd, facts: &Option<Facts>) -> bool {
    let Some(f) = facts else { return cmd == Cmd::Close; };
    match cmd {
        Cmd::Label | Cmd::Value => false,
        Cmd::CropShape | Cmd::Wrap(ImageWrap::Inline) | Cmd::Width(_) | Cmd::Height(_) | Cmd::LockRatio | Cmd::Rotate | Cmd::Delete | Cmd::Close => true,
        Cmd::Wrap(_) | Cmd::Place | Cmd::MoreOptions => f.in_body,
        Cmd::MoveWithText | Cmd::FixOnPage => f.in_body && f.wrap != ImageWrap::Inline,
        Cmd::Align(_) => f.wrap == ImageWrap::Inline && f.alignment.is_some(),
    }
}

fn toggled(cmd: Cmd, facts: &Option<Facts>, lock_ratio: bool) -> bool {
    let Some(f) = facts else { return false; };
    match cmd {
        Cmd::Wrap(wrap) => wrap == f.wrap,
        Cmd::MoveWithText => f.wrap != ImageWrap::Inline && !f.fixed,
        Cmd::FixOnPage => f.wrap != ImageWrap::Inline && f.fixed,
        Cmd::LockRatio => lock_ratio,
        Cmd::Align(at) => f.alignment.is_some_and(|a| a as u8 == at),
        _ => false,
    }
}

/// The new size after a stepper press: `step` millimetres on one side, the
/// other side following when the ratio is locked.
fn stepped(size: (f32, f32), width_step: f32, height_step: f32, lock: bool) -> (f32, f32) {
    let (w, h) = size;
    if width_step != 0.0 {
        let nw = (w + width_step).max(MIN_SIZE_MM);
        (nw, if lock && w > 0.0 { (h * nw / w).max(MIN_SIZE_MM) } else { h })
    } else {
        let nh = (h + height_step).max(MIN_SIZE_MM);
        (if lock && h > 0.0 { (w * nh / h).max(MIN_SIZE_MM) } else { w }, nh)
    }
}

impl PictureBar {
    /// Repaints only when the selected picture's facts changed (or the page was hidden).
    pub(super) fn refresh(&self) {
        if !self.panel.is_shown() { self.shown.replace(None); return; }
        let facts = self.facts();
        if *self.shown.borrow() != Some(facts.clone()) { self.shown.replace(Some(facts)); self.panel.refresh(false, None); }
    }

    fn facts(&self) -> Option<Facts> {
        let ribbon = self.ribbon.upgrade()?;
        let state = ribbon.state.try_borrow().ok()?;
        let Some(objects_ui::SelectedObject::Picture(id)) = state.selected_object.as_ref() else { return None; };
        let editor = &state.editor;
        let alignment = editor.document.sections.iter().flat_map(|s| s.blocks.iter())
            .find_map(|b| match b { model::Block::Image(i) if &i.id == id => Some(i.alignment), _ => None });
        Some(Facts {
            id: id.clone(), wrap: editor.picture_wrap(id),
            fixed: editor.picture_anchor(id).is_some_and(|a| a.reference == model::AnchorReference::Page),
            size_mm: editor.picture_size(id).unwrap_or((0.0, 0.0)), alignment, in_body: editor.picture_section(id).is_some(),
        })
    }

    /// The button's rectangle on screen, for a menu or gallery below it.
    fn screen_rect(&self, (x, y, w, h): (i32, i32, i32, i32)) -> wxdragon::geometry::Rect {
        let origin = crate::gtk_native::to_screen(&self.panel,wxdragon::geometry::Point::new(x, y));
        wxdragon::geometry::Rect::new(origin.x, origin.y, w, h)
    }

    fn activate(&self, cmd: Cmd, rect: (i32, i32, i32, i32)) {
        let Some(ribbon) = self.ribbon.upgrade() else { return; };
        let Some(facts) = self.facts() else { if cmd == Cmd::Close { ribbon.run(Action::ClosePictureTools); } return; };
        let id = facts.id.clone();
        let result = match cmd {
            Cmd::CropShape => { super::picture_shapes::show(&ribbon); Ok(()) }
            Cmd::Wrap(wrap) => picture_layout::set_wrap(&ribbon, &id, wrap),
            Cmd::Place => { picture_layout::show_place_gallery(&ribbon, self.screen_rect(rect)); Ok(()) }
            Cmd::MoveWithText => picture_layout::set_fixed(&ribbon, &id, false),
            Cmd::FixOnPage => picture_layout::set_fixed(&ribbon, &id, true),
            Cmd::MoreOptions => layout_dialog::show(&ribbon, &id),
            Cmd::Width(sign) | Cmd::Height(sign) => {
                let step = f32::from(sign) * STEP_MM;
                let (w, h) = if matches!(cmd, Cmd::Width(_)) { stepped(facts.size_mm, step, 0.0, self.lock_ratio.get()) } else { stepped(facts.size_mm, 0.0, step, self.lock_ratio.get()) };
                ribbon.edit(|e| e.resize_picture(&id, w, h))
            }
            Cmd::LockRatio => { self.lock_ratio.set(!self.lock_ratio.get()); Ok(()) }
            Cmd::Align(at) => { ribbon.run(Action::PictureAlign(at)); Ok(()) }
            Cmd::Rotate => { self.show_rotate_menu(rect); Ok(()) }
            Cmd::Delete => { ribbon.run(Action::PictureDelete); Ok(()) }
            Cmd::Close => { ribbon.run(Action::ClosePictureTools); Ok(()) }
            Cmd::Label | Cmd::Value => Ok(()),
        };
        if let Err(error) = result { ribbon.state.borrow_mut().action_status = Some(error); }
        self.panel.refresh(false, None);
    }

    fn rotate(&self, degrees: f32) {
        let (Some(ribbon), Some(facts)) = (self.ribbon.upgrade(), self.facts()) else { return; };
        if let Err(error) = ribbon.edit(|e| e.rotate_picture(&facts.id, degrees)) { ribbon.state.borrow_mut().action_status = Some(error); }
    }

    fn show_rotate_menu(&self, (x, y, _, h): (i32, i32, i32, i32)) {
        let mut menu = Menu::builder()
            .append_item(MENU_FIRST, &crate::i18n::text(&("Rotate Right 90°")), &crate::i18n::text(&("")))
            .append_item(MENU_FIRST + 1, &crate::i18n::text(&("Rotate Left 90°")), &crate::i18n::text(&("")))
            .append_item(MENU_FIRST + 2, &crate::i18n::text(&("Rotate 180°")), &crate::i18n::text(&("")))
            .build();
        self.panel.set_tooltip(&crate::i18n::text(&("")));
        let anchor = crate::gtk_native::to_screen(&self.panel,wxdragon::geometry::Point::new(x, y + h));
        self.panel.popup_menu(&mut menu, Some(anchor));
    }

    fn hit(&self, x: f32, y: f32) -> Option<Button> {
        self.layout(&self.facts()).buttons.into_iter().find(|b| {
            let (bx, by, bw, bh) = b.rect;
            x >= bx as f32 && x < (bx + bw) as f32 && y >= by as f32 && y < (by + bh) as f32
        })
    }

    /// Measured with the panel's font, set once at install (see table_bar.rs).
    fn text_width(&self, text: &str) -> i32 { crate::i18n::text(text).lines().map(|l| self.panel.get_text_extent(l).width).max().unwrap_or(0) }

    fn layout(&self, facts: &Option<Facts>) -> Layout {
        let full = self.layout_with(facts, false);
        let width = self.panel.get_client_size().width;
        if full.groups.last().is_some_and(|g| g.1 > width) { self.layout_with(facts, true) } else { full }
    }

    fn layout_with(&self, facts: &Option<Facts>, compact: bool) -> Layout {
        let d = |v: i32| self.panel.from_dip_int(v);
        let body = self.panel.get_client_size().height - d(CAPTION);
        let mut buttons = Vec::new();
        let mut groups = Vec::new();
        let mut x = d(4);
        let small_style = if compact { Style::IconOnly } else { Style::Small };
        let large = |buttons: &mut Vec<Button>, x: &mut i32, cmd: Cmd, glyph: Glyph, label: &str, tip: &'static str| {
            let w = self.text_width(label).max(d(34)) + d(14);
            buttons.push(Button { cmd, glyph: Some(glyph), label: label.into(), tip, style: Style::Large, rect: (*x, d(3), w, body - d(5)) });
            *x += w + d(1);
        };
        let group = |groups: &mut Vec<(i32, i32, &'static str)>, x: &mut i32, start: i32, caption: &'static str| {
            let end = (*x + d(GROUP_PAD)).max(start + self.text_width(caption) + d(16));
            groups.push((start, end, caption));
            *x = end + d(1 + GROUP_PAD);
        };
        // A column of small buttons, centred in the body.
        let column = |buttons: &mut Vec<Button>, x: &mut i32, items: &[(Cmd, Glyph, &str, &'static str)]| {
            let width = items.iter().map(|i| if compact { d(26) } else { self.text_width(i.2) + d(34) }).max().unwrap_or(0);
            let top = (body - items.len() as i32 * d(SMALL_H) - (items.len() as i32 - 1) * d(4)) / 2;
            for (row, (cmd, glyph, label, tip)) in items.iter().enumerate() {
                buttons.push(Button { cmd: *cmd, glyph: Some(*glyph), label: (*label).into(), tip, style: small_style, rect: (*x, top + row as i32 * d(SMALL_H + 4), width, d(SMALL_H)) });
            }
            *x += width + d(2);
        };

        let start = x; x += d(GROUP_PAD);
        large(&mut buttons, &mut x, Cmd::CropShape, Glyph::Yaru("shapes"), "Crop to Shape ▾", "Crop to Shape");
        group(&mut groups, &mut x, start, "");

        // Wrapping styles: one large button each, as Word's gallery shows them.
        let start = x; x += d(GROUP_PAD);
        for (wrap, name, tip) in WRAPS {
            let label = match wrap {
                ImageWrap::Inline => "In Line\nwith Text", ImageWrap::TopAndBottom => "Top and\nBottom",
                ImageWrap::BehindText => "Behind\nText", ImageWrap::InFrontOfText => "In Front\nof Text", _ => name,
            };
            large(&mut buttons, &mut x, Cmd::Wrap(wrap), Glyph::Yaru(wrap_icon(wrap)), label, tip);
        }
        group(&mut groups, &mut x, start, "Wrap Text");

        let start = x; x += d(GROUP_PAD);
        large(&mut buttons, &mut x, Cmd::Place, Glyph::Yaru("position"), "Position", "Put the picture in line with the text or at a place on the page");
        column(&mut buttons, &mut x, &[
            (Cmd::MoveWithText, Glyph::Yaru("anchor-move"), "Move with Text", "The picture moves with its paragraph"),
            (Cmd::FixOnPage, Glyph::Yaru("anchor-page"), "Fix Position on Page", "The picture stays where it is on the page"),
            (Cmd::MoreOptions, Glyph::Yaru("layout-more"), "More Options…", "Position, wrapping and size in detail"),
        ]);
        group(&mut groups, &mut x, start, "Position");

        // Size: labelled steppers, as the Table tools' Cell Size.
        let start = x; x += d(GROUP_PAD);
        let value = |mm: f32| if facts.is_some() { format!("{mm:.1} mm") } else { "—".into() };
        let size = facts.as_ref().map_or((0.0, 0.0), |f| f.size_mm);
        let value_w = self.text_width("000.0 mm") + d(10);
        let label_w = self.text_width("Height").max(self.text_width("Width")) + d(8);
        let top = (body - 3 * d(SMALL_H)) / 2;
        for (row, label, text, minus, plus, tips) in [
            (0, "Width", value(size.0), Cmd::Width(-1), Cmd::Width(1), ("Narrower (1 mm)", "Wider (1 mm)")),
            (1, "Height", value(size.1), Cmd::Height(-1), Cmd::Height(1), ("Lower (1 mm)", "Taller (1 mm)"))] {
            let y = top + row * d(SMALL_H);
            buttons.push(Button { cmd: Cmd::Label, glyph: None, label: label.into(), tip: "", style: Style::Caption, rect: (x, y, label_w, d(SMALL_H)) });
            let vx = x + label_w;
            buttons.push(Button { cmd: minus, glyph: Some(Glyph::Table(Icon::Minus)), label: String::new(), tip: tips.0, style: Style::IconOnly, rect: (vx, y, d(20), d(SMALL_H)) });
            buttons.push(Button { cmd: Cmd::Value, glyph: None, label: text, tip: "", style: Style::Value, rect: (vx + d(21), y, value_w, d(SMALL_H)) });
            buttons.push(Button { cmd: plus, glyph: Some(Glyph::Table(Icon::Plus)), label: String::new(), tip: tips.1, style: Style::IconOnly, rect: (vx + d(22) + value_w, y, d(20), d(SMALL_H)) });
        }
        let lock_label = "Lock aspect ratio";
        let lock_w = if compact { d(26) } else { self.text_width(lock_label) + d(34) };
        buttons.push(Button { cmd: Cmd::LockRatio, glyph: Some(Glyph::Yaru("lock")), label: lock_label.into(), tip: "Keep the proportions when one side changes",
            style: small_style, rect: (x, top + 2 * d(SMALL_H) + d(2), lock_w, d(SMALL_H)) });
        x += (label_w + d(42) + value_w).max(lock_w) + d(2);
        group(&mut groups, &mut x, start, "Size");

        // Arrange: alignment of a picture in line, rotation and deletion.
        let start = x; x += d(GROUP_PAD);
        let top = (body - 2 * d(SMALL_H) - d(4)) / 2;
        for (i, tip) in ["Align the picture left", "Centre the picture", "Align the picture right"].into_iter().enumerate() {
            buttons.push(Button { cmd: Cmd::Align(i as u8), glyph: Some(Glyph::Yaru(["align-object-left", "align-object-center", "align-object-right"][i])), label: String::new(), tip, style: Style::IconOnly,
                rect: (x + i as i32 * d(28), top, d(26), d(SMALL_H) + d(2)) });
        }
        let rotate_label = "Rotate ▾";
        let rotate_w = if compact { d(26) } else { self.text_width(rotate_label) + d(34) };
        buttons.push(Button { cmd: Cmd::Rotate, glyph: Some(Glyph::Yaru("rotate")), label: rotate_label.into(), tip: "Turn the picture by 90° or 180°",
            style: small_style, rect: (x, top + d(SMALL_H) + d(6), rotate_w, d(SMALL_H)) });
        x += rotate_w.max(3 * d(28)) + d(4);
        large(&mut buttons, &mut x, Cmd::Delete, Glyph::Yaru("trash"), "Delete", "Delete the picture (Delete)");
        group(&mut groups, &mut x, start, "Arrange");

        let start = x; x += d(GROUP_PAD);
        large(&mut buttons, &mut x, Cmd::Close, Glyph::Table(Icon::Close), "Close\nPicture Tools", "Close the Picture tools (Esc)");
        group(&mut groups, &mut x, start, "");
        Layout { buttons, groups }
    }

    fn paint(&self) {
        let dc = AutoBufferedPaintDC::new(&self.panel);
        let size = self.panel.get_client_size();
        let (w, h) = (size.width.max(1), size.height.max(1));
        let scale = scale_of(&self.panel);
        let px = scale.max(1.0).round();
        let facts = self.facts();
        let layout = self.layout(&facts);
        let Some((mut pixmap, canvas)) = crate::crisp::Canvas::new(&self.panel, w as i32, h as i32) else { return; };
        pixmap.fill(tiny_skia::Color::WHITE);
        let caption_top = (h - self.panel.from_dip_int(CAPTION)) as f32;
        fill_round(&mut pixmap, (0.0, caption_top, w as f32, h as f32 - caption_top), 0.0, (243, 246, 251));
        line(&mut pixmap, &[(0.0, h as f32 - 0.5 * px), (w as f32, h as f32 - 0.5 * px)], (214, 221, 232), px);
        for &(_, end, _) in &layout.groups {
            let x = end as f32 + 0.5 * px;
            line(&mut pixmap, &[(x, 6.0 * scale), (x, h as f32 - 4.0 * scale)], (222, 228, 237), px);
        }
        for b in &layout.buttons {
            let on_enabled = enabled(b.cmd, &facts);
            let (x, y, bw, bh) = (b.rect.0 as f32, b.rect.1 as f32, b.rect.2 as f32, b.rect.3 as f32);
            let hot = on_enabled && self.hover.get() == Some(b.cmd);
            let pressed = on_enabled && self.pressed.get() == Some(b.cmd);
            // A command that does not apply now shows no state either.
            let on = on_enabled && toggled(b.cmd, &facts, self.lock_ratio.get());
            match b.style {
                Style::Caption => continue,
                Style::Value => {
                    fill_round(&mut pixmap, (x + 0.5 * px, y + 2.0 * scale, bw - px, bh - 4.0 * scale), 3.0 * scale, (247, 249, 252));
                    stroke_round(&mut pixmap, (x + 0.5 * px, y + 2.0 * scale, bw - px, bh - 4.0 * scale), 3.0 * scale, (214, 221, 232), px);
                }
                _ if pressed || hot || on => {
                    let fill = if pressed { (206, 222, 250) } else if on { (221, 233, 252) } else { (234, 241, 253) };
                    fill_round(&mut pixmap, (x + px, y + px, bw - 2.0 * px, bh - 2.0 * px), 4.0 * scale, fill);
                    if on { stroke_round(&mut pixmap, (x + 1.5 * px, y + 1.5 * px, bw - 3.0 * px, bh - 3.0 * px), 4.0 * scale, (120, 160, 230), px); }
                }
                _ => {}
            }
            let Some(glyph) = b.glyph else { continue; };
            let (size, ix, iy) = match b.style {
                Style::Large => (32.0 * scale, x + (bw - 32.0 * scale) * 0.5, y + large_top(bh, scale)),
                Style::IconOnly => (16.0 * scale, x + (bw - 16.0 * scale) * 0.5, y + (bh - 16.0 * scale) * 0.5),
                _ => (16.0 * scale, x + 5.0 * scale, y + (bh - 16.0 * scale) * 0.5),
            };
            draw_glyph(&mut pixmap, glyph, ix, iy, size, on_enabled);
        }
        canvas.present(&dc, &pixmap, 0, 0);

        dc.set_background_mode(wxdragon::dc::BackgroundMode::Transparent);
        if let Some(font) = self.font.as_ref() { dc.set_font(font); }
        let d = |v: i32| self.panel.from_dip_int(v);
        for b in &layout.buttons {
            if b.label.is_empty() || b.style == Style::IconOnly { continue; }
            let active = enabled(b.cmd, &facts) || matches!(b.style, Style::Value | Style::Caption);
            dc.set_text_foreground(if active { Colour::rgb(36, 48, 66) } else { Colour::rgb(172, 180, 192) });
            let (x, y, bw, bh) = b.rect;
            match b.style {
                Style::Large => {
                    let mut lines: Vec<String> = crate::i18n::text(&b.label).lines().map(str::to_string).collect();
                    if b.cmd == Cmd::Place { if let Some(last) = lines.last_mut() { last.push_str(" ▾"); } }
                    let lh = dc.get_text_extent(&crate::i18n::text(&("Ag"))).1 - d(1);
                    let mut ty = y + large_top(bh as f32, scale) as i32 + d(38);
                    for l in lines { let lw = dc.get_text_extent(&l).0; dc.draw_text(&l, x + (bw - lw) / 2, ty); ty += lh; }
                }
                Style::Caption => {
                    dc.set_text_foreground(Colour::rgb(92, 108, 132));
                    let lh = dc.get_text_extent(&crate::i18n::text(&(&b.label))).1;
                    dc.draw_text(&crate::i18n::text(&(&b.label)), x, y + (bh - lh) / 2);
                }
                Style::Value => {
                    let (lw, lh) = dc.get_text_extent(&crate::i18n::text(&(&b.label)));
                    dc.draw_text(&crate::i18n::text(&(&b.label)), x + (bw - lw) / 2, y + (bh - lh) / 2);
                }
                _ => {
                    let lh = dc.get_text_extent(&crate::i18n::text(&(&b.label))).1;
                    dc.draw_text(&crate::i18n::text(&(&b.label)), x + d(26), y + (bh - lh) / 2);
                }
            }
        }
        dc.set_text_foreground(Colour::rgb(92, 108, 132));
        let cap_y = h - d(CAPTION);
        for &(start, end, caption) in &layout.groups {
            let (cw, ch) = dc.get_text_extent(&crate::i18n::text(&(caption)));
            dc.draw_text(&crate::i18n::text(&(caption)), start + (end - start - cw) / 2, cap_y + (d(CAPTION) - ch) / 2);
        }
    }
}

/// The Yaru icon of each wrapping style.
fn wrap_icon(wrap: ImageWrap) -> &'static str {
    match wrap {
        ImageWrap::Inline => "wrap-inline",
        ImageWrap::Square => "wrap-square",
        ImageWrap::Tight => "wrap-tight",
        ImageWrap::TopAndBottom => "wrap-top-bottom",
        ImageWrap::BehindText => "wrap-behind",
        ImageWrap::InFrontOfText => "wrap-front",
    }
}

fn draw_glyph(pix: &mut Pixmap, glyph: Glyph, x: f32, y: f32, size: f32, enabled: bool) {
    match glyph {
        Glyph::Table(icon) => draw_icon(pix, icon, x, y, size, enabled),
        Glyph::Yaru(name) => {
            // Rasterized at the screen's resolution, as the shapes are drawn.
            let k = crate::crisp::scale();
            let side = (size * k).round().max(1.0) as u32;
            let Some(art) = art::raster(name, side, side, !enabled) else { return; };
            blend(pix, &art, side, (x * k).round() as i32, (y * k).round() as i32);
        }
    }
}

/// Lays straight-alpha RGBA artwork over the premultiplied pixmap.
fn blend(pix: &mut Pixmap, art: &[u8], side: u32, left: i32, top: i32) {
    let (width, height) = (pix.width() as i32, pix.height() as i32);
    let data = pix.data_mut();
    for row in 0..side as i32 {
        for column in 0..side as i32 {
            let (dx, dy) = (left + column, top + row);
            if dx < 0 || dy < 0 || dx >= width || dy >= height { continue; }
            let source = ((row * side as i32 + column) * 4) as usize;
            let alpha = art[source + 3] as u32;
            if alpha == 0 { continue; }
            let target = ((dy * width + dx) * 4) as usize;
            for channel in 0..3 {
                data[target + channel] = ((art[source + channel] as u32 * alpha + data[target + channel] as u32 * (255 - alpha) + 127) / 255) as u8;
            }
            data[target + 3] = (alpha + (data[target + 3] as u32 * (255 - alpha) + 127) / 255) as u8;
        }
    }
}

