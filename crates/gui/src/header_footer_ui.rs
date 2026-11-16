//! Canvas chrome while a header or footer is being edited.
//!
//! The body text of every page is veiled so the header and footer stand out.
//! Each header and footer band of the edited section gets a dashed boundary
//! line: in the variant's colour for the stream holding the caret, grey for
//! the others. The status bar names the stream in words.
use crate::*;
use editor::HeaderFooterContext;
use model::{FieldKind, HeaderFooterVariant};
use render::{ChromeShape, Color, ObjectKind, Point, Rect};

/// Whitening laid over the body text while a header or footer is edited.
const BODY_VEIL: Color = Color { r: 1.0, g: 1.0, b: 1.0, a: 0.55 };
/// Boundary of the band that does not hold the caret.
const INACTIVE_BOUNDARY: Color = Color { r: 0.56, g: 0.60, b: 0.66, a: 1.0 };
/// Yaru accent colours, one per variant, so the three streams are told apart
/// at a glance: orange for the default, aubergine for the first page and
/// blue for even pages.
const DEFAULT_VARIANT_COLOR: Color = Color { r: 233.0 / 255.0, g: 84.0 / 255.0, b: 32.0 / 255.0, a: 1.0 };
const FIRST_VARIANT_COLOR: Color = Color { r: 119.0 / 255.0, g: 33.0 / 255.0, b: 111.0 / 255.0, a: 1.0 };
const EVEN_VARIANT_COLOR: Color = Color { r: 0.0, g: 115.0 / 255.0, b: 229.0 / 255.0, a: 1.0 };

// Sizes in device-independent pixels; `Scale` converts them for the screen.
const BOUNDARY_WIDTH_DIP: f32 = 1.0;
const BOUNDARY_DASH_DIP: f32 = 4.0;

fn variant_color(variant: HeaderFooterVariant) -> Color {
    match variant {
        HeaderFooterVariant::Default => DEFAULT_VARIANT_COLOR,
        HeaderFooterVariant::First => FIRST_VARIANT_COLOR,
        HeaderFooterVariant::Even => EVEN_VARIANT_COLOR,
    }
}

/// The field behind a Ribbon field button: 0 page number, 1 page count,
/// 2 today's date. The date is fixed when inserted (ISO 8601, UTC).
pub fn field(index: u8) -> (FieldKind, Option<String>) {
    match index {
        0 => (FieldKind::PageNumber, None),
        1 => (FieldKind::PageCount, None),
        _ => (FieldKind::Date { format: None }, Some(today_iso())),
    }
}

fn today_iso() -> String {
    const SECONDS_PER_DAY: u64 = 86_400;
    let seconds = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (year, month, day) = civil_from_days((seconds / SECONDS_PER_DAY) as i64);
    format!("{year:04}-{month:02}-{day:02}")
}

/// Gregorian date of a day count since 1970-01-01 (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_index + 2) / 5 + 1) as u32;
    let month = if month_index < 10 { month_index + 3 } else { month_index - 9 } as u32;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// Header/footer chrome for this frame; empty unless a stream is edited.
pub fn chrome(state: &CanvasState) -> Vec<ChromeShape> {
    let mut out = Vec::new();
    if state.display_generation != Some(state.layout_generation) { return out; }
    let Some(context) = state.editor.header_footer_context() else { return out; };
    let scale = Scale::of(state);
    for page in 0..state.vector_document.pages.len() {
        let Some(origin) = state.viewport.page_rect_px(&state.vector_document, page) else { continue; };
        let bands = Bands::on_page(state, page);
        if let Some(body) = bands.body() {
            out.push(ChromeShape::RoundRect { rect: scale.to_view(origin, body), radius: 0.0, fill: Some(BODY_VEIL), stroke: None });
        }
        for band in bands.of_section(&context.section_id.0) {
            draw_band(&mut out, &context, band, origin, &scale);
        }
    }
    out
}

/// One header or footer hit band of a page, in page points.
struct Band<'a> {
    section_id: &'a str,
    header: bool,
    variant: HeaderFooterVariant,
    rect: Rect,
}

struct Bands<'a> {
    bands: Vec<Band<'a>>,
    page_width: f32,
}

impl<'a> Bands<'a> {
    fn on_page(state: &'a CanvasState, page: usize) -> Self {
        let vector_page = &state.vector_document.pages[page];
        let bands = vector_page.object_regions.iter().filter_map(|region| match &region.kind {
            ObjectKind::HeaderFooter { section_id, header, variant } => Some(Band {
                section_id, header: *header, variant: HeaderFooterVariant::from_index(*variant), rect: region.rect,
            }),
            _ => None,
        }).collect();
        Self { bands, page_width: vector_page.size.width }
    }

    /// The body area between the header and footer bands, across the page.
    fn body(&self) -> Option<Rect> {
        let top = self.bands.iter().find(|band| band.header)?.rect.max_y();
        let bottom = self.bands.iter().find(|band| !band.header)?.rect.y;
        (bottom > top).then(|| Rect::new(0.0, top, self.page_width, bottom - top))
    }

    fn of_section(&self, section_id: &str) -> impl Iterator<Item = &Band<'a>> {
        let section_id = section_id.to_owned();
        self.bands.iter().filter(move |band| band.section_id == section_id)
    }
}

fn draw_band(out: &mut Vec<ChromeShape>, context: &HeaderFooterContext, band: &Band, origin: Rect, scale: &Scale) {
    let active = band.header == context.header && band.variant == context.variant;
    let boundary_color = if active { variant_color(band.variant) } else { INACTIVE_BOUNDARY };
    // The boundary is the band edge that faces the body text.
    let boundary_y = if band.header { band.rect.max_y() } else { band.rect.y };
    let y = origin.y + boundary_y * scale.points_to_pixels;
    out.push(ChromeShape::DashedLine {
        from: Point::new(origin.x, y), to: Point::new(origin.x + origin.width, y),
        width: scale.dip(BOUNDARY_WIDTH_DIP), color: boundary_color, dash: scale.dip(BOUNDARY_DASH_DIP),
    });
}

/// Converts page points and device-independent pixels to view pixels.
struct Scale {
    points_to_pixels: f32,
    dip_to_pixels: f32,
}

impl Scale {
    fn of(state: &CanvasState) -> Self {
        Self {
            points_to_pixels: state.viewport.points_to_pixels(),
            dip_to_pixels: (state.viewport.dpi / 96.0).max(1.0),
        }
    }

    fn dip(&self, value: f32) -> f32 { value * self.dip_to_pixels }

    fn to_view(&self, origin: Rect, rect: Rect) -> Rect {
        let s = self.points_to_pixels;
        Rect::new(origin.x + rect.x * s, origin.y + rect.y * s, rect.width * s, rect.height * s)
    }
}

