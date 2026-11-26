//! Interface language preferences and translated UI text, separate from documents.
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU8, Ordering},
        OnceLock,
    },
};

mod widgets;

const SETTINGS_FILE: &str = "interface-language.json";
const CATALOG: &str = include_str!("../assets/locales/translations.json");
static LANGUAGE: AtomicU8 = AtomicU8::new(0);
static STRINGS: OnceLock<HashMap<String, [String; 4]>> = OnceLock::new();

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[default]
    English,
    Greek,
    Spanish,
    PortugueseBrazil,
    PortuguesePortugal,
}
impl Language {
    pub const ALL: [Self; 5] = [
        Self::English,
        Self::Greek,
        Self::Spanish,
        Self::PortugueseBrazil,
        Self::PortuguesePortugal,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Greek => "Ελληνικά",
            Self::Spanish => "Español",
            Self::PortugueseBrazil => "Português (Brasil)",
            Self::PortuguesePortugal => "Português (Portugal)",
        }
    }
    pub fn tag(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Greek => "el",
            Self::Spanish => "es",
            Self::PortugueseBrazil => "pt-BR",
            Self::PortuguesePortugal => "pt-PT",
        }
    }
}
pub fn language() -> Language {
    Language::ALL
        .get(LANGUAGE.load(Ordering::Relaxed) as usize)
        .copied()
        .unwrap_or_default()
}
pub fn initialize() {
    let load = || -> Option<Language> {
        let file =
            std::fs::File::open(crate::data_dir::user_data_dir()?.join(SETTINGS_FILE)).ok()?;
        use std::io::Read;
        serde_json::from_reader(file.take(1024)).ok()
    };
    LANGUAGE.store(load().unwrap_or_default() as u8, Ordering::Relaxed);
}
pub fn select(language: Language) -> Result<(), String> {
    let root = crate::data_dir::user_data_dir().ok_or("The user data folder is unavailable.")?;
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec(&language).map_err(|e| e.to_string())?;
    format::atomic_write(&root.join(SETTINGS_FILE), |file| {
        use std::io::Write;
        file.write_all(&bytes).map_err(|e| e.to_string())
    })?;
    LANGUAGE.store(language as u8, Ordering::Relaxed);
    install_native();
    Ok(())
}
fn catalog() -> &'static HashMap<String, [String; 4]> {
    STRINGS.get_or_init(|| serde_json::from_str(CATALOG).expect("validated interface catalog"))
}
struct Template {
    expression: Regex,
    needle: String,
    fields: Vec<String>,
    values: [String; 4],
}
fn placeholders() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"\{[^{}]*\}").expect("format placeholder pattern"))
}
static TEMPLATES: OnceLock<Vec<Template>> = OnceLock::new();
fn templates() -> &'static [Template] {
    TEMPLATES.get_or_init(|| {
        let fields = placeholders();
        let mut rows = catalog()
            .iter()
            .filter(|(key, _)| fields.is_match(key))
            .collect::<Vec<_>>();
        rows.sort_by_key(|(key, _)| std::cmp::Reverse(fields.replace_all(key, "").len()));
        rows.into_iter()
            .filter_map(|(key, values)| {
                if fields.replace_all(key, "").trim().len() < 4 {
                    return None;
                }
                let mut expression = String::from("(?s)^");
                let mut names = Vec::new();
                let mut end = 0;
                for field in fields.find_iter(key) {
                    expression.push_str(&regex::escape(&key[end..field.start()]));
                    expression.push_str("(.*?)");
                    names.push(field.as_str().to_string());
                    end = field.end();
                }
                expression.push_str(&regex::escape(&key[end..]));
                expression.push('$');
                let needle = fields
                    .split(key)
                    .max_by_key(|part| part.len())
                    .unwrap_or("")
                    .to_string();
                Some(Template {
                    expression: Regex::new(&expression).ok()?,
                    needle,
                    fields: names,
                    values: values.clone(),
                })
            })
            .collect()
    })
}
/// Translate interface output; captured document text and filenames stay intact.
pub fn text(source: &str) -> String {
    text_inner(source, 0)
}
fn text_inner(source: &str, depth: usize) -> String {
    translate(source, depth, language() as usize)
}
fn translate(source: &str, depth: usize, index: usize) -> String {
    if index == 0 {
        return source.to_string();
    }
    if let Some(row) = catalog().get(source) {
        return row[index - 1].clone();
    }
    if source.len() > 65536 || depth > 3 {
        return source.to_string();
    }
    // Decorative caption suffixes are added at runtime by several painted
    // controls. Translate the caption before restoring the arrow/ellipsis.
    for suffix in [" ▾", "…", "..."] {
        if let Some(caption) = source.strip_suffix(suffix).filter(|s| !s.is_empty()) {
            let translated = translate(caption, depth + 1, index);
            if translated != caption { return format!("{translated}{suffix}"); }
        }
    }
    for template in templates() {
        if !source.contains(&template.needle) {
            continue;
        }
        let Some(captures) = template.expression.captures(source) else {
            continue;
        };
        let translated = &template.values[index - 1];
        let mut output = String::new();
        let mut end = 0;
        let mut position = 0;
        let placeholders = placeholders();
        for field in placeholders.find_iter(translated) {
            output.push_str(&translated[end..field.start()]);
            let at = if field.as_str() == "{}" {
                let at = position;
                position += 1;
                at
            } else {
                template
                    .fields
                    .iter()
                    .position(|value| value == field.as_str())
                    .unwrap_or(position)
            };
            let value = captures.get(at + 1).map_or("", |capture| capture.as_str());
            if [
                "err",
                "message",
                "operation",
                "label",
                "kind",
                "scope",
                "status",
                "hint",
                "notes",
                "reason",
            ]
            .iter()
            .any(|name| field.as_str().contains(name))
                || matches!(field.as_str(), "{e}" | "{e:?}")
            {
                output.push_str(&translate(value, depth + 1, index));
            } else {
                output.push_str(value);
            }
            end = field.end();
        }
        output.push_str(&translated[end..]);
        return output;
    }
    source.to_string()
}
/// Decode a displayed enum choice before comparing it with stable model values.
pub fn source(value: &str) -> String {
    let index = language() as usize;
    if index != 0 {
        if let Some((key, _)) = catalog().iter().find(|(_, row)| row[index - 1] == value) {
            return key.clone();
        }
    }
    value.to_string()
}
pub trait Localize {
    fn localize(&self);
}
pub fn localized<T: Localize>(ui: T) -> T {
    ui.localize();
    ui
}

pub fn choices(values: Vec<String>) -> Vec<String> {
    values.iter().map(|value| text(value)).collect()
}

/// Enum controls resolve only against their own stable choices, avoiding
/// ambiguous translations shared by unrelated commands.
pub fn choice_source(value: &str, choices: &[&str]) -> String {
    choices
        .iter()
        .find(|choice| text(choice) == value)
        .map_or_else(|| value.to_string(), |choice| choice.to_string())
}

struct NativeCatalog;
impl wxdragon::translations::TranslationsLoader for NativeCatalog {
    fn available_translations(&self, domain: &str) -> Vec<String> {
        if domain == "impress" {
            vec!["el".into(), "es".into(), "pt_BR".into(), "pt_PT".into()]
        } else {
            Vec::new()
        }
    }
    fn load_catalog(&self, domain: &str, language: &str) -> Option<std::borrow::Cow<'_, [u8]>> {
        if domain != "impress" {
            return None;
        }
        let bytes: &[u8] = match language.replace('-', "_").as_str() {
            "el" | "el_GR" => include_bytes!("../assets/locales/el.mo"),
            "es" | "es_ES" => include_bytes!("../assets/locales/es.mo"),
            "pt_BR" => include_bytes!("../assets/locales/pt_BR.mo"),
            "pt" | "pt_PT" => include_bytes!("../assets/locales/pt_PT.mo"),
            _ => return None,
        };
        Some(std::borrow::Cow::Borrowed(bytes))
    }
}
/// Called on the GUI thread, after wxWidgets initialization.
pub fn install_native() {
    let translations = wxdragon::translations::Translations::new();
    translations.set_loader(NativeCatalog);
    // wxWidgets matches catalog names literally and expects underscore locales.
    translations.set_language_str(&language().tag().replace('-', "_"));
    if language() != Language::English {
        translations.add_catalog("impress");
    }
    wxdragon::translations::Translations::set_global(translations);
}

/// Built-in style labels are localized without renaming styles in the document.
pub fn style_name(style: &model::Style) -> String {
    if style.id.0.starts_with("builtin.") {
        text(&style.name)
    } else {
        style.name.clone()
    }
}

pub fn control_value(name: &str, value: &str) -> String {
    widgets::canonical_choice(name, value)
}
/// Translate wildcard descriptions while keeping filename patterns unchanged.
pub fn wildcard(value: &str) -> String {
    value
        .split('|')
        .enumerate()
        .map(|(index, part)| {
            if index % 2 == 1 {
                return part.to_string();
            }
            if let Some(at) = part.rfind(" (") {
                format!("{}{}", text(&part[..at]), &part[at..])
            } else {
                text(part)
            }
        })
        .collect::<Vec<_>>()
        .join("|")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_errors_translate_in_every_shipped_language() {
        for index in 1..=4 {
            let source = "Simplified document view: Invalid floating image dimensions";
            let expected = catalog()["Simplified document view: {error}"][index - 1]
                .replace("{error}", &catalog()["Invalid floating image dimensions"][index - 1]);
            assert_eq!(translate(source, 0, index), expected);
            let source = "Cannot embed My Font: Invalid font. Save without embedding to continue.";
            let template = "Cannot embed {family}: {e}. Save without embedding to continue.";
            assert_eq!(translate(source, 0, index), catalog()[template][index - 1]
                    .replace("{family}", "My Font").replace("{e}", &catalog()["Invalid font"][index - 1]));
        }
    }

    #[test]
    fn captured_document_values_remain_unchanged() {
        for index in 1..=4 {
            let template = "Unknown function \"{callee}\"";
            assert_eq!(translate("Unknown function \"Invalid font\"", 0, index),
                catalog()[template][index - 1].replace("{callee}", "Invalid font"));
        }
    }
    #[test]
    fn decorated_captions_and_multiline_toolbar_labels_translate() {
        for index in 1..=4 {
            for source in ["Select", "Text color", "Cell fill"] {
                assert_eq!(translate(&format!("{source} ▾"),0,index),
                    format!("{} ▾",catalog()[source][index-1]));
            }
            for source in ["Insert\nAbove", "Insert\nBelow", "Close\nTable Tools", "Close\nPicture Tools"] {
                let value=translate(source,0,index);
                assert_eq!(value.lines().count(),2,"{source}: {value}");
                assert_ne!(value,source);
            }
        }
    }

    #[test]
    fn every_non_english_language_has_a_matching_native_catalog_tag() {
        use wxdragon::translations::TranslationsLoader;
        let available=NativeCatalog.available_translations("impress");
        for language in Language::ALL.into_iter().skip(1) {
            let tag=language.tag().replace('-',"_");
            assert!(available.contains(&tag),"{tag} must match the native catalog name");
            assert!(NativeCatalog.load_catalog("impress",&tag).is_some());
        }
    }

}
