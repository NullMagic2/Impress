//! Native File > Recent menu; menu IDs address an immutable path snapshot.
use super::*;
use crate::recent_files::{self, History};
use wxdragon::geometry::Point;
use wxdragon::{
    dialogs::{DialogBuilder, DialogStyle},
    id::ID_CANCEL,
    sizers::{BoxSizer, Orientation, SizerFlag},
    widgets::{checkbox::CheckBox, spinctrl::SpinCtrl, static_text::StaticText},
};
const FIRST_ID: i32 = 32_650;
const CLEAR_ID: i32 = 32_675;
const COUNT_ID: i32 = 32_676;
const EMPTY_ID: i32 = 32_677;
// wxWidgets native menu IDs are restricted to signed 16-bit positive IDs.
const _: () = assert!(FIRST_ID + recent_files::MAX_LIMIT as i32 <= CLEAR_ID && EMPTY_ID < 0x7fff);

impl Ribbon {
    pub(super) fn show_recent_files(&self) -> Result<(), String> {
        let settings = recent_files::settings_path()?;
        let history = History::load(&settings);
        let mut menu = Menu::builder().build();
        for (index, path) in history.files.iter().enumerate() {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            let parent = path.parent().unwrap_or(path).display();
            let label = format!("{}. {} — {}", index + 1, name, parent)
                .replace('&', "&&")
                .replace(['\t', '\n', '\r'], " ");
            let id = FIRST_ID + index as i32;
            menu.append(
                id,
                &label,
                &path.to_string_lossy(),
                wxdragon::menus::ItemKind::Normal,
            );
            menu.enable_item(id, path.is_file());
        }
        if history.files.is_empty() {
            menu.append(
                EMPTY_ID,
                &crate::i18n::text("No recent files"),
                "",
                wxdragon::menus::ItemKind::Normal,
            );
            menu.enable_item(EMPTY_ID, false);
        }
        menu.append_separator();
        menu.append(
            COUNT_ID,
            &crate::i18n::text("Recent files settings…"),
            "",
            wxdragon::menus::ItemKind::Normal,
        );
        menu.append(
            CLEAR_ID,
            &crate::i18n::text("Clear recent files"),
            "",
            wxdragon::menus::ItemKind::Normal,
        );
        menu.enable_item(CLEAR_ID, !history.files.is_empty());
        self.recent_paths.replace(history.files);
        let button = self.ui.recent_files_button;
        let anchor = crate::gtk_native::to_screen(&button, Point::new(0, button.get_size().height));
        self.ui.main_frame.popup_menu(&mut menu, Some(anchor));
        self.recent_paths.borrow_mut().clear();
        Ok(())
    }
    pub(super) fn handle_recent_files(&self, id: i32) -> bool {
        let result = if (FIRST_ID..FIRST_ID + recent_files::MAX_LIMIT as i32).contains(&id) {
            let path = self
                .recent_paths
                .borrow()
                .get((id - FIRST_ID) as usize)
                .cloned();
            path.map(|path| background_io::queue_open(path, &self.state))
                .unwrap_or(Ok(()))
        } else if id == CLEAR_ID || id == COUNT_ID {
            (|| {
                let settings = recent_files::settings_path()?;
                let mut history = History::load(&settings);
                if id == CLEAR_ID {
                    history.files.clear();
                } else {
                    let Some(count) = settings_dialog(&self.ui.main_frame, history.limit) else {
                        return Ok(());
                    };
                    // A background open/save may have recorded a file while
                    // this modal dialog was open; change only the latest limit.
                    history = History::load(&settings);
                    history.set_limit(count);
                }
                history.store(&settings)
            })()
        } else {
            return false;
        };
        if let Err(error) = result {
            self.state.borrow_mut().action_status = Some(error);
        }
        self.last_key.borrow_mut().take();
        self.focus_document();
        true
    }
}

/// Native settings controls keep numeric entry bounded and use the desktop theme.
fn settings_dialog(parent:&wxdragon::widgets::frame::Frame,limit:usize)->Option<usize> {
    let window=DialogBuilder::new(parent,&crate::i18n::text("Recent files"))
        .with_style(DialogStyle::DefaultDialogStyle).build();
    let dip=move |v|window.from_dip_int(v);
    let root=BoxSizer::builder(Orientation::Vertical).build();
    let row=BoxSizer::builder(Orientation::Horizontal).build();
    let remember=CheckBox::builder(&window)
        .with_label(&crate::i18n::text("Remember up to"))
        .build();
    remember.set_value(limit>0);
    let count=SpinCtrl::builder(&window).with_range(1,recent_files::MAX_LIMIT as i32)
        .with_initial_value(if limit==0{recent_files::DEFAULT_LIMIT}else{limit.min(recent_files::MAX_LIMIT)} as i32)
        .with_size(Size::new(dip(136),-1)).build();
    let suffix=StaticText::builder(&window)
        .with_label(&crate::i18n::text("recently opened files."))
        .build();
    row.add(&remember,0,SizerFlag::AlignCenterVertical|SizerFlag::Right,dip(8));
    row.add(&count,0,SizerFlag::AlignCenterVertical|SizerFlag::Right,dip(8));
    row.add(&suffix,0,SizerFlag::AlignCenterVertical,0);
    let update=move || {
        let enabled=remember.is_checked();count.enable(enabled);suffix.enable(enabled);
    };
    remember.on_toggled(move |_|update());
    update();
    root.add_sizer(&row,0,SizerFlag::All|SizerFlag::Expand,dip(20));
    let footer=BoxSizer::builder(Orientation::Horizontal).build();
    let cancel=Button::builder(&window).with_id(ID_CANCEL).with_label(&crate::i18n::text("Cancel")).build();
    let ok=Button::builder(&window).with_id(ID_OK).with_label(&crate::i18n::text("OK")).build();
    for button in [cancel,ok] {
        button.set_min_size(Size::new(dip(112),dip(36)));
    }
    footer.add_stretch_spacer(1);
    let buttons=if cfg!(windows){[ok,cancel]}else{[cancel,ok]};
    footer.add(&buttons[0],0,SizerFlag::Right,dip(16));
    footer.add(&buttons[1],0,SizerFlag::empty(),0);
    footer.add_stretch_spacer(1);
    root.add_sizer(&footer,0,SizerFlag::Left|SizerFlag::Right|SizerFlag::Bottom|SizerFlag::Expand,dip(20));
    window.set_sizer_and_fit(root,true);
    window.set_escape_id(ID_CANCEL);ok.set_default();
    ok.on_click(move |_|window.end_modal(ID_OK));
    cancel.on_click(move |_|window.end_modal(ID_CANCEL));
    crate::placement::centre(&window);
    remember.set_focus();
    let accepted=window.show_modal()==ID_OK;
    let selected=if remember.is_checked(){count.value() as usize}else{0};
    window.destroy();
    accepted.then_some(selected)
}
