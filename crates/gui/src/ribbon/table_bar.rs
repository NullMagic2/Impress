//! The Table bar shown above the page while the caret is in a table: one
//! painted surface with grouped icon commands (select, insert, delete,
//! merge, cell size, alignment), drawn the same on every platform.
use super::*;
use crate::table_ui::{CellFacts, TableOp};
use super::bar_paint::{fill_alpha, fill_round, large_top, line, paint, position, scale_of, stroke_round};
use tiny_skia::{FillRule, PathBuilder, Pixmap};
use wxdragon::{
    dc::AutoBufferedPaintDC,
    event::window_events::WindowEvents,
    font::{Font, FontFamily, FontStyle, FontWeight},
    widgets::panel::Panel,
    window::BackgroundStyle,
    DeviceContext,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cmd { Select(u8), Op(TableOp), HeightValue, PositionDialog,
    /// Make the table free with text around it (true) or above and below.
    FreeWrap(bool),
    /// A button that opens a menu.
    Menu(MenuKind),
    /// A caption beside a stepper; does nothing.
    Label,
    /// Close the Table tools and return to the Insert ribbon.
    Close }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MenuKind { Select, Position, Delete, InsertCells, Merge }

const MENU_FIRST: i32 = 32100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Icon {
    SelCell, SelRow, SelCol, SelTable,
    InsAbove, InsBelow, InsLeft, InsRight, CellsRight, CellsDown,
    DelRow, DelCol, DelTable, ShiftLeft, ShiftUp,
    MergeRight, MergeDown, Split, Distribute,
    Width, Height, AlignTop, AlignCenter, AlignBottom, Header, Minus, Plus,
    Inline, Free, WrapAround, WrapTopBottom, Position, Close,
}

#[derive(Clone, Copy, PartialEq)]
enum Style { Large, Small, IconOnly, Value, Caption }

#[derive(Clone)]
struct Button { cmd: Cmd, icon: Option<Icon>, label: String, tip: &'static str, style: Style, rect: (i32, i32, i32, i32) }

struct Layout { buttons: Vec<Button>, groups: Vec<(i32, i32, &'static str)> }

type Item = (Cmd, Icon, &'static str, &'static str);

pub(super) struct TableBar {
    panel: Panel,
    ribbon: std::rc::Weak<Ribbon>,
    hover: Cell<Option<Cmd>>,
    pressed: Cell<Option<Cmd>>,
    /// The menu last opened, for its selection event.
    menu: Cell<Option<MenuKind>>,
    font: Option<Font>,
    shown: RefCell<Option<Option<CellFacts>>>,
}

pub(super) fn install(ribbon: &Rc<Ribbon>) -> Rc<TableBar> {
    let panel = ribbon.ui.table_context_tools;
    let font = Font::new_with_details(9, FontFamily::Default as i32, FontStyle::Normal as i32, FontWeight::Normal as i32, false, "Segoe UI");
    if let Some(font) = &font { panel.set_font(font); }
    let bar = Rc::new(TableBar { panel, ribbon: Rc::downgrade(ribbon), hover: Cell::new(None), pressed: Cell::new(None), menu: Cell::new(None), font, shown: RefCell::new(None) });
    let b = Rc::downgrade(&bar);
    panel.on_menu_selected(move |event| {
        let Some(b) = b.upgrade() else { return; };
        let Some(kind) = b.menu.take() else { event.skip(true); return; };
        let index = (event.get_id() - MENU_FIRST) as usize;
        if let Some((_, Some(cmd))) = TableBar::menu_items(kind).get(index).copied() { b.activate(cmd); }
    });
    panel.set_background_style(BackgroundStyle::Paint);
    panel.on_erase_background(|_| {});
    let b = Rc::downgrade(&bar);
    panel.on_paint(move |_| { if let Some(b) = b.upgrade() { b.paint(); } });
    panel.on_size(move |event| { panel.refresh(false, None); event.skip(true); });
    let b = Rc::downgrade(&bar);
    panel.on_mouse_motion(move |event| {
        let (Some(b), Some((x, y))) = (b.upgrade(), position(&event)) else { return; };
        let hit = b.hit(x, y);
        if b.hover.get() != hit.as_ref().map(|h| h.cmd) {
            b.hover.set(hit.as_ref().map(|h| h.cmd));
            panel.set_tooltip(&crate::i18n::text(&(hit.as_ref().map_or("", |h| h.tip))));
            panel.refresh(false, None);
        }
    });
    let b = Rc::downgrade(&bar);
    panel.on_mouse_leave(move |_| { if let Some(b) = b.upgrade() { b.pressed.set(None); if b.hover.replace(None).is_some() { panel.refresh(false, None); } } });
    let b = Rc::downgrade(&bar);
    panel.on_mouse_left_down(move |event| {
        let (Some(b), Some((x, y))) = (b.upgrade(), position(&event)) else { return; };
        b.pressed.set(b.hit(x, y).filter(|h| b.enabled(h.cmd)).map(|h| h.cmd));
        panel.refresh(false, None);
    });
    let b = Rc::downgrade(&bar);
    panel.on_mouse_left_up(move |event| {
        let (Some(b), Some((x, y))) = (b.upgrade(), position(&event)) else { return; };
        let pressed = b.pressed.take();
        panel.refresh(false, None);
        let hit = b.hit(x, y).map(|h| h.cmd);
        if let (Some(cmd), true) = (pressed, pressed == hit) { b.activate(cmd); }
    });
    bar
}

const GROUP_PAD: i32 = 6;
const CAPTION: i32 = 17;
const SMALL_H: i32 = 22;

impl TableBar {
    /// Repaints only when the current cell's facts changed (or the bar was hidden).
    pub(super) fn refresh(&self) {
        if !self.panel.is_shown() { self.shown.replace(None); return; }
        let facts = self.facts();
        if *self.shown.borrow() != Some(facts.clone()) { self.shown.replace(Some(facts)); self.panel.refresh(false, None); }
    }

    fn facts(&self) -> Option<CellFacts> {
        let ribbon = self.ribbon.upgrade()?;
        let state = ribbon.state.try_borrow().ok()?;
        crate::table_ui::facts(&state)
    }

    fn enabled(&self, cmd: Cmd) -> bool {
        if cmd == Cmd::Close { return true; }
        let Some(facts) = self.facts() else { return false; };
        match cmd {
            Cmd::Op(TableOp::Split) => facts.merged,
            Cmd::Op(TableOp::HeightAuto) | Cmd::HeightValue => facts.height_mm.is_some(),
            Cmd::Op(TableOp::Float(_)) | Cmd::PositionDialog => facts.movable,
            Cmd::Op(TableOp::WrapAround(_)) => facts.position.is_some(),
            Cmd::FreeWrap(_) => facts.movable,
            Cmd::Label => false,
            Cmd::Close => true,
            _ => true,
        }
    }

    fn activate(&self, cmd: Cmd) {
        let Some(ribbon) = self.ribbon.upgrade() else { return; };
        match cmd {
            Cmd::Select(mode) => ribbon.run(Action::SelectCells(mode)),
            Cmd::Op(op) => ribbon.run(Action::Table(op)),
            Cmd::HeightValue => ribbon.run(Action::Table(TableOp::HeightAuto)),
            Cmd::PositionDialog => ribbon.run(Action::TablePosition),
            Cmd::Menu(kind) => self.show_menu(kind),
            Cmd::FreeWrap(around) => {
                if self.facts().is_some_and(|f| f.position.is_none()) { ribbon.run(Action::Table(TableOp::Float(true))); }
                ribbon.run(Action::Table(TableOp::WrapAround(around)));
            }
            Cmd::Label => {}
            Cmd::Close => ribbon.run(Action::CloseTableTools),
        }
        self.panel.refresh(false, None);
    }

    fn hit(&self, x: f32, y: f32) -> Option<Button> {
        let layout = self.layout(&self.facts());
        layout.buttons.into_iter().find(|b| {
            let (bx, by, bw, bh) = b.rect;
            x >= bx as f32 && x < (bx + bw) as f32 && y >= by as f32 && y < (by + bh) as f32
        })
    }

    fn font(&self) -> Option<&Font> { self.font.as_ref() }

    /// Measured with the panel's font, which is set once at install:
    /// wxDragon's `set_font` leaks a copy of the font on every call, and on
    /// Windows repeated calls exhaust GDI handles (the Ribbon then paints
    /// blank grey buttons).
    fn text_width(&self, text: &str) -> i32 {
        text.lines().map(|l| self.panel.get_text_extent(&crate::i18n::text(&(l))).width).max().unwrap_or(0)
    }

    fn layout(&self, facts: &Option<CellFacts>) -> Layout {
        let full = self.layout_with(facts, false);
        let width = self.panel.get_client_size().width;
        if full.groups.last().is_some_and(|g| g.1 > width) { self.layout_with(facts, true) } else { full }
    }

    fn layout_with(&self, facts: &Option<CellFacts>, compact: bool) -> Layout {
        let d = |v: i32| self.panel.from_dip_int(v);
        let h = self.panel.get_client_size().height;
        let body = h - d(CAPTION);
        let mut buttons = Vec::new();
        let mut groups = Vec::new();
        let mut x = d(4);
        let small_style = if compact { Style::IconOnly } else { Style::Small };
        // Large buttons, as in the Ribbon: icon above a caption; a menu
        // button shows ▾ under its caption.
        let large = |buttons: &mut Vec<Button>, x: &mut i32, cmd: Cmd, icon: Icon, label: &str, tip: &'static str| {
            let w = self.text_width(label).max(d(34)) + d(14);
            buttons.push(Button { cmd, icon: Some(icon), label: label.into(), tip, style: Style::Large, rect: (*x, d(3), w, body - d(5)) });
            *x += w + d(1);
        };
        let group = |groups: &mut Vec<(i32, i32, &'static str)>, x: &mut i32, start: i32, caption: &'static str| {
            let end = (*x + d(GROUP_PAD)).max(start + self.text_width(caption) + d(16));
            groups.push((start, end, caption));
            *x = end + d(1 + GROUP_PAD);
        };
        let position_icon = match facts.as_ref().and_then(|f| f.position.as_ref()) {
            None => Icon::Inline,
            Some(p) if p.wrap == model::TableWrap::TopAndBottom => Icon::WrapTopBottom,
            Some(_) => Icon::WrapAround,
        };

        let start = x; x += d(GROUP_PAD);
        large(&mut buttons, &mut x, Cmd::Menu(MenuKind::Select), Icon::SelTable, "Select", "Select the cell, row, column or whole table");
        large(&mut buttons, &mut x, Cmd::Menu(MenuKind::Position), position_icon, "Position", "Keep the table in line with the text, or let it float with text wrapping around it");
        group(&mut groups, &mut x, start, "Table");

        let start = x; x += d(GROUP_PAD);
        large(&mut buttons, &mut x, Cmd::Menu(MenuKind::Delete), Icon::DelTable, "Delete", "Delete cells, rows, columns or the table");
        large(&mut buttons, &mut x, Cmd::Op(TableOp::InsertRowAbove), Icon::InsAbove, "Insert\nAbove", "Insert a row above");
        large(&mut buttons, &mut x, Cmd::Op(TableOp::InsertRowBelow), Icon::InsBelow, "Insert\nBelow", "Insert a row below");
        large(&mut buttons, &mut x, Cmd::Op(TableOp::InsertColumnLeft), Icon::InsLeft, "Insert\nLeft", "Insert a column to the left");
        large(&mut buttons, &mut x, Cmd::Op(TableOp::InsertColumnRight), Icon::InsRight, "Insert\nRight", "Insert a column to the right");
        large(&mut buttons, &mut x, Cmd::Menu(MenuKind::InsertCells), Icon::CellsRight, "Insert\nCells", "Insert a cell and shift the others right or down");
        group(&mut groups, &mut x, start, "Rows & Columns");

        let start = x; x += d(GROUP_PAD);
        large(&mut buttons, &mut x, Cmd::Menu(MenuKind::Merge), Icon::MergeRight, "Merge", "Merge with the cell to the right or below");
        large(&mut buttons, &mut x, Cmd::Op(TableOp::Split), Icon::Split, "Split", "Split a merged cell");
        group(&mut groups, &mut x, start, "Merge");

        // Cell size: labelled steppers.
        let start = x; x += d(GROUP_PAD);
        let width_text = facts.as_ref().map_or("—".into(), |f| format!("{:.1} mm", f.width_mm));
        let height_text = facts.as_ref().map_or("—".into(), |f| match f.height_mm { Some(v) => format!("{v:.1} mm"), None => "Auto".into() });
        let value_w = self.text_width("000.0 mm").max(self.text_width("Auto")) + d(10);
        let label_w = self.text_width("Height").max(self.text_width("Width")) + d(8);
        let top = (body - 3 * d(SMALL_H)) / 2;
        for (row, label, text, minus, plus, tips, value_cmd, value_tip) in [
            (0, "Width", width_text, TableOp::Width(-1), TableOp::Width(1), ("Narrower (1 mm)", "Wider (1 mm)"), Cmd::Label, "Column width"),
            (1, "Height", height_text, TableOp::Height(-1), TableOp::Height(1), ("Lower (1 mm)", "Taller (1 mm)"), Cmd::HeightValue, "Minimum row height · click to return to Auto")] {
            let y = top + row * d(SMALL_H);
            buttons.push(Button { cmd: Cmd::Label, icon: None, label: label.into(), tip: "", style: Style::Caption, rect: (x, y, label_w, d(SMALL_H)) });
            let vx = x + label_w;
            buttons.push(Button { cmd: Cmd::Op(minus), icon: Some(Icon::Minus), label: String::new(), tip: tips.0, style: Style::IconOnly, rect: (vx, y, d(20), d(SMALL_H)) });
            buttons.push(Button { cmd: value_cmd, icon: None, label: text, tip: value_tip, style: Style::Value, rect: (vx + d(21), y, value_w, d(SMALL_H)) });
            buttons.push(Button { cmd: Cmd::Op(plus), icon: Some(Icon::Plus), label: String::new(), tip: tips.1, style: Style::IconOnly, rect: (vx + d(22) + value_w, y, d(20), d(SMALL_H)) });
        }
        let dist_label = "Distribute columns";
        let dist_w = if compact { d(26) } else { self.text_width(dist_label) + d(34) };
        buttons.push(Button { cmd: Cmd::Op(TableOp::Distribute), icon: Some(Icon::Distribute), label: dist_label.into(), tip: "Give every column the same width",
            style: small_style, rect: (x, top + 2 * d(SMALL_H), dist_w, d(SMALL_H)) });
        x += (label_w + d(42) + value_w).max(dist_w) + d(2);
        group(&mut groups, &mut x, start, "Cell Size");

        // Alignment and the header row.
        let start = x; x += d(GROUP_PAD);
        let top = (body - 2 * d(SMALL_H) - d(4)) / 2;
        for (i, (icon, align, tip)) in [(Icon::AlignTop, 0, "Align text to the top of the cell"), (Icon::AlignCenter, 1, "Centre text vertically"),
            (Icon::AlignBottom, 2, "Align text to the bottom of the cell")].into_iter().enumerate() {
            buttons.push(Button { cmd: Cmd::Op(TableOp::Align(align)), icon: Some(icon), label: String::new(), tip, style: Style::IconOnly,
                rect: (x + i as i32 * d(28), top, d(26), d(SMALL_H) + d(2)) });
        }
        let header_label = "Repeat header";
        let header_w = if compact { d(26) } else { self.text_width(header_label) + d(34) };
        buttons.push(Button { cmd: Cmd::Op(TableOp::HeaderRow), icon: Some(Icon::Header), label: header_label.into(),
            tip: "Repeat this row at the top of every page", style: small_style, rect: (x, top + d(SMALL_H) + d(6), header_w, d(SMALL_H)) });
        x += header_w.max(3 * d(28));
        group(&mut groups, &mut x, start, "Alignment");

        let start = x; x += d(GROUP_PAD);
        large(&mut buttons, &mut x, Cmd::Close, Icon::Close, "Close\nTable Tools", "Close the Table tools and return to the Insert ribbon");
        group(&mut groups, &mut x, start, "");
        Layout { buttons, groups }
    }

    /// The items of a menu button: label and command (None: separator).
    fn menu_items(kind: MenuKind) -> Vec<(&'static str, Option<Cmd>)> {
        match kind {
            MenuKind::Select => vec![("Select Cell", Some(Cmd::Select(0))), ("Select Row", Some(Cmd::Select(1))),
                ("Select Column", Some(Cmd::Select(2))), ("Select Table", Some(Cmd::Select(3)))],
            MenuKind::Position => vec![("In Line with Text", Some(Cmd::Op(TableOp::Float(false)))),
                ("Free, Text Around", Some(Cmd::FreeWrap(true))), ("Free, Text Above and Below", Some(Cmd::FreeWrap(false))),
                ("", None), ("More Position Options…", Some(Cmd::PositionDialog))],
            MenuKind::Delete => vec![("Delete Cell, Shift Left", Some(Cmd::Op(TableOp::DeleteCellsLeft))), ("Delete Cell, Shift Up", Some(Cmd::Op(TableOp::DeleteCellsUp))),
                ("", None), ("Delete Rows", Some(Cmd::Op(TableOp::DeleteRow))), ("Delete Columns", Some(Cmd::Op(TableOp::DeleteColumn))),
                ("Delete Table", Some(Cmd::Op(TableOp::DeleteTable)))],
            MenuKind::InsertCells => vec![("Insert Cell, Shift Right", Some(Cmd::Op(TableOp::InsertCellsRight))), ("Insert Cell, Shift Down", Some(Cmd::Op(TableOp::InsertCellsDown)))],
            MenuKind::Merge => vec![("Merge with Cell to the Right", Some(Cmd::Op(TableOp::MergeRight))), ("Merge with Cell Below", Some(Cmd::Op(TableOp::MergeDown)))],
        }
    }

    fn show_menu(&self, kind: MenuKind) {
        let facts = self.facts();
        let items = Self::menu_items(kind);
        let checks = matches!(kind, MenuKind::Position);
        let mut builder = Menu::builder();
        for (i, (label, cmd)) in items.iter().enumerate() {
            builder = match cmd {
                None => builder.append_separator(),
                Some(_) if checks && i < 3 => builder.append_check_item(MENU_FIRST + i as i32, &crate::i18n::text(&(label)), &crate::i18n::text(&(""))),
                Some(_) => builder.append_item(MENU_FIRST + i as i32, &crate::i18n::text(&(label)), &crate::i18n::text(&(""))),
            };
        }
        let mut menu = builder.build();
        for (i, (_, cmd)) in items.iter().enumerate() {
            let Some(cmd) = cmd else { continue; };
            menu.enable_item(MENU_FIRST + i as i32, self.enabled(*cmd));
            if checks && i < 3 { menu.check_item(MENU_FIRST + i as i32, self.toggled(*cmd, &facts)); }
        }
        self.panel.set_tooltip(&crate::i18n::text(&("")));
        self.menu.set(Some(kind));
        let layout = self.layout(&facts);
        let Some(button) = layout.buttons.iter().find(|b| b.cmd == Cmd::Menu(kind)) else { return; };
        let anchor = crate::gtk_native::to_screen(&self.panel,wxdragon::geometry::Point::new(button.rect.0, button.rect.1 + button.rect.3));
        self.panel.popup_menu(&mut menu, Some(anchor));
    }

    fn toggled(&self, cmd: Cmd, facts: &Option<CellFacts>) -> bool {
        let Some(f) = facts else { return false; };
        match cmd {
            Cmd::Op(TableOp::Align(a)) => a == f.align, Cmd::Op(TableOp::HeaderRow) => f.header,
            Cmd::Op(TableOp::Float(free)) => free == f.position.is_some(),
            Cmd::Op(TableOp::WrapAround(around)) | Cmd::FreeWrap(around) => f.position.as_ref().is_some_and(|p| (p.wrap == model::TableWrap::Around) == around),
            _ => false,
        }
    }

    fn paint(&self) {
        let dc = AutoBufferedPaintDC::new(&self.panel);
        let size = self.panel.get_client_size();
        let (w, h) = (size.width.max(1), size.height.max(1));
        let scale = self.panel.from_dip_int(1000) as f32 / 1000.0;
        let px = scale.max(1.0).round();
        let facts = self.facts();
        let layout = self.layout(&facts);
        let Some((mut pixmap, canvas)) = crate::crisp::Canvas::new(&self.panel, w as i32, h as i32) else { return; };
        pixmap.fill(tiny_skia::Color::WHITE);
        let caption_top = (h - self.panel.from_dip_int(CAPTION)) as f32;
        // Caption band and group separators, as in the ribbon.
        fill_round(&mut pixmap, (0.0, caption_top, w as f32, h as f32 - caption_top), 0.0, (243, 246, 251));
        line(&mut pixmap, &[(0.0, h as f32 - 0.5 * px), (w as f32, h as f32 - 0.5 * px)], (214, 221, 232), px);
        for &(_, end, _) in &layout.groups {
            let x = end as f32 + 0.5 * px;
            line(&mut pixmap, &[(x, 6.0 * scale), (x, h as f32 - 4.0 * scale)], (222, 228, 237), px);
        }
        for b in &layout.buttons {
            let enabled = self.enabled(b.cmd);
            let (x, y, bw, bh) = (b.rect.0 as f32, b.rect.1 as f32, b.rect.2 as f32, b.rect.3 as f32);
            let hot = enabled && self.hover.get() == Some(b.cmd) && b.style != Style::Value || (b.style == Style::Value && b.cmd == Cmd::HeightValue && enabled && self.hover.get() == Some(b.cmd));
            let pressed = enabled && self.pressed.get() == Some(b.cmd);
            let on = self.toggled(b.cmd, &facts);
            if b.style == Style::Caption { continue; }
            if b.style == Style::Value {
                fill_round(&mut pixmap, (x + 0.5 * px, y + 2.0 * scale, bw - px, bh - 4.0 * scale), 3.0 * scale, if hot { (232, 239, 253) } else { (247, 249, 252) });
                stroke_round(&mut pixmap, (x + 0.5 * px, y + 2.0 * scale, bw - px, bh - 4.0 * scale), 3.0 * scale, (214, 221, 232), px);
            } else if pressed || hot || on {
                let fill = if pressed { (206, 222, 250) } else if on { (221, 233, 252) } else { (234, 241, 253) };
                fill_round(&mut pixmap, (x + px, y + px, bw - 2.0 * px, bh - 2.0 * px), 4.0 * scale, fill);
                if on { stroke_round(&mut pixmap, (x + 1.5 * px, y + 1.5 * px, bw - 3.0 * px, bh - 3.0 * px), 4.0 * scale, (120, 160, 230), px); }
            }
            let Some(icon) = b.icon else { continue; };
            let (size, ix, iy) = match b.style {
                Style::Large => (32.0 * scale, x + (bw - 32.0 * scale) * 0.5, y + large_top(bh, scale)),
                Style::Caption => continue,
                Style::Value => (16.0 * scale, x - 38.0 * scale, y + (bh - 16.0 * scale) * 0.5),
                Style::IconOnly => (16.0 * scale, x + (bw - 16.0 * scale) * 0.5, y + (bh - 16.0 * scale) * 0.5),
                Style::Small => (16.0 * scale, x + 5.0 * scale, y + (bh - 16.0 * scale) * 0.5),
            };
            draw_icon(&mut pixmap, icon, ix, iy, size, enabled || b.style == Style::Value);
        }
        canvas.present(&dc, &pixmap, 0, 0);

        dc.set_background_mode(wxdragon::dc::BackgroundMode::Transparent);
        if let Some(font) = self.font() { dc.set_font(font); }
        let d = |v: i32| self.panel.from_dip_int(v);
        for b in &layout.buttons {
            if b.label.is_empty() || b.style == Style::IconOnly { continue; }
            let enabled = self.enabled(b.cmd) || matches!(b.style, Style::Value);
            dc.set_text_foreground(if enabled { Colour::rgb(36, 48, 66) } else { Colour::rgb(172, 180, 192) });
            let (x, y, bw, bh) = b.rect;
            match b.style {
                Style::Large => {
                    let mut lines: Vec<String> = b.label.lines().map(str::to_string).collect();
                    // Menu buttons end their caption with ▾, as in the Ribbon.
                    if matches!(b.cmd, Cmd::Menu(_)) { if let Some(last) = lines.last_mut() { last.push_str(" ▾"); } }
                    let lh = dc.get_text_extent(&crate::i18n::text(&("Ag"))).1 - d(1);
                    let mut ty = y + large_top(bh as f32, scale_of(&self.panel)) as i32 + d(38);
                    for l in lines { let lw = dc.get_text_extent(&crate::i18n::text(&(&l))).0; dc.draw_text(&crate::i18n::text(&(&l)), x + (bw - lw) / 2, ty); ty += lh; }
                }
                Style::Caption => {
                    dc.set_text_foreground(Colour::rgb(92, 108, 132));
                    let lh = dc.get_text_extent(&crate::i18n::text(&(&b.label))).1;
                    dc.draw_text(&crate::i18n::text(&(&b.label)), x, y + (bh - lh) / 2);
                }
                Style::Value => {
                    let (lw, lh) = dc.get_text_extent(&crate::i18n::text(&(&b.label)));
                    dc.draw_text(&crate::i18n::text(&(&b.label)), x + (bw - lw) / 2, y + (bh - lh) / 2);
                }
                _ => {
                    let lh = dc.get_text_extent(&crate::i18n::text(&(&b.label))).1;
                    dc.draw_text(&crate::i18n::text(&(&b.label)), x + d(26), y + (bh - lh) / 2);
                }
            }
        }
        dc.set_text_foreground(Colour::rgb(92, 108, 132));
        let cap_y = h - d(CAPTION);
        for &(start, end, caption) in &layout.groups {
            let (cw, ch) = dc.get_text_extent(&crate::i18n::text(&(caption)));
            dc.draw_text(&crate::i18n::text(&(caption)), start + (end - start - cw) / 2, cap_y + (d(CAPTION) - ch) / 2);
        }
    }
}

const INK: (u8, u8, u8) = (88, 102, 124);
const BLUE: (u8, u8, u8) = (43, 111, 232);
const RED: (u8, u8, u8) = (206, 52, 40);

/// Icons on a 16-unit grid: a 3 × 3 table with the affected part tinted.
pub(super) fn draw_icon(pix: &mut Pixmap, icon: Icon, x: f32, y: f32, size: f32, enabled: bool) {
    let u = size / 16.0;
    let p = |px: f32, py: f32| (x + px * u, y + py * u);
    let grey = |c: (u8, u8, u8)| if enabled { c } else { let g = ((c.0 as u32 + c.1 as u32 + c.2 as u32) / 3) as u8; (g / 3 + 150, g / 3 + 155, g / 3 + 165) };
    let ink = grey(INK);
    let blue = grey(BLUE);
    let red = grey(RED);
    let w = (1.1 * u).max(1.0);
    let (x0, x1, y0, y1) = (1.5, 14.5, 2.5, 13.5);
    let cx = |i: usize| x0 + (x1 - x0) * i as f32 / 3.0;
    let cy = |i: usize| y0 + (y1 - y0) * i as f32 / 3.0;
    let tint = |pix: &mut Pixmap, c0: usize, r0: usize, c1: usize, r1: usize, color: (u8, u8, u8), alpha: u8| {
        let (ax, ay) = p(cx(c0), cy(r0)); let (bx, by) = p(cx(c1), cy(r1));
        fill_alpha(pix, (ax, ay, bx - ax, by - ay), color, alpha);
    };
    let grid = |pix: &mut Pixmap| {
        let (ax, ay) = p(x0, y0); let (bx, by) = p(x1, y1);
        stroke_round(pix, (ax, ay, bx - ax, by - ay), 1.5 * u, ink, w);
        for i in 1..3 { line(pix, &[p(cx(i), y0), p(cx(i), y1)], ink, w * 0.8); line(pix, &[p(x0, cy(i)), p(x1, cy(i))], ink, w * 0.8); }
    };
    let badge = |pix: &mut Pixmap, bx: f32, by: f32, color: (u8, u8, u8), cross: bool| {
        let (cxp, cyp) = p(bx, by);
        if let Some(c) = PathBuilder::from_circle(cxp, cyp, 3.6 * u) { pix.fill_path(&c, &paint(color), FillRule::Winding, crate::crisp::transform(), None); }
        let d = 1.8 * u;
        if cross {
            line(pix, &[(cxp - d, cyp - d), (cxp + d, cyp + d)], (255, 255, 255), w * 1.1);
            line(pix, &[(cxp - d, cyp + d), (cxp + d, cyp - d)], (255, 255, 255), w * 1.1);
        } else {
            line(pix, &[(cxp - d, cyp), (cxp + d, cyp)], (255, 255, 255), w * 1.1);
            line(pix, &[(cxp, cyp - d), (cxp, cyp + d)], (255, 255, 255), w * 1.1);
        }
    };
    let arrow = |pix: &mut Pixmap, from: (f32, f32), to: (f32, f32), color: (u8, u8, u8)| {
        let (a, b) = (p(from.0, from.1), p(to.0, to.1));
        line(pix, &[a, b], color, w * 1.2);
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len = (dx * dx + dy * dy).sqrt().max(0.001);
        let (ux, uy) = (dx / len, dy / len);
        let hl = 2.4 * u;
        line(pix, &[(b.0 - ux * hl - uy * hl, b.1 - uy * hl + ux * hl), b, (b.0 - ux * hl + uy * hl, b.1 - uy * hl - ux * hl)], color, w * 1.2);
    };
    match icon {
        Icon::SelCell => { tint(pix, 1, 1, 2, 2, blue, 200); grid(pix); }
        Icon::SelRow => { tint(pix, 0, 1, 3, 2, blue, 200); grid(pix); }
        Icon::SelCol => { tint(pix, 1, 0, 2, 3, blue, 200); grid(pix); }
        Icon::SelTable => { tint(pix, 0, 0, 3, 3, blue, 110); grid(pix); }
        Icon::InsAbove => { tint(pix, 0, 0, 3, 1, blue, 60); grid(pix); badge(pix, 8.0, cy(0) + (cy(1) - cy(0)) * 0.5, blue, false); }
        Icon::InsBelow => { tint(pix, 0, 2, 3, 3, blue, 60); grid(pix); badge(pix, 8.0, cy(2) + (cy(3) - cy(2)) * 0.5, blue, false); }
        Icon::InsLeft => { tint(pix, 0, 0, 1, 3, blue, 60); grid(pix); badge(pix, cx(0) + (cx(1) - cx(0)) * 0.5, 8.0, blue, false); }
        Icon::InsRight => { tint(pix, 2, 0, 3, 3, blue, 60); grid(pix); badge(pix, cx(2) + (cx(3) - cx(2)) * 0.5, 8.0, blue, false); }
        Icon::CellsRight => { tint(pix, 1, 1, 2, 2, blue, 90); grid(pix); arrow(pix, (7.0, 8.0), (13.2, 8.0), blue); }
        Icon::CellsDown => { tint(pix, 1, 1, 2, 2, blue, 90); grid(pix); arrow(pix, (8.0, 7.0), (8.0, 12.6), blue); }
        Icon::DelRow => { tint(pix, 0, 1, 3, 2, red, 60); grid(pix); badge(pix, 8.0, 8.0, red, true); }
        Icon::DelCol => { tint(pix, 1, 0, 2, 3, red, 60); grid(pix); badge(pix, 8.0, 8.0, red, true); }
        Icon::DelTable => { tint(pix, 0, 0, 3, 3, red, 45); grid(pix); badge(pix, 8.0, 8.0, red, true); }
        Icon::ShiftLeft => { tint(pix, 1, 1, 2, 2, red, 70); grid(pix); arrow(pix, (13.2, 8.0), (7.0, 8.0), red); }
        Icon::ShiftUp => { tint(pix, 1, 1, 2, 2, red, 70); grid(pix); arrow(pix, (8.0, 12.6), (8.0, 7.0), red); }
        Icon::MergeRight | Icon::MergeDown | Icon::Split => {
            let vertical = icon == Icon::MergeDown;
            let (ax, ay) = if vertical { p(4.0, 1.5) } else { p(1.5, 4.0) };
            let (bw, bh) = if vertical { (8.0 * u, 13.0 * u) } else { (13.0 * u, 8.0 * u) };
            if icon != Icon::Split { fill_alpha(pix, (ax, ay, bw, bh), blue, 55); }
            stroke_round(pix, (ax, ay, bw, bh), 1.5 * u, ink, w);
            if icon == Icon::Split {
                line(pix, &[p(8.0, 4.0), p(8.0, 12.0)], blue, w * 1.2);
                arrow(pix, (6.8, 8.0), (3.6, 8.0), blue);
                arrow(pix, (9.2, 8.0), (12.4, 8.0), blue);
            } else if vertical {
                arrow(pix, (8.0, 3.2), (8.0, 6.8), blue);
                arrow(pix, (8.0, 12.8), (8.0, 9.2), blue);
            } else {
                arrow(pix, (3.2, 8.0), (6.8, 8.0), blue);
                arrow(pix, (12.8, 8.0), (9.2, 8.0), blue);
            }
        }
        Icon::Distribute => {
            let (ax, ay) = p(x0, 2.5); stroke_round(pix, (ax, ay, 13.0 * u, 7.0 * u), 1.5 * u, ink, w);
            for i in 1..3 { line(pix, &[p(cx(i), 2.5), p(cx(i), 9.5)], ink, w * 0.8); }
            arrow(pix, (8.0, 12.5), (2.2, 12.5), blue); arrow(pix, (8.0, 12.5), (13.8, 12.5), blue);
        }
        Icon::Width => {
            line(pix, &[p(2.0, 4.0), p(2.0, 12.0)], ink, w); line(pix, &[p(14.0, 4.0), p(14.0, 12.0)], ink, w);
            arrow(pix, (8.0, 8.0), (3.4, 8.0), blue); arrow(pix, (8.0, 8.0), (12.6, 8.0), blue);
        }
        Icon::Height => {
            line(pix, &[p(4.0, 2.0), p(12.0, 2.0)], ink, w); line(pix, &[p(4.0, 14.0), p(12.0, 14.0)], ink, w);
            arrow(pix, (8.0, 8.0), (8.0, 3.4), blue); arrow(pix, (8.0, 8.0), (8.0, 12.6), blue);
        }
        Icon::AlignTop | Icon::AlignCenter | Icon::AlignBottom => {
            let (ax, ay) = p(2.0, 2.0); stroke_round(pix, (ax, ay, 12.0 * u, 12.0 * u), 1.5 * u, ink, w);
            let base = match icon { Icon::AlignTop => 4.8, Icon::AlignCenter => 6.8, _ => 8.8 };
            line(pix, &[p(4.8, base), p(11.2, base)], blue, w * 1.3);
            line(pix, &[p(4.8, base + 2.4), p(9.2, base + 2.4)], blue, w * 1.3);
        }
        Icon::Header => { tint(pix, 0, 0, 3, 1, ink, 170); grid(pix); }
        Icon::Minus => line(pix, &[p(4.5, 8.0), p(11.5, 8.0)], ink, w * 1.3),
        Icon::Inline | Icon::Free | Icon::WrapAround | Icon::WrapTopBottom => {
            // Text lines (grey) and the table (blue grid).
            let text = |pix: &mut Pixmap, y: f32, x0: f32, x1: f32| line(pix, &[p(x0, y), p(x1, y)], grey((150, 160, 176)), w);
            let table = |pix: &mut Pixmap, x0: f32, y0: f32, x1: f32, y1: f32| {
                let (ax, ay) = p(x0, y0); let (bx, by) = p(x1, y1);
                fill_alpha(pix, (ax, ay, bx - ax, by - ay), blue, 50);
                stroke_round(pix, (ax, ay, bx - ax, by - ay), 1.0 * u, blue, w);
                line(pix, &[p((x0 + x1) * 0.5, y0), p((x0 + x1) * 0.5, y1)], blue, w * 0.8);
                line(pix, &[p(x0, (y0 + y1) * 0.5), p(x1, (y0 + y1) * 0.5)], blue, w * 0.8);
            };
            match icon {
                Icon::Inline => { text(pix, 2.0, 1.5, 14.5); table(pix, 1.5, 4.5, 10.0, 11.0); text(pix, 13.5, 1.5, 14.5); }
                Icon::Free => { table(pix, 7.5, 4.0, 14.5, 10.5); for y in [2.0, 5.5, 9.0] { text(pix, y, 1.5, 5.5); } text(pix, 2.0, 1.5, 14.5); text(pix, 13.5, 1.5, 14.5); }
                Icon::WrapAround => { table(pix, 5.0, 4.5, 11.0, 11.0); for y in [4.5, 7.75, 11.0] { text(pix, y, 1.0, 3.0); text(pix, y, 13.0, 15.0); } text(pix, 2.0, 1.0, 15.0); text(pix, 13.5, 1.0, 15.0); }
                _ => { text(pix, 2.0, 1.5, 14.5); table(pix, 4.5, 4.5, 11.5, 11.0); text(pix, 13.5, 1.5, 14.5); }
            }
        }
        Icon::Close => {
            // A soft red disc with a white cross, as Word's Close buttons.
            let (cx, cy) = p(8.0, 8.0);
            let red = grey((212, 58, 47));
            if let Some(c) = PathBuilder::from_circle(cx, cy, 7.0 * u) { pix.fill_path(&c, &paint(red), FillRule::Winding, crate::crisp::transform(), None); }
            let (hx, hy) = p(8.0, 5.2);
            if let Some(c) = PathBuilder::from_circle(hx, hy, 4.0 * u) {
                let mut sheen = paint((255, 255, 255)); sheen.set_color_rgba8(255, 255, 255, 34);
                pix.fill_path(&c, &sheen, FillRule::Winding, crate::crisp::transform(), None);
            }
            let d = 2.9 * u;
            line(pix, &[(cx - d, cy - d), (cx + d, cy + d)], (255, 255, 255), 1.7 * u);
            line(pix, &[(cx - d, cy + d), (cx + d, cy - d)], (255, 255, 255), 1.7 * u);
        }
        Icon::Position => {
            let (ax, ay) = p(2.0, 2.0); stroke_round(pix, (ax, ay, 12.0 * u, 12.0 * u), 1.5 * u, ink, w);
            let (bx, by) = p(6.0, 6.0); fill_alpha(pix, (bx, by, 5.5 * u, 4.5 * u), blue, 120);
            arrow(pix, (4.0, 13.0), (4.0, 7.0), blue); arrow(pix, (3.0, 4.0), (8.5, 4.0), blue);
        }
        Icon::Plus => { line(pix, &[p(4.5, 8.0), p(11.5, 8.0)], ink, w * 1.3); line(pix, &[p(8.0, 4.5), p(8.0, 11.5)], ink, w * 1.3); }
    }
}
