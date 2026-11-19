//! Sticky comment threads placed alongside the paper, in the document canvas.
use super::*;
use model::{ReviewComment, TextAnchor};
use render::{ChromeShape, ViewportState};
use std::collections::{BTreeMap, BTreeSet};
use wxdragon::{
    dc::{AutoBufferedPaintDC, ClientDC, Point as DcPoint, PolygonFillMode},
    event::{EventType, WxEvtHandler},
    font::{Font, FontFamily, FontStyle, FontWeight},
    geometry::{Point, Rect},
    keycode::{WXK_ESCAPE, WXK_RETURN, WXK_NUMPAD_ENTER},
    widgets::{panel::Panel, textctrl::{TextCtrl, TextCtrlStyle}},
    window::BackgroundStyle,
    DeviceContext,
};

const WIDTH: i32 = 260;
const GAP: i32 = 16;
const PAD: i32 = 12;
const FOLD: i32 = 16;
const TOP: i32 = 7;
const LINE: i32 = 17;
const INPUT_HEIGHT: i32 = 83;
const MARK: (u8, u8, u8) = (195, 136, 38);
const INK: (u8, u8, u8) = (54, 43, 32);
const MUTED: (u8, u8, u8) = (113, 88, 50);
const PAPER: (u8, u8, u8) = (255, 239, 170);
const FOLDED: (u8, u8, u8) = (234, 204, 113);
const CLOSED_PAPER: (u8, u8, u8) = (244, 235, 210);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum NoteId { Thread(NodeId), Draft }

#[derive(Clone)]
enum Compose {
    New(editor::Selection),
    Reply(NodeId),
    Edit(NodeId),
}

struct Draft {
    document: u64,
    compose: Compose,
    anchor: TextAnchor,
    text: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action { Reply, Edit, Resolve, Reopen, Delete, Post, Cancel }

impl Action {
    fn label(self) -> &'static str {
        match self {
            Self::Reply => "Reply", Self::Edit => "Edit", Self::Resolve => "Resolve",
            Self::Reopen => "Reopen", Self::Delete => "Delete", Self::Post => "Post",
            Self::Cancel => "Cancel",
        }
    }
}

#[derive(Clone, PartialEq)]
struct ViewKey {
    document: u64,
    viewport: ViewportState,
    layout: usize,
    revision: u64,
    selected: Option<NodeId>,
}

#[derive(Clone, Copy)]
struct Anchor {
    page_right: i32,
    x: i32,
    y: i32,
}

#[derive(Clone, PartialEq)]
struct Content {
    author: String,
    meta: String,
    body: String,
    replies: Vec<(String, String)>,
    selected: bool,
    closed: bool,
    draft: bool,
}

struct Line {
    text: String,
    bold: bool,
}

#[derive(Default)]
struct CardLayout {
    content: Option<Content>,
    lines: Vec<Line>,
    height: i32,
    body_rows: usize,
    scroll: usize,
    actions: Vec<(Rect, Action)>,
    hover: Option<Action>,
}

struct Card {
    panel: Panel,
    position: Cell<Option<Rect>>,
    input: Option<TextCtrl>,
    layout: RefCell<CardLayout>,
}

pub(super) struct CommentNotes {
    ribbon: std::rc::Weak<Ribbon>,
    visible: Cell<bool>,
    cards: RefCell<BTreeMap<NoteId, Rc<Card>>>,
    draft: RefCell<Option<Draft>>,
    previous: RefCell<Option<ViewKey>>,
}

fn rgb(c: (u8, u8, u8)) -> Colour { Colour::rgb(c.0, c.1, c.2) }
fn font(points: i32, bold: bool) -> Option<Font> {
    Font::new_with_details(points, FontFamily::Default as i32, FontStyle::Normal as i32,
        if bold { FontWeight::Bold } else { FontWeight::Normal } as i32, false, "")
}

/// The comment author: the document's review author, else the account's name.
fn author(state: &CanvasState) -> String {
    let named = state.editor.document.review.author.trim().to_string();
    if !named.is_empty() { return named; }
    let user = std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_default();
    let full = std::fs::read_to_string("/etc/passwd").ok().and_then(|passwd| passwd.lines()
        .find(|line| line.split(':').next() == Some(user.as_str()))
        .and_then(|line| line.split(':').nth(4).map(|g| g.split(',').next().unwrap_or("").trim().to_string())))
        .filter(|name| !name.is_empty());
    full.unwrap_or(if user.is_empty() { "Author".into() } else { user })
}

/// When a comment was written, as Word shows it: "Just now", "5 minutes
/// ago", "3 hours ago", else the date. Stored times are UTC
/// (YYYY-MM-DDTHH:MM:SSZ), so a relative time holds in any time zone.
fn when(stamp: &str) -> String {
    let number = |range: std::ops::Range<usize>| stamp.get(range).and_then(|s| s.parse::<i64>().ok());
    let (Some(y), Some(m), Some(d), Some(hh), Some(mm)) = (number(0..4), number(5..7), number(8..10), number(11..13), number(14..16)) else {
        return stamp.to_string();
    };
    // Days since 1970 of a Gregorian date.
    let (y2, m2) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * m2 + 2) / 5 + d - 1;
    let days = era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719468;
    let written = days * 86400 + hh * 3600 + mm * 60;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |t| t.as_secs() as i64);
    let minutes = (now - written).max(0) / 60;
    match minutes {
        0 => "Just now".into(),
        1 => "1 minute ago".into(),
        2..=59 => format!("{minutes} minutes ago"),
        60..=119 => "1 hour ago".into(),
        120..=1439 => format!("{} hours ago", minutes / 60),
        _ => format!("{y:04}-{m:02}-{d:02}"),
    }
}

/// The threads in document order (unanchored ones last).
fn threads(state: &CanvasState) -> Vec<ReviewComment> {
    let order: std::collections::BTreeMap<_, _> = model::document_paragraphs(&state.editor.document).iter().enumerate().map(|(i, p)| (p.id.clone(), i)).collect();
    let mut list: Vec<ReviewComment> = state.editor.document.review.comments.values().cloned().collect();
    list.sort_by_key(|c| c.range.as_ref().map(|r| (order.get(&r.start.paragraph).copied().unwrap_or(usize::MAX), r.start.offset)).unwrap_or((usize::MAX, 0)));
    list
}

/// The open comment at the caret, if any.
fn at_caret(state: &CanvasState) -> Option<NodeId> {
    let comments = &state.editor.document.review.comments;
    if comments.is_empty() { return None; }
    let focus = state.editor.focus_position()?;
    let order = &state.vector_document.text_order;
    let here = (*order.get(&focus.node_id.0)?, focus.byte_offset);
    comments.values().filter(|comment| !comment.is_closed()).filter_map(|comment| {
        let range = comment.range.as_ref()?;
        let start = (*order.get(&range.start.paragraph.0)?, range.start.offset);
        let end = (*order.get(&range.end.paragraph.0)?, range.end.offset);
        (start <= here && here <= end).then_some((start, &comment.id))
    }).min().map(|(_, id)| id.clone())
}

/// The current comment: the selected note, else the caret's.
pub(super) fn current(state: &CanvasState) -> Option<NodeId> {
    state.comment_selected.clone().filter(|id| state.editor.document.review.comments.contains_key(id)).or_else(|| at_caret(state))
}

/// Highlights of the commented text on the page; the current comment's is
/// stronger. Part of the overlay, as spelling marks are.
pub(crate) fn comment_marks(state: &mut CanvasState) {
    if !state.comments_visible || state.display_generation != Some(state.layout_generation) { return; }
    let selected = current(state);
    let mut marks = Vec::new();
    for comment in state.editor.document.review.comments.values().filter(|c| !c.is_closed()) {
        let Some(range) = &comment.range else { continue; };
        let source = render::SourceSelection {
            anchor: render::SourcePosition { node_id: range.start.paragraph.0.clone(), byte_offset: range.start.offset },
            focus: render::SourcePosition { node_id: range.end.paragraph.0.clone(), byte_offset: range.end.offset },
        };
        let alpha = if selected.as_ref() == Some(&comment.id) { 0.26 } else { 0.14 };
        let color = render::Color { r: MARK.0 as f32 / 255.0, g: MARK.1 as f32 / 255.0, b: MARK.2 as f32 / 255.0, a: alpha };
        let overlay = render::overlay_for_selection(&state.vector_document, Some(&source), 1.0);
        marks.extend(overlay.selections.into_iter().map(|area| render::ColoredOverlayRect { area, color }));
        if range.start == range.end {
            if let Some(area) = overlay.caret {
                marks.push(render::ColoredOverlayRect { area, color });
            }
        }
    }
    state.overlay.review_marks.extend(marks);
}

/// A comment with nothing selected comments the word at the caret, as Word.
fn word_at_caret(editor: &mut editor::EditorState) {
    let Some(selection) = editor.selection.clone() else { return; };
    if !selection.is_collapsed() { return; }
    let position = selection.focus;
    editor.select_word_at(position.node_id, position.byte_offset);
}

/// Locate the visible occurrence of each anchor in one pass. Geometry-only
/// pages receive their note once the adjacent paper has been drawn.
fn anchors(
    state: &CanvasState,
    wanted: &BTreeMap<&str, Vec<(&NoteId, usize)>>,
    rects: &[render::Rect],
) -> BTreeMap<NoteId, Anchor> {
    let mut found = BTreeMap::new();
    if state.display_generation.is_none() || wanted.is_empty() { return found; }
    let k = state.pixel_scale;
    let scale = state.viewport.points_to_pixels();
    for (page, rect) in state.vector_document.pages.iter().zip(rects) {
        if !page.has_paint || rect.max_y() < 0.0 || rect.y > state.viewport.viewport_px.height { continue; }
        for hit in &page.hit_regions {
            let Some(requests) = wanted.get(hit.node_id.as_str()) else { continue };
            let y = rect.y + hit.rect.max_y() * scale;
            if y < 0.0 || y >= state.viewport.viewport_px.height { continue; }
            for &(id, offset) in requests {
                if hit.byte_range.contains(&offset) || hit.caret_stops.iter().any(|stop| stop.byte_offset == offset) {
                    let caret = hit.caret_stops.iter().min_by_key(|stop| stop.byte_offset.abs_diff(offset))
                        .map_or(hit.rect.x, |stop| stop.x_pt);
                    let x = rect.x + caret * scale;
                    found.entry(id.clone()).or_insert(Anchor {
                        page_right: (rect.max_x() / k).round() as i32,
                        x: (x / k).round() as i32,
                        y: (y / k).round() as i32,
                    });
                }
            }
        }
    }
    found
}

fn wrap(dc: &impl DeviceContext, text: &str, width: i32, limit: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.lines() {
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            let candidate = if line.is_empty() { word.to_owned() } else { format!("{line} {word}") };
            if dc.get_text_extent(&candidate).0 <= width {
                line = candidate;
                continue;
            }
            if !line.is_empty() { lines.push(std::mem::take(&mut line)); }
            if lines.len() >= limit { return lines; }
            if dc.get_text_extent(word).0 <= width {
                line.push_str(word);
                continue;
            }
            for ch in word.chars() {
                line.push(ch);
                if line.len() > ch.len_utf8() && dc.get_text_extent(&line).0 > width {
                    line.pop();
                    lines.push(std::mem::take(&mut line));
                    if lines.len() >= limit { return lines; }
                    line.push(ch);
                }
            }
        }
        lines.push(line);
        if lines.len() >= limit { break; }
    }
    lines
}

impl Card {
    fn new(notes: &Rc<CommentNotes>, id: &NoteId, parent: Panel, draft: Option<&str>) -> Rc<Self> {
        let panel = Panel::builder(&parent).with_pos(Point::new(-500, -500)).with_size(Size::new(1, 1)).build();
        panel.set_background_style(BackgroundStyle::Paint);
        if let Some(cursor) = wxdragon::cursor::Cursor::from_stock(wxdragon::cursor::StockCursor::Arrow) {
            panel.set_cursor(Some(&cursor));
        }
        if draft.is_none() { panel.set_can_focus(false); }
        panel.on_erase_background(|_| {});
        let input = draft.map(|text| TextCtrl::builder(&panel).with_value(text)
            .with_style(TextCtrlStyle::MultiLine | TextCtrlStyle::ProcessEnter).build());
        let card = Rc::new(Self { panel, input, position: Cell::new(None), layout: RefCell::default() });

        let weak = Rc::downgrade(&card);
        panel.on_paint(move |_| if let Some(card) = weak.upgrade() { card.paint(); });
        let weak = Rc::downgrade(&card);
        panel.on_mouse_motion(move |event| {
            let (Some(card), WindowEventData::MouseMotion(mouse)) = (weak.upgrade(), &event) else { return };
            let hit = mouse.get_position().and_then(|point| card.hit(point));
            if card.layout.borrow().hover != hit {
                card.layout.borrow_mut().hover = hit;
                card.panel.refresh(false, None);
            }
        });
        let weak = Rc::downgrade(&card);
        panel.on_mouse_leave(move |_| {
            if let Some(card) = weak.upgrade() {
                let changed = card.layout.borrow_mut().hover.take().is_some();
                if changed { card.panel.refresh(false, None); }
            }
        });
        let (weak_notes, weak_card, key) = (Rc::downgrade(notes), Rc::downgrade(&card), id.clone());
        panel.on_mouse_left_up(move |event| {
            let (Some(notes), Some(card), WindowEventData::MouseButton(mouse)) =
                (weak_notes.upgrade(), weak_card.upgrade(), &event) else { return };
            if let Some(point) = mouse.get_position() { notes.act(&key, card.hit(point)); }
        });
        let (weak_notes, weak_card) = (Rc::downgrade(notes), Rc::downgrade(&card));
        panel.on_mouse_wheel(move |event| {
            let (Some(notes), Some(card)) = (weak_notes.upgrade(), weak_card.upgrade()) else { return };
            if let Some(mouse) = mouse_data(&event) {
                let rotation = mouse.get_wheel_rotation();
                if !card.scroll(rotation) { notes.scroll_document(rotation); }
            }
            event.skip(false);
        });
        if let Some(input) = input {
            let weak = Rc::downgrade(notes);
            input.bind_internal(EventType::CHAR_HOOK, move |event| {
                let Some(notes) = weak.upgrade() else { return };
                match event.get_key_code() {
                    Some(WXK_ESCAPE) => notes.finish_draft(true),
                    Some(WXK_RETURN | WXK_NUMPAD_ENTER) if event.control_down() => notes.post(),
                    _ => event.skip(true),
                }
            });
            input.set_tooltip(&crate::i18n::text("Comment"));
        }
        card
    }

    fn hit(&self, point: Point) -> Option<Action> {
        self.layout.borrow().actions.iter().find(|(rect, _)| {
            point.x >= rect.x && point.x < rect.x + rect.width &&
                point.y >= rect.y && point.y < rect.y + rect.height
        }).map(|(_, action)| *action)
    }

    fn scroll(&self, rotation: i32) -> bool {
        let mut layout = self.layout.borrow_mut();
        if !layout.content.as_ref().is_some_and(|content| content.selected) { return false; }
        let maximum = layout.lines.len().saturating_sub(layout.body_rows);
        let next = if rotation > 0 { layout.scroll.saturating_sub(3) } else { (layout.scroll + 3).min(maximum) };
        if next == layout.scroll { return false; }
        layout.scroll = next;
        drop(layout);
        self.panel.refresh(false, None);
        true
    }

    fn measure(&self, dc: &ClientDC, content: Content, viewport_height: i32) -> i32 {
        let d = |v| self.panel.from_dip_int(v);
        let width = d(WIDTH) - 2 * d(PAD);
        let mut layout = self.layout.borrow_mut();
        if layout.content.as_ref() != Some(&content) {
            let mut lines = Vec::new();
            if let Some(font) = font(9, false) { dc.set_font(&font); }
            let limit = if content.selected { usize::MAX } else { 4 };
            lines.extend(wrap(dc, &content.body, width, limit).into_iter().map(|text| Line { text, bold: false }));
            if !content.selected && lines.len() == 4 {
                lines.truncate(3);
                lines[2].text.push('…');
            }
            if content.selected {
                for (author, text) in &content.replies {
                    lines.push(Line { text: author.clone(), bold: true });
                    lines.extend(wrap(dc, text, width, usize::MAX).into_iter().map(|text| Line { text, bold: false }));
                }
            }
            layout.lines = lines;
            layout.scroll = 0;
            layout.content = Some(content.clone());
        }

        let header = d(PAD + LINE + if content.meta.is_empty() { 0 } else { LINE });
        let actions: &[Action] = if content.draft { &[Action::Cancel, Action::Post] }
            else if !content.selected { &[] }
            else if content.closed { &[Action::Reopen, Action::Edit, Action::Delete] }
            else { &[Action::Reply, Action::Resolve, Action::Edit, Action::Delete] };
        let mut row = 0;
        let mut x = d(PAD);
        let mut buttons = Vec::new();
        if let Some(font) = font(9, true) { dc.set_font(&font); }
        for &action in actions {
            let button_width = dc.get_text_extent(&crate::i18n::text(action.label())).0 + d(10);
            if x + button_width > d(WIDTH - PAD) && x > d(PAD) {
                row += 1;
                x = d(PAD);
            }
            buttons.push((Rect::new(x, row * d(22), button_width, d(22)), action));
            x += button_width + d(2);
        }
        let controls = if buttons.is_empty() { 0 } else { d(5 + 22 * (row + 1)) };
        let max_rows = ((viewport_height / 2 - header - controls - d(PAD)) / d(LINE)).max(2) as usize;
        let rows = layout.lines.len().min(max_rows);
        layout.body_rows = rows;
        layout.scroll = layout.scroll.min(layout.lines.len().saturating_sub(rows));
        let body = if content.draft { d(INPUT_HEIGHT + 5) } else { rows as i32 * d(LINE) };
        for (rect, _) in &mut buttons { rect.y += header + body + d(5); }
        layout.actions = buttons;
        layout.height = header + body + controls + d(PAD);
        let height = layout.height;
        drop(layout);
        if let Some(input) = self.input {
            input.set_size_with_pos(d(PAD), header + d(3), width, d(INPUT_HEIGHT));
        }
        height
    }

    fn place(&self, x: i32, y: i32, height: i32) {
        let bounds = Rect::new(x, y, self.panel.from_dip_int(WIDTH), height);
        if self.position.replace(Some(bounds)) != Some(bounds) {
            self.panel.set_size_with_pos(bounds.x, bounds.y, bounds.width, bounds.height);
        }
        self.panel.refresh(false, None);
    }

    fn paint(&self) {
        let dc = AutoBufferedPaintDC::new(&self.panel);
        let d = |v| self.panel.from_dip_int(v);
        let size = self.panel.get_client_size();
        let layout = self.layout.borrow();
        let Some(content) = &layout.content else { return };
        let paper = if content.closed { CLOSED_PAPER } else { PAPER };
        let (w, h, fold) = (size.width, size.height, d(FOLD));
        dc.set_background(rgb(paper));
        dc.clear();
        dc.set_pen(rgb(if content.selected { MARK } else { FOLDED }), 1, PenStyle::Solid);
        dc.set_brush(rgb(paper), BrushStyle::Solid);
        dc.draw_rectangle(0, 0, w, h);
        dc.set_brush(rgb(FOLDED), BrushStyle::Solid);
        dc.draw_polygon(&[DcPoint::new(w - fold, 0), DcPoint::new(w - 1, fold),
            DcPoint::new(w - 1, 0)], 0, 0, PolygonFillMode::OddEven);
        dc.set_background_mode(wxdragon::dc::BackgroundMode::Transparent);

        let mut y = d(PAD);
        let text_x = d(PAD);
        if let Some(font) = font(10, true) { dc.set_font(&font); }
        dc.set_text_foreground(rgb(INK));
        dc.set_clipping_region(text_x, y, (w - 2 * text_x - fold).max(1), d(LINE));
        dc.draw_text(&content.author, text_x, y);
        dc.destroy_clipping_region();
        y += d(LINE);
        if !content.meta.is_empty() {
            if let Some(font) = font(8, false) { dc.set_font(&font); }
            dc.set_text_foreground(rgb(MUTED));
            dc.draw_text(&content.meta, text_x, y);
            y += d(LINE);
        }
        dc.set_text_foreground(rgb(INK));
        for line in layout.lines.iter().skip(layout.scroll).take(layout.body_rows) {
            if let Some(font) = font(9, line.bold) { dc.set_font(&font); }
            dc.set_clipping_region(text_x, y, (w - 2 * text_x).max(1), d(LINE));
            dc.draw_text(&line.text, text_x, y);
            dc.destroy_clipping_region();
            y += d(LINE);
        }
        if layout.lines.len() > layout.body_rows && content.selected {
            let body_top = d(PAD + LINE + if content.meta.is_empty() { 0 } else { LINE });
            let track = layout.body_rows as i32 * d(LINE);
            let grip = ((track as usize * layout.body_rows / layout.lines.len()) as i32).max(d(8));
            let offset = (track - grip).max(0) as usize * layout.scroll / (layout.lines.len() - layout.body_rows);
            dc.set_pen(rgb(MARK), d(2).max(1), PenStyle::Solid);
            dc.draw_line(w - d(5), body_top + offset as i32, w - d(5), body_top + offset as i32 + grip);
        }
        if let Some(font) = font(9, true) { dc.set_font(&font); }
        for (rect, action) in &layout.actions {
            let label = crate::i18n::text(action.label());
            if layout.hover == Some(*action) || *action == Action::Post {
                dc.set_pen(rgb(FOLDED), 1, PenStyle::Solid);
                dc.set_brush(rgb(if *action == Action::Post { FOLDED } else { (248, 225, 147) }), BrushStyle::Solid);
                dc.draw_rounded_rectangle(rect.x, rect.y, rect.width, rect.height, 4.0);
            }
            dc.set_text_foreground(rgb(if *action == Action::Delete { (146, 60, 44) } else { INK }));
            let (_, th) = dc.get_text_extent(&label);
            dc.draw_text(&label, rect.x + d(5), rect.y + (rect.height - th) / 2);
        }
    }
}

impl CommentNotes {
    pub(super) fn install(ribbon: &Rc<Ribbon>) -> Rc<Self> {
        Rc::new(Self {
            ribbon: Rc::downgrade(ribbon), visible: Cell::new(true),
            cards: RefCell::default(), draft: RefCell::default(), previous: RefCell::default(),
        })
    }

    pub(super) fn shown(&self) -> bool { self.visible.get() }

    pub(super) fn show(self: &Rc<Self>, visible: bool) {
        if self.visible.replace(visible) == visible { return; }
        let Some(ribbon) = self.ribbon.upgrade() else { return };
        if !visible { self.finish_draft(false); }
        {
            let mut state = ribbon.state.borrow_mut();
            state.comments_visible = visible;
            if !visible { state.comment_selected = None; }
            refresh_overlay(&mut state);
        }
        self.refresh(&ribbon);
    }

    fn refresh(self: &Rc<Self>, ribbon: &Ribbon) {
        self.previous.borrow_mut().take();
        ribbon.last_key.borrow_mut().take();
        self.sync_view();
        ribbon.ui.paper_canvas.refresh(false, None);
    }

    fn select_thread(self: &Rc<Self>, id: NodeId, reveal: bool) {
        let Some(ribbon) = self.ribbon.upgrade() else { return };
        self.show(true);
        {
            let mut state = ribbon.state.borrow_mut();
            if !state.editor.document.review.comments.contains_key(&id) { return; }
            state.comment_selected = Some(id.clone());
            if reveal {
                let anchor = state.editor.document.review.comments[&id].range.as_ref().map(|r| r.start.clone());
                let document = Arc::clone(&state.vector_document);
                if let Some(anchor) = anchor {
                    let target = document.pages.iter().enumerate().find_map(|(page, content)| {
                        let hit = content.hit_regions.iter().find(|hit| hit.node_id == anchor.paragraph.0
                            && (hit.byte_range.contains(&anchor.offset) || hit.caret_stops.iter().any(|s| s.byte_offset == anchor.offset)))?;
                        Some((page, hit.rect.y))
                    });
                    if let Some((page, y)) = target {
                        if let Some(point) = state.viewport.page_point_to_view(&document, page, render::Point::new(0.0, y)) {
                            let dy = point.y - state.viewport.viewport_px.height / 4.0;
                            state.viewport.scroll_by(0.0, dy, &document);
                        }
                    }
                }
                Self::reveal_margin(&mut state);
            }
            refresh_overlay(&mut state);
        }
        self.refresh(&ribbon);
    }

    pub(super) fn select(self: &Rc<Self>, id: Option<NodeId>) {
        if let Some(id) = id { self.select_thread(id, true); }
    }

    fn reveal_margin(state: &mut CanvasState) {
        let document = Arc::clone(&state.vector_document);
        let rects = state.viewport.page_rects_px(&document);
        let right = rects.iter().find(|page| page.max_y() > 0.0).map_or(0.0, |p| p.max_x());
        let dx = (right + state.viewport.right_inset_px - state.viewport.viewport_px.width).max(0.0);
        state.viewport.scroll_by(dx, 0.0, &document);
    }

    pub(super) fn sync_view(self: &Rc<Self>) {
        let Some(ribbon) = self.ribbon.upgrade() else { return };
        let canvas = ribbon.ui.paper_canvas;
        let d = |value| canvas.from_dip_int(value);
        let active = ribbon.state.borrow().documents.active_id();
        if self.draft.borrow().as_ref().is_some_and(|draft| draft.document != active) {
            self.draft.borrow_mut().take();
        }
        if self.previous.borrow().as_ref().is_some_and(|prior| prior.document != active) {
            self.clear_cards();
        }
        let (key, entries) = {
            let mut state = ribbon.state.borrow_mut();
            let comments = &state.editor.document.review.comments;
            let visible = self.visible.get() && ribbon.mode.get() != ViewMode::Typst;
            let selected = state.comment_selected.clone();
            let draft = self.draft.borrow();
            let show_notes = visible && (draft.is_some() || comments.values().any(|comment| !comment.is_closed())
                || selected.as_ref().is_some_and(|id| comments.contains_key(id)));
            let inset = if show_notes { d(WIDTH + GAP + 8) as f32 * state.pixel_scale } else { 0.0 };
            if state.viewport.right_inset_px != inset {
                state.viewport.right_inset_px = inset;
                let document = Arc::clone(&state.vector_document);
                state.viewport.clamp_scroll(&document);
                if draft.is_some() || selected.is_some() { Self::reveal_margin(&mut state); }
                state.needs_present = true;
            }
            let key = ViewKey {
                document: active, viewport: state.viewport, layout: Arc::as_ptr(&state.vector_document) as usize,
                revision: state.editor.revision, selected: selected.clone(),
            };
            if self.previous.borrow().as_ref() == Some(&key) { return; }
            if !show_notes { (key, Vec::new()) } else {
                let rects = state.viewport.page_rects_px(&state.vector_document);
                let leftmost_note = rects.iter().find(|r| r.max_y() > 0.0).map_or(
                    canvas.get_client_size().width - d(WIDTH + 8),
                    |r| (r.max_x() / state.pixel_scale).round() as i32 + d(GAP),
                );
                let mut targets = Vec::new();
                for comment in state.editor.document.review.comments.values() {
                    let is_selected = selected.as_ref() == Some(&comment.id);
                    if !is_selected && (comment.is_closed() || comment.range.is_none()) { continue; }
                    targets.push((NoteId::Thread(comment.id.clone()), comment.range.as_ref().map(|r| &r.start)));
                }
                if let Some(draft) = draft.as_ref() { targets.push((NoteId::Draft, Some(&draft.anchor))); }
                let mut wanted: BTreeMap<&str, Vec<(&NoteId, usize)>> = BTreeMap::new();
                for (id, target) in &targets {
                    if let Some(target) = target { wanted.entry(target.paragraph.0.as_str()).or_default().push((id, target.offset)); }
                }
                let locations = anchors(&state, &wanted, &rects);
                let mut entries = Vec::new();
                for (id, target) in targets {
                    let location = locations.get(&id).copied();
                    let (x, y) = match location {
                        Some(anchor) => (anchor.page_right + d(GAP), anchor.y - d(PAD)),
                        None if id == NoteId::Draft || target.is_none() => (leftmost_note, d(TOP)),
                        _ => continue,
                    };
                    // Keep the full editor accessible while drafting even if the
                    // user scrolls horizontally away from the margin.
                    let x = if id == NoteId::Draft {
                        x.min(canvas.get_client_size().width - d(WIDTH + 6)).max(0)
                    } else { x };
                    if x >= canvas.get_client_size().width || x + d(WIDTH) <= 0 { continue; }
                    let content = match &id {
                        NoteId::Thread(thread) => {
                            let comment = &state.editor.document.review.comments[thread];
                            let selected = selected.as_ref() == Some(thread);
                            let closed = comment.is_closed();
                            let meta = [
                                comment.date.as_deref().map(when),
                                closed.then(|| "Resolved".to_owned()),
                                comment.range.is_none().then(|| "Text removed".to_owned()),
                            ].into_iter().flatten().map(|label| crate::i18n::text(&label)).collect::<Vec<_>>().join(" · ");
                            Content {
                                author: comment.author.clone(), meta, body: comment.text.clone(),
                                replies: if selected { comment.replies.iter().map(|reply|
                                    (format!("{} · {}", reply.author, crate::i18n::text(&when(&reply.date))), reply.text.clone())).collect()
                                } else { Vec::new() },
                                selected, closed, draft: false,
                            }
                        }
                        NoteId::Draft => {
                            let label = match &draft.as_ref().expect("draft exists").compose {
                                Compose::New(_) => "New comment", Compose::Reply(_) => "Reply", Compose::Edit(_) => "Edit comment",
                            };
                            Content { author: crate::i18n::text(label), meta: String::new(), body: String::new(),
                                replies: Vec::new(), selected: true, closed: false, draft: true }
                        }
                    };
                    if id != NoteId::Draft && draft.as_ref().is_some_and(|d| match &d.compose {
                        Compose::Reply(source) | Compose::Edit(source) => id == NoteId::Thread(source.clone()),
                        Compose::New(_) => false,
                    }) { continue; }
                    entries.push((id, location, x, y, content));
                }
                (key, entries)
            }
        };
        self.previous.replace(Some(key));
        self.place_cards(canvas, entries);
    }

    fn clear_cards(&self) {
        // Do not hold the map borrowed while destroying native child windows.
        for (_, card) in self.cards.take() { card.panel.hide(); card.panel.destroy(); }
    }

    fn place_cards(self: &Rc<Self>, canvas: Panel, mut entries: Vec<(NoteId, Option<Anchor>, i32, i32, Content)>) {
        let Some(ribbon) = self.ribbon.upgrade() else { return };
        let height = canvas.get_client_size().height;
        let gap = canvas.from_dip_int(7);
        entries.sort_by_key(|(_, anchor, x, y, _)| (*y, anchor.map_or(*x, |a| a.x)));
        let mut kept = BTreeSet::new();
        let mut measured = Vec::new();
        let dc = ClientDC::new(&canvas);
        let mut next = gap;
        for (id, anchor, x, y, content) in entries {
            let prior = self.cards.borrow().get(&id).cloned();
            let card = prior.unwrap_or_else(|| {
                let draft = self.draft.borrow();
                let input = if id == NoteId::Draft { draft.as_ref().map(|draft| draft.text.as_str()) } else { None };
                let card = Card::new(self, &id, canvas, input);
                self.cards.borrow_mut().insert(id.clone(), Rc::clone(&card));
                card
            });
            let selected = content.selected;
            let size = card.measure(&dc, content, height);
            let y = y.clamp(gap, (height - size - gap).max(gap)).max(next);
            next = y + size + gap;
            measured.push((id, card, anchor, x, y, size, selected));
        }
        drop(dc);

        let priority = measured.iter().position(|(id, ..)| *id == NoteId::Draft)
            .or_else(|| measured.iter().position(|entry| entry.6));
        if let Some(index) = priority {
            let limit = (height - measured[index].5 - gap).max(gap);
            if measured[index].4 > limit {
                measured[index].4 = limit;
                for i in (0..index).rev() {
                    measured[i].4 = measured[i].4.min(measured[i + 1].4 - gap - measured[i].5);
                }
            }
        }

        let mut chrome = Vec::new();
        let k = ribbon.state.borrow().pixel_scale;
        let accent = render::Color::rgba(0.69, 0.43, 0.12, 0.74);
        for (id, card, anchor, x, y, size, selected) in measured {
            if y < 0 || y >= height {
                if id != NoteId::Draft { continue; }
            }
            card.place(x, y, size);
            kept.insert(id);
            chrome.push(ChromeShape::RoundRect {
                rect: render::Rect::new((x + 2) as f32 * k, (y + 3) as f32 * k, canvas.from_dip_int(WIDTH) as f32 * k, size as f32 * k),
                radius: 2.0 * k, fill: Some(render::Color::rgba(0.28, 0.23, 0.13, 0.16)), stroke: None,
            });
            if let Some(anchor) = anchor.filter(|anchor| x > anchor.x) {
                let from = render::Point::new(anchor.x as f32 * k, anchor.y as f32 * k);
                let to = render::Point::new(x as f32 * k, (y + canvas.from_dip_int(13)) as f32 * k);
                chrome.push(ChromeShape::DashedLine { from, to, width: k, color: accent, dash: 4.0 * k });
                if selected {
                    chrome.push(ChromeShape::Circle { center: from, radius: 2.0 * k, fill: Some(accent), stroke: None });
                }
            }
        }
        let mut cards = self.cards.borrow_mut();
        let stale = cards.keys().filter(|id| !kept.contains(*id)).cloned().collect::<Vec<_>>();
        let stale = stale.into_iter().filter_map(|id| cards.remove(&id)).collect::<Vec<_>>();
        drop(cards);
        for card in stale { card.panel.hide(); card.panel.destroy(); }
        let mut state = ribbon.state.borrow_mut();
        if state.comment_leaders != chrome {
            state.comment_leaders = chrome;
            state.needs_present = true;
        }
    }

    pub(super) fn new_comment(self: &Rc<Self>) {
        let Some(ribbon) = self.ribbon.upgrade() else { return };
        self.show(true);
        let selection = {
            let mut state = ribbon.state.borrow_mut();
            word_at_caret(&mut state.editor);
            refresh_overlay(&mut state);
            state.editor.selection.clone()
        };
        if let Some(selection) = selection {
            let anchor = TextAnchor { paragraph: selection.anchor.node_id.clone(), offset: selection.anchor.byte_offset };
            self.write(Compose::New(selection), anchor, String::new());
        } else {
            ribbon.state.borrow_mut().action_status = Some("No active caret".into());
        }
    }

    fn write(self: &Rc<Self>, compose: Compose, anchor: TextAnchor, text: String) {
        let Some(ribbon) = self.ribbon.upgrade() else { return };
        if self.draft.borrow().is_none() {
            let document = ribbon.state.borrow().documents.active_id();
            self.draft.replace(Some(Draft { document, compose, anchor, text }));
            self.refresh(&ribbon);
        }
        if let Some(input) = self.cards.borrow().get(&NoteId::Draft).and_then(|card| card.input) {
            input.set_focus();
            input.set_insertion_point_end();
        }
    }

    fn finish_draft(self: &Rc<Self>, refocus: bool) {
        if self.draft.borrow_mut().take().is_none() { return; }
        if let Some(ribbon) = self.ribbon.upgrade() {
            self.refresh(&ribbon);
            if refocus { ribbon.focus_document(); }
        }
    }

    fn post(self: &Rc<Self>) {
        let Some(ribbon) = self.ribbon.upgrade() else { return };
        let Some(input) = self.cards.borrow().get(&NoteId::Draft).and_then(|card| card.input) else { return };
        let Some((document, compose)) = self.draft.borrow().as_ref().map(|d| (d.document, d.compose.clone())) else { return };
        if document != ribbon.state.borrow().documents.active_id() {
            self.finish_draft(false);
            return;
        }
        let text = input.get_value();
        if text.trim().is_empty() { return; }
        let name = author(&ribbon.state.borrow());
        let result = match compose {
            Compose::New(selection) => {
                let before: BTreeSet<_> = ribbon.state.borrow().editor.document.review.comments.keys().cloned().collect();
                ribbon.edit_without_focus(|e| { e.selection = Some(selection); e.add_comment(&name, &text) }).map(|()| {
                    let made = ribbon.state.borrow().editor.document.review.comments.keys().find(|id| !before.contains(*id)).cloned();
                    ribbon.state.borrow_mut().comment_selected = made;
                })
            }
            Compose::Reply(id) => ribbon.edit_without_focus(|e| e.reply_to_comment(&id, &name, &text)),
            Compose::Edit(id) => ribbon.edit_without_focus(|e| e.update_comment(&id, Some(&text), None)),
        };
        match result {
            Ok(()) => self.finish_draft(true),
            Err(error) => ribbon.state.borrow_mut().action_status = Some(error),
        }
    }

    pub(super) fn delete(self: &Rc<Self>, id: &NodeId) {
        let Some(ribbon) = self.ribbon.upgrade() else { return };
        if self.draft.borrow().as_ref().is_some_and(|draft| matches!(&draft.compose,
            Compose::Reply(source) | Compose::Edit(source) if source == id)) {
            self.finish_draft(false);
        }
        if let Err(error) = ribbon.edit(|e| e.delete_comment(id)) { ribbon.state.borrow_mut().action_status = Some(error); }
        if ribbon.state.borrow().comment_selected.as_ref() == Some(id) { ribbon.state.borrow_mut().comment_selected = None; }
        self.refresh(&ribbon);
    }

    pub(super) fn toggle_resolved(self: &Rc<Self>, id: &NodeId) {
        let Some(ribbon) = self.ribbon.upgrade() else { return };
        let closed = ribbon.state.borrow().editor.document.review.comments.get(id).is_some_and(ReviewComment::is_closed);
        let result = ribbon.edit_without_focus(|e| e.update_comment(id, None, Some(!closed)));
        let message = match result {
            Ok(()) => {
                if !closed { ribbon.state.borrow_mut().comment_selected = None; }
                if closed { "Comment reopened".into() } else { "Comment resolved".into() }
            }
            Err(error) => error,
        };
        ribbon.state.borrow_mut().action_status = Some(message);
        self.refresh(&ribbon);
    }

    /// Previous and Next include resolved and detached threads so they can
    /// still be found and reopened from the Review ribbon.
    pub(super) fn step(self: &Rc<Self>, forward: bool) {
        let Some(ribbon) = self.ribbon.upgrade() else { return };
        let id = {
            let state = ribbon.state.borrow();
            let list = threads(&state);
            if list.is_empty() { return; }
            let at = current(&state).and_then(|id| list.iter().position(|comment| comment.id == id));
            let index = match (at, forward) {
                (Some(index), true) => (index + 1) % list.len(),
                (Some(index), false) => (index + list.len() - 1) % list.len(),
                (None, true) => 0,
                (None, false) => list.len() - 1,
            };
            list[index].id.clone()
        };
        self.select_thread(id, true);
    }

    fn scroll_document(&self, rotation: i32) {
        let Some(ribbon) = self.ribbon.upgrade() else { return };
        let mut state = ribbon.state.borrow_mut();
        let document = Arc::clone(&state.vector_document);
        let distance = ribbon.ui.paper_canvas.from_dip_int(58) as f32 * state.pixel_scale * rotation.signum() as f32;
        state.viewport.scroll_by(0.0, -distance, &document);
        state.needs_present = true;
    }

    fn act(self: &Rc<Self>, id: &NoteId, action: Option<Action>) {
        if *id == NoteId::Draft {
            match action {
                Some(Action::Post) => self.post(),
                Some(Action::Cancel) => self.finish_draft(true),
                _ => {}
            }
            return;
        }
        let NoteId::Thread(id) = id else { return };
        let Some(ribbon) = self.ribbon.upgrade() else { return };
        match action {
            Some(Action::Reply | Action::Edit) => {
                let info = {
                    let state = ribbon.state.borrow();
                    state.editor.document.review.comments.get(id).map(|comment| {
                        let anchor = comment.range.as_ref().map(|r| r.start.clone());
                        (anchor, comment.text.clone())
                    })
                };
                if let Some((anchor, text)) = info {
                    // Detached threads can still be edited at their current note.
                    let anchor = anchor.unwrap_or(TextAnchor { paragraph: NodeId::from(""), offset: 0 });
                    if action == Some(Action::Edit) { self.write(Compose::Edit(id.clone()), anchor, text); }
                    else { self.write(Compose::Reply(id.clone()), anchor, String::new()); }
                }
            }
            Some(Action::Resolve | Action::Reopen) => self.toggle_resolved(id),
            Some(Action::Delete) => self.delete(id),
            None => self.select_thread(id.clone(), false),
            _ => {}
        }
    }
}
