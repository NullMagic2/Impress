//! Visual equation editor in the docked equation pane.
//!
//! One owner-painted surface shows the formula as it will look, with a caret
//! and dashed boxes for empty slots, a categorized palette, and a close
//! button. The equation in the document follows every change. Typing builds structure directly (`/` fraction, `^`
//! power, `_` subscript, `\name` symbols). An optional source field shows the
//! same equation as Typst math (`$x$` inline, `$ x $` display) and accepts
//! Typst math typed into it.
pub mod layout;
pub mod palette;
pub mod slots;
pub mod tree;

use crate::{equation_ui, CanvasState};
use layout::{Draw, FontId, Fonts, Ink, MathBox, Rect};
use tree::{Editor, Item, Node, Path};
use palette::Insert;
use std::{cell::{Cell, RefCell}, collections::HashMap, rc::Rc, time::{Duration, Instant}};
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Stroke, StrokeDash, Transform};
use wxdragon::{
    dc::AutoBufferedPaintDC, event::WindowEventData, font::{Font, FontFamily, FontStyle, FontWeight},
    keycode::*, prelude::*, widgets::{panel::Panel, textctrl::TextCtrl}, window::BackgroundStyle, Bitmap,
};

#[derive(Clone, Copy)]
struct Rgb(u8, u8, u8);
const PANE: Rgb = Rgb(247, 249, 251);
const CARD: Rgb = Rgb(255, 255, 255);
const BORDER: Rgb = Rgb(214, 222, 234);
const INK: Rgb = Rgb(31, 39, 51);
const MUTED: Rgb = Rgb(107, 119, 138);
const ACCENT: Rgb = Rgb(43, 111, 232);
const ACCENT_SOFT: Rgb = Rgb(232, 240, 253);
const ACCENT_EDGE: Rgb = Rgb(157, 182, 232);
const HOVER: Rgb = Rgb(236, 241, 248);
const TRACK: Rgb = Rgb(233, 237, 243);
const ERROR: Rgb = Rgb(194, 65, 58);

impl Rgb {
    fn colour(self) -> Colour { Colour::rgb(self.0, self.1, self.2) }
    fn paint(self, alpha: u8) -> Paint<'static> {
        let mut paint = Paint::default();
        paint.set_color_rgba8(self.0, self.1, self.2, alpha);
        paint.anti_alias = true;
        paint
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hit { Formula, Tab(usize), Cell(usize), Matrix { add: bool, column: bool }, Close, Source }

#[derive(Clone, Copy, PartialEq, Eq)]
enum TextStyle { Title, Small, Button }

struct Text { text: String, x: i32, y: i32, style: TextStyle, colour: Rgb }

struct Fonts2 { title: Font, body: Font, small: Font }

struct Inner {
    editor: Editor,
    undo: Vec<Editor>,
    redo: Vec<Editor>,
    tab: usize,
    hover: Option<Hit>,
    pressed: Option<Hit>,
    drag: Option<(Path, usize)>,
    /// Hit areas and the formula below are in the drawing's physical pixels:
    /// `scale` of them per logical (mouse) pixel.
    hits: Vec<(Rect, Hit)>,
    scale: f32,
    /// Formula layout and its origin in surface pixels.
    formula: Option<(f32, f32, MathBox)>,
    last_pushed: Option<Result<::model::EquationNode, String>>,
    up_to_date: bool,
    draft_error: Option<String>,
    preview_ready: bool,
    show_source: bool,
    /// First palette row shown, and (visible, total) rows.
    palette_scroll: usize,
    palette_rows: (usize, usize),
    focused: bool,
    caret_on: bool,
    blink: Instant,
    glyphs: HashMap<(FontId, u16), Option<tiny_skia::Path>>,
}

pub struct EquationEditor {
    surface: Panel,
    pane: Panel,
    source: TextCtrl,
    state: Rc<RefCell<CanvasState>>,
    inner: RefCell<Inner>,
    fonts: Fonts2,
    syncing: Cell<bool>,
    on_close: Box<dyn Fn()>,
    /// A click on the equation on the page, waiting for this pane's layout.
    pending_click: Cell<Option<(f32, f32)>>,
}

impl EquationEditor {
    pub fn install(surface: Panel, pane: Panel, source: TextCtrl, state: Rc<RefCell<CanvasState>>,
        on_close: Box<dyn Fn()>) -> Rc<Self> {
        let font = |size, weight: FontWeight| Font::new_with_details(size, FontFamily::Default as i32,
            FontStyle::Normal as i32, weight as i32, false, "Segoe UI")
            .or_else(|| Font::new_with_details(size, FontFamily::Default as i32, FontStyle::Normal as i32, weight as i32, false, ""))
            .unwrap_or_default();
        let fonts = Fonts2 { title: font(12, FontWeight::Bold), body: font(9, FontWeight::Normal),
            small: font(8, FontWeight::Normal) };
        let editor = Rc::new(Self {
            surface, pane, source, state, fonts, syncing: Cell::new(false), on_close, pending_click: Cell::new(None),
            inner: RefCell::new(Inner {
                editor: Editor::default(), undo: Vec::new(), redo: Vec::new(), tab: 0, hover: None, pressed: None,
                drag: None, hits: Vec::new(), scale: 1.0, formula: None, last_pushed: None,
                up_to_date: true, draft_error: None, preview_ready: false, show_source: false, palette_scroll: 0, palette_rows: (0, 0), focused: false,
                caret_on: true, blink: Instant::now(), glyphs: HashMap::new(),
            }),
        });
        editor.bind();
        editor
    }

    fn bind(self: &Rc<Self>) {
        self.surface.set_background_style(BackgroundStyle::Paint);
        let weak = Rc::downgrade(self);
        self.surface.on_paint(move |_| { if let Some(e) = weak.upgrade() { e.paint(); } });
        self.surface.on_erase_background(|_| {});
        let weak = Rc::downgrade(self);
        self.surface.on_size(move |event| { if let Some(e) = weak.upgrade() { e.surface.refresh(false, None); } event.skip(true); });
        let weak = Rc::downgrade(self);
        self.surface.on_mouse_left_down(move |event| {
            if let (Some(e), Some(p)) = (weak.upgrade(), position(&event)) { e.mouse_down(p.0, p.1); }
        });
        let weak = Rc::downgrade(self);
        self.surface.on_mouse_motion(move |event| {
            if let (Some(e), Some(p)) = (weak.upgrade(), position(&event)) { e.mouse_move(p.0, p.1); }
        });
        let weak = Rc::downgrade(self);
        self.surface.on_mouse_left_up(move |event| {
            if let (Some(e), Some(p)) = (weak.upgrade(), position(&event)) { e.mouse_up(p.0, p.1); }
        });
        let weak = Rc::downgrade(self);
        self.surface.on_mouse_wheel(move |event| {
            let Some(e) = weak.upgrade() else { return; };
            let rotation = match &event {
                WindowEventData::MouseButton(m) => m.event.get_wheel_rotation(),
                WindowEventData::MouseMotion(m) => m.event.get_wheel_rotation(),
                _ => 0,
            };
            if rotation != 0 {
                {
                    let mut inner = e.inner.borrow_mut();
                    let (visible, total) = inner.palette_rows;
                    let max = total.saturating_sub(visible);
                    inner.palette_scroll = if rotation > 0 { inner.palette_scroll.saturating_sub(1) } else { (inner.palette_scroll + 1).min(max) };
                }
                e.surface.refresh(false, None);
            }
            event.skip(false);
        });
        let weak = Rc::downgrade(self);
        self.surface.on_mouse_leave(move |_| {
            if let Some(e) = weak.upgrade() {
                let changed = e.inner.borrow_mut().hover.take().is_some();
                if changed { e.surface.refresh(false, None); }
            }
        });
        let weak = Rc::downgrade(self);
        self.surface.on_set_focus(move |event| {
            if let Some(e) = weak.upgrade() { { let mut i = e.inner.borrow_mut(); i.focused = true; i.caret_on = true; i.blink = Instant::now(); } e.surface.refresh(false, None); }
            event.skip(true);
        });
        let weak = Rc::downgrade(self);
        self.surface.on_kill_focus(move |event| {
            if let Some(e) = weak.upgrade() {
                {
                    let mut inner = e.inner.borrow_mut();
                    inner.focused = false;
                    inner.drag = None;
                    inner.pressed = None;
                    inner.editor.deactivate_slot();
                }
                if e.surface.has_capture() { e.surface.release_mouse(); }
                e.surface.refresh(false, None);
            }
            event.skip(true);
        });
        let weak = Rc::downgrade(self);
        self.surface.on_key_down(move |event| {
            let Some(e) = weak.upgrade() else { return; };
            if let WindowEventData::Keyboard(key) = &event {
                if let Some(code) = crate::shortcuts::letter(key).map(|c| c as i32).or_else(|| key.get_key_code()) {
                    if e.key(code, key.shift_down(), key.cmd_down() && !key.alt_down()) { event.skip(false); return; }
                }
            }
            event.skip(true);
        });
        let weak = Rc::downgrade(self);
        self.surface.on_char(move |event| {
            let Some(e) = weak.upgrade() else { return; };
            if let WindowEventData::Keyboard(key) = &event {
                if key.cmd_down() && !key.alt_down() { event.skip(true); return; }
                if let Some(c) = key.get_unicode_key().and_then(|c| char::from_u32(c as u32)) {
                    if !c.is_control() { e.mutate(|ed| ed.type_char(c)); event.skip(false); return; }
                }
            }
            event.skip(true);
        });
        let weak = Rc::downgrade(self);
        self.source.on_text_updated(move |_| { if let Some(e) = weak.upgrade() { e.source_edited(); } });
    }

    /// Starts editing `root`, or a new, empty equation.
    pub fn load(&self, root: Option<&::model::EquationNode>) {
        {
            let mut inner = self.inner.borrow_mut();
            inner.editor = root.map(Editor::from_node).unwrap_or_default();
            inner.undo.clear(); inner.redo.clear();
            inner.drag = None;
            inner.last_pushed = Some(root.cloned().ok_or_else(|| tree::EMPTY_SLOT.to_string()));
            inner.caret_on = true; inner.blink = Instant::now();
        }
        let display = equation_ui::display(&self.state.borrow());
        self.set_source_text(&root.map(|r| editor::equation_input::format_with_display(r, display)).unwrap_or_default());
        self.surface.refresh(false, None);
    }

    pub fn focus(&self) { self.surface.set_focus(); }

    /// Tells the page which slot the cursor is in, for its blue box, while
    /// the pane is in use (a click elsewhere on the page clears the box).
    fn share_slot(&self) {
        let path = self.inner.borrow().editor.cursor.path.clone();
        let Ok(mut state) = self.state.try_borrow_mut() else { return; };
        let slot = equation_ui::draft_equation(&state).filter(|_| !path.is_empty() && self.surface.has_focus()).map(|id| (id, path));
        if state.equation_pane_slot != slot { state.equation_pane_slot = slot; state.needs_present = true; }
    }

    /// Puts the cursor where a click on the equation on the page landed,
    /// given as a fraction of the equation's width and height: the slot at
    /// that place in this pane's layout of the same formula.
    /// The pane may just have opened, so the click is applied once the
    /// formula is laid out (in the next paint).
    pub fn click_at(&self, fx: f32, fy: f32) {
        self.pending_click.set(Some((fx, fy)));
        self.focus();
        self.surface.refresh(false, None);
    }

    fn apply_pending_click(&self) {
        let Some((fx, fy)) = self.pending_click.take() else { return; };
        let target = {
            let inner = self.inner.borrow();
            inner.formula.as_ref().and_then(|(_, _, mbox)| mbox.hit(fx * mbox.w, -mbox.asc + fy * mbox.height()))
        };
        if let Some((path, offset)) = target { self.navigate(|e| e.set_cursor(path, offset)); self.surface.refresh(false, None); }
    }

    /// Called on every UI tick while the pane is open: follows whether the
    /// document holds the equation yet, and blinks the caret.
    pub fn poll(&self) {
        let (up_to_date, error, ready) = {
            let state = self.state.borrow();
            match state.equation_draft.as_ref() {
                Some(d) => (d.up_to_date(), d.error.clone(), d.preview_ready),
                None => return,
            }
        };
        let mut refresh = false;
        {
            let mut inner = self.inner.borrow_mut();
            if inner.up_to_date != up_to_date || inner.draft_error != error || inner.preview_ready != ready {
                inner.up_to_date = up_to_date; inner.draft_error = error.clone(); inner.preview_ready = ready; refresh = true;
            }
            if inner.focused && inner.blink.elapsed() >= Duration::from_millis(530) {
                inner.caret_on = !inner.caret_on; inner.blink = Instant::now(); refresh = true;
            }
        }
        if refresh { self.surface.refresh(false, None); }
    }

    // ---- Editing ---------------------------------------------------------

    /// Applies an edit with undo, then updates the draft and repaints.
    fn mutate(&self, edit: impl FnOnce(&mut Editor)) {
        {
            let mut inner = self.inner.borrow_mut();
            let before = inner.editor.clone();
            edit(&mut inner.editor);
            if inner.editor.root != before.root {
                inner.undo.push(before);
                if inner.undo.len() > 200 { inner.undo.remove(0); }
                inner.redo.clear();
            }
            inner.caret_on = true; inner.blink = Instant::now();
        }
        self.changed();
    }

    /// Moves the caret; content only changes when a command resolves.
    fn navigate(&self, step: impl FnOnce(&mut Editor)) { self.mutate(step); }

    /// Sends the equation as it stands to the document, unfinished slots
    /// included, so the page follows every keystroke. The source field is
    /// rewritten only for a finished equation.
    fn changed(&self) {
        let (live, finished, differs) = {
            let mut inner = self.inner.borrow_mut();
            let live = inner.editor.to_node_live();
            let differs = inner.last_pushed.as_ref() != Some(&live);
            if differs { inner.last_pushed = Some(live.clone()); }
            (live, inner.editor.to_node(), differs)
        };
        if differs {
            let display = equation_ui::display(&self.state.borrow());
            let text = match &finished { Ok(root) => editor::equation_input::format_with_display(root, display), Err(_) => self.source.get_value() };
            if finished.is_ok() { self.set_source_text(&text); }
            equation_ui::set_root(&mut self.state.borrow_mut(), text, live);
        }
        self.surface.refresh(false, None);
    }

    fn set_source_text(&self, text: &str) {
        if self.source.get_value() == text { return; }
        self.syncing.set(true); self.source.change_value(text); self.syncing.set(false);
    }

    fn source_edited(&self) {
        if self.syncing.get() { return; }
        let text = self.source.get_value();
        let parsed = editor::equation_input::parse_with_display(&text);
        if let Ok((_, Some(display))) = &parsed { equation_ui::set_display(&mut self.state.borrow_mut(), *display); }
        let parsed = parsed.map(|(root, _)| root);
        {
            let mut inner = self.inner.borrow_mut();
            if let Ok(root) = &parsed {
                let before = inner.editor.clone();
                inner.editor = Editor::from_node(root);
                inner.undo.push(before);
            }
            inner.last_pushed = Some(parsed.clone());
        }
        equation_ui::set_root(&mut self.state.borrow_mut(), text, parsed);
        self.surface.refresh(false, None);
    }

    fn undo(&self, redo: bool) {
        {
            let mut inner = self.inner.borrow_mut();
            let Some(previous) = (if redo { inner.redo.pop() } else { inner.undo.pop() }) else { return; };
            let current = std::mem::replace(&mut inner.editor, previous);
            if redo { inner.undo.push(current); } else { inner.redo.push(current); }
        }
        self.changed();
    }

    fn insert_entry(&self, index: usize) {
        let tab = self.inner.borrow().tab;
        let Some(entry) = palette::tabs().get(tab).and_then(|t| t.entries.get(index)) else { return; };
        let insert = entry.insert.clone();
        self.mutate(|e| match insert {
            Insert::Atom(kind, text) => e.insert_atom(tree::Atom::new(kind, text)),
            Insert::Node(kind) => e.insert_node(kind),
        });
        self.focus();
    }

    /// Keyboard commands. Returns true when handled; printable input
    /// arrives through the character event.
    fn key(&self, code: i32, shift: bool, ctrl: bool) -> bool {
        if ctrl {
            match code {
                c if c == 'A' as i32 => self.navigate(|e| e.select_all()),
                c if c == 'Z' as i32 && shift => self.undo(true),
                c if c == 'Z' as i32 => self.undo(false),
                c if c == 'Y' as i32 => self.undo(true),
                _ => return false,
            }
            return true;
        }
        match code {
            WXK_LEFT => self.navigate(|e| e.move_left(shift)),
            WXK_RIGHT => self.navigate(|e| e.move_right(shift)),
            WXK_HOME => self.navigate(|e| e.home(shift)),
            WXK_END => self.navigate(|e| e.end(shift)),
            WXK_UP | WXK_DOWN => self.vertical(code == WXK_UP),
            WXK_TAB => self.navigate(|e| e.next_slot(shift)),
            WXK_BACK => self.mutate(|e| e.backspace()),
            WXK_DELETE => self.mutate(|e| e.delete_forward()),
            WXK_RETURN | WXK_NUMPAD_ENTER => {
                if self.inner.borrow().editor.has_command() { self.mutate(|e| e.resolve_command()); }
            }
            WXK_ESCAPE => {
                if self.inner.borrow().editor.has_command() {
                    self.mutate(|e| { while e.has_command() { e.backspace(); } });
                } else { (self.on_close)(); }
            }
            _ => return false,
        }
        true
    }

    /// Up/Down keep the caret's horizontal position.
    fn vertical(&self, up: bool) {
        let target = {
            let inner = self.inner.borrow();
            let Some(path) = inner.editor.vertical_target(up) else { return; };
            let Some((_, _, mbox)) = inner.formula.as_ref() else { return; };
            let cursor = &inner.editor.cursor;
            let x = mbox.row(&cursor.path).and_then(|r| r.carets.get(cursor.offset)).copied().unwrap_or(0.0);
            let offset = mbox.row(&path).map_or(0, |row| row.carets.iter().enumerate()
                .min_by(|a, b| (a.1 - x).abs().total_cmp(&(b.1 - x).abs())).map_or(0, |(i, _)| i));
            (path, offset)
        };
        self.navigate(|e| e.set_cursor(target.0, target.1));
    }

    // ---- Mouse -----------------------------------------------------------

    fn hit(&self, x: f32, y: f32) -> Option<Hit> {
        let inner = self.inner.borrow();
        let (x, y) = (x * inner.scale, y * inner.scale);
        inner.hits.iter().rev().find(|(r, _)| r.contains(x, y)).map(|(_, h)| *h)
    }

    fn formula_position(&self, x: f32, y: f32) -> Option<(Path, usize)> {
        let inner = self.inner.borrow();
        let (ox, oy, mbox) = inner.formula.as_ref()?;
        mbox.hit(x * inner.scale - ox, y * inner.scale - oy)
    }

    fn mouse_down(&self, x: f32, y: f32) {
        self.focus();
        let hit = self.hit(x, y);
        {
            let mut inner = self.inner.borrow_mut();
            inner.pressed = hit;
            inner.drag = None;
        }
        if self.surface.has_capture() { self.surface.release_mouse(); }
        if hit == Some(Hit::Formula) {
            if let Some((path, offset)) = self.formula_position(x, y) {
                self.inner.borrow_mut().drag = Some((path.clone(), offset));
                if !self.surface.has_capture() { self.surface.capture_mouse(); }
                self.navigate(|e| e.set_cursor(path, offset));
            }
        } else if hit.is_none() {
            self.navigate(|editor| editor.set_cursor(Vec::new(), editor.root.len()));
        }
        self.surface.refresh(false, None);
    }

    fn mouse_move(&self, x: f32, y: f32) {
        let drag = self.inner.borrow().drag.clone();
        if let Some((anchor_path, anchor)) = drag {
            if let Some((path, offset)) = self.formula_position(x, y) {
                self.navigate(|e| select_between(e, &anchor_path, anchor, &path, offset));
            }
            return;
        }
        let hit = self.hit(x, y);
        let changed = { let mut inner = self.inner.borrow_mut(); let changed = inner.hover != hit; inner.hover = hit; changed };
        if changed { self.surface.refresh(false, None); }
    }

    fn mouse_up(&self, x: f32, y: f32) {
        let (pressed, dragging) = { let mut inner = self.inner.borrow_mut(); (inner.pressed.take(), inner.drag.take().is_some()) };
        if self.surface.has_capture() { self.surface.release_mouse(); }
        if dragging { return; }
        let hit = self.hit(x, y);
        if hit != pressed { self.surface.refresh(false, None); return; }
        match hit {
            Some(Hit::Tab(index)) => { { let mut inner = self.inner.borrow_mut(); inner.tab = index; inner.palette_scroll = 0; } self.surface.refresh(false, None); }
            Some(Hit::Cell(index)) => self.insert_entry(index),
            Some(Hit::Matrix { add, column }) => { self.mutate(|e| e.matrix_edit(add, column)); self.focus(); }
            Some(Hit::Close) => (self.on_close)(),
            Some(Hit::Source) => {
                let show = { let mut inner = self.inner.borrow_mut(); inner.show_source = !inner.show_source; inner.show_source };
                self.source.show(show);
                self.pane.layout();
                self.surface.refresh(false, None);
            }
            _ => {}
        }
    }

    // ---- Painting --------------------------------------------------------

    fn paint(&self) {
        self.share_slot();
        let dc = AutoBufferedPaintDC::new(&self.surface);
        let size = self.surface.get_client_size();
        // Drawn at the screen's resolution, as the Ribbon is: on a HiDPI
        // screen an image at logical size is stretched and blurs every icon.
        let scale = crate::display_scale::window_scale(&self.surface);
        let (w, h) = ((size.width.max(1) as f32 * scale).round() as u32, (size.height.max(1) as f32 * scale).round() as u32);
        let k = self.surface.from_dip_int(1000) as f32 / 1000.0 * scale;
        let measure = |text: &str, style: TextStyle| { dc.set_font(self.font(style)); let (w, h) = dc.get_text_extent(&crate::i18n::text(&(text))); (w as f32 * scale, h as f32 * scale) };
        self.inner.borrow_mut().scale = scale;
        let Some(pixmap_and_texts) = self.build(w, h, k, &measure) else { return; };
        // Laid out now: put the cursor where the page was clicked (it paints again).
        if self.pending_click.get().is_some() { self.apply_pending_click(); }
        let (pixmap, texts) = pixmap_and_texts;
        if let Some(bitmap) = Bitmap::from_rgba(pixmap.data(), w, h) {
            let reduction = 1.0 / f64::from(scale);
            dc.set_user_scale(reduction, reduction);
            dc.draw_bitmap(&bitmap, 0, 0, false);
            dc.set_user_scale(1.0, 1.0);
        }
        dc.set_background_mode(wxdragon::dc::BackgroundMode::Transparent);
        for text in texts {
            dc.set_font(self.font(text.style));
            dc.set_text_foreground(text.colour.colour());
            dc.draw_text(&crate::i18n::text(&(&text.text)), (text.x as f32 / scale).round() as i32, (text.y as f32 / scale).round() as i32);
        }
    }

    fn font(&self, style: TextStyle) -> &Font {
        match style { TextStyle::Title => &self.fonts.title, TextStyle::Button => &self.fonts.body,
            TextStyle::Small => &self.fonts.small }
    }

    fn build(&self, w: u32, h: u32, k: f32, measure: &dyn Fn(&str, TextStyle) -> (f32, f32)) -> Option<(Pixmap, Vec<Text>)> {
        let fonts = Fonts::get()?;
        let mut pixmap = Pixmap::new(w, h)?;
        let mut texts: Vec<Text> = Vec::new();
        let mut hits: Vec<(Rect, Hit)> = Vec::new();
        let mut inner = self.inner.borrow_mut();
        let (wf, hf) = (w as f32, h as f32);
        pixmap.fill(tiny_skia::Color::from_rgba8(PANE.0, PANE.1, PANE.2, 255));
        let pad = 14.0 * k;
        let mut text = |s: &str, x: f32, y: f32, style, colour| texts.push(Text { text: s.to_string(), x: x.round() as i32, y: y.round() as i32, style, colour });

        // Header: the title and a close button, as the Styles pane has.
        text("Equation", pad, 12.0 * k, TextStyle::Title, INK);
        let close = Rect { x: wf - pad - 28.0 * k + 5.0 * k, y: 8.0 * k, w: 28.0 * k, h: 28.0 * k };
        if inner.hover == Some(Hit::Close) { fill_round(&mut pixmap, close, 6.0 * k, HOVER.paint(255)); }
        draw_close_mark(&mut pixmap, close, k);
        hits.push((close, Hit::Close));

        // Formula card.
        let card_x = pad; let card_w = wf - 2.0 * pad;
        let focus = inner.editor.cursor.path.clone();
        let mut size = 26.0 * k;
        let mut mbox = layout::layout(fonts, &inner.editor.root, size, &focus);
        let room = card_w - 32.0 * k;
        if mbox.w > room && mbox.w > 0.0 {
            size = (size * room / mbox.w).max(12.0 * k);
            mbox = layout::layout(fonts, &inner.editor.root, size, &focus);
        }
        let card_y = 44.0 * k;
        let card_h = (mbox.height() + 44.0 * k).clamp(112.0 * k, 0.42 * hf);
        let card = Rect { x: card_x, y: card_y, w: card_w, h: card_h };
        fill_round(&mut pixmap, Rect { y: card.y + 1.5 * k, ..card }, 10.0 * k, Rgb(20, 40, 80).paint(14));
        fill_round(&mut pixmap, card, 10.0 * k, CARD.paint(255));
        if inner.focused {
            stroke_round(&mut pixmap, card, 10.0 * k, ACCENT.paint(255), 1.6 * k, None);
        } else {
            stroke_round(&mut pixmap, card, 10.0 * k, BORDER.paint(255), 1.0 * k, None);
        }
        hits.push((card, Hit::Formula));
        let ox = (card_x + (card_w - mbox.w) * 0.5).round();
        let oy = (card_y + (card_h - mbox.height()) * 0.5 + mbox.asc).round();
        // The slot being edited is tinted, then any selection, the formula, and the caret.
        let cursor = inner.editor.cursor.clone();
        if inner.focused && !cursor.path.is_empty() {
            if let Some(row) = mbox.row(&cursor.path) {
                let r = Rect { x: ox + row.rect.x - 3.0 * k, y: oy + row.rect.y - 2.0 * k, w: row.rect.w + 6.0 * k, h: row.rect.h + 4.0 * k };
                fill_round(&mut pixmap, r, 4.0 * k, ACCENT_SOFT.paint(255));
            }
        }
        if let (true, Some((start, end)), Some(row)) = (inner.focused, inner.editor.selection(), mbox.row(&cursor.path)) {
            let (x0, x1) = (row.carets[start], row.carets[end]);
            let r = Rect { x: ox + x0, y: oy + row.caret_y.0 - 1.0 * k, w: x1 - x0, h: row.caret_y.1 - row.caret_y.0 + 2.0 * k };
            fill_round(&mut pixmap, r, 2.0 * k, ACCENT.paint(64));
        }
        draw_box(&mut pixmap, &mut inner.glyphs, fonts, &mbox, ox, oy, INK, k);
        if inner.focused && inner.caret_on && inner.editor.selection().is_none() {
            if let Some(row) = mbox.row(&cursor.path) {
                let x = ox + row.carets.get(cursor.offset).copied().unwrap_or(0.0);
                fill_rect(&mut pixmap, Rect { x: (x - 0.75 * k).round(), y: oy + row.caret_y.0, w: (1.5 * k).max(1.0), h: row.caret_y.1 - row.caret_y.0 }, ACCENT.paint(255));
            }
        }
        inner.formula = Some((ox, oy, mbox));

        // Status line.
        let mut y = card_y + card_h + 8.0 * k;
        let (status, colour) = self.status(&inner);
        let status = fit(&status, card_w, TextStyle::Small, measure);
        text(&status, pad + 2.0 * k, y, TextStyle::Small, colour);
        y += 22.0 * k;

        // Matrix tools, when the caret is in a matrix.
        if inner.editor.matrix().is_some() {
            let labels = [("+ Row", true, false), ("+ Column", true, true), ("− Row", false, false), ("− Column", false, true)];
            let mut x = pad;
            for (label, add, column) in labels {
                let (tw, _) = measure(label, TextStyle::Small);
                let r = Rect { x, y, w: tw + 18.0 * k, h: 24.0 * k };
                let hit = Hit::Matrix { add, column };
                let hovered = inner.hover == Some(hit);
                fill_round(&mut pixmap, r, 12.0 * k, if hovered { ACCENT_SOFT.paint(255) } else { CARD.paint(255) });
                stroke_round(&mut pixmap, r, 12.0 * k, if hovered { ACCENT_EDGE.paint(255) } else { BORDER.paint(255) }, 1.0 * k, None);
                let (_, th) = measure(label, TextStyle::Small);
                text(label, r.x + 9.0 * k, r.y + (r.h - th) * 0.5, TextStyle::Small, if hovered { ACCENT } else { INK });
                hits.push((r, hit));
                x += r.w + 6.0 * k;
            }
            y += 32.0 * k;
        }

        // Palette tabs.
        let tabs = palette::tabs();
        let track = Rect { x: pad, y, w: card_w, h: 34.0 * k };
        fill_round(&mut pixmap, track, 9.0 * k, TRACK.paint(255));
        let seg_w = (track.w - 6.0 * k) / tabs.len() as f32;
        for (index, tab) in tabs.iter().enumerate() {
            let r = Rect { x: track.x + 3.0 * k + seg_w * index as f32, y: track.y + 3.0 * k, w: seg_w, h: track.h - 6.0 * k };
            let active = inner.tab == index;
            if active {
                fill_round(&mut pixmap, Rect { y: r.y + 1.0 * k, ..r }, 7.0 * k, Rgb(20, 40, 80).paint(18));
                fill_round(&mut pixmap, r, 7.0 * k, CARD.paint(255));
            } else if inner.hover == Some(Hit::Tab(index)) {
                fill_round(&mut pixmap, r, 7.0 * k, Rgb(255, 255, 255).paint(120));
            }
            let face = symbol_box(fonts, tab.face, 16.0 * k);
            let colour = if active { ACCENT } else { MUTED };
            draw_box(&mut pixmap, &mut inner.glyphs, fonts, &face, (r.x + (r.w - face.w) * 0.5).round(),
                (r.y + (r.h - face.height()) * 0.5 + face.asc).round(), colour, k);
            hits.push((r, Hit::Tab(index)));
        }
        y += track.h + 10.0 * k;

        // Palette cells.
        let bottom_bar = 60.0 * k;
        let tab = &tabs[inner.tab.min(tabs.len() - 1)];
        let limit = hf - bottom_bar - 26.0 * k;
        // Cells shrink a little before the palette needs scrolling.
        let gap = 5.0 * k;
        let count = tab.entries.len();
        let fit_cell = |cell: f32| {
            let cols = (((card_w + gap) / (cell + gap)).floor() as usize).max(4);
            let rows = count.div_ceil(cols);
            (cols, rows, rows as f32 * (cell + gap) - gap <= limit - y)
        };
        let (cell, cols) = [40.0, 37.0, 34.0, 31.0].iter().map(|c| c * k)
            .find_map(|cell| { let (cols, _, fits) = fit_cell(cell); fits.then_some((cell, cols)) })
            .unwrap_or_else(|| (31.0 * k, fit_cell(31.0 * k).0));
        let visible_rows = (((limit - y + gap) / (cell + gap)).floor() as usize).max(1);
        let total_rows = count.div_ceil(cols);
        let first_row = inner.palette_scroll.min(total_rows.saturating_sub(visible_rows));
        inner.palette_scroll = first_row;
        inner.palette_rows = (visible_rows, total_rows);
        let grid_w = cols as f32 * (cell + gap) - gap;
        let gx = pad + (card_w - grid_w) * 0.5;
        let mut grid_bottom = y;
        for (index, entry) in tab.entries.iter().enumerate() {
            let (col, row) = (index % cols, index / cols);
            if row < first_row { continue; }
            let row = row - first_row;
            let r = Rect { x: gx + col as f32 * (cell + gap), y: y + row as f32 * (cell + gap), w: cell, h: cell };
            if r.y + r.h > limit { break; }
            let hovered = inner.hover == Some(Hit::Cell(index));
            fill_round(&mut pixmap, r, 8.0 * k, if hovered { ACCENT_SOFT.paint(255) } else { CARD.paint(255) });
            stroke_round(&mut pixmap, r, 8.0 * k, if hovered { ACCENT_EDGE.paint(255) } else { Rgb(226, 231, 239).paint(255) }, 1.0 * k, None);
            let row = palette_row(&entry.insert);
            let mut size = if matches!(entry.insert, Insert::Node(_)) { 15.0 * k } else { 19.0 * k };
            let mut preview = layout::preview(fonts, &row, size);
            let fit_to = cell - 10.0 * k;
            if preview.w > fit_to || preview.height() > fit_to {
                size *= (fit_to / preview.w.max(1.0)).min(fit_to / preview.height().max(1.0));
                preview = layout::preview(fonts, &row, size);
            }
            draw_box_with(&mut pixmap, &mut inner.glyphs, fonts, &preview, (r.x + (r.w - preview.w) * 0.5).round(),
                (r.y + (r.h - preview.height()) * 0.5 + preview.asc).round(), if hovered { ACCENT } else { INK }, k, true);
            hits.push((r, Hit::Cell(index)));
            grid_bottom = r.y + r.h;
        }
        let hint = match inner.hover {
            Some(Hit::Cell(index)) => tab.entries.get(index).map(|e| format!("{} · {}", e.name, e.shortcut)),
            Some(Hit::Tab(index)) => Some(tabs[index].title.to_string()),
            _ => None,
        }.unwrap_or_else(|| if total_rows > visible_rows { format!("{} · scroll for more", tab.title) } else { tab.title.to_string() });
        text(&fit(&hint, card_w, TextStyle::Small, measure), pad + 2.0 * k, grid_bottom + 8.0 * k, TextStyle::Small, MUTED);

        // Command bar.
        let bar_y = hf - bottom_bar;
        fill_rect(&mut pixmap, Rect { x: 0.0, y: bar_y, w: wf, h: 1.0_f32.max(k) }, BORDER.paint(255));
        let button_h = 32.0 * k;
        let by = bar_y + (bottom_bar - button_h) * 0.5;
        let source_label = if inner.show_source { "Hide source" } else { "Source" };
        let (sw, sh) = measure(source_label, TextStyle::Button);
        let source = Rect { x: pad - 6.0 * k, y: by, w: sw + 12.0 * k + 18.0 * k, h: button_h };
        let hovered = inner.hover == Some(Hit::Source);
        if hovered { fill_round(&mut pixmap, source, 8.0 * k, HOVER.paint(255)); }
        // A small "</>" mark before the label.
        let (mx, my) = (source.x + 8.0 * k, source.y + source.h * 0.5);
        let mark = MUTED.paint(255);
        stroke_poly(&mut pixmap, &[(mx + 4.0 * k, my - 4.0 * k), (mx, my), (mx + 4.0 * k, my + 4.0 * k)], &mark, 1.3 * k);
        stroke_poly(&mut pixmap, &[(mx + 8.0 * k, my - 4.0 * k), (mx + 12.0 * k, my), (mx + 8.0 * k, my + 4.0 * k)], &mark, 1.3 * k);
        text(source_label, source.x + 24.0 * k, source.y + (source.h - sh) * 0.5, TextStyle::Button, if hovered { INK } else { MUTED });
        hits.push((source, Hit::Source));

        inner.hits = hits;
        drop(text);
        Some((pixmap, texts))
    }

    fn status(&self, inner: &Inner) -> (String, Rgb) {
        if let Some(prefix) = inner.editor.pending_command() {
            let options = palette::completions(prefix);
            return match (palette::command(prefix), options.first()) {
                (Some(_), _) => (format!("\\{prefix} — press Space or Enter to insert"), ACCENT),
                (None, Some(_)) => (format!("\\{prefix}… {}", options.iter().map(|(n, g)| format!("{n} {g}")).collect::<Vec<_>>().join(" · ")), ACCENT),
                (None, None) => (format!("\\{prefix} — unknown command; Space keeps it as letters"), MUTED),
            };
        }
        if inner.editor.root.is_empty() { return ("Type an equation, or Typst math under Source".into(), MUTED); }
        if let Err(message) = inner.editor.to_node() { return (message, MUTED); }
        if let Some(error) = &inner.draft_error { return (format!("Typst cannot render this: {}", error.lines().next().unwrap_or(error)), ERROR); }
        if !inner.up_to_date { return ("Updating the equation…".into(), MUTED); }
        let tip = if inner.editor.matrix().is_some() { "Tab moves between cells · the buttons above add rows and columns" }
            else if !inner.editor.cursor.path.is_empty() { "↑ ↓ move between slots · Tab or → leaves the box" }
            else { "/ fraction  ·  ^ power  ·  _ subscript  ·  \\ symbols" };
        (tip.into(), MUTED)
    }
}

/// Selects from one editing position to another, widening to the nearest
/// row that contains both.
fn select_between(editor: &mut Editor, a_path: &Path, a: usize, b_path: &Path, b: usize) {
    if a_path == b_path { editor.select(a_path.clone(), a, b); return; }
    let common = a_path.iter().zip(b_path).take_while(|(x, y)| x == y).count();
    let span = |path: &Path, offset: usize| if path.len() == common { (offset, offset) } else { (path[common].0, path[common].0 + 1) };
    let (sa, sb) = (span(a_path, a), span(b_path, b));
    let base = a_path[..common].to_vec();
    if sa.0 <= sb.0 { editor.select(base, sa.0, sb.1.max(sa.1)); } else { editor.select(base, sa.1.max(sb.1), sb.0); }
}

fn position(event: &WindowEventData) -> Option<(f32, f32)> {
    let p = match event {
        WindowEventData::MouseButton(m) => m.event.get_position(),
        WindowEventData::MouseMotion(m) => m.event.get_position(),
        _ => None,
    }?;
    Some((p.x as f32, p.y as f32))
}

/// Shortens `text` with an ellipsis to fit `width`.
fn fit(text: &str, width: f32, style: TextStyle, measure: &dyn Fn(&str, TextStyle) -> (f32, f32)) -> String {
    if measure(text, style).0 <= width { return text.to_string(); }
    let mut chars: Vec<char> = text.chars().collect();
    while !chars.is_empty() {
        chars.pop();
        let candidate: String = chars.iter().collect::<String>() + "…";
        if measure(&candidate, style).0 <= width { return candidate; }
    }
    String::new()
}

fn symbol_box(fonts: &Fonts, text: &str, size: f32) -> MathBox {
    let kind = if text.chars().next().is_some_and(char::is_alphabetic) { tree::AtomKind::Ident } else { tree::AtomKind::Symbol };
    let mut row: Vec<Item> = Vec::new();
    for c in text.chars() {
        row.push(if c == '²' { Item::Node(Node { kind: tree::Kind::Sup, slots: vec![vec![Item::Atom(tree::Atom::new(tree::AtomKind::Number, "2"))]] }) }
            else { Item::Atom(tree::Atom::new(kind, c.to_string())) });
    }
    layout::preview(fonts, &row, size)
}

/// What a palette cell shows: the structure with empty slots, and a muted
/// base for scripts.
fn palette_row(insert: &Insert) -> Vec<Item> {
    match insert {
        Insert::Atom(kind, text) => vec![Item::Atom(tree::Atom::new(*kind, text.clone()))],
        Insert::Node(kind) if matches!(kind, tree::Kind::Sup | tree::Kind::Sub | tree::Kind::SubSup) =>
            vec![Item::Node(Node { kind: tree::Kind::Group, slots: vec![Vec::new()] }), Item::Node(Node::new(kind.clone()))],
        Insert::Node(tree::Kind::Text) => vec![Item::Node(Node { kind: tree::Kind::Text,
            slots: vec!["abc".chars().map(|c| Item::Atom(tree::Atom::new(tree::AtomKind::Text, c.to_string()))).collect()] })],
        Insert::Node(kind) => vec![Item::Node(Node::new(kind.clone()))],
    }
}

// ---- tiny-skia helpers ------------------------------------------------------

/// The panes' red cross, centred in `area`.
fn draw_close_mark(pixmap: &mut Pixmap, area: Rect, k: f32) {
    let size = (crate::ribbon::CLOSE_MARK_DIP * k).round().max(1.0) as u32;
    let Some(rgba) = crate::ribbon::close_mark(size) else { return; };
    let Some(mut mark) = Pixmap::new(size, size) else { return; };
    for (target, source) in mark.data_mut().as_chunks_mut::<4>().0.iter_mut().zip(rgba.as_chunks::<4>().0) {
        let alpha = u16::from(source[3]);
        let premultiply = |c: u8| ((u16::from(c) * alpha + 127) / 255) as u8;
        *target = [premultiply(source[0]), premultiply(source[1]), premultiply(source[2]), source[3]];
    }
    let x = (area.x + (area.w - size as f32) * 0.5).round() as i32;
    let y = (area.y + (area.h - size as f32) * 0.5).round() as i32;
    pixmap.draw_pixmap(x, y, mark.as_ref(), &tiny_skia::PixmapPaint::default(), Transform::identity(), None);
}

fn round_path(r: Rect, radius: f32) -> Option<tiny_skia::Path> {
    let radius = radius.min(r.w * 0.5).min(r.h * 0.5).max(0.0);
    let (x0, y0, x1, y1) = (r.x, r.y, r.x + r.w, r.y + r.h);
    let c = radius * 0.4477;
    let mut p = PathBuilder::new();
    p.move_to(x0 + radius, y0);
    p.line_to(x1 - radius, y0); p.cubic_to(x1 - c, y0, x1, y0 + c, x1, y0 + radius);
    p.line_to(x1, y1 - radius); p.cubic_to(x1, y1 - c, x1 - c, y1, x1 - radius, y1);
    p.line_to(x0 + radius, y1); p.cubic_to(x0 + c, y1, x0, y1 - c, x0, y1 - radius);
    p.line_to(x0, y0 + radius); p.cubic_to(x0, y0 + c, x0 + c, y0, x0 + radius, y0);
    p.close();
    p.finish()
}

fn fill_round(pixmap: &mut Pixmap, r: Rect, radius: f32, paint: Paint) {
    if let Some(path) = round_path(r, radius) { pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None); }
}

fn stroke_round(pixmap: &mut Pixmap, r: Rect, radius: f32, paint: Paint, width: f32, dash: Option<StrokeDash>) {
    let inset = width * 0.5;
    let r = Rect { x: r.x + inset, y: r.y + inset, w: r.w - width, h: r.h - width };
    if let Some(path) = round_path(r, radius) {
        let stroke = Stroke { width, dash, ..Stroke::default() };
        pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }
}

fn fill_rect(pixmap: &mut Pixmap, r: Rect, paint: Paint) {
    if let Some(rect) = tiny_skia::Rect::from_xywh(r.x, r.y, r.w.max(0.5), r.h.max(0.5)) {
        pixmap.fill_path(&PathBuilder::from_rect(rect), &paint, FillRule::Winding, Transform::identity(), None);
    }
}

fn stroke_poly(pixmap: &mut Pixmap, points: &[(f32, f32)], paint: &Paint, width: f32) {
    let mut p = PathBuilder::new();
    for (i, (x, y)) in points.iter().enumerate() { if i == 0 { p.move_to(*x, *y) } else { p.line_to(*x, *y) } }
    if let Some(path) = p.finish() {
        let stroke = Stroke { width, line_cap: tiny_skia::LineCap::Round, line_join: tiny_skia::LineJoin::Round, ..Stroke::default() };
        pixmap.stroke_path(&path, paint, &stroke, Transform::identity(), None);
    }
}

struct Outline(PathBuilder);
impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) { self.0.move_to(x, y); }
    fn line_to(&mut self, x: f32, y: f32) { self.0.line_to(x, y); }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) { self.0.quad_to(x1, y1, x, y); }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) { self.0.cubic_to(x1, y1, x2, y2, x, y); }
    fn close(&mut self) { self.0.close(); }
}

/// Paints a laid-out formula with its baseline origin at (ox, oy).
fn draw_box(pixmap: &mut Pixmap, glyphs: &mut HashMap<(FontId, u16), Option<tiny_skia::Path>>, fonts: &Fonts,
    mbox: &MathBox, ox: f32, oy: f32, ink: Rgb, k: f32) {
    draw_box_with(pixmap, glyphs, fonts, mbox, ox, oy, ink, k, false);
}

#[allow(clippy::too_many_arguments)]
fn draw_box_with(pixmap: &mut Pixmap, glyphs: &mut HashMap<(FontId, u16), Option<tiny_skia::Path>>, fonts: &Fonts,
    mbox: &MathBox, ox: f32, oy: f32, ink: Rgb, k: f32, solid: bool) {
    let colour = |i: Ink| match i { Ink::Normal => ink, Ink::Muted => MUTED, Ink::Accent => ACCENT };
    for draw in &mbox.draws {
        match draw {
            Draw::Glyph { font, id, x, y, size, ink: i } => {
                let path = glyphs.entry((*font, *id)).or_insert_with(|| {
                    let mut outline = Outline(PathBuilder::new());
                    fonts.face(*font).outline_glyph(ttf_parser::GlyphId(*id), &mut outline)?;
                    outline.0.finish()
                });
                if let Some(path) = path {
                    let scale = size / fonts.face(*font).units_per_em() as f32;
                    let transform = Transform::from_row(scale, 0.0, 0.0, -scale, ox + x, oy + y);
                    pixmap.fill_path(path, &colour(*i).paint(255), FillRule::Winding, transform, None);
                }
            }
            Draw::Rule { rect, ink: i } => fill_rect(pixmap, Rect { x: ox + rect.x, y: oy + rect.y, ..*rect }, colour(*i).paint(255)),
            Draw::Line { points, width, ink: i } => {
                let points: Vec<(f32, f32)> = points.iter().map(|(x, y)| (ox + x, oy + y)).collect();
                stroke_poly(pixmap, &points, &colour(*i).paint(255), *width);
            }
            Draw::Placeholder { rect, active, faint } => {
                let r = Rect { x: ox + rect.x, y: oy + rect.y, ..*rect };
                if solid {
                    fill_round(pixmap, r, 2.0 * k, ink.paint(if *faint { 40 } else { 70 }));
                } else if *active {
                    fill_round(pixmap, r, 3.0 * k, ACCENT_SOFT.paint(255));
                    stroke_round(pixmap, r, 3.0 * k, ACCENT.paint(255), 1.2 * k, None);
                } else {
                    let dash = StrokeDash::new(vec![2.5 * k, 2.0 * k], 0.0);
                    stroke_round(pixmap, r, 3.0 * k, ACCENT_EDGE.paint(if *faint { 150 } else { 255 }), 1.1 * k, dash);
                }
            }
            Draw::Chip { rect, ink: i } => {
                let r = Rect { x: ox + rect.x, y: oy + rect.y, ..*rect };
                let fill = match i { Ink::Accent => ACCENT_SOFT, _ => Rgb(238, 241, 245) };
                fill_round(pixmap, r, 4.0 * k, fill.paint(255));
            }
        }
    }
}
