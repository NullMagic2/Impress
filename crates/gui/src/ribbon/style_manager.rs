//! Document style configuration opened from the Styles pane footer.
use super::*;
use model::{Style, StyleId, StyleKind};
use wxdragon::{
    dialogs::{Dialog, DialogBuilder, DialogStyle},
    id::ID_CANCEL,
    sizers::{BoxSizer, Orientation, SizerFlag},
    widgets::{listbox::ListBox, static_text::StaticText},
};

const WIDTH_DIP: i32 = 640;
const HEIGHT_DIP: i32 = 420;
const GAP_DIP: i32 = 12;
const BUTTON_WIDTH_DIP: i32 = 206;
const DETAILS_HEIGHT_DIP: i32 = 70;

struct Manager {
    ribbon: std::rc::Weak<Ribbon>,
    window: Dialog,
    list: ListBox,
    details: StaticText,
    modify: Button,
    delete: Button,
    styles: RefCell<Vec<Style>>,
    navigate_to: RefCell<Option<editor::StyleMatch>>,
}

pub(super) fn show(ribbon: &Ribbon) {
    let Some(owner) = ribbon.owner.upgrade() else {
        return;
    };
    let manager = Manager::new(&owner);
    let selected = owner.styles_view.borrow().selected_id();
    manager.refresh(selected.as_ref());
    crate::placement::centre(&manager.window);
    manager.window.show_modal();
    manager.window.destroy();
    owner.styles_revision.set(None);
    owner.refresh_styles_pane();
    let target = manager.navigate_to.borrow_mut().take();
    if let Some(target) = target { super::style_tools::navigate(&owner, target); }
    else { owner.focus_document(); }
}

impl Manager {
    fn new(ribbon: &Rc<Ribbon>) -> Rc<Self> {
        let window = DialogBuilder::new(&ribbon.ui.main_frame, &crate::i18n::text(&("Manage Styles")))
            .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
            .build();
        let dip = |value| window.from_dip_int(value);
        window.set_escape_id(ID_CANCEL);
        let root = BoxSizer::builder(Orientation::Vertical).build();
        let heading = StaticText::builder(&window)
            .with_label(&crate::i18n::text(&("Styles in this document")))
            .build();
        dialog_actions::heading(&heading);
        root.add(&heading, 0, SizerFlag::All, dip(GAP_DIP));
        let body = BoxSizer::builder(Orientation::Horizontal).build();
        let list = ListBox::builder(&window).build();
        body.add(&list, 1, SizerFlag::Expand | SizerFlag::Right, dip(GAP_DIP));
        let actions = BoxSizer::builder(Orientation::Vertical).build();
        let mut buttons = Vec::new();
        let tools_top = BoxSizer::builder(Orientation::Horizontal).build();
        let tools_bottom = BoxSizer::builder(Orientation::Horizontal).build();
        for (index, label) in [
            "New paragraph style…",
            "New character style…",
            "Modify…",
            "Delete…",
            "Defaults…",
            "New from Selection…",
            "Find and Replace…",
            "Style Sets…",
            "Linked Styles…",
            "Table Styles…",
            "List Styles…",
            "Inspect…",
        ].into_iter().enumerate() {
            let mut builder = Button::builder(&window).with_label(&crate::i18n::text(label));
            if index < 5 { builder = builder.with_size(Size::new(dip(BUTTON_WIDTH_DIP), -1)); }
            let button = builder.build();
            match index {
                0..=4 => actions.add(&button, 0, SizerFlag::Expand | SizerFlag::Bottom, dip(GAP_DIP)),
                5..=7 => tools_top.add(&button, 1, SizerFlag::Expand | SizerFlag::Right, dip(5)),
                _ => tools_bottom.add(&button, 1, SizerFlag::Expand | SizerFlag::Right, dip(5)),
            };
            buttons.push(button);
        }
        body.add_sizer(&actions, 0, SizerFlag::Expand, 0);
        root.add_sizer(
            &body,
            1,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
            dip(GAP_DIP),
        );
        let details = StaticText::builder(&window)
            .with_label(&crate::i18n::text(&(" ")))
            .with_size(Size::new(-1, dip(DETAILS_HEIGHT_DIP)))
            .build();
        root.add(
            &details,
            0,
            SizerFlag::Expand | SizerFlag::All,
            dip(GAP_DIP),
        );
        for tools in [tools_top, tools_bottom] {
            root.add_sizer(&tools, 0, SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom, dip(8));
        }
        let close = Button::builder(&window)
            .with_id(ID_CANCEL)
            .with_label(&crate::i18n::text(&("Close")))
            .build();
        close.set_default();
        dialog_actions::size(close);
        dialog_actions::divider(&window, &root, dip(GAP_DIP));
        root.add(
            &close,
            0,
            SizerFlag::AlignCenterHorizontal | SizerFlag::All,
            dip(GAP_DIP),
        );
        window.set_sizer_and_fit(root, true);
        let fitted = window.get_size();
        window.set_size(Size::new(fitted.width.max(dip(WIDTH_DIP)), fitted.height.max(dip(HEIGHT_DIP))));
        let manager = Rc::new(Self {
            ribbon: Rc::downgrade(ribbon),
            window,
            list,
            details,
            modify: buttons[2],
            delete: buttons[3],
            styles: RefCell::new(Vec::new()),
            navigate_to: RefCell::new(None),
        });
        for (button, kind) in [
            (buttons[0], StyleKind::Paragraph),
            (buttons[1], StyleKind::Character),
        ] {
            let weak = Rc::downgrade(&manager);
            button.on_click(move |_| {
                if let Some(manager) = weak.upgrade() {
                    manager.create(kind);
                }
            });
        }
        let weak = Rc::downgrade(&manager);
        buttons[2].on_click(move |_| {
            if let Some(manager) = weak.upgrade() {
                manager.modify_selected();
            }
        });
        let weak = Rc::downgrade(&manager);
        buttons[3].on_click(move |_| {
            if let Some(manager) = weak.upgrade() {
                manager.delete_selected();
            }
        });
        let weak = Rc::downgrade(&manager);
        buttons[4].on_click(move |_| {
            if let Some(manager) = weak.upgrade() {
                manager.defaults();
            }
        });
        for (index, button) in buttons.into_iter().enumerate().skip(5) {
            let weak = Rc::downgrade(&manager);
            button.on_click(move |_| {
                let Some(manager) = weak.upgrade() else { return };
                manager.run_tool(index);
            });
        }
        let weak = Rc::downgrade(&manager);
        list.on_selection_changed(move |_| {
            if let Some(manager) = weak.upgrade() {
                manager.describe();
            }
        });
        let weak = Rc::downgrade(&manager);
        list.on_item_double_clicked(move |_| {
            if let Some(manager) = weak.upgrade() {
                manager.modify_selected();
            }
        });
        close.on_click(move |_| window.end_modal(ID_CANCEL));
        manager
    }

    fn run_tool(&self, action: usize) {
        let Some(ribbon) = self.ribbon.upgrade() else { return };
        let selected = self.selected().map(|style| style.id);
        let result = match action {
            5 => super::style_tools::new_style(&ribbon, &self.window, true),
            6 => match selected.as_ref().ok_or_else(|| "Select a style first.".to_owned())
                .and_then(|id| super::style_tools::find_replace(&ribbon, &self.window, id)) {
                Ok(Some(target)) => {
                    self.navigate_to.replace(Some(target));
                    self.window.end_modal(ID_CANCEL);
                    return;
                }
                Ok(None) => Ok(()),
                Err(error) => Err(error),
            },
            7 => super::style_tools::style_sets(&ribbon, &self.window, selected.as_ref()),
            8 => super::style_tools::linked_styles(&ribbon, &self.window, selected.as_ref()),
            9 => super::style_tools::table_styles(&ribbon, &self.window),
            10 => super::style_tools::list_styles(&ribbon, &self.window),
            11 => { super::style_tools::inspect(&ribbon, &self.window); Ok(()) }
            _ => return,
        };
        self.refresh(selected.as_ref());
        if let Err(error) = result { self.message(&error); }
    }

    fn selected(&self) -> Option<Style> {
        self.list
            .get_selection()
            .and_then(|index| self.styles.borrow().get(index as usize).cloned())
    }

    fn refresh(&self, selected: Option<&StyleId>) {
        let Some(ribbon) = self.ribbon.upgrade() else {
            return;
        };
        let mut styles = ribbon
            .state
            .borrow()
            .editor
            .document
            .styles
            .styles
            .values()
            .cloned()
            .collect::<Vec<_>>();
        styles.sort_by_key(|style| style.name.to_lowercase());
        let index = selected
            .and_then(|id| styles.iter().position(|style| &style.id == id))
            .unwrap_or(0);
        self.styles.replace(styles.clone());
        self.list.clear();
        for style in &styles {
            self.list.append(&crate::i18n::style_name(style));
        }
        if !styles.is_empty() {
            self.list.set_selection(index as u32, true);
        }
        self.describe();
    }

    fn message(&self, text: &str) {
        self.details.set_label(&crate::i18n::text(&(text)));
        self.details
            .wrap(self.window.get_client_size().width - self.window.from_dip_int(GAP_DIP * 2));
        self.window.layout();
    }

    fn describe(&self) {
        let selected = self.selected();
        self.modify.enable(selected.is_some());
        let Some(style) = selected else {
            self.delete.enable(false);
            return;
        };
        let Some(ribbon) = self.ribbon.upgrade() else {
            return;
        };
        let (base, next, deletable) = {
            let state = ribbon.state.borrow();
            let sheet = &state.editor.document.styles;
            let name = |id: &StyleId| {
                sheet
                    .get(id)
                    .map(crate::i18n::style_name)
                    .unwrap_or_else(|| crate::i18n::text("None"))
            };
            let base = style
                .based_on
                .as_ref()
                .map(name)
                .unwrap_or_else(|| crate::i18n::text("None"));
            let next = style
                .next_style
                .as_ref()
                .map(name)
                .unwrap_or_else(|| crate::i18n::style_name(&style));
            let mut probe = state.editor.clone();
            (base, next, probe.remove_unused_style(&style.id).is_ok())
        };
        self.delete.enable(deletable);
        let kind = match style.kind {
            StyleKind::Paragraph => crate::i18n::text("Paragraph style"),
            StyleKind::Character => crate::i18n::text("Character style"),
        };
        self.message(&if style.kind == StyleKind::Paragraph {
            format!("{kind} · Based on: {base}\nFollowing paragraph: {next}")
        } else {
            format!("{kind} · Based on: {base}")
        });
    }

    fn edit(&self, style: Style) {
        let Some(ribbon) = self.ribbon.upgrade() else {
            return;
        };
        let id = style.id.clone();
        match ribbon.edit_style_with_parent(style, &self.window) {
            Ok(true) => {
                self.refresh(Some(&id));
                ribbon.styles_revision.set(None);
                ribbon.refresh_styles_pane();
            }
            Ok(false) => (),
            Err(error) => self.message(&error),
        }
        self.window.raise();
        self.list.set_focus();
    }

    fn modify_selected(&self) {
        if let Some(style) = self.selected() {
            self.edit(style);
        }
    }

    fn create(&self, kind: StyleKind) {
        let Some(ribbon) = self.ribbon.upgrade() else {
            return;
        };
        let mut name = crate::i18n::text("New style");
        let mut number = 2;
        while ribbon
            .state
            .borrow()
            .editor
            .document
            .styles
            .id_by_name(&name)
            .is_some()
        {
            name = format!("{} {number}",crate::i18n::text("New style"));
            number += 1;
        }
        self.edit(Style {
            id: StyleId::new(),
            name,
            kind,
            based_on: if kind == StyleKind::Paragraph {
                Some(StyleId::from(model::STYLE_NORMAL))
            } else {
                None
            },
            next_style: None,
            character: Default::default(),
            paragraph: Default::default(),
            formatting: Default::default(),
        });
    }

    fn defaults(&self) {
        let style = self.ribbon.upgrade().and_then(|ribbon| {
            ribbon
                .state
                .borrow()
                .editor
                .document
                .styles
                .get(&StyleId::from(model::STYLE_NORMAL))
                .cloned()
        });
        if let Some(style) = style {
            self.edit(style);
        }
    }

    fn delete_selected(&self) {
        let Some(style) = self.selected() else {
            return;
        };
        let Some(ribbon) = self.ribbon.upgrade() else {
            return;
        };
        let dialog = MessageDialog::builder(
            &self.window,
            &crate::i18n::text(&(&format!("Delete the unused style ‘{}’?", style.name))),
            &crate::i18n::text(&("Delete style")),
        )
        .with_style(crate::cautious_question())
        .build();
        dialog.set_yes_no_labels(&crate::i18n::text(&("Delete")), &crate::i18n::text(&("Cancel")));
        crate::placement::centre(&dialog);
        let accepted = dialog.show_modal() == wxdragon::id::ID_YES;
        dialog.destroy();
        if accepted {
            match ribbon.edit_without_focus(|editor| editor.remove_unused_style(&style.id)) {
                Ok(()) => {
                    self.refresh(None);
                    ribbon.styles_revision.set(None);
                    ribbon.refresh_styles_pane();
                }
                Err(error) => self.message(&error),
            }
        }
    }
}
