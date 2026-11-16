//! Flag menu for the persisted interface language; proofing remains independent.
use super::*;
use crate::i18n::{self, Language, Localize};

const FIRST_LANGUAGE_ID: i32 = 32_500;

pub(super) fn install(ribbon: &Rc<Ribbon>) {
    let weak = Rc::downgrade(ribbon);
    ribbon.ui.main_frame.on_menu_selected(move |event| {
        let Some(language) = Language::ALL
            .get((event.get_id() - FIRST_LANGUAGE_ID) as usize)
            .copied()
        else {
            event.skip(true);
            return;
        };
        let Some(ribbon) = weak.upgrade() else {
            return;
        };
        if let Err(error) = i18n::select(language) {
            ribbon.state.borrow_mut().action_status = Some(error);
            return;
        }
        ribbon.ui.root_panel.freeze();
        ribbon.syncing.set(true);
        ribbon.styles_syncing.set(true);
        ribbon.ui.localize();
        ribbon.styles_syncing.set(false);
        ribbon.syncing.set(false);
        ribbon.label_text.borrow_mut().clear();
        ribbon.last_status.borrow_mut().clear();
        ribbon.last_title.borrow_mut().clear();
        ribbon.dpi.set(0);
        ribbon.styles_revision.set(None);
        ribbon.navigation_revision.set(None);
        skin::language_changed();
        if let Some(strip) = ribbon.tab_strip.borrow().as_ref() {
            strip.language_changed();
        }
        ribbon.refresh_art();
        ribbon.refresh_styles_pane();
        ribbon.ui.interface_language_button.set_tooltip(&format!(
            "{}: {}",
            i18n::text("Interface language"),
            language.name()
        ));
        ribbon.ui.root_panel.layout();
        ribbon.ui.main_frame.layout();
        ribbon.ui.root_panel.thaw();
        ribbon.ui.main_frame.refresh(false, None);
        ribbon.dictionary_manager_language_changed();
        if let Some(progress) = ribbon.dictionary_install.borrow().as_ref() {
            progress.language_changed();
        }
    });
}

impl Ribbon {
    pub(super) fn show_interface_languages(&self) {
        let mut builder = Menu::builder();
        for (index, language) in Language::ALL.iter().enumerate() {
            builder = builder.append_radio_item(
                FIRST_LANGUAGE_ID + index as i32,
                language.name(),
                language.name(),
            );
        }
        let mut menu = builder.build();
        menu.check_item(FIRST_LANGUAGE_ID + i18n::language() as i32, true);
        let anchor = crate::gtk_native::to_screen(
            &self.ui.interface_language_button,
            wxdragon::geometry::Point::new(0, self.ui.interface_language_button.get_size().height),
        );
        self.ui.main_frame.popup_menu(&mut menu, Some(anchor));
    }
}

pub(super) fn flag_name() -> &'static str {
    match i18n::language() {
        Language::English => "flag-gb",
        Language::Greek => "flag-gr",
        Language::Spanish => "flag-es",
        Language::PortugueseBrazil => "flag-br",
        Language::PortuguesePortugal => "flag-pt",
    }
}
