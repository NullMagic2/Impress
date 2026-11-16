//! How many physical pixels a window draws per logical pixel.
//!
//! Windows paints in physical pixels and reports the display DPI through
//! FromDIP, so the factor is 1 there. GTK keeps logical pixels equal to DIPs
//! and scales whole windows on a HiDPI display (2 at 200 %). Anything drawn as
//! a bitmap at logical size is then stretched and looks blurry, so bitmaps are
//! painted at `window_scale` times their logical size instead.
use wxdragon::{dc::{ClientDC, DeviceContext}, window::WxWidget};

/// Largest factor accepted from the toolkit; anything above is treated as bogus.
const MAX_WINDOW_SCALE: f32 = 4.0;

/// Physical pixels per logical pixel for `window` (1 on Windows).
pub fn window_scale(window: &impl WxWidget) -> f32 {
    if cfg!(windows) { return 1.0; }
    // GTK knows the factor directly; a device context is the fallback.
    let scale = crate::gtk_native::scale_factor(window).map(|s| s as f32)
        .unwrap_or_else(|| ClientDC::new(window).get_content_scale_factor() as f32);
    scale.clamp(1.0, MAX_WINDOW_SCALE)
}
