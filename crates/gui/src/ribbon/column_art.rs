//! Small pictures of column layouts, shared by the Columns menu and the
//! Columns dialog: a page outline with grey text lines in each column and,
//! for "Line between", a rule in each gap.
use editor::ColumnLayout;
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap};

const PAPER: (u8, u8, u8) = (255, 255, 255);
const PAGE_EDGE: (u8, u8, u8) = (160, 170, 184);
const TEXT_LINE: (u8, u8, u8) = (150, 160, 175);
const RULE: (u8, u8, u8) = (60, 70, 86);
/// Share of the picture's width kept as page margin on each side.
const MARGIN_SHARE: f32 = 0.14;
/// Text line height and gap, as shares of the picture's height.
const LINE_SHARE: f32 = 0.045;
const LINE_STEP_SHARE: f32 = 0.11;

/// Each column's left edge and width as shares of the text width (0..1).
pub(super) fn bands(layout: &ColumnLayout) -> Vec<(f32, f32)> {
    // Any text width works: only the proportions are drawn.
    const NOMINAL_TEXT_WIDTH_MM: f32 = 160.0;
    let mut left = 0.0;
    layout.resolved(NOMINAL_TEXT_WIDTH_MM).iter().map(|column| {
        let band = (left / NOMINAL_TEXT_WIDTH_MM, column.width_mm / NOMINAL_TEXT_WIDTH_MM);
        left += column.width_mm + column.spacing_mm;
        band
    }).collect()
}

/// Draw the page picture into the rectangle `(x, y, width, height)` of `pixmap`.
pub(super) fn page(pixmap: &mut Pixmap, (x, y, width, height): (f32, f32, f32, f32), layout: &ColumnLayout) {
    fill(pixmap, x, y, width, height, PAGE_EDGE);
    fill(pixmap, x + 1.0, y + 1.0, width - 2.0, height - 2.0, PAPER);
    let margin = width * MARGIN_SHARE;
    let (text_left, text_width) = (x + margin, width - 2.0 * margin);
    let (top, bottom) = (y + height * MARGIN_SHARE, y + height * (1.0 - MARGIN_SHARE));
    let line = (height * LINE_SHARE).max(1.0);
    let step = (height * LINE_STEP_SHARE).max(line + 1.0);
    let columns = bands(layout);
    for &(left, share) in &columns {
        let mut line_top = top;
        while line_top + line <= bottom {
            fill(pixmap, text_left + left * text_width, line_top, (share * text_width).max(1.0), line, TEXT_LINE);
            line_top += step;
        }
    }
    if layout.line_between {
        for pair in columns.windows(2) {
            let gap_middle = text_left + ((pair[0].0 + pair[0].1) + pair[1].0) / 2.0 * text_width;
            fill(pixmap, gap_middle - 0.5, top, 1.0, bottom - top, RULE);
        }
    }
}

fn fill(pixmap: &mut Pixmap, x: f32, y: f32, width: f32, height: f32, (r, g, b): (u8, u8, u8)) {
    let mut paint = Paint::default();
    paint.set_color_rgba8(r, g, b, 255);
    if let Some(rect) = tiny_skia::Rect::from_xywh(x, y, width.max(0.5), height.max(0.5)) {
        pixmap.fill_path(&PathBuilder::from_rect(rect), &paint, FillRule::Winding, crate::crisp::transform(), None);
    }
}

