//! Native citation manager for the mockup's Insert Citation command.
use super::*;
use super::format_dialogs::Chrome;

#[derive(Clone)]
struct Source { author: String, title: String, year: String, publisher: String }

fn citation(source: &Source, style: &str, locator: &str) -> String {
    let author = source.author.split(',').next().unwrap_or(&source.author).trim();
    let location = if locator.trim().is_empty() { String::new() } else { format!(", {}", locator.trim()) };
    match style {
        "IEEE" => format!("[{}{}]", author, location),
        "MLA" => format!("({}{})", author, location),
        "Chicago" => format!("({} {}{})", author, source.year, location),
        _ => format!("({}, {}{})", author, source.year, location),
    }
}

pub(super) fn open(ribbon: &Ribbon) -> Result<(), String> {
    let ui = crate::i18n::localized(crate::CitationDialogUi::new(Some(&ribbon.ui.main_frame), false));
    let sources = Rc::new(RefCell::new(Vec::<Source>::new()));
    let chrome = Chrome::new(ui.citation_manager_dialog, ui.citation_manager_card, &[], &[],
        (ui.citation_manager_ok, "Insert citation"), Some(ui.citation_manager_cancel), &[],
        &[(ui.add_source_button, "Add source"), (ui.remove_source_button, "Remove"),
          (ui.new_citation_style_button, "New preset"), (ui.add_citation_style_button, "Add preset"),
          (ui.save_source_button, "Save source"), (ui.cancel_source_button, "Cancel")]);

    let form = ui.source_form;
    let dialog = ui.citation_manager_dialog;
    ui.add_source_button.on_click(move |_| { form.show(true); dialog.layout(); dialog.fit(); });
    let form = ui.source_form;
    let dialog = ui.citation_manager_dialog;
    ui.cancel_source_button.on_click(move |_| { form.show(false); dialog.layout(); dialog.fit(); });

    let list = ui.source_list;
    let form = ui.source_form;
    let dialog = ui.citation_manager_dialog;
    let author = ui.source_author_field;
    let title = ui.source_title_field;
    let year = ui.source_year_field;
    let publisher = ui.source_container_field;
    let preview = ui.citation_preview;
    let style = ui.citation_style_list;
    let items = sources.clone();
    ui.save_source_button.on_click(move |_| {
        let entry = Source { author: author.get_value().trim().to_string(), title: title.get_value().trim().to_string(),
            year: year.get_value().trim().to_string(), publisher: publisher.get_value().trim().to_string() };
        if entry.author.is_empty() || entry.title.is_empty() { return; }
        list.append(&format!("{} — {} ({})", entry.author, entry.title, entry.year));
        items.borrow_mut().push(entry.clone());
        list.set_selection(items.borrow().len() as u32 - 1, true);
        preview.set_value(&format!("{}\n\n{} — {}. {}.", citation(&entry, &style.get_value(), ""), entry.author, entry.title, entry.publisher));
        form.show(false); dialog.layout(); dialog.fit();
    });

    let list = ui.source_list;
    let items = sources.clone();
    let preview = ui.citation_preview;
    ui.remove_source_button.on_click(move |_| {
        if let Some(index) = list.get_selection() {
            list.delete(index);
            items.borrow_mut().remove(index as usize);
            preview.set_value("Select a source to preview its citation and bibliography entry.");
        }
    });

    let name = ui.new_style_name_field;
    let base = ui.new_style_base_combo;
    let add = ui.add_citation_style_button;
    let dialog = ui.citation_manager_dialog;
    ui.new_citation_style_button.on_click(move |_| {
        name.show(true); base.show(true); add.show(true); dialog.layout(); dialog.fit(); name.set_focus();
    });
    let name = ui.new_style_name_field;
    let base = ui.new_style_base_combo;
    let add = ui.add_citation_style_button;
    let style = ui.citation_style_list;
    let dialog = ui.citation_manager_dialog;
    ui.add_citation_style_button.on_click(move |_| {
        let label = name.get_value();
        if label.trim().is_empty() { return; }
        style.append(label.trim());
        style.set_value(label.trim());
        name.show(false); base.show(false); add.show(false); dialog.layout(); dialog.fit();
    });

    let list = ui.source_list;
    let items = sources.clone();
    let preview = ui.citation_preview;
    let style = ui.citation_style_list;
    let locator = ui.citation_locator_field;
    ui.source_list.on_selection_changed(move |_| {
        if let Some(source) = list.get_selection().and_then(|i| items.borrow().get(i as usize).cloned()) {
            preview.set_value(&format!("{}\n\n{} — {}. {}.", citation(&source, &style.get_value(), &locator.get_value()), source.author, source.title, source.publisher));
        }
    });

    if chrome.show() {
        if let Some(source) = ui.source_list.get_selection().and_then(|i| sources.borrow().get(i as usize).cloned()) {
            let value = citation(&source, &crate::i18n::choice_source(&ui.citation_style_list.get_value(), &["APA", "IEEE", "Chicago", "MLA", "Harvard"]), &ui.citation_locator_field.get_value());
            ribbon.edit(|editor| editor.insert_text(&value))?;
        }
    }
    Ok(())
}
