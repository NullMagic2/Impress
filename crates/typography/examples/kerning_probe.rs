//! Inspect actual shaped positions in a supplied font: cargo run -p
//! typography --example kerning_probe -- FONT.ttf "AVATAR To Wa"
use model::KerningMode;
use typography::{shape_text, ShapeOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let font = std::fs::read(args.next().ok_or("pass a font path")?)?;
    let text = args.next().unwrap_or_else(|| "AVATAR To Wa Te Yo FA PA VA We Ty".into());
    for (name, mode) in [("off", KerningMode::Off), ("metrics", KerningMode::Metrics),
        ("optical", KerningMode::Optical)] {
        let mut options = ShapeOptions::default();
        options.size_pt = 40.0;
        options.typography.kerning_mode = mode;
        options.typography.kerning = mode != KerningMode::Off;
        let run = shape_text(&font, 0, &text, &options)?;
        println!("mode\t{name}\t{:.4}", run.advance_pt);
        let mut x = 0.0;
        for glyph in &run.glyphs {
            println!("glyph\t{name}\t{}\t{}\t{:.4}\t{:.4}",
                glyph.cluster, glyph.glyph_id, x, glyph.x_advance_pt);
            x += glyph.x_advance_pt;
        }
    }
    Ok(())
}
