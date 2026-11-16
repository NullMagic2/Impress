//! Renderer boundary for positioned text.
//!
//! Layout uses floating-point point units all the way to this boundary.
//! Typography hands off shaped glyph runs, but Impress's preferred rendering path
//! is vector: a downstream renderer should turn those glyph IDs into outlines and
//! keep them as path geometry in the page scene. Screen presentation eventually
//! reaches a pixel framebuffer because displays are pixel grids, but the Impress
//! page model and display list remain vector.

use crate::ShapedRun;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextOrigin {
    pub x_pt: f32,
    pub baseline_y_pt: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextRenderIntent {
    Screen,
    Print,
    PdfVector,
}

/// Rendering backend contract. `font_key` is deliberately opaque to the
/// document model; it belongs to a resolved font cache owned by typography/UI.
pub trait GlyphRunRenderer {
    type FontKey;
    type Error;

    fn draw_run(
        &mut self,
        font_key: &Self::FontKey,
        run: &ShapedRun,
        origin: TextOrigin,
        intent: TextRenderIntent,
    ) -> Result<(), Self::Error>;
}
