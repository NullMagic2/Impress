//! Word-style in-ribbon style gallery: three style cards that page through
//! every paragraph style, beside a strip with row up, row down and More
//! (the expanded gallery).
use super::*;
use tiny_skia::{FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke};
use wxdragon::{
    dc::AutoBufferedPaintDC,
    event::window_events::WindowEvents,
    font::{Font, FontFamily, FontStyle, FontWeight},
    widgets::panel::Panel,
    window::BackgroundStyle,
    DeviceContext,
};

const VISIBLE: usize = 3;
const STRIP_DIP: i32 = 17;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hit { Card(usize), Up, Down, More }

pub(super) struct Gallery {
    panel: Panel,
    ribbon: std::rc::Weak<Ribbon>,
    /// Index of the first card shown; always a multiple of `VISIBLE`.
    first: Cell<usize>,
    hover: Cell<Option<Hit>>,
    pressed: Cell<Option<Hit>>,
    /// What was last painted, to skip repaints that would change nothing.
    shown: RefCell<Option<((Vec<gallery_popup::Card>, Option<model::StyleId>), bool)>>,
}

pub(super) fn install(ribbon: &Rc<Ribbon>) -> Rc<Gallery> {
    let panel = ribbon.ui.style_gallery;
    let gallery = Rc::new(Gallery { panel, ribbon: Rc::downgrade(ribbon), first: Cell::new(0), hover: Cell::new(None), pressed: Cell::new(None), shown: RefCell::new(None) });
    panel.set_background_style(BackgroundStyle::Paint);
    panel.on_erase_background(|_| {});
    let g = Rc::downgrade(&gallery);
    panel.on_paint(move |_| { if let Some(g) = g.upgrade() { g.paint(); } });
    panel.on_size(move |event| { panel.refresh(false, None); event.skip(true); });
    let g = Rc::downgrade(&gallery);
    panel.on_mouse_motion(move |event| {
        if let (Some(g), Some(p)) = (g.upgrade(), position(&event)) {
            let hit = g.hit(p.0, p.1);
            if g.hover.replace(hit) != hit { g.update_tooltip(hit); panel.refresh(false, None); }
        }
    });
    let g = Rc::downgrade(&gallery);
    panel.on_mouse_leave(move |_| { if let Some(g) = g.upgrade() { if g.hover.replace(None).is_some() { panel.refresh(false, None); } } });
    let g = Rc::downgrade(&gallery);
    panel.on_mouse_left_down(move |event| {
        if let (Some(g), Some(p)) = (g.upgrade(), position(&event)) { g.pressed.set(g.hit(p.0, p.1)); panel.refresh(false, None); }
    });
    let g = Rc::downgrade(&gallery);
    panel.on_mouse_left_up(move |event| {
        let (Some(g), Some(p)) = (g.upgrade(), position(&event)) else { return; };
        let pressed = g.pressed.take();
        let hit = g.hit(p.0, p.1);
        panel.refresh(false, None);
        if hit.is_some() && hit == pressed { g.activate(hit.expect("hit")); }
    });
    let g = Rc::downgrade(&gallery);
    panel.on_mouse_right_up(move |event| {
        let (Some(g),Some(p))=(g.upgrade(),position(&event)) else{return;};
        let Some(Hit::Card(index))=g.hit(p.0,p.1) else{return;};
        let Some(ribbon)=g.ribbon.upgrade() else{return;};
        let (cards,_)=gallery_popup::cards(&ribbon);
        if let Some(card)=cards.get(index) {
            ribbon.show_style_context(card.id.clone(),crate::gtk_native::to_screen(&panel,wxdragon::geometry::Point::new(p.0 as i32,p.1 as i32)));
        }
        event.skip(false);
    });
    let g = Rc::downgrade(&gallery);
    panel.on_mouse_wheel(move |event| {
        let Some(g) = g.upgrade() else { return; };
        let rotation = match &event {
            WindowEventData::MouseButton(m) => m.event.get_wheel_rotation(),
            WindowEventData::MouseMotion(m) => m.event.get_wheel_rotation(),
            _ => 0,
        };
        if rotation > 0 { g.scroll(false); } else if rotation < 0 { g.scroll(true); }
        event.skip(false);
    });
    gallery
}

impl Gallery {
    /// Repaints only when what the gallery shows changed: the styles, the
    /// current style or whether it is enabled. Typing changes none of them.
    pub(super) fn refresh(&self) {
        let Some(ribbon) = self.ribbon.upgrade() else { return; };
        let shown = (gallery_popup::cards(&ribbon), self.enabled(&ribbon));
        if self.shown.borrow().as_ref() != Some(&shown) { self.shown.replace(Some(shown)); self.panel.refresh(false, None); }
    }

    fn enabled(&self, ribbon: &Ribbon) -> bool {
        ribbon.mode.get() != ViewMode::Typst
    }

    fn count(&self) -> usize { self.ribbon.upgrade().map_or(0, |r| gallery_popup::cards(&r).0.len()) }

    /// Pages one row of cards, like Word's gallery arrows.
    fn scroll(&self, down: bool) {
        let count = self.count();
        let first = self.first.get();
        let next = if down { if first + VISIBLE < count { first + VISIBLE } else { first } } else { first.saturating_sub(VISIBLE) };
        if next != first { self.first.set(next); self.panel.refresh(false, None); }
    }

    fn activate(&self, hit: Hit) {
        let Some(ribbon) = self.ribbon.upgrade() else { return; };
        match hit {
            Hit::Up => self.scroll(false),
            Hit::Down => self.scroll(true),
            Hit::More => { if self.enabled(&ribbon) { ribbon.run(Action::StyleGalleryMore); } }
            Hit::Card(index) => {
                if !self.enabled(&ribbon) { return; }
                let (cards, _) = gallery_popup::cards(&ribbon);
                let Some(card) = cards.get(index) else { return; };
                if let Err(error) = ribbon.edit(|editor| editor.apply_named_style(&card.id, true)) {
                    ribbon.state.borrow_mut().action_status = Some(error);
                }
                self.panel.refresh(false, None);
            }
        }
    }

    fn update_tooltip(&self, hit: Option<Hit>) {
        let tip = match hit {
            Some(Hit::Up) => "Previous row of styles".to_string(),
            Some(Hit::Down) => "Next row of styles".to_string(),
            Some(Hit::More) => "More styles".to_string(),
            Some(Hit::Card(index)) => self.ribbon.upgrade()
                .and_then(|r| gallery_popup::cards(&r).0.get(index).map(|c| format!("{} paragraph style", c.name)))
                .unwrap_or_default(),
            None => String::new(),
        };
        self.panel.set_tooltip(&crate::i18n::text(&(&tip)));
    }

    fn geometry(&self) -> (i32, i32, i32, i32) {
        let size = self.panel.get_client_size();
        let strip = self.panel.from_dip_int(STRIP_DIP);
        (size.width, size.height, strip, (size.width - strip).max(VISIBLE as i32))
    }

    fn hit(&self, x: f32, y: f32) -> Option<Hit> {
        let (w, h, strip, cards_w) = self.geometry();
        if x < 0.0 || y < 0.0 || x >= w as f32 || y >= h as f32 { return None; }
        if x >= cards_w as f32 {
            let third = h as f32 / 3.0;
            return Some(if y < third { Hit::Up } else if y < 2.0 * third { Hit::Down } else { Hit::More });
        }
        let _ = strip;
        let slot = ((x / (cards_w as f32 / VISIBLE as f32)) as usize).min(VISIBLE - 1);
        let index = self.first.get() + slot;
        (index < self.count()).then_some(Hit::Card(index))
    }

    fn paint(&self) {
        let dc = AutoBufferedPaintDC::new(&self.panel);
        let Some(ribbon) = self.ribbon.upgrade() else { return; };
        let (w, h, strip, cards_w) = self.geometry();
        if w <= 2 || h <= 2 { return; }
        let scale = self.panel.from_dip_int(1000) as f32 / 1000.0;
        let (cards, current) = gallery_popup::cards(&ribbon);
        let enabled = self.enabled(&ribbon);
        let count = cards.len();
        if self.first.get() >= count.max(1) { self.first.set(count.saturating_sub(1) / VISIBLE * VISIBLE); }
        let first = self.first.get();
        // Drawn at the screen's resolution (crisp.rs).
        let Some((mut pixmap, canvas)) = crate::crisp::Canvas::new(&self.panel, w, h) else { return; };
        pixmap.fill(tiny_skia::Color::WHITE);
        let (wf, hf) = (w as f32, h as f32);
        let px = scale.max(1.0).round();
        let card_w = cards_w as f32 / VISIBLE as f32;
        let hover = if enabled { self.hover.get() } else { None };
        let pressed = self.pressed.get();

        // Card states.
        for slot in 0..VISIBLE {
            let index = first + slot;
            let Some(card) = cards.get(index) else { continue; };
            let x = slot as f32 * card_w;
            let rect = (x + px, px, card_w - 2.0 * px, hf - 2.0 * px);
            let selected = current.as_ref() == Some(&card.id);
            if hover == Some(Hit::Card(index)) || pressed == Some(Hit::Card(index)) {
                let fill = if pressed == Some(Hit::Card(index)) { (214, 227, 250) } else { (232, 239, 253) };
                fill_round(&mut pixmap, rect, 3.0 * scale, fill);
            }
            if selected {
                stroke_round(&mut pixmap, (rect.0 + 0.5 * px, rect.1 + 0.5 * px, rect.2 - px, rect.3 - px), 3.0 * scale, (58, 104, 194), px);
            }
        }
        // Strip: row up, row down, More.
        let sx = cards_w as f32;
        let third = hf / 3.0;
        let can_up = first > 0;
        let can_down = first + VISIBLE < count;
        for (row, hit, active) in [(0, Hit::Up, can_up), (1, Hit::Down, can_down), (2, Hit::More, enabled)] {
            let y = row as f32 * third;
            if active && (hover == Some(hit) || pressed == Some(hit)) {
                let fill = if pressed == Some(hit) { (214, 227, 250) } else { (232, 239, 253) };
                fill_round(&mut pixmap, (sx + px, y + px, strip as f32 - 2.0 * px, third - px), 2.0 * scale, fill);
            }
            let ink = if active { (68, 81, 102) } else { (190, 198, 210) };
            let cx = sx + strip as f32 * 0.5;
            let cy = y + third * 0.5;
            match hit {
                Hit::Up => chevron(&mut pixmap, cx, cy, 3.2 * scale, true, ink, px),
                Hit::Down => chevron(&mut pixmap, cx, cy, 3.2 * scale, false, ink, px),
                _ => {
                    chevron(&mut pixmap, cx, cy + 1.5 * scale, 3.2 * scale, false, ink, px);
                    line(&mut pixmap, &[(cx - 3.5 * scale, cy - 2.5 * scale), (cx + 3.5 * scale, cy - 2.5 * scale)], ink, px);
                }
            }
        }
        // Frame, with the strip and each row separated.
        stroke_round(&mut pixmap, (0.5 * px, 0.5 * px, wf - px, hf - px), 3.0 * scale, (211, 219, 231), px);
        line(&mut pixmap, &[(sx + 0.5 * px, 0.0), (sx + 0.5 * px, hf)], (221, 227, 236), px);
        for row in 1..3 {
            let y = (row as f32 * third).round() + 0.5 * px;
            line(&mut pixmap, &[(sx + 3.0 * scale, y), (wf - 3.0 * scale, y)], (230, 234, 241), px);
        }
        canvas.present(&dc, &pixmap, 0, 0);

        // Card text: the preview in the style's own font, the name below.
        dc.set_background_mode(wxdragon::dc::BackgroundMode::Transparent);
        for slot in 0..VISIBLE {
            let Some(card) = cards.get(first + slot) else { continue; };
            let x = (slot as f32 * card_w) as i32;
            let cw = card_w as i32;
            // Shrink the sample to fit the card before truncating it.
            let limit = cw - self.panel.from_dip_int(8);
            let mut sample = String::from("AaBbCc");
            let mut points = card.points.clamp(9, 13);
            loop {
                if let Some(font) = Font::new_with_details(points, FontFamily::Default as i32,
                    if card.italic { FontStyle::Italic } else { FontStyle::Normal } as i32,
                    if card.bold { FontWeight::Bold } else { FontWeight::Normal } as i32, false, &card.face) { dc.set_font(&font); }
                if dc.get_text_extent(&crate::i18n::text(&(&sample))).0 <= limit || points <= 8 { break; }
                points -= 1;
            }
            while dc.get_text_extent(&crate::i18n::text(&(&sample))).0 > limit && sample.len() > 2 { sample.pop(); }
            dc.set_text_foreground(if enabled { gallery_popup::sample_colour(card) } else { Colour::rgb(170, 178, 190) });
            let (sw, sh) = dc.get_text_extent(&crate::i18n::text(&(&sample)));
            let _ = sh;
            dc.draw_text(&crate::i18n::text(&(&sample)), x + (cw - sw) / 2, self.panel.from_dip_int(12));
            if let Some(font) = Font::new_with_details(9, FontFamily::Default as i32, FontStyle::Normal as i32,
                FontWeight::Normal as i32, false, "Segoe UI") { dc.set_font(&font); }
            dc.set_text_foreground(if enabled { Colour::rgb(36, 48, 66) } else { Colour::rgb(170, 178, 190) });
            let mut name = card.name.clone();
            let limit = cw - self.panel.from_dip_int(8);
            if dc.get_text_extent(&crate::i18n::text(&(&name))).0 > limit {
                while dc.get_text_extent(&crate::i18n::text(&(&format!("{name}…")))).0 > limit && name.chars().count() > 1 { name.pop(); }
                name.push('…');
            }
            let (nw, nh) = dc.get_text_extent(&crate::i18n::text(&(&name)));
            dc.draw_text(&crate::i18n::text(&(&name)), x + (cw - nw) / 2, h - nh - self.panel.from_dip_int(6));
        }
    }
}

fn position(event: &WindowEventData) -> Option<(f32, f32)> {
    let p = match event {
        WindowEventData::MouseButton(m) => m.event.get_position(),
        WindowEventData::MouseMotion(m) => m.event.get_position(),
        _ => None,
    }?;
    Some((p.x as f32, p.y as f32))
}


fn paint(rgb: (u8, u8, u8)) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color_rgba8(rgb.0, rgb.1, rgb.2, 255);
    paint.anti_alias = true;
    paint
}

fn round_rect(r: (f32, f32, f32, f32), radius: f32) -> Option<tiny_skia::Path> {
    let (x, y, w, h) = r;
    let radius = radius.min(w * 0.5).min(h * 0.5).max(0.0);
    let c = radius * 0.4477;
    let (x1, y1) = (x + w, y + h);
    let mut p = PathBuilder::new();
    p.move_to(x + radius, y);
    p.line_to(x1 - radius, y); p.cubic_to(x1 - c, y, x1, y + c, x1, y + radius);
    p.line_to(x1, y1 - radius); p.cubic_to(x1, y1 - c, x1 - c, y1, x1 - radius, y1);
    p.line_to(x + radius, y1); p.cubic_to(x + c, y1, x, y1 - c, x, y1 - radius);
    p.line_to(x, y + radius); p.cubic_to(x, y + c, x + c, y, x + radius, y);
    p.close();
    p.finish()
}

fn fill_round(pixmap: &mut Pixmap, r: (f32, f32, f32, f32), radius: f32, rgb: (u8, u8, u8)) {
    if let Some(path) = round_rect(r, radius) { pixmap.fill_path(&path, &paint(rgb), FillRule::Winding, crate::crisp::transform(), None); }
}

fn stroke_round(pixmap: &mut Pixmap, r: (f32, f32, f32, f32), radius: f32, rgb: (u8, u8, u8), width: f32) {
    if let Some(path) = round_rect(r, radius) {
        pixmap.stroke_path(&path, &paint(rgb), &Stroke { width, ..Stroke::default() }, crate::crisp::transform(), None);
    }
}

fn line(pixmap: &mut Pixmap, points: &[(f32, f32)], rgb: (u8, u8, u8), width: f32) {
    let mut p = PathBuilder::new();
    for (i, (x, y)) in points.iter().enumerate() { if i == 0 { p.move_to(*x, *y) } else { p.line_to(*x, *y) } }
    if let Some(path) = p.finish() {
        let stroke = Stroke { width, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Stroke::default() };
        pixmap.stroke_path(&path, &paint(rgb), &stroke, crate::crisp::transform(), None);
    }
}

fn chevron(pixmap: &mut Pixmap, cx: f32, cy: f32, half: f32, up: bool, rgb: (u8, u8, u8), width: f32) {
    let d = if up { -half * 0.55 } else { half * 0.55 };
    line(pixmap, &[(cx - half, cy - d), (cx, cy + d), (cx + half, cy - d)], rgb, width);
}
