//! The canvas frame as one persistent cairo image surface (Linux).
//!
//! Drawing a wxBitmap on GTK converts its whole GdkPixbuf into a cairo surface
//! on every paint, and replacing the bitmap after a resize goes through a
//! wxImage and a pixbuf as well. On a HiDPI screen a frame is tens of
//! megabytes, so maximizing, restoring and even a caret blink paid for that.
//! Here the frame lives in a cairo surface that only takes the rectangles
//! that changed, and a paint just sets it as the source.
use std::ffi::c_void;
use tiny_skia::Pixmap;
use render::Rect;

#[link(name = "cairo")]
unsafe extern "C" {
    fn cairo_image_surface_create(format: i32, width: i32, height: i32) -> *mut c_void;
    fn cairo_image_surface_get_data(surface: *mut c_void) -> *mut u8;
    fn cairo_image_surface_get_stride(surface: *mut c_void) -> i32;
    fn cairo_surface_flush(surface: *mut c_void);
    fn cairo_surface_mark_dirty_rectangle(surface: *mut c_void, x: i32, y: i32, width: i32, height: i32);
    fn cairo_surface_destroy(surface: *mut c_void);
    fn cairo_surface_status(surface: *mut c_void) -> i32;
    fn cairo_save(cr: *mut c_void);
    fn cairo_restore(cr: *mut c_void);
    fn cairo_scale(cr: *mut c_void, sx: f64, sy: f64);
    fn cairo_set_source_surface(cr: *mut c_void, surface: *mut c_void, x: f64, y: f64);
    fn cairo_paint(cr: *mut c_void);
}

unsafe extern "C" {
    /// From the vendored wxdragon-sys: the DC's cairo_t on GTK 3.
    fn wxd_DC_GetNativeHandle(dc: *mut c_void) -> *mut c_void;
}

/// CAIRO_FORMAT_ARGB32: premultiplied, 32-bit native-endian ARGB.
const ARGB32: i32 = 0;

pub struct CairoFrame { surface: *mut c_void, width: u32, height: u32 }

impl CairoFrame {
    /// A surface of `width` × `height` pixels, or None when cairo cannot make one.
    pub fn new(width: u32, height: u32) -> Option<Self> {
        let surface = unsafe { cairo_image_surface_create(ARGB32, width as i32, height as i32) };
        if surface.is_null() || unsafe { cairo_surface_status(surface) } != 0 {
            if !surface.is_null() { unsafe { cairo_surface_destroy(surface) }; }
            return None;
        }
        Some(Self { surface, width, height })
    }

    pub fn size(&self) -> (u32, u32) { (self.width, self.height) }

    /// Copies `rects` (view pixels; all of it when None) of `frame`, which has
    /// this surface's size, converting tiny-skia's RGBA to cairo's ARGB words.
    pub fn update(&mut self, frame: &Pixmap, rects: Option<&[Rect]>) {
        let (w, h) = (self.width as usize, self.height as usize);
        if frame.width() as usize != w || frame.height() as usize != h { return; }
        let whole = [Rect::new(0.0, 0.0, w as f32, h as f32)];
        let rects = rects.unwrap_or(&whole);
        unsafe {
            cairo_surface_flush(self.surface);
            let stride = cairo_image_surface_get_stride(self.surface) as usize;
            let data = cairo_image_surface_get_data(self.surface);
            if data.is_null() { return; }
            let target = std::slice::from_raw_parts_mut(data, stride * h);
            let source = frame.data();
            for rect in rects {
                let x0 = (rect.x.floor().max(0.0) as usize).min(w);
                let y0 = (rect.y.floor().max(0.0) as usize).min(h);
                let x1 = (rect.max_x().ceil().max(0.0) as usize).min(w);
                let y1 = (rect.max_y().ceil().max(0.0) as usize).min(h);
                if x0 >= x1 || y0 >= y1 { continue; }
                for row in y0..y1 {
                    let src = source[(row * w + x0) * 4..(row * w + x1) * 4].as_chunks::<4>().0;
                    let dst = target[row * stride + x0 * 4..row * stride + x1 * 4].as_chunks_mut::<4>().0;
                    // Both premultiplied; cairo's word is A,R,G,B in native order.
                    // Whole pixels as arrays: in this size-optimized build the
                    // loop then vectorizes, about six times faster.
                    for (d, s) in dst.iter_mut().zip(src) {
                        *d = u32::from_be_bytes([s[3], s[0], s[1], s[2]]).to_ne_bytes();
                    }
                }
                cairo_surface_mark_dirty_rectangle(self.surface, x0 as i32, y0 as i32, (x1 - x0) as i32, (y1 - y0) as i32);
            }
        }
    }

    /// Paints the surface into `dc`'s cairo context, scaled by `reduction`
    /// (logical pixels per frame pixel), so it lands 1:1 on device pixels.
    /// False when the DC has no cairo context.
    pub fn paint(&self, dc: *mut c_void, reduction: f64) -> bool {
        let cr = unsafe { wxd_DC_GetNativeHandle(dc) };
        if cr.is_null() { return false; }
        unsafe {
            cairo_save(cr);
            cairo_scale(cr, reduction, reduction);
            cairo_set_source_surface(cr, self.surface, 0.0, 0.0);
            cairo_paint(cr);
            cairo_restore(cr);
        }
        true
    }
}

impl Drop for CairoFrame {
    fn drop(&mut self) { unsafe { cairo_surface_destroy(self.surface) }; }
}
