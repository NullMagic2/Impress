//! References ribbon commands for the document's table of contents.
use super::*;
use crate::i18n::text;
use wxdragon::{
    dialogs::DialogBuilder,
    geometry::Point,
    id::ID_CANCEL,
    sizers::{BoxSizer, Orientation, SizerFlag},
    widgets::{StaticText, TextCtrl, spinctrl::SpinCtrl},
};

const INSERT_OR_UPDATE: i32 = 32_680;
const CUSTOM: i32 = 32_681;
const AUTOMATIC: i32 = 32_682;

pub(super) fn show(ribbon: &Ribbon) {
    let (exists, automatic) = {
        let state = ribbon.state.borrow();
        let review = &state.editor.document.review;
        (
            !review.contents_paragraphs.is_empty(),
            review.automatic_contents,
        )
    };
    let command = if exists {
        "Update table of contents"
    } else {
        "Insert table of contents"
    };
    let mut menu = Menu::builder()
        .append_item(INSERT_OR_UPDATE, &text(command), "")
        .append_item(CUSTOM, &text("Custom table of contents…"), "")
        .append_separator()
        .append_check_item(AUTOMATIC, &text("Update automatically"), "")
        .build();
    menu.check_item(AUTOMATIC, !exists || automatic);
    menu.enable_item(AUTOMATIC, exists);
    let button = ribbon.ui.toc_button;
    let anchor = crate::gtk_native::to_screen(&button, Point::new(0, button.get_size().height));
    ribbon.ui.main_frame.popup_menu(&mut menu, Some(anchor));
}

pub(super) fn handle(ribbon: &Ribbon, id: i32) -> bool {
    let result = match id {
        INSERT_OR_UPDATE => ribbon.edit(|editor| editor.update_contents(&Default::default())),
        CUSTOM => {
            let (options, insert) = {
                let state = ribbon.state.borrow();
                let review = &state.editor.document.review;
                (
                    review.contents_options.clone(),
                    review.contents_paragraphs.is_empty(),
                )
            };
            match settings(ribbon, options, insert) {
                Some(options) => ribbon.edit(|editor| editor.update_contents_options(options)),
                None => Ok(()),
            }
        }
        AUTOMATIC => ribbon.edit(|editor| {
            editor.set_automatic_contents(!editor.document.review.automatic_contents)
        }),
        _ => return false,
    };
    if let Err(error) = result {
        ribbon.state.borrow_mut().action_status = Some(error);
    }
    ribbon.last_key.borrow_mut().take();
    ribbon.focus_document();
    true
}

fn settings(
    ribbon: &Ribbon,
    options: model::ContentsOptions,
    insert: bool,
) -> Option<model::ContentsOptions> {
    let window = DialogBuilder::new(&ribbon.ui.main_frame, &text("Table of contents")).build();
    let root = BoxSizer::builder(Orientation::Vertical).build();
    let form = BoxSizer::builder(Orientation::Vertical).build();
    let margin = window.from_dip_int(16);
    let gap = window.from_dip_int(6);
    let title_label = StaticText::builder(&window)
        .with_label(&text("Title:"))
        .build();
    let title = TextCtrl::builder(&window)
        .with_size(Size::new(window.from_dip_int(300), -1))
        .with_value(&options.title)
        .build();
    title.set_name(&text("Title:"));
    form.add(&title_label, 0, SizerFlag::Bottom, gap);
    form.add(&title, 0, SizerFlag::Expand | SizerFlag::Bottom, margin);
    let levels_label = StaticText::builder(&window)
        .with_label(&text("Heading levels:"))
        .build();
    let levels = SpinCtrl::builder(&window)
        .with_range(1, 9)
        .with_initial_value(i32::from(options.max_heading_level))
        .build();
    levels.set_name(&text("Heading levels:"));
    form.add(&levels_label, 0, SizerFlag::Bottom, gap);
    form.add(&levels, 0, SizerFlag::empty(), 0);
    root.add_sizer(&form, 0, SizerFlag::All | SizerFlag::Expand, margin);
    dialog_actions::divider(&window, &root, margin);
    let footer = BoxSizer::builder(Orientation::Horizontal).build();
    let ok = Button::builder(&window)
        .with_id(ID_OK)
        .with_label(&text(if insert { "Insert" } else { "Update table" }))
        .build();
    let cancel = Button::builder(&window)
        .with_id(ID_CANCEL)
        .with_label(&text("Cancel"))
        .build();
    dialog_actions::pair(&footer, ok, cancel);
    root.add_sizer(&footer, 0, SizerFlag::All | SizerFlag::Expand, margin);
    window.set_sizer_and_fit(root, true);
    let validate = move || {
        let value = title.get_value();
        ok.enable(
            !value.trim().is_empty() && value.len() <= 256 && !value.contains(['\r', '\n', '\0']),
        );
    };
    validate();
    title.on_text_updated(move |_| validate());
    ok.set_default();
    window.set_escape_id(ID_CANCEL);
    ok.on_click(move |_| window.end_modal(ID_OK));
    cancel.on_click(move |_| window.end_modal(ID_CANCEL));
    crate::placement::centre(&window);
    title.set_focus();
    let result = (window.show_modal() == ID_OK).then(|| model::ContentsOptions {
        title: title.get_value(),
        max_heading_level: levels.value() as u8,
    });
    window.destroy();
    result
}
