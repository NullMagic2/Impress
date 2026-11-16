//! One continuous, lightly raised ribbon frame with shared group seams.
use wxdragon::{
    BrushStyle, DeviceContext, PenStyle,
    color::Colour,
    dc::AutoBufferedPaintDC,
    event::window_events::WindowEvents,
    font::{Font, FontFamily, FontStyle, FontWeight},
    widgets::panel::Panel,
    window::{BackgroundStyle, WxWidget},
};

pub(super) fn install(panel: Panel, first: bool, last: bool) {
    panel.set_background_style(BackgroundStyle::Paint);
    panel.on_paint(move |_| {
        let dc = AutoBufferedPaintDC::new(&panel);
        let size = panel.get_client_size();
        dc.set_background(Colour::rgb(255, 255, 255));
        dc.clear();
        if size.width < 8 || size.height < 8 { return; }

        let unit = panel.from_dip_int(1).max(1);
        let radius = panel.from_dip_int(6).max(3);
        let (right, bottom) = (size.width - unit, size.height - unit);
        dc.set_brush(Colour::rgb(255, 255, 255), BrushStyle::Solid);
        dc.set_pen(Colour::rgb(211, 220, 233), unit, PenStyle::Solid);
        // The hidden side of a rounded rectangle lies beyond this panel's
        // clipping region. Only the outside corners of the ribbon stay round.
        // The frame's top edge lies above the panel: the ribbon body meets the
        // active tab directly, as one white surface.
        let top = -2 * radius;
        if first && last {
            dc.draw_rounded_rectangle(0, top, right, bottom - top, radius as f64);
        } else if first {
            dc.draw_rounded_rectangle(0, top, size.width + radius, bottom - top, radius as f64);
        } else if last {
            dc.draw_rounded_rectangle(-radius, top, size.width + radius - unit, bottom - top, radius as f64);
        }

        // Each group owns only its right seam, so adjacent groups meet on a
        // single pixel instead of showing two borders and an empty gutter.
        // No top rule: the active tab above is the same white and flows
        // straight into the ribbon body, as one uniform surface.
        let top_start = if first { radius } else { 0 };
        let top_end = if last { right - radius } else { right };
        dc.set_pen(Colour::rgb(194, 207, 224), unit, PenStyle::Solid);
        dc.draw_line(top_start, bottom, top_end, bottom);
        if !last {
            dc.set_pen(Colour::rgb(207, 218, 232), unit, PenStyle::Solid);
            dc.draw_line(right, 0, right, bottom);
        }
    });
    panel.on_size(move |event| { panel.refresh(false, None); event.skip(true); });
}

/// A short tinted label strip, shared by every ribbon group.
/// Width of the dialog launcher at a caption's right end.
const LAUNCHER_DIP: i32 = 20;

/// True when `x` (caption pixels) is on the caption's dialog launcher.
pub(super) fn launcher_hit(panel: Panel, x: i32) -> bool {
    x >= panel.get_client_size().width - panel.from_dip_int(LAUNCHER_DIP)
}

pub(super) fn install_caption(panel: Panel, label: &'static str, first: bool, last: bool, launcher: bool) {
    let hot = std::rc::Rc::new(std::cell::Cell::new(false));
    if launcher {
        let hot_motion = std::rc::Rc::clone(&hot);
        panel.on_mouse_motion(move |event| {
            let over = match &event { wxdragon::event::WindowEventData::MouseMotion(m) => m.event.get_position(), _ => None }
                .is_some_and(|p| launcher_hit(panel, p.x));
            if hot_motion.replace(over) != over { panel.refresh(false, None); }
        });
        let hot_leave = std::rc::Rc::clone(&hot);
        panel.on_mouse_leave(move |_| { if hot_leave.replace(false) { panel.refresh(false, None); } });
    }
    if let Some(font) = Font::new_with_details(9, FontFamily::Default as i32,
        FontStyle::Normal as i32, FontWeight::Normal as i32, false, "Segoe UI") {
        panel.set_font(&font);
    }
    measure_caption(panel, label, launcher);
    panel.set_background_style(BackgroundStyle::Paint);
    // The caption's font and label size, made and measured once (per scale)
    // rather than on every paint.
    let font = Font::new_with_details(9, FontFamily::Default as i32,
        FontStyle::Normal as i32, FontWeight::Normal as i32, false, "Segoe UI");
    let measured: std::cell::Cell<Option<(i32, (i32, i32))>> = std::cell::Cell::new(None);
    panel.on_paint(move |_| {
        let dc = AutoBufferedPaintDC::new(&panel);
        let size = panel.get_client_size();
        dc.set_background(Colour::rgb(255, 255, 255)); dc.clear();
        let radius = panel.from_dip_int(5);
        dc.set_pen(Colour::rgb(238, 243, 252), 1, PenStyle::Transparent);
        dc.set_brush(Colour::rgb(238, 243, 252), BrushStyle::Solid);
        let left = if first { 0 } else { -radius };
        let right = size.width + if last { 0 } else { radius };
        dc.draw_rounded_rectangle(left, -radius, right - left, size.height + radius, radius as f64);
        if let Some(font) = &font { dc.set_font(font); }
        dc.set_text_foreground(Colour::rgb(53, 78, 123));
        let scale = panel.from_dip_int(1000) + (crate::i18n::language() as i32) * 100_000;
        let (width, height) = match measured.get() {
            Some((at, extent)) if at == scale => extent,
            _ => { let extent = dc.get_text_extent(&crate::i18n::text(&(label))); measured.set(Some((scale, extent))); extent }
        };
        dc.draw_text(&crate::i18n::text(&(label)), (size.width - width).max(0) / 2, (size.height - height).max(0) / 2);
        if launcher { paint_launcher(&dc, panel, size.width, size.height, hot.get()); }
    });
    panel.on_size(move |event| { panel.refresh(false, None); event.skip(true); });
}

/// Recompute caption width after a scale or interface-language change.
pub(super) fn measure_caption(panel: Panel, label: &str, launcher: bool) {
    panel.set_min_size(wxdragon::geometry::Size::new(
        panel.get_text_extent(&crate::i18n::text(label)).width + panel.from_dip_int(12 + if launcher { 2 * LAUNCHER_DIP } else { 0 }),
        panel.from_dip_int(18)));
}

pub(super) fn layout(panel: Panel) {
    if panel.is_shown() {
        // Recompute the containing panel's cached best size after its buttons
        // have been measured. Layout alone retains the old group width.
        if let Some(sizer) = panel.get_sizer() { panel.set_sizer_and_fit(sizer, false); }
        panel.layout();
    }
}

/// Arrow of the dialog launcher in 16-unit icon space (design/icons/dialog-launcher.svg):
/// a shaft from the top left into an arrowhead whose tip is at the bottom right.
const LAUNCHER_ARROW: [(f32, f32); 9] = [(5.18, 4.4), (4.0, 5.73), (8.66, 9.82), (7.68, 10.51), (7.86, 11.03),
    (12.0, 11.5), (10.98, 7.47), (10.34, 7.43), (9.86, 8.45)];

/// Word's dialog launcher: a small rounded square with a diagonal arrow
/// pointing into its bottom-right corner, painted antialiased.
fn paint_launcher(dc: &AutoBufferedPaintDC, panel: Panel, width: i32, height: i32, hot: bool) {
    use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Stroke, Transform};
    let scale = panel.from_dip_int(1000) as f32 / 1000.0;
    let cell = (16.0 * scale).round() as i32;
    // Drawn at the screen's resolution (see crisp.rs): `unit` is in its pixels.
    let k = crate::display_scale::window_scale(&panel);
    let physical = (cell as f32 * k).round().max(1.0) as u32;
    let Some(mut pixmap) = Pixmap::new(physical, physical) else { return; };
    let background = if hot { (214, 224, 240) } else { (238, 243, 252) };
    pixmap.fill(tiny_skia::Color::from_rgba8(background.0, background.1, background.2, 255));
    let ink = if hot { (32, 38, 48) } else { (92, 100, 112) };
    let mut paint = Paint::default(); paint.anti_alias = true;
    paint.set_color_rgba8(ink.0, ink.1, ink.2, 255);
    let unit = physical as f32 / 16.0;
    // Square outline on whole-pixel centres, so its edges stay crisp.
    let stroke_width = unit.max(1.0).round();
    let inset = (1.0 * unit).round() + stroke_width * 0.5;
    let side = physical as f32 - 2.0 * inset;
    if let Some(rect) = tiny_skia::Rect::from_xywh(inset, inset, side, side) {
        let radius = 2.0 * unit;
        let (x0, y0, x1, y1) = (rect.left(), rect.top(), rect.right(), rect.bottom());
        let c = radius * 0.4477;
        let mut p = PathBuilder::new();
        p.move_to(x0 + radius, y0);
        p.line_to(x1 - radius, y0); p.cubic_to(x1 - c, y0, x1, y0 + c, x1, y0 + radius);
        p.line_to(x1, y1 - radius); p.cubic_to(x1, y1 - c, x1 - c, y1, x1 - radius, y1);
        p.line_to(x0 + radius, y1); p.cubic_to(x0 + c, y1, x0, y1 - c, x0, y1 - radius);
        p.line_to(x0, y0 + radius); p.cubic_to(x0, y0 + c, x0 + c, y0, x0 + radius, y0);
        p.close();
        if let Some(path) = p.finish() {
            pixmap.stroke_path(&path, &paint, &Stroke { width: stroke_width, ..Stroke::default() }, Transform::identity(), None);
        }
    }
    let mut arrow = PathBuilder::new();
    for (i, (x, y)) in LAUNCHER_ARROW.iter().enumerate() {
        if i == 0 { arrow.move_to(x * unit, y * unit); } else { arrow.line_to(x * unit, y * unit); }
    }
    arrow.close();
    if let Some(path) = arrow.finish() {
        pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
    }
    if let Some(bitmap) = crate::crisp::bitmap(&pixmap) {
        let x = width - panel.from_dip_int(4) - cell;
        let y = (height - cell) / 2;
        crate::crisp::draw(dc, &bitmap, k, x, y);
    }
}
