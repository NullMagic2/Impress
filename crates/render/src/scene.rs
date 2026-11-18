use crate::{Affine2D, Color, Point, Rect, Size, VectorPath};
use std::ops::Range;

#[derive(Clone, Debug, PartialEq)]
pub enum Paint {
    Solid(Color),
}

#[derive(Clone, Debug, PartialEq)]
pub struct StrokeStyle {
    pub width_pt: f32,
}

impl Default for StrokeStyle {
    fn default() -> Self { Self { width_pt: 1.0 } }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PathObject {
    pub path: VectorPath,
    pub fill: Option<Paint>,
    pub stroke: Option<(Paint, StrokeStyle)>,
    pub transform: Affine2D,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GlyphVector {
    pub glyph_id: u32,
    /// Shared by every placement of the same glyph at the same size, so the
    /// scene stores each shape once and renderers can cache it by identity.
    pub outline: std::sync::Arc<VectorPath>,
    pub transform: Affine2D,
    /// UTF-8 byte cluster relative to the source text, when the run came from
    /// editable prose. Typst-created equation glyphs may leave this unset.
    pub cluster: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextSource {
    pub node_id: String,
    pub byte_range: Range<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VectorTextRun {
    /// Logical displayed Unicode, distinct from source offsets (e.g. page fields).
    pub actual_text: Option<String>,
    pub source: Option<TextSource>,
    pub font_family: String,
    pub font_size_pt: f32,
    pub fill: Paint,
    pub glyphs: Vec<GlyphVector>,
    pub bounds: Rect,
    pub advance_pt: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DrawCommand {
    /// Derived rectangular clipping, shared by screen/PDF/print. Nested table
    /// clips intersect instead of replacing their parent cell's content bounds.
    Clipped { rect: Rect, command: std::sync::Arc<DrawCommand> },
    FillRect { rect: Rect, paint: Paint },
    Path(PathObject),
    /// Shared so that pages, caches and scene snapshots copy runs cheaply.
    TextRun(std::sync::Arc<VectorTextRun>),
    /// Original-resolution raster; never tessellated into colored pixel tiles.
    Image { rect: Rect, image: std::sync::Arc<crate::RasterImage> },
    /// Layout-only recipe for a plain picture. Page materialization resolves
    /// it before delivering a scene to a renderer.
    EncodedImage { rect: Rect, data: std::sync::Arc<Vec<u8>> },
}

impl DrawCommand {
    pub fn clipped(self, rect: Rect) -> Self {
        match self {
            Self::Clipped { rect: inner, command } => Self::Clipped { rect:rect.intersection(inner),command },
            command => Self::Clipped { rect, command:std::sync::Arc::new(command) },
        }
    }
    pub fn unclipped(&self) -> (&Self, Option<Rect>) {
        let mut command=self; let mut clip:Option<Rect>=None;
        while let Self::Clipped {rect,command:inner}=command {
            clip=Some(clip.map_or(*rect,|r|r.intersection(*rect))); command=inner;
        }
        (command,clip)
    }

    pub fn translated(&self, dx: f32, dy: f32) -> Self {
        let t = Affine2D::translation(dx, dy);
        match self {
            Self::Clipped { rect, command } => command.translated(dx,dy).clipped(Rect::new(rect.x+dx,rect.y+dy,rect.width,rect.height)),
            Self::Image { rect, image } => Self::Image {
                rect: Rect::new(rect.x + dx, rect.y + dy, rect.width, rect.height),
                image: image.clone(),
            },
            Self::EncodedImage { rect, data } => Self::EncodedImage {
                rect: Rect::new(rect.x + dx, rect.y + dy, rect.width, rect.height),
                data: data.clone(),
            },
            Self::FillRect { rect, paint } => Self::FillRect {
                rect: Rect::new(rect.x + dx, rect.y + dy, rect.width, rect.height),
                paint: paint.clone(),
            },
            Self::Path(path) => Self::Path(PathObject {
                path: path.path.clone(),
                fill: path.fill.clone(),
                stroke: path.stroke.clone(),
                transform: path.transform.then(t),
            }),
            Self::TextRun(run) => {
                let glyphs = run.glyphs.iter().map(|g| GlyphVector {
                    glyph_id: g.glyph_id,
                    outline: g.outline.clone(),
                    transform: g.transform.then(t),
                    cluster: g.cluster,
                }).collect();
                Self::TextRun(std::sync::Arc::new(VectorTextRun {
                    actual_text: run.actual_text.clone(),
                    source: run.source.clone(),
                    font_family: run.font_family.clone(),
                    font_size_pt: run.font_size_pt,
                    fill: run.fill.clone(),
                    glyphs,
                    bounds: Rect::new(run.bounds.x + dx, run.bounds.y + dy, run.bounds.width, run.bounds.height),
                    advance_pt: run.advance_pt,
                }))
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CaretStop {
    pub byte_offset: usize,
    pub x_pt: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextHitRegion {
    pub node_id: String,
    /// Zero-based visual line within the semantic paragraph. Multiple hit
    /// regions (mixed formatting, inline math, spaces) share this value.
    pub visual_line: usize,
    pub byte_range: Range<usize>,
    pub rect: Rect,
    pub caret_stops: Vec<CaretStop>,
}

impl TextHitRegion {
    pub fn clipped(mut self, clip:Rect) -> Option<Self> {
        self.rect=self.rect.intersection(clip);
        if self.rect.is_empty(){return None;}
        for caret in &mut self.caret_stops {caret.x_pt=caret.x_pt.clamp(self.rect.x,self.rect.max_x());}
        Some(self)
    }

    pub fn translated(&self, dx: f32, dy: f32) -> Self {
        Self {
            node_id: self.node_id.clone(),
            visual_line: self.visual_line,
            byte_range: self.byte_range.clone(),
            rect: Rect::new(self.rect.x + dx, self.rect.y + dy, self.rect.width, self.rect.height),
            caret_stops: self.caret_stops.iter().map(|s| CaretStop { byte_offset: s.byte_offset, x_pt: s.x_pt + dx }).collect(),
        }
    }

    pub fn nearest_byte_offset(&self, x_pt: f32) -> usize {
        self.caret_stops.iter()
            .min_by(|a, b| (a.x_pt - x_pt).abs().total_cmp(&(b.x_pt - x_pt).abs()))
            .map(|s| s.byte_offset)
            .unwrap_or(self.byte_range.start)
    }
}

/// Semantic object geometry in page points. This is ephemeral layout data, not
/// a second document model. Stable IDs address edits even when the caret moves.
#[derive(Clone, Debug, PartialEq)]
pub struct TableCellHit {
    pub table_id: String,
    pub row_index: usize,
    pub cell_index: usize,
    pub column_index: usize,
    pub column_span: usize,
    pub row_span: usize,
    pub column_widths_pt: Vec<f32>,
    pub row_height_pt: f32,
    pub can_resize_row: bool,
}
#[derive(Clone, Debug, PartialEq)]
pub enum ObjectKind {
    Picture { source: Option<(String, usize)> },
    TextBox { source: Option<(String, usize)> },
    EmbeddedObject { source: Option<(String, usize)> },
    Equation { source: Option<(String, usize)> },
    TableCell(TableCellHit),
    CommentThread { comment_id:String },
    HeaderFooter { section_id: String, header: bool, variant: u8 },
}
#[derive(Clone, Debug, PartialEq)]
pub struct ObjectHitRegion {
    pub node_id: String,
    pub rect: Rect,
    pub kind: ObjectKind,
}
impl ObjectHitRegion {
    pub fn translated(&self, dx: f32, dy: f32) -> Self {
        Self { node_id: self.node_id.clone(), kind: self.kind.clone(),
            rect: Rect::new(self.rect.x + dx, self.rect.y + dy, self.rect.width, self.rect.height) }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReviewCard { pub comment_id:String, pub rect:Rect, pub commands:Vec<DrawCommand> }

#[derive(Clone, Debug, PartialEq)]
pub struct VectorPage {
    pub size: Size,
    /// Physical page margins in page points, independent of paragraph indents,
    /// columns, table cells and header/footer reservations. Screen rulers use
    /// this layout metadata rather than guessing from the current caret.
    pub margin_rect: Option<Rect>,
    /// Display value of a PAGE field; independent of the physical page index.
    pub page_number_label: Option<String>,
    /// A geometry-only page still participates in scrolling and navigation.
    pub has_paint: bool,
    pub commands: Vec<DrawCommand>,
    pub hit_regions: Vec<TextHitRegion>,
    pub object_regions: Vec<ObjectHitRegion>,
    /// Screen-only review furniture, kept separate from document/output geometry.
    pub review_cards: Vec<ReviewCard>,
}

impl VectorPage {
    pub fn new(size: Size) -> Self { Self { size, margin_rect:None, page_number_label:None, has_paint:true, commands: Vec::new(), hit_regions: Vec::new(), object_regions: Vec::new(), review_cards:Vec::new() } }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VectorDocument {
    pub pages: Vec<VectorPage>,
    /// Each paragraph's place in the document (by node id). Pages alone do
    /// not give it: when a table row breaks across pages, a cell's later
    /// paragraph appears after the next cell's text. Empty: page order.
    pub text_order: std::sync::Arc<std::collections::HashMap<String, usize>>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VectorFragment {
    pub size: Size,
    /// Baseline measured from the top of the fragment. Inline objects align
    /// this value with the surrounding text baseline.
    pub baseline_pt: f32,
    pub commands: Vec<DrawCommand>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewTransform {
    pub zoom: f32,
    pub scroll: Point,
    pub viewport: Size,
}

impl ViewTransform {
    pub fn page_to_view(&self) -> Affine2D {
        Affine2D::translation(-self.scroll.x, -self.scroll.y).then(Affine2D::scale(self.zoom, self.zoom))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OverlayRect {
    pub page_index: usize,
    pub rect: Rect,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ColoredOverlayRect {pub area:OverlayRect,pub color:Color}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CanvasOverlay {
    pub selections: Vec<OverlayRect>,
    pub caret: Option<OverlayRect>,
    /// Handles and outlines are painted after document content.
    pub adornments: Vec<OverlayRect>,
    pub review_marks: Vec<ColoredOverlayRect>,
    /// Interactive controls drawn over the page (table handles and the
    /// like), in view pixels so they keep their size at every zoom.
    pub chrome: Vec<ChromeShape>,
}

/// Anti-aliased overlay primitive in view pixels.
#[derive(Clone, Debug, PartialEq)]
pub enum ChromeShape {
    RoundRect { rect: Rect, radius: f32, fill: Option<Color>, stroke: Option<(Color, f32)> },
    Circle { center: Point, radius: f32, fill: Option<Color>, stroke: Option<(Color, f32)> },
    Line { from: Point, to: Point, width: f32, color: Color },
    /// A straight line drawn as dashes; `dash` is the length of each dash and
    /// of each gap between dashes.
    DashedLine { from: Point, to: Point, width: f32, color: Color, dash: f32 },
}

impl ChromeShape {
    /// View-pixel area the shape can touch.
    pub fn bounds(&self) -> Rect {
        match self {
            ChromeShape::RoundRect { rect, stroke, .. } => {
                let m = stroke.map_or(0.0, |s| s.1) + 1.0;
                Rect::new(rect.x - m, rect.y - m, rect.width + 2.0 * m, rect.height + 2.0 * m)
            }
            ChromeShape::Circle { center, radius, stroke, .. } => {
                let r = radius + stroke.map_or(0.0, |s| s.1) + 1.0;
                Rect::new(center.x - r, center.y - r, 2.0 * r, 2.0 * r)
            }
            ChromeShape::Line { from, to, width, .. } | ChromeShape::DashedLine { from, to, width, .. } => {
                let m = width + 1.0;
                Rect::new(from.x.min(to.x) - m, from.y.min(to.y) - m, (from.x - to.x).abs() + 2.0 * m, (from.y - to.y).abs() + 2.0 * m)
            }
        }
    }
}

