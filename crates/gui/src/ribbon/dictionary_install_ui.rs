//! Installation scope prompt and a modeless progress window that can be closed.
use super::*;
use crate::dictionary_install::{self, Operation, Outcome, Progress, Scope};
use wxdragon::{
    dialogs::{Dialog, DialogBuilder, DialogStyle},
    id::ID_CANCEL,
    sizers::{BoxSizer, Orientation, SizerFlag},
    widgets::{
        gauge::Gauge,
        static_text::StaticText,
        textctrl::{TextCtrl, TextCtrlStyle},
    },
};

const WIDTH_DIP: i32 = 560;
const HEIGHT_DIP: i32 = 320;
const GAP_DIP: i32 = 12;
const STEPS: i32 = 4;
const LOCAL_ID: i32 = 32_001;
const SYSTEM_ID: i32 = 32_002;

struct Job {
    tag: String,
    operation: Operation,
    progress: Receiver<Progress>,
    result: Receiver<Result<Outcome, String>>,
}

pub(super) struct InstallDialog {
    window: Dialog,
    caption: StaticText,
    gauge: Gauge,
    details: TextCtrl,
    hint: StaticText,
    job: RefCell<Option<Job>>,
    close: Button,
    language_tag: RefCell<String>,
    last_scope: Cell<Scope>,
    last_operation: Cell<Operation>,
    stages: RefCell<Vec<String>>,
    hint_source: RefCell<String>,
}

impl InstallDialog {
    fn new(ribbon: &Ribbon) -> Rc<Self> {
        let window = DialogBuilder::new(&ribbon.ui.main_frame, &crate::i18n::text(&("Dictionary progress")))
            .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
            .build();
        let dip = |value| window.from_dip_int(value);
        window.set_size(window.from_dip(Size::new(WIDTH_DIP, HEIGHT_DIP)));
        window.set_min_size(window.from_dip(Size::new(WIDTH_DIP, HEIGHT_DIP)));
        let caption = StaticText::builder(&window)
            .with_label(&crate::i18n::text(&("Dictionary progress")))
            .build();
        dialog_actions::heading(&caption);
        let gauge = Gauge::builder(&window).with_range(STEPS).build();
        let details = TextCtrl::builder(&window)
            .with_style(TextCtrlStyle::MultiLine | TextCtrlStyle::ReadOnly)
            .build();
        let hint = StaticText::builder(&window).with_label(&crate::i18n::text(&(""))).build();
        let close = Button::builder(&window).with_label(&crate::i18n::text(&("Close"))).build();
        close.set_default();
        let layout = BoxSizer::builder(Orientation::Vertical).build();
        layout.add(
            &caption,
            0,
            SizerFlag::Expand | SizerFlag::All,
            dip(GAP_DIP),
        );
        layout.add(
            &gauge,
            0,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
            dip(GAP_DIP),
        );
        layout.add(
            &details,
            1,
            SizerFlag::Expand | SizerFlag::All,
            dip(GAP_DIP),
        );
        layout.add(
            &hint,
            0,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
            dip(GAP_DIP),
        );
        dialog_actions::size(close);
        dialog_actions::divider(&window, &layout, dip(GAP_DIP));
        layout.add(
            &close,
            0,
            SizerFlag::AlignCenterHorizontal | SizerFlag::All,
            dip(GAP_DIP),
        );
        window.set_sizer(layout, true);
        close.on_click(move |_| {
            window.show(false);
        });
        window.on_close(move |event| {
            let can_veto = match &event {
                WindowEventData::General(data) => data.can_veto(),
                WindowEventData::Session(data) => data.can_veto(),
                _ => false,
            };
            if can_veto {
                window.show(false);
                match &event {
                    WindowEventData::General(data) => data.veto(),
                    WindowEventData::Session(data) => data.veto(),
                    _ => {}
                }
                event.skip(false);
            } else {
                event.skip(true);
            }
        });
        let this = Rc::new(Self {
            window,
            caption,
            gauge,
            details,
            hint,
            job: RefCell::new(None),
            close,
            language_tag: RefCell::new(String::new()),
            last_scope: Cell::new(Scope::Local),
            last_operation: Cell::new(Operation::Install),
            stages: RefCell::new(Vec::new()),
            hint_source: RefCell::new(String::new()),
        });
        let weak = Rc::downgrade(&this);
        window.on_key_down(move |event| {
            if let WindowEventData::Keyboard(key) = &event {
                if key.get_key_code() == Some(WXK_ESCAPE) {
                    if let Some(dialog) = weak.upgrade() {
                        dialog.window.show(false);
                    }
                    event.skip(false);
                    return;
                }
            }
            event.skip(true);
        });
        this
    }

    fn show(&self) {
        if !self.window.is_shown() {
            crate::placement::centre(&self.window);
        }
        self.window.show(true);
        self.window.raise();
    }

    fn start(&self, tag: String, scope: Scope, operation: Operation) -> Result<(), String> {
        let (progress_sender, progress) = mpsc::channel();
        let (result_sender, result) = mpsc::channel();
        let worker_tag = tag.clone();
        std::thread::Builder::new()
            .name("impress-dictionary-install".into())
            .spawn(move || {
                let outcome =
                    dictionary_install::apply(&worker_tag, scope, operation, &progress_sender);
                let _ = result_sender.send(outcome);
            })
            .map_err(|error| error.to_string())?;
        self.language_tag.replace(tag.clone());
        self.last_scope.set(scope);
        self.last_operation.set(operation);
        self.stages.replace(vec![format!("Preparing {}…",operation.title().to_lowercase())]);
        self.hint_source.borrow_mut().clear();
        self.language_changed();
        self.gauge.set_value(0);
        self.job.replace(Some(Job {
            tag,
            operation,
            progress,
            result,
        }));
        self.show();
        Ok(())
    }

    pub(super) fn is_busy(&self) -> bool {
        self.job.borrow().is_some()
    }

    fn render_stages(&self) {
        let text=self.stages.borrow().iter().map(|stage|crate::i18n::text(stage)).collect::<Vec<_>>().join("\n");
        self.details.change_value(&text);
    }
    pub(super) fn language_changed(&self) {
        self.window.set_label(&crate::i18n::text("Dictionary progress"));
        self.close.set_label(&crate::i18n::text("Close"));
        let tag=self.language_tag.borrow();
        self.caption.set_label(&format!("{} · {} · {}",crate::spelling::language_label(&tag),crate::i18n::text(self.last_scope.get().label()),crate::i18n::text(self.last_operation.get().title())));
        self.hint.set_label(&crate::i18n::text(&self.hint_source.borrow()));
        self.render_stages();
        self.window.layout();
    }
    fn poll(&self, ribbon: &Ribbon) {
        let completed = {
            let slot = self.job.borrow();
            let Some(job) = slot.as_ref() else {
                return;
            };
            while let Ok(progress) = job.progress.try_recv() {
                let duplicate=self.stages.borrow().last()==Some(&progress.stage);
                if !duplicate {self.stages.borrow_mut().push(progress.stage); self.render_stages();}
                self.gauge
                    .set_value(progress.completed_steps.clamp(0, STEPS));
            }
            match job.result.try_recv() {
                Ok(result) => Some(result),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => {
                    Some(Err("Installation was interrupted. Try again.".into()))
                }
            }
        };
        let Some(result) = completed else {
            return;
        };
        let Some(job) = self.job.borrow_mut().take() else {
            return;
        };
        match result {
            Ok(outcome) => {
                let available = crate::spelling::languages().ok().is_some_and(|tags| {
                    crate::spelling::dictionary_language(&job.tag, &tags).is_some()
                });
                let message = if outcome.restart_required {
                    format!(
                        "{} complete. Restart Windows to finish setup.",
                        job.operation.title()
                    )
                } else if job.operation == Operation::Remove {
                    "Dictionary deleted.".into()
                } else if available {
                    "Installation complete. The dictionary is ready to use.".into()
                } else {
                    "Installation complete. Restart Impress to refresh the system dictionaries."
                        .into()
                };
                self.gauge.set_value(STEPS);
                self.hint_source.replace(format!("{} complete.",job.operation.title()));
                self.stages.borrow_mut().extend([String::new(),message.clone()]);
                self.language_changed();
                let mut state = ribbon.state.borrow_mut();
                crate::inline_spelling::invalidate(&mut state);
                state.action_status = Some(format!(
                    "{}: {message}",
                    crate::spelling::language_label(&job.tag)
                ));
            }
            Err(error) => {
                self.hint_source.replace(format!("{} did not finish.",job.operation.title()));
                self.stages.borrow_mut().extend([String::new(),error.clone()]);
                self.language_changed();
                ribbon.state.borrow_mut().action_status = Some(error);
            }
        }
        crate::inline_spelling::invalidate(&mut ribbon.state.borrow_mut());
        ribbon.refresh_dictionary_manager();
        self.window.layout();
    }
}

fn choose_scope(ribbon: &Ribbon, tag: &str) -> Option<Scope> {
    let window = DialogBuilder::new(&ribbon.ui.main_frame, &crate::i18n::text(&("Install spelling dictionary"))).build();
    let dip = |value| window.from_dip_int(value);
    let question = StaticText::builder(&window)
        .with_style(wxdragon::widgets::static_text::StaticTextStyle::AlignCenterHorizontal)
        .with_label(&crate::i18n::text(&(&format!(
            "The {} dictionary is not installed.\nWhere would you like to install it?",
            crate::spelling::language_label(tag)
        ))))
        .build();
    let local = Button::builder(&window)
        .with_label(&crate::i18n::text(&("Install locally")))
        .build();
    let system = Button::builder(&window)
        .with_label(&crate::i18n::text(&("Install system-wide")))
        .build();
    let cancel = Button::builder(&window)
        .with_id(ID_CANCEL)
        .with_label(&crate::i18n::text(&("Cancel")))
        .build();
    local.set_default();
    window.set_escape_id(ID_CANCEL);
    local.on_click(move |_| window.end_modal(LOCAL_ID));
    system.on_click(move |_| window.end_modal(SYSTEM_ID));
    cancel.on_click(move |_| window.end_modal(ID_CANCEL));
    let layout = BoxSizer::builder(Orientation::Vertical).build();
    layout.add(
        &question,
        0,
        SizerFlag::Expand | SizerFlag::All,
        dip(GAP_DIP),
    );
    let buttons = BoxSizer::builder(Orientation::Horizontal).build();
    for button in [local, system, cancel] {
        buttons.add(&button, 0, SizerFlag::Left, dip(GAP_DIP));
    }
    layout.add_sizer(
        &buttons,
        0,
        SizerFlag::AlignCenterHorizontal | SizerFlag::All,
        dip(GAP_DIP),
    );
    window.set_sizer_and_fit(layout, true);
    crate::placement::centre(&window);
    let choice = window.show_modal();
    window.destroy();
    match choice {
        LOCAL_ID => Some(Scope::Local),
        SYSTEM_ID => Some(Scope::System),
        _ => None,
    }
}

impl Ribbon {
    pub(super) fn request_dictionary_install(&self, tag: &str) -> Result<(), String> {
        let existing = self.dictionary_install.borrow().clone();
        if let Some(dialog) = existing
            .as_ref()
            .filter(|dialog| dialog.job.borrow().is_some())
        {
            dialog.show();
            return Ok(());
        }
        let Some(scope) = choose_scope(self, tag) else {
            return Ok(());
        };
        self.start_dictionary_operation(tag, scope, Operation::Install)
    }

    pub(super) fn dictionary_operation_busy(&self) -> bool {
        self.dictionary_install
            .borrow()
            .as_ref()
            .is_some_and(|dialog| dialog.is_busy())
    }

    pub(super) fn start_dictionary_operation(
        &self,
        tag: &str,
        scope: Scope,
        operation: Operation,
    ) -> Result<(), String> {
        if self.dictionary_operation_busy() {
            return Err("A dictionary change is already running.".into());
        }
        let existing = self.dictionary_install.borrow().clone();
        let dialog = existing.unwrap_or_else(|| {
            let dialog = InstallDialog::new(self);
            self.dictionary_install.replace(Some(Rc::clone(&dialog)));
            dialog
        });
        dialog.start(tag.to_string(), scope, operation)
    }

    pub(super) fn show_dictionary_install(&self) {
        if let Some(dialog) = self.dictionary_install.borrow().as_ref() {
            dialog.show();
        }
    }

    pub(super) fn poll_dictionary_install(&self) {
        let dialog = self.dictionary_install.borrow().clone();
        if let Some(dialog) = dialog {
            dialog.poll(self);
        }
    }
}
