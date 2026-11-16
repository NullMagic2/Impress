//! Dialogs open centred on the Impress window (not the screen, and not
//! wherever they were created), kept inside that window's display.
use std::cell::RefCell;
use wxdragon::prelude::*;

thread_local! { static APP: RefCell<Option<Frame>> = const { RefCell::new(None) }; }

/// Remembers the main window that dialogs centre on.
pub fn set_app(frame: &Frame) { APP.with(|app| *app.borrow_mut() = Some(frame.clone())); }

/// Moves `dialog` to the centre of the Impress window. Call it after the
/// dialog has its final size, just before showing it.
pub fn centre(dialog: &impl WxWidget) {
    let Some(app) = APP.with(|app| app.borrow().clone()) else { dialog.centre(); return; };
    let (at, size, own) = (app.get_position(), app.get_size(), dialog.get_size());
    let mut x = at.x + (size.width - own.width) / 2;
    let mut y = at.y + (size.height - own.height) / 2;
    if let Some(area) = wxdragon::display::Display::from_window(&app).map(|d| d.client_area()) {
        x = x.clamp(area.x, (area.x + area.width - own.width).max(area.x));
        y = y.clamp(area.y, (area.y + area.height - own.height).max(area.y));
    }
    dialog.move_window(x, y);
}
