use super::*;
use crate::i18n::{style_name, text};
use model::{
    ListDefinition, ListFormat, ListLevel, NamedListStyle, NodeId, Style, StyleId, StyleKind,
    StyleSheet, TableStyle,
};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use wxdragon::{
    dialogs::{
        Dialog, DialogBuilder, DialogStyle,
        file_dialog::{FileDialog, FileDialogStyle},
    },
    id::ID_CANCEL,
    sizers::{BoxSizer, Orientation, SizerFlag},
    widgets::{
        Choice, StaticText, TextCtrl,
        listbox::{ListBox, ListBoxStyle},
    },
};

pub(super) fn new_style(
    ribbon: &Ribbon,
    parent: &impl WxWidget,
    from_selection: bool,
) -> Result<(), String> {
    let choices = labels(&["Paragraph style", "Character style"]);
    let Some(kind) = input_dialog::choice(parent, "New style", "Choose the style type:", &choices)
    else {
        return Ok(());
    };
    let kind = if kind == 0 {
        StyleKind::Paragraph
    } else {
        StyleKind::Character
    };
    let mut style = Style {
        id: StyleId::new(),
        name: text("New style"),
        kind,
        based_on: (kind == StyleKind::Paragraph).then(|| StyleId::from(model::STYLE_NORMAL)),
        next_style: None,
        character: Default::default(),
        paragraph: Default::default(),
        formatting: Default::default(),
    };
    {
        let state = ribbon.state.borrow();
        let original = style.name.clone();
        let mut index = 2;
        while state
            .editor
            .document
            .styles
            .id_by_name(&style.name)
            .is_some()
        {
            style.name = format!("{original} {index}");
            index += 1;
        }
        if from_selection {
            state.editor.populate_style_from_selection(&mut style)?;
            // A direct list reference is part of the sample's numbering instance.
            style.paragraph.list_id = None;
            style.paragraph.list_level = None;
        }
    }
    ribbon.edit_style_with_parent(style, parent)?;
    ribbon.styles_revision.set(None);
    ribbon.refresh_styles_pane();
    Ok(())
}

fn labels(strings: &[&str]) -> Vec<String> {
    strings.iter().map(|s| text(s)).collect()
}

pub(super) fn find_replace(
    ribbon: &Ribbon,
    parent: &impl WxWidget,
    id: &StyleId,
) -> Result<Option<editor::StyleMatch>, String> {
    let (name, kind, uses, targets, revision) = {
        let state = ribbon.state.borrow();
        let editor = &state.editor;
        let sheet = &editor.document.styles;
        let style = sheet.get(id).ok_or("Style not found")?;
        let mut targets = sheet
            .styles
            .values()
            .filter(|s| s.kind == style.kind && s.id != *id)
            .cloned()
            .collect::<Vec<_>>();
        targets.sort_by_key(|s| s.name.to_lowercase());
        (
            style_name(style),
            style.kind,
            editor.style_matches(id),
            targets,
            editor.revision,
        )
    };
    if uses.is_empty() {
        info(
            parent,
            "Find and Replace Styles",
            "No uses of this style were found.",
        );
        return Ok(None);
    }
    let window = DialogBuilder::new(parent, &text("Find and Replace Styles"))
        .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
        .build();
    let root = BoxSizer::builder(Orientation::Vertical).build();
    let margin = window.from_dip_int(12);
    let prompt = StaticText::builder(&window)
        .with_label(&format!("{name} · {} {}", text("Matches"), uses.len()))
        .build();
    root.add(&prompt, 0, SizerFlag::All, margin);
    let matches = ListBox::builder(&window)
        .with_size(window.from_dip(Size::new(510, 220)))
        .with_style(ListBoxStyle::Default | ListBoxStyle::HorizontalScrollbar)
        .build();
    matches.set_name(&text("Matches"));
    for (index, item) in uses.iter().enumerate() {
        let sample = if item.preview.is_empty() {
            text("Empty paragraph")
        } else {
            item.preview.clone()
        };
        matches.append(&format!("{}. {sample}", index + 1));
    }
    matches.set_selection(0, true);
    root.add(
        &matches,
        1,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
        margin,
    );
    let mut choices = labels(&["Choose the replacement style."]);
    let replacement = Choice::builder(&window)
        .with_choices({
            choices.extend(targets.iter().map(style_name));
            choices
        })
        .build();
    replacement.set_name(&text("Replace with Style"));
    replacement.set_selection(0);
    root.add(&replacement, 0, SizerFlag::All | SizerFlag::Expand, margin);
    let actions = BoxSizer::builder(Orientation::Horizontal).build();
    const GOTO: i32 = 32_183;
    const REPLACE: i32 = 32_184;
    const REPLACE_ALL: i32 = 32_185;
    let mut buttons = Vec::new();
    for (label, id) in [
        ("Go to selection", GOTO),
        ("Replace", REPLACE),
        ("Replace All", REPLACE_ALL),
        ("Close", ID_CANCEL),
    ] {
        let button = Button::builder(&window)
            .with_id(id)
            .with_label(&text(label))
            .build();
        if id == REPLACE || id == REPLACE_ALL {
            button.enable(false);
        }
        if id != ID_CANCEL {
            button.on_click(move |_| window.end_modal(id));
        }
        actions.add(&button, 0, SizerFlag::Right, window.from_dip_int(5));
        buttons.push(button);
    }
    let (replace, all) = (buttons[1], buttons[2]);
    replacement.on_selection_changed(move |_| {
        let ready = replacement.get_selection().is_some_and(|index| index > 0);
        replace.enable(ready);
        all.enable(ready);
    });
    matches.on_item_double_clicked(move |_| window.end_modal(GOTO));
    root.add_sizer(
        &actions,
        0,
        SizerFlag::All | SizerFlag::AlignCenterHorizontal,
        margin,
    );
    window.set_sizer_and_fit(root, true);
    window.set_escape_id(ID_CANCEL);
    crate::placement::centre(&window);
    matches.set_focus();
    let action = window.show_modal();
    let selected = matches
        .get_selection()
        .and_then(|index| uses.get(index as usize));
    let target = replacement
        .get_selection()
        .and_then(|i| i.checked_sub(1))
        .map(|i| i as usize);
    window.destroy();
    if action == GOTO {
        return Ok(selected.cloned());
    }
    if action != REPLACE && action != REPLACE_ALL {
        return Ok(None);
    }
    let Some(target) = target else {
        return Ok(None);
    };
    let one = if action == REPLACE {
        let Some(selected) = selected else {
            return Ok(None);
        };
        Some(selected)
    } else {
        None
    };
    let target = &targets[target].id;
    let mut count = 0;
    ribbon.edit_without_focus(|editor| {
        if editor.revision != revision
            || editor
                .document
                .styles
                .get(target)
                .is_none_or(|s| s.kind != kind)
        {
            return Err("The document changed; search again before replacing".into());
        }
        count = editor.replace_style_uses(id, target, one)?;
        Ok(())
    })?;
    info(
        parent,
        "Find and Replace Styles",
        &format!("{}: {count}", text("Replaced matches")),
    );
    Ok(None)
}

pub(super) fn navigate(ribbon: &Ribbon, item: editor::StyleMatch) {
    ribbon.navigate_entry(navigation::Entry {
        label: item.preview,
        node: item.node,
        start: item.range.start,
        end: item.range.end,
    });
}

fn info(parent: &impl WxWidget, title: &str, message: &str) {
    let dialog = MessageDialog::builder(parent, &text(message), &text(title)).build();
    dialog.show_modal();
    dialog.destroy();
}

fn confirm(parent: &impl WxWidget, title: &str, message: &str) -> bool {
    let dialog = MessageDialog::builder(parent, &text(message), &text(title))
        .with_style(crate::cautious_question())
        .build();
    let ok = dialog.show_modal() == wxdragon::id::ID_YES;
    dialog.destroy();
    ok
}

const SET_EXTENSION: &str = "impress-styles";
const SET_LIMIT: u64 = 16 * 1024 * 1024;

fn choose_style_file(
    parent: &impl WxWidget,
    save: bool,
    documents: bool,
    suggested: &str,
) -> Option<PathBuf> {
    let title = if save {
        "Save Style Set"
    } else {
        "Import Styles"
    };
    let wildcard = if documents {
        "Impress style sets (*.impress-styles)|*.impress-styles|Impress documents (*.impress)|*.impress|Word documents (*.docx;*.dotx)|*.docx;*.dotx"
    } else {
        "Impress style sets (*.impress-styles)|*.impress-styles"
    };
    let style = if save {
        FileDialogStyle::Save | FileDialogStyle::OverwritePrompt
    } else {
        FileDialogStyle::Open | FileDialogStyle::FileMustExist
    };
    let dialog = FileDialog::builder(parent)
        .with_message(&text(title))
        .with_wildcard(&crate::i18n::wildcard(wildcard))
        .with_default_file(suggested)
        .with_style(style)
        .build();
    let path = (dialog.show_modal() == ID_OK)
        .then(|| dialog.get_path())
        .flatten()
        .map(PathBuf::from);
    dialog.destroy();
    path.map(|path| {
        if save && path.extension().is_none() {
            path.with_extension(SET_EXTENSION)
        } else {
            path
        }
    })
}

fn portable_sheet(document: &model::Document) -> StyleSheet {
    let mut sheet = document.styles.clone();
    // Word can bring its own Normal, Heading 1 and Title alongside Impress's
    // fallback definitions. The imported definition is the one to share.
    for builtin in [
        model::STYLE_NORMAL,
        model::STYLE_HEADING_1,
        model::STYLE_TITLE,
    ] {
        let builtin = StyleId::from(builtin);
        let preferred = sheet
            .get(&builtin)
            .and_then(|fallback| {
                sheet.styles.values().find(|style| {
                    style.id != builtin
                        && style.kind == fallback.kind
                        && style.name.eq_ignore_ascii_case(&fallback.name)
                })
            })
            .map(|style| style.id.clone());
        if let Some(preferred) = preferred {
            let fallback = sheet.styles.remove(&builtin).unwrap();
            if sheet.styles[&preferred].based_on.as_ref() == Some(&builtin) {
                sheet.styles.get_mut(&preferred).unwrap().based_on =
                    fallback.based_on.filter(|id| id != &preferred);
            }
            for style in sheet.styles.values_mut() {
                if style.based_on.as_ref() == Some(&builtin) {
                    style.based_on = Some(preferred.clone());
                }
                if style.next_style.as_ref() == Some(&builtin) {
                    style.next_style = Some(preferred.clone());
                }
            }
            if let Some(link) = sheet.linked_styles.remove(&builtin) {
                sheet.linked_styles.entry(preferred.clone()).or_insert(link);
            }
            let already_numbered = sheet.linked_list(&preferred).is_some();
            for list in sheet.list_styles.values_mut() {
                list.headings
                    .retain(|_, heading| !already_numbered || heading != &builtin);
                for heading in list
                    .headings
                    .values_mut()
                    .filter(|heading| **heading == builtin)
                {
                    *heading = preferred.clone();
                }
            }
        }
    }
    for style in sheet.styles.values() {
        if let Some(list) = style
            .paragraph
            .list_id
            .as_ref()
            .and_then(|id| document.lists.get(id))
        {
            sheet
                .list_styles
                .entry(list.id.clone())
                .or_insert_with(|| NamedListStyle {
                    name: format!("{} · {}", text("Numbering"), list.id),
                    definition: list.clone(),
                    headings: BTreeMap::new(),
                });
        }
    }
    sheet
}

fn read_set(ribbon: &Ribbon, path: &Path) -> Result<StyleSheet, String> {
    if path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case(SET_EXTENSION))
    {
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        file.take(SET_LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > SET_LIMIT {
            return Err("The style set is too large.".into());
        }
        return serde_json::from_slice(&bytes).map_err(|e| e.to_string());
    }
    let formats = ribbon.state.borrow().formats.clone();
    // DOTX uses DOCX's package structure and style parts.
    let document = if path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("dotx"))
    {
        let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        formats
            .for_path(Path::new("template.docx"))
            .ok_or("No Word importer is available.")?
            .import(&mut file)
            .map_err(|e| e.to_string())?
    } else {
        crate::file_io::load_document(path, &formats)?
    };
    Ok(portable_sheet(&document))
}

pub(super) fn style_sets(
    ribbon: &Ribbon,
    parent: &impl WxWidget,
    selected: Option<&StyleId>,
) -> Result<(), String> {
    let actions = labels(&[
        "Save document styles as a set…",
        "Save selected style…",
        "Import a style set…",
        "Import one style from a file…",
        "Use these styles for new documents",
        "Restore built-in styles for new documents",
    ]);
    let Some(action) = input_dialog::choice(parent, "Style Sets", "", &actions) else {
        return Ok(());
    };
    match action {
        0 | 1 | 4 => {
            let sheet = portable_sheet(&ribbon.state.borrow().editor.document);
            if action == 4 {
                if confirm(
                    parent,
                    "New Document Styles",
                    "Use this document's style definitions as the default for new documents?",
                ) {
                    crate::document_defaults::set_default_styles(Some(sheet))?;
                }
                return Ok(());
            }
            let sheet = if action == 1 {
                let id = selected
                    .filter(|id| sheet.get(id).is_some())
                    .ok_or("Select a style first.")?;
                editor::style_subset(&sheet, id)
            } else {
                sheet
            };
            editor::validate_styles(&sheet)?;
            editor::validate_extras(&sheet)?;
            let default = format!("{}.{}", text("Style Set"), SET_EXTENSION);
            let Some(path) = choose_style_file(parent, true, false, &default) else {
                return Ok(());
            };
            let bytes = serde_json::to_vec_pretty(&sheet).map_err(|e| e.to_string())?;
            if bytes.len() as u64 > SET_LIMIT {
                return Err("The style set is too large.".into());
            }
            format::atomic_write(&path, |file| {
                file.write_all(&bytes).map_err(|e| e.to_string())
            })?;
        }
        2 | 3 => {
            let Some(path) = choose_style_file(parent, false, true, "") else {
                return Ok(());
            };
            let mut sheet = read_set(ribbon, &path)?;
            if action == 3 {
                let mut styles = sheet.styles.values().collect::<Vec<_>>();
                styles.sort_by_key(|style| style.name.to_lowercase());
                let names = styles
                    .iter()
                    .map(|style| style_name(style))
                    .collect::<Vec<_>>();
                let Some(index) = input_dialog::choice(
                    parent,
                    "Import Styles",
                    "Choose a style to import with its dependencies.",
                    &names,
                ) else {
                    return Ok(());
                };
                sheet = editor::style_subset(&sheet, &styles[index].id);
            }
            let collisions = {
                let state = ribbon.state.borrow();
                let current = &state.editor.document.styles;
                let same = |a: &str, b: &str| a.to_lowercase() == b.to_lowercase();
                sheet.styles.values().any(|style| {
                    current
                        .styles
                        .values()
                        .any(|other| same(&style.name, &other.name))
                }) || sheet.table_styles.values().any(|style| {
                    current
                        .table_styles
                        .values()
                        .any(|other| same(&style.name, &other.name))
                }) || sheet.list_styles.values().any(|style| {
                    current
                        .list_styles
                        .values()
                        .any(|other| same(&style.name, &other.name))
                })
            };
            if !collisions
                || confirm(
                    parent,
                    "Import Styles",
                    "Replace definitions with matching names and add the new styles?",
                )
            {
                ribbon.edit_without_focus(|editor| editor.import_style_sheet(&sheet))?;
            }
        }
        5 => {
            if confirm(
                parent,
                "New Document Styles",
                "Restore the built-in styles for future documents?",
            ) {
                crate::document_defaults::set_default_styles(None)?;
            }
        }
        _ => (),
    }
    Ok(())
}

pub(super) fn linked_styles(
    ribbon: &Ribbon,
    parent: &impl WxWidget,
    selected: Option<&StyleId>,
) -> Result<(), String> {
    let sheet = ribbon.state.borrow().editor.document.styles.clone();
    let mut paragraphs = sheet
        .styles
        .values()
        .filter(|s| s.kind == StyleKind::Paragraph)
        .collect::<Vec<_>>();
    let mut characters = sheet
        .styles
        .values()
        .filter(|s| s.kind == StyleKind::Character)
        .collect::<Vec<_>>();
    paragraphs.sort_by_key(|s| (Some(&s.id) != selected, s.name.to_lowercase()));
    characters.sort_by_key(|s| s.name.to_lowercase());
    let names = paragraphs
        .iter()
        .map(|style| style_name(style))
        .collect::<Vec<_>>();
    let Some(index) =
        input_dialog::choice(parent, "Linked Styles", "Choose a paragraph style.", &names)
    else {
        return Ok(());
    };
    let paragraph = paragraphs[index];
    let mut options = labels(&["Create linked character style…", "Unlink styles"]);
    options.extend(characters.iter().map(|style| style_name(style)));
    let prompt = sheet
        .linked_styles
        .get(&paragraph.id)
        .and_then(|id| sheet.get(id))
        .map(|style| format!("{} · {}", style_name(paragraph), style_name(style)))
        .unwrap_or_else(|| style_name(paragraph));
    let Some(choice) = input_dialog::choice(parent, "Linked Styles", &prompt, &options) else {
        return Ok(());
    };
    if choice == 0 {
        let suggested = format!("{} {}", paragraph.name, text("Characters"));
        let Some(name) = input_dialog::text(parent, "Linked Styles", "Name:", &suggested, false)
        else {
            return Ok(());
        };
        ribbon.edit_without_focus(|editor| {
            let mut sheet = editor.document.styles.clone();
            let id = StyleId::new();
            sheet.styles.insert(
                id.clone(),
                Style {
                    id: id.clone(),
                    name: name.trim().into(),
                    kind: StyleKind::Character,
                    based_on: None,
                    next_style: None,
                    character: Default::default(),
                    paragraph: Default::default(),
                    formatting: Default::default(),
                },
            );
            sheet.linked_styles.insert(paragraph.id.clone(), id);
            editor.install_style_sheet(sheet)
        })?;
    } else if choice == 1 {
        ribbon.edit_without_focus(|editor| {
            let mut sheet = editor.document.styles.clone();
            if let Some(id) = sheet.linked_styles.get(&paragraph.id).cloned() {
                let effective = sheet.resolve_character(Some(&id));
                sheet.linked_styles.remove(&paragraph.id);
                if let Some(character) = sheet.styles.get_mut(&id) {
                    character.character = effective;
                }
            }
            editor.install_style_sheet(sheet)
        })?;
    } else {
        ribbon.edit_without_focus(|editor| {
            editor.link_style(&paragraph.id, &characters[choice - 2].id)
        })?;
    }
    Ok(())
}

struct Form {
    window: Dialog,
    root: BoxSizer,
}

impl Form {
    fn new(parent: &impl WxWidget, title: &str) -> Self {
        let window = DialogBuilder::new(parent, &text(title))
            .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
            .build();
        let root = BoxSizer::builder(Orientation::Vertical).build();
        Self { window, root }
    }

    fn row(&self, label: &str, field: &impl WxWidget) {
        let row = BoxSizer::builder(Orientation::Horizontal).build();
        let name = StaticText::builder(&self.window)
            .with_label(&text(label))
            .build();
        name.set_min_size(Size::new(self.window.from_dip_int(166), -1));
        row.add(
            &name,
            0,
            SizerFlag::AlignCenterVertical | SizerFlag::Right,
            self.window.from_dip_int(8),
        );
        row.add(field, 1, SizerFlag::Expand, 0);
        field.set_name(&text(label));
        self.root.add_sizer(
            &row,
            0,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
            self.window.from_dip_int(12),
        );
    }

    fn text(&self, label: &str, value: &str) -> TextCtrl {
        let field = TextCtrl::builder(&self.window)
            .with_size(Size::new(self.window.from_dip_int(220), -1))
            .with_value(value)
            .build();
        self.row(label, &field);
        field
    }

    fn choice(&self, label: &str, choices: Vec<String>, index: usize) -> Choice {
        let field = Choice::builder(&self.window).with_choices(choices).build();
        field.set_selection(index as u32);
        self.row(label, &field);
        field
    }

    fn show(&self) -> bool {
        let margin = self.window.from_dip_int(12);
        dialog_actions::divider(&self.window, &self.root, margin);
        let row = BoxSizer::builder(Orientation::Horizontal).build();
        let ok = Button::builder(&self.window)
            .with_id(ID_OK)
            .with_label(&text("OK"))
            .build();
        let cancel = Button::builder(&self.window)
            .with_id(ID_CANCEL)
            .with_label(&text("Cancel"))
            .build();
        dialog_actions::pair(&row, ok, cancel);
        self.root
            .add_sizer(&row, 0, SizerFlag::All | SizerFlag::Expand, margin);
        let window = self.window;
        window.set_sizer_and_fit(self.root, true);
        window.set_escape_id(ID_CANCEL);
        ok.set_default();
        crate::placement::centre(&window);
        window.show_modal() == ID_OK
    }
}

impl Drop for Form {
    fn drop(&mut self) {
        self.window.destroy();
    }
}

pub(super) fn table_styles(ribbon: &Ribbon, parent: &impl WxWidget) -> Result<(), String> {
    let mut entries = ribbon
        .state
        .borrow()
        .editor
        .document
        .styles
        .table_styles
        .clone()
        .into_iter()
        .collect::<Vec<_>>();
    entries.sort_by_key(|(_, style)| style.name.to_lowercase());
    let mut names = labels(&["New table style…", "Remove style from current table"]);
    names.extend(entries.iter().map(|(_, style)| style.name.clone()));
    let Some(index) = input_dialog::choice(parent, "Table Styles", "", &names) else {
        return Ok(());
    };
    if index == 1 {
        return ribbon.edit_without_focus(|editor| editor.apply_table_style(None));
    }
    let (id, mut style) = if index == 0 {
        (
            StyleId::new(),
            TableStyle {
                name: text("New table style"),
                body_fill: None,
                header_fill: Some("#DCE8F8".into()),
                band_fill: Some("#F2F6FC".into()),
                borders: None,
            },
        )
    } else {
        let entry = &entries[index - 2];
        match input_dialog::choice(
            parent,
            "Table Styles",
            &entry.1.name,
            &labels(&["Apply to current table", "Modify…"]),
        ) {
            Some(0) => {
                return ribbon.edit_without_focus(|editor| editor.apply_table_style(Some(&entry.0)));
            }
            Some(1) => entry.clone(),
            _ => return Ok(()),
        }
    };
    let form = Form::new(parent, "Table Style");
    let name = form.text("Name:", &style.name);
    let body = form.text(
        "Body fill (#RRGGBB)",
        style.body_fill.as_deref().unwrap_or(""),
    );
    let header = form.text(
        "First-row fill (#RRGGBB)",
        style.header_fill.as_deref().unwrap_or(""),
    );
    let band = form.text(
        "Band fill (#RRGGBB)",
        style.band_fill.as_deref().unwrap_or(""),
    );
    let prior = style
        .borders
        .as_ref()
        .and_then(|borders| borders.top.as_ref());
    let border = form.text(
        "Border color (#RRGGBB)",
        prior.map(|b| b.color.as_str()).unwrap_or(""),
    );
    let width = form.text(
        "Border width (pt)",
        &prior.map(|b| b.width_pt).unwrap_or(0.5).to_string(),
    );
    if !form.show() {
        return Ok(());
    }
    let optional = |field: TextCtrl| {
        let v = field.get_value().trim().to_owned();
        (!v.is_empty()).then_some(v)
    };
    style.name = name.get_value().trim().to_owned();
    style.body_fill = optional(body);
    style.header_fill = optional(header);
    style.band_fill = optional(band);
    if optional(border).as_deref() != prior.map(|b| b.color.as_str())
        || width.get_value().parse::<f32>().ok() != prior.map(|b| b.width_pt)
    {
        style.borders = optional(border)
            .map(|color| {
                let width_pt = width
                    .get_value()
                    .parse::<f32>()
                    .map_err(|_| "Enter a border width in points.")?;
                let border = model::Border {
                    color,
                    width_pt,
                    style: model::BorderStyle::Single,
                };
                Ok::<_, &str>(model::BorderSet {
                    top: Some(border.clone()),
                    right: Some(border.clone()),
                    bottom: Some(border.clone()),
                    left: Some(border.clone()),
                    inside_horizontal: Some(border.clone()),
                    inside_vertical: Some(border),
                })
            })
            .transpose()?;
    }
    ribbon.edit_without_focus(|editor| editor.put_table_style(id, style))
}

const FORMATS: [(ListFormat, &str); 6] = [
    (ListFormat::Decimal, "Decimal"),
    (ListFormat::Bullet, "Bullets"),
    (ListFormat::UpperLetter, "Uppercase letters"),
    (ListFormat::LowerLetter, "Lowercase letters"),
    (ListFormat::UpperRoman, "Uppercase Roman"),
    (ListFormat::LowerRoman, "Lowercase Roman"),
];

fn default_level(level: u8) -> ListLevel {
    let text = (1..=level + 1).map(|i| format!("%{i}.")).collect();
    ListLevel {
        level,
        format: ListFormat::Decimal,
        text,
        start_at: 1,
        left_indent_mm: 6.35 + 6.35 * f32::from(level),
        hanging_indent_mm: 3.18,
    }
}

pub(super) fn list_styles(ribbon: &Ribbon, parent: &impl WxWidget) -> Result<(), String> {
    let sheet = ribbon.state.borrow().editor.document.styles.clone();
    let mut entries = sheet.list_styles.values().cloned().collect::<Vec<_>>();
    entries.sort_by_key(|style| style.name.to_lowercase());
    let mut names = labels(&["New list style…"]);
    names.extend(entries.iter().map(|style| style.name.clone()));
    let Some(index) = input_dialog::choice(parent, "List Styles", "", &names) else {
        return Ok(());
    };
    let mut draft = if index == 0 {
        let Some(name) = input_dialog::text(
            parent,
            "New List Style",
            "Name:",
            &text("New list style"),
            false,
        ) else {
            return Ok(());
        };
        NamedListStyle {
            name: name.trim().into(),
            definition: ListDefinition {
                id: NodeId::new(),
                levels: vec![default_level(0)],
            },
            headings: BTreeMap::new(),
        }
    } else {
        let entry = &entries[index - 1];
        match input_dialog::choice(
            parent,
            "List Styles",
            &entry.name,
            &labels(&["Apply to selection…", "Modify…"]),
        ) {
            Some(0) => {
                let choices = entry
                    .definition
                    .levels
                    .iter()
                    .map(|level| format!("{} {}", text("Level"), u16::from(level.level) + 1))
                    .collect::<Vec<_>>();
                let Some(level) =
                    input_dialog::choice(parent, "List Styles", "Choose a list level.", &choices)
                else {
                    return Ok(());
                };
                return ribbon.edit_without_focus(|editor| {
                    editor.apply_list_style(
                        &entry.definition.id,
                        entry.definition.levels[level].level,
                    )
                });
            }
            Some(1) => entry.clone(),
            _ => return Ok(()),
        }
    };
    let mut paragraphs = sheet
        .styles
        .values()
        .filter(|s| s.kind == StyleKind::Paragraph)
        .collect::<Vec<_>>();
    paragraphs.sort_by_key(|s| s.name.to_lowercase());
    loop {
        let actions = labels(&["Save", "Rename…"]);
        let mut actions = actions
            .into_iter()
            .chain(draft.definition.levels.iter().map(|level| {
                let label = format!(
                    "{} {} · {}",
                    text("Level"),
                    u16::from(level.level) + 1,
                    level.text
                );
                draft
                    .headings
                    .get(&level.level)
                    .and_then(|id| sheet.get(id))
                    .map(|style| format!("{label} · {}", style_name(style)))
                    .unwrap_or(label)
            }))
            .collect::<Vec<_>>();
        let add = draft.definition.levels.len() < 9;
        if add {
            actions.push(text("Add level…"));
        }
        let Some(index) = input_dialog::choice(parent, "List Style", &draft.name, &actions) else {
            return Ok(());
        };
        match index {
            0 => return ribbon.edit_without_focus(|editor| editor.put_list_style(draft)),
            1 => {
                if let Some(name) =
                    input_dialog::text(parent, "List Style", "Name:", &draft.name, false)
                {
                    draft.name = name.trim().into();
                }
            }
            _ => {
                let slot = index - 2;
                let existing = draft.definition.levels.get(slot).cloned();
                let Some(next) = (0..9)
                    .find(|i| {
                        !draft
                            .definition
                            .levels
                            .iter()
                            .any(|level| level.level == *i)
                    })
                    .or_else(|| existing.as_ref().map(|level| level.level))
                else {
                    continue;
                };
                let mut level = existing.unwrap_or_else(|| default_level(next));
                let form = Form::new(parent, "List Level");
                let format = form.choice(
                    "Number format",
                    labels(&FORMATS.map(|(_, label)| label)),
                    FORMATS
                        .iter()
                        .position(|(format, _)| *format == level.format)
                        .unwrap_or(0),
                );
                let pattern = form.text("Number pattern or bullet", &level.text);
                let start = form.text("Start at", &level.start_at.to_string());
                let indent = form.text("Left indent (mm)", &level.left_indent_mm.to_string());
                let hanging =
                    form.text("Hanging indent (mm)", &level.hanging_indent_mm.to_string());
                let mut names = labels(&["None"]);
                names.extend(paragraphs.iter().map(|style| style_name(style)));
                let linked = form.choice(
                    "Link to paragraph style",
                    names,
                    draft
                        .headings
                        .get(&level.level)
                        .and_then(|id| paragraphs.iter().position(|style| style.id == *id))
                        .map(|i| i + 1)
                        .unwrap_or(0),
                );
                if !form.show() {
                    continue;
                }
                level.format = FORMATS[format.get_selection().unwrap_or(0) as usize].0;
                level.text = pattern.get_value();
                if level.format == ListFormat::Bullet
                    && (level.text.is_empty() || level.text.contains('%'))
                {
                    level.text = "•".into();
                }
                let parsed = (
                    start.get_value().parse(),
                    indent.get_value().parse(),
                    hanging.get_value().parse(),
                );
                let (Ok(start), Ok(indent), Ok(hanging)) = parsed else {
                    info(
                        parent,
                        "List Level",
                        "Enter a number for start and both indents.",
                    );
                    continue;
                };
                if !(1..=999_999).contains(&start)
                    || !(0.0..=200.0).contains(&indent)
                    || !(0.0..=200.0).contains(&hanging)
                    || level.text.len() > 128
                    || level.text.chars().any(char::is_control)
                {
                    info(
                        parent,
                        "List Level",
                        "Invalid list level, numbering pattern or indentation.",
                    );
                    continue;
                }
                level.start_at = start;
                level.left_indent_mm = indent;
                level.hanging_indent_mm = hanging;
                let linked = linked
                    .get_selection()
                    .and_then(|i| i.checked_sub(1))
                    .map(|i| &paragraphs[i as usize].id);
                draft.headings.remove(&level.level);
                if let Some(id) = linked {
                    draft.headings.retain(|_, old| old != id);
                    draft.headings.insert(level.level, id.clone());
                }
                if slot == draft.definition.levels.len() {
                    draft.definition.levels.push(level);
                } else {
                    draft.definition.levels[slot] = level;
                }
                draft.definition.levels.sort_by_key(|level| level.level);
            }
        }
    }
}

pub(super) fn inspect(ribbon: &Ribbon, parent: &impl WxWidget) {
    let Some(snapshot) = ribbon.state.borrow().editor.inspect_styles() else {
        return;
    };
    let mut report = format!(
        "{}\n{}  {}\n{}  {}\n\n",
        text("Formatting at the start of the selection"),
        text("Paragraph style"),
        snapshot
            .paragraph_style
            .as_ref()
            .map(style_name)
            .unwrap_or_else(|| text("None")),
        text("Character style"),
        snapshot
            .character_style
            .as_ref()
            .map(style_name)
            .unwrap_or_else(|| text("None"))
    );
    append_properties(
        &mut report,
        "Paragraph formatting",
        snapshot.inherited_paragraph,
        snapshot.direct_paragraph,
    );
    append_properties(
        &mut report,
        "Character formatting",
        snapshot.inherited_character,
        snapshot.direct_character,
    );
    let form = Form::new(parent, "Style Inspector");
    let output = TextCtrl::builder(&form.window)
        .with_style(TextCtrlStyle::MultiLine | TextCtrlStyle::ReadOnly)
        .with_size(form.window.from_dip(Size::new(560, 360)))
        .with_value(&report)
        .build();
    output.set_name(&text("Style Inspector"));
    form.root.add(
        &output,
        1,
        SizerFlag::All | SizerFlag::Expand,
        form.window.from_dip_int(12),
    );
    form.show();
}

fn append_properties<T: serde::Serialize>(
    report: &mut String,
    heading: &str,
    inherited: T,
    direct: T,
) {
    use serde_json::Value;
    let Ok(Value::Object(inherited)) = serde_json::to_value(inherited) else {
        return;
    };
    let Ok(Value::Object(direct)) = serde_json::to_value(direct) else {
        return;
    };
    report.push_str(&text(heading));
    report.push('\n');
    let keys = inherited
        .keys()
        .chain(direct.keys())
        .collect::<std::collections::BTreeSet<_>>();
    for key in keys {
        let inherited = inherited.get(key).filter(|value| !value.is_null());
        let direct = direct.get(key).filter(|value| !value.is_null());
        let show = |value: &Value| match value {
            Value::String(s) => text(&s.replace('_', " ")),
            Value::Bool(value) => text(if *value { "On" } else { "Off" }),
            Value::Object(map) => map
                .iter()
                .filter(|(_, v)| !v.is_null())
                .map(|(k, v)| format!("{} {v}", k.replace('_', " ")))
                .collect::<Vec<_>>()
                .join(", "),
            other => other.to_string(),
        };
        if let Some(value) = direct.or(inherited) {
            let name = key.replace('_', " ");
            let source = if direct.is_some() {
                text("Direct formatting")
            } else {
                text("From style")
            };
            report.push_str(&format!("{}  {}  ({source})", text(&name), show(value)));
            if let Some(base) = inherited.filter(|base| direct.is_some_and(|v| v != *base)) {
                report.push_str(&format!("   {}  {}", text("Style value"), show(base)));
            }
            report.push('\n');
        }
    }
    report.push('\n');
}
