//! Compile equations into vector fragments with their mathematical baseline.
use model::Equation;
use format_typst::equation_to_typst;
use render::{
    extract_glyph_outline, Affine2D, Color, DrawCommand, GlyphVector, Paint as ImpressPaint,
    PathObject, PathVerb, Point, Rect, Size, StrokeStyle, VectorFragment, VectorPath, VectorTextRun,
};
use thiserror::Error;
use typst::layout::{Frame, FrameItem};
use typst_layout::PagedDocument;
use typst::visualize::{CurveItem, Geometry};
use typst_as_lib::{TypstEngine, typst_kit_options::TypstKitFontOptions};

#[derive(Debug, Error)]
pub enum TypesetError {
    #[error("Typst compilation failed: {0}")]
    Compilation(String),
    #[error("Typst produced no page for the equation")]
    EmptyOutput,
    #[error("Typst vector conversion failed: {0}")]
    Vector(String),
}

/// The main bar of a fraction over or under another fraction reaches well
/// past the inner bars, so a stacked fraction reads at a glance. The equation
/// editor draws the same overhang.
const NESTED_FRACTIONS: &str = "#show math.frac: it => if it.num.func() == math.frac or it.denom.func() == math.frac { let pad = h(0.24em); math.frac(pad + it.num + pad, pad + it.denom + pad) } else { it }";

/// Inline equations draw fractions as LaTeX's `\dfrac` does: the outermost
/// fraction's numerator and denominator are full size instead of Typst's
/// inline 70% (and 50% for a fraction within it), so they read at text size.
/// Fractions in scripts stay small. Only rendering changes, not the source.
fn readable_inline(equation: &Equation) -> std::borrow::Cow<'_, Equation> {
    use model::{EquationDisplay, EquationNode as N};
    fn walk(node: &N) -> N {
        let b = |n: &N| Box::new(walk(n));
        match node {
            N::Fraction { .. } => N::RawTypst { source: format!("display({})", format_typst::node_to_typst(node)) },
            N::Row { children } => N::Row { children: children.iter().map(walk).collect() },
            N::Delimited { left, body, right } => N::Delimited { left: left.clone(), body: b(body), right: right.clone() },
            N::Function { name, argument } => N::Function { name: name.clone(), argument: b(argument) },
            N::Accent { accent, body } => N::Accent { accent: accent.clone(), body: b(body) },
            N::Boxed { body } => N::Boxed { body: b(body) },
            N::Nary { operator, body, subscript, superscript } =>
                N::Nary { operator: operator.clone(), body: b(body), subscript: subscript.clone(), superscript: superscript.clone() },
            other => other.clone(),
        }
    }
    if equation.display == EquationDisplay::Block { return std::borrow::Cow::Borrowed(equation); }
    let mut readable = equation.clone();
    readable.root = walk(&equation.root);
    std::borrow::Cow::Owned(readable)
}

/// The rendering backend for both first-class equations and full-document
/// Typst previews. It lives outside every file-format adapter.
pub struct TypstTypesetter {
    include_system_fonts: bool,
}

impl Default for TypstTypesetter {
    fn default() -> Self { Self { include_system_fonts: true } }
}

impl TypstTypesetter {
    pub fn new() -> Self { Self::default() }

    pub fn with_system_fonts(mut self, enabled: bool) -> Self {
        self.include_system_fonts = enabled;
        self
    }

    pub fn render_equation_vector(&self, equation: &Equation, font_size_pt: f32) -> Result<VectorFragment, TypesetError> {
        let document = self.compile_equation(equation, font_size_pt)?;
        let page = document.pages().first().ok_or(TypesetError::EmptyOutput)?;
        frame_to_vector(&page.frame)
    }

    pub fn render_equation_svg(&self, equation: &Equation, font_size_pt: f32) -> Result<String, TypesetError> {
        let document = self.compile_equation(equation, font_size_pt)?;
        let page = document.pages().first().ok_or(TypesetError::EmptyOutput)?;
        Ok(typst_svg::svg(page, &Default::default()))
    }

    pub fn compile_document_svg_pages(&self, source: impl Into<String>) -> Result<Vec<String>, TypesetError> {
        let engine = self.engine(source.into());
        let compiled = engine.compile::<PagedDocument>();
        let document = compiled.output.map_err(|errors| TypesetError::Compilation(format!("{errors:#?}")))?;
        Ok(document.pages().iter().map(|page| typst_svg::svg(page, &Default::default())).collect())
    }

    fn compile_equation(&self, equation: &Equation, font_size_pt: f32) -> Result<PagedDocument, TypesetError> {
        let math = equation_to_typst(&readable_inline(equation));
        // A hard box preserves the inline math baseline when Typst flattens
        // the surrounding line into the page frame. Without it, the page's
        // bottom edge is mistaken for the equation baseline.
        let math = if equation.display == model::EquationDisplay::Inline {
            format!("#box[{math}]")
        } else {
            math
        };
        let source = format!(
            "#set page(width: auto, height: auto, margin: 2pt, fill: none)\n#set text(size: {font_size_pt}pt, top-edge: \"bounds\", bottom-edge: \"bounds\")\n{NESTED_FRACTIONS}\n{math}\n"
        );
        let compiled = self.engine(source).compile::<PagedDocument>();
        compiled.output.map_err(|errors| TypesetError::Compilation(format!("{errors:#?}")))
    }

    fn engine(&self, source: String) -> TypstEngine<typst_as_lib::TypstTemplateMainFile> {
        TypstEngine::builder()
            .main_file(source)
            .search_fonts_with(
                TypstKitFontOptions::default()
                    .include_system_fonts(self.include_system_fonts)
                    .include_embedded_fonts(true),
            )
            .build()
    }
}

fn frame_to_vector(frame: &Frame) -> Result<VectorFragment, TypesetError> {
    let size = frame.size();
    let mut commands = Vec::new();
    walk_frame(frame, Affine2D::IDENTITY, &mut commands)?;
    Ok(VectorFragment {
        size: Size::new(size.x.to_pt() as f32, size.y.to_pt() as f32),
        baseline_pt: content_baseline(frame).unwrap_or(frame.baseline().to_pt()) as f32,
        commands,
    })
}

/// The baseline of the first line in `frame`, from its top. A page frame has
/// no baseline of its own (Typst reports its bottom edge), so an inline
/// equation set on it sat too high by its depth and the page margin, which
/// showed plainly next to a fraction.
fn content_baseline(frame: &Frame) -> Option<f64> {
    if frame.has_baseline() { return Some(frame.baseline().to_pt()); }
    frame.items().find_map(|(position, item)| match item {
        FrameItem::Group(group) if group.transform.is_identity() =>
            content_baseline(&group.frame).map(|baseline| position.y.to_pt() + baseline),
        _ => None,
    })
}

fn walk_frame(frame: &Frame, parent: Affine2D, commands: &mut Vec<DrawCommand>) -> Result<(), TypesetError> {
    for (position, item) in frame.items() {
        let positioned = Affine2D::translation(position.x.to_pt() as f32, position.y.to_pt() as f32).then(parent);
        match item {
            FrameItem::Group(group) => {
                let tr = group.transform;
                let local = Affine2D {
                    sx: tr.sx.get() as f32,
                    kx: tr.kx.get() as f32,
                    tx: tr.tx.to_pt() as f32,
                    ky: tr.ky.get() as f32,
                    sy: tr.sy.get() as f32,
                    ty: tr.ty.to_pt() as f32,
                };
                walk_frame(&group.frame, local.then(positioned), commands)?;
            }
            FrameItem::Text(text) => {
                let size_pt = text.size.to_pt() as f32;
                let font_data = text.font.data().as_slice();
                let face_index = text.font.index();
                let mut pen_x = 0.0_f32;
                let mut pen_y = 0.0_f32;
                let mut glyphs = Vec::with_capacity(text.glyphs.len());

                for glyph in &text.glyphs {
                    let outline = extract_glyph_outline(font_data, face_index, glyph.id as u32)
                        .map_err(|e| TypesetError::Vector(e.to_string()))?;
                    let local_outline = outline.scale_to_points(size_pt);
                    let gx = pen_x + glyph.x_offset.at(text.size).to_pt() as f32;
                    // Typst glyph y offsets are y-up. Our page space is y-down.
                    let gy = pen_y - glyph.y_offset.at(text.size).to_pt() as f32;
                    glyphs.push(GlyphVector {
                        glyph_id: glyph.id as u32,
                        outline: std::sync::Arc::new(local_outline),
                        transform: Affine2D::translation(gx, gy).then(positioned),
                        cluster: None,
                    });
                    pen_x += glyph.x_advance.at(text.size).to_pt() as f32;
                    pen_y -= glyph.y_advance.at(text.size).to_pt() as f32;
                }

                let bbox = text.bbox();
                let bbox_size = bbox.size();
                commands.push(DrawCommand::TextRun(std::sync::Arc::new(VectorTextRun {
                    actual_text: None,
                    source: None,
                    font_family: text.font.info().family.to_string(),
                    font_size_pt: size_pt,
                    fill: ImpressPaint::Solid(Color::BLACK),
                    glyphs,
                    bounds: Rect::new(
                        positioned.tx,
                        positioned.ty - size_pt,
                        bbox_size.x.to_pt() as f32,
                        bbox_size.y.to_pt() as f32,
                    ),
                    advance_pt: text.width().to_pt() as f32,
                })));
            }
            FrameItem::Shape(shape, _) => {
                let path = geometry_to_path(&shape.geometry);
                let stroke = shape.stroke.as_ref().map(|s| {
                    (ImpressPaint::Solid(Color::BLACK), StrokeStyle { width_pt: s.thickness.to_pt() as f32 })
                });
                commands.push(DrawCommand::Path(PathObject {
                    path,
                    fill: shape.fill.as_ref().map(|_| ImpressPaint::Solid(Color::BLACK)),
                    stroke,
                    transform: positioned,
                }));
            }
            FrameItem::Image(_, _, _) | FrameItem::Link(_, _) | FrameItem::Tag(_) => {}
        }
    }
    Ok(())
}

fn geometry_to_path(geometry: &Geometry) -> VectorPath {
    match geometry {
        Geometry::Line(to) => VectorPath { verbs: vec![
            PathVerb::MoveTo(Point::new(0.0, 0.0)),
            PathVerb::LineTo(Point::new(to.x.to_pt() as f32, to.y.to_pt() as f32)),
        ]},
        Geometry::Rect(size) => {
            let w = size.x.to_pt() as f32;
            let h = size.y.to_pt() as f32;
            VectorPath { verbs: vec![
                PathVerb::MoveTo(Point::new(0.0, 0.0)),
                PathVerb::LineTo(Point::new(w, 0.0)),
                PathVerb::LineTo(Point::new(w, h)),
                PathVerb::LineTo(Point::new(0.0, h)),
                PathVerb::Close,
            ]}
        }
        Geometry::Curve(curve) => {
            let verbs = curve.0.iter().map(|item| match item {
                CurveItem::Move(p) => PathVerb::MoveTo(Point::new(p.x.to_pt() as f32, p.y.to_pt() as f32)),
                CurveItem::Line(p) => PathVerb::LineTo(Point::new(p.x.to_pt() as f32, p.y.to_pt() as f32)),
                CurveItem::Cubic(a, b, c) => PathVerb::CubicTo {
                    ctrl1: Point::new(a.x.to_pt() as f32, a.y.to_pt() as f32),
                    ctrl2: Point::new(b.x.to_pt() as f32, b.y.to_pt() as f32),
                    to: Point::new(c.x.to_pt() as f32, c.y.to_pt() as f32),
                },
                CurveItem::Close => PathVerb::Close,
            }).collect();
            VectorPath { verbs }
        }
    }
}

#[cfg(test)] #[path = "../tests/unit/lib.rs"] mod tests;
