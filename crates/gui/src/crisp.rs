//! Custom-drawn controls paint at the screen's resolution. Their shapes are
//! given in logical pixels, but on a HiDPI screen an image of logical size is
//! stretched and blurs every edge and icon. A `Canvas` therefore has one pixel
//! per screen pixel, and while it is alive `transform()` maps logical pixels
//! onto it (it is the identity at any other time, for images that are not
//! drawn on screen this way).
use std::cell::Cell;
use tiny_skia::{Pixmap, Transform};
use wxdragon::{bitmap::Bitmap, dc::DeviceContext, window::WxWidget};

thread_local! { static SCALE: Cell<f32> = const { Cell::new(1.0) }; }

/// Screen pixels per logical pixel of the canvas being painted.
pub fn scale() -> f32 { SCALE.with(Cell::get) }
/// Logical pixels to the pixels of the canvas being painted.
pub fn transform() -> Transform { let s = scale(); Transform::from_scale(s, s) }

/// Sets the drawing scale while it lives; made with the image it is for.
pub struct Canvas { scale: f32, previous: f32 }

impl Canvas {
    /// An image for `window`'s `width` × `height` logical pixels, at the
    /// screen's resolution, and the canvas that presents it.
    pub fn new(window: &impl WxWidget, width: i32, height: i32) -> Option<(Pixmap, Self)> {
        let scale = crate::display_scale::window_scale(window);
        let side = |v: i32| (v.max(1) as f32 * scale).round().max(1.0) as u32;
        let pixmap = Pixmap::new(side(width), side(height))?;
        let previous = SCALE.with(|s| s.replace(scale));
        Some((pixmap, Self { scale, previous }))
    }

    pub fn scale(&self) -> f32 { self.scale }

    /// Draws `pixmap` with its top left at logical (x, y).
    pub fn present(&self, dc: &impl DeviceContext, pixmap: &Pixmap, x: i32, y: i32) {
        if let Some(bitmap) = bitmap(pixmap) { draw(dc, &bitmap, self.scale, x, y); }
    }
}

impl Drop for Canvas {
    fn drop(&mut self) { SCALE.with(|s| s.set(self.previous)); }
}

/// `pixmap` as a bitmap, to draw later with `draw`.
pub fn bitmap(pixmap: &Pixmap) -> Option<Bitmap> { Bitmap::from_rgba(pixmap.data(), pixmap.width(), pixmap.height()) }

/// Draws a canvas's `bitmap` made at `scale` with its top left at logical (x, y).
pub fn draw(dc: &impl DeviceContext, bitmap: &Bitmap, scale: f32, x: i32, y: i32) {
    let reduction = 1.0 / f64::from(scale);
    dc.set_user_scale(reduction, reduction);
    dc.draw_bitmap(bitmap, (x as f32 * scale).round() as i32, (y as f32 * scale).round() as i32, true);
    dc.set_user_scale(1.0, 1.0);
}
