//! Cancelable output progress; native widget work runs outside editor borrows.
use super::*;
use crate::output_ui::ProgressState;
use std::sync::atomic::Ordering;
use wxdragon::{
    dialogs::{Dialog, DialogBuilder, DialogStyle},
    id::ID_CANCEL,
    sizers::{BoxSizer, Orientation, SizerFlag},
    widgets::{gauge::Gauge, static_text::StaticText},
};

const WIDTH_DIP: i32 = 420;
const GAP_DIP: i32 = 12;

pub(super) struct Progress {
    window: Dialog,
    message: StaticText,
    gauge: Option<Gauge>,
    cancel: Button,
    state: ProgressState,
    shown: Cell<Option<(usize, bool, crate::i18n::Language)>>,
}

fn title(printing: bool) -> &'static str {
    if printing {
        "Printing…"
    } else {
        "Exporting PDF…"
    }
}

fn request_cancel(state: &ProgressState, button: Button, message: StaticText) {
    state.cancel.store(true, Ordering::Release);
    button.enable(false);
    message.set_label(&crate::i18n::text("Cancelling…"));
}

impl Progress {
    fn new(parent: &impl WxWidget, state: ProgressState) -> Self {
        let window = DialogBuilder::new(parent, &crate::i18n::text(title(state.printing)))
            .with_style(DialogStyle::DefaultDialogStyle)
            .build();
        let dip = |value| window.from_dip_int(value);
        let message = StaticText::builder(&window)
            .with_label(&crate::i18n::text(title(state.printing)))
            .with_size(Size::new(dip(WIDTH_DIP - 2 * GAP_DIP), -1))
            .build();
        dialog_actions::heading(&message);
        let root = BoxSizer::builder(Orientation::Vertical).build();
        root.add(
            &message,
            0,
            SizerFlag::All | SizerFlag::Expand,
            dip(GAP_DIP),
        );
        let gauge = state.total.map(|total| {
            let gauge = Gauge::builder(&window).with_range(total as i32 + 1).build();
            root.add(
                &gauge,
                0,
                SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom | SizerFlag::Expand,
                dip(GAP_DIP),
            );
            gauge
        });
        let cancel = Button::builder(&window)
            .with_id(ID_CANCEL)
            .with_label(&crate::i18n::text("Cancel"))
            .build();
        dialog_actions::size(cancel);
        dialog_actions::divider(&window, &root, dip(GAP_DIP));
        root.add(
            &cancel,
            0,
            SizerFlag::All | SizerFlag::AlignCenterHorizontal,
            dip(GAP_DIP),
        );
        window.set_sizer_and_fit(root, true);
        window.set_escape_id(ID_CANCEL);
        let click_state = state.clone();
        cancel.on_click(move |_| request_cancel(&click_state, cancel, message));
        let close_state = state.clone();
        window.on_close(move |event| {
            if let WindowEventData::General(data) = &event {
                if data.can_veto() {
                    data.veto();
                    request_cancel(&close_state, cancel, message);
                    event.skip(false);
                    return;
                }
            }
            event.skip(true);
        });
        let progress = Self {
            window,
            message,
            gauge,
            cancel,
            state,
            shown: Cell::new(None),
        };
        progress.update();
        crate::placement::centre(&window);
        window.show(true);
        progress
    }

    fn update(&self) {
        let completed = self.state.completed.load(Ordering::Acquire);
        let cancelled = self.state.cancel.load(Ordering::Acquire);
        let key = (completed, cancelled, crate::i18n::language());
        if self.shown.replace(Some(key)) == Some(key) {
            return;
        }
        let message = if cancelled {
            "Cancelling…".into()
        } else if let Some(total) = self.state.total {
            if completed >= total {
                "Finishing export…".into()
            } else {
                format!("Exporting page {} of {}…", completed + 1, total)
            }
        } else {
            title(self.state.printing).into()
        };
        self.window
            .set_label(&crate::i18n::text(title(self.state.printing)));
        self.message.set_label(&crate::i18n::text(&message));
        self.cancel.set_label(&crate::i18n::text("Cancel"));
        self.cancel.enable(!cancelled);
        if let Some(gauge) = self.gauge {
            gauge.set_value(completed.min(self.state.total.unwrap_or(0)) as i32);
        }
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        if self.window.is_valid() {
            self.window.destroy();
        }
    }
}

impl Ribbon {
    pub(super) fn sync_output_progress(&self) {
        let state = { crate::output_ui::progress(&self.state.borrow()) };
        let previous = self.output_progress.borrow_mut().take();
        match (previous, state) {
            (Some(progress), Some(state)) if Arc::ptr_eq(&progress.state.cancel, &state.cancel) => {
                progress.update();
                self.output_progress.replace(Some(progress));
            }
            (previous, Some(state)) => {
                drop(previous);
                self.output_progress
                    .replace(Some(Progress::new(&self.ui.main_frame, state)));
            }
            (previous, None) => drop(previous),
        }
    }
}
