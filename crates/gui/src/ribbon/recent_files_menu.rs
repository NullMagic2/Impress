//! Native File > Recent menu; menu IDs address an immutable path snapshot.
use super::*;
use crate::recent_files::{self, History};
use wxdragon::geometry::Point;
use wxdragon::{
    dialogs::{DialogBuilder, DialogStyle},
    id::ID_CANCEL,
    sizers::{BoxSizer, Orientation, SizerFlag},
    widgets::{
        StaticBitmap, StaticLine,
        checkbox::CheckBox,
        static_text::StaticText,
        textctrl::{TextCtrl, TextCtrlStyle},
    },
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
                    let Some((enabled, count)) = settings_dialog(&self.ui.main_frame, &history)
                    else {
                        return Ok(());
                    };
                    // A background open/save may have recorded a file while
                    // this modal dialog was open; change only the latest limit.
                    history = History::load(&settings);
                    history.preferred_limit = count;
                    history.set_limit(if enabled { count } else { 0 });
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

// The illustration scales with the display; widgets use the platform theme.
fn pocket(count: usize) -> Option<wxdragon::bitmap_bundle::BitmapBundle> {
    use std::fmt::Write;
    const SHEET_COLOURS: [&str; 4] = ["#fff8e9", "#eadcc5", "#dbc2af", "#d0b5c1"];
    let mut sheets = String::new();
    for index in (0..count.min(SHEET_COLOURS.len())).rev() {
        let angle = 5.0 - index as f32 * (4.0 + count as f32 / 8.0);
        let top = -65.0 - index as f32 * (2.0 + count as f32 / 10.0);
        let colour = SHEET_COLOURS[index];
        write!(
            sheets,
            r##"<g transform="rotate({angle}) translate(-48 {top})">
            <rect width="98" height="95" rx="3" fill="{colour}"/>
            <path d="M11 12h29" stroke="#cbbdb4" stroke-width="3"/>
            <path d="M11 18h69 M11 23h63 M11 28h57" stroke="#dbd1c6"/></g>"##
        )
        .ok()?;
    }
    let svg =
        include_str!("../../assets/dialogs/recent-pocket.svg").replace("<!-- sheets -->", &sheets);
    wxdragon::bitmap_bundle::BitmapBundle::from_svg_text(&svg, Size::new(128, 158))
}

fn settings_dialog(
    parent: &wxdragon::widgets::frame::Frame,
    history: &History,
) -> Option<(bool, usize)> {
    let window = DialogBuilder::new(parent, &crate::i18n::text("Recent files"))
        .with_style(DialogStyle::DefaultDialogStyle)
        .build();
    let dip = |value| window.from_dip_int(value);
    let body = BoxSizer::builder(Orientation::Horizontal).build();
    let illustration = pocket(if history.limit > 0 { history.limit } else { 0 }).map(|bundle| {
        let picture =
            StaticBitmap::new_with_bitmap_bundle(&window, wxdragon::id::ID_ANY as i32, &bundle);
        body.add(&picture, 0, SizerFlag::AlignCenterVertical, 0);
        picture
    });
    let content = BoxSizer::builder(Orientation::Vertical).build();
    let row = BoxSizer::builder(Orientation::Horizontal).build();
    let remember = CheckBox::builder(&window)
        .with_label(&crate::i18n::text("Remember up to"))
        .build();
    remember.set_value(history.limit > 0);
    let count = TextCtrl::builder(&window)
        .with_value(&history.preferred_limit.to_string())
        .with_style(TextCtrlStyle::Centre)
        .with_size(Size::new(dip(44), -1))
        .build();
    count.set_name(&crate::i18n::text("Number of files:"));
    count.set_tooltip(&crate::i18n::text("Enter a number from 1 to 20."));
    let suffix = StaticText::builder(&window)
        .with_label(&crate::i18n::text("recently opened files."))
        .build();
    row.add(
        &remember,
        0,
        SizerFlag::AlignCenterVertical | SizerFlag::Right,
        dip(7),
    );
    row.add(
        &count,
        0,
        SizerFlag::AlignCenterVertical | SizerFlag::Right,
        dip(7),
    );
    row.add(&suffix, 0, SizerFlag::AlignCenterVertical, 0);
    content.add_sizer(&row, 0, SizerFlag::AlignCenterHorizontal, 0);
    let line = StaticLine::builder(&window).build();
    content.add(
        &line,
        0,
        SizerFlag::Expand | SizerFlag::Top | SizerFlag::Bottom,
        dip(16),
    );
    let footer = BoxSizer::builder(Orientation::Horizontal).build();
    let ok = Button::builder(&window)
        .with_id(ID_OK)
        .with_label(&crate::i18n::text("OK"))
        .build();
    let cancel = Button::builder(&window)
        .with_id(ID_CANCEL)
        .with_label(&crate::i18n::text("Cancel"))
        .build();
    dialog_actions::pair(&footer, ok, cancel);
    content.add_sizer(&footer, 0, SizerFlag::Expand, 0);
    body.add_sizer(
        &content,
        1,
        SizerFlag::All | SizerFlag::AlignCenterVertical,
        dip(21),
    );

    let last_valid = Rc::new(Cell::new(history.preferred_limit));
    let saved = last_valid.clone();
    let update = move || {
        let enabled = remember.is_checked();
        let parsed = count
            .get_value()
            .trim()
            .parse::<usize>()
            .ok()
            .filter(|n| (1..=recent_files::MAX_LIMIT).contains(n));
        if let Some(value) = parsed {
            saved.set(value);
        }
        count.enable(enabled);
        suffix.enable(enabled);
        ok.enable(!enabled || parsed.is_some());
        if let Some(picture) = illustration {
            if let Some(bundle) = pocket(if enabled { saved.get() } else { 0 }) {
                StaticBitmap::set_bitmap_bundle(&picture, &bundle);
            }
        }
    };
    let on_toggle = update.clone();
    remember.on_toggled(move |_| on_toggle());
    update();
    count.on_text_updated(move |_| update());
    window.set_sizer_and_fit(body, true);
    window.set_escape_id(ID_CANCEL);
    ok.set_default();
    ok.on_click(move |_| window.end_modal(ID_OK));
    cancel.on_click(move |_| window.end_modal(ID_CANCEL));
    crate::placement::centre(&window);
    remember.set_focus();
    let accepted = window.show_modal() == ID_OK;
    let result = (remember.is_checked(), last_valid.get());
    window.destroy();
    accepted.then_some(result)
}
