//! Vector drawings (charts, diagrams, logos drawn with shapes) become
//! pictures, as in Word's conversion. Painted shapes close together form a
//! group; a group with real shapes in it, rather than only table rules,
//! cell shading or a frame around running text, is rendered as a picture,
//! together with the labels written over it.
use crate::extract::{Area, Line, Mark, PageText, Picture};
use crate::render::{each_page, png};
use std::sync::Arc;

/// Resolution of rendered drawings.
const RENDER_DPI: f32 = 150.0;
/// Shapes this close (points) belong to one drawing.
const JOIN_PT: f64 = 6.0;
/// A rule (line, underline, table border) is at most this thick.
const RULE_PT: f64 = 2.0;
/// Smallest drawing kept, in points.
const MIN_DRAWING: (f64, f64) = (36.0, 24.0);
/// Space kept around a rendered drawing.
const PADDING_PT: f64 = 3.0;
/// More text than this inside a frame means a text box, not a drawing.
const MAX_LABEL_CHARS: usize = 400;
/// Longest line taken for a label of a drawing.
const MAX_LABEL_LINE_CHARS: usize = 60;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role { Rule, Shading, Shape }

/// Finds the drawings of every page, renders them, and replaces the text
/// lines written over them.
pub(crate) fn convert(bytes: &[u8], pages: &mut [PageText], marks: &[Vec<Mark>]) {
    let found: Vec<Vec<Area>> = pages.iter().zip(marks).map(|(page, marks)| find(page, marks)).collect();
    let wanted: Vec<usize> = found.iter().enumerate().filter(|(_, d)| !d.is_empty()).map(|(i, _)| i).collect();
    let scale = f64::from(RENDER_DPI) / 72.0;
    let rendered = each_page(bytes, &wanted, RENDER_DPI, |index, image| {
        found[index].iter().filter_map(|area| {
            let x = (area.x * scale).max(0.0) as u32;
            let y = (area.y * scale).max(0.0) as u32;
            let width = ((area.width * scale) as u32).min(image.width().saturating_sub(x));
            let height = ((area.height * scale) as u32).min(image.height().saturating_sub(y));
            if width == 0 || height == 0 { return None; }
            png(&image::imageops::crop_imm(image, x, y, width, height).to_image()).map(|data| (*area, data))
        }).collect::<Vec<_>>()
    });
    for (index, drawings) in rendered {
        let page = &mut pages[index];
        for (area, data) in drawings {
            page.lines.retain(|line| !mostly_inside(line, &area));
            page.pictures.push(Picture { area, media_type: "image/png", data: Arc::new(data) });
        }
    }
}

fn line_area(line: &Line) -> Area { Area { x: line.x0, y: line.y - line.size, width: line.x1 - line.x0, height: line.size * 1.2 } }

fn overlap(a: &Area, b: &Area) -> f64 {
    let w = (a.right().min(b.right()) - a.x.max(b.x)).max(0.0);
    let h = (a.bottom().min(b.bottom()) - a.y.max(b.y)).max(0.0);
    w * h
}

fn mostly_inside(line: &Line, area: &Area) -> bool {
    let own = line_area(line);
    overlap(&own, area) >= own.width * own.height * 0.6
}

fn near(a: &Area, b: &Area) -> bool {
    a.x - JOIN_PT <= b.right() && b.x - JOIN_PT <= a.right() && a.y - JOIN_PT <= b.bottom() && b.y - JOIN_PT <= a.bottom()
}

fn union(a: &Area, b: &Area) -> Area {
    let (x, y) = (a.x.min(b.x), a.y.min(b.y));
    Area { x, y, width: a.right().max(b.right()) - x, height: a.bottom().max(b.bottom()) - y }
}

/// Shapes that touch or nearly touch, joined into groups: union-find over a
/// coarse grid, so pages with many shapes stay quick.
fn group(roles: &[(Area, Role)]) -> Vec<(Area, Vec<Role>)> {
    const CELL: f64 = 48.0;
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i { parent[i] = parent[parent[i]]; i = parent[i]; }
        i
    }
    let mut parent: Vec<usize> = (0..roles.len()).collect();
    let mut grid: std::collections::HashMap<(i64, i64), Vec<usize>> = std::collections::HashMap::new();
    for (index, (area, _)) in roles.iter().enumerate() {
        let cells = |from: f64, to: f64| ((from - JOIN_PT) / CELL).floor() as i64..=((to + JOIN_PT) / CELL).floor() as i64;
        for cx in cells(area.x, area.right()) {
            for cy in cells(area.y, area.bottom()) {
                let bucket = grid.entry((cx, cy)).or_default();
                for &other in bucket.iter() {
                    if near(area, &roles[other].0) { let (a, b) = (root(&mut parent, index), root(&mut parent, other)); parent[a] = b; }
                }
                bucket.push(index);
            }
        }
    }
    let mut groups: std::collections::HashMap<usize, (Area, Vec<Role>)> = std::collections::HashMap::new();
    for (index, (area, role)) in roles.iter().enumerate() {
        let key = root(&mut parent, index);
        let entry = groups.entry(key).or_insert((*area, Vec::new()));
        entry.0 = union(&entry.0, area);
        entry.1.push(*role);
    }
    groups.into_values().collect()
}

/// Areas of the page's drawings.
fn find(page: &PageText, marks: &[Mark]) -> Vec<Area> {
    let page_area = page.width * page.height;
    let roles: Vec<(Area, Role)> = marks.iter().filter(|m| !m.white && m.area.width * m.area.height < page_area * 0.7)
        // Shapes inside a picture are its frame or clip.
        .filter(|m| !page.pictures.iter().any(|p| overlap(&m.area, &p.area) >= m.area.width * m.area.height * 0.9))
        .map(|m| {
            let thin = m.area.width <= RULE_PT || m.area.height <= RULE_PT;
            let holds_text = page.lines.iter().any(|l| m.area.contains((l.x0 + l.x1) / 2.0, l.y - l.size * 0.3));
            let role = if thin && !m.curved { Role::Rule } else if !m.curved && holds_text { Role::Shading } else { Role::Shape };
            (m.area, role)
        }).collect();
    let groups = group(&roles);
    let drawings: Vec<Area> = groups.into_iter().filter_map(|(area, roles)| {
        let shapes = roles.iter().filter(|r| **r == Role::Shape).count();
        if shapes == 0 || area.width < MIN_DRAWING.0 || area.height < MIN_DRAWING.1 { return None; }
        // A single plain frame is a border or a text box, not a drawing.
        if roles.len() == 1 { return None; }
        // Labels just outside the shapes (titles, axis values) count too.
        let reach = Area { x: area.x - JOIN_PT, y: area.y - JOIN_PT, width: area.width + 2.0 * JOIN_PT, height: area.height + 2.0 * JOIN_PT };
        let inside: Vec<&Line> = page.lines.iter().filter(|l| overlap(&line_area(l), &reach) > 0.0).collect();
        let chars: usize = inside.iter().map(|l| l.text.chars().count()).sum();
        if chars > MAX_LABEL_CHARS { return None; }
        // Labels written over the drawing's edge go with it.
        let mut grown = area;
        // Text over the shapes is part of the drawing. Text just outside is
        // a label only when it is short and within the drawing's width, so
        // the paragraph before or after is never taken.
        let label = |l: &Line| {
            let own = line_area(l);
            let over = overlap(&own, &area) >= own.width * own.height * 0.5;
            let within = l.x0 >= area.x - JOIN_PT && l.x1 <= area.right() + JOIN_PT && l.text.chars().count() <= MAX_LABEL_LINE_CHARS;
            over || within
        };
        for line in inside.iter().filter(|l| label(l)) {
            grown = union(&grown, &line_area(line));
        }
        Some(Area { x: grown.x - PADDING_PT, y: grown.y - PADDING_PT, width: grown.width + 2.0 * PADDING_PT, height: grown.height + 2.0 * PADDING_PT })
    }).collect();
    merge_side_by_side(drawings, &page.lines)
}

/// Drawings beside each other on one band with no text between them are
/// parts of one figure (a diagram's boxes are often far apart).
fn merge_side_by_side(mut drawings: Vec<Area>, lines: &[Line]) -> Vec<Area> {
    drawings.sort_by(|a, b| a.x.total_cmp(&b.x));
    let mut out: Vec<Area> = Vec::new();
    for area in drawings {
        if let Some(last) = out.last_mut() {
            let shared = (last.bottom().min(area.bottom()) - last.y.max(area.y)).max(0.0);
            let between = Area { x: last.right(), y: last.y.max(area.y), width: (area.x - last.right()).max(0.0), height: shared };
            let text_between = lines.iter().any(|l| overlap(&line_area(l), &between) > 0.0);
            if shared >= last.height.min(area.height) * 0.5 && !text_between { *last = union(last, &area); continue; }
        }
        out.push(area);
    }
    out
}

