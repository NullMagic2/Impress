//! The Comments pane, as Word's: the document's comment threads as cards in
//! document order (author, date, text, replies; resolved ones dimmed), the
//! commented text highlighted on the page. Selecting a card selects its text;
//! the selected card offers Reply, Edit, Resolve or Reopen, and Delete. New
//! comments, replies and edits are written in the field at the bottom (Ctrl+
//! Enter posts, Escape cancels). Review > Comment, Delete, Resolve, Previous
//! and Next work on the current comment: the one selected here, or the one
//! the caret is in.
use super::*;
use model::{AnchorRange, ReviewComment};
use render::Rect;
use wxdragon::{
    dc::AutoBufferedPaintDC,
    event::{EventType, WxEvtHandler},
    font::{Font, FontFamily, FontStyle, FontWeight},
    geometry::Point,
    keycode::{WXK_ESCAPE, WXK_RETURN, WXK_NUMPAD_ENTER},
    widgets::{panel::Panel, textctrl::TextCtrl},
    window::BackgroundStyle,
    DeviceContext,
};

const PAD: i32 = 14;
const HEADER: i32 = 44;
const CARD_PAD: i32 = 10;
const CARD_GAP: i32 = 8;
const STRIP: i32 = 38;
/// Lines of a comment shown on a card that is not selected.
const PREVIEW_LINES: usize = 3;

const INK: (u8, u8, u8) = (36, 48, 66);
const MUTED: (u8, u8, u8) = (107, 119, 138);
const ACCENT: (u8, u8, u8) = (43, 111, 232);
const CARD: (u8, u8, u8) = (255, 255, 255);
const CARD_EDGE: (u8, u8, u8) = (214, 221, 232);
const MARK: (u8, u8, u8) = (226, 150, 40);

/// What is being written in the field.
#[derive(Clone)]
enum Compose {
    /// A new comment on this selection.
    New(Option<editor::Selection>),
    Reply(NodeId),
    Edit(NodeId),
}

#[derive(Clone, PartialEq)]
enum Hit { Close, Card(NodeId), Reply(NodeId), Edit(NodeId), Resolve(NodeId), Delete(NodeId), Post, Cancel }

#[derive(Default)]
struct Inner {
    compose: Option<Compose>,
    scroll: i32,
    content_height: i32,
    hits: Vec<(Rect, Hit)>,
    hover: Option<Hit>,
}

pub(super) struct CommentsPane {
    ribbon: std::rc::Weak<Ribbon>,
    panel: Panel,
    surface: Panel,
    input: TextCtrl,
    inner: RefCell<Inner>,
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
    let user = std::env::var("USER").unwrap_or_default();
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

/// Whether `range` holds the caret position (`paragraph`, `offset`).
fn contains(state: &CanvasState, range: &AnchorRange, paragraph: &NodeId, offset: usize) -> bool {
    let order: Vec<NodeId> = model::document_paragraphs(&state.editor.document).iter().map(|p| p.id.clone()).collect();
    let at = |id: &NodeId| order.iter().position(|p| p == id);
    let (Some(start), Some(end), Some(here)) = (at(&range.start.paragraph), at(&range.end.paragraph), at(paragraph)) else { return false; };
    (start, range.start.offset) <= (here, offset) && (here, offset) <= (end, range.end.offset)
}

/// The comment the caret is in, if any.
fn at_caret(state: &CanvasState) -> Option<NodeId> {
    let focus = state.editor.focus_position()?;
    threads(state).into_iter().filter(|c| !c.resolved)
        .find(|c| c.range.as_ref().is_some_and(|r| contains(state, r, &focus.node_id, focus.byte_offset)))
        .map(|c| c.id)
}

/// The current comment: the one selected in the pane, else the caret's.
pub(super) fn current(state: &CanvasState) -> Option<NodeId> {
    state.comment_selected.clone().filter(|id| state.editor.document.review.comments.contains_key(id)).or_else(|| at_caret(state))
}

/// Highlights of the commented text on the page; the current comment's is
/// stronger. Part of the overlay, as spelling marks are.
pub(crate) fn comment_marks(state: &mut CanvasState) {
    if state.display_generation != Some(state.layout_generation) { return; }
    let selected = current(state);
    let mut marks = Vec::new();
    for comment in state.editor.document.review.comments.values().filter(|c| !c.resolved) {
        let Some(range) = &comment.range else { continue; };
        let source = render::SourceSelection {
            anchor: render::SourcePosition { node_id: range.start.paragraph.0.clone(), byte_offset: range.start.offset },
            focus: render::SourcePosition { node_id: range.end.paragraph.0.clone(), byte_offset: range.end.offset },
        };
        let alpha = if selected.as_ref() == Some(&comment.id) { 0.42 } else { 0.2 };
        let color = render::Color { r: MARK.0 as f32 / 255.0, g: MARK.1 as f32 / 255.0, b: MARK.2 as f32 / 255.0, a: alpha };
        for area in render::overlay_for_selection(&state.vector_document, Some(&source), 1.0).selections {
            marks.push(render::ColoredOverlayRect { area, color });
        }
    }
    state.overlay.review_marks.extend(marks);
}

/// A comment with nothing selected comments the word at the caret, as Word.
fn word_at_caret(editor: &mut editor::EditorState) {
    let Some(selection) = editor.selection.clone() else { return; };
    if !selection.is_collapsed() { return; }
    let position = selection.focus;
    let Some(text) = editor.paragraph_text(&position.node_id) else { return; };
    let at = position.byte_offset.min(text.len());
    let word = |c: char| c.is_alphanumeric() || c == '\'' || c == '-';
    let start = text[..at].char_indices().rev().take_while(|(_, c)| word(*c)).last().map_or(at, |(i, _)| i);
    let end = text[at..].char_indices().find(|(_, c)| !word(*c)).map_or(text.len(), |(i, _)| at + i);
    if start < end {
        editor.set_caret(position.node_id.clone(), start);
        editor.extend_selection(position.node_id, end);
    }
}

impl CommentsPane {
    pub(super) fn install(ribbon: &Rc<Ribbon>) -> Rc<Self> {
        let ui = &ribbon.ui;
        let pane = Rc::new(Self { ribbon: Rc::downgrade(ribbon), panel: ui.comments_panel, surface: ui.comments_surface,
            input: ui.comments_input, inner: RefCell::new(Inner::default()) });
        pane.surface.set_background_style(BackgroundStyle::Paint);
        pane.surface.on_erase_background(|_| {});
        let weak = Rc::downgrade(&pane);
        pane.surface.on_paint(move |_| { if let Some(p) = weak.upgrade() { p.paint(); } });
        let weak = Rc::downgrade(&pane);
        pane.surface.on_size(move |event| { if let Some(p) = weak.upgrade() { p.surface.refresh(false, None); } event.skip(true); });
        let weak = Rc::downgrade(&pane);
        pane.surface.on_mouse_motion(move |event| {
            let Some(p) = weak.upgrade() else { return; };
            let WindowEventData::MouseMotion(m) = &event else { return; };
            let hit = m.event.get_position().and_then(|at| p.hit(at));
            let changed = { let mut inner = p.inner.borrow_mut(); let changed = inner.hover != hit; inner.hover = hit; changed };
            if changed { p.surface.refresh(false, None); }
        });
        let weak = Rc::downgrade(&pane);
        pane.surface.on_mouse_leave(move |_| {
            if let Some(p) = weak.upgrade() { if p.inner.borrow_mut().hover.take().is_some() { p.surface.refresh(false, None); } }
        });
        let weak = Rc::downgrade(&pane);
        pane.surface.on_mouse_left_up(move |event| {
            let Some(p) = weak.upgrade() else { return; };
            let WindowEventData::MouseButton(m) = &event else { return; };
            if let Some(hit) = m.event.get_position().and_then(|at| p.hit(at)) { p.act(hit); }
        });
        let weak = Rc::downgrade(&pane);
        pane.surface.on_mouse_wheel(move |event| {
            let Some(p) = weak.upgrade() else { return; };
            let rotation = match &event {
                WindowEventData::MouseButton(m) => m.event.get_wheel_rotation(),
                WindowEventData::MouseMotion(m) => m.event.get_wheel_rotation(),
                _ => 0,
            };
            let step = p.surface.from_dip_int(48);
            { let mut inner = p.inner.borrow_mut(); inner.scroll = (inner.scroll - rotation.signum() * step).clamp(0, inner.content_height.max(0)); }
            p.surface.refresh(false, None);
            event.skip(false);
        });
        let weak = Rc::downgrade(&pane);
        pane.input.bind_internal(EventType::CHAR_HOOK, move |event| {
            let Some(p) = weak.upgrade() else { return; };
            match event.get_key_code() {
                Some(WXK_ESCAPE) => p.stop_writing(),
                Some(WXK_RETURN | WXK_NUMPAD_ENTER) if event.control_down() => p.post(),
                _ => event.skip(true),
            }
        });
        pane
    }

    fn hit(&self, at: Point) -> Option<Hit> {
        let (x, y) = (at.x as f32, at.y as f32);
        self.inner.borrow().hits.iter().rev().find(|(r, _)| x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height).map(|(_, h)| h.clone())
    }

    pub(super) fn shown(&self) -> bool { self.panel.is_shown() }

    pub(super) fn show(&self, shown: bool) {
        let Some(ribbon) = self.ribbon.upgrade() else { return; };
        if self.panel.is_shown() == shown { return; }
        self.panel.show(shown);
        if !shown { self.stop_writing(); ribbon.state.borrow_mut().comment_selected = None; }
        ribbon.ui.workspace_panel.layout(); ribbon.ui.body_panel.layout();
        self.refresh(&ribbon);
    }

    /// Repaints the pane and the page highlights, and the Ribbon's buttons.
    fn refresh(&self, ribbon: &Ribbon) {
        { let mut state = ribbon.state.borrow_mut(); refresh_overlay(&mut state); state.needs_present = true; }
        ribbon.last_key.borrow_mut().take();
        self.surface.refresh(false, None);
    }

    /// Selects `id` (and its text on the page), opening the pane.
    pub(super) fn select(&self, id: Option<NodeId>) {
        let Some(ribbon) = self.ribbon.upgrade() else { return; };
        self.show(true);
        let range = id.as_ref().and_then(|id| ribbon.state.borrow().editor.document.review.comments.get(id).and_then(|c| c.range.clone()));
        ribbon.state.borrow_mut().comment_selected = id;
        if let Some(range) = range { let _ = ribbon.edit(|e| e.go_to_range(&range)); }
        self.scroll_to_selected();
        self.refresh(&ribbon);
    }

    /// Follows the caret into a comment, as the caret moves in the text.
    pub(super) fn follow_caret(&self) {
        let Some(ribbon) = self.ribbon.upgrade() else { return; };
        if !self.shown() || self.inner.borrow().compose.is_some() { return; }
        let here = at_caret(&ribbon.state.borrow());
        let changed = { let mut state = ribbon.state.borrow_mut(); let changed = here.is_some() && state.comment_selected != here; if changed { state.comment_selected = here; } changed };
        if changed { self.scroll_to_selected(); self.refresh(&ribbon); }
    }

    fn scroll_to_selected(&self) { self.inner.borrow_mut().scroll = -1; }

    /// Starts a new comment on the selection (the word at the caret if none).
    pub(super) fn new_comment(&self) {
        let Some(ribbon) = self.ribbon.upgrade() else { return; };
        self.show(true);
        let selection = { let mut state = ribbon.state.borrow_mut(); word_at_caret(&mut state.editor); state.editor.selection.clone() };
        ribbon.state.borrow_mut().comment_selected = None;
        self.write(Compose::New(selection), "");
    }

    fn write(&self, compose: Compose, text: &str) {
        self.inner.borrow_mut().compose = Some(compose);
        self.input.set_value(text);
        self.input.show(true);
        self.panel.layout();
        self.input.set_focus();
        self.input.set_insertion_point_end();
        self.surface.refresh(false, None);
    }

    fn stop_writing(&self) {
        if self.inner.borrow_mut().compose.take().is_none() { return; }
        self.input.set_value("");
        self.input.show(false);
        self.panel.layout();
        self.surface.refresh(false, None);
        if let Some(ribbon) = self.ribbon.upgrade() { ribbon.focus_document(); }
    }

    fn post(&self) {
        let Some(ribbon) = self.ribbon.upgrade() else { return; };
        let Some(compose) = self.inner.borrow().compose.clone() else { return; };
        let text = self.input.get_value();
        if text.trim().is_empty() { self.stop_writing(); return; }
        let name = author(&ribbon.state.borrow());
        let result = match &compose {
            Compose::New(selection) => {
                let before: std::collections::BTreeSet<NodeId> = ribbon.state.borrow().editor.document.review.comments.keys().cloned().collect();
                let selection = selection.clone();
                ribbon.edit(|e| { if selection.is_some() { e.selection = selection; } e.add_comment(&name, &text) }).map(|()| {
                    let made = ribbon.state.borrow().editor.document.review.comments.keys().find(|id| !before.contains(*id)).cloned();
                    ribbon.state.borrow_mut().comment_selected = made;
                })
            }
            Compose::Reply(id) => ribbon.edit(|e| e.reply_to_comment(id, &name, &text)),
            Compose::Edit(id) => ribbon.edit(|e| e.update_comment(id, Some(&text), None)),
        };
        match result {
            Ok(()) => { self.stop_writing(); self.scroll_to_selected(); self.refresh(&ribbon); }
            Err(error) => ribbon.state.borrow_mut().action_status = Some(error),
        }
    }

    pub(super) fn delete(&self, id: &NodeId) {
        let Some(ribbon) = self.ribbon.upgrade() else { return; };
        if let Err(error) = ribbon.edit(|e| e.delete_comment(id)) { ribbon.state.borrow_mut().action_status = Some(error); }
        if ribbon.state.borrow().comment_selected.as_ref() == Some(id) { ribbon.state.borrow_mut().comment_selected = None; }
        self.refresh(&ribbon);
    }

    pub(super) fn toggle_resolved(&self, id: &NodeId) {
        let Some(ribbon) = self.ribbon.upgrade() else { return; };
        let resolved = ribbon.state.borrow().editor.document.review.comments.get(id).is_some_and(|c| c.resolved);
        if let Err(error) = ribbon.edit(|e| e.update_comment(id, None, Some(!resolved))) { ribbon.state.borrow_mut().action_status = Some(error); }
        self.refresh(&ribbon);
    }

    /// Selects the next (or previous) comment from the current one or the caret.
    pub(super) fn step(&self, forward: bool) {
        let Some(ribbon) = self.ribbon.upgrade() else { return; };
        let next = {
            let state = ribbon.state.borrow();
            let list = threads(&state);
            if list.is_empty() { None } else {
                let at = current(&state).and_then(|id| list.iter().position(|c| c.id == id));
                let index = match (at, forward) {
                    (Some(i), true) => (i + 1) % list.len(),
                    (Some(i), false) => (i + list.len() - 1) % list.len(),
                    (None, true) => 0,
                    (None, false) => list.len() - 1,
                };
                Some(list[index].id.clone())
            }
        };
        if next.is_some() { self.select(next); }
    }

    fn act(&self, hit: Hit) {
        let Some(ribbon) = self.ribbon.upgrade() else { return; };
        match hit {
            Hit::Close => self.show(false),
            Hit::Card(id) => self.select(Some(id)),
            Hit::Reply(id) => self.write(Compose::Reply(id), ""),
            Hit::Edit(id) => {
                let text = ribbon.state.borrow().editor.document.review.comments.get(&id).map(|c| c.text.clone()).unwrap_or_default();
                self.write(Compose::Edit(id), &text);
            }
            Hit::Resolve(id) => self.toggle_resolved(&id),
            Hit::Delete(id) => self.delete(&id),
            Hit::Post => self.post(),
            Hit::Cancel => self.stop_writing(),
        }
    }

    fn paint(&self) {
        let Some(ribbon) = self.ribbon.upgrade() else { return; };
        let dc = AutoBufferedPaintDC::new(&self.surface);
        let size = self.surface.get_client_size();
        let (w, h) = (size.width.max(1), size.height.max(1));
        let d = |v: i32| self.surface.from_dip_int(v);
        let f = |v: i32| d(v) as f32;
        let Some((mut pix, canvas)) = crate::crisp::Canvas::new(&self.surface, w, h) else { return; };
        pix.fill(tiny_skia::Color::from_rgba8(247, 249, 251, 255));
        let state = ribbon.state.borrow();
        let list = threads(&state);
        let selected = current(&state);
        let compose = self.inner.borrow().compose.clone();
        let strip = if compose.is_some() { d(STRIP) } else { 0 };
        let (top, bottom) = (d(HEADER), h - strip);
        let mut hits: Vec<(Rect, Hit)> = Vec::new();
        let hover = self.inner.borrow().hover.clone();

        // Lay the cards out: measured lines of text with their fonts.
        struct Line { text: String, bold: bool, small: bool, color: (u8, u8, u8), indent: i32 }
        let wrap = |text: &str, width: i32, bold: bool, small: bool| -> Vec<String> {
            if let Some(font) = font(if small { 8 } else { 9 }, bold) { dc.set_font(&font); }
            let mut lines = Vec::new();
            for paragraph in text.split('\n') {
                let mut line = String::new();
                for word in paragraph.split(' ') {
                    let candidate = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
                    if !line.is_empty() && dc.get_text_extent(&candidate).0 > width { lines.push(std::mem::take(&mut line)); line = word.to_string(); }
                    else { line = candidate; }
                }
                lines.push(line);
            }
            lines
        };
        let card_w = w - 2 * d(PAD);
        let text_w = card_w - 2 * d(CARD_PAD);
        let line_h = d(17);
        let mut y = top + d(4) - self.inner.borrow().scroll.max(0);
        let mut selected_span = None;
        let mut cards = Vec::new();
        for c in &list {
            let is_selected = selected.as_ref() == Some(&c.id);
            let date = c.date.as_deref().map(when).unwrap_or_default();
            let mut lines = vec![Line { text: c.author.clone(), bold: true, small: false, color: INK, indent: 0 }];
            let meta = format!("{}{}{}", date, if c.resolved { " · Resolved" } else { "" }, if c.range.is_none() { " · Text removed" } else { "" });
            if !meta.is_empty() { lines.push(Line { text: meta, bold: false, small: true, color: MUTED, indent: 0 }); }
            let body = wrap(&c.text, text_w, false, false);
            let shown = if is_selected { body.len() } else { body.len().min(PREVIEW_LINES) };
            for (i, text) in body.iter().take(shown).enumerate() {
                let text = if !is_selected && i + 1 == shown && body.len() > shown { format!("{text}…") } else { text.clone() };
                lines.push(Line { text, bold: false, small: false, color: if c.resolved { MUTED } else { INK }, indent: 0 });
            }
            if is_selected {
                for reply in &c.replies {
                    lines.push(Line { text: format!("{} · {}", reply.author, when(&reply.date)), bold: true, small: true, color: INK, indent: d(12) });
                    for text in wrap(&reply.text, text_w - d(12), false, false) { lines.push(Line { text, bold: false, small: false, color: INK, indent: d(12) }); }
                }
            } else if !c.replies.is_empty() {
                lines.push(Line { text: format!("{} {}", c.replies.len(), if c.replies.len() == 1 { "reply" } else { "replies" }), bold: false, small: true, color: ACCENT, indent: 0 });
            }
            let actions = if is_selected { line_h + d(6) } else { 0 };
            let height = 2 * d(CARD_PAD) + lines.len() as i32 * line_h + actions;
            if is_selected { selected_span = Some((y, y + height)); }
            cards.push((c.clone(), y, height, lines, is_selected));
            y += height + d(CARD_GAP);
        }
        let content = y + self.inner.borrow().scroll.max(0) - top;
        // Bring the selected card into view when asked (scroll = -1).
        if self.inner.borrow().scroll < 0 {
            let mut scroll = 0;
            if let Some((a, b)) = selected_span {
                let a = a + 0 - top; let b = b - top;
                if b > bottom - top { scroll = (b - (bottom - top) + d(8)).min(a); }
            }
            drop(state);
            { let mut inner = self.inner.borrow_mut(); inner.scroll = scroll; inner.content_height = (content - (bottom - top)).max(0); }
            self.surface.refresh(false, None);
            return;
        }
        self.inner.borrow_mut().content_height = (content - (bottom - top)).max(0);

        let fill = |pix: &mut tiny_skia::Pixmap, (x, y, w, h): (f32, f32, f32, f32), radius: f32, rgb: (u8, u8, u8), edge: Option<((u8, u8, u8), f32)>| {
            let mut p = tiny_skia::PathBuilder::new();
            let r = radius.min(w / 2.0).min(h / 2.0);
            p.move_to(x + r, y); p.line_to(x + w - r, y); p.quad_to(x + w, y, x + w, y + r);
            p.line_to(x + w, y + h - r); p.quad_to(x + w, y + h, x + w - r, y + h);
            p.line_to(x + r, y + h); p.quad_to(x, y + h, x, y + h - r); p.line_to(x, y + r); p.quad_to(x, y, x + r, y); p.close();
            let Some(path) = p.finish() else { return; };
            let mut paint = tiny_skia::Paint::default(); paint.anti_alias = true;
            paint.set_color_rgba8(rgb.0, rgb.1, rgb.2, 255);
            pix.fill_path(&path, &paint, tiny_skia::FillRule::Winding, crate::crisp::transform(), None);
            if let Some((c, width)) = edge {
                paint.set_color_rgba8(c.0, c.1, c.2, 255);
                pix.stroke_path(&path, &paint, &tiny_skia::Stroke { width, ..Default::default() }, crate::crisp::transform(), None);
            }
        };
        let x = d(PAD);
        for (c, cy, height, _, is_selected) in &cards {
            if cy + height < top || *cy > bottom { continue; }
            let hovered = hover.as_ref() == Some(&Hit::Card(c.id.clone()));
            let edge = if *is_selected { (ACCENT, f(1).max(1.5)) } else if hovered { ((168, 190, 222), 1.0) } else { (CARD_EDGE, 1.0) };
            fill(&mut pix, (x as f32 + 0.5, *cy as f32 + 0.5, card_w as f32 - 1.0, *height as f32 - 1.0), f(6), CARD, Some(edge));
            // The comment's mark colour along its left edge.
            if !c.resolved { fill(&mut pix, (x as f32 + f(1), *cy as f32 + f(6), f(3), *height as f32 - f(12)), f(1), MARK, None); }
            hits.push((Rect::new(x as f32, *cy as f32, card_w as f32, *height as f32), Hit::Card(c.id.clone())));
        }
        // Header and the writing strip cover the list where it scrolls under them.
        fill(&mut pix, (0.0, 0.0, w as f32, top as f32), 0.0, (247, 249, 251), None);
        if strip > 0 { fill(&mut pix, (0.0, bottom as f32, w as f32, strip as f32), 0.0, (238, 242, 247), None); }
        canvas.present(&dc, &pix, 0, 0);

        dc.set_background_mode(wxdragon::dc::BackgroundMode::Transparent);
        for (c, cy, _, lines, is_selected) in &cards {
            let mut ly = cy + d(CARD_PAD);
            for line in lines {
                if ly >= top && ly + line_h <= bottom {
                    if let Some(font) = font(if line.small { 8 } else { 9 }, line.bold) { dc.set_font(&font); }
                    dc.set_text_foreground(rgb(line.color));
                    dc.draw_text(&line.text, x + d(CARD_PAD) + line.indent, ly);
                }
                ly += line_h;
            }
            if *is_selected && ly + line_h <= bottom && ly >= top {
                // Commands of the selected thread, as links.
                let mut ax = x + d(CARD_PAD);
                let commands = [("Reply", Hit::Reply(c.id.clone())), ("Edit", Hit::Edit(c.id.clone())),
                    (if c.resolved { "Reopen" } else { "Resolve" }, Hit::Resolve(c.id.clone())), ("Delete", Hit::Delete(c.id.clone()))];
                for (label, hit) in commands {
                    if let Some(font) = font(9, hover.as_ref() == Some(&hit)) { dc.set_font(&font); }
                    dc.set_text_foreground(rgb(if matches!(hit, Hit::Delete(_)) { (190, 42, 42) } else { ACCENT }));
                    let (tw, th) = dc.get_text_extent(&crate::i18n::text(label));
                    dc.draw_text(&crate::i18n::text(label), ax, ly + d(6));
                    hits.push((Rect::new(ax as f32, (ly + d(4)) as f32, tw as f32, (th + d(4)) as f32), hit));
                    ax += tw + d(16);
                }
            }
        }
        if list.is_empty() {
            if let Some(font) = font(9, false) { dc.set_font(&font); }
            dc.set_text_foreground(rgb(MUTED));
            let mut ly = top + d(8);
            for text in wrap(&crate::i18n::text("No comments yet. Select text and choose Review > Comment to add one."), card_w, false, false) {
                if let Some(font) = font(9, false) { dc.set_font(&font); }
                dc.draw_text(&text, x, ly); ly += line_h;
            }
        }
        // Header: title and the panes' red cross.
        if let Some(font) = font(11, true) { dc.set_font(&font); }
        dc.set_text_foreground(rgb(INK));
        dc.draw_text(&crate::i18n::text("Comments"), d(PAD), d(12));
        let side = d(CLOSE_MARK_DIP as i32);
        let k = crate::display_scale::window_scale(&self.surface);
        let physical = (side as f32 * k).round().max(1.0) as u32;
        if let Some(bitmap) = close_mark(physical).and_then(|rgba| wxdragon::Bitmap::from_rgba(&rgba, physical, physical)) {
            crate::crisp::draw(&dc, &bitmap, k, w - d(PAD) - side, (d(HEADER) - side) / 2);
        }
        hits.push((Rect::new((w - d(PAD) - side - d(4)) as f32, 0.0, (side + d(8)) as f32, d(HEADER) as f32), Hit::Close));
        // Writing strip: what is being written, Post and Cancel.
        if let Some(compose) = &compose {
            let label = match compose {
                Compose::New(_) => "New comment".to_string(),
                Compose::Reply(id) => format!("Reply to {}", state.editor.document.review.comments.get(id).map(|c| c.author.as_str()).unwrap_or("comment")),
                Compose::Edit(_) => "Edit comment".to_string(),
            };
            if let Some(font) = font(9, true) { dc.set_font(&font); }
            dc.set_text_foreground(rgb(INK));
            let (_, th) = dc.get_text_extent(&crate::i18n::text(&label));
            dc.draw_text(&crate::i18n::text(&label), d(PAD), bottom + (strip - th) / 2);
            let mut bx = w - d(PAD);
            for (label, hit) in [("Cancel", Hit::Cancel), ("Post", Hit::Post)] {
                if let Some(font) = font(9, hit == Hit::Post) { dc.set_font(&font); }
                let (tw, th) = dc.get_text_extent(&crate::i18n::text(label));
                bx -= tw;
                dc.set_text_foreground(rgb(if hit == Hit::Post { ACCENT } else { MUTED }));
                dc.draw_text(&crate::i18n::text(label), bx, bottom + (strip - th) / 2);
                hits.push((Rect::new((bx - d(4)) as f32, bottom as f32, (tw + d(8)) as f32, strip as f32), hit));
                bx -= d(18);
            }
        }
        drop(state);
        self.inner.borrow_mut().hits = hits;
    }
}
