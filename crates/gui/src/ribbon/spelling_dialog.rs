//! One native spelling review window, with context and undoable corrections.
use super::*;
use crate::spelling::{Dictionary, Issue, Report};
use wxdragon::{
    dialogs::{Dialog, DialogBuilder, DialogStyle},
    id::ID_CANCEL,
    sizers::{BoxSizer, Orientation, SizerFlag},
    widgets::{
        listbox::ListBox,
        static_text::StaticText,
        textctrl::{TextAttr, TextCtrl, TextCtrlStyle},
    },
};

/// Minimum client area and spacing, in device-independent pixels.
const WIDTH_DIP: i32 = 680;
const HEIGHT_DIP: i32 = 470;
const GAP_DIP: i32 = 10;
const CONTEXT_HEIGHT_DIP: i32 = 94;
const ACTION_WIDTH_DIP: i32 = 150;
const CONTEXT_CHARS: usize = 90;
const MAX_REPLACEMENT_BYTES: usize = 1024;
const MISSPELLING_INK: Colour = Colour::rgb(190, 35, 45);

#[derive(Clone, Copy)]
enum Action {
    Ignore,
    IgnoreAll,
    Add,
    Change,
    ChangeAll,
}

struct Session {
    report: Report,
    dictionary: Dictionary,
    revision: u64,
    reviewed: usize,
    changed: usize,
}

struct SpellingDialog {
    window: Dialog,
    heading: StaticText,
    language: StaticText,
    context: TextCtrl,
    replacement: TextCtrl,
    suggestions: ListBox,
    status: StaticText,
    actions: Vec<(Button, Action)>,
    close: Button,
    scope: &'static str,
    session: RefCell<Session>,
}

pub(super) fn show(ribbon: &Ribbon, job: review_ui::SpellJob, report: Report) {
    let Some(owner) = ribbon.owner.upgrade() else {
        return;
    };
    if let Some(issue) = report.issues.first() {
        if let Err(error) = owner.edit_without_focus(|editor| {
            editor.set_caret(issue.node.clone(), issue.start);
            editor.extend_selection(issue.node.clone(), issue.end);
            Ok(())
        }) {
            owner.state.borrow_mut().action_status = Some(error);
            return;
        }
    }
    owner.state.borrow_mut().action_status = None;
    let dialog = SpellingDialog::new(&owner, job, report);
    dialog.refresh(&owner);
    crate::placement::centre(&dialog.window);
    dialog.window.show_modal();
    dialog.window.destroy();
    owner.focus_document();
}

impl SpellingDialog {
    fn new(ribbon: &Rc<Ribbon>, job: review_ui::SpellJob, report: Report) -> Rc<Self> {
        let window = DialogBuilder::new(&ribbon.ui.main_frame, &crate::i18n::text(&("Spelling")))
            .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
            .build();
        let dip = |value| window.from_dip_int(value);
        window.set_min_size(window.from_dip(Size::new(WIDTH_DIP, HEIGHT_DIP)));
        window.set_size(window.from_dip(Size::new(WIDTH_DIP, HEIGHT_DIP)));
        window.set_escape_id(ID_CANCEL);
        let root = BoxSizer::builder(Orientation::Vertical).build();
        let body = BoxSizer::builder(Orientation::Horizontal).build();
        let fields = BoxSizer::builder(Orientation::Vertical).build();
        let buttons = BoxSizer::builder(Orientation::Vertical).build();
        let heading = StaticText::builder(&window)
            .with_label(&crate::i18n::text(&("Not in dictionary")))
            .build();
        dialog_actions::heading(&heading);
        let language = StaticText::builder(&window).with_label(&crate::i18n::text(&(""))).build();
        let context = TextCtrl::builder(&window)
            .with_style(TextCtrlStyle::MultiLine | TextCtrlStyle::ReadOnly | TextCtrlStyle::Rich2)
            .with_size(Size::new(-1, dip(CONTEXT_HEIGHT_DIP)))
            .build();
        context.set_tooltip(&crate::i18n::text(&("The highlighted word is being checked.")));
        let replacement_label = StaticText::builder(&window)
            .with_label(&crate::i18n::text(&("Change &to:")))
            .build();
        let replacement = TextCtrl::builder(&window).build();
        let suggestions_label = StaticText::builder(&window)
            .with_label(&crate::i18n::text(&("&Suggestions:")))
            .build();
        let suggestions = ListBox::builder(&window).build();
        fields.add(
            &heading,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            dip(GAP_DIP),
        );
        fields.add(
            &context,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            dip(GAP_DIP),
        );
        fields.add(&replacement_label, 0, SizerFlag::Bottom, dip(GAP_DIP / 2));
        fields.add(
            &replacement,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            dip(GAP_DIP),
        );
        fields.add(&suggestions_label, 0, SizerFlag::Bottom, dip(GAP_DIP / 2));
        fields.add(&suggestions, 1, SizerFlag::Expand, 0);
        let mut actions = Vec::new();
        for (label, action) in [
            ("&Ignore Once", Action::Ignore),
            ("I&gnore All", Action::IgnoreAll),
            ("&Add to Dictionary", Action::Add),
            ("&Change", Action::Change),
            ("Change A&ll", Action::ChangeAll),
        ] {
            let button = Button::builder(&window)
                .with_label(&crate::i18n::text(&(label)))
                .with_size(Size::new(dip(ACTION_WIDTH_DIP), -1))
                .build();
            buttons.add(
                &button,
                0,
                SizerFlag::Expand | SizerFlag::Bottom,
                dip(GAP_DIP),
            );
            actions.push((button, action));
            if matches!(action, Action::Add) {
                buttons.add_spacer(dip(GAP_DIP));
            }
        }
        let close = Button::builder(&window)
            .with_id(ID_CANCEL)
            .with_label(&crate::i18n::text(&("Close")))
            .build();
        dialog_actions::size(close);
        body.add_sizer(
            &fields,
            1,
            SizerFlag::Expand | SizerFlag::Right,
            dip(GAP_DIP * 2),
        );
        body.add_sizer(&buttons, 0, SizerFlag::Expand, 0);
        root.add(
            &language,
            0,
            SizerFlag::Expand | SizerFlag::All,
            dip(GAP_DIP),
        );
        root.add_sizer(
            &body,
            1,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
            dip(GAP_DIP),
        );
        let status = StaticText::builder(&window).with_label(&crate::i18n::text(&(""))).build();
        root.add_spacer(dip(GAP_DIP));
        dialog_actions::divider(&window, &root, dip(GAP_DIP));
        let footer = BoxSizer::builder(Orientation::Horizontal).build();
        footer.add(&status, 1, SizerFlag::AlignCenterVertical | SizerFlag::Right, dip(GAP_DIP));
        footer.add(&close, 0, SizerFlag::AlignCenterVertical, 0);
        root.add_sizer(&footer, 0, SizerFlag::Expand | SizerFlag::All, dip(GAP_DIP));
        window.set_sizer(root, true);
        let this = Rc::new(Self {
            window,
            heading,
            language,
            context,
            replacement,
            suggestions,
            status,
            actions,
            close,
            scope: if job.selection_only {
                "Selection"
            } else {
                "Document"
            },
            session: RefCell::new(Session {
                report,
                dictionary: job.dictionary,
                revision: job.revision,
                reviewed: 0,
                changed: 0,
            }),
        });
        this.bind(ribbon);
        this
    }

    fn bind(self: &Rc<Self>, ribbon: &Rc<Ribbon>) {
        for &(button, action) in &self.actions {
            let (popup, owner) = (Rc::downgrade(self), Rc::downgrade(ribbon));
            button.on_click(move |_| {
                if let (Some(dialog), Some(ribbon)) = (popup.upgrade(), owner.upgrade()) {
                    dialog.act(&ribbon, action);
                }
            });
        }
        let (list, replacement) = (self.suggestions, self.replacement);
        list.on_selection_changed(move |_| {
            if let Some(value) = list.get_string_selection() {
                replacement.change_value(&value);
            }
        });
        let (popup, owner) = (Rc::downgrade(self), Rc::downgrade(ribbon));
        list.on_item_double_clicked(move |_| {
            if let (Some(dialog), Some(ribbon)) = (popup.upgrade(), owner.upgrade()) {
                dialog.act(&ribbon, Action::Change);
            }
        });
        let window = self.window;
        self.close.on_click(move |_| window.end_modal(ID_CANCEL));
    }

    fn message(&self, text: &str) {
        self.status
            .set_label(&crate::i18n::text(&(&text.chars().take(105).collect::<String>())));
        self.status.set_tooltip(&crate::i18n::text(&(text)));
        self.window.layout();
    }

    fn refresh(&self, ribbon: &Ribbon) {
        let session = self.session.borrow();
        let current = session.report.issues.first();
        for &(button, _) in &self.actions {
            button.enable(current.is_some());
        }
        self.replacement.enable(current.is_some());
        self.suggestions.enable(current.is_some());
        self.suggestions.clear();
        let Some(issue) = current else {
            self.heading.set_label(&crate::i18n::text(&("Spelling check complete")));
            self.language.set_label(&crate::i18n::text(&(self.scope)));
            self.context.change_value("");
            self.replacement.change_value("");
            let status = if !session.report.warnings.is_empty() {
                "Some text could not be checked because its dictionary is unavailable.".to_string()
            } else if session.report.limited {
                "The check limit was reached. Check again to review more text.".to_string()
            } else {
                format!(
                    "{} reviewed · {} changed",
                    session.reviewed, session.changed
                )
            };
            self.message(&status);
            self.close.set_default();
            self.close.set_focus();
            return;
        };
        self.heading.set_label(&crate::i18n::text(&(&format!(
            "Not in dictionary: {}",
            issue.word.replace('&', "&&")
        ))));
        self.language.set_label(&crate::i18n::text(&(&format!(
            "{} · {}",
            self.scope,
            crate::spelling::language_label(&issue.language)
        ))));
        let paragraph = ribbon
            .state
            .borrow()
            .editor
            .paragraph_text(&issue.node)
            .unwrap_or_default();
        let (text, start, end) = context_preview(&paragraph, issue);
        self.context.change_value(&text);
        let mut normal = TextAttr::new();
        normal.set_text_colour(self.replacement.get_foreground_color());
        normal.set_font_weight(wxdragon::font::FontWeight::Normal);
        self.context.set_style(0, -1, &normal);
        let mut mark = TextAttr::new();
        mark.set_text_colour(MISSPELLING_INK);
        mark.set_font_weight(wxdragon::font::FontWeight::Bold);
        self.context.set_style(start, end, &mark);
        self.context.set_selection(start, end);
        for suggestion in &issue.suggestions {
            self.suggestions.append(suggestion);
        }
        if let Some(suggestion) = issue.suggestions.first() {
            self.suggestions.set_selection(0, true);
            self.replacement.change_value(suggestion);
        } else {
            self.replacement.change_value(&issue.word);
        }
        self.message(&format!(
            "{} remaining · {} changed{}",
            session.report.issues.len(),
            session.changed,
            if issue.suggestions.is_empty() {
                " · No suggestions"
            } else {
                ""
            }
        ));
        if let Some(&(button, _)) = self
            .actions
            .iter()
            .find(|(_, action)| matches!(action, Action::Change))
        {
            button.set_default();
        }
        self.replacement.set_selection(0, -1);
        self.replacement.set_focus();
    }

    fn act(&self, ribbon: &Ribbon, action: Action) {
        match self.apply(ribbon, action) {
            Ok(()) => self.refresh(ribbon),
            Err(error) => self.message(&error),
        }
    }

    fn apply(&self, ribbon: &Ribbon, action: Action) -> Result<(), String> {
        let mut session = self.session.borrow_mut();
        if ribbon.state.borrow().editor.revision != session.revision {
            return Err("The document changed. Close this window and check spelling again.".into());
        }
        let Some(issue) = session.report.issues.first().cloned() else {
            return Ok(());
        };
        let same_word = |other: &Issue| {
            other.word == issue.word && other.language.eq_ignore_ascii_case(&issue.language)
        };
        match action {
            Action::Ignore => {
                session.report.issues.remove(0);
                session.reviewed += 1;
            }
            Action::IgnoreAll | Action::Add => {
                if matches!(action, Action::Add) {
                    session.dictionary.add(&issue.language, &issue.word)?;
                    crate::inline_spelling::invalidate(&mut ribbon.state.borrow_mut());
                }
                let before = session.report.issues.len();
                session.report.issues.retain(|other| !same_word(other));
                session.reviewed += before - session.report.issues.len();
            }
            Action::Change | Action::ChangeAll => {
                let replacement = self.replacement.get_value();
                if replacement.is_empty()
                    || replacement.contains(['\n', '\r', '\0'])
                    || replacement.len() > MAX_REPLACEMENT_BYTES
                {
                    return Err("Enter a single-line replacement of 1–1024 bytes.".into());
                }
                loop {
                    let next = if matches!(action, Action::ChangeAll) {
                        session
                            .report
                            .issues
                            .iter()
                            .find(|other| same_word(other))
                            .cloned()
                    } else {
                        Some(issue.clone())
                    };
                    let Some(next) = next else {
                        break;
                    };
                    replace(ribbon, &next, &replacement, session.revision)?;
                    session.revision = ribbon.state.borrow().editor.revision;
                    crate::spelling::adjust_after_replacement(
                        &mut session.report.issues,
                        &next,
                        replacement.len(),
                    );
                    session.reviewed += 1;
                    session.changed += 1;
                    if matches!(action, Action::Change) {
                        break;
                    }
                }
                crate::inline_spelling::invalidate(&mut ribbon.state.borrow_mut());
            }
        }
        if let Some(next) = session.report.issues.first() {
            ribbon.edit_without_focus(|editor| {
                editor.set_caret(next.node.clone(), next.start);
                editor.extend_selection(next.node.clone(), next.end);
                Ok(())
            })?;
        }
        Ok(())
    }
}

fn replace(ribbon: &Ribbon, issue: &Issue, replacement: &str, revision: u64) -> Result<(), String> {
    ribbon.edit_without_focus(|editor| {
        if editor.revision != revision
            || editor
                .paragraph_text(&issue.node)
                .as_ref()
                .and_then(|text| text.get(issue.start..issue.end))
                != Some(issue.word.as_str())
        {
            return Err("The text changed. Close this window and check spelling again.".into());
        }
        editor.replace_spelling_word(&issue.node, issue.start..issue.end, &issue.word, replacement, revision)
    })
}

/// Show bounded surrounding text without cutting a UTF-8 character.
fn context_preview(paragraph: &str, issue: &Issue) -> (String, i64, i64) {
    let Some(before) = paragraph.get(..issue.start) else {
        return (issue.word.clone(), 0, text_position(&issue.word));
    };
    let Some(after) = paragraph.get(issue.end..) else {
        return (issue.word.clone(), 0, text_position(&issue.word));
    };
    let left = before
        .chars()
        .rev()
        .take(CONTEXT_CHARS)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    let right = after.chars().take(CONTEXT_CHARS).collect::<String>();
    let prefix = if left.len() < before.len() { "…" } else { "" };
    let suffix = if right.len() < after.len() { "…" } else { "" };
    let start = text_position(prefix) + text_position(&left);
    (
        format!("{prefix}{left}{}{right}{suffix}", issue.word),
        start,
        start + text_position(&issue.word),
    )
}

fn text_position(text: &str) -> i64 {
    #[cfg(windows)]
    {
        text.encode_utf16().count() as i64
    }
    #[cfg(not(windows))]
    {
        text.chars().count() as i64
    }
}
