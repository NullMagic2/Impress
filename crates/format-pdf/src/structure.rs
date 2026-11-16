//! Page structure, as Word's PDF conversion recovers it: running headers
//! and footers, page columns and their reading order, tables, pictures, and
//! paragraphs with headings, alignment, indents and lists.
use crate::extract::{Line, PageText, Picture};
use crate::{ImportOptions, Kind, PdfSource};
use model::ListFormat;
use std::collections::{HashMap, HashSet};

/// Lines within this share of the page height from an edge may be running
/// headers or footers.
const RUNNING_BAND: f64 = 0.1;
/// A page column gutter is at least this wide (points).
const MIN_GUTTER_PT: f64 = 6.0;
/// Text at least this much larger than the body size is a heading.
const HEADING_SCALE: f64 = 1.18;
/// Longest text (characters) still taken for a heading.
const MAX_HEADING_CHARS: usize = 220;
/// A gap between lines this many times the usual line spacing starts a
/// paragraph (paragraph spacing is rarely under a quarter of a line).
const PARAGRAPH_GAP: f64 = 1.25;
/// Share of letters among the visible characters of a page column's text.
const COLUMN_LETTER_SHARE: f64 = 0.7;
/// Typical length (characters) of a page column's lines.
const MIN_COLUMN_LINE_CHARS: f64 = 20.0;
/// Most columns a table may have.
const MAX_TABLE_COLUMNS: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align { Left, Center, Right, Justify }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListMark { pub format: ListFormat, pub level: u8, pub start: i32 }

/// A paragraph rebuilt from lines. A list item's first line still begins
/// with its marker; `marker_chars` says how many characters to drop.
#[derive(Clone, Debug, PartialEq)]
pub struct Para {
    pub kind: Kind,
    pub lines: Vec<Line>,
    pub align: Align,
    /// Indents relative to the column's left edge, in points.
    pub left_indent: f64,
    pub first_indent: f64,
    pub list: Option<ListMark>,
    pub marker_chars: usize,
    pub page_break_before: bool,
    /// Character-weighted text size.
    pub size: f64,
    marker_x: f64,
}

/// A table: rows of cells, each cell a list of lines; column widths in points.
#[derive(Clone, Debug, PartialEq)]
pub struct Grid { pub rows: Vec<Vec<Vec<Line>>>, pub widths: Vec<f64>, pub page_break_before: bool }

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Para(Para),
    Table(Grid),
    Picture { picture: Picture, align: Align, page_break_before: bool },
    /// From here the text is set in `count` columns `gap` points apart
    /// (1 for full width).
    Columns { count: usize, gap: f64 },
    /// The PDF's column ended here before the bottom of its band.
    ColumnBreak,
}

impl Item {
    /// Marks a page break before this item; returns false for markers.
    fn set_page_break(&mut self) -> bool {
        match self {
            Item::Para(p) => p.page_break_before = true,
            Item::Table(t) => t.page_break_before = true,
            Item::Picture { page_break_before, .. } => *page_break_before = true,
            Item::Columns { .. } | Item::ColumnBreak => return false,
        }
        true
    }
}

/// A running header or footer line: its text, split around the page number
/// when it carries one.
#[derive(Clone, Debug, PartialEq)]
pub struct RunningLine { pub before: String, pub page_number: bool, pub after: String, pub align: Align, pub size: f64 }

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Analysis {
    pub items: Vec<Item>,
    pub body_size: f64,
    pub headers: Vec<RunningLine>,
    pub footers: Vec<RunningLine>,
    /// Extent of the body text over the selected pages: left, top, right, bottom.
    pub text_box: Option<(f64, f64, f64, f64)>,
    pub page_size: Option<(f64, f64)>,
}

fn median(mut values: Vec<f64>) -> Option<f64> {
    values.retain(|v| v.is_finite());
    if values.is_empty() { return None; }
    values.sort_by(|a, b| a.total_cmp(b));
    Some(values[values.len() / 2])
}

/// Text with digits folded, to recognise "Page 3" / "Page 4" as one header.
fn running_key(text: &str) -> String {
    text.chars().map(|c| if c.is_ascii_digit() { '#' } else { c.to_ascii_lowercase() }).collect::<String>()
}

pub(crate) fn is_page_number(text: &str) -> bool {
    let t = text.trim().to_ascii_lowercase();
    let t = t.trim_start_matches("page").trim();
    let t = t.trim_matches(|c: char| c == '-' || c == '–' || c == '—' || c.is_whitespace());
    !t.is_empty() && t.split(|c: char| c == '/' || c.is_whitespace() || c == 'o' || c == 'f').filter(|p| !p.is_empty())
        .all(|p| p.chars().all(|c| c.is_ascii_digit()) || matches!(p, "i" | "ii" | "iii" | "iv" | "v" | "vi" | "vii" | "viii" | "ix" | "x"))
}

const BULLETS: [char; 17] = ['•', '◦', '▪', '‣', '●', '○', '■', '□', '➢', '✓', '❖', '◆', '➔', '→', '·', '∙', '⁃'];
const ROMAN: [&str; 12] = ["i", "ii", "iii", "iv", "v", "vi", "vii", "viii", "ix", "x", "xi", "xii"];

/// A list marker at the start of `text`: its format, number, and the
/// characters it takes with the space after it.
pub(crate) fn list_marker(text: &str) -> Option<(ListFormat, i32, usize)> {
    let t = text;
    let first = t.chars().next()?;
    let spaced = |len: usize| t[len..].starts_with(' ').then(|| t[..len].chars().count() + 1);
    if BULLETS.contains(&first) {
        let len = first.len_utf8();
        let chars = t[..len].chars().count() + usize::from(t[len..].starts_with(' '));
        return (t.len() > len).then_some((ListFormat::Bullet, 1, chars));
    }
    if matches!(first, '-' | '–' | '*') { return spaced(first.len_utf8()).map(|n| (ListFormat::Bullet, 1, n)); }
    let (inner, open) = match t.strip_prefix('(') { Some(rest) => (rest, 1), None => (t, 0) };
    let token: String = inner.chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
    if token.is_empty() || token.len() > 4 { return None; }
    let close = inner[token.len()..].chars().next()?;
    let closes = if open == 1 { close == ')' } else { close == '.' || close == ')' };
    if !closes { return None; }
    let chars = spaced(open + token.len() + 1)?;
    if token.chars().all(|c| c.is_ascii_digit()) {
        return (token.len() <= 3).then(|| (ListFormat::Decimal, token.parse().unwrap_or(1), chars));
    }
    let lower = token.to_ascii_lowercase();
    if let Some(index) = ROMAN.iter().position(|r| *r == lower) {
        // "(i)" and "v)" are roman numerals; "i." is too ambiguous to take.
        let cased = token == lower || token == token.to_ascii_uppercase();
        if cased && (token.len() > 1 || close == ')') {
            let format = if token.chars().all(|c| c.is_ascii_uppercase()) { ListFormat::UpperRoman } else { ListFormat::LowerRoman };
            return Some((format, index as i32 + 1, chars));
        }
    }
    if token.len() == 1 && (close == ')' || open == 1) {
        let c = token.chars().next()?;
        let format = if c.is_ascii_uppercase() { ListFormat::UpperLetter } else { ListFormat::LowerLetter };
        return Some((format, (c.to_ascii_lowercase() as u8 - b'a') as i32 + 1, chars));
    }
    None
}

/// The pages to import, in order.
fn selected_pages(source: &PdfSource, options: &ImportOptions) -> Vec<usize> {
    match &options.pages {
        Some(pages) => pages.iter().copied().filter(|&p| p < source.pages.len()).collect(),
        None => (0..source.pages.len()).collect(),
    }
}

pub fn analyze_structure(source: &PdfSource, options: &ImportOptions) -> Analysis {
    let selected = selected_pages(source, options);
    let mut analysis = Analysis { page_size: selected.first().map(|&p| (source.pages[p].width, source.pages[p].height)), ..Analysis::default() };

    // Running headers and footers: lines near the top or bottom edge whose
    // text (digits folded) recurs on at least half of the pages, and page numbers.
    let mut recurring: HashMap<String, usize> = HashMap::new();
    if options.strip_running && source.pages.len() >= 2 {
        for page in &source.pages {
            let mut seen = HashSet::new();
            for line in page.lines.iter().filter(|l| in_running_band(page, l)) {
                let key = running_key(&line.text);
                if seen.insert(key.clone()) { *recurring.entry(key).or_default() += 1; }
            }
        }
    }
    let threshold = (source.pages.len() as f64 * 0.5).ceil().max(2.0) as usize;
    let is_running = |page: &PageText, line: &Line| options.strip_running && in_running_band(page, line)
        && (is_page_number(&line.text) || recurring.get(&running_key(&line.text)).is_some_and(|&n| n >= threshold));
    let mut header_keys = HashSet::new();
    let mut footer_keys = HashSet::new();
    for &p in &selected {
        let page = &source.pages[p];
        for line in page.lines.iter().filter(|l| is_running(page, l)) {
            let top = line.y < page.height * 0.5;
            let (keys, list) = if top { (&mut header_keys, &mut analysis.headers) } else { (&mut footer_keys, &mut analysis.footers) };
            if keys.insert(running_key(&line.text)) && list.len() < 3 { list.push(running_line(page, line)); }
        }
    }

    // Body size: the size carrying the most characters.
    let mut weights: HashMap<i64, usize> = HashMap::new();
    for &p in &selected {
        let page = &source.pages[p];
        for line in page.lines.iter().filter(|l| !is_running(page, l)) {
            for span in &line.spans { *weights.entry((span.size * 2.0).round() as i64).or_default() += span.text.chars().count(); }
        }
    }
    let body = weights.iter().max_by_key(|(size, n)| (**n, **size)).map_or(11.0, |(s, _)| *s as f64 / 2.0);
    analysis.body_size = body;

    let mut pages: Vec<Vec<Region>> = Vec::new();
    for &p in &selected {
        let page = &source.pages[p];
        let lines: Vec<&Line> = page.lines.iter().filter(|l| !is_running(page, l)).collect();
        // A page scan behind its own text layer is not a picture of the document.
        let has_text = !lines.is_empty();
        let pictures: Vec<&Picture> = page.pictures.iter()
            .filter(|pic| !(has_text && pic.area.width * pic.area.height > page.width * page.height * 0.8)).collect();
        for line in &lines {
            let b = analysis.text_box.get_or_insert((line.x0, line.y - line.size, line.x1, line.y));
            *b = (b.0.min(line.x0), b.1.min(line.y - line.size), b.2.max(line.x1), b.3.max(line.y + line.size * 0.25));
        }
        pages.push(regions(&lines, &pictures));
    }
    let mut items: Vec<Item> = Vec::new();
    let mut columns = 1;
    for (order, regions) in pages.iter().enumerate() {
        let first = items.len();
        for region in regions {
            // A change in the number of columns starts a new column section.
            if options.keep_columns && region.columns != columns {
                columns = region.columns;
                items.push(Item::Columns { count: region.columns, gap: region.gap });
            }
            items.extend(region_items(region, body));
            // Layout balances the last columns of a section itself; a column
            // the PDF ended early (a deliberate break) keeps its break.
            if options.keep_columns && region.column + 1 < region.columns && region.ends_early { items.push(Item::ColumnBreak); }
        }
        if order > 0 && options.page_breaks { if let Some(item) = items[first..].iter_mut().find(|i| !matches!(i, Item::Columns { .. } | Item::ColumnBreak)) { item.set_page_break(); } }
    }
    merge_across_pages(&mut items);
    assign_list_levels(&mut items);
    if options.headings { rank_headings(&mut items, body); }
    analysis.items = items;
    analysis
}

fn in_running_band(page: &PageText, line: &Line) -> bool {
    line.y < page.height * RUNNING_BAND || line.y > page.height * (1.0 - RUNNING_BAND)
}

fn horizontal_align(line: &Line, left: f64, right: f64) -> Align {
    let width = (right - left).max(1.0);
    let middle = (line.x0 + line.x1) / 2.0;
    if (middle - (left + right) / 2.0).abs() < width * 0.08 { Align::Center }
    else if line.x0 > left + width * 0.55 { Align::Right }
    else { Align::Left }
}

/// A running line split around its page number, if it has one.
fn running_line(page: &PageText, line: &Line) -> RunningLine {
    let align = horizontal_align(line, 0.0, page.width);
    if is_page_number(&line.text) {
        // "Page 3", "- 3 -", "3": keep the words around the number.
        let start = line.text.find(|c: char| c.is_ascii_digit());
        if let Some(start) = start {
            let end = line.text[start..].find(|c: char| !c.is_ascii_digit()).map_or(line.text.len(), |e| start + e);
            let after = &line.text[end..];
            // "page 2 of 9": the total stays as text; it changes rarely.
            return RunningLine { before: line.text[..start].to_string(), page_number: true, after: after.to_string(), align, size: line.size };
        }
        return RunningLine { before: String::new(), page_number: true, after: String::new(), align, size: line.size };
    }
    if let Some(start) = line.text.find(|c: char| c.is_ascii_digit()) {
        let end = line.text[start..].find(|c: char| !c.is_ascii_digit()).map_or(line.text.len(), |e| start + e);
        if end - start <= 4 && line.text[..start].to_ascii_lowercase().trim_end().ends_with("page") {
            return RunningLine { before: line.text[..start].to_string(), page_number: true, after: line.text[end..].to_string(), align, size: line.size };
        }
    }
    RunningLine { before: line.text.clone(), page_number: false, after: String::new(), align, size: line.size }
}

/// A column of a page between two full-width elements, in reading order.
struct Region<'a> {
    lines: Vec<&'a Line>, pictures: Vec<&'a Picture>, left: f64, right: f64,
    /// Columns of the band the region belongs to (1 for full width), its
    /// column, and the gap between the band's columns.
    columns: usize, column: usize, gap: f64,
    /// The column ends at least two lines above the bottom of its band.
    ends_early: bool,
    /// Left and right edges pictures are aligned against: the page's text
    /// for full-width regions, the column's for columns.
    frame: (f64, f64),
}

/// Rows of a page laid out in columns, between `top` and `bottom`, and the
/// x of each gutter.
struct Band { top: f64, bottom: f64, gutters: Vec<f64>, gap: f64 }

/// Column bands of a page. Consecutive rows (chunks on one baseline) that
/// share a wide gap, with running text on both sides, are in columns; the
/// full-width rows above, below and between bands are not, so one page may
/// mix both. Tables share gaps too, but their cells are far narrower than
/// the columns they would make.
fn column_bands(lines: &[&Line]) -> Vec<Band> {
    if lines.len() < 6 { return Vec::new(); }
    let mut sorted: Vec<&Line> = lines.to_vec();
    sorted.sort_by(|a, b| a.y.total_cmp(&b.y).then(a.x0.total_cmp(&b.x0)));
    let mut rows: Vec<Vec<&Line>> = Vec::new();
    for line in sorted {
        match rows.last_mut() {
            Some(row) if (row[0].y - line.y).abs() <= row[0].size.max(line.size) * 0.35 => row.push(line),
            _ => rows.push(vec![line]),
        }
    }
    let left = lines.iter().map(|l| l.x0).fold(f64::MAX, f64::min);
    let right = lines.iter().map(|l| l.x1).fold(f64::MIN, f64::max);
    let width = right - left;
    // Gaps between a row's chunks, away from the text edges.
    let gaps_of = |row: &[&Line]| -> Vec<(f64, f64)> {
        let mut chunks = row.to_vec();
        chunks.sort_by(|a, b| a.x0.total_cmp(&b.x0));
        chunks.windows(2).map(|w| (w[0].x1, w[1].x0))
            .filter(|&(a, b)| b - a >= MIN_GUTTER_PT && (a + b) / 2.0 > left + width * 0.15 && (a + b) / 2.0 < right - width * 0.15).collect()
    };
    let intersect = |common: &[(f64, f64)], gaps: &[(f64, f64)]| -> Vec<(f64, f64)> {
        common.iter().flat_map(|&(a, b)| gaps.iter().map(move |&(c, d)| (a.max(c), b.min(d)))).filter(|&(a, b)| b - a >= MIN_GUTTER_PT).collect()
    };
    let mut bands = Vec::new();
    let mut i = 0;
    while i < rows.len() {
        let mut common = gaps_of(&rows[i]);
        if common.is_empty() { i += 1; continue; }
        let mut end = i + 1;
        let mut with_gaps = 1;
        while end < rows.len() {
            if rows[end][0].y - rows[end - 1][0].y > rows[end][0].size.max(rows[end - 1][0].size) * 4.0 { break; }
            let gaps = gaps_of(&rows[end]);
            if gaps.is_empty() {
                // A row on one side only (a column's last line) stays in the band.
                // Edges may differ by a fraction of a point between lines.
                if rows[end].iter().any(|l| common.iter().any(|&(a, b)| l.x0 < b - 1.0 && l.x1 > a + 1.0)) { break; }
            } else {
                let next = intersect(&common, &gaps);
                if next.is_empty() { break; }
                common = next;
                with_gaps += 1;
            }
            end += 1;
        }
        let band_lines: Vec<&Line> = rows[i..end].iter().flatten().copied().collect();
        let mut gutters: Vec<f64> = Vec::new();
        if with_gaps >= 2 {
            let mut centres: Vec<f64> = common.iter().map(|&(a, b)| (a + b) / 2.0).collect();
            centres.sort_by(|a, b| a.total_cmp(b));
            // Each gutter needs columns of running text on both sides.
            for (index, &g) in centres.iter().enumerate() {
                let from = gutters.last().copied().unwrap_or(left);
                let to = centres.get(index + 1).copied().unwrap_or(right);
                // Running prose: lines that fill the column, mostly letters,
                // and long enough to be sentences (not code, figures or dumps).
                let side = |a: f64, b: f64| -> bool {
                    let inside: Vec<&&Line> = band_lines.iter().filter(|l| l.x0 >= a - 1.0 && l.x1 <= b + 1.0).collect();
                    let letters = inside.iter().flat_map(|l| l.text.chars()).filter(|c| c.is_alphabetic()).count();
                    let visible = inside.iter().flat_map(|l| l.text.chars()).filter(|c| !c.is_whitespace()).count().max(1);
                    inside.len() >= 2 && letters as f64 >= visible as f64 * COLUMN_LETTER_SHARE
                        && median(inside.iter().map(|l| l.text.chars().count() as f64).collect()).is_some_and(|m| m >= MIN_COLUMN_LINE_CHARS)
                        && median(inside.iter().map(|l| l.x1 - l.x0).collect()).is_some_and(|m| m >= (b - a) * 0.5)
                };
                if side(from, g) && side(g, to) { gutters.push(g); }
            }
        }
        if gutters.is_empty() { i += 1; continue; }
        let top = rows[i].iter().map(|l| l.y - l.size).fold(f64::MAX, f64::min);
        let bottom = rows[end - 1].iter().map(|l| l.y).fold(f64::MIN, f64::max);
        let gap = median(common.iter().map(|&(a, b)| b - a).collect()).unwrap_or(MIN_GUTTER_PT);
        bands.push(Band { top, bottom, gutters, gap });
        i = end;
    }
    bands
}

/// Splits a page into regions in reading order: full-width stretches from
/// the top, and within each column band every column in turn.
fn regions<'a>(lines: &[&'a Line], pictures: &[&'a Picture]) -> Vec<Region<'a>> {
    let bands = column_bands(lines);
    enum Entry<'a> { Line(&'a Line), Picture(&'a Picture) }
    // (top for ordering, y for band membership, x0, x1, entry)
    let mut entries: Vec<(f64, f64, f64, f64, Entry)> = lines.iter().map(|l| (l.y - l.size, l.y, l.x0, l.x1, Entry::Line(l)))
        .chain(pictures.iter().map(|p| (p.area.y, p.area.y + p.area.height / 2.0, p.area.x, p.area.right(), Entry::Picture(p)))).collect();
    entries.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.2.total_cmp(&b.2)));
    let empty = || Region { lines: Vec::new(), pictures: Vec::new(), left: f64::MAX, right: f64::MIN, columns: 1, column: 0, gap: 0.0, ends_early: false, frame: (0.0, 0.0) };
    // A band's columns, numbered, and whether each ends well above the band's bottom.
    let finish = |k: usize, open: Vec<Region<'a>>| -> Vec<Region<'a>> {
        let band = &bands[k];
        open.into_iter().enumerate().map(|(column, mut region)| {
            region.columns = band.gutters.len() + 1; region.column = column; region.gap = band.gap;
            let bottom = region.lines.iter().map(|l| l.y).fold(f64::MIN, f64::max);
            let size = region.lines.iter().map(|l| l.size).fold(0.0, f64::max);
            region.ends_early = bottom < band.bottom - size * 2.0;
            region.frame = (region.left, region.right);
            region
        }).filter(|r| !r.lines.is_empty() || !r.pictures.is_empty()).collect()
    };
    let add = |region: &mut Region<'a>, x0: f64, x1: f64, entry: Entry<'a>| {
        region.left = region.left.min(x0); region.right = region.right.max(x1);
        match entry { Entry::Line(l) => region.lines.push(l), Entry::Picture(p) => region.pictures.push(p) }
    };
    let mut out: Vec<Region> = Vec::new();
    let mut wide = empty();
    let mut columns: Option<(usize, Vec<Region>)> = None;
    for (_, y, x0, x1, entry) in entries {
        let band = bands.iter().position(|b| y >= b.top - 0.5 && y <= b.bottom + 0.5)
            .filter(|&k| !bands[k].gutters.iter().any(|&g| x0 < g && x1 > g));
        match band {
            Some(k) => {
                if columns.as_ref().is_none_or(|(open, _)| *open != k) {
                    if let Some((k, open)) = columns.take() { out.extend(finish(k, open)); }
                    if !wide.lines.is_empty() || !wide.pictures.is_empty() { out.push(std::mem::replace(&mut wide, empty())); }
                    columns = Some((k, (0..=bands[k].gutters.len()).map(|_| empty()).collect()));
                }
                let column = bands[k].gutters.iter().filter(|&&g| x0 >= g).count();
                if let Some((_, open)) = columns.as_mut() { add(&mut open[column], x0, x1, entry); }
            }
            None => {
                if let Some((k, open)) = columns.take() { out.extend(finish(k, open)); }
                add(&mut wide, x0, x1, entry);
            }
        }
    }
    if let Some((k, open)) = columns.take() { out.extend(finish(k, open)); }
    if !wide.lines.is_empty() || !wide.pictures.is_empty() { out.push(wide); }
    // Full-width regions align pictures with the page's text.
    let text = lines.iter().fold((f64::MAX, f64::MIN), |(l, r), line| (l.min(line.x0), r.max(line.x1)));
    for region in out.iter_mut().filter(|r| r.columns == 1) {
        region.frame = if text.0 < text.1 { text } else { (region.left, region.right) };
    }
    out
}

/// Chunks of a region on one baseline, left to right.
struct Row { chunks: Vec<Line>, y: f64, size: f64 }

fn rows(lines: &[&Line]) -> Vec<Row> {
    let mut sorted: Vec<&Line> = lines.to_vec();
    sorted.sort_by(|a, b| a.y.total_cmp(&b.y).then(a.x0.total_cmp(&b.x0)));
    let mut out: Vec<Row> = Vec::new();
    for line in sorted {
        match out.last_mut() {
            Some(row) if (row.y - line.y).abs() <= row.size.max(line.size) * 0.35 => {
                row.chunks.push(line.clone());
                row.chunks.sort_by(|a, b| a.x0.total_cmp(&b.x0));
                row.size = row.size.max(line.size);
            }
            _ => out.push(Row { chunks: vec![line.clone()], y: line.y, size: line.size }),
        }
    }
    // A list marker set apart from its text joins it.
    for row in &mut out {
        if row.chunks.len() >= 2 && list_marker(&format!("{} x", row.chunks[0].text)).is_some_and(|(_, _, n)| n == row.chunks[0].text.chars().count() + 1) {
            let text = row.chunks.remove(1);
            joined_into(&mut row.chunks[0], text);
        }
    }
    out
}

/// Appends `next` to `line` with a space, as one line.
fn joined_into(line: &mut Line, next: Line) {
    if let Some(last) = line.spans.last_mut() { if !last.text.ends_with(' ') { last.text.push(' '); } }
    line.spans.extend(next.spans);
    line.x1 = line.x1.max(next.x1);
    line.size = line.size.max(next.size);
    line.text = line.spans.iter().map(|s| s.text.as_str()).collect();
}

fn merged_row(row: Row) -> Line {
    let mut chunks = row.chunks.into_iter();
    let mut line = chunks.next().expect("a row has a chunk");
    for chunk in chunks { joined_into(&mut line, chunk); }
    line
}

enum Flow { Line(Line), Table(Grid), Picture(Picture, (f64, f64)) }

/// Rows of a region become lines and tables, with pictures in place.
fn region_flow(region: &Region, body: f64) -> Vec<Flow> {
    let mut rows = rows(&region.lines);
    let mut flow: Vec<(f64, Flow)> = Vec::new();
    let mut i = 0;
    while i < rows.len() {
        let y = rows[i].y - rows[i].size;
        if let Some((grid, used)) = table_at(&rows[i..], body) {
            flow.push((y, Flow::Table(grid)));
            i += used;
        } else {
            let row = std::mem::replace(&mut rows[i], Row { chunks: Vec::new(), y: 0.0, size: 0.0 });
            flow.push((y, Flow::Line(merged_row(row))));
            i += 1;
        }
    }
    for picture in &region.pictures { flow.push((picture.area.y, Flow::Picture((*picture).clone(), region.frame))); }
    flow.sort_by(|a, b| a.0.total_cmp(&b.0));
    flow.into_iter().map(|(_, f)| f).collect()
}

/// A table starting at the first row: consecutive rows of two or more
/// chunks whose gaps line up into column separators. Returns the table and
/// the number of rows it takes.
fn table_at(rows: &[Row], body: f64) -> Option<(Grid, usize)> {
    let multi = |row: &Row| row.chunks.len() >= 2;
    if !rows.first().is_some_and(multi) { return None; }
    // Extent of the candidate: multi-chunk rows close together, allowing
    // single-chunk rows (wrapped cell text) between them.
    let mut end = 1;
    let mut last_multi = 0;
    while end < rows.len() {
        let gap = rows[end].y - rows[end - 1].y;
        if gap > body.max(rows[end].size) * 2.6 { break; }
        if multi(&rows[end]) { last_multi = end; }
        else if end - last_multi > 2 { break; }
        end += 1;
    }
    let end = last_multi + 1;
    let multi_rows: Vec<&Row> = rows[..end].iter().filter(|r| multi(r)).collect();
    if multi_rows.len() < 2 { return None; }
    // Long text in every cell means page columns, not a table.
    let cells: Vec<usize> = multi_rows.iter().flat_map(|r| r.chunks.iter().map(|c| c.text.chars().count())).collect();
    if median(cells.iter().map(|&c| c as f64).collect()).is_some_and(|m| m > 45.0) { return None; }
    // Separators: gaps between the multi-chunk rows' chunks no chunk crosses.
    let left = multi_rows.iter().map(|r| r.chunks[0].x0).fold(f64::MAX, f64::min);
    let right = multi_rows.iter().map(|r| r.chunks.last().map_or(0.0, |c| c.x1)).fold(f64::MIN, f64::max);
    let mut separators: Vec<f64> = Vec::new();
    let mut gap_start: Option<f64> = None;
    let mut x = left;
    let covered = |x: f64| multi_rows.iter().any(|r| r.chunks.iter().any(|c| c.x0 - 1.0 < x && c.x1 + 1.0 > x));
    while x <= right {
        if !covered(x) { gap_start.get_or_insert(x); }
        else if let Some(start) = gap_start.take() { if x - start >= body * 0.6 { separators.push((start + x) / 2.0); } }
        x += 1.0;
    }
    if separators.is_empty() || separators.len() + 1 > MAX_TABLE_COLUMNS { return None; }
    // Every multi-chunk row must use at least two columns.
    let column = |c: &Line| separators.iter().filter(|&&s| (c.x0 + c.x1) / 2.0 > s).count();
    if multi_rows.iter().any(|r| r.chunks.iter().map(column).collect::<HashSet<_>>().len() < 2) { return None; }
    let columns = separators.len() + 1;
    let mut grid: Vec<Vec<Vec<Line>>> = Vec::new();
    for row in &rows[..end] {
        let mut cells: Vec<Vec<Line>> = vec![Vec::new(); columns];
        for chunk in &row.chunks {
            let cell = &mut cells[column(chunk)];
            match cell.last_mut() { Some(line) if (line.y - chunk.y).abs() < 1.0 => joined_into(line, chunk.clone()), _ => cell.push(chunk.clone()) }
        }
        // A single chunk below a row continues that row's cell.
        if !multi(row) {
            if let Some(previous) = grid.last_mut() {
                for (index, cell) in cells.into_iter().enumerate() { previous[index].extend(cell); }
                continue;
            }
        }
        grid.push(cells);
    }
    let mut edges = vec![left];
    edges.extend(separators.iter().copied());
    edges.push(right);
    let widths = edges.windows(2).map(|w| w[1] - w[0]).collect();
    Some((Grid { rows: grid, widths, page_break_before: false }, end))
}

fn weighted_size(lines: &[Line]) -> f64 {
    let (mut total, mut count) = (0.0, 0usize);
    for span in lines.iter().flat_map(|l| &l.spans) { let n = span.text.chars().count(); total += span.size * n as f64; count += n; }
    if count == 0 { lines.first().map_or(0.0, |l| l.size) } else { total / count as f64 }
}

/// Paragraphs, tables and pictures of one region.
fn region_items(region: &Region, body: f64) -> Vec<Item> {
    let flow = region_flow(region, body);
    let text_lines: Vec<&Line> = flow.iter().filter_map(|f| if let Flow::Line(l) = f { Some(l) } else { None }).collect();
    // Indents and alignment are measured from the text; a picture's padding
    // must not move the column's edge.
    let left = if text_lines.is_empty() { region.left } else { text_lines.iter().map(|l| l.x0).fold(f64::MAX, f64::min) };
    let right = text_lines.iter().map(|l| l.x1).fold(f64::MIN, f64::max).max(left + 1.0);
    let spacing = median(text_lines.windows(2).filter(|w| (w[0].size - body).abs() < 1.0 && (w[1].size - body).abs() < 1.0)
        .map(|w| w[1].y - w[0].y).filter(|d| *d > 0.0).collect()).unwrap_or(body * 1.2);
    let mut items: Vec<Item> = Vec::new();
    let mut open: Option<Para> = None;
    let close = |open: &mut Option<Para>, items: &mut Vec<Item>| {
        if let Some(mut para) = open.take() { finish_para(&mut para, left, right); items.push(Item::Para(para)); }
    };
    for entry in flow {
        let line = match entry {
            Flow::Line(line) => line,
            Flow::Table(grid) => { close(&mut open, &mut items); items.push(Item::Table(grid)); continue; }
            Flow::Picture(picture, (frame_left, frame_right)) => {
                let middle = (frame_left + frame_right) / 2.0;
                close(&mut open, &mut items);
                let centre = (picture.area.x + picture.area.right()) / 2.0;
                let align = if (centre - middle).abs() < (frame_right - frame_left) * 0.08 { Align::Center } else if picture.area.x > middle { Align::Right } else { Align::Left };
                items.push(Item::Picture { picture, align, page_break_before: false });
                continue;
            }
        };
        let marker = list_marker(&line.text);
        let starts = match open.as_ref() {
            None => true,
            Some(para) => {
                let prev = para.lines.last().expect("an open paragraph has a line");
                let gap = line.y - prev.y;
                let size_change = (line.size - prev.size).abs() > 0.9;
                let big_gap = gap > spacing * PARAGRAPH_GAP || gap < -spacing * 0.5;
                // A list item continues on lines indented past its marker.
                let list_continues = para.list.is_some() && marker.is_none() && line.x0 > para.marker_x + 2.0 && !big_gap && !size_change;
                if list_continues { false } else if para.list.is_some() { true } else {
                    let short = prev.x1 < right - (right - left) * 0.12 && prev.text.ends_with(['.', '!', '?', ':', '"', '”', ')']);
                    let indented = line.x0 > left + body * 1.2 && prev.x0 <= left + body * 0.5;
                    let centred_change = (horizontal_align(prev, left, right) == Align::Center) != (horizontal_align(&line, left, right) == Align::Center)
                        && prev.x1 - prev.x0 < (right - left) * 0.8 && line.x1 - line.x0 < (right - left) * 0.8;
                    size_change || big_gap || short || indented || marker.is_some() || centred_change
                }
            }
        };
        let heading = line.size >= body * HEADING_SCALE;
        if starts {
            close(&mut open, &mut items);
            let list = marker.map(|(format, start, _)| ListMark { format, level: 0, start });
            let marker_chars = marker.map_or(0, |(_, _, n)| n);
            open = Some(Para { kind: if heading { Kind::Heading(0) } else { Kind::Body }, marker_x: line.x0, lines: vec![line], align: Align::Left,
                left_indent: 0.0, first_indent: 0.0, list, marker_chars, page_break_before: false, size: 0.0 });
        } else if let Some(para) = open.as_mut() {
            para.lines.push(line);
        }
    }
    close(&mut open, &mut items);
    items
}

/// Alignment, indents and size of a finished paragraph.
fn finish_para(para: &mut Para, left: f64, right: f64) {
    para.size = weighted_size(&para.lines);
    let width = right - left;
    let lines = &para.lines;
    let tolerance = (width * 0.01).max(2.0);
    let centred = lines.iter().all(|l| horizontal_align(l, left, right) == Align::Center && ((l.x0 + l.x1) / 2.0 - (left + right) / 2.0).abs() < (width * 0.03).max(4.0))
        && lines.iter().any(|l| l.x1 - l.x0 < width * 0.85);
    let right_aligned = lines.iter().all(|l| l.x1 >= right - tolerance) && lines.iter().any(|l| l.x0 > left + width * 0.15);
    let justified = lines.len() >= 2 && lines[..lines.len() - 1].iter().all(|l| l.x1 >= right - tolerance);
    para.align = if para.list.is_none() && centred { Align::Center } else if para.list.is_none() && right_aligned { Align::Right }
        else if justified { Align::Justify } else { Align::Left };
    if matches!(para.align, Align::Left | Align::Justify) && para.list.is_none() {
        let first = lines[0].x0 - left;
        let rest = if lines.len() > 1 { lines[1..].iter().map(|l| l.x0).fold(f64::MAX, f64::min) - left } else { first };
        para.left_indent = if rest > 2.0 { rest } else { 0.0 };
        let hang = first - para.left_indent;
        para.first_indent = if hang.abs() > 2.0 { hang } else { 0.0 };
    }
}

/// Paragraphs continued across a page or column break (no full stop at the
/// end, lowercase start) are joined.
fn merge_across_pages(items: &mut Vec<Item>) {
    let mut merged: Vec<Item> = Vec::new();
    for item in items.drain(..) {
        if let (Some(Item::Para(last)), Item::Para(next)) = (merged.last_mut(), &item) {
            let continues = !next.page_break_before && last.kind == Kind::Body && next.kind == Kind::Body && next.list.is_none()
                && last.lines.last().is_some_and(|l| !l.text.ends_with(['.', '!', '?', ':', '"', '”']))
                && next.lines.first().is_some_and(|l| l.text.chars().next().is_some_and(|c| c.is_lowercase()));
            if continues {
                if let Item::Para(next) = item { last.lines.extend(next.lines); if last.align == Align::Left && next.align == Align::Justify { last.align = Align::Justify; } }
                continue;
            }
        }
        merged.push(item);
    }
    *items = merged;
}

/// Levels of consecutive list items, from how far their markers are indented.
fn assign_list_levels(items: &mut [Item]) {
    let mut start = 0;
    while start < items.len() {
        let is_list = |item: &Item| matches!(item, Item::Para(p) if p.list.is_some());
        if !is_list(&items[start]) { start += 1; continue; }
        let mut end = start;
        while end < items.len() && is_list(&items[end]) { end += 1; }
        let mut positions: Vec<f64> = Vec::new();
        for item in &items[start..end] {
            if let Item::Para(p) = item { if !positions.iter().any(|x| (x - p.marker_x).abs() < 4.0) { positions.push(p.marker_x); } }
        }
        positions.sort_by(|a, b| a.total_cmp(b));
        for item in &mut items[start..end] {
            if let Item::Para(p) = item {
                let level = positions.iter().position(|x| (x - p.marker_x).abs() < 4.0).unwrap_or(0).min(2) as u8;
                if let Some(mark) = p.list.as_mut() { mark.level = level; }
            }
        }
        start = end;
    }
}

/// Heading levels by size, largest first; a single largest heading is the
/// title. A short, wholly bold line at body size standing alone is a heading
/// one level below the smallest larger heading.
fn rank_headings(items: &mut [Item], body: f64) {
    let paras = || items.iter().filter_map(|i| if let Item::Para(p) = i { Some(p) } else { None });
    let chars = |p: &Para| p.lines.iter().map(|l| l.text.chars().count()).sum::<usize>();
    let mut sizes: Vec<f64> = Vec::new();
    for p in paras().filter(|p| p.kind != Kind::Body && chars(p) <= MAX_HEADING_CHARS) {
        if !sizes.iter().any(|s| (s - p.size).abs() < 0.9) { sizes.push(p.size); }
    }
    sizes.sort_by(|a, b| b.total_cmp(a));
    let largest = sizes.first().copied();
    let largest_count = largest.map_or(0, |top| paras().filter(|p| p.kind != Kind::Body && (p.size - top).abs() < 0.9).count());
    let title_level = largest_count == 1 && sizes.len() > 1;
    let levels = sizes.len() - usize::from(title_level);
    for item in items.iter_mut() {
        let Item::Para(p) = item else { continue; };
        let count = p.lines.iter().map(|l| l.text.chars().count()).sum::<usize>();
        if p.kind != Kind::Body {
            if count > MAX_HEADING_CHARS { p.kind = Kind::Body; continue; }
            let rank = sizes.iter().position(|s| (s - p.size).abs() < 0.9).unwrap_or(0);
            p.kind = if title_level && rank == 0 { Kind::Title } else { Kind::Heading((rank + 1 - usize::from(title_level)).clamp(1, 3) as u8) };
        } else if p.lines.len() == 1 && p.list.is_none() && count <= 80 && (p.size - body).abs() < 1.0 && p.lines[0].all_bold()
            && !p.lines[0].text.ends_with(['.', ',', ';', ':']) && p.align != Align::Center {
            p.kind = Kind::Heading((levels + 1).clamp(1, 3) as u8);
        }
    }
}

#[cfg(test)] #[path = "../tests/unit/structure.rs"] mod tests;
