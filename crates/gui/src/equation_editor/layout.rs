//! Math box layout for the visual equation editor.
//!
//! It follows Typst's conventions (New Computer Modern Math, italic letters,
//! upright operators, spacing around relations and binary operators) closely
//! enough to feel like the page, and records the geometry of every row and
//! caret stop so clicks and arrow keys can map to editing positions.
use super::tree::{AtomKind, Item, Kind, Node, Path, Row};
use std::sync::OnceLock;
use ttf_parser::{Face, GlyphId};

/// Extra horizontal target reach relative to a populated slot's height.
const ROW_END_REACH_RATIO: f32 = 0.45;
/// Padding for empty placeholders relative to their largest dimension.
const EMPTY_SLOT_REACH_RATIO: f32 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect { pub x: f32, pub y: f32, pub w: f32, pub h: f32 }

impl Rect {
    pub fn contains(&self, x: f32, y: f32) -> bool { x >= self.x && x <= self.x + self.w && y >= self.y && y <= self.y + self.h }
    fn moved(self, dx: f32, dy: f32) -> Self { Rect { x: self.x + dx, y: self.y + dy, ..self } }
    fn inflated(self, by: f32) -> Self { Rect { x: self.x - by, y: self.y - by, w: self.w + 2.0 * by, h: self.h + 2.0 * by } }
    fn distance(&self, x: f32, y: f32) -> f32 {
        let dx = (self.x - x).max(0.0).max(x - self.x - self.w);
        let dy = (self.y - y).max(0.0).max(y - self.y - self.h);
        (dx * dx + dy * dy).sqrt()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ink { Normal, Muted, Accent }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FontId { Math, Text }

#[derive(Clone, Debug, PartialEq)]
pub enum Draw {
    /// A glyph with its baseline origin at (x, y), `size` pixels per em.
    Glyph { font: FontId, id: u16, x: f32, y: f32, size: f32, ink: Ink },
    Rule { rect: Rect, ink: Ink },
    /// Stroked polyline.
    Line { points: Vec<(f32, f32)>, width: f32, ink: Ink },
    /// An empty slot. `active` when the caret is in it.
    Placeholder { rect: Rect, active: bool, faint: bool },
    /// Background of a command being typed or of a kept-whole construct.
    Chip { rect: Rect, ink: Ink },
}

/// Geometry of one editable row.
#[derive(Clone, Debug)]
pub struct RowGeometry {
    pub path: Path,
    pub rect: Rect,
    /// Caret x for every offset (len + 1 entries).
    pub carets: Vec<f32>,
    /// Caret top and bottom.
    pub caret_y: (f32, f32),
}

#[derive(Clone, Debug, Default)]
pub struct MathBox {
    pub w: f32,
    pub asc: f32,
    pub desc: f32,
    pub draws: Vec<Draw>,
    pub rows: Vec<RowGeometry>,
}

impl MathBox {
    fn place(&mut self, other: MathBox, dx: f32, dy: f32) {
        for mut d in other.draws {
            match &mut d {
                Draw::Glyph { x, y, .. } => { *x += dx; *y += dy; }
                Draw::Rule { rect, .. } | Draw::Placeholder { rect, .. } | Draw::Chip { rect, .. } => *rect = rect.moved(dx, dy),
                Draw::Line { points, .. } => for p in points { p.0 += dx; p.1 += dy; },
            }
            self.draws.push(d);
        }
        for mut row in other.rows {
            row.rect = row.rect.moved(dx, dy);
            for x in &mut row.carets { *x += dx; }
            row.caret_y = (row.caret_y.0 + dy, row.caret_y.1 + dy);
            self.rows.push(row);
        }
    }
    pub fn height(&self) -> f32 { self.asc + self.desc }

    /// Nearest editing position to (x, y): the innermost row under the
    /// point, or the nearest row. An empty slot's square is a small target
    /// (a script or a limit is smaller still), so a click near one counts as
    /// on it; of two candidates as deep, the nearer wins.
    pub fn hit(&self, x: f32, y: f32) -> Option<(Path, usize)> {
        // A nested row (under a root, in an exponent…) also takes clicks a
        // little past its end: a click just after "27" in √(3+27) means its
        // end, to type on there, not the place after the root.
        let reach = |r: &RowGeometry| {
            let empty = r.carets.len() == 1 && r.rect.w > 0.0;
            if empty { r.rect.inflated(EMPTY_SLOT_REACH_RATIO * r.rect.w.max(r.rect.h)) }
            else if !r.path.is_empty() { Rect { w: r.rect.w + ROW_END_REACH_RATIO * r.rect.h, ..r.rect } }
            else { r.rect }
        };
        let row = self.rows.iter().filter(|r| {
            reach(r).contains(x, y) && self.inside_node(r, x, y)
        })
            .max_by(|a, b| a.path.len().cmp(&b.path.len()).then(b.rect.distance(x, y).total_cmp(&a.rect.distance(x, y))))
            // Blank space belongs to the equation's outer row, never to a
            // nearby denominator or exponent just because it is closest.
            .or_else(|| self.row(&[]))?;
        let offset = row.carets.iter().enumerate()
            .min_by(|a, b| (a.1 - x).abs().total_cmp(&(b.1 - x).abs())).map_or(0, |(i, _)| i);
        Some((row.path.clone(), offset))
    }

    /// A slot's larger click target must stay inside its owning structure.
    fn inside_node(&self, row: &RowGeometry, x: f32, y: f32) -> bool {
        let Some((&(index, _), parent)) = row.path.split_last() else { return true; };
        let Some(parent) = self.row(parent) else { return false; };
        let (Some(left), Some(right)) = (parent.carets.get(index), parent.carets.get(index + 1)) else { return false; };
        x >= left.min(*right) && x <= left.max(*right)
            && y >= parent.rect.y && y <= parent.rect.y + parent.rect.h
    }

    pub fn row(&self, path: &[(usize, usize)]) -> Option<&RowGeometry> { self.rows.iter().find(|r| r.path == path) }
}

// ---- Fonts ---------------------------------------------------------------

pub struct Fonts { math: Face<'static>, text: Face<'static> }

impl Fonts {
    /// Typst's own math and text fonts, embedded in the application.
    pub fn get() -> Option<&'static Fonts> {
        static FONTS: OnceLock<Option<Fonts>> = OnceLock::new();
        FONTS.get_or_init(|| {
            let mut math = None; let mut text = None;
            for data in typst_assets::fonts() {
                let Ok(face) = Face::parse(data, 0) else { continue; };
                let name = face.names().into_iter().filter(|n| n.name_id == ttf_parser::name_id::POST_SCRIPT_NAME)
                    .find_map(|n| n.to_string()).unwrap_or_default();
                match name.as_str() {
                    "NewCMMath-Regular" => math = Some(face),
                    "NewCM10-Regular" => text = Some(face),
                    _ => {}
                }
            }
            Some(Fonts { math: math?, text: text? })
        }).as_ref()
    }
    pub fn face(&self, font: FontId) -> &Face<'static> { match font { FontId::Math => &self.math, FontId::Text => &self.text } }
    fn glyph(&self, font: FontId, c: char) -> Option<(FontId, u16)> {
        self.face(font).glyph_index(c).map(|g| (font, g.0))
            .or_else(|| self.math.glyph_index(c).map(|g| (FontId::Math, g.0)))
            .or_else(|| self.text.glyph_index(c).map(|g| (FontId::Text, g.0)))
    }
    fn advance(&self, font: FontId, id: u16, size: f32) -> f32 {
        let face = self.face(font);
        face.glyph_hor_advance(GlyphId(id)).unwrap_or(0) as f32 * size / face.units_per_em() as f32
    }
    /// Ink extent above and below the baseline.
    fn extent(&self, font: FontId, id: u16, size: f32) -> (f32, f32) {
        let face = self.face(font);
        let scale = size / face.units_per_em() as f32;
        face.glyph_bounding_box(GlyphId(id)).map_or((0.0, 0.0), |b| (b.y_max as f32 * scale, -(b.y_min as f32) * scale))
    }
}

/// Typst draws single letters in math italic.
fn math_italic(c: char) -> char {
    let shifted = |base: u32, from: char| char::from_u32(base + (c as u32 - from as u32)).unwrap_or(c);
    match c {
        'h' => 'ℎ',
        'a'..='z' => shifted(0x1D44E, 'a'),
        'A'..='Z' => shifted(0x1D434, 'A'),
        'α'..='ω' => shifted(0x1D6FC, 'α'),
        'ϵ' => '𝜖', 'ϑ' => '𝜗', 'ϕ' => '𝜙', 'ϱ' => '𝜚',
        c => c,
    }
}

// ---- Layout --------------------------------------------------------------

#[derive(Clone, Copy)]
struct Style { size: f32, level: u8 }

impl Style {
    fn script(self) -> Style { Style { size: (self.size * 0.7).max(self.base_min()), level: self.level + 1 } }
    fn fraction(self) -> Style { if self.level == 0 { Style { level: 1, ..self } } else { self.script() } }
    fn base_min(self) -> f32 { 7.0 }
    fn em(self, v: f32) -> f32 { self.size * v }
}

pub struct Layout<'a> { fonts: &'a Fonts, focus: &'a Path, show_optional: bool }

/// Lays out `root` at `size` pixels per em. `focus` is the cursor's row path;
/// structures on it show their empty optional slots.
pub fn layout(fonts: &Fonts, root: &Row, size: f32, focus: &Path) -> MathBox {
    let layout = Layout { fonts, focus, show_optional: false };
    layout.row(root, &Vec::new(), Style { size, level: 0 }, false)
}

/// A standalone preview, for the palette.
pub fn preview(fonts: &Fonts, row: &Row, size: f32) -> MathBox {
    let focus = vec![(usize::MAX, 0)];
    let layout = Layout { fonts, focus: &focus, show_optional: true };
    layout.row(row, &Vec::new(), Style { size, level: 0 }, false)
}

impl Layout<'_> {
    fn glyphs(&self, text: &str, font: FontId, italic: bool, style: Style, ink: Ink) -> MathBox {
        let mut b = MathBox { asc: style.em(0.68), desc: style.em(0.2), ..Default::default() };
        for c in text.chars() {
            let c = if italic { math_italic(c) } else { c };
            let Some((font, id)) = self.fonts.glyph(font, c) else { b.w += style.em(0.5); continue; };
            let (asc, desc) = self.fonts.extent(font, id, style.size);
            b.asc = b.asc.max(asc); b.desc = b.desc.max(desc);
            b.draws.push(Draw::Glyph { font, id, x: b.w, y: 0.0, size: style.size, ink });
            b.w += self.fonts.advance(font, id, style.size);
        }
        b
    }

    fn placeholder(&self, style: Style, active: bool, faint: bool) -> MathBox {
        let (w, asc, desc) = (style.em(0.62), style.em(0.72), style.em(0.16));
        MathBox { w, asc, desc, draws: vec![Draw::Placeholder { rect: Rect { x: 0.0, y: -asc, w, h: asc + desc }, active, faint }], rows: Vec::new() }
    }

    fn focused(&self, path: &[(usize, usize)]) -> bool { self.focus.len() >= path.len() && self.focus[..path.len()] == *path }

    /// Lays out a row and records its geometry. Empty rows draw a
    /// placeholder (faint for optional slots).
    fn row(&self, row: &Row, path: &Path, style: Style, optional: bool) -> MathBox {
        let mut out = MathBox { asc: style.em(0.68), desc: style.em(0.2), ..Default::default() };
        let active = self.focus == path;
        if row.is_empty() {
            if optional && !self.show_optional && !self.focused(path) {
                out.w = 0.0;
                out.rows.push(RowGeometry { path: path.clone(), rect: Rect { x: 0.0, y: -style.em(0.4), w: 0.0, h: style.em(0.6) }, carets: vec![0.0], caret_y: (-style.em(0.4), style.em(0.2)) });
                return out;
            }
            let p = self.placeholder(style, active, optional);
            let (w, asc, desc) = (p.w, p.asc, p.desc);
            out.place(p, 0.0, 0.0);
            out.w = w; out.asc = asc; out.desc = desc;
            out.rows.push(RowGeometry { path: path.clone(), rect: Rect { x: 0.0, y: -asc, w, h: asc + desc }, carets: vec![w * 0.5], caret_y: (-asc + style.em(0.06), desc - style.em(0.04)) });
            return out;
        }
        let mut carets = Vec::with_capacity(row.len() + 1);
        let mut previous: Option<(f32, f32)> = None; // (asc, desc) of the previous item, for scripts
        let mut previous_kind: Option<&Item> = None;
        let mut gap_before = 0.0;
        for (index, item) in row.iter().enumerate() {
            let (lead, trail) = spacing(previous_kind, item, style);
            let caret = out.w + (gap_before + lead) * 0.5;
            carets.push(caret);
            out.w += gap_before.max(0.0) + lead;
            let mut child_path = path.clone();
            child_path.push((index, 0));
            let b = self.item(item, &child_path, style, previous);
            out.asc = out.asc.max(b.asc); out.desc = out.desc.max(b.desc);
            previous = Some((b.asc, b.desc));
            let w = b.w;
            out.place(b, out.w, 0.0);
            out.w += w;
            gap_before = trail;
            previous_kind = Some(item);
        }
        carets.push(out.w + gap_before * 0.25);
        out.w += gap_before;
        let caret_y = (-style.em(0.78).max(out.asc.min(style.em(1.1))), style.em(0.24).max(out.desc.min(style.em(0.5))));
        out.rows.push(RowGeometry { path: path.clone(), rect: Rect { x: 0.0, y: -out.asc, w: out.w.max(1.0), h: out.asc + out.desc }, carets, caret_y });
        out
    }

    fn slot(&self, node: &Node, path: &Path, slot: usize, style: Style, optional: bool) -> MathBox {
        let mut slot_path = path.clone();
        slot_path.last_mut().expect("node path").1 = slot;
        self.row(&node.slots[slot], &slot_path, style, optional)
    }

    fn item(&self, item: &Item, path: &Path, style: Style, previous: Option<(f32, f32)>) -> MathBox {
        match item {
            Item::Atom(atom) => {
                let single = atom.text.chars().count() == 1;
                match atom.kind {
                    AtomKind::Ident if !single && super::tree::FUNCTIONS.contains(&atom.text.as_str()) =>
                        self.glyphs(&atom.text, FontId::Math, false, style, Ink::Normal),
                    AtomKind::Ident => {
                        let upright = atom.text.chars().all(|c| ('Α'..='Ω').contains(&c));
                        self.glyphs(&atom.text, FontId::Math, !upright, style, Ink::Normal)
                    }
                    AtomKind::Text => self.glyphs(&atom.text, FontId::Text, false, style, Ink::Normal),
                    _ => self.glyphs(&atom.text, FontId::Math, false, style, Ink::Normal),
                }
            }
            Item::Command(name) => {
                let label = self.glyphs(&format!("\\{name}"), FontId::Text, false, Style { size: style.size * 0.8, ..style }, Ink::Accent);
                let pad = style.em(0.12);
                let mut b = MathBox { w: label.w + 2.0 * pad, asc: label.asc.max(style.em(0.6)) + pad * 0.5, desc: label.desc + pad * 0.5, ..Default::default() };
                b.draws.push(Draw::Chip { rect: Rect { x: 0.0, y: -b.asc, w: b.w, h: b.asc + b.desc }, ink: Ink::Accent });
                b.place(label, pad, 0.0);
                b
            }
            Item::Opaque(node) => {
                let mut text = editor::equation_input::format(node);
                if text.starts_with("@ast") { text = "…".into(); }
                if text.chars().count() > 18 { text = text.chars().take(17).collect::<String>() + "…"; }
                let label = self.glyphs(&text, FontId::Text, false, Style { size: style.size * 0.75, ..style }, Ink::Muted);
                let pad = style.em(0.15);
                let mut b = MathBox { w: label.w + 2.0 * pad, asc: label.asc + pad, desc: label.desc + pad, ..Default::default() };
                b.draws.push(Draw::Chip { rect: Rect { x: 0.0, y: -b.asc, w: b.w, h: b.asc + b.desc }, ink: Ink::Muted });
                b.place(label, pad, 0.0);
                b
            }
            Item::Node(node) => self.node(node, path, style, previous),
        }
    }

    fn node(&self, node: &Node, path: &Path, style: Style, previous: Option<(f32, f32)>) -> MathBox {
        let axis = style.em(0.25);
        let rule = style.em(0.045).max(1.0);
        let mut b = MathBox::default();
        match &node.kind {
            Kind::Fraction => {
                let inner = style.fraction();
                let num = self.slot(node, path, 0, inner, false);
                let den = self.slot(node, path, 1, inner, false);
                // The bar overhangs its content; over a fraction it reaches
                // clearly past that fraction's own bar, so the main bar of
                // a stacked fraction reads as the main one.
                let holds_fraction = node.slots.iter().any(|slot| matches!(slot.as_slice(), [Item::Node(Node { kind: Kind::Fraction, .. })]));
                let margin = style.em(0.06);
                let overhang = style.em(if holds_fraction { 0.3 } else { 0.06 });
                let gap = style.em(0.14);
                b.w = num.w.max(den.w) + 2.0 * (margin + overhang);
                let num_base = -axis - rule * 0.5 - gap - num.desc;
                let den_base = -axis + rule * 0.5 + gap + den.asc;
                b.asc = -num_base + num.asc;
                b.desc = den_base + den.desc;
                b.draws.push(Draw::Rule { rect: Rect { x: margin, y: -axis - rule * 0.5, w: b.w - 2.0 * margin, h: rule }, ink: Ink::Normal });
                let (nw, dw) = (num.w, den.w);
                b.place(num, (b.w - nw) * 0.5, num_base);
                b.place(den, (b.w - dw) * 0.5, den_base);
            }
            Kind::Sqrt | Kind::Root => {
                let body_slot = if node.kind == Kind::Root { 1 } else { 0 };
                let body = self.slot(node, path, body_slot, style, false);
                let gap = style.em(0.14);
                let top = -(body.asc + gap);
                let bottom = body.desc;
                let height = bottom - top;
                let mut sign_x = 0.0;
                if node.kind == Kind::Root {
                    let index = self.slot(node, path, 0, style.script().script(), false);
                    let iw = index.w;
                    let base = top + height * 0.45 - index.desc;
                    b.asc = b.asc.max(-base + index.asc);
                    b.place(index, 0.0, base);
                    sign_x = (iw - style.em(0.28)).max(0.0);
                }
                let sign_w = style.em(0.55);
                let points = vec![
                    (sign_x, top + height * 0.62), (sign_x + sign_w * 0.22, top + height * 0.52),
                    (sign_x + sign_w * 0.5, bottom), (sign_x + sign_w, top), (sign_x + sign_w + body.w + style.em(0.1), top),
                ];
                b.draws.push(Draw::Line { points, width: rule, ink: Ink::Normal });
                let bw = body.w;
                b.place(body, sign_x + sign_w + style.em(0.05), 0.0);
                b.w = sign_x + sign_w + bw + style.em(0.15);
                b.asc = b.asc.max(-top + rule);
                b.desc = b.desc.max(bottom);
            }
            Kind::Sup | Kind::Sub | Kind::SubSup => {
                let (base_asc, base_desc) = previous.unwrap_or((style.em(0.68), style.em(0.2)));
                let script = style.script();
                let has_sub = node.kind != Kind::Sup;
                let has_sup = node.kind != Kind::Sub;
                let mut w: f32 = 0.0;
                b.w = style.em(0.04);
                if has_sup {
                    let slot = if node.kind == Kind::SubSup { 1 } else { 0 };
                    let sup = self.slot(node, path, slot, script, false);
                    let shift = style.em(0.42).max(base_asc - sup.asc * 0.55);
                    b.asc = b.asc.max(shift + sup.asc);
                    w = w.max(sup.w);
                    b.place(sup, b.w, -shift);
                }
                if has_sub {
                    let sub = self.slot(node, path, 0, script, false);
                    let shift = style.em(0.2).max(base_desc + sub.asc * 0.4 - style.em(0.2));
                    let shift = if has_sup { shift.max(style.em(0.26)) } else { shift };
                    b.desc = b.desc.max(shift + sub.desc);
                    b.asc = b.asc.max(sub.asc - shift);
                    w = w.max(sub.w);
                    b.place(sub, b.w, shift);
                }
                b.w += w + style.em(0.04);
            }
            Kind::Group => {
                let body = self.slot(node, path, 0, style, false);
                b.w = body.w; b.asc = body.asc; b.desc = body.desc;
                b.place(body, 0.0, 0.0);
            }
            Kind::Delimited { left, right } => {
                let body = self.slot(node, path, 0, style, false);
                self.delimit(&mut b, body, left, right, style);
            }
            Kind::Function(name) => {
                let label = self.glyphs(name, FontId::Math, false, style, Ink::Normal);
                let lw = label.w;
                b.asc = label.asc; b.desc = label.desc;
                b.place(label, 0.0, 0.0);
                let body = self.slot(node, path, 0, style, false);
                let mut paren = MathBox::default();
                self.delimit(&mut paren, body, "(", ")", style);
                b.asc = b.asc.max(paren.asc); b.desc = b.desc.max(paren.desc);
                let pw = paren.w;
                b.place(paren, lw + style.em(0.05), 0.0);
                b.w = lw + style.em(0.05) + pw;
            }
            Kind::Nary(operator) => {
                let (glyph, integral) = nary_glyph(operator);
                let large = Style { size: style.size * if integral { 1.75 } else { 1.45 }, ..style };
                let symbol = self.glyphs(glyph, FontId::Math, false, large, Ink::Normal);
                // Centre the symbol on the math axis.
                let shift = (symbol.asc - symbol.desc) * 0.5 - axis;
                let (sasc, sdesc, sw) = (symbol.asc - shift, symbol.desc + shift, symbol.w);
                let limits = style.script();
                let lower = self.slot(node, path, 0, limits, true);
                let upper = self.slot(node, path, 1, limits, true);
                let gap = style.em(0.08);
                let x0;
                if integral {
                    b.place(symbol, 0.0, shift);
                    let (lw, uw) = (lower.w, upper.w);
                    let up_base = -sasc + upper.asc * 0.6;
                    let low_base = sdesc - lower.desc * 0.3;
                    b.asc = sasc.max(-up_base + upper.asc);
                    b.desc = sdesc.max(low_base + lower.desc);
                    b.place(upper, sw * 0.9, up_base);
                    b.place(lower, sw * 0.55, low_base);
                    x0 = (sw * 0.9 + uw).max(sw * 0.55 + lw);
                } else {
                    let width = sw.max(lower.w).max(upper.w);
                    let (lw, uw) = (lower.w, upper.w);
                    let up_base = -sasc - gap - upper.desc;
                    let low_base = sdesc + gap + lower.asc;
                    b.asc = if uw > 0.0 { -up_base + upper.asc } else { sasc };
                    b.desc = if lw > 0.0 { low_base + lower.desc } else { sdesc };
                    b.place(symbol, (width - sw) * 0.5, shift);
                    b.place(upper, (width - uw) * 0.5, up_base);
                    b.place(lower, (width - lw) * 0.5, low_base);
                    x0 = width;
                }
                let body = self.slot(node, path, 2, style, false);
                b.asc = b.asc.max(body.asc); b.desc = b.desc.max(body.desc);
                let bw = body.w;
                b.place(body, x0 + style.em(0.16), 0.0);
                b.w = x0 + style.em(0.16) + bw;
            }
            Kind::Accent(accent) => {
                let body = self.slot(node, path, 0, style, false);
                let (w, asc) = (body.w, body.asc);
                let top = -asc - style.em(0.1);
                let mid = w * 0.5;
                let ink = Ink::Normal;
                let line = |points: Vec<(f32, f32)>| Draw::Line { points, width: rule, ink };
                let mark_w = (w * 0.8).clamp(style.em(0.3), style.em(0.6));
                match accent.as_str() {
                    "hat" | "\u{302}" => b.draws.push(line(vec![(mid - mark_w * 0.5, top), (mid, top - style.em(0.16)), (mid + mark_w * 0.5, top)])),
                    "overline" | "macron" | "\u{305}" | "\u{304}" => b.draws.push(Draw::Rule { rect: Rect { x: 0.0, y: top - rule, w, h: rule }, ink }),
                    "arrow" | "\u{20D7}" => {
                        let (y, x1) = (top - style.em(0.08), w.max(style.em(0.45)));
                        b.draws.push(line(vec![(0.0, y), (x1, y)]));
                        b.draws.push(line(vec![(x1 - style.em(0.12), y - style.em(0.08)), (x1, y), (x1 - style.em(0.12), y + style.em(0.08))]));
                    }
                    "tilde" | "\u{303}" => {
                        let points = (0..=12).map(|i| { let t = i as f32 / 12.0;
                            (mid - mark_w * 0.5 + t * mark_w, top - style.em(0.08) - (t * std::f32::consts::TAU).sin() * style.em(0.05)) }).collect();
                        b.draws.push(line(points));
                    }
                    "dot" | "dot.double" | "\u{307}" | "\u{308}" => {
                        let r = style.em(0.05);
                        let centres: Vec<f32> = if accent.contains("double") || accent == "\u{308}" { vec![mid - style.em(0.1), mid + style.em(0.1)] } else { vec![mid] };
                        for c in centres { b.draws.push(Draw::Rule { rect: Rect { x: c - r, y: top - style.em(0.08) - r, w: 2.0 * r, h: 2.0 * r }, ink }); }
                    }
                    other => {
                        let mark = self.glyphs(other, FontId::Math, false, Style { size: style.size * 0.8, ..style }, ink);
                        let mw = mark.w;
                        b.place(mark, mid - mw * 0.5, top);
                    }
                }
                b.w = w.max(style.em(0.45)); b.asc = asc + style.em(0.3); b.desc = body.desc;
                b.place(body, (b.w - w) * 0.5, 0.0);
            }
            Kind::Matrix { cols } => {
                let cols = (*cols).max(1);
                let cells: Vec<MathBox> = (0..node.slots.len()).map(|i| self.slot(node, path, i, style, false)).collect();
                let rows = cells.len() / cols;
                let col_w: Vec<f32> = (0..cols).map(|c| (0..rows).map(|r| cells[r * cols + c].w).fold(0.0, f32::max)).collect();
                let row_asc: Vec<f32> = (0..rows).map(|r| (0..cols).map(|c| cells[r * cols + c].asc).fold(0.0, f32::max)).collect();
                let row_desc: Vec<f32> = (0..rows).map(|r| (0..cols).map(|c| cells[r * cols + c].desc).fold(0.0, f32::max)).collect();
                let (col_gap, row_gap) = (style.em(0.8), style.em(0.3));
                let total_h: f32 = row_asc.iter().zip(&row_desc).map(|(a, d)| a + d).sum::<f32>() + row_gap * (rows.saturating_sub(1)) as f32;
                let total_w: f32 = col_w.iter().sum::<f32>() + col_gap * (cols - 1) as f32;
                let mut grid = MathBox { w: total_w, asc: total_h * 0.5 + axis, desc: total_h * 0.5 - axis, ..Default::default() };
                let mut y = -grid.asc;
                for (index, cell) in cells.into_iter().enumerate() {
                    let (r, c) = (index / cols, index % cols);
                    if c == 0 && r > 0 { y += row_desc[r - 1] + row_gap; }
                    let x: f32 = col_w[..c].iter().sum::<f32>() + col_gap * c as f32;
                    let cw = cell.w;
                    grid.place(cell, x + (col_w[c] - cw) * 0.5, y + row_asc[r]);
                    if c + 1 == cols { y += row_asc[r]; }
                }
                self.delimit(&mut b, grid, "(", ")", style);
            }
            Kind::Boxed => {
                let body = self.slot(node, path, 0, style, false);
                let pad = style.em(0.15);
                b.w = body.w + 2.0 * pad; b.asc = body.asc + pad; b.desc = body.desc + pad;
                let (w, h) = (b.w, b.asc + b.desc);
                b.draws.push(Draw::Line { points: vec![(0.0, -b.asc), (w, -b.asc), (w, h - b.asc), (0.0, h - b.asc), (0.0, -b.asc)], width: rule, ink: Ink::Normal });
                b.place(body, pad, 0.0);
            }
            Kind::Text => {
                let body = self.slot(node, path, 0, style, false);
                let pad = style.em(0.05);
                b.w = body.w + 2.0 * pad; b.asc = body.asc; b.desc = body.desc;
                b.place(body, pad, 0.0);
            }
        }
        b
    }

    /// Wraps `body` in stretchy delimiters drawn as strokes.
    fn delimit(&self, b: &mut MathBox, body: MathBox, left: &str, right: &str, style: Style) {
        let axis = style.em(0.25);
        let half = ((body.asc - axis).max(body.desc + axis) + style.em(0.1)).max(style.em(0.55));
        let (top, bottom) = (-axis - half, -axis + half);
        let width = style.em(0.3) + (half - style.em(0.55)).max(0.0) * 0.12;
        let stroke = style.em(0.05).max(1.0);
        let gap = style.em(0.06);
        let bw = body.w;
        b.draws.extend(delimiter(left, 0.0, width, top, bottom, stroke, false));
        b.place(body, width + gap, 0.0);
        let rx = width + gap + bw + gap;
        b.draws.extend(delimiter(right, rx, width, top, bottom, stroke, true));
        b.w = rx + width;
        b.asc = b.asc.max(-top); b.desc = b.desc.max(bottom);
    }
}

fn nary_glyph(operator: &str) -> (&'static str, bool) {
    match operator {
        "sum" | "∑" => ("∑", false), "product" | "prod" | "∏" => ("∏", false),
        "coproduct" | "∐" => ("∐", false), "union.big" | "⋃" => ("⋃", false), "sect.big" | "⋂" => ("⋂", false),
        "integral.double" | "∬" => ("∬", true), "integral.triple" | "∭" => ("∭", true),
        "integral.cont" | "∮" => ("∮", true), _ => ("∫", true),
    }
}

fn delimiter(kind: &str, x: f32, w: f32, top: f32, bottom: f32, width: f32, right: bool) -> Vec<Draw> {
    let ink = Ink::Normal;
    let flip = |px: f32| if right { x + w - px } else { x + px };
    let line = |points: Vec<(f32, f32)>| Draw::Line { points: points.into_iter().map(|(px, py)| (flip(px), py)).collect(), width, ink };
    let mid = (top + bottom) * 0.5;
    match kind {
        "(" | ")" => {
            let points = (0..=16).map(|i| {
                let t = i as f32 / 16.0; let a = (t - 0.5) * std::f32::consts::PI * 0.9;
                (w * 0.85 - a.cos() * w * 0.6, mid + a.sin() / (0.45 * std::f32::consts::PI).sin() * (bottom - top) * 0.5)
            }).collect();
            vec![line(points)]
        }
        "[" | "]" => vec![line(vec![(w * 0.75, top), (w * 0.3, top), (w * 0.3, bottom), (w * 0.75, bottom)])],
        "{" | "}" => {
            let h = bottom - top;
            let points = (0..=24).map(|i| {
                let t = i as f32 / 24.0;
                let y = top + t * h;
                let d = (t - 0.5).abs() * 2.0; // 0 at the middle point
                let bulge = if d < 0.12 { 0.2 + d / 0.12 * 0.25 } else { 0.45 + (1.0 - (1.0 - d).powi(2)) * 0.1 };
                let x = if d > 0.85 { 0.45 + (d - 0.85) / 0.15 * 0.35 } else { bulge };
                (w * x, y)
            }).collect();
            vec![line(points)]
        }
        "‖" => vec![line(vec![(w * 0.35, top), (w * 0.35, bottom)]), line(vec![(w * 0.65, top), (w * 0.65, bottom)])],
        "|" => vec![line(vec![(w * 0.5, top), (w * 0.5, bottom)])],
        "" | "." => Vec::new(),
        _ => vec![line(vec![(w * 0.5, top), (w * 0.5, bottom)])],
    }
}

/// Space before and after `item`: relations and binary operators breathe,
/// like Typst's math spacing.
fn spacing(previous: Option<&Item>, item: &Item, style: Style) -> (f32, f32) {
    if style.level > 1 { return (0.0, 0.0); }
    let factor = if style.level == 1 { 0.6 } else { 1.0 };
    match item {
        Item::Atom(a) if a.kind == AtomKind::Relation => (style.em(0.28) * factor, style.em(0.28) * factor),
        Item::Atom(a) if a.kind == AtomKind::Operator && matches!(a.text.as_str(), "," | ";") => (0.0, style.em(0.17) * factor),
        Item::Atom(a) if a.kind == AtomKind::Operator && !matches!(a.text.as_str(), "!" | "′" | "'") => {
            let binary = previous.is_some_and(|p| !matches!(p, Item::Atom(q) if matches!(q.kind, AtomKind::Operator | AtomKind::Relation)));
            if binary { (style.em(0.22) * factor, style.em(0.22) * factor) } else { (0.0, 0.0) }
        }
        Item::Atom(a) if a.kind == AtomKind::Ident && a.text.chars().count() > 1 => (style.em(0.12), style.em(0.12)),
        _ => (0.0, 0.0),
    }
}


#[cfg(test)] #[path = "../../tests/unit/equation_editor/layout.rs"] mod tests;
