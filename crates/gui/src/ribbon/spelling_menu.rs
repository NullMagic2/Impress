//! Bottom-bar proofing controls and corrections for an underlined word.
use super::*;
use wxdragon::geometry::Point;

const MANAGE_ID: i32 = 31_006;
const INSTALL_ID: i32 = 31_005;
const TOGGLE_ID: i32 = 31_000;
const CHECK_ID: i32 = 31_001;
const HEADING_ID: i32 = 31_003;
const SELECTION_ID: i32 = 31_004;
const LANGUAGES_FIRST_ID: i32 = 31_100;
const LANGUAGES_LIMIT: usize = 256;
const ADD_ID: i32 = 31_400;
const SUGGESTIONS_FIRST_ID: i32 = 31_410;
const SUGGESTIONS_LIMIT: usize = 8;
const CUT_ID:i32=31_430;
const COPY_ID:i32=31_431;
const PASTE_ID:i32=31_432;

impl Ribbon {
    pub(super) fn show_spelling_menu(&self) {
        let (enabled, language) = {
            let state = self.state.borrow();
            (
                state.inline_spelling.enabled,
                crate::proofing::selection_language(&state.editor, &state.inline_spelling.language)
                    .unwrap_or_else(|| "Mixed".into()),
            )
        };
        let mut menu = Menu::builder()
            .append_check_item(
                TOGGLE_ID,
                &crate::i18n::text(&("Check spelling as you type")),
                &crate::i18n::text(&("Underline misspelled words in red")),
            )
            .append_item(
                SELECTION_ID,
                &crate::i18n::text(&("Check selection…")),
                &crate::i18n::text(&("Review only the selected text")),
            )
            .append_item(
                CHECK_ID,
                &crate::i18n::text(&("Check document…")),
                &crate::i18n::text(&("Review suggestions, ignore words or add them to your dictionary")),
            )
            .append_separator()
            .append_item(
                HEADING_ID,
                &crate::i18n::text(&(&format!("Language: {}", crate::spelling::language_label(&language)))),
                &crate::i18n::text(&("")),
            )
            .build();
        menu.check_item(TOGGLE_ID, enabled);
        menu.enable_item(
            SELECTION_ID,
            self.state
                .borrow()
                .editor
                .selection
                .as_ref()
                .is_some_and(|selection| !selection.is_collapsed()),
        );
        menu.enable_item(HEADING_ID, false);
        let installed = match crate::spelling::languages() {
            Ok(tags) => tags,
            Err(error) => {
                self.state.borrow_mut().action_status = Some(error);
                Vec::new()
            }
        };
        let mut builder = Menu::builder();
        for (index, choice) in crate::spelling::LANGUAGES.iter().enumerate() {
            let available = crate::spelling::dictionary_language(choice.tag, &installed).is_some();
            let label = if available {
                crate::i18n::text(choice.name)
            } else {
                format!("{} — dictionary not installed", crate::i18n::text(choice.name))
            };
            builder = builder.append_check_item(
                LANGUAGES_FIRST_ID + index as i32,
                &crate::i18n::text(&(&label)),
                &crate::i18n::text(&("Set the language of selected text or future typing")),
            );
        }
        let languages = builder.build();
        let selected =
            crate::spelling::dictionary_language(&language, &installed).unwrap_or(language.clone());
        if let Some(index) = crate::spelling::LANGUAGES.iter().position(|choice| {
            choice.tag.eq_ignore_ascii_case(&language)
                || crate::spelling::dictionary_language(choice.tag, &installed)
                    .is_some_and(|tag| tag.eq_ignore_ascii_case(&selected))
        }) {
            languages.check_item(LANGUAGES_FIRST_ID + index as i32, true);
        }
        menu.append_submenu(
            languages,
            &crate::i18n::text(&("Language")),
            &crate::i18n::text(&("Choose from twenty proofing languages")),
        );
        self.spelling_languages.replace(
            crate::spelling::LANGUAGES
                .iter()
                .map(|choice| choice.tag.to_string())
                .collect(),
        );
        menu.append_separator();
        menu.append(MANAGE_ID, &crate::i18n::text(&("Manage dictionaries…")), &crate::i18n::text(&("Add or delete spelling dictionaries")), wxdragon::menus::ItemKind::Normal);
        if self.dictionary_install.borrow().is_some() {
            menu.append_separator();
            menu.append(
                INSTALL_ID,
                "Dictionary progress…",
                "View dictionary progress or its result",
                wxdragon::menus::ItemKind::Normal,
            );
        }
        let button = self.ui.text_language_button;
        let anchor = crate::gtk_native::to_screen(&button, Point::new(0, 0));
        self.ui.main_frame.popup_menu(&mut menu, Some(anchor));
    }

    fn save_spelling_settings(&self) -> Result<(), String> {
        let state = self.state.borrow();
        crate::spelling::Settings {
            enabled: state.inline_spelling.enabled,
            language: state.inline_spelling.language.clone(),
        }
        .save()
    }

    pub(super) fn handle_spelling_menu(&self, id: i32) -> bool {
        let clipboard=match id {CUT_ID=>Some(AppCommand::Cut),COPY_ID=>Some(AppCommand::Copy),PASTE_ID=>Some(AppCommand::Paste),_=>None};
        if let Some(command)=clipboard {
            self.spelling_context.borrow_mut().take();
            self.run(Action::Editor(command));self.focus_document();return true;
        }
        let result = match id {
            TOGGLE_ID => {
                let result = {
                    let mut state = self.state.borrow_mut();
                    if state.inline_spelling.enabled {
                        crate::inline_spelling::disable(&mut state);
                        Ok(())
                    } else {
                        let language = state.inline_spelling.language.clone();
                        crate::inline_spelling::enable(&mut state, language)
                    }
                };
                result.and_then(|_| self.save_spelling_settings())
            }
            MANAGE_ID => { self.show_dictionary_manager(); Ok(()) }
            INSTALL_ID => {
                self.show_dictionary_install();
                Ok(())
            }
            CHECK_ID => self.start_spelling_scope(false),
            SELECTION_ID => self.start_spelling_scope(true),
            id if (LANGUAGES_FIRST_ID..LANGUAGES_FIRST_ID + LANGUAGES_LIMIT as i32)
                .contains(&id) =>
            {
                let tag = self
                    .spelling_languages
                    .borrow()
                    .get((id - LANGUAGES_FIRST_ID) as usize)
                    .cloned();
                if let Some(tag) = tag {
                    self.edit(|editor| {
                        editor.apply_character_properties(&CharacterProperties {
                            language: Some(tag.clone()),
                            ..Default::default()
                        })
                    })
                    .and_then(|_| {
                        let mut state = self.state.borrow_mut();
                        crate::inline_spelling::invalidate(&mut state);
                        Ok(())
                    })
                    .and_then(|_| self.save_spelling_settings())
                    .and_then(|_| {
                        let installed = crate::spelling::languages().unwrap_or_default();
                        if crate::spelling::dictionary_language(&tag, &installed).is_none() {
                            self.request_dictionary_install(&tag)?;
                        }
                        Ok(())
                    })
                } else {
                    Ok(())
                }
            }
            ADD_ID => self.correct_spelling(None),
            id if (SUGGESTIONS_FIRST_ID..SUGGESTIONS_FIRST_ID + SUGGESTIONS_LIMIT as i32)
                .contains(&id) =>
            {
                self.correct_spelling(Some((id - SUGGESTIONS_FIRST_ID) as usize))
            }
            _ => return false,
        };
        if let Err(error) = result {
            self.state.borrow_mut().action_status = Some(error);
        }
        self.sync_spelling_status();
        true
    }

    pub(super) fn sync_spelling_status(&self) {
        skin::selected(
            &self.ui.text_language_button,
            self.state.borrow().inline_spelling.enabled,
        );
    }

    pub(super) fn show_spelling_context(&self, at: Point) -> bool {
        let context = {
            let state = self.state.borrow();
            crate::inline_spelling::issue_at_caret(&state)
                .map(|issue| (state.layout_generation, state.editor.revision, issue))
        };
        let Some((generation, revision, issue)) = context else {
            return false;
        };
        let mut builder = Menu::builder()
            .append_item(CUT_ID,&format!("{}\tCtrl+X",crate::i18n::text("Cut")),"")
            .append_item(COPY_ID,&format!("{}\tCtrl+C",crate::i18n::text("Copy")),"")
            .append_item(PASTE_ID,&format!("{}\tCtrl+V",crate::i18n::text("Paste")),"")
            .append_separator();
        if issue.suggestions.is_empty() {
            builder = builder.append_item(HEADING_ID, &crate::i18n::text(&("No suggestions")), &crate::i18n::text(&("")));
        } else {
            for (index, suggestion) in issue.suggestions.iter().take(SUGGESTIONS_LIMIT).enumerate()
            {
                builder = builder.append_item(
                    SUGGESTIONS_FIRST_ID + index as i32,
                    &crate::i18n::text(&(&suggestion.replace('&', "&&"))),
                    &crate::i18n::text(&("Replace the misspelled word")),
                );
            }
        }
        let mut menu = builder
            .append_separator()
            .append_item(
                ADD_ID,
                &crate::i18n::text(&("Add to dictionary")),
                &crate::i18n::text(&("Accept this word in future spelling checks")),
            )
            .append_item(
                CHECK_ID,
                &crate::i18n::text(&("Check document…")),
                &crate::i18n::text(&("Review spelling throughout the document")),
            )
            .append_separator()
            .append_check_item(
                TOGGLE_ID,
                &crate::i18n::text(&("Check spelling as you type")),
                &crate::i18n::text(&("Underline misspelled words in red")),
            )
            .build();
        menu.enable_item(HEADING_ID, false);
        menu.check_item(TOGGLE_ID, true);
        let has_selection={let s=self.state.borrow();s.selected_object.is_some() || s.editor.selection.as_ref().is_some_and(|v|!v.is_collapsed())};
        menu.enable_item(CUT_ID,has_selection);menu.enable_item(COPY_ID,has_selection);
        if let Some(target) = self.table_context_menu.borrow().as_ref() {
            target.append_commands(&menu);
        }
        self.spelling_context
            .replace(Some((generation, revision, issue)));
        self.ui.main_frame.popup_menu(&mut menu, Some(at));
        self.spelling_context.take();
        true
    }

    fn correct_spelling(&self, suggestion: Option<usize>) -> Result<(), String> {
        let Some((generation, revision, issue)) = self.spelling_context.borrow_mut().take() else {
            return Ok(());
        };
        {
            let state = self.state.borrow();
            if state.layout_generation != generation
                || state.editor.revision != revision
                || state
                    .editor
                    .paragraph_text(&issue.node)
                    .as_ref()
                    .and_then(|text| text.get(issue.start..issue.end))
                    != Some(issue.word.as_str())
            {
                return Err(
                    "The text changed. Right-click the word again for fresh suggestions.".into(),
                );
            }
        }
        if let Some(index) = suggestion {
            let replacement = issue
                .suggestions
                .get(index)
                .ok_or("This suggestion is no longer available")?;
            self.edit(|editor| {
                editor.set_caret(issue.node.clone(), issue.start);
                editor.extend_selection(issue.node.clone(), issue.end);
                editor.insert_text(replacement)
            })?;
        } else {
            crate::spelling::Dictionary::load()?.add(&issue.language, &issue.word)?;
        }
        crate::inline_spelling::invalidate(&mut self.state.borrow_mut());
        Ok(())
    }
}
