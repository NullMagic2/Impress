//! Ribbon tabs, painted by the strip itself so they look the same on every
//! platform: plain captions on the title colour, and one white active tab
//! that flows into the white ribbon body with no seam or native frame.
use super::*;
use tiny_skia::{FillRule, Paint, PathBuilder, Stroke};
use wxdragon::{dc::AutoBufferedPaintDC, event::window_events::WindowEvents, window::BackgroundStyle, DeviceContext};

const BORDER: (u8, u8, u8) = (218, 224, 233);
const TITLE: (u8, u8, u8) = (232, 236, 242);
const PAD_DIP: i32 = 20;
const LEFT_DIP: i32 = 8;
const TOP_DIP: i32 = 6;

pub(super) struct TabStrip {
    ribbon: std::rc::Weak<Ribbon>,
    tabs: Vec<(&'static str, usize)>,
    hover: Cell<Option<usize>>,
    /// Tab rectangles and label sizes, measured once per scale: painting and
    /// hit-testing on every mouse move would otherwise measure every label.
    measured: std::cell::RefCell<Option<Measured>>,
    /// The strip's drawn background and tab shapes, and what they show: drawn
    /// again only when the size, scale, active tab or hovered tab changes.
    background: std::cell::RefCell<Option<(BackgroundKey, wxdragon::Bitmap, f32)>>,
}

type BackgroundKey = (i32, i32, i32, i32, usize, Option<usize>);

struct Measured { scale: i32, tabs: Vec<(i32, i32)>, labels: Vec<(i32, i32)> }

pub(super) fn install(ribbon: &Rc<Ribbon>) -> Rc<TabStrip> {
    let panel = ribbon.ui.ribbon_tab_strip;
    let tabs = ribbon.buttons.iter().filter_map(|spec| match spec.action {
        Action::SelectRibbonTab(index) => Some((spec.label, index as usize)),
        _ => None,
    }).collect();
    let strip = Rc::new(TabStrip { ribbon: Rc::downgrade(ribbon), tabs, hover: Cell::new(None), measured: Default::default(), background: Default::default() });
    panel.set_background_style(BackgroundStyle::Paint);
    panel.on_erase_background(|_| {});
    let s = Rc::downgrade(&strip);
    panel.on_paint(move |_| { if let Some(s) = s.upgrade() { s.paint(); } });
    panel.on_size(move |event| { panel.refresh(false, None); event.skip(true); });
    let s = Rc::downgrade(&strip);
    panel.on_mouse_motion(move |event| {
        let Some(s) = s.upgrade() else { return; };
        let hit = position(&event).and_then(|(x, y)| s.hit(x, y));
        if s.hover.replace(hit) != hit {
            panel.set_tooltip(&crate::i18n::text(&(&hit.map(|i| format!("{} ribbon", s.tabs[i].0)).unwrap_or_default())));
            panel.refresh(false, None);
        }
    });
    let s = Rc::downgrade(&strip);
    panel.on_mouse_leave(move |_| { if let Some(s) = s.upgrade() { if s.hover.replace(None).is_some() { panel.refresh(false, None); } } });
    let s = Rc::downgrade(&strip);
    panel.on_mouse_left_down(move |event| {
        let Some(s) = s.upgrade() else { return; };
        let Some(hit) = position(&event).and_then(|(x, y)| s.hit(x, y)) else { return; };
        let index = s.tabs[hit].1;
        if let Some(ribbon) = s.ribbon.upgrade() { ribbon.run(Action::SelectRibbonTab(index as u8)); }
        panel.refresh(false, None);
    });
    strip
}

impl TabStrip {
    pub(super) fn language_changed(&self) {
        self.measured.borrow_mut().take();
        self.background.borrow_mut().take();
        self.refresh();
    }

    pub(super) fn refresh(&self) {
        if let Some(r) = self.ribbon.upgrade() { r.ui.ribbon_tab_strip.refresh(false, None); }
    }

    /// Tab rectangles (x, width) and label sizes in strip pixels, measured
    /// again only when the scale changes.
    fn measure(&self, panel: &impl WxWidget) -> std::cell::Ref<'_, Measured> {
        let scale = panel.from_dip_int(1000);
        if self.measured.borrow().as_ref().is_none_or(|m| m.scale != scale) {
            let mut x = panel.from_dip_int(LEFT_DIP);
            let labels: Vec<(i32, i32)> = self.tabs.iter().map(|(label, _)| {
                let size = panel.get_text_extent(&crate::i18n::text(&(label)));
                (size.width, size.height)
            }).collect();
            let tabs = labels.iter().map(|(w, _)| {
                let w = w + 2 * panel.from_dip_int(PAD_DIP);
                let tab = (x, w);
                x += w;
                tab
            }).collect();
            self.measured.replace(Some(Measured { scale, tabs, labels }));
        }
        std::cell::Ref::map(self.measured.borrow(), |m| m.as_ref().expect("measured above"))
    }

    /// Tab rectangles (x, width) in strip pixels.
    fn layout(&self, panel: &impl WxWidget) -> Vec<(i32, i32)> { self.measure(panel).tabs.clone() }

    fn hit(&self, x: f32, y: f32) -> Option<usize> {
        let ribbon = self.ribbon.upgrade()?;
        let panel = ribbon.ui.ribbon_tab_strip;
        if y < panel.from_dip_int(TOP_DIP) as f32 { return None; }
        self.layout(&panel).iter().position(|(tx, tw)| x >= *tx as f32 && x < (tx + tw) as f32)
    }

    fn paint(&self) {
        let Some(ribbon) = self.ribbon.upgrade() else { return; };
        let panel = ribbon.ui.ribbon_tab_strip;
        let dc = AutoBufferedPaintDC::new(&panel);
        let size = panel.get_client_size();
        let (w, h) = (size.width.max(1), size.height.max(1));
        let scale = panel.from_dip_int(1000) as f32 / 1000.0;
        let tabs = self.layout(&panel);
        let active = ribbon.tab_index.get();
        let screen = crate::display_scale::window_scale(&panel);
        let key = (w, h, panel.from_dip_int(1000), (screen * 1000.0).round() as i32, active, self.hover.get());
        if self.background.borrow().as_ref().is_none_or(|(k, ..)| *k != key) {
            if let Some((bitmap, drawn)) = self.draw_background(&panel, &tabs, active, w, h, scale) {
                self.background.replace(Some((key, bitmap, drawn)));
            }
        }
        if let Some((_, bitmap, drawn)) = self.background.borrow().as_ref() { crate::crisp::draw(&dc, bitmap, *drawn, 0, 0); }
        dc.set_background_mode(wxdragon::dc::BackgroundMode::Transparent);
        if let Some(font) = panel.get_font() { dc.set_font(&font); }
        for (slot, (tx, tw)) in tabs.iter().enumerate() {
            let (label, index) = self.tabs[slot];
            dc.set_text_foreground(if index == active { Colour::rgb(28, 44, 70) } else { Colour::rgb(52, 64, 82) });
            let (lw, lh) = self.measure(&panel).labels[slot];
            let y = panel.from_dip_int(TOP_DIP) + (h - panel.from_dip_int(TOP_DIP) - lh) / 2;
            dc.draw_text(&crate::i18n::text(&(label)), tx + (tw - lw) / 2, y);
        }
    }

    /// The strip's background with the active and hovered tab shapes.
    /// Drawn at the screen's resolution: the bitmap and its scale.
    fn draw_background(&self, panel: &impl WxWidget, tabs: &[(i32, i32)], active: usize, w: i32, h: i32, scale: f32) -> Option<(wxdragon::Bitmap, f32)> {
        let (mut pixmap, canvas) = crate::crisp::Canvas::new(panel, w, h)?;
        pixmap.fill(tiny_skia::Color::from_rgba8(TITLE.0, TITLE.1, TITLE.2, 255));
        let top = panel.from_dip_int(TOP_DIP) as f32;
        let radius = 6.0 * scale;
        let px = scale.max(1.0).round();
        for (slot, (tx, tw)) in tabs.iter().enumerate() {
            let (x0, x1, hf) = (*tx as f32, (*tx + *tw) as f32, h as f32);
            let index = self.tabs[slot].1;
            if index == active {
                // Rounded top, straight sides, and outward flares at the
                // bottom so the tab and the body read as one surface.
                let mut p = PathBuilder::new();
                p.move_to(x0 - radius, hf);
                p.quad_to(x0, hf, x0, hf - radius);
                p.line_to(x0, top + radius);
                p.quad_to(x0, top, x0 + radius, top);
                p.line_to(x1 - radius, top);
                p.quad_to(x1, top, x1, top + radius);
                p.line_to(x1, hf - radius);
                p.quad_to(x1, hf, x1 + radius, hf);
                let outline = p.clone().finish();
                p.close();
                if let Some(path) = p.finish() { pixmap.fill_path(&path, &paint((255, 255, 255)), FillRule::Winding, crate::crisp::transform(), None); }
                if let Some(path) = outline {
                    pixmap.stroke_path(&path, &paint(BORDER), &Stroke { width: px, ..Stroke::default() }, crate::crisp::transform(), None);
                }
            } else if self.hover.get() == Some(slot) {
                let mut p = PathBuilder::new();
                p.move_to(x0, hf - 2.0 * px);
                p.line_to(x0, top + radius);
                p.quad_to(x0, top, x0 + radius, top);
                p.line_to(x1 - radius, top);
                p.quad_to(x1, top, x1, top + radius);
                p.line_to(x1, hf - 2.0 * px);
                p.close();
                if let Some(path) = p.finish() { pixmap.fill_path(&path, &paint((222, 228, 237)), FillRule::Winding, crate::crisp::transform(), None); }
            }
        }
        crate::crisp::bitmap(&pixmap).map(|bitmap| (bitmap, canvas.scale()))
    }
}

fn paint(rgb: (u8, u8, u8)) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color_rgba8(rgb.0, rgb.1, rgb.2, 255);
    paint.anti_alias = true;
    paint
}

fn position(event: &WindowEventData) -> Option<(f32, f32)> {
    let p = match event {
        WindowEventData::MouseButton(m) => m.event.get_position(),
        WindowEventData::MouseMotion(m) => m.event.get_position(),
        _ => None,
    }?;
    Some((p.x as f32, p.y as f32))
}
