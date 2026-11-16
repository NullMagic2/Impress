//! Stable proofing choices and their platform dictionary setup instructions.
#[derive(Clone, Copy)]
pub struct Language {
    pub tag: &'static str,
    pub name: &'static str,
}

/// The same twenty choices appear on Linux and Windows, even before a
/// dictionary is installed. Regional Portuguese dictionaries stay distinct.
pub const LANGUAGES: [Language; 20] = [
    Language {
        tag: "cs-CZ",
        name: "Czech",
    },
    Language {
        tag: "da-DK",
        name: "Danish",
    },
    Language {
        tag: "nl-NL",
        name: "Dutch",
    },
    Language {
        tag: "en-GB",
        name: "English (United Kingdom)",
    },
    Language {
        tag: "en-US",
        name: "English (United States)",
    },
    Language {
        tag: "fr-FR",
        name: "French",
    },
    Language {
        tag: "de-DE",
        name: "German",
    },
    Language {
        tag: "el-GR",
        name: "Greek",
    },
    Language {
        tag: "hu-HU",
        name: "Hungarian",
    },
    Language {
        tag: "it-IT",
        name: "Italian",
    },
    Language {
        tag: "nb-NO",
        name: "Norwegian (Bokmål)",
    },
    Language {
        tag: "pl-PL",
        name: "Polish",
    },
    Language {
        tag: "pt-BR",
        name: "Portuguese (Brazil)",
    },
    Language {
        tag: "pt-PT",
        name: "Portuguese (Portugal)",
    },
    Language {
        tag: "ro-RO",
        name: "Romanian",
    },
    Language {
        tag: "ru-RU",
        name: "Russian",
    },
    Language {
        tag: "sk-SK",
        name: "Slovak",
    },
    Language {
        tag: "es-ES",
        name: "Spanish",
    },
    Language {
        tag: "sv-SE",
        name: "Swedish",
    },
    Language {
        tag: "uk-UA",
        name: "Ukrainian",
    },
];

pub fn language_label(tag: &str) -> String { crate::i18n::text(&language_name(tag)) }
fn language_name(tag: &str) -> String {
    let normalized = tag.replace('_', "-");
    if let Some(language) = LANGUAGES
        .iter()
        .find(|language| language.tag.eq_ignore_ascii_case(&normalized))
    {
        return language.name.to_string();
    }
    match normalized.to_ascii_lowercase().as_str() {
        "en" => "English".into(),
        "pt" => "Portuguese".into(),
        "en-au" => "English (Australia)".into(),
        "en-ca" => "English (Canada)".into(),
        _ => LANGUAGES
            .iter()
            .find(|language| {
                language
                    .tag
                    .split('-')
                    .next()
                    .is_some_and(|base| base.eq_ignore_ascii_case(&normalized))
            })
            .map_or_else(|| tag.to_string(), |language| language.name.to_string()),
    }
}

/// Match actual providers without substituting one Portuguese or English
/// region for another. A neutral Greek dictionary can serve `el-GR`.
pub fn dictionary_language(requested: &str, installed: &[String]) -> Option<String> {
    let requested = requested.replace('_', "-");
    if let Some(tag) = installed
        .iter()
        .find(|tag| tag.replace('_', "-").eq_ignore_ascii_case(&requested))
    {
        return Some(tag.clone());
    }
    let base = requested.split('-').next()?;
    if requested.contains('-') {
        let regions = LANGUAGES
            .iter()
            .filter(|language| {
                language
                    .tag
                    .split('-')
                    .next()
                    .is_some_and(|tag| tag.eq_ignore_ascii_case(base))
            })
            .count();
        if regions > 1 {
            return None;
        }
        return installed
            .iter()
            .find(|tag| tag.eq_ignore_ascii_case(base))
            .cloned();
    }
    if requested.eq_ignore_ascii_case("en") {
        if let Some(tag) = installed
            .iter()
            .find(|tag| tag.eq_ignore_ascii_case("en-US"))
        {
            return Some(tag.clone());
        }
    }
    installed
        .iter()
        .find(|tag| {
            tag.replace('_', "-")
                .split('-')
                .next()
                .is_some_and(|tag| tag.eq_ignore_ascii_case(base))
        })
        .cloned()
}
