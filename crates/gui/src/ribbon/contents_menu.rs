//! References ribbon commands for the document's table of contents.
use super::*;
use crate::i18n::text;
use model::{ContentsOptions, ContentsTemplate, TabLeader};
use wxdragon::{
    dialogs::DialogBuilder,
    geometry::Point,
    id::ID_CANCEL,
    sizers::{BoxSizer, Orientation, SizerFlag},
    widgets::{CheckBox, Choice, StaticText, TextCtrl, spinctrl::SpinCtrl},
};

#[path = "contents_gallery.rs"]
mod gallery;

const INSERT_OR_UPDATE: i32 = 32_680;
const CUSTOM: i32 = 32_681;
const AUTOMATIC: i32 = 32_682;
const REMOVE: i32 = 32_683;
const GALLERY: i32 = 32_684;
const FIRST_LEVEL: i32 = 32_688;

const TEMPLATES: [(ContentsTemplate, &str); 8] = [
    (ContentsTemplate::Classic, "Classic contents"),
    (ContentsTemplate::Compact, "Compact contents"),
    (ContentsTemplate::Modern, "Modern contents"),
    (ContentsTemplate::Book, "Book contents"),
    (ContentsTemplate::Atlas, "Atlas contents"),
    (ContentsTemplate::Technical, "Technical contents"),
    (ContentsTemplate::Formal, "Formal contents"),
    (ContentsTemplate::ReadingOutline, "Reading outline"),
];
const LEADERS: [(TabLeader, &str); 4] = [
    (TabLeader::None, "None"),
    (TabLeader::Dots, "Dots"),
    (TabLeader::Dashes, "Dashes"),
    (TabLeader::Line, "Solid line"),
];

fn state(ribbon: &Ribbon) -> (ContentsOptions, bool, bool) {
    let canvas = ribbon.state.borrow();
    let review = &canvas.editor.document.review;
    let insert = review.contents_paragraphs.is_empty();
    let mut options = review.contents_options.clone();
    if insert && options.title == ContentsOptions::default().title { options.title = text("Contents"); }
    (options, insert, review.automatic_contents)
}

fn popup(ribbon: &Ribbon, button: Button, tooltip: &str, mut menu: Menu) {
    let anchor = crate::gtk_native::to_screen(&button, Point::new(0, button.get_size().height));
    button.set_tooltip("");
    ribbon.ui.main_frame.popup_menu(&mut menu, Some(anchor));
    button.set_tooltip(&text(tooltip));
}

pub(super) fn show(ribbon: &Ribbon) {
    let (_, insert, automatic) = state(ribbon);
    let menu = Menu::builder()
        .append_item(GALLERY, &text("Contents gallery…"), "")
        .append_item(CUSTOM, &text("Custom table of contents…"), "")
        .append_separator()
        .append_item(INSERT_OR_UPDATE, &text(if insert { "Insert table of contents" } else { "Update table of contents" }), "")
        .append_check_item(AUTOMATIC, &text("Update automatically"), "")
        .append_item(REMOVE, &text("Remove table of contents"), "")
        .build();
    menu.check_item(AUTOMATIC, insert || automatic);
    menu.enable_item(AUTOMATIC, !insert);
    menu.enable_item(REMOVE, !insert);
    popup(ribbon, ribbon.ui.toc_button, "Table of contents", menu);
}

pub(super) fn show_levels(ribbon: &Ribbon) {
    let (level, valid) = {
        let state = ribbon.state.borrow();
        let editor = &state.editor;
        let paragraph = editor.focus_position().and_then(|position| {
            model::document_paragraphs(&editor.document).into_iter().find(|p| p.id == position.node_id)
        });
        (paragraph.and_then(|p| editor.document.styles.paragraph_outline_level(p)).unwrap_or(0),
         paragraph.is_some_and(|p| !editor.document.review.contents_paragraphs.contains(&p.id)
             && !editor.is_editing_header_footer() && editor.note_for_paragraph(&p.id).is_none()))
    };
    if !valid { return; }
    let mut builder = Menu::builder().append_radio_item(FIRST_LEVEL, &text("Exclude from contents"), "");
    for level in 1..=9 {
        builder = builder.append_radio_item(FIRST_LEVEL + level, &text(&format!("Level {level}")), "");
    }
    let menu = builder.build();
    menu.check_item(FIRST_LEVEL + i32::from(level), true);
    popup(ribbon, ribbon.ui.toc_add_text_button, "Add text", menu);
}

pub(super) fn contains_paragraph(ribbon: &Ribbon) -> bool {
    let canvas = ribbon.state.borrow();
    canvas.editor.focus_position().is_some_and(|p| canvas.editor.document.review.contents_paragraphs.contains(&p.node_id))
}

pub(super) fn append_context(menu: &Menu) {
    use wxdragon::menus::menuitem::ItemKind;
    menu.append_separator();
    for (id, title) in [
        (INSERT_OR_UPDATE, "Update table of contents"),
        (CUSTOM, "Custom table of contents…"),
        (REMOVE, "Remove table of contents"),
    ] {
        menu.append(id, &text(title), "", ItemKind::Normal);
    }
}

pub(super) fn update(ribbon: &Ribbon) -> Result<(), String> {
    let (options, _, _) = state(ribbon);
    apply_options(ribbon, options)
}

fn apply_options(ribbon: &Ribbon, options: ContentsOptions) -> Result<(), String> {
    ribbon.edit_without_focus(|editor| editor.update_contents_options(options))?;
    let mut state = ribbon.state.borrow_mut();
    if state.editor.document.review.contents_paragraphs.len() == 1 {
        state.action_status = Some("No headings in the table of contents yet.".into());
    }
    Ok(())
}

pub(super) fn handle(ribbon: &Ribbon, id: i32) -> bool {
    let result = match id {
        INSERT_OR_UPDATE => update(ribbon),
        CUSTOM => {
            let (options, insert, _) = state(ribbon);
            match settings(ribbon, options, insert) {
                Some(options) => apply_options(ribbon, options),
                None => Ok(()),
            }
        }
        AUTOMATIC => ribbon.edit_without_focus(|editor| {
            editor.set_automatic_contents(!editor.document.review.automatic_contents)
        }),
        REMOVE => ribbon.edit_without_focus(EditorState::remove_contents),
        GALLERY => {
            let (mut options, insert, _) = state(ribbon);
            if let Some(template) = gallery::choose(ribbon, &options, insert) {
                options.apply_template(template);
                apply_options(ribbon, options)
            } else { Ok(()) }
        }
        id if (FIRST_LEVEL..=FIRST_LEVEL + 9).contains(&id) => {
            ribbon.edit_formatting(|editor| editor.set_contents_level((id - FIRST_LEVEL) as u8))
        }
        _ => return false,
    };
    if let Err(error) = result { ribbon.state.borrow_mut().action_status = Some(error); }
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
    form.add(&levels, 0, SizerFlag::Bottom, margin);
    let template_label = StaticText::builder(&window).with_label(&text("Contents template:")).build();
    let template = Choice::builder(&window)
        .with_choices(TEMPLATES.iter().map(|item| text(item.1)).collect::<Vec<_>>())
        .build();
    template.set_selection(TEMPLATES.iter().position(|item| item.0 == options.template).unwrap_or(0) as u32);
    template.set_name(&text("Contents template:"));
    form.add(&template_label, 0, SizerFlag::Bottom, gap);
    form.add(&template, 0, SizerFlag::Expand | SizerFlag::Bottom, margin);
    let page_numbers = CheckBox::builder(&window).with_label(&text("Show page numbers")).build();
    page_numbers.set_value(options.show_page_numbers);
    form.add(&page_numbers, 0, SizerFlag::Bottom, gap);
    let leader_label = StaticText::builder(&window).with_label(&text("Tab leader:")).build();
    let leader = Choice::builder(&window)
        .with_choices(LEADERS.iter().map(|item| text(item.1)).collect::<Vec<_>>())
        .build();
    leader.set_selection(LEADERS.iter().position(|item| item.0 == options.leader).unwrap_or(0) as u32);
    leader.set_name(&text("Tab leader:"));
    leader.enable(options.show_page_numbers);
    form.add(&leader_label, 0, SizerFlag::Bottom, gap);
    form.add(&leader, 0, SizerFlag::Expand, 0);
    page_numbers.on_toggled(move |_| leader.enable(page_numbers.is_checked()));
    template.on_selection_changed(move |_| {
        let mut options = ContentsOptions::default();
        options.apply_template(TEMPLATES[template.get_selection().unwrap_or(0) as usize].0);
        leader.set_selection(LEADERS.iter().position(|item| item.0 == options.leader).unwrap_or(0) as u32);
        page_numbers.set_value(options.show_page_numbers);
        leader.enable(options.show_page_numbers);
    });
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
        template: TEMPLATES[template.get_selection().unwrap_or(0) as usize].0,
        show_page_numbers: page_numbers.is_checked(),
        leader: LEADERS[leader.get_selection().unwrap_or(0) as usize].0,
    });
    window.destroy();
    result
}
