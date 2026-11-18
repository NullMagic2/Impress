//! The page's right-click menu, as in Word: Cut, Copy, Paste and Select All
//! with their shortcuts. A right-click outside the selection first moves the
//! caret there; inside it (or on selected cells) the selection is kept. Drawn
//! inside the window like the Ribbon's dropdowns: flat, with a hairline edge.
use super::*;
use crate::table_ui::TableOp;
use wxdragon::{
    dc::AutoBufferedPaintDC,
    event::window_events::WindowEvents,
    font::{Font, FontFamily, FontStyle, FontWeight},
    geometry::{Point, Rect, Size},
    widgets::panel::Panel,
    window::BackgroundStyle,
    DeviceContext,
};

// Sizes in device-independent pixels.
const WIDTH: i32 = 220;
const ROW: i32 = 28;
const PAD: i32 = 4;
const SEPARATOR: i32 = 9;
const HOVER: (u8, u8, u8) = (226, 236, 253);
const INK: (u8, u8, u8) = (36, 48, 66);
const MUTED: (u8, u8, u8) = (150, 158, 170);
const RULE: (u8, u8, u8) = (223, 228, 236);

#[derive(Clone, Copy, PartialEq)]
enum Item { Cut, Copy, Paste, SelectAll, ContentControl }

const ITEMS: [Item; 5] = [Item::Cut, Item::Copy, Item::Paste, Item::SelectAll, Item::ContentControl];
const TABLE_FIRST_ID: i32 = 32_200;
const TABLE_ITEMS: [(TableOp, &str); 7] = [
    (TableOp::InsertRowAbove, "Insert row above"),
    (TableOp::InsertRowBelow, "Insert row below"),
    (TableOp::InsertColumnLeft, "Insert column left"),
    (TableOp::InsertColumnRight, "Insert column right"),
    (TableOp::Distribute, "Distribute columns evenly"),
    (TableOp::DistributeRows, "Distribute rows evenly"),
    (TableOp::FitToMargins, "Fit table to page margins"),
];
const EDIT_FIRST_ID: i32 = TABLE_FIRST_ID + TABLE_ITEMS.len() as i32;

pub(super) struct TableMenuTarget {
    position: editor::DocumentPosition,
    revision: u64,
    table_id: NodeId,
    cell_id: NodeId,
    rows: usize,
    columns: usize,
    fit_margins: bool,
}

impl TableMenuTarget {
    fn at(state: &CanvasState, point: render::Point) -> Option<Self> {
        if state.display_generation != Some(state.layout_generation) { return None; }
        let document = &state.vector_document;
        let hit = state.viewport.hit_test(document, point)?;
        let node = NodeId::from(hit.node_id.as_str());
        let (table_id, cell_id) = state.editor.table_cell_of(&node)?;
        let table = state.editor.table_by_id(&table_id)?;
        let origin = state.viewport.page_rect_px(document, hit.page_index)?;
        let scale = state.viewport.points_to_pixels();
        let x = (point.x - origin.x) / scale;
        let y = (point.y - origin.y) / scale;
        let columns = document.pages[hit.page_index].object_regions.iter().find_map(|region| {
            let render::ObjectKind::TableCell(info) = &region.kind else { return None; };
            let rect = region.rect;
            (region.node_id == cell_id.0 && info.table_id == table_id.0
                && x >= rect.x && x <= rect.max_x() && y >= rect.y && y <= rect.max_y())
                .then_some(info.column_widths_pt.len())
        })?;
        Some(Self {
            position: editor::DocumentPosition::new(node, hit.byte_offset),
            revision: state.editor.revision,
            fit_margins: state.editor.table_margin_width(&table_id).is_some(),
            table_id, cell_id, rows: table.rows.len(), columns,
        })
    }

    fn enabled(&self, op: TableOp) -> bool {
        match op {
            TableOp::Distribute => self.columns > 1,
            TableOp::DistributeRows => self.rows > 1,
            TableOp::FitToMargins => self.fit_margins,
            _ => true,
        }
    }

    pub(super) fn append_commands(&self, menu: &Menu) {
        menu.append_separator();
        for (title, items) in [("Insert", 0..4), ("Table layout", 4..TABLE_ITEMS.len())] {
            let submenu = Menu::builder().build();
            for index in items {
                let (op, label) = TABLE_ITEMS[index];
                let id = TABLE_FIRST_ID + index as i32;
                submenu.append(id, &crate::i18n::text(label), "", wxdragon::menus::menuitem::ItemKind::Normal);
                submenu.enable_item(id, self.enabled(op));
            }
            menu.append_submenu(submenu, &crate::i18n::text(title), "");
        }
    }
}

impl Item {
    fn label(self) -> &'static str {
        match self { Item::Cut => "Cut", Item::Copy => "Copy", Item::Paste => "Paste", Item::SelectAll => "Select All", Item::ContentControl=>"Content control…" }
    }
    fn shortcut(self) -> &'static str {
        match self { Item::Cut => "Ctrl+X", Item::Copy => "Ctrl+C", Item::Paste => "Ctrl+V", Item::SelectAll => "Ctrl+A", Item::ContentControl=>"" }
    }
    fn activate(self, ribbon:&Ribbon) {
        if self==Item::ContentControl {if let Err(error)=ribbon.edit_content_control(){ribbon.state.borrow_mut().action_status=Some(error);}return;}
        let command=match self {Item::Cut=>AppCommand::Cut,Item::Copy=>AppCommand::Copy,Item::Paste=>AppCommand::Paste,_=>AppCommand::SelectAll};
        ribbon.run(Action::Editor(command));
    }
}

/// Top edge and height of each row, in physical pixels; Select All is set
/// apart from the clipboard commands.
fn rows(surface: Panel, has_control:bool) -> Vec<(Item, i32, i32)> {
    let d = |v: i32| surface.from_dip_int(v);
    let mut top = d(PAD);
    ITEMS.into_iter().filter(|item|has_control||*item!=Item::ContentControl).map(|item| {
        if item == Item::SelectAll { top += d(SEPARATOR); }
        let row = (item, top, d(ROW));
        top += d(ROW);
        row
    }).collect()
}

fn item_at(surface: Panel, point: Point, has_control:bool) -> Option<Item> {
    rows(surface,has_control).into_iter().find(|&(_, top, height)| point.y >= top && point.y < top + height).map(|(item, ..)| item)
}

fn paint(surface: Panel, hovered: Option<Item>, has_control:bool, enabled: impl Fn(Item) -> bool) {
    let dc = AutoBufferedPaintDC::new(&surface);
    let size = surface.get_client_size();
    let (width, height) = (size.width.max(1), size.height.max(1));
    let d = |v: i32| surface.from_dip_int(v);
    let Some((mut pixmap, canvas)) = crate::crisp::Canvas::new(&surface, width as i32, height as i32) else { return; };
    pixmap.fill(tiny_skia::Color::WHITE);
    let rows = rows(surface,has_control);
    for &(item, top, row_height) in &rows {
        if hovered == Some(item) && enabled(item) { rect(&mut pixmap, d(PAD), top, width - 2 * d(PAD), row_height, HOVER); }
        if item == Item::SelectAll { rect(&mut pixmap, d(PAD) * 2, top - d(SEPARATOR) / 2 - 1, width - 4 * d(PAD), 1, RULE); }
    }
    canvas.present(&dc, &pixmap, 0, 0);
    dc.set_background_mode(wxdragon::dc::BackgroundMode::Transparent);
    if let Some(font) = Font::new_with_details(10, FontFamily::Default as i32, FontStyle::Normal as i32, FontWeight::Normal as i32, false, "") { dc.set_font(&font); }
    for &(item, top, row_height) in &rows {
        let ink = if enabled(item) { INK } else { MUTED };
        dc.set_text_foreground(Colour::rgb(ink.0, ink.1, ink.2));
        let (_, text_height) = dc.get_text_extent(&crate::i18n::text(&(item.label())));
        let y = top + (row_height - text_height) / 2;
        dc.draw_text(&crate::i18n::text(&(item.label())), d(PAD) * 4, y);
        if enabled(item) { dc.set_text_foreground(Colour::rgb(MUTED.0, MUTED.1, MUTED.2)); }
        let (shortcut_width, _) = dc.get_text_extent(&crate::i18n::text(&(item.shortcut())));
        dc.draw_text(&crate::i18n::text(&(item.shortcut())), width - d(PAD) * 4 - shortcut_width, y);
    }
}

fn rect(pixmap: &mut tiny_skia::Pixmap, x: i32, y: i32, width: i32, height: i32, (r, g, b): (u8, u8, u8)) {
    let mut paint = tiny_skia::Paint::default();
    paint.set_color_rgba8(r, g, b, 255);
    if let Some(area) = tiny_skia::Rect::from_xywh(x as f32, y as f32, width.max(1) as f32, height.max(1) as f32) {
        pixmap.fill_path(&tiny_skia::PathBuilder::from_rect(area), &paint, tiny_skia::FillRule::Winding, crate::crisp::transform(), None);
    }
}

/// Moves the caret to a right-click outside the selection, as a left click
/// would; a click inside the selected text or cells keeps the selection.
fn place_caret(state: &mut CanvasState, view_point: render::Point) {
    let Some(hit) = state.viewport.hit_test(&state.vector_document, view_point) else { return; };
    let position = editor::DocumentPosition { node_id: NodeId::from(hit.node_id.as_str()), byte_offset: hit.byte_offset };
    if state.editor.selection_contains(&position) { return; }
    if let Some(objects_ui::SelectedObject::Cells { cell_ids, .. }) = &state.selected_object {
        let cell = state.editor.table_cell_of(&position.node_id);
        if cell.is_some_and(|(_, id)| cell_ids.contains(&id)) { return; }
    }
    state.selected_object = None;
    state.editor.set_caret(position.node_id, position.byte_offset);
    state.preferred_caret_x_pt = None;
    state.caret_visual_line = Some(hit.visual_line);
    state.caret_page_hint = Some(hit.page_index);
    wake_caret(state);
    refresh_overlay(state);
}

impl Ribbon {
    /// Opens the menu on a right-click in the page.
    pub(super) fn bind_context_menu(&self) {
        let canvas = self.ui.paper_canvas;
        let weak = self.owner.clone();
        canvas.on_mouse_right_down(move |event| {
            let WindowEventData::MouseButton(mouse) = &event else { return; };
            let Some(position) = mouse.get_position() else { return; };
            let Some(ribbon) = weak.upgrade() else { return; };
            if ribbon.mode.get() == ViewMode::Typst { event.skip(true); return; }
            let target = {
                let mut state = ribbon.state.borrow_mut();
                if state.dragging_selection || state.object_drag.is_some() { return; }
                if equation_ui::previewing(&state) {
                    None
                } else {
                    let view_point = logical_to_view(position, state.pixel_scale);
                    place_caret(&mut state, view_point);
                    TableMenuTarget::at(&state, view_point)
                }
            };
            canvas.set_focus();
            canvas.refresh(false, None);
            show(&ribbon, crate::gtk_native::to_screen(&canvas, position), target);
        });
    }

    pub(super) fn handle_table_context_menu(&self, id: i32) -> bool {
        if let Some(&(op, _)) = TABLE_ITEMS.get(id.wrapping_sub(TABLE_FIRST_ID) as usize) {
            {
                let context = self.table_context_menu.borrow();
                let Some(target) = context.as_ref() else { return false; };
                let mut state = self.state.borrow_mut();
                if !target.enabled(op) || state.editor.revision != target.revision { return true; }
                let same_cell = state.editor.table_cell_context()
                    .is_some_and(|cell| cell.table_id == target.table_id && cell.cell_id == target.cell_id);
                if !same_cell {
                    state.editor.set_caret(target.position.node_id.clone(), target.position.byte_offset);
                }
            }
            self.run(Action::Table(op));
            self.focus_document();
            true
        } else if let Some(item) = ITEMS.get(id.wrapping_sub(EDIT_FIRST_ID) as usize) {
            if self.table_context_menu.borrow().is_none() { return false; }
            item.activate(self);
            self.focus_document();
            true
        } else {
            false
        }
    }
}

fn show(ribbon: &Ribbon, at: Point, target: Option<TableMenuTarget>) {
    ribbon.table_context_menu.replace(target);
    if !ribbon.show_spelling_context(at) {
        if ribbon.table_context_menu.borrow().is_some() {
            show_native_table_menu(ribbon, at);
        } else {
            show_edit_menu(ribbon, at);
        }
    }
    ribbon.table_context_menu.take();
}

fn show_native_table_menu(ribbon: &Ribbon, at: Point) {
    let mut menu = Menu::builder().build();
    let has_control = ribbon.selected_content_control().is_some();
    let has_selection = {
        let state = ribbon.state.borrow();
        state.selected_object.is_some() || state.editor.selection.as_ref().is_some_and(|s| !s.is_collapsed())
    };
    for (index, item) in ITEMS.into_iter().enumerate() {
        if item == Item::ContentControl && !has_control { continue; }
        if item == Item::SelectAll { menu.append_separator(); }
        let id = EDIT_FIRST_ID + index as i32;
        let label = crate::i18n::text(item.label());
        let label = if item.shortcut().is_empty() { label } else { format!("{label}\t{}", item.shortcut()) };
        menu.append(id, &label, "", wxdragon::menus::menuitem::ItemKind::Normal);
        menu.enable_item(id, has_selection || !matches!(item, Item::Cut | Item::Copy));
    }
    if let Some(target) = ribbon.table_context_menu.borrow().as_ref() {
        target.append_commands(&menu);
    }
    ribbon.ui.main_frame.popup_menu(&mut menu, Some(at));
}

fn show_edit_menu(ribbon: &Ribbon, at: Point) {
    let has_control=ribbon.selected_content_control().is_some();
    let has_selection = {
        let state = ribbon.state.borrow();
        state.selected_object.is_some() || state.editor.selection.as_ref().is_some_and(|s| !s.is_collapsed())
    };
    let enabled = move |item: Item| has_selection || !matches!(item, Item::Cut | Item::Copy);
    let height = 2 * PAD + (ITEMS.len() as i32 - if has_control{0}else{1}) * ROW + SEPARATOR;
    let scaled = ribbon.ui.main_frame.from_dip(Size::new(WIDTH, height));
    let popup = dropdown::create(&ribbon.ui.main_frame, "Edit", scaled);
    let surface = Panel::builder(&popup).with_pos(dropdown::origin(&popup)).with_size(scaled).build();
    surface.set_background_style(BackgroundStyle::Paint);
    let hovered = Rc::new(Cell::new(None::<Item>));
    { let hovered = Rc::clone(&hovered); surface.on_paint(move |_| paint(surface, hovered.get(), has_control, enabled)); }
    { let hovered = Rc::clone(&hovered); surface.on_mouse_motion(move |event| {
        let WindowEventData::MouseMotion(m) = &event else { return; };
        let next = m.event.get_position().and_then(|p| item_at(surface, p,has_control));
        if hovered.replace(next) != next { surface.refresh(false, None); }
    }); }
    { let weak = ribbon.owner.clone(); surface.on_mouse_left_up(move |event| {
        let WindowEventData::MouseButton(m) = &event else { return; };
        let Some(item) = m.event.get_position().and_then(|p| item_at(surface, p,has_control)).filter(|i| enabled(*i)) else { return; };
        dropdown::dismiss(popup);
        if let Some(ribbon) = weak.upgrade() { item.activate(&ribbon); ribbon.focus_document(); }
    }); }
    { let hovered = Rc::clone(&hovered); let weak = ribbon.owner.clone();
      dropdown::bind_keys(popup, ribbon.ui.paper_canvas, move |key| {
        use wxdragon::keycode::*;
        let all: Vec<Item> = ITEMS.into_iter().filter(|i| (has_control||*i!=Item::ContentControl)&&enabled(*i)).collect();
        let index = hovered.get().and_then(|item| all.iter().position(|i| *i == item));
        let next = match key {
            WXK_RETURN | WXK_NUMPAD_ENTER => {
                let Some(item) = hovered.get() else { return true; };
                dropdown::dismiss(popup);
                if let Some(ribbon) = weak.upgrade() { item.activate(&ribbon); ribbon.focus_document(); }
                return true;
            }
            WXK_UP => index.map_or(all.len() - 1, |i| i.saturating_sub(1)),
            WXK_DOWN => index.map_or(0, |i| (i + 1).min(all.len() - 1)),
            _ => {
                // The underlined-letter keys of Word's menu.
                let item = match u8::try_from(key).map(|k| k.to_ascii_lowercase()) {
                    Ok(b't') => Item::Cut, Ok(b'c') => Item::Copy, Ok(b'p') => Item::Paste, Ok(b'a') => Item::SelectAll,
                    _ => return false,
                };
                if !enabled(item) { return true; }
                dropdown::dismiss(popup);
                if let Some(ribbon) = weak.upgrade() { item.activate(&ribbon); ribbon.focus_document(); }
                return true;
            }
        };
        hovered.set(Some(all[next])); surface.refresh(false, None);
        true
    }); }
    dropdown::show_below_rect(popup, Rect::new(at.x, at.y, 0, 0), &surface);
}
