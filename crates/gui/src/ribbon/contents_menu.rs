//! References ribbon commands for the document's table of contents.
use super::*;
use crate::i18n::text;
use model::{ContentsOptions, ContentsTemplate, TabLeader};
use wxdragon::{
    dialogs::DialogBuilder,
    event::WindowEvents, window::BackgroundStyle, widgets::panel::Panel,
    geometry::Point,
    id::ID_CANCEL,
    sizers::{BoxSizer, Orientation, SizerFlag},
    widgets::{CheckBox, Choice, StaticText, TextCtrl, spinctrl::SpinCtrl},
};

#[path = "contents_gallery.rs"]
mod gallery;
#[path = "contents_library.rs"]
mod library;

const INSERT_OR_UPDATE: i32 = 32_680;
const CUSTOM: i32 = 32_681;
const AUTOMATIC: i32 = 32_682;
const REMOVE: i32 = 32_683;
const FIRST_LEVEL: i32 = 32_688;

const TEMPLATES: [(ContentsTemplate, &str); 10] = [
    (ContentsTemplate::Classic, "Classic contents"),
    (ContentsTemplate::Compact, "Compact contents"),
    (ContentsTemplate::Modern, "Modern contents"),
    (ContentsTemplate::Book, "Book contents"),
    (ContentsTemplate::Atlas, "Atlas contents"),
    (ContentsTemplate::Technical, "Technical contents"),
    (ContentsTemplate::Formal, "Formal contents"),
    (ContentsTemplate::ReadingOutline, "Reading outline"),
    (ContentsTemplate::InlinePages, "Inline page numbers"),
    (ContentsTemplate::ChapterGuide, "Chapter guide"),
];
const LEADERS: [(TabLeader, &str); 6] = [
    (TabLeader::None, "None"),
    (TabLeader::Dots, "Dots"),
    (TabLeader::Dashes, "Dashes"),
    (TabLeader::Line, "Solid line"),
    (TabLeader::Heavy, "Heavy line"),
    (TabLeader::MiddleDots, "Middle dots"),
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
        .append_item(CUSTOM, &text("Contents templates…"), "")
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
    let entries = match library::path().and_then(|p| library::load(&p)) {
        Ok(saved) => saved,
        Err(error) => { library_error(&window, &error); library::Templates::new() }
    };
    let initial = entries.values().position(|saved| saved == &options).map(|i| TEMPLATES.len() + i)
        .unwrap_or_else(|| design_index(options.template));
    let template = ComboBox::builder(&window)
        .with_style(wxdragon::widgets::combobox::ComboBoxStyle::ReadOnly)
        .with_choices(template_names(&entries)).build();
    template.set_name(&text("Template:"));
    template.set_selection(initial as u32);
    let entries = Rc::new(RefCell::new(entries));
    let design = Rc::new(Cell::new(options.template));
    let toolbar = BoxSizer::builder(Orientation::Horizontal).build();
    let label = StaticText::builder(&window).with_label(&text("Template:")).build();
    toolbar.add(&label, 0, SizerFlag::AlignCenterVertical | SizerFlag::Right, gap);
    toolbar.add(&template, 1, SizerFlag::AlignCenterVertical | SizerFlag::Right, gap);
    let save = Button::builder(&window).with_label(&text("Save")).build();
    let delete = Button::builder(&window).with_label(&text("Delete")).build();
    delete.enable(initial >= TEMPLATES.len());
    toolbar.add(&save, 0, SizerFlag::Right, gap);
    toolbar.add(&delete, 0, SizerFlag::empty(), 0);
    root.add_sizer(&toolbar, 0, SizerFlag::Top | SizerFlag::Left | SizerFlag::Right | SizerFlag::Expand, margin);
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
    let page_numbers = CheckBox::builder(&window).with_label(&text("Show page numbers")).build();
    page_numbers.set_value(options.show_page_numbers);
    form.add(&page_numbers, 0, SizerFlag::Bottom, gap);
    let align = CheckBox::builder(&window).with_label(&text("Right-align page numbers")).build();
    align.set_value(options.right_align_page_numbers);
    form.add(&align, 0, SizerFlag::Bottom, gap);
    let numbered_label = StaticText::builder(&window).with_label(&text("Show page numbers on:")).build();
    let numbered = Choice::builder(&window).with_choices((1..=9).map(|level| match level {
        1 => text("Level 1"),
        9 => text("All levels"),
        _ => format!("1–{level}"),
    }).collect::<Vec<_>>()).build();
    numbered.set_name(&text("Show page numbers on:"));
    numbered.set_selection(u32::from(options.max_numbered_level.clamp(1, 9) - 1));
    form.add(&numbered_label, 0, SizerFlag::Bottom, gap);
    form.add(&numbered, 0, SizerFlag::Expand | SizerFlag::Bottom, gap);
    let leader_label = StaticText::builder(&window).with_label(&text("Tab leader:")).build();
    let leader = Choice::builder(&window)
        .with_choices(LEADERS.iter().map(|item| text(item.1)).collect::<Vec<_>>())
        .build();
    leader.set_selection(LEADERS.iter().position(|item| item.0 == options.leader).unwrap_or(0) as u32);
    leader.set_name(&text("Tab leader:"));
    leader.enable(options.show_page_numbers);
    form.add(&leader_label, 0, SizerFlag::Bottom, gap);
    form.add(&leader, 0, SizerFlag::Expand, 0);
    let current = design.clone();
    let read = Rc::new(move || ContentsOptions {
        title: title.get_value(), max_heading_level: levels.value() as u8,
        template: current.get(),
        show_page_numbers: page_numbers.is_checked(),
        right_align_page_numbers: align.is_checked(),
        max_numbered_level: numbered.get_selection().unwrap_or(8) as u8 + 1,
        leader: LEADERS[leader.get_selection().unwrap_or(0) as usize].0,
    });
    let preview = Panel::builder(&window).build();
    preview.set_min_size(Size::new(window.from_dip_int(280), window.from_dip_int(215)));
    preview.set_background_style(BackgroundStyle::Paint);
    preview.set_name(&text("Example"));
    let fragment = Rc::new(RefCell::new(Err(String::new())));
    let painted = fragment.clone();
    let renderer = RefCell::new(output::raster::BandRenderer::default());
    preview.on_paint(move |_| super::header_footer_gallery::paint_preview(preview, &painted.borrow(), &mut renderer.borrow_mut()));
    let snapshot = read.clone();
    let refresh = Rc::new(move || {
        let options = snapshot();
        numbered.enable(options.show_page_numbers);
        align.enable(options.show_page_numbers);
        leader.enable(options.show_page_numbers && options.right_align_page_numbers);
        *fragment.borrow_mut() = gallery::sample_fragment(options).ok_or_else(String::new);
        preview.refresh(false, None);
    });
    let changed = refresh.clone();
    let presets = entries.clone();
    let snapshot = read.clone();
    let active_design = design.clone();
    template.on_selection_changed(move |_| {
        let Some(index) = template.get_selection().map(|i| i as usize) else { return; };
        let options = if let Some((base, _)) = TEMPLATES.get(index) {
            let mut options = snapshot();
            options.apply_template(*base);
            options
        } else {
            let presets = presets.borrow();
            let Some(saved) = presets.values().nth(index - TEMPLATES.len()) else { return; };
            title.set_value(&saved.title);
            levels.set_value(i32::from(saved.max_heading_level));
            saved.clone()
        };
        active_design.set(options.template);
        delete.enable(index >= TEMPLATES.len());
        leader.set_selection(LEADERS.iter().position(|item| item.0 == options.leader).unwrap_or(0) as u32);
        page_numbers.set_value(options.show_page_numbers);
        align.set_value(options.right_align_page_numbers);
        numbered.set_selection(u32::from(options.max_numbered_level.clamp(1, 9) - 1));
        changed();
    });
    let presets = entries.clone();
    let snapshot = read.clone();
    save.on_click(move |_| {
        let initial = selected_name(template, &presets.borrow()).unwrap_or_default();
        let Some(input) = input_dialog::text(&window, "Save as template…", "Template name:", &initial, false) else { return; };
        let name = input.trim();
        if name.is_empty() || name.chars().any(char::is_control) || name.chars().count() > 128 {
            library_error(&window, "Enter a template name of 1 to 128 characters.");
            return;
        }
        if TEMPLATES.iter().any(|(_, label)| [text(label), label.to_string()].iter().any(|s| s.to_lowercase() == name.to_lowercase())) {
            library_error(&window, "Choose a different name from a built-in template.");
            return;
        }
        let existing = library::matching_name(&presets.borrow(), name).map(str::to_owned);
        if let Some(key) = &existing {
            if !confirm(&window, "Replace template?", key, "Replace") { return; }
        }
        let key = existing.as_deref().unwrap_or(name);
        let expected = presets.borrow().get(key).cloned();
        match library::path().and_then(|path| library::change(&path, key, expected.as_ref(), Some(&snapshot()))) {
            Ok(updated) => {
                let index = TEMPLATES.len() + updated.keys().position(|k| k == key).unwrap();
                reset_list(template, &updated, index);
                *presets.borrow_mut() = updated;
                delete.enable(true);
            }
            Err(error) => library_error(&window, &error),
        }
    });
    delete.on_click(move |_| {
        let Some(key) = selected_name(template, &entries.borrow()) else { return; };
        if !confirm(&window, "Delete template?", &key, "Delete") { return; }
        let expected = entries.borrow().get(&key).cloned();
        match library::path().and_then(|path| library::change(&path, &key, expected.as_ref(), None)) {
            Ok(updated) => {
                reset_list(template, &updated, design_index(design.get()));
                *entries.borrow_mut() = updated;
                delete.enable(false);
            }
            Err(error) => library_error(&window, &error),
        }
    });
    for check in [page_numbers, align] {
        let changed = refresh.clone();
        check.on_toggled(move |_| changed());
    }
    for choice in [numbered, leader] {
        let changed = refresh.clone();
        choice.on_selection_changed(move |_| changed());
    }
    let changed = refresh.clone();
    levels.on_value_changed(move |_| changed());
    let body = BoxSizer::builder(Orientation::Horizontal).build();
    body.add_sizer(&form, 0, SizerFlag::Right, margin);
    let sample = BoxSizer::builder(Orientation::Vertical).build();
    let label = StaticText::builder(&window).with_label(&text("Example")).build();
    sample.add(&label, 0, SizerFlag::Bottom, gap);
    sample.add(&preview, 1, SizerFlag::Expand, 0);
    body.add_sizer(&sample, 1, SizerFlag::Expand, 0);
    root.add_sizer(&body, 1, SizerFlag::All | SizerFlag::Expand, margin);
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
        let valid = !value.trim().is_empty() && value.len() <= 256 && !value.contains(['\r', '\n', '\0']);
        ok.enable(valid);
        save.enable(valid);
    };
    validate();
    refresh();
    title.on_text_updated(move |_| { validate(); refresh(); });
    ok.set_default();
    window.set_escape_id(ID_CANCEL);
    ok.on_click(move |_| window.end_modal(ID_OK));
    cancel.on_click(move |_| window.end_modal(ID_CANCEL));
    crate::placement::centre(&window);
    template.set_focus();
    let result = (window.show_modal() == ID_OK).then(|| read());
    window.destroy();
    result
}

fn design_index(template: ContentsTemplate) -> usize {
    TEMPLATES.iter().position(|item| item.0 == template).unwrap_or(0)
}

fn template_names(saved: &library::Templates) -> Vec<String> {
    TEMPLATES.iter().map(|(_, name)| text(name)).chain(saved.keys().cloned()).collect()
}

fn selected_name(combo: ComboBox, entries: &library::Templates) -> Option<String> {
    let index = (combo.get_selection()? as usize).checked_sub(TEMPLATES.len())?;
    entries.keys().nth(index).cloned()
}

fn reset_list(combo: ComboBox, entries: &library::Templates, index: usize) {
    combo.clear();
    for name in template_names(entries) { combo.append(&name); }
    combo.set_selection(index as u32);
}

fn confirm(parent: &Dialog, question: &str, name: &str, action: &str) -> bool {
    let dialog = MessageDialog::builder(parent, name, &text(question)).with_style(crate::cautious_question()).build();
    dialog.set_yes_no_labels(&text(action), &text("Cancel"));
    crate::placement::centre(&dialog);
    let accepted = dialog.show_modal() == wxdragon::id::ID_YES;
    dialog.destroy();
    accepted
}

fn library_error(parent: &Dialog, error: &str) {
    let dialog = MessageDialog::builder(parent, &text(error), &text("Contents templates")).build();
    crate::placement::centre(&dialog);
    dialog.show_modal();
    dialog.destroy();
}
