//! Embedded theme artwork with a stable logical size at every DPI.
//! These are UI bitmaps only; document pages and glyphs remain vector geometry.
use wxdragon::{bitmap::Bitmap, bitmap_bundle::BitmapBundle};
use std::{collections::HashMap, sync::{Mutex, OnceLock}};

#[path = "embedded.rs"]
mod embedded;

/// Transparent space around the artwork, separate from the button's padding.
pub const INSET_DIP: u32 = 2;
const SCALE_PERCENT: [u32; 7] = [100, 125, 150, 175, 200, 250, 300];

pub struct ButtonArt {
    pub normal: BitmapBundle,
    pub disabled: BitmapBundle,
}

type DecodedAsset = (&'static [u8], u32, u32);
static DECODED: OnceLock<Mutex<HashMap<&'static str, DecodedAsset>>> = OnceLock::new();

fn asset(name: &str) -> Option<DecodedAsset> {
    let (key, png, width, height) = match name {
        "flag-gb" => ("flag-gb",include_bytes!("../../assets/flags/gb.png").as_slice(),64,64),
        "flag-gr" => ("flag-gr",include_bytes!("../../assets/flags/gr.png").as_slice(),64,64),
        "flag-es" => ("flag-es",include_bytes!("../../assets/flags/es.png").as_slice(),64,64),
        "flag-br" => ("flag-br",include_bytes!("../../assets/flags/br.png").as_slice(),64,64),
        "flag-pt" => ("flag-pt",include_bytes!("../../assets/flags/pt.png").as_slice(),64,64),
        _ => embedded::asset(name)?,
    };
    let mut cache = DECODED.get_or_init(|| Mutex::new(HashMap::new())).lock().ok()?;
    if let Some(&asset) = cache.get(key) { return Some(asset); }
    let image = image::load_from_memory_with_format(png, image::ImageFormat::Png).ok()?.into_rgba8();
    if image.dimensions() != (width, height) { return None; }
    // Gallery previews have wide transparent padding in the source PNGs.
    // Trim it so their lettering remains legible in a narrow style tile.
    let image = if matches!(key, "style_normal" | "style_heading1" | "style_title") {
        let bounds = image.enumerate_pixels().filter(|(_, _, pixel)| pixel[3] != 0)
            .fold(None, |bounds: Option<(u32, u32, u32, u32)>, (x, y, _)| Some(match bounds {
                Some((left, top, right, bottom)) => (left.min(x), top.min(y), right.max(x), bottom.max(y)),
                None => (x, y, x, y),
            }));
        if let Some((left, top, right, bottom)) = bounds {
            image::imageops::crop_imm(&image, left, top, right - left + 1, bottom - top + 1).to_image()
        } else { image }
    } else { image };
    let (width, height) = image.dimensions();
    let pixels: &'static [u8] = Box::leak(image.into_raw().into_boxed_slice());
    let asset = (pixels, width, height);
    cache.insert(key, asset);
    Some(asset)
}

/// Fit supplied transparent PNG artwork to a square using the toolbar's alpha-safe scaler.
pub(super) fn bitmap_from_png(png: &[u8], side_px: u32) -> Option<Bitmap> {
    if side_px == 0 { return None; }
    let image = image::load_from_memory_with_format(png, image::ImageFormat::Png).ok()?.into_rgba8();
    let (width, height) = image.dimensions();
    let rgba = fitted_rgba(image.as_raw(), width, height, side_px, side_px, 0);
    Bitmap::from_rgba(&rgba, side_px, side_px)
}

/// Width and height are unscaled DIPs, NOT FromDIP() physical pixels.
///
/// A 1x bitmap is always first. wxWidgets can then choose a higher-resolution
/// image without treating an already DPI-scaled bitmap as its logical 1x size.
/// Keep normal/disabled dimensions identical so a state change cannot
/// invalidate button proportions or move the label.
pub fn load(name: &str, width_dip: u32, height_dip: u32) -> Option<ButtonArt> {
    let (rgba, source_w, source_h) = asset(name)?;
    if width_dip == 0 || height_dip == 0 { return None; }
    let mut normal = Vec::with_capacity(SCALE_PERCENT.len());
    let mut disabled = Vec::with_capacity(SCALE_PERCENT.len());
    for percent in SCALE_PERCENT {
        let width = scaled(width_dip + 2 * INSET_DIP, percent);
        let height = scaled(height_dip + 2 * INSET_DIP, percent);
        let inset = scaled(INSET_DIP, percent);
        let pixels = fitted_rgba(rgba, source_w, source_h, width, height, inset);
        let muted = disabled_rgba(&pixels);
        normal.push(Bitmap::from_rgba(&pixels, width, height)?);
        disabled.push(Bitmap::from_rgba(&muted, width, height)?);
    }
    let art = ButtonArt {
        normal: BitmapBundle::from_bitmaps(&normal),
        disabled: BitmapBundle::from_bitmaps(&disabled),
    };
    if art.normal.is_ok() && art.disabled.is_ok() { Some(art) } else { None }
}

fn scaled(dip: u32, percent: u32) -> u32 { ((dip * percent + 50) / 100).max(1) }

/// Contain the full source, preserving its aspect ratio. Nothing is cropped.
fn fitted_rgba(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32, inset: u32) -> Vec<u8> {
    assert!(dw > 2 * inset && dh > 2 * inset);
    let inner_w = dw - 2 * inset;
    let inner_h = dh - 2 * inset;
    let factor = (inner_w as f64 / sw as f64).min(inner_h as f64 / sh as f64);
    let w = ((sw as f64 * factor).round() as u32).clamp(1, inner_w);
    let h = ((sh as f64 * factor).round() as u32).clamp(1, inner_h);
    let image = resize_rgba(src, sw, sh, w, h);
    let mut output = vec![0; (dw * dh * 4) as usize];
    let x = (dw - w) / 2;
    let y = (dh - h) / 2;
    for row in 0..h {
        let from = (row * w * 4) as usize;
        let to = (((row + y) * dw + x) * 4) as usize;
        output[to..to + (w * 4) as usize].copy_from_slice(&image[from..from + (w * 4) as usize]);
    }
    output
}

fn disabled_rgba(pixels: &[u8]) -> Vec<u8> {
    let mut out = pixels.to_vec();
    for pixel in out.chunks_exact_mut(4) {
        let gray = (u32::from(pixel[0]) * 30 + u32::from(pixel[1]) * 59 + u32::from(pixel[2]) * 11) / 100;
        let muted = ((gray * 2 + 165) / 3) as u8;
        pixel[0] = muted; pixel[1] = muted; pixel[2] = muted;
        pixel[3] = (u16::from(pixel[3]) * 175 / 255) as u8;
    }
    out
}

type RasterKey = (String, u32, u32, bool);
thread_local! {
    /// Scaled artwork by icon, size and state. Every button face is painted in
    /// four states, and many buttons share an icon, so each is scaled once.
    static RASTERS: std::cell::RefCell<HashMap<RasterKey, Option<std::rc::Rc<[u8]>>>> = Default::default();
}

/// The live color band in the font-color/highlighter artwork.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum ColourIndicator { Colour([u8;3]), NoColour }
type ColourRasterKey = (String,u32,u32,bool,ColourIndicator);
thread_local! { static COLOUR_RASTERS: std::cell::RefCell<HashMap<ColourRasterKey,std::rc::Rc<[u8]>>> = Default::default(); }

pub(super) fn colour_raster(name:&str,width:u32,height:u32,disabled:bool,indicator:ColourIndicator)->Option<std::rc::Rc<[u8]>> {
    let key=(name.to_owned(),width,height,disabled,indicator);
    if let Some(image)=COLOUR_RASTERS.with(|cache|cache.borrow().get(&key).cloned()){return Some(image);}
    let (source,sw,sh)=asset(name)?;
    let tinted=tint_indicator(source,sw,sh,indicator);
    let pixels=fitted_rgba(&tinted,sw,sh,width,height,0);
    let image=std::rc::Rc::<[u8]>::from(if disabled{disabled_rgba(&pixels)}else{pixels});
    COLOUR_RASTERS.with(|cache|{
        let mut cache=cache.borrow_mut();
        // Imported documents can contain arbitrarily many colors.
        if cache.len()>=128{cache.clear();}
        cache.insert(key,image.clone());
    });
    Some(image)
}
fn tint_indicator(source:&[u8],width:u32,height:u32,indicator:ColourIndicator)->Vec<u8>{
    let accent=|p:&[u8]|p[3]>0 && p[..3].iter().max().unwrap()-p[..3].iter().min().unwrap()>20;
    let mut bounds:Option<(u32,u32,u32,u32)>=None;
    let mut strongest=1u8;
    for y in 0..height {for x in 0..width {
        let p=&source[((y*width+x)*4) as usize..][..4];
        if accent(p){
            strongest=strongest.max(*p[..3].iter().max().unwrap());
            // Both 64px source icons place their full-width band below 2/3.
            if y>=height*2/3 {bounds=Some(match bounds{
                Some((left,top,right,bottom))=>(x.min(left),y.min(top),x.max(right),y.max(bottom)),
                None=>(x,y,x,y),
            });}
        }
    }}
    let mut out=source.to_vec();
    for y in 0..height {for x in 0..width {
        let i=((y*width+x)*4) as usize;let p=&source[i..i+4];if !accent(p){continue;}
        let shade=*p[..3].iter().max().unwrap() as f32/strongest as f32;
        let color=match indicator{ColourIndicator::Colour(c)=>c,ColourIndicator::NoColour=>[143,150,160]};
        for channel in 0..3{out[i+channel]=(color[channel] as f32*shade.min(1.0)).round() as u8;}
        if let Some((left,top,right,bottom))=bounds {
            if y>=top {
                let edge=x<=left+1||x+1>=right||y<=top+1||y+1>=bottom;
                match indicator {
                    ColourIndicator::NoColour=>{
                        let slash=((x-left) as f32/(right-left).max(1) as f32+(y-top) as f32/(bottom-top).max(1) as f32-1.0).abs()<0.09;
                        out[i+3]=if edge||slash{255}else{0};
                    }
                    ColourIndicator::Colour(c) if c.iter().all(|v|*v>220)&&edge=>out[i..i+3].copy_from_slice(&[143,150,160]),
                    _=>{}
                }
            }
        }
    }}
    out
}

/// DPI-sized transparent artwork for the native full-button painter.
pub(super) fn raster(name: &str, width: u32, height: u32, disabled: bool) -> Option<std::rc::Rc<[u8]>> {
    let key = (name.to_owned(), width, height, disabled);
    if let Some(cached) = RASTERS.with(|cache| cache.borrow().get(&key).cloned()) { return cached; }
    let scaled = asset(name).map(|(bytes, sw, sh)| {
        let pixels = fitted_rgba(bytes, sw, sh, width, height, 0);
        std::rc::Rc::from(if disabled { disabled_rgba(&pixels) } else { pixels })
    });
    RASTERS.with(|cache| cache.borrow_mut().insert(key, scaled.clone()));
    scaled
}

/// Resampling in premultiplied-alpha space prevents dark fringes around the
/// antialiased icon edges at fractional Windows DPI scales.
///
/// Shrinking averages every source pixel a destination pixel covers. The
/// 64 px artwork is shown at 16-36 px, and plain bilinear sampling would skip
/// most of its thin strokes there, leaving broken, jagged lines. Enlarging
/// keeps bilinear interpolation.
fn resize_rgba(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<u8> {
    assert_eq!(src.len(), (sw * sh * 4) as usize);
    assert!(sw > 0 && sh > 0 && dw > 0 && dh > 0);
    let columns = resample_taps(sw, dw);
    let rows = resample_taps(sh, dh);
    let mut dst = vec![0_u8; (dw * dh * 4) as usize];
    for (y, row_taps) in rows.iter().enumerate() {
        for (x, column_taps) in columns.iter().enumerate() {
            let mut alpha = 0.0_f32; let mut color = [0.0_f32; 3];
            for &(py, wy) in row_taps {
                for &(px, wx) in column_taps {
                    let i = ((py * sw + px) * 4) as usize;
                    let a = f32::from(src[i + 3]) / 255.0 * wx * wy;
                    alpha += a;
                    for channel in 0..3 { color[channel] += f32::from(src[i + channel]) * a; }
                }
            }
            let i = ((y as u32 * dw + x as u32) * 4) as usize;
            if alpha > 0.0 {
                for channel in 0..3 { dst[i + channel] = (color[channel] / alpha).round().min(255.0) as u8; }
            }
            dst[i + 3] = (alpha * 255.0).round().min(255.0) as u8;
        }
    }
    dst
}

/// Source pixels and weights for each destination pixel along one axis. The
/// weights of every destination pixel add up to 1.
fn resample_taps(source: u32, destination: u32) -> Vec<Vec<(u32, f32)>> {
    let scale = source as f32 / destination as f32;
    (0..destination).map(|d| {
        if scale > 1.0 {
            // Area average: each covered source pixel, weighted by its overlap.
            let start = d as f32 * scale;
            let end = start + scale;
            (start.floor() as u32..(end.ceil() as u32).min(source))
                .map(|s| (s, (end.min(s as f32 + 1.0) - start.max(s as f32)) / scale))
                .filter(|&(_, weight)| weight > 0.0)
                .collect()
        } else {
            let centre = ((d as f32 + 0.5) * scale - 0.5).clamp(0.0, (source - 1) as f32);
            let first = centre.floor() as u32;
            let fraction = centre - first as f32;
            vec![(first, 1.0 - fraction), ((first + 1).min(source - 1), fraction)]
        }
    }).collect()
}

#[cfg(test)] #[path = "../../tests/unit/ribbon/art.rs"] mod tests;
