//! Native File > Recent menu; menu IDs address an immutable path snapshot.
use super::*;
use crate::recent_files::{self, History};
use wxdragon::geometry::Point;
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
            &format!(
                "{} ({})…",
                crate::i18n::text("Number of recent files"),
                history.limit
            ),
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
                    let Some(value) = self.text_review(
                        "Recent files",
                        "Number of files to remember (0–20; 0 disables history):",
                        &history.limit.to_string(),
                        false,
                    ) else {
                        return Ok(());
                    };
                    let count = value
                        .trim()
                        .parse::<usize>()
                        .ok()
                        .filter(|count| *count <= recent_files::MAX_LIMIT)
                        .ok_or("Choose a number from 0 to 20")?;
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
