//! Format ▾ dialogs of Modify Style, modelled on Microsoft Word: Font,
//! Paragraph, Tabs, Borders and Shading, Language, Frame, Numbering and
//! Bullets, Customize Keyboard (Shortcut key) and Format Text Effects.
//!
//! Every dialog edits a copy of the Modify Style draft; OK writes it back.
//! A value is stored only when the user changed it from what the dialog showed
//! (the style's own value, else the inherited one), so opening and confirming
//! a dialog never turns inherited values into explicit ones.
//!
//! Controls without dedicated layout properties are retained in the style's
//! formatting map, so they survive Save, undo and document reload.
use super::*;
use super::styles::{AUTOMATIC, NO_COLOUR, colour_value, fill_colour_list, bind_colour_list, paint_rounded_surface,
    PreviewModel, PreviewParagraph, preview_from, paint_style_preview, hex_colour};
use crate::{BordersDialogUi, FontDialogUi, FrameDialogUi, LanguageDialogUi, NumberingDialogUi, ParagraphDialogUi,
    ShortcutDialogUi, TabsDialogUi, TextEffectsDialogUi};
use model::{Border, BorderSet, BorderStyle, KerningMode, LigatureMode, ParagraphDirection, Style, StyleId, StyleSheet,
    TabAlignment, TabLeader, TabStop};
use std::collections::BTreeMap;
use wxdragon::font::{Font, FontFamily, FontStyle, FontWeight};
use wxdragon::{BrushStyle, DeviceContext, PenStyle, dc::AutoBufferedPaintDC, window::BackgroundStyle,
    event::window_events::WindowEvents,
    widgets::{BitmapComboBox, CheckBox, ListBox, RadioButton, SpinCtrlDouble, TextCtrl, panel::Panel}};

/// The Format ▾ menu, in Word's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FormatItem { Font, Paragraph, Tabs, Border, Language, Frame, Numbering, Shortcut, TextEffects }
impl FormatItem {
    pub(super) const ALL: [FormatItem; 9] = [FormatItem::Font, FormatItem::Paragraph, FormatItem::Tabs, FormatItem::Border,
        FormatItem::Language, FormatItem::Frame, FormatItem::Numbering, FormatItem::Shortcut, FormatItem::TextEffects];
    pub(super) fn menu_label(self) -> &'static str {
        match self {
            FormatItem::Font => "Font…", FormatItem::Paragraph => "Paragraph…", FormatItem::Tabs => "Tabs…",
            FormatItem::Border => "Border…", FormatItem::Language => "Language…", FormatItem::Frame => "Frame…",
            FormatItem::Numbering => "Numbering…", FormatItem::Shortcut => "Shortcut key…",
            FormatItem::TextEffects => "Text Effects…",
        }
    }
    /// Items that only make sense for paragraph styles.
    pub(super) fn paragraph_only(self) -> bool {
        matches!(self, FormatItem::Paragraph | FormatItem::Tabs | FormatItem::Border | FormatItem::Frame | FormatItem::Numbering)
    }
}

/// Shared handle: nested dialogs (Text Effects from Font, Tabs from
/// Paragraph) remember into the same session store.
pub(super) type Pending = Rc<RefCell<PendingFormat>>;
/// Advanced control values remembered while Modify Style is open.
#[derive(Clone, Default)]
pub(super) struct PendingFormat { values: BTreeMap<String, String>, inherited: BTreeMap<String, String> }
impl PendingFormat {
    pub(super) fn from_style(style: &Style, sheet: &StyleSheet) -> Self {
        Self { values: style.formatting.clone(), inherited: sheet.resolve_formatting(style.based_on.as_ref()) }
    }
    fn get(&self, key: &str) -> Option<&String> { self.values.get(key).or_else(|| self.inherited.get(key)) }
    fn set(&mut self, key: &str, value: String) {
        if self.inherited.get(key) == Some(&value) { self.values.remove(key); }
        else { self.values.insert(key.to_owned(), value); }
    }
}

/// Open one Format dialog on `style`. Returns true when the user chose OK.
pub(super) fn run(item: FormatItem, parent: Dialog, style: &mut Style, sheet: &StyleSheet, pending: &Pending) -> bool {
    // Each child dialog gets its own draft. Nested dialogs may change it, but
    // Cancel on the parent must discard those changes as well.
    let mut starting = pending.borrow().clone();
    starting.inherited = sheet.resolve_formatting(style.based_on.as_ref());
    let draft = Rc::new(RefCell::new(starting));
    let mut style_draft = style.clone();
    let accepted = match item {
        FormatItem::Font => font_dialog(parent, &mut style_draft, sheet, &draft),
        FormatItem::Paragraph => paragraph_dialog(parent, &mut style_draft, sheet, &draft),
        FormatItem::Tabs => tabs_dialog(parent, &mut style_draft, sheet, &draft),
        FormatItem::Border => borders_dialog(parent, &mut style_draft, sheet, &draft),
        FormatItem::Language => language_dialog(parent, &mut style_draft, sheet, &draft),
        FormatItem::Frame => frame_dialog(parent, &draft),
        FormatItem::Numbering => numbering_dialog(parent, &draft),
        FormatItem::Shortcut => shortcut_dialog(parent, &mut style_draft, sheet, &draft),
        FormatItem::TextEffects => text_effects_dialog(parent, &draft),
    };
    if accepted {
        let values = draft.borrow().values.clone();
        style_draft.formatting = values.clone();
        *style = style_draft;
        let mut pending = pending.borrow_mut();
        pending.values = values;
        pending.inherited = draft.borrow().inherited.clone();
    }
    accepted
}

// Shared pieces ----------------------------------------------------------------------

const CHROME: (u8, u8, u8) = (232, 236, 242);
const LINE: (u8, u8, u8) = (223, 228, 236);
const LINE_STRONG: (u8, u8, u8) = (202, 211, 223);
const INK: (u8, u8, u8) = (36, 48, 66);
const MUTED: (u8, u8, u8) = (107, 119, 138);
const GHOST: (u8, u8, u8) = (163, 174, 191);
const ACCENT: (u8, u8, u8) = (58, 104, 194);
const ACCENT_SOFT: (u8, u8, u8) = (229, 237, 252);
const ACCENT_LINE: (u8, u8, u8) = (181, 200, 238);
const HOVER: (u8, u8, u8) = (238, 242, 248);
fn rgb(c: (u8, u8, u8)) -> Colour { Colour::rgb(c.0, c.1, c.2) }
fn dip(window: &impl WxWidget, v: i32) -> i32 { window.from_dip(Size::new(v, v)).width }
fn px_per_pt(window: &impl WxWidget) -> f32 { window.from_dip(Size::new(960, 960)).width as f32 / 720.0 }

/// Store a value only when it differs from what the dialog showed.
fn changed<T: PartialEq + Clone>(own: &Option<T>, shown: &T, new: T) -> Option<T> {
    if &new == shown { own.clone() } else { Some(new) }
}

/// A control whose value can be remembered as text.
#[derive(Clone, Copy)]
enum Ctl { Combo(ComboBox), Check(CheckBox), Radio(RadioButton), Spin(SpinCtrlDouble), Colour(BitmapComboBox, Dialog, bool) }
impl Ctl {
    fn get(&self) -> String {
        match self {
            Ctl::Combo(c) => crate::i18n::control_value(&c.get_name(), &c.get_value()), Ctl::Check(c) => c.is_checked().to_string(),
            Ctl::Radio(r) => r.get_value().to_string(), Ctl::Spin(s) => s.get_value().to_string(),
            Ctl::Colour(c, _, _) => c.get_value(),
        }
    }
    fn set(&self, value: &str) {
        match self {
            Ctl::Combo(c) => c.set_value(&crate::i18n::text(value)), Ctl::Check(c) => c.set_value(value == "true"),
            Ctl::Radio(r) => if value == "true" { r.set_value(true) },
            Ctl::Spin(s) => if let Ok(n) = value.parse() { s.set_value(n) },
            Ctl::Colour(c, window, empty) => fill_colour_list(*c, window, colour_value(value).as_deref(), if *empty { NO_COLOUR } else { AUTOMATIC }),
        }
    }
}
fn control_snapshot(controls: &[(&str, Ctl)]) -> BTreeMap<String, String> {
    controls.iter().map(|(key, control)|((*key).to_owned(),control.get())).collect()
}
fn restore(pending: &RefCell<PendingFormat>, controls: &[(&str, Ctl)]) -> BTreeMap<String, String> {
    let pending = pending.borrow();
    for (key, control) in controls { if let Some(value) = pending.get(key) { control.set(value); } }
    control_snapshot(controls)
}
fn remember_changed(pending: &RefCell<PendingFormat>, controls: &[(&str, Ctl)], before: &BTreeMap<String, String>) {
    let mut pending = pending.borrow_mut();
    for (key, control) in controls {
        let value=control.get();
        if before.get(*key)!=Some(&value) { pending.set(key, value); }
    }
}

/// Paint `button` with the Ribbon skin. `label` is what the face shows.
fn skin(button: Button, kind: &'static str, label: &'static str, icon: &'static str) -> bool {
    if !icon.is_empty() {
        if let Some(art) = art::load(icon, 16, 16) {
            button.set_label(&crate::i18n::text(&(""))); bitmap_button::set_bundle(&button, &art.normal, ButtonBitmapPosition::Left);
            button.set_bitmap_bundle_disabled(&art.disabled);
        }
    }
    skin::install(&ButtonSpec { button, name: "format_dialog", label, icon, action: Action::StyleModify, kind, segment: "single",
        tip: label, disabled: false, width_dip: 88, height_dip: 28, art_width_dip: 16, art_height_dip: 16 })
}

/// Dialog chrome shared by all Format dialogs: Ribbon-style tabs joined to the
/// white rounded card, one page per tab, native footer buttons, and a size
/// that fits the largest page (so switching tabs never resizes the dialog).
pub(super) struct Chrome { dialog: Dialog, card: Panel, tabs: Vec<Button>, pages: Vec<Panel>, current: Cell<usize> }
impl Chrome {
    pub(super) fn new(dialog: Dialog, card: Panel, tabs: &[(Button, &'static str)], pages: &[Panel], ok: (Button, &'static str),
           cancel: Option<Button>, footer: &[(Button, &'static str)], card_buttons: &[(Button, &'static str)]) -> Rc<Chrome> {
        let chrome = Rc::new(Chrome { dialog, card, tabs: tabs.iter().map(|t| t.0).collect(), pages: pages.to_vec(), current: Cell::new(0) });
        for &(tab, label) in tabs { skin(tab, "tab", label, ""); }
        ok.0.set_label(&crate::i18n::text(ok.1));
        for &(button, label) in footer { button.set_label(&crate::i18n::text(label)); }
        for &(button, label) in card_buttons { skin(button, "pane", label, ""); }
        card.set_background_style(BackgroundStyle::Paint);
        let weak = Rc::downgrade(&chrome);
        card.on_paint(move |_| {
            let dc = AutoBufferedPaintDC::new(&card);
            paint_rounded_surface(&dc, &card, rgb(CHROME), rgb(LINE), 7);
            // The selected tab flows into the card: no border under it.
            if let Some(chrome) = weak.upgrade() {
                if let Some(tab) = chrome.tabs.get(chrome.current.get()) {
                    let stroke = dip(&card, 1).max(1);
                    let x = tab.get_position().x - card.get_position().x;
                    dc.set_brush(Colour::rgb(255, 255, 255), BrushStyle::Solid); dc.set_pen(Colour::rgb(255, 255, 255), 1, PenStyle::Solid);
                    dc.draw_rectangle(x + stroke, 0, tab.get_size().width - 2 * stroke, stroke + 1);
                }
            }
        });
        card.on_size(move |event| { card.refresh(false, None); event.skip(true); });
        for (index, &(tab, _)) in tabs.iter().enumerate() {
            let weak = Rc::downgrade(&chrome);
            tab.on_click(move |_| if let Some(chrome) = weak.upgrade() { chrome.select(index); });
        }
        dialog.set_escape_id(cancel.map(|c| c.get_id()).unwrap_or(ok.0.get_id()));
        ok.0.on_click(move |_| dialog.end_modal(ID_OK));
        if let Some(cancel) = cancel { cancel.on_click(move |_| dialog.end_modal(wxdragon::id::ID_CANCEL)); }
        chrome
    }
    fn select(&self, index: usize) {
        self.current.set(index);
        for (i, page) in self.pages.iter().enumerate() { page.show(i == index); }
        for (i, tab) in self.tabs.iter().enumerate() { skin::selected(tab, i == index); }
        self.dialog.layout(); self.card.refresh(false, None);
        for tab in &self.tabs { tab.refresh(false, None); }
    }
    /// Size to the largest page, select the first page and show modally.
    pub(super) fn show(&self) -> bool {
        let mut best = Size::new(0, 0);
        for index in 0..self.pages.len().max(1) {
            for (i, page) in self.pages.iter().enumerate() { page.show(i == index); }
            self.dialog.set_min_size(Size::new(-1, -1)); self.dialog.layout();
            let size = self.dialog.get_best_size();
            best = Size::new(best.width.max(size.width), best.height.max(size.height));
        }
        self.select(0);
        self.dialog.set_min_size(best); self.dialog.set_size(best); self.dialog.layout();
        crate::placement::centre(&self.dialog);
        self.dialog.show_modal() == ID_OK
    }
}

/// A painted picker: a grid of tiles (Word's Setting column, galleries and
/// wrapping choices). `draw` paints one tile's content inside its rectangle.
type TileDraw = Box<dyn Fn(&AutoBufferedPaintDC, &Panel, (i32, i32, i32, i32), usize, bool)>;
struct Tiles { panel: Panel, count: usize, cols: usize, tile_h: i32, selected: Cell<usize>, hover: Cell<Option<usize>>,
    draw: TileDraw, changed: RefCell<Option<Box<dyn Fn(usize)>>> }
impl Tiles {
    fn new(panel: Panel, count: usize, cols: usize, tile_h_dip: i32, draw: TileDraw) -> Rc<Tiles> {
        let tiles = Rc::new(Tiles { panel, count, cols, tile_h: tile_h_dip, selected: Cell::new(0), hover: Cell::new(None), draw, changed: RefCell::new(None) });
        panel.set_background_style(BackgroundStyle::Paint);
        let weak = Rc::downgrade(&tiles);
        panel.on_paint(move |_| if let Some(t) = weak.upgrade() { t.paint(); });
        panel.on_size(move |event| { panel.refresh(false, None); event.skip(true); });
        let weak = Rc::downgrade(&tiles);
        panel.on_mouse_left_down(move |event| {
            if let Some(t) = weak.upgrade() {
                panel.set_focus();
                let position = match &event { WindowEventData::MouseButton(m) => m.event.get_position(), _ => None };
                if let Some(index) = position.and_then(|p| t.at(p.x, p.y)) { t.select(index); }
            }
            event.skip(true);
        });
        let weak = Rc::downgrade(&tiles);
        panel.on_mouse_motion(move |event| {
            if let Some(t) = weak.upgrade() {
                let position = match &event { WindowEventData::MouseMotion(m) => m.event.get_position(), _ => None };
                let hover = position.and_then(|p| t.at(p.x, p.y));
                if t.hover.replace(hover) != hover { panel.refresh(false, None); }
            }
            event.skip(true);
        });
        let weak = Rc::downgrade(&tiles);
        panel.on_mouse_leave(move |event| { if let Some(t) = weak.upgrade() { t.hover.set(None); panel.refresh(false, None); } event.skip(true); });
        let weak = Rc::downgrade(&tiles);
        panel.on_key_down(move |event| {
            if let (Some(t), WindowEventData::Keyboard(key)) = (weak.upgrade(), &event) {
                let current = t.selected.get() as i32; let cols = t.cols as i32;
                let target = match key.get_key_code() {
                    Some(wxdragon::keycode::WXK_LEFT) => current - 1, Some(wxdragon::keycode::WXK_RIGHT) => current + 1,
                    Some(wxdragon::keycode::WXK_UP) => current - cols, Some(wxdragon::keycode::WXK_DOWN) => current + cols,
                    _ => { event.skip(true); return; }
                };
                if (0..t.count as i32).contains(&target) { t.select(target as usize); }
                return;
            }
            event.skip(true);
        });
        tiles
    }
    fn rects(&self) -> Vec<(i32, i32, i32, i32)> {
        let size = self.panel.get_client_size();
        let (gap, inset) = (dip(&self.panel, 6), dip(&self.panel, 6));
        let cols = self.cols.max(1) as i32;
        let w = (size.width - 2 * inset - (cols - 1) * gap) / cols;
        let rows = self.count.div_ceil(cols as usize).max(1) as i32;
        let available = (size.height - 2 * inset - (rows - 1) * gap) / rows;
        let h = dip(&self.panel, self.tile_h).min(available).max(1);
        (0..self.count).map(|i| {
            let (c, r) = ((i as i32) % cols, (i as i32) / cols);
            (inset + c * (w + gap), inset + r * (h + gap), w, h)
        }).collect()
    }
    fn at(&self, x: i32, y: i32) -> Option<usize> {
        self.rects().iter().position(|&(rx, ry, rw, rh)| x >= rx && x < rx + rw && y >= ry && y < ry + rh)
    }
    fn select(&self, index: usize) {
        self.selected.set(index); self.panel.refresh(false, None);
        if let Some(changed) = self.changed.borrow().as_ref() { changed(index); }
    }
    fn on_change(&self, f: impl Fn(usize) + 'static) { self.changed.replace(Some(Box::new(f))); }
    fn paint(&self) {
        let dc = AutoBufferedPaintDC::new(&self.panel);
        paint_rounded_surface(&dc, &self.panel, Colour::rgb(255, 255, 255),
            if self.panel.has_focus() { rgb(ACCENT) } else { rgb(LINE) }, 5);
        for (index, rect) in self.rects().into_iter().enumerate() {
            let selected = self.selected.get() == index;
            let (fill, border) = if selected { (rgb(ACCENT_SOFT), rgb(ACCENT_LINE)) }
                else if self.hover.get() == Some(index) { (rgb(HOVER), rgb(LINE)) } else { (Colour::rgb(255, 255, 255), rgb(LINE)) };
            dc.set_brush(fill, BrushStyle::Solid); dc.set_pen(border, dip(&self.panel, 1).max(1), PenStyle::Solid);
            dc.draw_rounded_rectangle(rect.0, rect.1, rect.2, rect.3, dip(&self.panel, 5) as f64);
            (self.draw)(&dc, &self.panel, rect, index, selected);
        }
    }
}

/// Keep a text box and its list in step, as in Word's Font dialog.
fn link_list(text: TextCtrl, list: ListBox, changed: Rc<dyn Fn()>) {
    {
        let changed = Rc::clone(&changed);
        list.on_selection_changed(move |_| { if let Some(value) = list.get_string_selection() { text.change_value(&value); } changed(); });
    }
    text.on_text_updated(move |_| {
        let value = text.get_value().trim().to_lowercase();
        if let Some(i) = (0..list.get_count()).find(|&i| list.get_string(i).is_some_and(|s| s.to_lowercase() == value)) {
            list.set_selection(i, true); list.ensure_visible(i as i32);
        }
        changed();
    });
}
fn select_in_list(text: TextCtrl, list: ListBox, value: &str) {
    text.change_value(value);
    if let Some(i) = (0..list.get_count()).find(|&i| list.get_string(i).is_some_and(|s| s.eq_ignore_ascii_case(value))) {
        list.set_selection(i, true); list.ensure_visible(i as i32);
    }
}
fn painted(panel: Panel, paint: impl Fn(&AutoBufferedPaintDC, &Panel) + 'static) {
    panel.set_background_style(BackgroundStyle::Paint);
    panel.on_paint(move |_| { let dc = AutoBufferedPaintDC::new(&panel); paint(&dc, &panel); });
    panel.on_size(move |event| { panel.refresh(false, None); event.skip(true); });
}
fn face_font(points: f32, bold: bool, italic: bool, underline: bool, face: &str) -> Option<Font> {
    Font::new_with_details((points.round() as i32).max(1), FontFamily::Default as i32,
        if italic { FontStyle::Italic as i32 } else { FontStyle::Normal as i32 },
        if bold { FontWeight::Bold as i32 } else { FontWeight::Normal as i32 }, underline, face)
}
fn combo_index(combo: ComboBox, value: &str) { combo.set_value(&crate::i18n::text(value)); }

// Font ----------------------------------------------------------------------------------

/// Everything the Font preview shows, read from the dialog's controls.
struct FontLook { face: String, size: f32, bold: bool, italic: bool, underline: String, colour: Colour, underline_colour: Option<Colour>,
    highlight: Option<Colour>, strike: bool, double_strike: bool, superscript: bool, subscript: bool, small_caps: bool,
    all_caps: bool, hidden: bool, scale: f32, spacing_pt: f32, position_pt: f32 }

fn read_font_look(ui: &FontDialogUi) -> FontLook {
    let style = crate::i18n::source(&ui.fnt_style.get_value()).to_lowercase();
    let spacing = ui.fnt_spacing_by.get_value() as f32;
    let position = ui.fnt_position_by.get_value() as f32;
    FontLook {
        face: ui.fnt_family.get_value().trim().to_owned(),
        size: ui.fnt_size.get_value().trim().parse::<f32>().ok().filter(|n| n.is_finite() && *n > 0.0).unwrap_or(11.0),
        bold: style.contains("bold"), italic: style.contains("italic"),
        underline: crate::i18n::choice_source(&ui.fnt_underline.get_value(), &["(none)", "Words only", "Single", "Double", "Thick", "Dotted", "Dashed", "Dot-dash", "Dot-dot-dash", "Wave"]),
        colour: colour_value(&crate::i18n::source(&ui.fnt_color.get_value())).and_then(|h| hex_colour(&h)).unwrap_or(rgb(INK)),
        underline_colour: colour_value(&crate::i18n::source(&ui.fnt_underline_color.get_value())).and_then(|h| hex_colour(&h)),
        highlight: colour_value(&ui.fnt_highlight.get_value()).and_then(|h| hex_colour(&h)),
        strike: ui.fnt_strike.is_checked(), double_strike: ui.fnt_double_strike.is_checked(),
        superscript: ui.fnt_superscript.is_checked(), subscript: ui.fnt_subscript.is_checked(),
        small_caps: ui.fnt_small_caps.is_checked(), all_caps: ui.fnt_all_caps.is_checked(), hidden: ui.fnt_hidden.is_checked(),
        scale: ui.fnt_scale.get_value().trim().trim_end_matches('%').trim().parse::<f32>().ok().filter(|n| *n > 0.0).unwrap_or(100.0) / 100.0,
        spacing_pt: match crate::i18n::choice_source(&ui.fnt_spacing.get_value(), &["Normal", "Expanded", "Condensed"]).as_str() { "Expanded" => spacing, "Condensed" => -spacing, _ => 0.0 },
        position_pt: match crate::i18n::choice_source(&ui.fnt_position.get_value(), &["Normal", "Raised", "Lowered"]).as_str() { "Raised" => position, "Lowered" => -position, _ => 0.0 },
    }
}

/// Word's Font preview: the face name in the chosen formatting, centred, with
/// the baseline drawn out to both edges. Effects the model does not store yet
/// are drawn too, so every control has a visible result.
fn paint_font_preview(dc: &AutoBufferedPaintDC, panel: &Panel, look: &FontLook) {
    paint_rounded_surface(dc, panel, Colour::rgb(255, 255, 255), rgb(LINE_STRONG), 5);
    let size = panel.get_client_size();
    let px = px_per_pt(panel);
    let display = look.size.clamp(8.0, 26.0);
    let k = display / look.size.max(1.0);
    let script = look.superscript || look.subscript;
    let main = if script { display * 0.65 } else { display };
    let text = if look.face.is_empty() { "Sample".to_owned() } else { look.face.clone() };
    let text = if look.all_caps { text.to_uppercase() } else { text };
    let Some(big) = face_font(main, look.bold, look.italic, false, if look.face.is_empty() { "Linux Libertine G" } else { &look.face }) else { return };
    let small = face_font(main * 0.78, look.bold, look.italic, false, if look.face.is_empty() { "Linux Libertine G" } else { &look.face });
    // Lay out glyph by glyph: small caps, scale and character spacing.
    let extra = look.spacing_pt * k * px;
    let mut glyphs = Vec::new(); let mut width = 0.0_f32;
    for ch in text.chars() {
        let lowered = look.small_caps && ch.is_lowercase();
        let glyph: String = if lowered { ch.to_uppercase().collect() } else { ch.to_string() };
        dc.set_font(if lowered { small.as_ref().unwrap_or(&big) } else { &big });
        let w = dc.get_text_extent(&crate::i18n::text(&(&glyph))).0 as f32 * look.scale;
        glyphs.push((glyph, lowered, width, w)); width += w + extra;
    }
    width -= extra.max(0.0);
    dc.set_font(&big);
    let text_h = dc.get_text_extent(&crate::i18n::text(&("Hg"))).1 as f32;
    let baseline = size.height as f32 * 0.5 + text_h * 0.3;
    let shift = if look.superscript { -display * 0.33 * px } else if look.subscript { display * 0.15 * px } else { 0.0 } - look.position_pt * k * px;
    let left = (size.width as f32 - width) / 2.0;
    let top = baseline - text_h * 0.8 + shift;
    // Baseline guides to both edges (Word draws these beside the sample).
    let stroke = dip(panel, 1).max(1);
    dc.set_pen(rgb(LINE_STRONG), stroke, PenStyle::Solid);
    let pad = dip(panel, 10);
    dc.draw_line(pad, baseline as i32, (left - dip(panel, 6) as f32) as i32, baseline as i32);
    dc.draw_line((left + width + dip(panel, 6) as f32) as i32, baseline as i32, size.width - pad, baseline as i32);
    if let Some(highlight) = look.highlight {
        dc.set_brush(highlight, BrushStyle::Solid); dc.set_pen(highlight, 1, PenStyle::Solid);
        dc.draw_rectangle(left as i32 - 2, top as i32, width as i32 + 4, text_h as i32);
    }
    dc.set_text_foreground(if look.hidden { rgb(MUTED) } else { look.colour });
    for (glyph, lowered, x, _) in &glyphs {
        dc.set_font(if *lowered { small.as_ref().unwrap_or(&big) } else { &big });
        let y = if *lowered { top + text_h * 0.22 * 0.78 + text_h * 0.06 } else { top };
        dc.draw_text(&crate::i18n::text(&(glyph)), (left + x) as i32, y as i32);
    }
    let line_y = (baseline + shift + (2.0 * px).max(2.0)) as i32;
    let colour = look.underline_colour.unwrap_or(look.colour);
    let (x0, x1) = (left as i32, (left + width) as i32);
    let thin = stroke.max((main * px / 18.0).round() as i32);
    let draw = |pen: PenStyle, w: i32, y: i32| { dc.set_pen(colour, w, pen); dc.draw_line(x0, y, x1, y); };
    match look.underline.as_str() {
        "Single" => draw(PenStyle::Solid, thin, line_y),
        "Words only" => {
            dc.set_pen(colour, thin, PenStyle::Solid);
            let mut start: Option<f32> = None;
            for (glyph, _, x, w) in glyphs.iter().chain(std::iter::once(&(" ".to_owned(), false, width + 1.0, 0.0))) {
                if glyph.trim().is_empty() { if let Some(s) = start.take() { dc.draw_line((left + s) as i32, line_y, (left + x) as i32, line_y); } }
                else if start.is_none() { start = Some(*x); }
                let _ = w;
            }
        }
        "Double" => { draw(PenStyle::Solid, thin, line_y); draw(PenStyle::Solid, thin, line_y + 3 * thin); }
        "Thick" => draw(PenStyle::Solid, thin * 2, line_y),
        "Dotted" => draw(PenStyle::Dot, thin, line_y),
        "Dashed" => draw(PenStyle::ShortDash, thin, line_y),
        "Dot-dash" | "Dot-dot-dash" => draw(PenStyle::DotDash, thin, line_y),
        "Wave" => {
            dc.set_pen(colour, thin, PenStyle::Solid);
            let step = dip(panel, 3).max(2); let mut x = x0; let mut up = true;
            while x < x1 { let nx = (x + step).min(x1); dc.draw_line(x, line_y + if up { 0 } else { step }, nx, line_y + if up { step } else { 0 }); x = nx; up = !up; }
        }
        _ => {}
    }
    if look.hidden { draw(PenStyle::Dot, stroke, line_y + thin * 2); }
    let strike_y = (baseline + shift - text_h * 0.28) as i32;
    dc.set_pen(look.colour, thin, PenStyle::Solid);
    if look.strike { dc.draw_line(x0, strike_y, x1, strike_y); }
    if look.double_strike { dc.draw_line(x0, strike_y - thin * 2, x1, strike_y - thin * 2); dc.draw_line(x0, strike_y + thin, x1, strike_y + thin); }
}

fn font_dialog(parent: Dialog, style: &mut Style, sheet: &StyleSheet, pending: &Pending) -> bool {
    let ui = Rc::new(crate::i18n::localized(FontDialogUi::new(Some(&parent), false)));
    ui.fnt_family_list.clear();
    for family in installed_font_families() { ui.fnt_family_list.append(family); }
    let chrome = Chrome::new(ui.fnt_dialog, ui.fnt_card, &[(ui.fnt_tab_0, "Font"), (ui.fnt_tab_1, "Advanced")],
        &[ui.fnt_page_0, ui.fnt_page_1], (ui.fnt_ok, "OK"), Some(ui.fnt_cancel), &[(ui.fnt_text_effects, "Text Effects…")], &[]);
    let inherited = sheet.resolve_character(style.based_on.as_ref());
    let own = style.character.clone();
    let shown = inherited.clone().over(&own);
    let typo = shown.typography.clone().unwrap_or_default();
    // Show the style's values (inherited where it sets none).
    let family = shown.font_family.clone().unwrap_or_else(|| "Linux Libertine G".into());
    select_in_list(ui.fnt_family, ui.fnt_family_list, &family);
    let style_name = match (shown.bold == Some(true), shown.italic == Some(true)) {
        (true, true) => "Bold Italic", (true, false) => "Bold", (false, true) => "Italic", _ => "Regular" };
    select_in_list(ui.fnt_style, ui.fnt_style_list, &crate::i18n::text(style_name));
    let size = shown.font_size_pt.unwrap_or(11.0);
    select_in_list(ui.fnt_size, ui.fnt_size_list, &size.to_string());
    fill_colour_list(ui.fnt_color, &ui.fnt_dialog, shown.color.as_deref(), AUTOMATIC);
    fill_colour_list(ui.fnt_underline_color, &ui.fnt_dialog, None, AUTOMATIC);
    let highlight = shown.highlight.clone().filter(|h| h != "none");
    fill_colour_list(ui.fnt_highlight, &ui.fnt_dialog, highlight.as_deref(), NO_COLOUR);
    let underline_on = shown.underline == Some(true);
    combo_index(ui.fnt_underline, if underline_on { "Single" } else { "(none)" });
    ui.fnt_small_caps.set_value(typo.small_caps);
    combo_index(ui.fnt_kerning, if !typo.kerning { "Off" } else { match typo.kerning_mode { KerningMode::Optical => "Optical", KerningMode::Off => "Off", KerningMode::Metrics => "Metrics" } });
    combo_index(ui.fnt_ligatures, match (typo.ligatures, typo.contextual_ligatures) {
        (LigatureMode::None, _) => "None", (LigatureMode::Standard, false) => "Standard Only",
        (LigatureMode::Standard, true) => "Standard and Contextual", (LigatureMode::Discretionary, false) => "Historical and Discretionary",
        (LigatureMode::Discretionary, true) => "All" });
    combo_index(ui.fnt_number_spacing, if typo.tabular_figures { "Tabular" } else { "Default" });
    combo_index(ui.fnt_number_forms, if typo.oldstyle_figures { "Old-style" } else { "Default" });
    ui.fnt_contextual_alternates.set_value(typo.contextual_alternates);
    combo_index(ui.fnt_scale, "100%"); combo_index(ui.fnt_spacing, "Normal"); combo_index(ui.fnt_position, "Normal");
    combo_index(ui.fnt_stylistic_set, &typo.stylistic_set.map(|set|set.to_string()).unwrap_or_else(||"Default".into()));
    let ui_only = [
        ("fnt_underline_color", Ctl::Colour(ui.fnt_underline_color, ui.fnt_dialog, false)), ("fnt_strike", Ctl::Check(ui.fnt_strike)),
        ("fnt_double_strike", Ctl::Check(ui.fnt_double_strike)), ("fnt_superscript", Ctl::Check(ui.fnt_superscript)),
        ("fnt_subscript", Ctl::Check(ui.fnt_subscript)), ("fnt_all_caps", Ctl::Check(ui.fnt_all_caps)), ("fnt_hidden", Ctl::Check(ui.fnt_hidden)),
        ("fnt_scale", Ctl::Combo(ui.fnt_scale)), ("fnt_spacing", Ctl::Combo(ui.fnt_spacing)), ("fnt_spacing_by", Ctl::Spin(ui.fnt_spacing_by)),
        ("fnt_position", Ctl::Combo(ui.fnt_position)), ("fnt_position_by", Ctl::Spin(ui.fnt_position_by)),
        ("fnt_kerning_from", Ctl::Spin(ui.fnt_kerning_from)), ("fnt_stylistic_set", Ctl::Combo(ui.fnt_stylistic_set)),
    ];
    let ui_before=restore(pending, &ui_only);
    // Partly stored controls: restore the finer choice only when it agrees with the model.
    {
        let pending = pending.borrow();
        if let Some(v) = pending.get("fnt_underline") { if underline_on && v != "(none)" { ui.fnt_underline.set_value(&crate::i18n::text(&(v))); } }
        if pending.get("fnt_number_spacing").is_some_and(|v| v == "Proportional") && !typo.tabular_figures { ui.fnt_number_spacing.set_value(&crate::i18n::text(&("Proportional"))); }
        if pending.get("fnt_number_forms").is_some_and(|v| v == "Lining") && !typo.oldstyle_figures { ui.fnt_number_forms.set_value(&crate::i18n::text(&("Lining"))); }
    }
    let fine_controls=[("fnt_underline", Ctl::Combo(ui.fnt_underline)), ("fnt_number_spacing", Ctl::Combo(ui.fnt_number_spacing)),
        ("fnt_number_forms", Ctl::Combo(ui.fnt_number_forms))];
    let fine_before=control_snapshot(&fine_controls);
    // One refresh for every control (wx runs only the last handler bound to an
    // event, so each control gets exactly one handler that calls this).
    let refresh: Rc<dyn Fn()> = { let ui = Rc::clone(&ui); Rc::new(move || {
        ui.fnt_preview_note.set_label(&crate::i18n::text(&(&format!("{} · OpenType font; the preview uses the installed face.", ui.fnt_family.get_value().trim()))));
        ui.fnt_preview.refresh(false, None); ui.fnt_preview_advanced.refresh(false, None);
    }) };
    for preview in [ui.fnt_preview, ui.fnt_preview_advanced] {
        let ui = Rc::clone(&ui);
        painted(preview, move |dc, panel| paint_font_preview(dc, panel, &read_font_look(&ui)));
    }
    link_list(ui.fnt_family, ui.fnt_family_list, Rc::clone(&refresh));
    link_list(ui.fnt_style, ui.fnt_style_list, Rc::clone(&refresh));
    link_list(ui.fnt_size, ui.fnt_size_list, Rc::clone(&refresh));
    for combo in [ui.fnt_color, ui.fnt_underline_color, ui.fnt_highlight] { let refresh = Rc::clone(&refresh); bind_colour_list(combo, ui.fnt_dialog, move || refresh()); }
    for combo in [ui.fnt_underline, ui.fnt_scale, ui.fnt_spacing, ui.fnt_position, ui.fnt_kerning, ui.fnt_ligatures,
                  ui.fnt_number_spacing, ui.fnt_number_forms, ui.fnt_stylistic_set] {
        let refresh = Rc::clone(&refresh); combo.on_selection_changed(move |_| refresh());
    }
    { let refresh = Rc::clone(&refresh); ui.fnt_scale.on_text_updated(move |_| refresh()); }
    for spin in [ui.fnt_spacing_by, ui.fnt_position_by, ui.fnt_kerning_from] { let refresh = Rc::clone(&refresh); spin.on_value_changed(move |_| refresh()); }
    // Word's mutually exclusive effects.
    for (a, b) in [(ui.fnt_strike, ui.fnt_double_strike), (ui.fnt_superscript, ui.fnt_subscript), (ui.fnt_small_caps, ui.fnt_all_caps)] {
        for (this, other) in [(a, b), (b, a)] {
            let refresh = Rc::clone(&refresh);
            this.on_toggled(move |_| { if this.is_checked() { other.set_value(false); } refresh(); });
        }
    }
    for check in [ui.fnt_hidden, ui.fnt_contextual_alternates] { let refresh = Rc::clone(&refresh); check.on_toggled(move |_| refresh()); }
    { let dialog = ui.fnt_dialog; let pending = Rc::clone(pending); ui.fnt_text_effects.on_click(move |_| { text_effects_dialog(dialog, &pending); }); }
    let accepted = chrome.show();
    if accepted {
        let c = &mut style.character;
        let family = ui.fnt_family.get_value().trim().to_owned();
        if !family.is_empty() { let family=changed(&own.font_family, &shown.font_family.clone().unwrap_or_default(), family);if family!=c.font_family{c.set_font_family(family);} }
        let look = read_font_look(&ui);
        c.bold = changed(&own.bold, &(shown.bold == Some(true)), look.bold);
        c.italic = changed(&own.italic, &(shown.italic == Some(true)), look.italic);
        if let Ok(points) = ui.fnt_size.get_value().trim().parse::<f32>() {
            if (1.0..=400.0).contains(&points) { c.font_size_pt = changed(&own.font_size_pt, &size, points); }
        }
        c.underline = changed(&own.underline, &underline_on, crate::i18n::choice_source(&ui.fnt_underline.get_value(), &["(none)", "Words only", "Single", "Double", "Thick", "Dotted", "Dashed", "Dot-dash", "Dot-dot-dash", "Wave"]) != "(none)");
        let colour = colour_value(&crate::i18n::source(&ui.fnt_color.get_value()));
        if colour != shown.color {
            // Automatic is black on the page. An explicit black override is
            // needed when the base style supplies another colour.
            c.color = colour.or_else(|| inherited.color.as_ref().map(|_| "#000000".to_owned()));
        }
        let new_highlight = colour_value(&ui.fnt_highlight.get_value());
        if new_highlight != highlight {
            // "No Color" over an inherited highlight must clear it explicitly.
            c.highlight = new_highlight.or_else(|| inherited.highlight.as_ref().filter(|h| *h != "none").map(|_| "none".to_owned()));
        }
        let mut new_typo = typo.clone();
        new_typo.small_caps = ui.fnt_small_caps.is_checked();
        match crate::i18n::choice_source(&ui.fnt_kerning.get_value(), &["Optical", "Metrics", "Off"]).as_str() {
            "Optical" => { new_typo.kerning = true; new_typo.kerning_mode = KerningMode::Optical; }
            "Off" => { new_typo.kerning = false; new_typo.kerning_mode = KerningMode::Off; }
            _ => { new_typo.kerning = true; new_typo.kerning_mode = KerningMode::Metrics; }
        }
        let (ligatures, contextual) = match crate::i18n::choice_source(&ui.fnt_ligatures.get_value(), &["None", "Standard Only", "Standard and Contextual", "Historical and Discretionary", "All"]).as_str() {
            "None" => (LigatureMode::None, false), "Standard Only" => (LigatureMode::Standard, false),
            "Historical and Discretionary" => (LigatureMode::Discretionary, false), "All" => (LigatureMode::Discretionary, true),
            _ => (LigatureMode::Standard, true) };
        new_typo.ligatures = ligatures; new_typo.contextual_ligatures = contextual;
        new_typo.tabular_figures = crate::i18n::choice_source(&ui.fnt_number_spacing.get_value(), &["Default", "Proportional", "Tabular"]) == "Tabular";
        new_typo.oldstyle_figures = crate::i18n::choice_source(&ui.fnt_number_forms.get_value(), &["Default", "Lining", "Old-style"]) == "Old-style";
        new_typo.contextual_alternates = ui.fnt_contextual_alternates.is_checked();
        new_typo.stylistic_set=crate::i18n::choice_source(&ui.fnt_stylistic_set.get_value(), &["Default", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16", "17", "18", "19", "20"]).parse::<u8>().ok().filter(|set|(1..=20).contains(set));
        c.typography = changed(&own.typography, &typo, new_typo);
        remember_changed(pending, &ui_only, &ui_before);
        remember_changed(pending, &fine_controls, &fine_before);
    }
    ui.fnt_dialog.destroy();
    accepted
}

// Paragraph ---------------------------------------------------------------------------------

const ALIGNMENTS: [(&str, ParagraphAlignment); 4] = [("Left", ParagraphAlignment::Left), ("Centered", ParagraphAlignment::Center),
    ("Right", ParagraphAlignment::Right), ("Justified", ParagraphAlignment::Justify)];
const DIRECTIONS: [(&str, ParagraphDirection); 3] = [("Automatic", ParagraphDirection::Auto),
    ("Left-to-right", ParagraphDirection::LeftToRight), ("Right-to-left", ParagraphDirection::RightToLeft)];

/// The paragraph values the dialog edits, read from its controls.
struct ParagraphValues { alignment: ParagraphAlignment, direction: ParagraphDirection, left: f32, right: f32, first: f32,
    before: f32, after: f32, line: Option<f32>, widow: bool, keep_next: bool, keep_lines: bool, page_break: bool,
    no_hyphenate: bool, hanging: bool, expansion: u16 }

fn read_paragraph(ui: &ParagraphDialogUi) -> ParagraphValues {
    let special = crate::i18n::choice_source(&ui.par_special.get_value(), &["(none)", "First line", "Hanging"]); let by = ui.par_special_by.get_value() as f32;
    ParagraphValues {
        alignment: ALIGNMENTS.iter().find(|(n, _)| *n == crate::i18n::choice_source(&ui.par_alignment.get_value(), &["Left", "Centered", "Right", "Justified"])).map(|(_, a)| *a).unwrap_or(ParagraphAlignment::Left),
        direction: DIRECTIONS.iter().find(|(n, _)| *n == crate::i18n::choice_source(&ui.par_direction.get_value(), &["Automatic", "Left-to-right", "Right-to-left"])).map(|(_, d)| *d).unwrap_or(ParagraphDirection::Auto),
        left: ui.par_left.get_value() as f32, right: ui.par_right.get_value() as f32,
        first: match special.as_str() { "First line" => by, "Hanging" => -by, _ => 0.0 },
        before: ui.par_before.get_value() as f32, after: ui.par_after.get_value() as f32,
        // Exact and minimum point rules are retained separately from the multiplier.
        line: match crate::i18n::choice_source(&ui.par_line_rule.get_value(), &["Single", "1.5 lines", "Double", "At least", "Exactly", "Multiple"]).as_str() {
            "Single" => Some(1.0), "1.5 lines" => Some(1.5), "Double" => Some(2.0),
            "Multiple" => Some(ui.par_line_at.get_value() as f32), _ => None },
        widow: ui.par_widow.is_checked(), keep_next: ui.par_keep_next.is_checked(), keep_lines: ui.par_keep_lines.is_checked(),
        page_break: ui.par_page_break.is_checked(), no_hyphenate: ui.par_no_hyphenate.is_checked(),
        hanging: ui.par_hanging_punct.is_checked(), expansion: ui.par_expansion.get_value().round().clamp(0.0, 50.0) as u16,
    }
}

/// Preview model for a paragraph dialog: context paragraphs in the base style,
/// the sample with the dialog's values (at the context size, as in Modify Style).
fn paragraph_preview(sheet: &StyleSheet, style: &Style, values: &ParagraphValues, shown_line: f32) -> PreviewModel {
    let context_id = style.based_on.clone().unwrap_or_else(|| StyleId::from(model::STYLE_NORMAL));
    let context = preview_from(&sheet.resolve_character(Some(&context_id)), &sheet.resolve_paragraph(Some(&context_id)));
    let character = sheet.resolve_character(style.based_on.as_ref()).over(&style.character);
    let mut paragraph = ParagraphProperties::default();
    paragraph.alignment = Some(values.alignment);
    paragraph.left_indent_mm = Some(values.left); paragraph.right_indent_mm = Some(values.right);
    paragraph.first_line_indent_mm = Some(values.first);
    paragraph.space_before_pt = Some(values.before); paragraph.space_after_pt = Some(values.after);
    paragraph.line_spacing = Some(values.line.unwrap_or(shown_line));
    let mut sample: PreviewParagraph = preview_from(&character, &paragraph);
    sample.size = context.size;
    PreviewModel { previous: context.clone(), sample, following: context }
}

fn paragraph_dialog(parent: Dialog, style: &mut Style, sheet: &StyleSheet, pending: &Pending) -> bool {
    let ui = Rc::new(crate::i18n::localized(ParagraphDialogUi::new(Some(&parent), false)));
    let chrome = Chrome::new(ui.par_dialog, ui.par_card, &[(ui.par_tab_0, "Indents and Spacing"), (ui.par_tab_1, "Line and Page Breaks")],
        &[ui.par_page_0, ui.par_page_1], (ui.par_ok, "OK"), Some(ui.par_cancel), &[(ui.par_tabs, "Tabs…")], &[]);
    let inherited = sheet.resolve_paragraph(style.based_on.as_ref());
    let own = style.paragraph.clone();
    let mut shown = inherited.clone(); shown.overlay(&own);
    let size = sheet.resolve_character(style.based_on.as_ref()).over(&style.character).font_size_pt.unwrap_or(11.0);
    let s_alignment = shown.alignment.unwrap_or(ParagraphAlignment::Left);
    let s_direction = shown.direction.unwrap_or(ParagraphDirection::Auto);
    let (s_left, s_right, s_first) = (shown.left_indent_mm.unwrap_or(0.0), shown.right_indent_mm.unwrap_or(0.0), shown.first_line_indent_mm.unwrap_or(0.0));
    let (s_before, s_after) = (shown.space_before_pt.unwrap_or(0.0), shown.space_after_pt.unwrap_or(size * 0.3));
    let s_line = shown.line_spacing.unwrap_or(1.2);
    let s_widow = shown.widow_control.unwrap_or(false); let s_keep_next = shown.keep_with_next.unwrap_or(false);
    let s_keep_lines = shown.keep_lines_together.unwrap_or(false); let s_page_break = shown.page_break_before.unwrap_or(false);
    let s_no_hyphenate = shown.hyphenation == Some(false); let s_hanging = shown.hanging_punctuation.unwrap_or(false);
    let s_expansion = shown.glyph_expansion_per_mille.unwrap_or(15);
    combo_index(ui.par_alignment, ALIGNMENTS.iter().find(|(_, a)| *a == s_alignment).map(|(n, _)| *n).unwrap_or("Left"));
    combo_index(ui.par_direction, DIRECTIONS.iter().find(|(_, d)| *d == s_direction).map(|(n, _)| *n).unwrap_or("Automatic"));
    ui.par_left.set_value(s_left as f64); ui.par_right.set_value(s_right as f64);
    combo_index(ui.par_special, if s_first > 0.0 { "First line" } else if s_first < 0.0 { "Hanging" } else { "(none)" });
    ui.par_special_by.set_value(s_first.abs() as f64);
    ui.par_before.set_value(s_before as f64); ui.par_after.set_value(s_after as f64);
    let rule = if (s_line - 1.0).abs() < 0.001 { "Single" } else if (s_line - 1.5).abs() < 0.001 { "1.5 lines" } else if (s_line - 2.0).abs() < 0.001 { "Double" } else { "Multiple" };
    combo_index(ui.par_line_rule, rule); ui.par_line_at.set_value(s_line as f64);
    ui.par_widow.set_value(s_widow); ui.par_keep_next.set_value(s_keep_next); ui.par_keep_lines.set_value(s_keep_lines);
    ui.par_page_break.set_value(s_page_break); ui.par_no_hyphenate.set_value(s_no_hyphenate); ui.par_hanging_punct.set_value(s_hanging);
    ui.par_expansion.set_value(s_expansion as f64);
    let ui_only = [("par_outline_level", Ctl::Combo(ui.par_outline_level)), ("par_collapsed", Ctl::Check(ui.par_collapsed)),
        ("par_mirror", Ctl::Check(ui.par_mirror)), ("par_contextual", Ctl::Check(ui.par_contextual)),
        ("par_suppress_numbers", Ctl::Check(ui.par_suppress_numbers)), ("par_tight_wrap", Ctl::Combo(ui.par_tight_wrap))];
    let ui_before=restore(pending, &ui_only);
    // Restore an exact or minimum point rule with its point value.
    if let (Some(rule), Some(at)) = (pending.borrow().get("par_line_rule").cloned(), pending.borrow().get("par_line_at_pt").cloned()) {
        if rule == "At least" || rule == "Exactly" { ui.par_line_rule.set_value(&crate::i18n::text(&(&rule))); if let Ok(n) = at.parse() { ui.par_line_at.set_value(n); } }
    }
    let line_rule_before=crate::i18n::choice_source(&ui.par_line_rule.get_value(), &["Single", "1.5 lines", "Double", "At least", "Exactly", "Multiple"]);
    let line_at_before=ui.par_line_at.get_value();
    // Tabs… edits the paragraph's tab stops through the Tabs dialog.
    let tabs_style = Rc::new(RefCell::new(style.clone()));
    let style_for_preview = style.clone(); let sheet_for_preview = sheet.clone();
    let preview_model = { let ui = Rc::clone(&ui); move || paragraph_preview(&sheet_for_preview, &style_for_preview, &read_paragraph(&ui), s_line) };
    let preview_model = Rc::new(preview_model);
    for panel in [ui.par_preview, ui.par_preview_breaks] {
        let model = Rc::clone(&preview_model);
        painted(panel, move |dc, panel| { paint_rounded_surface(dc, panel, Colour::rgb(255, 255, 255), rgb(LINE_STRONG), 5); paint_style_preview(dc, panel, &model()); });
    }
    let refresh: Rc<dyn Fn()> = { let ui = Rc::clone(&ui); Rc::new(move || {
        // The "At" box follows the line-spacing rule, as in Word.
        let rule = crate::i18n::choice_source(&ui.par_line_rule.get_value(), &["Single", "1.5 lines", "Double", "At least", "Exactly", "Multiple"]);
        ui.par_line_at.enable(matches!(rule.as_str(), "At least" | "Exactly" | "Multiple"));
        ui.par_line_unit.set_label(&crate::i18n::text(&(if rule == "Multiple" { "lines" } else { "pt" })));
        ui.par_special_by.enable(crate::i18n::choice_source(&ui.par_special.get_value(), &["(none)", "First line", "Hanging"]) != "(none)");
        ui.par_preview.refresh(false, None); ui.par_preview_breaks.refresh(false, None);
    }) };
    refresh();
    for combo in [ui.par_alignment, ui.par_direction, ui.par_special, ui.par_outline_level, ui.par_tight_wrap] {
        let refresh = Rc::clone(&refresh); combo.on_selection_changed(move |_| refresh());
    }
    { let ui2 = Rc::clone(&ui); let refresh = Rc::clone(&refresh); ui.par_line_rule.on_selection_changed(move |_| {
        // Picking a rule sets Word's usual "At" value for it.
        match crate::i18n::choice_source(&ui2.par_line_rule.get_value(), &["Single", "1.5 lines", "Double", "At least", "Exactly", "Multiple"]).as_str() { "Single" => ui2.par_line_at.set_value(1.0), "1.5 lines" => ui2.par_line_at.set_value(1.5),
            "Double" => ui2.par_line_at.set_value(2.0), "At least" | "Exactly" => ui2.par_line_at.set_value(12.0), _ => {} }
        refresh(); }); }
    for spin in [ui.par_left, ui.par_right, ui.par_special_by, ui.par_before, ui.par_after, ui.par_line_at, ui.par_expansion] {
        let refresh = Rc::clone(&refresh); spin.on_value_changed(move |_| refresh());
    }
    for check in [ui.par_widow, ui.par_keep_next, ui.par_keep_lines, ui.par_page_break, ui.par_no_hyphenate, ui.par_hanging_punct,
                  ui.par_collapsed, ui.par_mirror, ui.par_contextual, ui.par_suppress_numbers] {
        let refresh = Rc::clone(&refresh); check.on_toggled(move |_| refresh());
    }
    { let tabs_style = Rc::clone(&tabs_style); let dialog = ui.par_dialog; let sheet = sheet.clone(); let pending = Rc::clone(pending);
      ui.par_tabs.on_click(move |_| { let mut copy = tabs_style.borrow().clone(); if tabs_dialog(dialog, &mut copy, &sheet, &pending) { tabs_style.replace(copy); } }); }
    let accepted = chrome.show();
    if accepted {
        let v = read_paragraph(&ui);
        let p = &mut style.paragraph;
        p.alignment = changed(&own.alignment, &s_alignment, v.alignment);
        p.direction = changed(&own.direction, &s_direction, v.direction);
        p.left_indent_mm = changed(&own.left_indent_mm, &s_left, v.left);
        p.right_indent_mm = changed(&own.right_indent_mm, &s_right, v.right);
        p.first_line_indent_mm = changed(&own.first_line_indent_mm, &s_first, v.first);
        p.space_before_pt = changed(&own.space_before_pt, &s_before, v.before);
        p.space_after_pt = changed(&own.space_after_pt, &s_after, v.after);
        if let Some(line) = v.line { p.line_spacing = changed(&own.line_spacing, &s_line, line.clamp(0.5, 10.0)); }
        p.widow_control = changed(&own.widow_control, &s_widow, v.widow);
        p.keep_with_next = changed(&own.keep_with_next, &s_keep_next, v.keep_next);
        p.keep_lines_together = changed(&own.keep_lines_together, &s_keep_lines, v.keep_lines);
        p.page_break_before = changed(&own.page_break_before, &s_page_break, v.page_break);
        if v.no_hyphenate != s_no_hyphenate { p.hyphenation = Some(!v.no_hyphenate); }
        p.hanging_punctuation = changed(&own.hanging_punctuation, &s_hanging, v.hanging);
        p.glyph_expansion_per_mille = changed(&own.glyph_expansion_per_mille, &s_expansion, v.expansion);
        p.tabs = tabs_style.borrow().paragraph.tabs.clone();
        remember_changed(pending, &ui_only, &ui_before);
        let mut pending = pending.borrow_mut();
        if crate::i18n::choice_source(&ui.par_line_rule.get_value(), &["Single", "1.5 lines", "Double", "At least", "Exactly", "Multiple"])!=line_rule_before {pending.set("par_line_rule", crate::i18n::choice_source(&ui.par_line_rule.get_value(), &["Single", "1.5 lines", "Double", "At least", "Exactly", "Multiple"]));}
        if (ui.par_line_at.get_value()-line_at_before).abs()>0.001 {pending.set("par_line_at_pt", ui.par_line_at.get_value().to_string());}
    }
    ui.par_dialog.destroy();
    accepted
}

// Tabs --------------------------------------------------------------------------------------

use model::TAB_POSITION_EPSILON_MM;

fn tab_label(stop: &TabStop) -> String {
    let position = format!("{:.2}", stop.position_mm); let position = position.trim_end_matches('0').trim_end_matches('.');
    format!("{position} mm")
}

fn tabs_dialog(parent: Dialog, style: &mut Style, sheet: &StyleSheet, _pending: &Pending) -> bool {
    let resolved = sheet.resolve_paragraph_with_direct(style.based_on.as_ref(), &style.paragraph);
    let default = resolved.default_tab_mm.unwrap_or(model::DEFAULT_TAB_INTERVAL_MM);
    if let Some(patch) = edit_tab_stops(&parent, Some(resolved.tabs.unwrap_or_default()), default) {
        style.paragraph.overlay(&patch);
        return true;
    }
    false
}

/// The same tab-stop controls edit paragraph selections and named styles.
pub(super) fn edit_tab_stops(parent: &impl WxWidget, shown: Option<Vec<TabStop>>, shown_default: f32) -> Option<model::ParagraphProperties> {
    let ui = Rc::new(crate::i18n::localized(TabsDialogUi::new(Some(parent), false)));
    let chrome = Chrome::new(ui.tab_dialog, ui.tab_card, &[], &[], (ui.tab_ok, "OK"), Some(ui.tab_cancel), &[],
        &[(ui.tab_set, "Set"), (ui.tab_clear, "Clear"), (ui.tab_clear_all, "Clear All")]);
    let mixed = shown.is_none();
    let shown = shown.unwrap_or_default();
    let stops = Rc::new(RefCell::new(shown.clone()));
    let dirty = Rc::new(Cell::new(false));
    if mixed { ui.tab_status.set_label(&crate::i18n::text(&("Selected paragraphs use different tab stops."))); }
    let cleared = Rc::new(RefCell::new(Vec::<String>::new()));
    let alignments = [(ui.tab_align_left, TabAlignment::Left), (ui.tab_align_center, TabAlignment::Center), (ui.tab_align_right, TabAlignment::Right),
        (ui.tab_align_decimal, TabAlignment::Decimal), (ui.tab_align_bar, TabAlignment::Bar)];
    let leaders = [(ui.tab_leader_none, TabLeader::None), (ui.tab_leader_dots, TabLeader::Dots), (ui.tab_leader_dashes, TabLeader::Dashes),
        (ui.tab_leader_line, TabLeader::Line), (ui.tab_leader_heavy, TabLeader::Heavy), (ui.tab_leader_middle, TabLeader::MiddleDots)];
    ui.tab_default.set_value(shown_default as f64);
    let fill = { let ui = Rc::clone(&ui); let stops = Rc::clone(&stops); move |select: Option<usize>| {
        ui.tab_list.clear();
        for stop in stops.borrow().iter() { ui.tab_list.append(&tab_label(stop)); }
        if let Some(i) = select { if i < ui.tab_list.get_count() as usize { ui.tab_list.set_selection(i as u32, true); } }
    } };
    let fill = Rc::new(fill);
    fill(if shown.is_empty() { None } else { Some(0) });
    let show_stop = { let ui = Rc::clone(&ui); let stops = Rc::clone(&stops); move |index: usize| {
        let stops = stops.borrow(); let Some(stop) = stops.get(index) else { return };
        ui.tab_position.change_value(tab_label(stop).trim_end_matches(" mm"));
        for (radio, alignment) in alignments { if alignment == stop.alignment { radio.set_value(true); } }
        let leader = stop.leader;
        for (radio, value) in leaders { if value == leader { radio.set_value(true); } }
    } };
    let show_stop = Rc::new(show_stop);
    if !shown.is_empty() { show_stop(0); }
    { let show_stop = Rc::clone(&show_stop); let list = ui.tab_list; ui.tab_list.on_selection_changed(move |_| if let Some(i) = list.get_selection() { show_stop(i as usize); }); }
    let set = {
      let ui2 = Rc::clone(&ui); let stops = Rc::clone(&stops); let fill = Rc::clone(&fill); let dirty = Rc::clone(&dirty);
      Rc::new(move || {
        let text = ui2.tab_position.get_value();
        let text = text.trim().trim_end_matches("mm").trim().replace(',', ".");
        let Ok(position) = text.parse::<f32>() else { ui2.tab_status.set_label(&crate::i18n::text(&("Enter a tab position in millimetres."))); return; };
        if !position.is_finite() || !(0.0..=model::MAX_TAB_POSITION_MM).contains(&position) {
            ui2.tab_status.set_label(&crate::i18n::text(&("Tab positions must be between 0 and 1000 mm."))); return;
        }
        let alignment = alignments.iter().find(|(r, _)| r.get_value()).map(|(_, a)| *a).unwrap_or(TabAlignment::Left);
        let leader = leaders.iter().find(|(r, _)| r.get_value()).map(|(_, l)| *l).unwrap_or(TabLeader::None);
        let mut list = stops.borrow_mut();
        let existing = list.iter().position(|stop| (stop.position_mm - position).abs() < TAB_POSITION_EPSILON_MM);
        if existing.is_none() && list.len() >= model::MAX_TAB_STOPS { ui2.tab_status.set_label(&crate::i18n::text(&("A paragraph can have at most 64 tab stops."))); return; }
        if let Some(index) = existing { list.remove(index); }
        list.push(TabStop { position_mm: position, alignment, leader });
        list.sort_by(|a, b| a.position_mm.total_cmp(&b.position_mm));
        let index = list.iter().position(|stop| (stop.position_mm - position).abs() < TAB_POSITION_EPSILON_MM);
        dirty.set(true); ui2.tab_status.set_label(&crate::i18n::text(&(" ")));
        drop(list); fill(index);
      })
    };
    { let set = Rc::clone(&set); ui.tab_set.on_click(move |_| set()); }
    { let set = Rc::clone(&set); ui.tab_position.on_text_enter(move |_| set()); }
    { let ui2 = Rc::clone(&ui); let stops = Rc::clone(&stops); let cleared = Rc::clone(&cleared); let fill = Rc::clone(&fill); let dirty = Rc::clone(&dirty);
      ui.tab_clear.on_click(move |_| {
        let Some(i) = ui2.tab_list.get_selection() else { return };
        let removed = stops.borrow_mut().remove(i as usize); dirty.set(true);
        cleared.borrow_mut().push(tab_label(&removed)); ui2.tab_cleared.set_label(&crate::i18n::text(&(&cleared.borrow().join(", "))));
        let count = stops.borrow().len(); fill(if count == 0 { None } else { Some((i as usize).min(count - 1)) });
      }); }
    { let ui2 = Rc::clone(&ui); let stops = Rc::clone(&stops); let cleared = Rc::clone(&cleared); let fill = Rc::clone(&fill); let dirty = Rc::clone(&dirty);
      ui.tab_clear_all.on_click(move |_| {
        cleared.borrow_mut().push("All".into()); ui2.tab_cleared.set_label(&crate::i18n::text(&("All")));
        stops.borrow_mut().clear(); dirty.set(true); fill(None); ui2.tab_position.change_value("");
      }); }
    let accepted = chrome.show();
    let patch = if accepted {
        let mut patch = model::ParagraphProperties::default();
        if dirty.get() { patch.tabs = Some(stops.borrow().clone()); }
        let default = ui.tab_default.get_value() as f32;
        if (default - shown_default).abs() > TAB_POSITION_EPSILON_MM { patch.default_tab_mm = Some(default); }
        Some(patch)
    } else { None };
    ui.tab_dialog.destroy();
    patch
}

// Borders and Shading -------------------------------------------------------------------

const SETTINGS: [&str; 5] = ["None", "Box", "Shadow", "3-D", "Custom"];
const LINE_STYLES: [&str; 9] = ["Single", "Dotted", "Dashed", "Dash-dot", "Dash-dot-dot", "Double", "Thick-thin", "Thin-thick", "Wave"];

/// Draw one border line (a Word line style) from (x0,y0) to (x1,y1).
fn border_line(dc: &AutoBufferedPaintDC, panel: &Panel, style: usize, colour: Colour, width: i32, from: (i32, i32), to: (i32, i32)) {
    let horizontal = from.1 == to.1;
    let offset = |d: i32| if horizontal { ((from.0, from.1 + d), (to.0, to.1 + d)) } else { ((from.0 + d, from.1), (to.0 + d, to.1)) };
    let line = |pen: PenStyle, w: i32, d: i32| { let (a, b) = offset(d); dc.set_pen(colour, w.max(1), pen); dc.draw_line(a.0, a.1, b.0, b.1); };
    let gap = width.max(dip(panel, 1)) + dip(panel, 1);
    match LINE_STYLES.get(style).copied().unwrap_or("Single") {
        "Dotted" => line(PenStyle::Dot, width, 0),
        "Dashed" => line(PenStyle::ShortDash, width, 0),
        "Dash-dot" | "Dash-dot-dot" => line(PenStyle::DotDash, width, 0),
        "Double" => { line(PenStyle::Solid, width, -gap / 2 - 1); line(PenStyle::Solid, width, gap / 2 + 1); }
        "Thick-thin" => { line(PenStyle::Solid, width * 2, -gap / 2 - 1); line(PenStyle::Solid, width, gap / 2 + 1); }
        "Thin-thick" => { line(PenStyle::Solid, width, -gap / 2 - 1); line(PenStyle::Solid, width * 2, gap / 2 + 1); }
        "Wave" => {
            dc.set_pen(colour, width.max(1), PenStyle::Solid);
            let step = dip(panel, 3).max(2);
            let (length, mut t, mut up) = (if horizontal { to.0 - from.0 } else { to.1 - from.1 }, 0, true);
            while t < length {
                let n = (t + step).min(length); let (a, b) = if up { (0, step) } else { (step, 0) };
                if horizontal { dc.draw_line(from.0 + t, from.1 + a - step / 2, from.0 + n, from.1 + b - step / 2); }
                else { dc.draw_line(from.0 + a - step / 2, from.1 + t, from.0 + b - step / 2, from.1 + n); }
                t = n; up = !up;
            }
        }
        _ => line(PenStyle::Solid, width, 0),
    }
}

fn border_width_pt(label: &str) -> f32 {
    match label { "¼ pt" => 0.25, "½ pt" => 0.5, "¾ pt" => 0.75, "1 pt" => 1.0, "1½ pt" => 1.5, "2¼ pt" => 2.25, "3 pt" => 3.0, "4½ pt" => 4.5, "6 pt" => 6.0, _ => 0.5 }
}
fn border_width_px(panel: &Panel, label: &str) -> i32 {
    ((border_width_pt(label) * px_per_pt(panel)).round() as i32).max(1)
}

/// Grey text lines standing in for a paragraph, inside `rect`.
fn paragraph_lines(dc: &AutoBufferedPaintDC, panel: &Panel, rect: (i32, i32, i32, i32), rows: i32) {
    let (x, y, w, h) = rect; let gap = (h / (rows + 1)).max(dip(panel, 3));
    dc.set_brush(rgb(LINE_STRONG), BrushStyle::Solid); dc.set_pen(rgb(LINE_STRONG), 1, PenStyle::Solid);
    for r in 0..rows { let lw = if r == rows - 1 { w * 3 / 5 } else { w }; dc.draw_rectangle(x, y + gap * (r + 1) - 1, lw, dip(panel, 2)); }
}

fn borders_dialog(parent: Dialog, style: &mut Style, sheet: &StyleSheet, pending: &Pending) -> bool {
    let ui = Rc::new(crate::i18n::localized(BordersDialogUi::new(Some(&parent), false)));
    let chrome = Chrome::new(ui.bdr_dialog, ui.bdr_card, &[(ui.bdr_tab_0, "Borders"), (ui.bdr_tab_1, "Shading")],
        &[ui.bdr_page_0, ui.bdr_page_1], (ui.bdr_ok, "OK"), Some(ui.bdr_cancel), &[(ui.bdr_options, "Horizontal Line…")], &[]);
    ui.bdr_options.set_tooltip(&crate::i18n::text(&("Apply a horizontal rule below the paragraph")));
    let edge_buttons = [ui.bdr_edge_top, ui.bdr_edge_bottom, ui.bdr_edge_left, ui.bdr_edge_right, ui.bdr_edge_between];
    let side = dip(&ui.bdr_dialog, 28);
    for (button, (edge, tip)) in edge_buttons.iter().zip([("style-border-top", "Top border"), ("style-border-bottom", "Bottom border"),
        ("style-border-left", "Left border"), ("style-border-right", "Right border"), ("style-border-between", "Border between paragraphs")]) {
        button.set_min_size(Size::new(side, side)); skin(*button, "icon", tip, edge);
    }
    // Start from the style's paragraph borders or its inherited borders.
    let own_borders = style.paragraph.borders.clone();
    let shown_borders = own_borders.clone().or_else(|| sheet.resolve_paragraph(style.based_on.as_ref()).borders).unwrap_or_default();
    let edges = Rc::new(RefCell::new([shown_borders.top.is_some(), shown_borders.bottom.is_some(),
        shown_borders.left.is_some(), shown_borders.right.is_some(), shown_borders.inside_horizontal.is_some()]));
    if let Some(saved) = pending.borrow().get("bdr_edges") { let mut e = edges.borrow_mut(); for (i, c) in saved.chars().enumerate().take(5) { e[i] = c == '1'; } }
    let setting = Tiles::new(ui.bdr_setting, SETTINGS.len(), 1, 42, Box::new(|dc, panel, (x, y, w, h), index, _| {
        // Icon: a small page with text lines and the setting's frame, then the label.
        let icon = (x + dip(panel, 6), y + dip(panel, 5), dip(panel, 28), h - dip(panel, 10));
        dc.set_brush(Colour::rgb(255, 255, 255), BrushStyle::Solid); dc.set_pen(rgb(LINE), 1, PenStyle::Solid);
        dc.draw_rectangle(icon.0, icon.1, icon.2, icon.3);
        paragraph_lines(dc, panel, (icon.0 + dip(panel, 5), icon.1 + dip(panel, 2), icon.2 - dip(panel, 10), icon.3 - dip(panel, 4)), 3);
        let ink = rgb(INK); let stroke = dip(panel, 1).max(1);
        match index {
            1 | 3 => { dc.set_brush(Colour::rgb(255, 255, 255), BrushStyle::Transparent); dc.set_pen(ink, stroke, PenStyle::Solid); dc.draw_rectangle(icon.0 + 2, icon.1 + 2, icon.2 - 4, icon.3 - 4); }
            2 => { dc.set_brush(rgb(GHOST), BrushStyle::Solid); dc.set_pen(rgb(GHOST), 1, PenStyle::Solid); dc.draw_rectangle(icon.0 + 5, icon.1 + 5, icon.2 - 5, icon.3 - 5);
                   dc.set_brush(Colour::rgb(255, 255, 255), BrushStyle::Solid); dc.set_pen(ink, stroke, PenStyle::Solid); dc.draw_rectangle(icon.0 + 2, icon.1 + 2, icon.2 - 6, icon.3 - 6); }
            4 => { dc.set_pen(ink, stroke, PenStyle::Solid); dc.draw_line(icon.0 + 2, icon.1 + 2, icon.0 + icon.2 - 2, icon.1 + 2); dc.draw_line(icon.0 + 2, icon.1 + 2, icon.0 + 2, icon.1 + icon.3 - 2); }
            _ => {}
        }
        if index == 3 { dc.set_pen(rgb(GHOST), stroke, PenStyle::Solid); dc.draw_line(icon.0 + icon.2 - 2, icon.1 + 4, icon.0 + icon.2 - 2, icon.1 + icon.3 - 1); dc.draw_line(icon.0 + 4, icon.1 + icon.3 - 2, icon.0 + icon.2 - 1, icon.1 + icon.3 - 2); }
        if let Some(font) = face_font(9.0, false, false, false, "Segoe UI") { dc.set_font(&font); }
        dc.set_text_foreground(ink);
        let th = dc.get_text_extent(&crate::i18n::text(&("Hg"))).1; dc.draw_text(&crate::i18n::text(&(SETTINGS[index])), icon.0 + icon.2 + dip(panel, 8), y + (h - th) / 2);
        let _ = w;
    }));
    let line_style = Tiles::new(ui.bdr_style_list, LINE_STYLES.len(), 1, 14, Box::new(|dc, panel, (x, y, w, h), index, _| {
        border_line(dc, panel, index, rgb(INK), dip(panel, 1).max(1), (x + dip(panel, 8), y + h / 2), (x + w - dip(panel, 8), y + h / 2));
    }));
    let shown_border = shown_borders.top.as_ref().or(shown_borders.bottom.as_ref()).or(shown_borders.left.as_ref())
        .or(shown_borders.right.as_ref()).or(shown_borders.inside_horizontal.as_ref());
    fill_colour_list(ui.bdr_color, &ui.bdr_dialog, shown_border.map(|b| b.color.as_str()), AUTOMATIC);
    let width_label = shown_border.map(|b| b.width_pt).and_then(|width| ["¼ pt","½ pt","¾ pt","1 pt","1½ pt","2¼ pt","3 pt","4½ pt","6 pt"]
        .into_iter().find(|label| (border_width_pt(label)-width).abs()<0.01)).unwrap_or("½ pt");
    combo_index(ui.bdr_width, width_label);
    fill_colour_list(ui.shd_fill, &ui.bdr_dialog, style.paragraph.shading.as_deref().or(sheet.resolve_paragraph(style.based_on.as_ref()).shading.as_deref()).filter(|v| *v != "none"), NO_COLOUR);
    fill_colour_list(ui.shd_pattern_color, &ui.bdr_dialog, None, AUTOMATIC);
    let shown_fill = colour_value(&ui.shd_fill.get_value());
    let ui_only = [("bdr_color", Ctl::Colour(ui.bdr_color, ui.bdr_dialog, false)), ("bdr_width", Ctl::Combo(ui.bdr_width)),
        ("bdr_apply", Ctl::Combo(ui.bdr_apply)), ("bdr_from_top", Ctl::Spin(ui.bdr_from_top)), ("bdr_from_bottom", Ctl::Spin(ui.bdr_from_bottom)),
        ("bdr_from_left", Ctl::Spin(ui.bdr_from_left)), ("bdr_from_right", Ctl::Spin(ui.bdr_from_right)),
        ("shd_pattern", Ctl::Combo(ui.shd_pattern)), ("shd_pattern_color", Ctl::Colour(ui.shd_pattern_color, ui.bdr_dialog, false)),
        ("shd_apply", Ctl::Combo(ui.shd_apply))];
    let ui_before=restore(pending, &ui_only);
    if let Some(i) = pending.borrow().get("bdr_setting").and_then(|v| v.parse().ok()) { setting.selected.set(i); }
    if let Some(i) = pending.borrow().get("bdr_style").and_then(|v| v.parse().ok()) { line_style.selected.set(i); }
    else if let Some(border) = shown_border { line_style.selected.set(match border.style { BorderStyle::Dotted=>1,BorderStyle::Dashed=>2,
        BorderStyle::Double=>5,_=>0 }); }
    let edges_before=*edges.borrow();let setting_before=setting.selected.get();let line_style_before=line_style.selected.get();
    // Preview: a paragraph with the chosen borders; clicking near an edge toggles it.
    let paint_edges = { let ui = Rc::clone(&ui); let edges = Rc::clone(&edges); let line_style = Rc::clone(&line_style);
        move |dc: &AutoBufferedPaintDC, panel: &Panel| {
        paint_rounded_surface(dc, panel, Colour::rgb(255, 255, 255), rgb(LINE_STRONG), 5);
        let size = panel.get_client_size(); let m = dip(panel, 22);
        let (x0, y0, x1, y1) = (m, m, size.width - m, size.height - m);
        let fill = colour_value(&ui.shd_fill.get_value()).and_then(|h| hex_colour(&h));
        if let Some(fill) = fill { dc.set_brush(fill, BrushStyle::Solid); dc.set_pen(fill, 1, PenStyle::Solid); dc.draw_rectangle(x0, y0, x1 - x0, y1 - y0); }
        let mid = (y0 + y1) / 2;
        paragraph_lines(dc, panel, (x0 + dip(panel, 8), y0, x1 - x0 - dip(panel, 16), mid - y0 - dip(panel, 4)), 3);
        paragraph_lines(dc, panel, (x0 + dip(panel, 8), mid + dip(panel, 2), x1 - x0 - dip(panel, 16), y1 - mid - dip(panel, 4)), 3);
        // Corner guides, as in Word's diagram.
        dc.set_pen(rgb(GHOST), 1, PenStyle::Solid);
        for (cx, cy) in [(x0, y0), (x1, y0), (x0, y1), (x1, y1)] { let t = dip(panel, 6);
            dc.draw_line(cx - if cx == x0 { t } else { -t }, cy, cx, cy); dc.draw_line(cx, cy - if cy == y0 { t } else { -t }, cx, cy); }
        let colour = colour_value(&ui.bdr_color.get_value()).and_then(|h| hex_colour(&h)).unwrap_or(Colour::rgb(0, 0, 0));
        let width = border_width_px(panel, &crate::i18n::choice_source(&ui.bdr_width.get_value(), &["¼ pt", "½ pt", "¾ pt", "1 pt", "1½ pt", "2¼ pt", "3 pt", "4½ pt", "6 pt"]));
        let style = line_style.selected.get(); let e = edges.borrow();
        if e[0] { border_line(dc, panel, style, colour, width, (x0, y0), (x1, y0)); }
        if e[1] { border_line(dc, panel, style, colour, width, (x0, y1), (x1, y1)); }
        if e[2] { border_line(dc, panel, style, colour, width, (x0, y0), (x0, y1)); }
        if e[3] { border_line(dc, panel, style, colour, width, (x1, y0), (x1, y1)); }
        if e[4] { border_line(dc, panel, style, colour, width, (x0, mid), (x1, mid)); }
    } };
    painted(ui.bdr_preview, paint_edges);
    // Shading preview: fill, then the pattern (percentage tint or hatching) in the pattern colour.
    { let ui2 = Rc::clone(&ui); painted(ui.shd_preview, move |dc, panel| {
        paint_rounded_surface(dc, panel, Colour::rgb(255, 255, 255), rgb(LINE_STRONG), 5);
        let size = panel.get_client_size(); let m = dip(panel, 16);
        let (x, y, w, h) = (m, m, size.width - 2 * m, size.height - 2 * m);
        let fill = colour_value(&ui2.shd_fill.get_value()).and_then(|v| hex_colour(&v)).unwrap_or(Colour::rgb(255, 255, 255));
        let pattern = colour_value(&ui2.shd_pattern_color.get_value()).and_then(|v| hex_colour(&v)).unwrap_or(Colour::rgb(0, 0, 0));
        let name = crate::i18n::choice_source(&ui2.shd_pattern.get_value(), &["Clear", "Solid (100%)", "5%", "10%", "12.5%", "20%", "25%", "30%", "40%", "50%", "60%", "70%", "75%", "80%", "90%", "Dk Horizontal", "Dk Vertical", "Dk Down Diagonal", "Dk Up Diagonal", "Dk Grid", "Lt Horizontal", "Lt Vertical", "Lt Grid"]);
        let percent = if name.starts_with("Solid") { Some(100.0) } else { name.trim_end_matches('%').parse::<f32>().ok() };
        let base = match percent { Some(p) => { let t = p / 100.0; let mix = |a: u8, b: u8| (a as f32 * (1.0 - t) + b as f32 * t).round() as u8;
            Colour::rgb(mix(fill.r, pattern.r), mix(fill.g, pattern.g), mix(fill.b, pattern.b)) } None => fill };
        dc.set_brush(base, BrushStyle::Solid); dc.set_pen(rgb(LINE), 1, PenStyle::Solid); dc.draw_rectangle(x, y, w, h);
        if percent.is_none() && name != "Clear" {
            let step = if name.starts_with("Dk") { dip(panel, 3) } else { dip(panel, 6) };
            dc.set_pen(pattern, 1, PenStyle::Solid);
            let mut t = 0;
            while t < w + h {
                if name.contains("Horizontal") || name.contains("Grid") { if t < h { dc.draw_line(x, y + t, x + w, y + t); } }
                if name.contains("Vertical") || name.contains("Grid") { if t < w { dc.draw_line(x + t, y, x + t, y + h); } }
                if name.contains("Down Diagonal") { let (a, b) = ((x + t - h).max(x), (x + t).min(x + w)); if a < b { dc.draw_line(a, y + (a - (x + t - h)), b, y + (b - (x + t - h))); } }
                if name.contains("Up Diagonal") { let (a, b) = ((x + t - h).max(x), (x + t).min(x + w)); if a < b { dc.draw_line(a, y + h - (a - (x + t - h)), b, y + h - (b - (x + t - h))); } }
                t += step;
            }
        }
        paragraph_lines(dc, panel, (x + dip(panel, 10), y + dip(panel, 4), w - dip(panel, 20), h - dip(panel, 8)), 5);
    }); }
    let refresh: Rc<dyn Fn()> = { let ui = Rc::clone(&ui); let edges = Rc::clone(&edges); Rc::new(move || {
        let e = edges.borrow(); for (button, on) in [ui.bdr_edge_top, ui.bdr_edge_bottom, ui.bdr_edge_left, ui.bdr_edge_right, ui.bdr_edge_between].iter().zip(e.iter()) { skin::selected(button, *on); }
        ui.bdr_preview.refresh(false, None); ui.shd_preview.refresh(false, None);
    }) };
    refresh();
    { let edges=Rc::clone(&edges);let refresh=Rc::clone(&refresh);let setting=Rc::clone(&setting);
      ui.bdr_options.on_click(move |_| {
        *edges.borrow_mut()=[false,true,false,false,false];
        setting.selected.set(4);setting.panel.refresh(false,None);refresh();
      }); }
    // Setting presets (Word): None clears, Box / Shadow / 3-D set the four sides.
    { let edges = Rc::clone(&edges); let refresh = Rc::clone(&refresh);
      setting.on_change(move |index| { match index { 0 => *edges.borrow_mut() = [false; 5], 1 | 2 | 3 => *edges.borrow_mut() = [true, true, true, true, false], _ => {} } refresh(); }); }
    { let refresh = Rc::clone(&refresh); line_style.on_change(move |_| refresh()); }
    for (index, button) in edge_buttons.iter().enumerate() {
        let edges = Rc::clone(&edges); let refresh = Rc::clone(&refresh); let setting = Rc::clone(&setting);
        button.on_click(move |_| { { let mut e = edges.borrow_mut(); e[index] = !e[index]; } setting.selected.set(4); setting.panel.refresh(false, None); refresh(); });
    }
    { let edges = Rc::clone(&edges); let refresh = Rc::clone(&refresh); let setting = Rc::clone(&setting); let panel = ui.bdr_preview;
      panel.on_mouse_left_down(move |event| {
        if let WindowEventData::MouseButton(m) = &event { if let Some(p) = m.event.get_position() {
            let size = panel.get_client_size(); let margin = dip(&panel, 22); let reach = dip(&panel, 12);
            let (x0, y0, x1, y1) = (margin, margin, size.width - margin, size.height - margin);
            let hit = [(p.y - y0).abs() < reach, (p.y - y1).abs() < reach, (p.x - x0).abs() < reach, (p.x - x1).abs() < reach, (p.y - (y0 + y1) / 2).abs() < reach / 2];
            if let Some(index) = hit.iter().position(|h| *h) { { let mut e = edges.borrow_mut(); e[index] = !e[index]; } setting.selected.set(4); setting.panel.refresh(false, None); refresh(); }
        } }
        event.skip(true);
      }); }
    for combo in [ui.bdr_width, ui.bdr_apply, ui.shd_pattern, ui.shd_apply] { let refresh = Rc::clone(&refresh); combo.on_selection_changed(move |_| refresh()); }
    for combo in [ui.bdr_color, ui.shd_fill, ui.shd_pattern_color] { let refresh = Rc::clone(&refresh); bind_colour_list(combo, ui.bdr_dialog, move || refresh()); }
    let accepted = chrome.show();
    if accepted {
        let border = Border { width_pt:border_width_pt(&crate::i18n::choice_source(&ui.bdr_width.get_value(), &["¼ pt", "½ pt", "¾ pt", "1 pt", "1½ pt", "2¼ pt", "3 pt", "4½ pt", "6 pt"])),
            color:colour_value(&ui.bdr_color.get_value()).unwrap_or_else(||"#000000".into()),
            style:match line_style.selected.get() {1=>BorderStyle::Dotted,2..=4=>BorderStyle::Dashed,5=>BorderStyle::Double,_=>BorderStyle::Single} };
        let selected = *edges.borrow();
        let borders = BorderSet {top:selected[0].then(||border.clone()),bottom:selected[1].then(||border.clone()),
            left:selected[2].then(||border.clone()),right:selected[3].then(||border.clone()),
            inside_horizontal:selected[4].then_some(border),inside_vertical:None};
        style.paragraph.borders = if borders == shown_borders { own_borders } else { Some(borders) };
        let fill = colour_value(&ui.shd_fill.get_value());
        if fill != shown_fill {
            // "No Color" over an inherited fill stores "none" so the base's fill is cleared.
            let inherited = sheet.resolve_paragraph(style.based_on.as_ref()).shading.filter(|v| v != "none");
            style.paragraph.shading = fill.or_else(|| inherited.map(|_| "none".to_owned()));
        }
        remember_changed(pending, &ui_only, &ui_before);
        let mut p = pending.borrow_mut();
        if *edges.borrow()!=edges_before {p.set("bdr_edges", edges.borrow().iter().map(|e| if *e { '1' } else { '0' }).collect());}
        if setting.selected.get()!=setting_before {p.set("bdr_setting", setting.selected.get().to_string());}
        if line_style.selected.get()!=line_style_before {p.set("bdr_style", line_style.selected.get().to_string());}
    }
    ui.bdr_dialog.destroy();
    accepted
}

// Language --------------------------------------------------------------------------------------

const LANGUAGES: [(&str, &str); 36] = [
    ("Arabic (Saudi Arabia)", "ar-SA"), ("Catalan", "ca-ES"), ("Chinese (Simplified)", "zh-CN"), ("Chinese (Traditional)", "zh-TW"),
    ("Czech", "cs-CZ"), ("Danish", "da-DK"), ("Dutch (Netherlands)", "nl-NL"), ("English (Australia)", "en-AU"),
    ("English (Canada)", "en-CA"), ("English (United Kingdom)", "en-GB"), ("English (United States)", "en-US"),
    ("Finnish", "fi-FI"), ("French (Canada)", "fr-CA"), ("French (France)", "fr-FR"), ("German (Germany)", "de-DE"),
    ("German (Switzerland)", "de-CH"), ("Greek", "el-GR"), ("Hebrew", "he-IL"), ("Hindi", "hi-IN"), ("Hungarian", "hu-HU"),
    ("Italian (Italy)", "it-IT"), ("Japanese", "ja-JP"), ("Korean", "ko-KR"), ("Latin", "la"), ("Norwegian (Bokmål)", "nb-NO"),
    ("Polish", "pl-PL"), ("Portuguese (Brazil)", "pt-BR"), ("Portuguese (Portugal)", "pt-PT"), ("Romanian", "ro-RO"),
    ("Russian", "ru-RU"), ("Spanish (Mexico)", "es-MX"), ("Spanish (Spain)", "es-ES"), ("Swedish", "sv-SE"), ("Turkish", "tr-TR"),
    ("Ukrainian", "uk-UA"), ("Vietnamese", "vi-VN"),
];

fn language_dialog(parent: Dialog, style: &mut Style, sheet: &StyleSheet, pending: &Pending) -> bool {
    let ui = Rc::new(crate::i18n::localized(LanguageDialogUi::new(Some(&parent), false)));
    let chrome = Chrome::new(ui.lng_dialog, ui.lng_card, &[], &[], (ui.lng_ok, "OK"), Some(ui.lng_cancel), &[(ui.lng_default, "Set As Default")], &[]);
    ui.lng_note.wrap(ui.lng_dialog.from_dip_int(400));
    ui.lng_default.set_tooltip(&crate::i18n::text(&("Use this language for the document's Normal style")));
    for (name, tag) in LANGUAGES { ui.lng_list.append(&format!("{}  ·  {tag}",crate::i18n::text(name))); }
    let shown = style.character.language.clone().or(sheet.resolve_character(style.based_on.as_ref()).language).unwrap_or_default();
    let select = { let ui = Rc::clone(&ui); move |tag: &str| {
        let lower = tag.to_ascii_lowercase();
        let index = LANGUAGES.iter().position(|(_, t)| t.to_ascii_lowercase() == lower)
            .or_else(|| (!lower.is_empty()).then(|| LANGUAGES.iter().position(|(_, t)| t.to_ascii_lowercase().starts_with(&format!("{lower}-")))).flatten());
        if let Some(i) = index { ui.lng_list.set_selection(i as u32, true); ui.lng_list.ensure_visible(i as i32); }
    } };
    ui.lng_tag.change_value(&shown); select(&shown);
    { let ui2 = Rc::clone(&ui); ui.lng_list.on_selection_changed(move |_| if let Some(i) = ui2.lng_list.get_selection() { ui2.lng_tag.change_value(LANGUAGES[i as usize].1); }); }
    { let ui2 = Rc::clone(&ui); ui.lng_tag.on_text_updated(move |_| select(ui2.lng_tag.get_value().trim())); }
    let ui_only = [("lng_no_proof", Ctl::Check(ui.lng_no_proof)), ("lng_detect", Ctl::Check(ui.lng_detect))];
    let ui_before=restore(pending, &ui_only);
    { let ui2=Rc::clone(&ui);let pending=Rc::clone(pending);
      ui.lng_default.on_click(move |_| {
        let tag=ui2.lng_tag.get_value().trim().to_owned();
        if !tag.is_empty() && tag.len()<=64 && tag.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-') {
            pending.borrow_mut().values.insert("lng_make_default".into(),tag.clone());
            ui2.lng_note.set_label(&crate::i18n::text(&(&format!("{tag} will be used by the Normal style after you save."))));
            ui2.lng_note.wrap(ui2.lng_dialog.from_dip_int(400));
            ui2.lng_dialog.layout();
        }
      }); }
    let accepted = chrome.show();
    if accepted {
        let tag = ui.lng_tag.get_value().trim().to_owned();
        let valid = tag.len() <= 64 && tag.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
        if valid && tag != shown { style.character.language = Some(tag); }
        remember_changed(pending, &ui_only, &ui_before);
    }
    ui.lng_dialog.destroy();
    accepted
}

// Frame ----------------------------------------------------------------------------------------

fn frame_dialog(parent: Dialog, pending: &Pending) -> bool {
    let ui = Rc::new(crate::i18n::localized(FrameDialogUi::new(Some(&parent), false)));
    let chrome = Chrome::new(ui.frm_dialog, ui.frm_card, &[], &[], (ui.frm_ok, "OK"), Some(ui.frm_cancel), &[(ui.frm_remove, "Remove Frame")], &[]);
    let wrapping = Tiles::new(ui.frm_wrapping, 2, 2, 80, Box::new(|dc, panel, (x, y, w, h), index, _| {
        // Word's two wrapping pictures: text above/below the frame, or around it.
        let inner = (x + dip(panel, 14), y + dip(panel, 8), w - dip(panel, 28), h - dip(panel, 30));
        let box_w = inner.2 / 3; let bx = inner.0 + (inner.2 - box_w) / 2; let by = inner.1 + inner.3 / 3;
        dc.set_brush(rgb(ACCENT_SOFT), BrushStyle::Solid); dc.set_pen(rgb(ACCENT), 1, PenStyle::Solid); dc.draw_rectangle(bx, by, box_w, inner.3 / 3);
        dc.set_brush(rgb(LINE_STRONG), BrushStyle::Solid); dc.set_pen(rgb(LINE_STRONG), 1, PenStyle::Solid);
        let rows = 6; let step = inner.3 / rows;
        for r in 0..rows { let ly = inner.1 + r * step + step / 2;
            let beside = ly >= by && ly <= by + inner.3 / 3;
            if beside { if index == 1 { dc.draw_rectangle(inner.0, ly, bx - inner.0 - 3, 2); dc.draw_rectangle(bx + box_w + 3, ly, inner.0 + inner.2 - bx - box_w - 3, 2); } }
            else { dc.draw_rectangle(inner.0, ly, inner.2, 2); } }
        if let Some(font) = face_font(9.0, false, false, false, "Segoe UI") { dc.set_font(&font); }
        dc.set_text_foreground(rgb(INK)); let label = ["None", "Around"][index];
        let (tw, th) = dc.get_text_extent(&crate::i18n::text(&(label))); dc.draw_text(&crate::i18n::text(&(label)), x + (w - tw) / 2, y + h - th - dip(panel, 6));
    }));
    let ui_only = [("frm_width_rule", Ctl::Combo(ui.frm_width_rule)), ("frm_width_at", Ctl::Spin(ui.frm_width_at)),
        ("frm_height_rule", Ctl::Combo(ui.frm_height_rule)), ("frm_height_at", Ctl::Spin(ui.frm_height_at)),
        ("frm_h_position", Ctl::Combo(ui.frm_h_position)), ("frm_h_relative", Ctl::Combo(ui.frm_h_relative)), ("frm_h_distance", Ctl::Spin(ui.frm_h_distance)),
        ("frm_v_position", Ctl::Combo(ui.frm_v_position)), ("frm_v_relative", Ctl::Combo(ui.frm_v_relative)), ("frm_v_distance", Ctl::Spin(ui.frm_v_distance)),
        ("frm_move", Ctl::Check(ui.frm_move)), ("frm_lock", Ctl::Check(ui.frm_lock))];
    let ui_before=restore(pending, &ui_only);
    if let Some(i) = pending.borrow().get("frm_wrapping").and_then(|v| v.parse().ok()) { wrapping.selected.set(i); }
    let wrapping_before=wrapping.selected.get();
    let refresh: Rc<dyn Fn()> = { let ui = Rc::clone(&ui); Rc::new(move || {
        // "At" applies only to exact / minimum sizes, as in Word.
        ui.frm_width_at.enable(crate::i18n::choice_source(&ui.frm_width_rule.get_value(), &["Auto", "Exactly"]) != "Auto");
        ui.frm_height_at.enable(crate::i18n::choice_source(&ui.frm_height_rule.get_value(), &["Auto", "At least", "Exactly"]) != "Auto");
    }) };
    refresh();
    for combo in [ui.frm_width_rule, ui.frm_height_rule] { let refresh = Rc::clone(&refresh); combo.on_selection_changed(move |_| refresh()); }
    { let ui2 = Rc::clone(&ui); let wrapping = Rc::clone(&wrapping); let refresh = Rc::clone(&refresh);
      ui.frm_remove.on_click(move |_| {
        // Remove Frame: back to Word's defaults for a frameless paragraph.
        wrapping.select(0);
        for (combo, value) in [(ui2.frm_width_rule, "Auto"), (ui2.frm_height_rule, "Auto"), (ui2.frm_h_position, "Left"),
            (ui2.frm_h_relative, "Column"), (ui2.frm_v_position, "Top"), (ui2.frm_v_relative, "Paragraph")] { combo.set_value(&crate::i18n::text(value)); }
        for spin in [ui2.frm_width_at, ui2.frm_height_at, ui2.frm_h_distance, ui2.frm_v_distance] { spin.set_value(0.0); }
        ui2.frm_move.set_value(true); ui2.frm_lock.set_value(false); refresh();
      }); }
    let accepted = chrome.show();
    if accepted { remember_changed(pending, &ui_only, &ui_before);
        if wrapping.selected.get()!=wrapping_before {pending.borrow_mut().set("frm_wrapping", wrapping.selected.get().to_string());}
    }
    ui.frm_dialog.destroy();
    accepted
}

// Numbering and Bullets ----------------------------------------------------------------

const BULLETS: [&str; 8] = ["None", "●", "○", "■", "◆", "➢", "✓", "–"];
const NUMBERS: [(&str, [&str; 3]); 8] = [("None", ["", "", ""]), ("1.", ["1.", "2.", "3."]), ("1)", ["1)", "2)", "3)"]),
    ("I.", ["I.", "II.", "III."]), ("A.", ["A.", "B.", "C."]), ("a)", ["a)", "b)", "c)"]), ("a.", ["a.", "b.", "c."]), ("i.", ["i.", "ii.", "iii."])];

/// One library tile: three list rows (marker + text line), or "None".
fn list_tile(dc: &AutoBufferedPaintDC, panel: &Panel, (x, y, w, h): (i32, i32, i32, i32), markers: [&str; 3], none: bool) {
    if let Some(font) = face_font(if none { 11.0 } else { 10.0 }, false, false, false, "Segoe UI") { dc.set_font(&font); }
    dc.set_text_foreground(rgb(INK));
    if none { let (tw, th) = dc.get_text_extent(&crate::i18n::text(&("None"))); dc.draw_text(&crate::i18n::text(&("None")), x + (w - tw) / 2, y + (h - th) / 2); return; }
    let row = h / 4;
    for (r, marker) in markers.iter().enumerate() {
        let cy = y + row * (r as i32 + 1);
        let (_, th) = dc.get_text_extent(&crate::i18n::text(&(marker)));
        dc.draw_text(&crate::i18n::text(&(marker)), x + dip(panel, 12), cy - th / 2);
        dc.set_brush(rgb(LINE_STRONG), BrushStyle::Solid); dc.set_pen(rgb(LINE_STRONG), 1, PenStyle::Solid);
        dc.draw_rectangle(x + dip(panel, 38), cy - 1, w - dip(panel, 52), dip(panel, 2));
    }
}

fn numbering_dialog(parent: Dialog, pending: &Pending) -> bool {
    let ui = Rc::new(crate::i18n::localized(NumberingDialogUi::new(Some(&parent), false)));
    let chrome = Chrome::new(ui.num_dialog, ui.num_card, &[(ui.num_tab_0, "Bullets"), (ui.num_tab_1, "Numbering")],
        &[ui.num_page_0, ui.num_page_1], (ui.num_ok, "OK"), Some(ui.num_cancel), &[],
        &[(ui.num_define_bullet, "Define New Bullet…"), (ui.num_define_number, "Define New Number Format…")]);
    let bullet_draft = Rc::clone(pending);
    let bullets = Tiles::new(ui.num_bullets, BULLETS.len() + 1, 4, 100, Box::new(move |dc, panel, rect, index, _| {
        let custom = bullet_draft.borrow().get("num_custom_bullet").cloned().unwrap_or_else(|| "★".into());
        let b = if index == BULLETS.len() { custom.as_str() } else { BULLETS[index] };
        list_tile(dc, panel, rect, [b, b, b], index == 0);
    }));
    let number_draft = Rc::clone(pending);
    let numbers = Tiles::new(ui.num_numbers, NUMBERS.len() + 1, 4, 100, Box::new(move |dc, panel, rect, index, _| {
        if index == NUMBERS.len() {
            let template = number_draft.borrow().get("num_custom_number").cloned().unwrap_or_else(|| "%1.".into());
            let labels = [1, 2, 3].map(|n| template.replace("%1", &n.to_string()));
            list_tile(dc, panel, rect, [labels[0].as_str(), labels[1].as_str(), labels[2].as_str()], false);
        } else { list_tile(dc, panel, rect, NUMBERS[index].1, index == 0); }
    }));
    let ui_only = [("num_restart", Ctl::Radio(ui.num_restart)), ("num_continue", Ctl::Radio(ui.num_continue)),
        ("num_start", Ctl::Spin(ui.num_start)), ("num_level", Ctl::Combo(ui.num_level))];
    let ui_before=restore(pending, &ui_only);
    if let Some(i) = pending.borrow().get("num_bullets").and_then(|v| v.parse().ok()) { bullets.selected.set(i); }
    if let Some(i) = pending.borrow().get("num_numbers").and_then(|v| v.parse().ok()) { numbers.selected.set(i); }
    let bullets_before=bullets.selected.get();let numbers_before=numbers.selected.get();
    // A list is either bulleted or numbered: choosing one clears the other.
    { let numbers = Rc::clone(&numbers); bullets.on_change(move |i| if i > 0 { numbers.selected.set(0); numbers.panel.refresh(false, None); }); }
    { let bullets = Rc::clone(&bullets); numbers.on_change(move |i| if i > 0 { bullets.selected.set(0); bullets.panel.refresh(false, None); }); }
    { let ui2=Rc::clone(&ui);let bullets=Rc::clone(&bullets);let pending=Rc::clone(pending);
      ui.num_define_bullet.on_click(move |_| {
        let initial=pending.borrow().get("num_custom_bullet").cloned().unwrap_or_else(||"★".into());
        if let Some(marker) = input_dialog::text(&ui2.num_dialog, "Define New Bullet", "Bullet character:", &initial, false) {
            if !marker.trim().is_empty() && marker.chars().count()<=8 {pending.borrow_mut().set("num_custom_bullet",marker);bullets.select(BULLETS.len());}
        }
      }); }
    { let ui2=Rc::clone(&ui);let numbers=Rc::clone(&numbers);let pending=Rc::clone(pending);
      ui.num_define_number.on_click(move |_| {
        let initial=pending.borrow().get("num_custom_number").cloned().unwrap_or_else(||"%1.".into());
        if let Some(template) = input_dialog::text(&ui2.num_dialog, "Define New Number Format", "Number format (use %1 for the number):", &initial, false) {
            if template.contains("%1") && template.chars().count()<=32 {pending.borrow_mut().set("num_custom_number",template);numbers.select(NUMBERS.len());}
        }
      }); }
    let accepted = chrome.show();
    if accepted {
        remember_changed(pending, &ui_only, &ui_before);
        let mut p = pending.borrow_mut();
        if bullets.selected.get()!=bullets_before {p.set("num_bullets", bullets.selected.get().to_string());}
        if numbers.selected.get()!=numbers_before {p.set("num_numbers", numbers.selected.get().to_string());}
    }
    ui.num_dialog.destroy();
    accepted
}

// Customize Keyboard --------------------------------------------------------------------------

/// "Ctrl+Shift+K" style text for a key press; None for bare modifiers.
pub(crate) fn shortcut_text(key: &wxdragon::event::window_events::KeyboardEvent) -> Option<String> {
    let code = key.get_key_code()?;
    let name = match code {
        c if (65..=90).contains(&c) || (48..=57).contains(&c) => (c as u8 as char).to_string(),
        c if (wxdragon::keycode::WXK_F1..=wxdragon::keycode::WXK_F24).contains(&c) => format!("F{}", c - wxdragon::keycode::WXK_F1 + 1),
        wxdragon::keycode::WXK_SPACE => "Space".into(), wxdragon::keycode::WXK_HOME => "Home".into(), wxdragon::keycode::WXK_END => "End".into(),
        wxdragon::keycode::WXK_PAGEUP => "PgUp".into(), wxdragon::keycode::WXK_PAGEDOWN => "PgDn".into(),
        wxdragon::keycode::WXK_INSERT => "Ins".into(), wxdragon::keycode::WXK_DELETE => "Del".into(),
        wxdragon::keycode::WXK_UP => "Up".into(), wxdragon::keycode::WXK_DOWN => "Down".into(),
        wxdragon::keycode::WXK_LEFT => "Left".into(), wxdragon::keycode::WXK_RIGHT => "Right".into(),
        c if (33..=126).contains(&c) => (c as u8 as char).to_string(),
        _ => return None,
    };
    let mut parts = Vec::new();
    if key.control_down() { parts.push("Ctrl"); } if key.alt_down() { parts.push("Alt"); } if key.shift_down() { parts.push("Shift"); }
    if parts.is_empty() && !name.starts_with('F') { return None; }
    Some(format!("{}{}{}", parts.join("+"), if parts.is_empty() { "" } else { "+" }, name))
}

fn shortcut_dialog(parent: Dialog, style: &mut Style, sheet: &StyleSheet, pending: &Pending) -> bool {
    let ui = Rc::new(crate::i18n::localized(ShortcutDialogUi::new(Some(&parent), false)));
    let chrome = Chrome::new(ui.key_dialog, ui.key_card, &[], &[], (ui.key_ok, "Close"), None,
        &[(ui.key_assign, "Assign"), (ui.key_remove, "Remove"), (ui.key_reset, "Reset All…")], &[]);
    {
        let mut draft = pending.borrow_mut();
        if !draft.values.contains_key("key_scope_changed") {
            for owner in sheet.styles.values() {
                for (key, value) in &owner.formatting {
                    if key.starts_with("key_keys:") { draft.values.entry(key.clone()).or_insert_with(|| value.clone()); }
                }
            }
        }
    }
    ui.key_categories.set_selection(0, true);
    let mut names = sheet.styles.values().map(|s| s.name.clone()).collect::<Vec<_>>();
    if !names.contains(&style.name) { names.push(style.name.clone()); }
    names.sort_by_key(|n| n.to_lowercase());
    for name in &names { ui.key_commands.append(name); }
    if let Some(i) = names.iter().position(|n| *n == style.name) { ui.key_commands.set_selection(i as u32, true); ui.key_commands.ensure_visible(i as i32); }
    let key_of = |name: &str| format!("key_keys:{name}");
    let command = { let ui = Rc::clone(&ui); move || ui.key_commands.get_string_selection().unwrap_or_default() };
    let command = Rc::new(command);
    let show = { let ui = Rc::clone(&ui); let pending = Rc::clone(pending); let command = Rc::clone(&command); move || {
        let name = command();
        ui.key_current.clear();
        for key in pending.borrow().values.get(&key_of(&name)).map(|v| v.lines().filter(|l| !l.is_empty()).map(str::to_owned).collect::<Vec<_>>()).unwrap_or_default() {
            ui.key_current.append(&key);
        }
        ui.key_description.set_label(&crate::i18n::text(&(&format!("Applies the “{name}” style."))));
    } };
    let show = Rc::new(show);
    show();
    // Which style (if any) already uses the typed shortcut.
    let assigned_to = { let pending = Rc::clone(pending); move |shortcut: &str| -> Option<String> {
        pending.borrow().values.iter().find(|(k, v)| k.starts_with("key_keys:") && v.lines().any(|l| l == shortcut)).map(|(k, _)| k["key_keys:".len()..].to_owned())
    } };
    let assigned_to = Rc::new(assigned_to);
    { let show = Rc::clone(&show); ui.key_commands.on_selection_changed(move |_| show()); }
    { let ui2 = Rc::clone(&ui); let assigned_to = Rc::clone(&assigned_to); let field = ui.key_new;
      field.on_key_down(move |event| {
        if let WindowEventData::Keyboard(key) = &event {
            if key.get_key_code() == Some(wxdragon::keycode::WXK_TAB) { event.skip(true); return; }
            if key.get_key_code() == Some(wxdragon::keycode::WXK_BACK) { ui2.key_new.change_value(""); ui2.key_assigned.set_label(&crate::i18n::text(&("[unassigned]"))); return; }
            if let Some(text) = shortcut_text(key) {
                ui2.key_new.change_value(&text);
                ui2.key_assigned.set_label(&crate::i18n::text(&(&assigned_to(&text).unwrap_or_else(|| "[unassigned]".into()))));
            }
            return;
        }
        event.skip(true);
      }); }
    { let ui2 = Rc::clone(&ui); let pending = Rc::clone(pending); let command = Rc::clone(&command); let show = Rc::clone(&show);
      ui.key_assign.on_click(move |_| {
        let shortcut = ui2.key_new.get_value(); if shortcut.trim().is_empty() { return; }
        let mut p = pending.borrow_mut();
        // A shortcut belongs to one command: take it away from any other style.
        let keys = p.values.keys().filter(|k| k.starts_with("key_keys:")).cloned().collect::<Vec<_>>();
        for k in keys { if let Some(v) = p.values.get_mut(&k) { *v = v.lines().filter(|l| *l != shortcut).collect::<Vec<_>>().join("\n"); } }
        let entry = p.values.entry(key_of(&command())).or_default();
        if !entry.is_empty() { entry.push('\n'); } entry.push_str(&shortcut);
        drop(p); ui2.key_new.change_value(""); ui2.key_assigned.set_label(&crate::i18n::text(&("[unassigned]"))); show();
      }); }
    { let ui2 = Rc::clone(&ui); let pending = Rc::clone(pending); let command = Rc::clone(&command); let show = Rc::clone(&show);
      ui.key_remove.on_click(move |_| {
        let Some(selected) = ui2.key_current.get_string_selection() else { return };
        if let Some(v) = pending.borrow_mut().values.get_mut(&key_of(&command())) { *v = v.lines().filter(|l| *l != selected).collect::<Vec<_>>().join("\n"); }
        show();
      }); }
    { let pending = Rc::clone(pending); let show = Rc::clone(&show);
      ui.key_reset.on_click(move |_| { pending.borrow_mut().values.retain(|k, _| !k.starts_with("key_keys:")); show(); }); }
    let ui_only = [("key_save_in", Ctl::Combo(ui.key_save_in))];
    let ui_before=restore(pending, &ui_only);
    let _ = chrome.show();
    remember_changed(pending, &ui_only, &ui_before);
    pending.borrow_mut().values.insert("key_scope_changed".into(), "true".into());
    ui.key_dialog.destroy();
    // Customize Keyboard has only Close; its assignments commit with the style.
    true
}

// Format Text Effects --------------------------------------------------------------------------

fn text_effects_dialog(parent: Dialog, pending: &Pending) -> bool {
    let ui = Rc::new(crate::i18n::localized(TextEffectsDialogUi::new(Some(&parent), false)));
    let chrome = Chrome::new(ui.fx_dialog, ui.fx_card, &[(ui.fx_tab_0, "Text Fill & Outline"), (ui.fx_tab_1, "Text Effects")],
        &[ui.fx_page_0, ui.fx_page_1], (ui.fx_ok, "OK"), Some(ui.fx_cancel), &[], &[]);
    for combo in [ui.fx_fill_color, ui.fx_line_color, ui.fx_shadow_color, ui.fx_glow_color] { fill_colour_list(combo, &ui.fx_dialog, None, AUTOMATIC); }
    ui.fx_fill_solid.set_value(true); ui.fx_line_none.set_value(true);
    let ui_only = [("fx_fill_none", Ctl::Radio(ui.fx_fill_none)), ("fx_fill_solid", Ctl::Radio(ui.fx_fill_solid)), ("fx_fill_gradient", Ctl::Radio(ui.fx_fill_gradient)),
        ("fx_fill_color", Ctl::Colour(ui.fx_fill_color, ui.fx_dialog, false)), ("fx_fill_transparency", Ctl::Spin(ui.fx_fill_transparency)),
        ("fx_line_none", Ctl::Radio(ui.fx_line_none)), ("fx_line_solid", Ctl::Radio(ui.fx_line_solid)), ("fx_line_gradient", Ctl::Radio(ui.fx_line_gradient)),
        ("fx_line_color", Ctl::Colour(ui.fx_line_color, ui.fx_dialog, false)), ("fx_line_transparency", Ctl::Spin(ui.fx_line_transparency)),
        ("fx_line_width", Ctl::Spin(ui.fx_line_width)), ("fx_line_dash", Ctl::Combo(ui.fx_line_dash)), ("fx_line_cap", Ctl::Combo(ui.fx_line_cap)),
        ("fx_line_join", Ctl::Combo(ui.fx_line_join)), ("fx_shadow", Ctl::Combo(ui.fx_shadow)), ("fx_shadow_color", Ctl::Colour(ui.fx_shadow_color, ui.fx_dialog, false)),
        ("fx_shadow_transparency", Ctl::Spin(ui.fx_shadow_transparency)), ("fx_shadow_blur", Ctl::Spin(ui.fx_shadow_blur)),
        ("fx_shadow_angle", Ctl::Spin(ui.fx_shadow_angle)), ("fx_shadow_distance", Ctl::Spin(ui.fx_shadow_distance)),
        ("fx_reflection", Ctl::Combo(ui.fx_reflection)), ("fx_reflection_size", Ctl::Spin(ui.fx_reflection_size)),
        ("fx_reflection_transparency", Ctl::Spin(ui.fx_reflection_transparency)), ("fx_reflection_distance", Ctl::Spin(ui.fx_reflection_distance)),
        ("fx_glow", Ctl::Combo(ui.fx_glow)), ("fx_glow_color", Ctl::Colour(ui.fx_glow_color, ui.fx_dialog, false)), ("fx_glow_size", Ctl::Spin(ui.fx_glow_size)),
        ("fx_glow_transparency", Ctl::Spin(ui.fx_glow_transparency)), ("fx_bevel", Ctl::Combo(ui.fx_bevel)), ("fx_material", Ctl::Combo(ui.fx_material)),
        ("fx_depth", Ctl::Spin(ui.fx_depth)), ("fx_lighting", Ctl::Combo(ui.fx_lighting))];
    let ui_before=restore(pending, &ui_only);
    // Preview: "AaBbCc" with fill, outline, glow and shadow approximated in the DC.
    for panel in [ui.fx_preview, ui.fx_preview_effects] {
        let ui = Rc::clone(&ui);
        painted(panel, move |dc, panel| {
            paint_rounded_surface(dc, panel, Colour::rgb(255, 255, 255), rgb(LINE_STRONG), 5);
            let colour = |combo: BitmapComboBox, fallback: Colour| colour_value(&combo.get_value()).and_then(|h| hex_colour(&h)).unwrap_or(fallback);
            let blend = |c: Colour, alpha: f64| { let a = alpha.clamp(0.0, 1.0); let mix = |v: u8| (v as f64 * a + 255.0 * (1.0 - a)).round() as u8; Colour::rgb(mix(c.r), mix(c.g), mix(c.b)) };
            let size = panel.get_client_size();
            let points = ((size.height - dip(panel, 6)).max(1) as f32 / (1.3 * px_per_pt(panel))).clamp(1.0, 26.0);
            let Some(font) = face_font(points, true, false, false, "Linux Libertine G") else { return };
            dc.set_font(&font);
            let text = "AaBbCc"; let (tw, th) = dc.get_text_extent(&crate::i18n::text(&(text)));
            let (x, y) = ((size.width - tw) / 2, (size.height - th) / 2);
            let px = px_per_pt(panel);
            if crate::i18n::choice_source(&ui.fx_glow.get_value(), &["No glow", "5 pt glow", "8 pt glow", "11 pt glow", "18 pt glow"]) != "No glow" {
                let radius = ((ui.fx_glow_size.get_value() as f32).max(2.0) * px * 0.5).round() as i32;
                dc.set_text_foreground(blend(colour(ui.fx_glow_color, rgb(ACCENT)), 1.0 - ui.fx_glow_transparency.get_value() / 100.0));
                for dx in (-radius..=radius).step_by(((radius / 3).max(1)) as usize) { for dy in (-radius..=radius).step_by(((radius / 3).max(1)) as usize) { dc.draw_text(&crate::i18n::text(&(text)), x + dx, y + dy); } }
            }
            if crate::i18n::choice_source(&ui.fx_shadow.get_value(), &["No shadow", "Outer: offset bottom", "Outer: offset right", "Outer: centered", "Inner: centered", "Perspective: below"]) != "No shadow" {
                let angle = (ui.fx_shadow_angle.get_value() as f32).to_radians(); let distance = ui.fx_shadow_distance.get_value() as f32 * px;
                dc.set_text_foreground(blend(colour(ui.fx_shadow_color, Colour::rgb(0, 0, 0)), 1.0 - ui.fx_shadow_transparency.get_value() / 100.0));
                dc.draw_text(&crate::i18n::text(&(text)), x + (angle.cos() * distance).round() as i32, y + (angle.sin() * distance).round() as i32);
            }
            if !ui.fx_line_none.get_value() {
                let width = ((ui.fx_line_width.get_value() as f32 * px).round() as i32).max(1);
                dc.set_text_foreground(blend(colour(ui.fx_line_color, rgb(INK)), 1.0 - ui.fx_line_transparency.get_value() / 100.0));
                for (dx, dy) in [(-width, 0), (width, 0), (0, -width), (0, width), (-width, -width), (width, width), (-width, width), (width, -width)] { dc.draw_text(&crate::i18n::text(&(text)), x + dx, y + dy); }
            }
            let fill = if ui.fx_fill_none.get_value() { Colour::rgb(255, 255, 255) } else { blend(colour(ui.fx_fill_color, rgb(INK)), 1.0 - ui.fx_fill_transparency.get_value() / 100.0) };
            dc.set_text_foreground(fill); dc.draw_text(&crate::i18n::text(&(text)), x, y);
            if ui.fx_fill_gradient.get_value() {
                // Gradient: the lower half in a lighter tint of the fill.
                dc.set_clipping_region(x, y + th / 2, tw, th - th / 2);
                dc.set_text_foreground(blend(fill, 0.55)); dc.draw_text(&crate::i18n::text(&(text)), x, y); dc.destroy_clipping_region();
            }
        });
    }
    let refresh: Rc<dyn Fn()> = { let ui = Rc::clone(&ui); Rc::new(move || { ui.fx_preview.refresh(false, None); ui.fx_preview_effects.refresh(false, None); }) };
    for radio in [ui.fx_fill_none, ui.fx_fill_solid, ui.fx_fill_gradient, ui.fx_line_none, ui.fx_line_solid, ui.fx_line_gradient] {
        let refresh = Rc::clone(&refresh); radio.on_selected(move |_| refresh());
    }
    for combo in [ui.fx_fill_color, ui.fx_line_color, ui.fx_shadow_color, ui.fx_glow_color] { let refresh = Rc::clone(&refresh); bind_colour_list(combo, ui.fx_dialog, move || refresh()); }
    for combo in [ui.fx_line_dash, ui.fx_line_cap, ui.fx_line_join, ui.fx_shadow, ui.fx_reflection, ui.fx_glow, ui.fx_bevel, ui.fx_material, ui.fx_lighting] {
        let refresh = Rc::clone(&refresh); combo.on_selection_changed(move |_| refresh());
    }
    for spin in [ui.fx_fill_transparency, ui.fx_line_transparency, ui.fx_line_width, ui.fx_shadow_transparency, ui.fx_shadow_blur, ui.fx_shadow_angle,
                 ui.fx_shadow_distance, ui.fx_reflection_size, ui.fx_reflection_transparency, ui.fx_reflection_distance, ui.fx_glow_size,
                 ui.fx_glow_transparency, ui.fx_depth] {
        let refresh = Rc::clone(&refresh); spin.on_value_changed(move |_| refresh());
    }
    let accepted = chrome.show();
    if accepted { remember_changed(pending, &ui_only, &ui_before); }
    ui.fx_dialog.destroy();
    accepted
}
