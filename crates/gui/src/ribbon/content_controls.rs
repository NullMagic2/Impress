//! A small context-menu entry edits the live control at the caret.
use super::*;
use model::{ContentControl, ContentControlKind};
impl Ribbon {
    pub(super) fn selected_content_control(&self) -> Option<ContentControl> {
        let state = self.state.borrow();
        control_at(&state.editor, &state.editor.focus_position()?)
    }
    pub(super) fn edit_content_control(&self) -> Result<(), String> {
        let Some(control) = self.selected_content_control() else {
            return Ok(());
        };
        let mut actions = Vec::new();
        if !control.lock.content {
            actions.push("Change value…".into());
        }
        if !control.lock.deletion {
            actions.push("Remove content control".into());
        }
        if actions.is_empty() {
            self.review_info("Content control", "This control is locked.");
            return Ok(());
        }
        let title = control
            .alias
            .as_deref()
            .or(control.tag.as_deref())
            .unwrap_or("Content control");
        let Some(action) = self.choose_review("Content control", title, &actions) else {
            return Ok(());
        };
        if actions[action] == "Remove content control" {
            return self.edit(|editor| editor.remove_content_control(&control.id));
        }
        let value = match &control.kind {
            ContentControlKind::DropDown { items } => {
                let labels = items
                    .iter()
                    .map(|item| item.display.clone())
                    .collect::<Vec<_>>();
                self.choose_review("Content control", "Value:", &labels)
                    .map(|index| items[index].value.clone())
            }
            ContentControlKind::CheckBox { checked, .. } => {
                Some(if *checked { "false" } else { "true" }.into())
            }
            ContentControlKind::Date { full_date, .. } => {
                self.pick_content_date(full_date.as_deref())
            }
            ContentControlKind::Picture => {
                use std::io::Read;
                use wxdragon::dialogs::file_dialog::{FileDialog, FileDialogStyle};
                let dialog = FileDialog::builder(&self.ui.main_frame)
                    .with_message(&crate::i18n::text("Picture"))
                    .with_wildcard(&crate::i18n::wildcard(
                        "Images (*.png;*.jpg;*.jpeg;*.gif;*.bmp)|*.png;*.jpg;*.jpeg;*.gif;*.bmp",
                    ))
                    .with_style(FileDialogStyle::Open | FileDialogStyle::FileMustExist)
                    .build();
                if dialog.show_modal() == ID_OK {
                    if let Some(path) = dialog.get_path() {
                        let mut bytes = Vec::new();
                        std::fs::File::open(path)
                            .map_err(|error| error.to_string())?
                            .take(model::MAX_CONTENT_CONTROL_IMAGE_BYTES as u64 + 1)
                            .read_to_end(&mut bytes)
                            .map_err(|error| error.to_string())?;
                        self.edit(|editor| {
                            editor.set_content_control_picture(&control.id, &bytes)
                        })?;
                    }
                }
                None
            }
            ContentControlKind::Group => None,
            _ => {
                let current = {
                    let state = self.state.borrow();
                    model::content_control_text(&state.editor.document, &control.range)
                        .unwrap_or_default()
                };
                self.text_review(
                    "Content control",
                    "Value:",
                    &current,
                    !matches!(
                        control.kind,
                        ContentControlKind::PlainText { multiline: false }
                            | ContentControlKind::ComboBox { .. }
                            | ContentControlKind::Date { .. }
                    ),
                )
            }
        };
        if let Some(value) = value {
            self.edit(|editor| editor.set_content_control_value(&control.id, &value))?;
        }
        Ok(())
    }
}

impl Ribbon {
    fn pick_content_date(&self, initial: Option<&str>) -> Option<String> {
        use wxdragon::{
            datetime::DateTime,
            dialogs::{DialogBuilder, DialogStyle},
            id::ID_CANCEL,
            sizers::{BoxSizer, Orientation, SizerFlag},
            widgets::date_picker_ctrl::{DatePickerCtrl, DatePickerCtrlStyle},
        };
        const GAP_DIP: i32 = 12;
        let dialog = DialogBuilder::new(&self.ui.main_frame, &crate::i18n::text("Date"))
            .with_style(DialogStyle::DefaultDialogStyle)
            .build();
        let root = BoxSizer::builder(Orientation::Vertical).build();
        let gap = dialog.from_dip_int(GAP_DIP);
        let picker = DatePickerCtrl::builder(&dialog)
            .with_style(DatePickerCtrlStyle::Dropdown | DatePickerCtrlStyle::ShowCentury)
            .build();
        if let Some((year, month, day)) = initial.and_then(model::control_date_parts) {
            picker.set_value(&DateTime::new(year, month as u16, day as i16, 0, 0, 0));
        } else {
            picker.set_value(&DateTime::now());
        }
        root.add(&picker, 0, SizerFlag::All | SizerFlag::Expand, gap);
        let buttons = BoxSizer::builder(Orientation::Horizontal).build();
        let ok = Button::builder(&dialog)
            .with_id(ID_OK)
            .with_label(&crate::i18n::text("OK"))
            .build();
        let cancel = Button::builder(&dialog)
            .with_id(ID_CANCEL)
            .with_label(&crate::i18n::text("Cancel"))
            .build();
        dialog_actions::pair(&buttons, ok, cancel);
        dialog_actions::divider(&dialog, &root, gap);
        root.add_sizer(&buttons, 0, SizerFlag::All | SizerFlag::Expand, gap);
        dialog.set_sizer_and_fit(root, true);
        dialog.set_escape_id(ID_CANCEL);
        ok.set_default();
        ok.on_click(move |_| dialog.end_modal(ID_OK));
        cancel.on_click(move |_| dialog.end_modal(ID_CANCEL));
        crate::placement::centre(&dialog);
        let result = if dialog.show_modal() == ID_OK {
            let date = picker.get_value();
            date.is_valid()
                .then(|| format!("{:04}-{:02}-{:02}", date.year(), date.month(), date.day()))
        } else {
            None
        };
        dialog.destroy();
        result
    }
}

// Native choices live beside their text. The model already owns the items,
// locks, XML bindings and the transaction that updates linked controls.
const FIRST_CHOICE: i32 = 25_000;
const OTHER_VALUE: i32 = 26_001;
thread_local! { static OPEN_CHOICE: RefCell<Option<(ContentControl, u64)>> = const { RefCell::new(None) }; }

fn control_at(editor: &EditorState, at: &editor::DocumentPosition) -> Option<ContentControl> {
    let controls = &editor.document.content_controls;
    let same_paragraph = controls.values().filter(|c| c.range.start.paragraph == c.range.end.paragraph
        && c.range.start.paragraph == at.node_id && c.range.start.offset <= at.byte_offset && at.byte_offset <= c.range.end.offset)
        .min_by_key(|c| c.range.end.offset.saturating_sub(c.range.start.offset));
    if let Some(control) = same_paragraph { return Some(control.clone()); }
    if !controls.values().any(|c| c.range.start.paragraph != c.range.end.paragraph) { return None; }
    let paragraphs = model::document_paragraphs(&editor.document);
    let position = |node: &NodeId, offset| paragraphs.iter().position(|p| &p.id == node).map(|index| (index, offset));
    let at = position(&at.node_id, at.byte_offset)?;
    controls.values().filter_map(|c| {
        let start = position(&c.range.start.paragraph, c.range.start.offset)?;
        let end = position(&c.range.end.paragraph, c.range.end.offset)?;
        (start <= at && at <= end).then_some((end.0 - start.0, end.1.saturating_sub(start.1), c))
    }).min_by_key(|(paragraphs, bytes, _)| (*paragraphs, *bytes)).map(|(_, _, c)| c.clone())
}
fn has_choices(control: &ContentControl) -> bool {
    !control.lock.content && match &control.kind {
        ContentControlKind::DropDown { items } => !items.is_empty(),
        ContentControlKind::ComboBox { .. } => true,
        _ => false,
    }
}
fn choice_geometry(state: &CanvasState) -> Option<(ContentControl, Vec<render::Rect>, render::Rect)> {
    if state.display_generation != Some(state.layout_generation) || state.revisions.is_original() { return None; }
    let control = control_at(&state.editor, &state.editor.focus_position()?)?;
    if !has_choices(&control) { return None; }
    let selection = render::SourceSelection {
        anchor: render::SourcePosition { node_id: control.range.start.paragraph.0.clone(), byte_offset: control.range.start.offset },
        focus: render::SourcePosition { node_id: control.range.end.paragraph.0.clone(), byte_offset: control.range.end.offset },
    };
    let overlay = render::overlay_for_selection(&state.vector_document, Some(&selection), 1.0);
    let scale = state.viewport.points_to_pixels();
    let origins = state.viewport.page_rects_px(&state.vector_document);
    let rects: Vec<_> = overlay.selections.iter().filter_map(|area| {
        let origin = origins.get(area.page_index)?;
        Some(render::Rect::new(origin.x + area.rect.x * scale, origin.y + area.rect.y * scale,
            area.rect.width * scale, area.rect.height * scale))
    }).collect();
    let last = *rects.last()?;
    let size = (state.viewport.dpi / 96.0).max(1.0) * 16.0;
    let arrow = render::Rect::new(last.x + last.width + 2.0, last.y, size, last.height.max(size));
    Some((control, rects, arrow))
}
pub(crate) fn arrow_at(state: &CanvasState, point: render::Point) -> Option<ContentControl> {
    let (control, _, r) = choice_geometry(state)?;
    (point.x >= r.x && point.x <= r.x + r.width && point.y >= r.y && point.y <= r.y + r.height).then_some(control)
}
pub(crate) fn chrome(state: &CanvasState) -> Vec<render::ChromeShape> {
    use render::{ChromeShape, Color, Point};
    let Some((_, rects, arrow)) = choice_geometry(state) else { return Vec::new(); };
    let blue = Color::rgb8(58, 104, 194);
    let mut shapes: Vec<_> = rects.into_iter().map(|rect| ChromeShape::RoundRect {
        rect, radius: 1.0, fill: None, stroke: Some((blue, 1.0)),
    }).collect();
    shapes.push(ChromeShape::RoundRect { rect: arrow, radius: 2.0,
        fill: Some(Color::rgb8(235, 242, 253)), stroke: Some((blue, 1.0)) });
    let x = arrow.x + arrow.width / 2.0; let y = arrow.y + arrow.height / 2.0;
    let d = arrow.width / 5.0;
    for (a, b) in [(Point::new(x - d, y - d / 2.0), Point::new(x, y + d / 2.0)),
        (Point::new(x, y + d / 2.0), Point::new(x + d, y - d / 2.0))] {
        shapes.push(ChromeShape::Line { from: a, to: b, width: 1.3, color: blue });
    }
    shapes
}
fn menu_anchor(ribbon: &Ribbon) -> wxdragon::geometry::Point {
    let state = ribbon.state.borrow();
    let rect = choice_geometry(&state).map(|(_, _, r)| r);
    let at = rect.map_or(wxdragon::geometry::Point::new(20, 20), |r|
        wxdragon::geometry::Point::new((r.x / state.pixel_scale) as i32, ((r.y + r.height) / state.pixel_scale) as i32));
    crate::gtk_native::to_screen(&ribbon.ui.paper_canvas, at)
}
fn show_choices(ribbon: &Ribbon, control: ContentControl) {
    use wxdragon::menus::menuitem::ItemKind;
    if !has_choices(&control) { return; }
    let items = match &control.kind { ContentControlKind::DropDown { items } | ContentControlKind::ComboBox { items } => items, _ => return };
    // A native menu can use a bounded command-ID range. The existing list
    // dialog remains available for unusually large lists, without truncation.
    if items.len() > 1000 {
        let mut labels: Vec<_> = items.iter().map(|item| item.display.clone()).collect();
        if matches!(control.kind, ContentControlKind::ComboBox { .. }) { labels.push(crate::i18n::text("Other value…")); }
        if let Some(index) = ribbon.choose_review("Content control", "Value:", &labels) {
            let value = items.get(index).map(|item| item.value.clone()).or_else(|| {
                let current = model::content_control_text(&ribbon.state.borrow().editor.document, &control.range).unwrap_or_default();
                ribbon.text_review("Content control", "Value:", &current, false)
            });
            if let Some(value) = value {
                if let Err(error) = ribbon.edit(|e| e.set_content_control_value(&control.id, &value)) {
                    ribbon.state.borrow_mut().action_status = Some(error);
                }
            }
        }
        return;
    }
    let (revision, current) = {
        let state = ribbon.state.borrow();
        (state.editor.revision, model::content_control_text(&state.editor.document, &control.range).unwrap_or_default())
    };
    let mut menu = Menu::builder().build();
    for (index, item) in items.iter().enumerate() {
        let id = FIRST_CHOICE + index as i32;
        menu.append(id, &item.display.replace('&', "&&"), "", ItemKind::Check);
        menu.check_item(id, item.display == current);
    }
    if matches!(control.kind, ContentControlKind::ComboBox { .. }) {
        if !items.is_empty() { menu.append_separator(); }
        menu.append(OTHER_VALUE, &crate::i18n::text("Other value…"), "", ItemKind::Normal);
    }
    OPEN_CHOICE.with(|open| open.replace(Some((control, revision))));
    ribbon.ui.main_frame.popup_menu(&mut menu, Some(menu_anchor(ribbon)));
    OPEN_CHOICE.with(|open| open.borrow_mut().take());
    ribbon.focus_document();
}
pub(super) fn handle(ribbon: &Ribbon, id: i32) -> bool {
    if id != OTHER_VALUE && !(FIRST_CHOICE..FIRST_CHOICE + 1000).contains(&id) { return false; }
    let Some((control, revision)) = OPEN_CHOICE.with(|open| open.borrow().clone()) else { return false; };
    if ribbon.state.borrow().editor.revision != revision { return true; }
    let value = if id == OTHER_VALUE {
        if !matches!(control.kind, ContentControlKind::ComboBox { .. }) { return true; }
        let current = model::content_control_text(&ribbon.state.borrow().editor.document, &control.range).unwrap_or_default();
        ribbon.text_review("Content control", "Value:", &current, false)
    } else {
        match &control.kind {
            ContentControlKind::DropDown { items } | ContentControlKind::ComboBox { items } =>
                items.get((id - FIRST_CHOICE) as usize).map(|item| item.value.clone()),
            _ => None,
        }
    };
    if let Some(value) = value {
        if let Err(error) = ribbon.edit(|editor| editor.set_content_control_value(&control.id, &value)) {
            ribbon.state.borrow_mut().action_status = Some(error);
        }
    }
    true
}
pub(super) fn bind(ribbon: &Rc<Ribbon>) {
    use wxdragon::event::{EventType, WxEvtHandler};
    let weak = Rc::downgrade(ribbon);
    ribbon.ui.main_frame.bind_internal(EventType::CHAR_HOOK, move |event| {
        let Some(ribbon) = weak.upgrade() else { return; };
        if event.get_key_code() == Some(wxdragon::keycode::WXK_DOWN) && event.alt_down()
            && !event.cmd_down() && !event.shift_down() && ribbon.ui.paper_canvas.has_focus() {
            let control = if ribbon.state.borrow().revisions.is_original() { None }
                else { ribbon.selected_content_control().filter(has_choices) };
            if let Some(control) = control { show_choices(&ribbon, control); event.skip(false); return; }
        }
        event.skip(true);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn greek_inline_choices_are_found_by_byte_range_and_respect_locks() {
        let mut document = model::Document::default();
        let paragraph = model::Paragraph::plain("πριν ____ μετά"); let id = paragraph.id.clone();
        document.sections[0].blocks = vec![model::Block::Paragraph(paragraph)].into();
        let start = "πριν ".len(); let end = start + 4;
        let mut control = ContentControl {
            id: NodeId::new(), source_id: None, tag: None, alias: None,
            range: model::AnchorRange { start: model::TextAnchor { paragraph: id.clone(), offset: start },
                end: model::TextAnchor { paragraph: id.clone(), offset: end } },
            scope: Default::default(), members: Vec::new(), kind: ContentControlKind::DropDown {
                items: vec![model::ContentControlItem { display: "φόροι".into(), value: "tax".into() }] },
            lock: Default::default(), binding: None, placeholder: true, temporary: false, properties_xml: None,
        };
        document.content_controls.insert(control.id.clone(), control.clone());
        let editor = EditorState::new(document);
        for offset in [start, start + 2, end] {
            let found = control_at(&editor, &editor::DocumentPosition::new(id.clone(), offset)).unwrap();
            assert_eq!(found.id, control.id); assert!(has_choices(&found));
        }
        assert!(control_at(&editor, &editor::DocumentPosition::new(id.clone(), start - 1)).is_none());
        assert!(control_at(&editor, &editor::DocumentPosition::new(id, end + 1)).is_none());
        control.lock.content = true; assert!(!has_choices(&control));
        control.lock.content = false; control.kind = ContentControlKind::ComboBox { items: Vec::new() }; assert!(has_choices(&control));
        control.kind = ContentControlKind::DropDown { items: Vec::new() }; assert!(!has_choices(&control));
    }
}

/// Mouse handling shares the canvas's existing event route, which consumes
/// selection gestures before a second wx handler would see them.
pub(crate) fn press(state: &mut CanvasState, point: render::Point) -> bool {
    state.pressed_content_choice = None;
    let Some(control) = arrow_at(state, point) else { return false; };
    state.dragging_selection = false; state.drag_pointer = None; state.key_repeat = None;
    state.pressed_content_choice = Some((control.id, state.editor.revision));
    true
}
pub(crate) fn release(state: &mut CanvasState, point: render::Point) -> bool {
    let Some((id, revision)) = state.pressed_content_choice.take() else { return false; };
    if state.editor.revision == revision && arrow_at(state, point).is_some_and(|c| c.id == id) {
        state.pending_content_choice = Some((id, revision));
    }
    true
}
pub(super) fn poll(ribbon: &Ribbon) {
    let request = ribbon.state.borrow_mut().pending_content_choice.take();
    let Some((id, revision)) = request else { return; };
    let control = {
        let state = ribbon.state.borrow();
        (state.editor.revision == revision).then(|| state.editor.document.content_controls.get(&id).cloned()).flatten()
    };
    if let Some(control) = control { show_choices(ribbon, control); }
}
