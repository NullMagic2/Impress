//! Paragraph-indent ruler handles; dragging produces a semantic property patch.
use super::*;
use wxdragon::{
    BrushStyle, DeviceContext, PenStyle,
    dc::{AutoBufferedPaintDC, Point, PolygonFillMode},
    widgets::panel::Panel,
};

/// Neutral handles remain visible against the white ruler.
const HANDLE_INK: Colour = Colour::rgb(0, 0, 0);
const HANDLE_FILL: Colour = Colour::rgb(255, 255, 255);
const HALF_WIDTH_DIP: i32 = 5;
const HIT_WIDTH_DIP: i32 = 7;
const FIRST_TOP_DIP: i32 = 2;
const FIRST_BOTTOM_DIP: i32 = 10;
const HANGING_TOP_DIP: i32 = 15;
const HANGING_BOTTOM_DIP: i32 = 23;
const BOX_TOP_DIP: i32 = 24;
const BOX_HEIGHT_DIP: i32 = 7;
/// Keep a usable text width while moving either boundary.
const MIN_TEXT_WIDTH_MM: f32 = 5.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Indents {
    pub left: f32,
    pub right: f32,
    pub first: f32,
}
impl Indents {
    pub fn from_properties(properties: &ParagraphProperties) -> Self {
        Self {
            left: properties.left_indent_mm.unwrap_or(0.0),
            right: properties.right_indent_mm.unwrap_or(0.0),
            first: properties.first_line_indent_mm.unwrap_or(0.0),
        }
    }
    pub fn position(self, handle: Handle, width: f32) -> f32 {
        match handle {
            Handle::FirstLine => self.left + self.first,
            Handle::Hanging | Handle::Left => self.left,
            Handle::Right => width - self.right,
        }
    }
    pub fn moved(self, handle: Handle, position: f32, width: f32) -> Self {
        let mut after = self;
        let right = (width - self.right).max(MIN_TEXT_WIDTH_MM);
        let first = (self.left + self.first).clamp(0.0, right);
        match handle {
            Handle::FirstLine => after.first = position.clamp(0.0, right) - self.left,
            Handle::Hanging => {
                after.left = position.clamp(0.0, (right - MIN_TEXT_WIDTH_MM).max(0.0));
                after.first = first - after.left;
            }
            Handle::Left => {
                after.left = position.clamp(
                    (-self.first).max(0.0),
                    (right - MIN_TEXT_WIDTH_MM - self.first.max(0.0)).max((-self.first).max(0.0)),
                );
            }
            Handle::Right => {
                after.right = width
                    - position.clamp(
                        (self.left + self.first.max(0.0) + MIN_TEXT_WIDTH_MM).min(width),
                        width,
                    )
            }
        }
        after
    }
    pub fn patch(self, handle: Handle) -> ParagraphProperties {
        let mut patch = ParagraphProperties::default();
        match handle {
            Handle::FirstLine => patch.first_line_indent_mm = Some(self.first),
            Handle::Hanging => {
                patch.left_indent_mm = Some(self.left);
                patch.first_line_indent_mm = Some(self.first);
            }
            Handle::Left => patch.left_indent_mm = Some(self.left),
            Handle::Right => patch.right_indent_mm = Some(self.right),
        }
        patch
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Handle {
    FirstLine,
    Hanging,
    Left,
    Right,
}
impl Handle {
    pub fn label(self) -> &'static str {
        match self {
            Self::FirstLine => "First-line indent",
            Self::Hanging => "Hanging indent",
            Self::Left => "Left indent",
            Self::Right => "Right indent",
        }
    }
}

pub(super) fn hit(
    panel: &Panel,
    indents: Indents,
    start: f32,
    pixels_per_mm: f32,
    width: f32,
    x: i32,
    y: i32,
) -> Option<Handle> {
    [
        Handle::Left,
        Handle::FirstLine,
        Handle::Hanging,
        Handle::Right,
    ]
    .into_iter()
    .find(|&handle| {
        let center = start + indents.position(handle, width) * pixels_per_mm;
        let (top, bottom) = match handle {
            Handle::FirstLine => (FIRST_TOP_DIP, FIRST_BOTTOM_DIP),
            Handle::Hanging | Handle::Right => (HANGING_TOP_DIP, HANGING_BOTTOM_DIP),
            Handle::Left => (BOX_TOP_DIP, BOX_TOP_DIP + BOX_HEIGHT_DIP),
        };
        (x as f32 - center).abs() <= panel.from_dip_int(HIT_WIDTH_DIP) as f32
            && y >= panel.from_dip_int(top)
            && y <= panel.from_dip_int(bottom)
    })
}
pub(super) fn paint(
    dc: &AutoBufferedPaintDC,
    panel: &Panel,
    indents: Indents,
    start: f32,
    pixels_per_mm: f32,
    width: f32,
    active: Option<Handle>,
) {
    let d = |v| panel.from_dip_int(v);
    for handle in [
        Handle::FirstLine,
        Handle::Hanging,
        Handle::Left,
        Handle::Right,
    ] {
        let x = (start + indents.position(handle, width) * pixels_per_mm).round() as i32;
        let color = HANDLE_INK;
        dc.set_pen(color, d(1).max(1), PenStyle::Solid);
        dc.set_brush(
            if active == Some(handle) {
                color
            } else {
                HANDLE_FILL
            },
            BrushStyle::Solid,
        );
        let half = d(HALF_WIDTH_DIP);
        if handle == Handle::Left {
            dc.draw_rectangle(x - half, d(BOX_TOP_DIP), half * 2 + 1, d(BOX_HEIGHT_DIP));
            continue;
        }
        let (top, bottom) = if handle == Handle::FirstLine {
            (d(FIRST_TOP_DIP), d(FIRST_BOTTOM_DIP))
        } else {
            (d(HANGING_TOP_DIP), d(HANGING_BOTTOM_DIP))
        };
        let points = if handle == Handle::FirstLine {
            vec![
                Point::new(x - half, top),
                Point::new(x + half, top),
                Point::new(x, bottom),
            ]
        } else {
            vec![
                Point::new(x, top),
                Point::new(x + half, bottom),
                Point::new(x - half, bottom),
            ]
        };
        dc.draw_polygon(&points, 0, 0, PolygonFillMode::Winding);
    }
}
