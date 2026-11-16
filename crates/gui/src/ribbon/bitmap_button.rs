//! BitmapBundle direction bridge for the pinned wxDragon 0.9.21 API.
//!
//! `Button::set_bitmap_bundle` casts ButtonBitmapPosition (0, 1, 2, 3)
//! straight to wxd_Direction_t. Its C bridge expects native wxDirection flags
//! (wxLEFT, wxRIGHT, wxTOP, wxBOTTOM), unlike the single-bitmap setter which
//! translates the enum. Use the existing C bridge with the correct flags; keep
//! the multi-resolution bundle rather than falling back to one bitmap.

use wxdragon::{
    bitmap_bundle::BitmapBundle,
    ffi,
    widgets::button::{Button, ButtonBitmapPosition},
    window::WxWidget,
};

const fn native_direction(position: ButtonBitmapPosition) -> ffi::wxd_Direction_t {
    match position {
        ButtonBitmapPosition::Left => ffi::WXD_LEFT as ffi::wxd_Direction_t,
        ButtonBitmapPosition::Right => ffi::WXD_RIGHT as ffi::wxd_Direction_t,
        ButtonBitmapPosition::Top => ffi::WXD_TOP as ffi::wxd_Direction_t,
        ButtonBitmapPosition::Bottom => ffi::WXD_BOTTOM as ffi::wxd_Direction_t,
    }
}

/// Install a valid bundle on the UI thread. Destroyed buttons are a no-op, just
/// as they are in wxDragon's high-level setter. No ownership is transferred.
pub(super) fn set_bundle(button: &Button, bundle: &BitmapBundle, position: ButtonBitmapPosition) {
    let window = button.handle_ptr();
    if window.is_null() || !bundle.is_ok() {
        return;
    }

    // SAFETY: handle_ptr() supplies the live wxWindow pointer of this typed
    // Button, NOT its platform HWND. The bundle is borrowed and stays alive
    // for this synchronous call; wxButton retains its own ref-counted copy.
    // native_direction() returns exactly one native wxDirection flag. All
    // callers are in the Ribbon's UI-thread initialization/state-update paths.
    unsafe {
        ffi::wxd_Button_SetBitmapBundle(
            window.cast::<ffi::wxd_Button_t>(),
            bundle.as_ptr(),
            native_direction(position),
        );
    }
}

