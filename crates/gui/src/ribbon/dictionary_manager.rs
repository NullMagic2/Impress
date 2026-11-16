//! Installed dictionary inventory and explicit add/delete actions.
use super::*;
use crate::dictionary_install::{self, InstalledDictionary, Inventory, Operation, Scope};
use wxdragon::{
    dialogs::{Dialog, DialogBuilder, DialogStyle},
    id::ID_CANCEL,
    sizers::{BoxSizer, Orientation, SizerFlag},
    widgets::{
        combobox::ComboBoxStyle,
        list_ctrl::{ListColumnFormat, ListCtrl, ListCtrlStyle, ListItemState},
        static_text::StaticText,
    },
};

/// Minimum manager size and spacing, in logical display-independent pixels.
const WIDTH_DIP: i32 = 690;
const HEIGHT_DIP: i32 = 420;
const GAP_DIP: i32 = 12;
const LANGUAGE_COLUMN_DIP: i32 = 360;
const SCOPE_COLUMN_DIP: i32 = 290;
const STATUS_HEIGHT_DIP: i32 = 45;
const ADD_WIDTH_DIP: i32 = 420;

pub(super) struct Manager {
    ribbon: std::rc::Weak<Ribbon>,
    window: Dialog,
    list: ListCtrl,
    status: StaticText,
    add: Button,
    delete: Button,
    refresh_button: Button,
    rows: RefCell<Vec<InstalledDictionary>>,
    installed: RefCell<Vec<InstalledDictionary>>,
    discovery: RefCell<Option<Receiver<Result<Inventory, String>>>>,
    refresh_again: Cell<bool>,
}

impl Manager {
    fn new(ribbon: &Rc<Ribbon>) -> Rc<Self> {
        let window = DialogBuilder::new(&ribbon.ui.main_frame, &crate::i18n::text(&("Spelling dictionaries")))
            .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
            .build();
        let dip = |v| window.from_dip_int(v);
        window.set_size(window.from_dip(Size::new(WIDTH_DIP, HEIGHT_DIP)));
        window.set_min_size(window.from_dip(Size::new(WIDTH_DIP, HEIGHT_DIP)));
        window.set_escape_id(ID_CANCEL);
        let heading = StaticText::builder(&window)
            .with_label(&crate::i18n::text(&("Installed spelling dictionaries")))
            .build();
        let list = ListCtrl::builder(&window)
            .with_style(ListCtrlStyle::Report | ListCtrlStyle::SingleSel | ListCtrlStyle::NoSort)
            .build();
        for (index, (label, width)) in [
            ("Language", LANGUAGE_COLUMN_DIP),
            ("Location", SCOPE_COLUMN_DIP),
        ]
        .into_iter()
        .enumerate()
        {
            list.insert_column(index as i64, &crate::i18n::text(&(label)), ListColumnFormat::Left, dip(width));
        }
        let status = StaticText::builder(&window)
            .with_label(&crate::i18n::text(&("Loading dictionaries…")))
            .with_size(Size::new(-1, dip(STATUS_HEIGHT_DIP)))
            .build();
        let add = Button::builder(&window).with_label(&crate::i18n::text(&("&Add…"))).build();
        let delete = Button::builder(&window).with_label(&crate::i18n::text(&("&Delete…"))).build();
        let refresh_button = Button::builder(&window).with_label(&crate::i18n::text(&("&Refresh"))).build();
        let close = Button::builder(&window)
            .with_id(ID_CANCEL)
            .with_label(&crate::i18n::text(&("Close")))
            .build();
        close.set_default();
        let root = BoxSizer::builder(Orientation::Vertical).build();
        root.add(
            &heading,
            0,
            SizerFlag::All | SizerFlag::Expand,
            dip(GAP_DIP),
        );
        root.add(
            &list,
            1,
            SizerFlag::Left | SizerFlag::Right | SizerFlag::Expand,
            dip(GAP_DIP),
        );
        root.add(&status, 0, SizerFlag::All | SizerFlag::Expand, dip(GAP_DIP));
        let buttons = BoxSizer::builder(Orientation::Horizontal).build();
        for button in [add, delete, refresh_button] {
            buttons.add(&button, 0, SizerFlag::Right, dip(GAP_DIP));
        }
        buttons.add_stretch_spacer(1);
        buttons.add(&close, 0, SizerFlag::empty(), 0);
        root.add_sizer(
            &buttons,
            0,
            SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom | SizerFlag::Expand,
            dip(GAP_DIP),
        );
        window.set_sizer(root, true);
        let this = Rc::new(Self {
            ribbon: Rc::downgrade(ribbon),
            window,
            list,
            status,
            add,
            delete,
            refresh_button,
            rows: RefCell::new(Vec::new()),
            installed: RefCell::new(Vec::new()),
            discovery: RefCell::new(None),
            refresh_again: Cell::new(false),
        });
        for selected in [true, false] {
            let weak = Rc::downgrade(&this);
            if selected {
                list.on_item_selected(move |_| {
                    if let Some(manager) = weak.upgrade() {
                        manager.update_buttons();
                    }
                });
            } else {
                list.on_item_deselected(move |_| {
                    if let Some(manager) = weak.upgrade() {
                        manager.update_buttons();
                    }
                });
            }
        }
        let weak = Rc::downgrade(&this);
        add.on_click(move |_| {
            if let Some(manager) = weak.upgrade() {
                manager.add_dictionary();
            }
        });
        let weak = Rc::downgrade(&this);
        delete.on_click(move |_| {
            if let Some(manager) = weak.upgrade() {
                manager.delete_dictionary();
            }
        });
        let weak = Rc::downgrade(&this);
        refresh_button.on_click(move |_| {
            if let Some(manager) = weak.upgrade() {
                manager.refresh();
            }
        });
        close.on_click(move |_| {
            window.show(false);
        });
        window.on_close(move |event| {
            if let WindowEventData::General(data) = &event {
                if data.can_veto() {
                    data.veto();
                    window.show(false);
                    event.skip(false);
                    return;
                }
            }
            event.skip(true);
        });
        this
    }

    fn selected(&self) -> Option<InstalledDictionary> {
        let index = self.list.get_first_selected_item();
        if index < 0 {
            return None;
        }
        self.rows.borrow().get(index as usize).cloned()
    }

    fn update_buttons(&self) {
        let busy = self.discovery.borrow().is_some()
            || self
                .ribbon
                .upgrade()
                .is_some_and(|r| r.dictionary_operation_busy());
        self.add.enable(!busy);
        self.delete
            .enable(!busy && self.selected().is_some_and(|row| row.removable));
        self.refresh_button.enable(!busy);
    }

    fn message(&self, text: &str) {
        self.status.set_label(&crate::i18n::text(&(text)));
        self.status
            .wrap(self.window.get_client_size().width - self.window.from_dip_int(GAP_DIP * 2));
        self.window.layout();
    }

    fn refresh(&self) {
        if self.discovery.borrow().is_some() {
            self.refresh_again.set(true);
            return;
        }
        let (sender, receiver) = mpsc::channel();
        match std::thread::Builder::new()
            .name("impress-dictionary-inventory".into())
            .spawn(move || {
                let _ = sender.send(dictionary_install::inventory());
            }) {
            Ok(_) => {
                self.discovery.replace(Some(receiver));
                self.message("Loading dictionaries…");
            }
            Err(error) => self.message(&format!("Could not load dictionaries: {error}")),
        }
        self.update_buttons();
    }

    fn poll(&self) {
        let result =
            self.discovery
                .borrow()
                .as_ref()
                .and_then(|receiver| match receiver.try_recv() {
                    Ok(result) => Some(result),
                    Err(TryRecvError::Empty) => None,
                    Err(TryRecvError::Disconnected) => Some(Err(
                        "Dictionary discovery was interrupted. Click Refresh to try again.".into(),
                    )),
                });
        if let Some(result) = result {
            self.discovery.borrow_mut().take();
            match result {
                Ok(mut inventory) => {
                    self.installed.replace(inventory.dictionaries.clone());
                    inventory.dictionaries.retain(|row| row.removable);
                    let selected = self.selected();
                    self.rows.replace(inventory.dictionaries.clone());
                    self.list.delete_all_items();
                    for (index, row) in inventory.dictionaries.iter().enumerate() {
                        self.list.insert_item(
                            index as i64,
                            &crate::spelling::language_label(&row.tag),
                            None,
                        );
                        self.list
                            .set_item_text_by_column(index as i64, 1, &crate::i18n::text(row.scope.label()));
                    }
                    let index = selected
                        .and_then(|old| {
                            inventory
                                .dictionaries
                                .iter()
                                .position(|row| row.tag == old.tag && row.scope == old.scope)
                        })
                        .unwrap_or(0);
                    if !inventory.dictionaries.is_empty() {
                        self.list.set_item_state(
                            index as i64,
                            ListItemState::Selected | ListItemState::Focused,
                            ListItemState::Selected | ListItemState::Focused,
                        );
                    }
                    self.message(inventory.warning.as_deref().unwrap_or(
                        if inventory.dictionaries.is_empty() {
                            "No dictionaries have been installed through Impress. Click Add to install one."
                        } else {
                            "Select a dictionary to delete it, or click Add to install another."
                        },
                    ));
                }
                Err(error) => self.message(&error),
            }
            if self.refresh_again.replace(false) {
                self.refresh();
            }
        }
        self.update_buttons();
    }

    fn show(&self) {
        if !self.window.is_shown() {
            crate::placement::centre(&self.window);
        }
        self.window.show(true);
        self.window.raise();
        self.refresh();
    }

    fn add_dictionary(&self) {
        let Some(ribbon) = self.ribbon.upgrade() else {
            return;
        };
        if ribbon.dictionary_operation_busy() {
            return;
        }
        let selected = self.selected().map(|row| row.tag);
        let installed = self.installed.borrow().clone();
        let Some((tag, scope)) = add_dialog(&self.window, &installed, selected.as_deref()) else {
            return;
        };
        match ribbon.start_dictionary_operation(&tag, scope, Operation::Install) {
            Ok(()) => self.message("Installation started."),
            Err(error) => self.message(&error),
        }
        self.update_buttons();
    }

    fn delete_dictionary(&self) {
        let Some(ribbon) = self.ribbon.upgrade() else {
            return;
        };
        if ribbon.dictionary_operation_busy() {
            return;
        }
        let Some(row) = self.selected().filter(|row| row.removable) else {
            return;
        };
        let language = crate::spelling::language_label(&row.tag);
        let text = match row.scope {
            Scope::Local => format!("Delete the {language} dictionary from your account?"),
            Scope::System if cfg!(windows) => format!(
                "Delete the {language} system dictionary? This removes its Windows Basic typing feature for all users."
            ),
            Scope::System => format!(
                "Delete the {language} system dictionary? Other applications and users will also lose this dictionary."
            ),
        };
        let confirm = MessageDialog::builder(&self.window, &crate::i18n::text(&(&text)), &crate::i18n::text(&("Delete spelling dictionary")))
            .with_style(crate::cautious_question())
            .build();
        confirm.set_yes_no_labels(&crate::i18n::text(&("Delete")), &crate::i18n::text(&("Cancel")));
        crate::placement::centre(&confirm);
        let accepted = confirm.show_modal() == wxdragon::id::ID_YES;
        confirm.destroy();
        if !accepted {
            return;
        }
        match ribbon.start_dictionary_operation(&row.tag, row.scope, Operation::Remove) {
            Ok(()) => self.message("Removal started."),
            Err(error) => self.message(&error),
        }
        self.update_buttons();
    }
}

fn add_dialog(
    parent: &Dialog,
    installed: &[InstalledDictionary],
    selected: Option<&str>,
) -> Option<(String, Scope)> {
    let window = DialogBuilder::new(parent, &crate::i18n::text(&("Add spelling dictionary"))).build();
    let dip = |v| window.from_dip_int(v);
    let root = BoxSizer::builder(Orientation::Vertical).build();
    let language_label = StaticText::builder(&window)
        .with_label(&crate::i18n::text(&("&Language:")))
        .build();
    let languages = ComboBox::builder(&window)
        .with_style(ComboBoxStyle::ReadOnly)
        .with_choices(
            crate::i18n::choices(crate::spelling::LANGUAGES
                .iter()
                .map(|choice| crate::i18n::text(choice.name))
                .collect()),
        )
        .build();
    let selected = selected.unwrap_or("en-US");
    let index = crate::spelling::LANGUAGES
        .iter()
        .position(|choice| choice.tag.eq_ignore_ascii_case(selected))
        .unwrap_or(0);
    languages.set_selection(index as u32);
    let scope_label = StaticText::builder(&window)
        .with_label(&crate::i18n::text(&("&Install for:")))
        .build();
    let scope = ComboBox::builder(&window)
        .with_style(ComboBoxStyle::ReadOnly)
        .with_choices(crate::i18n::choices(vec!["User".into(), "System-wide".into()]))
        .build();
    scope.set_selection(0);
    let status = StaticText::builder(&window)
        .with_label(&crate::i18n::text(&(" ")))
        .with_size(Size::new(dip(ADD_WIDTH_DIP), dip(STATUS_HEIGHT_DIP)))
        .build();
    let add = Button::builder(&window).with_label(&crate::i18n::text(&("Add"))).build();
    let cancel = Button::builder(&window)
        .with_id(ID_CANCEL)
        .with_label(&crate::i18n::text(&("Cancel")))
        .build();
    add.set_default();
    window.set_escape_id(ID_CANCEL);
    root.add(
        &language_label,
        0,
        SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
        dip(GAP_DIP),
    );
    root.add(
        &languages,
        0,
        SizerFlag::All | SizerFlag::Expand,
        dip(GAP_DIP),
    );
    root.add(
        &scope_label,
        0,
        SizerFlag::Left | SizerFlag::Right,
        dip(GAP_DIP),
    );
    root.add(&scope, 0, SizerFlag::All | SizerFlag::Expand, dip(GAP_DIP));
    root.add(
        &status,
        0,
        SizerFlag::Left | SizerFlag::Right | SizerFlag::Expand,
        dip(GAP_DIP),
    );
    let buttons = BoxSizer::builder(Orientation::Horizontal).build();
    buttons.add_stretch_spacer(1);
    buttons.add(&add, 0, SizerFlag::Right, dip(GAP_DIP));
    buttons.add(&cancel, 0, SizerFlag::empty(), 0);
    root.add_sizer(
        &buttons,
        0,
        SizerFlag::All | SizerFlag::Expand,
        dip(GAP_DIP),
    );
    window.set_sizer_and_fit(root, true);
    let installed = installed.to_vec();
    let update: Rc<dyn Fn()> = Rc::new(move || {
        let row = languages
            .get_selection()
            .and_then(|index| crate::spelling::LANGUAGES.get(index as usize));
        let selected_scope = if scope.get_selection() == Some(1) {
            Scope::System
        } else {
            Scope::Local
        };
        let exists = row.is_some_and(|choice| {
            installed
                .iter()
                .any(|row| row.tag == choice.tag && row.scope == selected_scope)
        });
        add.enable(row.is_some() && !exists);
        status.set_label(&crate::i18n::text(&(if exists {
            "This dictionary is already installed in that location."
        } else {
            " "
        })));
    });
    {
        let update = Rc::clone(&update);
        languages.on_selection_changed(move |_| update());
    }
    {
        let update = Rc::clone(&update);
        scope.on_selection_changed(move |_| update());
    }
    update();
    add.on_click(move |_| window.end_modal(ID_OK));
    cancel.on_click(move |_| window.end_modal(ID_CANCEL));
    crate::placement::centre(&window);
    let choice = window.show_modal();
    let result = (choice == ID_OK)
        .then(|| {
            let tag = languages
                .get_selection()
                .and_then(|index| crate::spelling::LANGUAGES.get(index as usize))?
                .tag
                .to_string();
            let selected_scope = if scope.get_selection() == Some(1) {
                Scope::System
            } else {
                Scope::Local
            };
            Some((tag, selected_scope))
        })
        .flatten();
    window.destroy();
    result
}

impl Ribbon {
    pub(super) fn dictionary_manager_language_changed(&self) {
        let previous=self.dictionary_manager.borrow_mut().take();
        let shown=previous.as_ref().is_some_and(|manager|manager.window.is_shown());
        if let Some(manager)=previous {manager.window.destroy();}
        if shown {self.show_dictionary_manager();}
    }
    pub(super) fn show_dictionary_manager(&self) {
        let Some(owner) = self.owner.upgrade() else {
            return;
        };
        let existing = self.dictionary_manager.borrow().clone();
        let manager = existing.unwrap_or_else(|| {
            let manager = Manager::new(&owner);
            self.dictionary_manager.replace(Some(Rc::clone(&manager)));
            manager
        });
        manager.show();
    }

    pub(super) fn refresh_dictionary_manager(&self) {
        if let Some(manager) = self.dictionary_manager.borrow().as_ref() {
            manager.refresh();
        }
    }

    pub(super) fn poll_dictionary_manager(&self) {
        let manager = self.dictionary_manager.borrow().clone();
        if let Some(manager) = manager {
            manager.poll();
        }
    }
}
