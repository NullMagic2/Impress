//! A small context-menu entry edits the live control at the caret.
use super::*;
use model::{ContentControl, ContentControlKind, TextAnchor};
impl Ribbon {
    pub(super) fn selected_content_control(&self) -> Option<ContentControl> {
        let state = self.state.borrow();
        let caret = state.editor.focus_position()?;
        let paragraphs = model::document_paragraphs(&state.editor.document);
        let position = |anchor: &TextAnchor| {
            paragraphs
                .iter()
                .position(|p| p.id == anchor.paragraph)
                .map(|index| (index, anchor.offset))
        };
        let at = position(&TextAnchor {
            paragraph: caret.node_id,
            offset: caret.byte_offset,
        })?;
        state
            .editor
            .document
            .content_controls
            .values()
            .filter_map(|control| {
                let start = position(&control.range.start)?;
                let end = position(&control.range.end)?;
                (start <= at && at <= end).then_some((
                    end.0 - start.0,
                    end.1.saturating_sub(start.1),
                    control,
                ))
            })
            .min_by_key(|(paragraphs, bytes, _)| (*paragraphs, *bytes))
            .map(|(_, _, control)| control.clone())
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
