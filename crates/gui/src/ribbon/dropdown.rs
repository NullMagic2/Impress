//! Shared modeless Ribbon dropdown, drawn inside the main window as a panel
//! over the other controls, as Word's galleries are. A separate top-level
//! window cannot be placed on Wayland (GNOME centred it over the window) and
//! got the desktop's drop shadow; a panel lands exactly below its button and
//! lies flat. It closes on Escape or Tab, when another window is activated,
//! and when the mouse is pressed outside it (`poll`, from the Ribbon's tick).
use wxdragon::{
    event::{EventType, WxEvtHandler, WindowEventData, WindowEvents},
    geometry::{Point, Rect, Size},
    keycode::{WXK_ESCAPE, WXK_TAB},
    widgets::panel::Panel,
    window::WxWidget,
};
use std::cell::{Cell, RefCell};
use std::time::Instant;

thread_local! {
    /// The window dropdowns are drawn in: the main frame's top panel.
    static HOST: Cell<Option<Panel>> = const { Cell::new(None) };
    // Clear before hiding: focus notifications can re-enter dismiss.
    static ACTIVE: Cell<Option<(Panel, Instant)>> = const { Cell::new(None) };
    /// What the open dropdown does when it closes because the user went
    /// elsewhere (a press outside it, another window): an editor commits.
    static ON_LEAVE: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
}

/// Runs `commit` if the open dropdown closes because the user pressed
/// outside it or switched windows (not on Escape or its own commands).
pub(super) fn on_leave(commit: impl FnOnce() + 'static) {
    ON_LEAVE.with(|cell| *cell.borrow_mut() = Some(Box::new(commit)));
}

/// Closes `popup` as `poll` and window switches do: its leave action runs.
fn leave(popup: Panel) {
    let commit = ON_LEAVE.with(|cell| cell.borrow_mut().take());
    dismiss(popup);
    if let Some(commit) = commit { commit(); }
}

/// Where dropdowns are drawn; set once when the Ribbon is built. Switching
/// to another window closes an open dropdown.
pub(super) fn set_host(host: Panel, frame: &wxdragon::widgets::frame::Frame) {
    HOST.with(|cell| cell.set(Some(host)));
    crate::gtk_native::watch_presses();
    frame.on_activate(move |event| {
        if let WindowEventData::Activate(activation) = &event {
            if !activation.is_active() { if let Some((popup, _)) = ACTIVE.with(Cell::get) { leave(popup); } }
        }
        event.skip(true);
    });
}

/// Every dropdown's frame: a border in this colour around its content, so a
/// dropdown stands out over the white page, panes and its own white content.
const FRAME: (u8, u8, u8) = (163, 176, 194);
/// The shading of a dropdown's header and footer bands.
pub(super) const BAND: (u8, u8, u8) = (243, 246, 250);

fn frame_width(window: &impl WxWidget) -> i32 { window.from_dip_int(1).max(1) }

/// Where a dropdown's content goes in it: inside the frame.
pub(super) fn origin(panel: &Panel) -> Point { let b = frame_width(panel); Point::new(b, b) }

/// A hidden dropdown panel for `client_size` pixels of content (placed at
/// `origin`) and its frame; `show` places it.
pub(super) fn create(parent: &impl WxWidget, _title: &str, client_size: Size) -> Panel {
    if let Some((previous, _)) = ACTIVE.with(Cell::get) { leave(previous); }
    ON_LEAVE.with(|cell| cell.borrow_mut().take());
    let b = frame_width(parent);
    let size = Size::new(client_size.width + 2 * b, client_size.height + 2 * b);
    let panel = match HOST.with(Cell::get) {
        Some(host) => Panel::builder(&host).with_size(size).build(),
        None => Panel::builder(parent).with_size(size).build(),
    };
    // The panel shows only around its content: that is the frame.
    panel.set_background_color(wxdragon::color::Colour::rgb(FRAME.0, FRAME.1, FRAME.2));
    panel.hide();
    ACTIVE.with(|active| active.set(Some((panel, Instant::now()))));
    panel
}

pub(super) fn dismiss(panel: Panel) {
    let owned = ACTIVE.with(|active| {
        if active.get().is_some_and(|(current, _)| current.handle_ptr() == panel.handle_ptr()) {
            active.set(None);
            true
        } else { false }
    });
    if owned { ON_LEAVE.with(|cell| cell.borrow_mut().take()); }
    if owned && panel.is_valid() {
        panel.hide();
        panel.destroy();
    }
}

/// Closes the open dropdown when the mouse was pressed outside it since it
/// opened. Called on every Ribbon tick.
pub(super) fn poll() {
    let Some((popup, opened)) = ACTIVE.with(Cell::get) else { return; };
    let Some((at, when)) = crate::gtk_native::last_press() else { return; };
    if when <= opened || !popup.is_valid() || !popup.is_shown() { return; }
    let origin = crate::gtk_native::to_screen(&popup, Point::new(0, 0));
    let size = popup.get_size();
    let inside = at.x >= origin.x && at.x < origin.x + size.width && at.y >= origin.y && at.y < origin.y + size.height;
    if !inside { leave(popup); }
}

pub(super) fn show(panel: Panel, anchor: &impl WxWidget, focus: &impl WxWidget) {
    show_at(panel, anchor, focus, true);
}

/// Open over the anchor, top-left corners aligned, as Word expands its
/// in-Ribbon style gallery in place.
pub(super) fn show_over(panel: Panel, anchor: &impl WxWidget, focus: &impl WxWidget) {
    show_at(panel, anchor, focus, false);
}

fn show_at(panel: Panel, anchor: &impl WxWidget, focus: &impl WxWidget, below: bool) {
    let origin = crate::gtk_native::to_screen(anchor, Point::new(0, 0));
    let anchor_size = anchor.get_size();
    let height = if below { anchor_size.height } else { 0 };
    show_below_rect(panel, Rect::new(origin.x, origin.y, anchor_size.width, height), focus);
}

/// Open with its top left at `at` (screen coordinates), kept inside the
/// window: over something on the page, such as an equation being edited.
pub(super) fn show_at_point(panel: Panel, at: Point, focus: &impl WxWidget) {
    show_below_rect(panel, Rect::new(at.x, at.y, 0, 0), focus);
}

/// Open below `rect` (screen coordinates), such as a button drawn on the page.
pub(super) fn show_below_rect(panel: Panel, rect: Rect, focus: &impl WxWidget) {
    // Screen positions of two windows in the same frame differ correctly even
    // where the frame's own position is unknown (Wayland).
    let host = HOST.with(Cell::get);
    let host_origin = host.map_or(Point::new(0, 0), |h| crate::gtk_native::to_screen(&h, Point::new(0, 0)));
    let area = host.map_or(Rect::new(0, 0, 1 << 15, 1 << 15), |h| { let s = h.get_client_size(); Rect::new(0, 0, s.width, s.height) });
    let local = Rect::new(rect.x - host_origin.x, rect.y - host_origin.y, rect.width, rect.height);
    let at = position(local, panel.get_size(), area);
    panel.move_window(at.x, at.y);
    panel.show(true);
    panel.raise();
    focus.set_focus();
}

/// Handle dismissal before child widgets consume Escape (including scrollbars).
pub(super) fn bind_keys(panel: Panel, return_focus: impl WxWidget + Copy + 'static,
    mut handle: impl FnMut(i32) -> bool + 'static) {
    panel.bind_internal(EventType::CHAR_HOOK, move |event| {
        if let Some(key) = event.get_key_code() {
            if key == WXK_ESCAPE || key == WXK_TAB {
                dismiss(panel);
                return_focus.set_focus();
                return;
            }
            if handle(key) { return; }
        }
        event.skip(true);
    });
}

fn position(anchor: Rect, size: Size, area: Rect) -> Point {
    let x = anchor.x.clamp(area.x, (area.x + area.width - size.width).max(area.x));
    let below = anchor.y + anchor.height;
    let y = if below + size.height <= area.y + area.height { below }
        else { anchor.y - size.height };
    Point::new(x, y.clamp(area.y, (area.y + area.height - size.height).max(area.y)))
}

