//! Rasterize a folder of SVG icons to 64x64 PNGs.
//!
//! Usage: render-mock-icons <svg dir> <png dir> [inset px] [ink px]
//!
//! `inset` keeps a transparent border on every side. `ink`, when given, makes
//! every icon look the same size and sit in the same place: the drawing is
//! scaled so its visible ink's longer side is `ink` pixels, and centred.
use resvg::{tiny_skia, usvg};
use std::{env, fs, path::PathBuf};

/// Every raster is a 64x64 square, as the Ribbon art loader expects.
const ICON_SIZE: u32 = 64;
/// Alpha below this counts as empty when measuring an icon's ink.
const INK_ALPHA: u8 = 8;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args_os().skip(1);
    let input = PathBuf::from(args.next().ok_or("missing SVG directory")?);
    let output = PathBuf::from(args.next().ok_or("missing output directory")?);
    let inset: u32 = args.next().map(|value| value.to_string_lossy().parse()).transpose()?.unwrap_or(0);
    let ink: Option<f32> = args.next().map(|value| value.to_string_lossy().parse()).transpose()?;
    let drawable = ICON_SIZE.checked_sub(2 * inset).filter(|size| *size > 0).ok_or("inset too large")?;
    fs::create_dir_all(&output)?;
    let mut count = 0;
    for entry in fs::read_dir(input)? {
        let path = entry?.path();
        if path.extension().is_none_or(|extension| extension != "svg") { continue; }
        let tree = usvg::Tree::from_data(&fs::read(&path)?, &usvg::Options::default())?;
        let scale = drawable as f32 / tree.size().width().max(tree.size().height());
        let mut transform = tiny_skia::Transform::from_scale(scale, scale);
        if let Some(ink) = ink {
            if let Some((x, y, width, height)) = ink_bounds(&render(&tree, drawable, transform)?) {
                // Scale the ink to `ink` pixels and centre it in the drawable square.
                let fit = ink / width.max(height);
                let centre = drawable as f32 / 2.0;
                transform = transform
                    .post_translate(-(x + width / 2.0), -(y + height / 2.0))
                    .post_scale(fit, fit)
                    .post_translate(centre, centre);
            }
        }
        // Drawing into the inner square clips artwork that strays outside it,
        // so the border really stays transparent.
        let artwork = render(&tree, drawable, transform)?;
        let mut pixmap = tiny_skia::Pixmap::new(ICON_SIZE, ICON_SIZE).ok_or("unable to create pixmap")?;
        pixmap.draw_pixmap(inset as i32, inset as i32, artwork.as_ref(), &tiny_skia::PixmapPaint::default(),
            tiny_skia::Transform::identity(), None);
        pixmap.save_png(output.join(path.file_stem().unwrap()).with_extension("png"))?;
        count += 1;
    }
    println!("Rendered {count} supplied SVG icons");
    Ok(())
}

fn render(tree: &usvg::Tree, side: u32, transform: tiny_skia::Transform) -> Result<tiny_skia::Pixmap, Box<dyn std::error::Error>> {
    let mut pixmap = tiny_skia::Pixmap::new(side, side).ok_or("unable to create pixmap")?;
    resvg::render(tree, transform, &mut pixmap.as_mut());
    Ok(pixmap)
}

/// Bounding box of the visible pixels as (x, y, width, height).
fn ink_bounds(pixmap: &tiny_skia::Pixmap) -> Option<(f32, f32, f32, f32)> {
    let width = pixmap.width();
    let (mut left, mut top, mut right, mut bottom) = (u32::MAX, u32::MAX, 0, 0);
    for (index, pixel) in pixmap.pixels().iter().enumerate() {
        if pixel.alpha() < INK_ALPHA { continue; }
        let (x, y) = (index as u32 % width, index as u32 / width);
        left = left.min(x); top = top.min(y); right = right.max(x); bottom = bottom.max(y);
    }
    (left <= right).then(|| (left as f32, top as f32, (right - left + 1) as f32, (bottom - top + 1) as f32))
}
