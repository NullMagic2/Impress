//! Renders PDF pages to pixels with hayro, for scanned pages and drawings.
use std::sync::Arc;

pub(crate) struct Renderer { pdf: hayro::hayro_syntax::Pdf }

impl Renderer {
    pub(crate) fn new(bytes: &[u8]) -> Option<Self> {
        hayro::hayro_syntax::Pdf::new(Arc::new(bytes.to_vec())).ok().map(|pdf| Self { pdf })
    }

    /// The page at `dpi` as RGB pixels on white. The renderer is young; a
    /// page it cannot handle gives None.
    pub(crate) fn page(&self, index: usize, dpi: f32) -> Option<image::RgbImage> {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let page = self.pdf.pages().get(index)?;
            let scale = dpi / 72.0;
            let settings = hayro::RenderSettings { x_scale: scale, y_scale: scale, bg_color: hayro::vello_cpu::color::palette::css::WHITE, ..Default::default() };
            let pixmap = hayro::render(page, &hayro::RenderCache::new(), &hayro::hayro_interpret::InterpreterSettings::default(), &settings);
            let (width, height) = (u32::from(pixmap.width()), u32::from(pixmap.height()));
            image::RgbImage::from_raw(width, height, pixmap.take_unpremultiplied().iter().flat_map(|p| [p.r, p.g, p.b]).collect())
        })).ok().flatten()
    }
}

/// Renders the given pages at `dpi` on every core, one renderer per thread,
/// and hands each page's pixels to `work`. Results come back in page order.
pub(crate) fn each_page<T: Send>(bytes: &[u8], pages: &[usize], dpi: f32, work: impl Fn(usize, &image::RgbImage) -> T + Sync) -> Vec<(usize, T)> {
    if pages.is_empty() { return Vec::new(); }
    let threads = std::thread::available_parallelism().map_or(2, |n| n.get()).clamp(1, 8).min(pages.len());
    let next = std::sync::atomic::AtomicUsize::new(0);
    let mut results: Vec<(usize, T)> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads).map(|_| scope.spawn(|| {
            let mut done = Vec::new();
            let Some(renderer) = Renderer::new(bytes) else { return done; };
            loop {
                let at = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some(&index) = pages.get(at) else { break; };
                if let Some(image) = renderer.page(index, dpi) { done.push((index, work(index, &image))); }
            }
            done
        })).collect();
        workers.into_iter().flat_map(|w| w.join().unwrap_or_default()).collect()
    });
    results.sort_by_key(|(index, _)| *index);
    results
}

/// PNG of an image, in greyscale when every pixel is grey (scans usually are).
pub(crate) fn png(image: &image::RgbImage) -> Option<Vec<u8>> {
    let mut out = std::io::Cursor::new(Vec::new());
    if image.pixels().all(|p| p[0] == p[1] && p[1] == p[2]) {
        image::DynamicImage::ImageRgb8(image.clone()).into_luma8().write_to(&mut out, image::ImageFormat::Png).ok()?;
    } else {
        image.write_to(&mut out, image::ImageFormat::Png).ok()?;
    }
    Some(out.into_inner())
}
