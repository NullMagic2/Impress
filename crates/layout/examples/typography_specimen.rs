use model::{Block, Document, KerningMode, Paragraph, StyleId, STYLE_NORMAL};
use layout::LayoutEngine;
use output::raster::BandRenderer;
use render::Affine2D;
use std::sync::atomic::AtomicBool;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("output PNG path required")?;
    let family = std::env::args().nth(2);
    let kerning_mode = match std::env::args().nth(3).as_deref() {
        Some("no-kern") => KerningMode::Off,
        Some("metrics") => KerningMode::Metrics,
        _ => KerningMode::Optical,
    };
    let mut document = Document::default();
    let lines = [
        "Typography and the printed page",
        "AVATAR  To Wa  Te  Yo  FA  PA  VA  We  Ty",
        "office efficient affinity official flexible waffle",
        "A refined page balances letters, words, and white space. The familiar forms of a serif face should look composed, never crowded or loose. Kerning quietly brings AV, To, and Wa together.",
        "“Quotation marks,” commas, periods, and parentheses (like these) test the margins. Subtle spacing matters most where letters meet at an angle.",
        "0123456789  —  Café naïve façade  —  fi fl ffi ffl",
    ];
    document.sections[0].blocks = lines.into_iter().enumerate().map(|(i, text)| {
        let mut paragraph = Paragraph::plain(text);
        if i == 0 {
            paragraph.style = Some(StyleId::from("builtin.title"));
        } else if i == 1 || i == 2 {
            paragraph.properties.space_before_pt = Some(7.0);
        } else {
            paragraph.properties.space_before_pt = Some(10.0);
        }
        Block::Paragraph(paragraph)
    }).collect::<Vec<_>>().into();
    let normal = document.styles.styles.get_mut(&StyleId::from(STYLE_NORMAL)).unwrap();
    if let Some(family) = family { normal.character.font_family = Some(family); }
    let typography=normal.character.typography.as_mut().unwrap();
    typography.kerning_mode=kerning_mode;
    typography.kerning=kerning_mode!=KerningMode::Off;
    normal.character.font_size_pt = Some(12.0);
    let scene = LayoutEngine::default().layout_document(&document)?;
    let page = &scene.pages[0];
    let scale = 2.0;
    let width = (page.size.width * scale).ceil() as u32;
    let height = (page.size.height * scale).ceil() as u32;
    let mut pixels = BandRenderer::default().render(page, width, height, Affine2D::scale(scale, scale), &AtomicBool::new(false))?;
    for pixel in pixels.chunks_exact_mut(4) { pixel.swap(0, 2); }
    image::save_buffer(path, &pixels, width, height, image::ColorType::Rgba8)?;
    Ok(())
}
