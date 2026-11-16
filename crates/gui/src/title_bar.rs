//! On Linux, Impress's top strip is the window's title bar, as in GNOME apps
//! and Word: the desktop's own title bar is replaced, so the document
//! switcher sits in it. GTK keeps drawing the window's shadow and resize
//! edges (client-side decorations). Dragging the strip moves the window,
//! double-clicking it maximizes or restores, and the strip ends with
//! minimize, maximize and close buttons. Windows keeps its native title bar.
use super::*;
use crate::ribbon::{mix, title_bar, title_ink};
#[cfg(target_os = "linux")]
use wxdragon::{geometry::Size, sizers::SizerFlag, widgets::panel::Panel, window::BackgroundStyle, BrushStyle, Colour, PenStyle};
#[cfg(target_os = "linux")]
use std::time::{Duration, Instant};


#[cfg(target_os = "linux")]
/// Sizes in device-independent pixels.
const BUTTON: i32 = 30;
#[cfg(target_os = "linux")]
const BUTTON_GAP: i32 = 4;
#[cfg(target_os = "linux")]
const END_GAP: i32 = 8;
#[cfg(target_os = "linux")]
/// Two presses this close together are a double-click.
const DOUBLE_CLICK: Duration = Duration::from_millis(450);

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, PartialEq)]
enum Kind { Minimize, Maximize, Close }

/// Replaces the desktop's title bar with the top strip. Call before the
/// frame is first shown; GTK cannot change a window that is already on screen.
#[cfg(target_os = "linux")]
pub fn install(ui: &MainUi) {
    let frame = ui.main_frame;
    if !crate::gtk_native::hide_system_title_bar(&frame) { return; }
    let strip = ui.title_strip;
    let last_press = Rc::new(Cell::new(None::<Instant>));
    strip.on_mouse_left_down(move |_| {
        let now = Instant::now();
        if last_press.replace(Some(now)).is_some_and(|before| now.duration_since(before) < DOUBLE_CLICK) {
            last_press.set(None);
            frame.maximize(!frame.is_maximized());
            return;
        }
        crate::gtk_native::begin_move(&frame);
    });
    if let Some(sizer) = strip.get_sizer() {
        for (index, kind) in [Kind::Minimize, Kind::Maximize, Kind::Close].into_iter().enumerate() {
            let button = window_button(ui, kind);
            let gap = strip.from_dip_int(if index == 0 { END_GAP } else { BUTTON_GAP });
            sizer.add(&button, 0, SizerFlag::AlignCenterVertical | SizerFlag::Left, gap);
        }
        sizer.add_spacer(strip.from_dip_int(END_GAP));
        strip.layout();
    }
}

#[cfg(not(target_os = "linux"))]
pub fn install(_ui: &MainUi) {}

/// A round window button, drawn like GNOME's: a symbol, a grey disc on hover.
#[cfg(target_os = "linux")]
fn window_button(ui: &MainUi, kind: Kind) -> Panel {
    use wxdragon::{dc::AutoBufferedPaintDC, event::window_events::WindowEvents, DeviceContext};
    let strip = ui.title_strip;
    let frame = ui.main_frame;
    let size = strip.from_dip(Size::new(BUTTON, BUTTON));
    let panel = Panel::builder(&strip).with_size(size).build();
    panel.set_min_size(size);
    panel.set_background_style(BackgroundStyle::Paint);
    panel.on_erase_background(|_| {});
    panel.set_tooltip(&crate::i18n::text(&(match kind { Kind::Minimize => "Minimize", Kind::Maximize => "Maximize or restore", Kind::Close => "Close" })));
    let hover = Rc::new(Cell::new(false));
    { let hover = Rc::clone(&hover); panel.on_mouse_motion(move |_| { if !hover.replace(true) { panel.refresh(false, None); } }); }
    { let hover = Rc::clone(&hover); panel.on_mouse_leave(move |_| { if hover.replace(false) { panel.refresh(false, None); } }); }
    panel.on_mouse_left_up(move |_| match kind {
        Kind::Minimize => frame.iconize(true),
        Kind::Maximize => frame.maximize(!frame.is_maximized()),
        Kind::Close => { frame.close(false); }
    });
    panel.on_paint(move |_| {
        let dc = AutoBufferedPaintDC::new(&panel);
        let size = panel.get_client_size();
        let dip = |v: i32| panel.from_dip_int(v);
        let (bg, ink) = (title_bar(), title_ink());
        let colour = |c: [u8; 3]| Colour::rgb(c[0], c[1], c[2]);
        dc.set_background(colour(bg)); dc.clear();
        let (cx, cy) = (size.width / 2, size.height / 2);
        let disc = colour(mix(bg, ink, if hover.get() { 0.16 } else { 0.08 }));
        dc.set_pen(disc, 1, PenStyle::Solid);
        dc.set_brush(disc, BrushStyle::Solid);
        dc.draw_circle(cx, cy, dip(11));
        let ink = colour(ink);
        dc.set_pen(ink, dip(1).max(1) * 3 / 2, PenStyle::Solid);
        dc.set_brush(Colour::rgb(0, 0, 0), BrushStyle::Transparent);
        let r = dip(4);
        match kind {
            Kind::Minimize => dc.draw_line(cx - r, cy + dip(2), cx + r, cy + dip(2)),
            Kind::Maximize if frame.is_maximized() => {
                dc.draw_rectangle(cx - r, cy - r + dip(2), 2 * r - dip(2), 2 * r - dip(2));
                dc.draw_line(cx - r + dip(2), cy - r, cx + r, cy - r);
                dc.draw_line(cx + r, cy - r, cx + r, cy + r - dip(2));
            }
            Kind::Maximize => dc.draw_rectangle(cx - r, cy - r, 2 * r, 2 * r),
            Kind::Close => {
                dc.draw_line(cx - r, cy - r, cx + r, cy + r);
                dc.draw_line(cx + r, cy - r, cx - r, cy + r);
            }
        }
    });
    // The maximize symbol follows the window's state.
    if kind == Kind::Maximize { frame.on_size(move |event| { panel.refresh(false, None); event.skip(true); }); }
    panel
}
