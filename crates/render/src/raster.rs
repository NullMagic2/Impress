//! Raster resources are derived, shareable and bounded. The document retains
//! the encoded original. RGB bytes here encode *premultiplied linear* values
//! through the sRGB transfer function; sampling an SRGB texture returns the
//! correct premultiplied linear RGBA for ONE / ONE_MINUS_SRC_ALPHA blending.
use std::{collections::{hash_map::DefaultHasher, VecDeque}, hash::{Hash, Hasher},
    io::Cursor, sync::{Arc, Mutex, OnceLock, atomic::{AtomicU64, Ordering}}};
use thiserror::Error;

pub const MAX_RASTER_BYTES: usize = 128 * 1024 * 1024;
const CACHE_BYTES: usize = 256 * 1024 * 1024;
static NEXT_RASTER: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RasterMip { pub width: u32, pub height: u32, pub offset: usize }
#[derive(Clone, Debug)]
pub struct RasterImage {
    pub key: u64,
    pub width: u32,
    pub height: u32,
    pub levels: Vec<RasterMip>,
    pub pixels: Arc<[u8]>,
}
// Identity, not multi-megabyte pixel comparisons, on every scene comparison.
impl PartialEq for RasterImage { fn eq(&self, other: &Self) -> bool { self.key == other.key } }

#[derive(Debug, Error)]
pub enum RasterError {
    #[error("image exceeds the decoded-image safety limit (128 MiB / 16384 pixels)")]
    Limit,
    #[error("image decode failed: {0}")]
    Decode(String),
}

pub fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}
fn linear_to_srgb(v: f32) -> f32 {
    if v <= 0.0031308 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }
}
fn byte(v: f32) -> u8 { (v.clamp(0.0, 1.0) * 255.0).round() as u8 }

impl RasterImage {
    pub fn from_rgba(width: u32, height: u32, rgba: &[u8]) -> Result<Self, RasterError> {
        let length = (width as usize).checked_mul(height as usize).and_then(|v| v.checked_mul(4)).ok_or(RasterError::Limit)?;
        if width == 0 || height == 0 || width > 16384 || height > 16384 || length > MAX_RASTER_BYTES || rgba.len() != length {
            return Err(RasterError::Limit);
        }
        let mut pixels = Vec::with_capacity(length.saturating_mul(4) / 3 + 64);
        for p in rgba.chunks_exact(4) {
            let alpha = p[3] as f32 / 255.0;
            for c in &p[..3] { pixels.push(byte(linear_to_srgb(srgb_to_linear(*c as f32 / 255.0) * alpha))); }
            pixels.push(p[3]);
        }
        let mut levels = vec![RasterMip { width, height, offset: 0 }];
        let (mut w, mut h, mut offset) = (width, height, 0);
        while w > 1 || h > 1 {
            let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
            let next_offset = pixels.len();
            for y in 0..nh { for x in 0..nw {
                // Area coverage includes the final row/column of odd images.
                let (x0, x1) = (x * w / nw, (x + 1) * w / nw);
                let (y0, y1) = (y * h / nh, (y + 1) * h / nh);
                let mut sum = [0.0f32; 4];
                for sy in y0..y1 { for sx in x0..x1 {
                    let i = offset + ((sy * w + sx) * 4) as usize;
                    for c in 0..3 { sum[c] += srgb_to_linear(pixels[i + c] as f32 / 255.0); }
                    sum[3] += pixels[i + 3] as f32 / 255.0;
                }}
                let count = ((x1 - x0) * (y1 - y0)) as f32;
                for value in &sum[..3] { pixels.push(byte(linear_to_srgb(*value / count))); }
                pixels.push(byte(sum[3] / count));
            }}
            levels.push(RasterMip { width: nw, height: nh, offset: next_offset });
            (w, h, offset) = (nw, nh, next_offset);
        }
        Ok(Self { key: NEXT_RASTER.fetch_add(1, Ordering::Relaxed), width, height, levels, pixels: pixels.into() })
    }
    pub fn white() -> Arc<Self> {
        static WHITE: OnceLock<Arc<RasterImage>> = OnceLock::new();
        WHITE.get_or_init(|| Arc::new(Self::from_rgba(1, 1, &[255; 4]).expect("white texel"))).clone()
    }
}

struct CacheEntry { hash: u64, encoded: Arc<[u8]>, image: Arc<RasterImage> }
#[derive(Default)]
struct DecodeCache { entries: VecDeque<CacheEntry>, bytes: usize }

/// Exact-byte validation avoids hash collisions and stale resources after undo.
/// Decoding/mipmap generation only occurs on the background layout path. Holding
/// this mutex avoids duplicate decodes when the same picture is used on many pages.
pub fn decode_raster_cached(encoded: &[u8]) -> Result<Arc<RasterImage>, RasterError> {
    if encoded.len() > MAX_RASTER_BYTES { return Err(RasterError::Limit); }
    static CACHE: OnceLock<Mutex<DecodeCache>> = OnceLock::new();
    let mut cache = CACHE.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    let mut hasher = DefaultHasher::new(); encoded.hash(&mut hasher); let hash = hasher.finish();
    if let Some(i) = cache.entries.iter().position(|e| e.hash == hash && e.encoded.as_ref() == encoded) {
        let entry = cache.entries.remove(i).expect("cache index");
        let image = entry.image.clone(); cache.entries.push_back(entry); return Ok(image);
    }
    let mut reader = image::ImageReader::new(Cursor::new(encoded)).with_guessed_format().map_err(|e| RasterError::Decode(e.to_string()))?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16384); limits.max_image_height = Some(16384);
    limits.max_alloc = Some(MAX_RASTER_BYTES as u64);
    reader.limits(limits);
    let decoded = reader.decode().map_err(|e| RasterError::Decode(e.to_string()))?;
    let (width, height) = (decoded.width(), decoded.height());
    let expected = (width as usize).checked_mul(height as usize).and_then(|n| n.checked_mul(4)).ok_or(RasterError::Limit)?;
    if expected > MAX_RASTER_BYTES { return Err(RasterError::Limit); }
    let rgba = decoded.into_rgba8();
    let image = Arc::new(RasterImage::from_rgba(width, height, rgba.as_raw())?);
    let cost = encoded.len() + image.pixels.len();
    while cache.bytes + cost > CACHE_BYTES && !cache.entries.is_empty() {
        let old = cache.entries.pop_front().expect("cache nonempty");
        cache.bytes -= old.encoded.len() + old.image.pixels.len();
    }
    if cost <= CACHE_BYTES {
        cache.entries.push_back(CacheEntry { hash, encoded: encoded.into(), image: image.clone() });
        cache.bytes += cost;
    }
    Ok(image)
}

#[cfg(test)] #[path = "../tests/unit/raster.rs"] mod tests;
