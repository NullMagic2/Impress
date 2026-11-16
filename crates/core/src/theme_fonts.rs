//! Concrete regional font choices and theme provenance shared by import and shaping.
use crate::CharacterProperties;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, ops::Range};
use unicode_segmentation::UnicodeSegmentation;

/// The regional font slot used by WordprocessingML and other multilingual formats.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FontRegion {
    Ascii,
    HighAnsi,
    EastAsia,
    ComplexScript,
}
/// Heading/body collections in a document font theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeFontSet {
    Major,
    Minor,
}
/// A theme reference retained alongside its already resolved concrete fonts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemeFontReference {
    pub set: ThemeFontSet,
    pub region: FontRegion,
}
/// Concrete typefaces for one regional slot. Supplemental keys are ISO15924 scripts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FontSlot {
    #[serde(default)]
    pub family: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub scripts: BTreeMap<String, String>,
    #[serde(default)]
    pub fallback_family: Option<String>,
    #[serde(default)]
    pub theme: Option<ThemeFontReference>,
}
impl FontSlot {
    pub fn explicit(family: impl Into<String>) -> Self {
        Self {
            family: Some(family.into()),
            ..Default::default()
        }
    }
    pub fn family_for(&self, character: char, language: Option<&str>) -> Option<&str> {
        self.family
            .as_deref()
            .filter(|family| !family.is_empty())
            .or_else(|| {
                language
                    .and_then(language_script)
                    .and_then(|script| self.scripts.get(script).map(String::as_str))
            })
            .or_else(|| {
                character_script(character)
                    .and_then(|script| self.scripts.get(script).map(String::as_str))
            })
            .or(self.fallback_family.as_deref())
    }
}
/// Font hints affect ambiguous punctuation and symbols rather than authored text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FontHint {
    Default,
    EastAsia,
    ComplexScript,
}
/// Sparse regional font settings. Each specified slot replaces that inherited slot.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FontSlots {
    #[serde(default)]
    pub ascii: Option<FontSlot>,
    #[serde(default)]
    pub h_ansi: Option<FontSlot>,
    #[serde(default)]
    pub east_asia: Option<FontSlot>,
    #[serde(default)]
    pub complex_script: Option<FontSlot>,
    #[serde(default)]
    pub hint: Option<FontHint>,
    #[serde(default)]
    pub force_complex_script: Option<bool>,
    #[serde(default)]
    pub east_asia_language: Option<String>,
    #[serde(default)]
    pub complex_script_language: Option<String>,
}
impl FontSlots {
    pub fn overlay(&mut self, other: &Self) {
        macro_rules! apply {($($field:ident),*)=>{$(if other.$field.is_some(){self.$field=other.$field.clone();})*};}
        apply!(
            ascii,
            h_ansi,
            east_asia,
            complex_script,
            hint,
            force_complex_script,
            east_asia_language,
            complex_script_language
        );
    }
    pub fn slot(&self, region: FontRegion) -> Option<&FontSlot> {
        match region {
            FontRegion::Ascii => self.ascii.as_ref(),
            FontRegion::HighAnsi => self.h_ansi.as_ref(),
            FontRegion::EastAsia => self.east_asia.as_ref(),
            FontRegion::ComplexScript => self.complex_script.as_ref(),
        }
    }
    fn region(&self, character: char, rtl: Option<bool>, language: Option<&str>) -> FontRegion {
        if self.force_complex_script == Some(true) || rtl == Some(true) {
            return FontRegion::ComplexScript;
        }
        let value = character as u32;
        if self.hint == Some(FontHint::ComplexScript) && character_script(character).is_none() {
            return FontRegion::ComplexScript;
        }
        let east_hint = self.hint == Some(FontHint::EastAsia);
        let chinese = self
            .east_asia_language
            .as_deref()
            .or(language)
            .is_some_and(|language| language.to_ascii_lowercase().starts_with("zh"));
        if value <= 0x7F {
            return FontRegion::Ascii;
        }
        if matches!(value,0x1100..=0x11FF|0x2F00..=0x4DBF|0x4E00..=0x9FFF|0xA000..=0xA4CF|0xAC00..=0xD7AF|0xF900..=0xFAFF|0xFE30..=0xFE6F|0xFF00..=0xFFEF|0x10000..=0x10FFFF)
        {
            return FontRegion::EastAsia;
        }
        if east_hint
            && (matches!(value,0x2B0..=0x4FF|0x2000..=0x27BF|0x2E80..=0x2EFF|0xE000..=0xF8FF|0xFB00..=0xFB1C|0xA1|0xA4|0xA7..=0xA8|0xAA|0xAD|0xAF|0xB0..=0xB4|0xB6..=0xBA|0xBC..=0xBF|0xD7|0xF7)
                || (chinese
                    && matches!(value,0xE0..=0xE1|0xE8..=0xEA|0xEC..=0xED|0xF2..=0xF3|0xF9..=0xFA|0xFC|0x100..=0x2AF|0x1E00..=0x1EFF)))
        {
            return FontRegion::EastAsia;
        }
        if matches!(value,0x590..=0x7BF|0xFB1D..=0xFDFF|0xFE70..=0xFEFE) {
            return FontRegion::Ascii;
        }
        FontRegion::HighAnsi
    }
    fn family_for(
        &self,
        character: char,
        rtl: Option<bool>,
        language: Option<&str>,
    ) -> Option<&str> {
        let region = self.region(character, rtl, language);
        if region != FontRegion::ComplexScript
            && self
                .east_asia
                .as_ref()
                .and_then(|slot| slot.family.as_deref())
                .is_some_and(|name| name.eq_ignore_ascii_case("Times New Roman"))
            && self
                .ascii
                .as_ref()
                .and_then(|slot| slot.family_for('a', language))
                == self
                    .h_ansi
                    .as_ref()
                    .and_then(|slot| slot.family_for('a', language))
        {
            return self
                .ascii
                .as_ref()
                .and_then(|slot| slot.family_for(character, language));
        }
        let language = match region {
            FontRegion::EastAsia => self.east_asia_language.as_deref().or_else(|| {
                language.filter(|lang| {
                    matches!(
                        language_script(lang),
                        Some("Jpan" | "Kore" | "Hans" | "Hant" | "Yiii")
                    )
                })
            }),
            FontRegion::ComplexScript => self.complex_script_language.as_deref().or(language),
            _ => language,
        };
        self.slot(region)
            .and_then(|slot| slot.family_for(character, language))
    }
}
/// UTF8 byte ranges that can be shaped with one requested font family.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontSpan<'a> {
    pub range: Range<usize>,
    pub family: Option<&'a str>,
}
impl CharacterProperties {
    /// Resolve actual regional fonts after style inheritance and direct overrides.
    pub fn font_family_for(&self, character: char) -> Option<&str> {
        self.font_slots
            .as_ref()
            .and_then(|slots| {
                slots.family_for(character, self.right_to_left, self.language.as_deref())
            })
            .or(self.font_family.as_deref())
    }
    /// Keep grapheme clusters intact when regional font choices split one run.
    pub fn font_spans<'a>(&'a self, text: &str) -> Vec<FontSpan<'a>> {
        let mut spans: Vec<FontSpan<'a>> = Vec::new();
        for (start, cluster) in text.grapheme_indices(true) {
            let Some(character) = cluster.chars().next() else {
                continue;
            };
            let family = self.font_family_for(character);
            let end = start + cluster.len();
            if let Some(last) = spans.last_mut().filter(|span| span.family == family) {
                last.range.end = end;
            } else {
                spans.push(FontSpan {
                    range: start..end,
                    family,
                });
            }
        }
        spans
    }
    /// A manual font selection replaces inherited regional/theme choices.
    pub fn set_font_family(&mut self, family: Option<String>) {
        self.font_family = family;
        self.font_slots = None;
    }
}
/// Resolve common BCP47 language tags and explicit script subtags to ISO15924.
pub fn language_script(language: &str) -> Option<&'static str> {
    let normalized = language.replace('_', "-").to_ascii_lowercase();
    for part in normalized.split('-').skip(1) {
        let script = match part {
            "latn" => Some("Latn"),
            "cyrl" => Some("Cyrl"),
            "arab" => Some("Arab"),
            "hebr" => Some("Hebr"),
            "hans" => Some("Hans"),
            "hant" => Some("Hant"),
            "deva" => Some("Deva"),
            "mong" => Some("Mong"),
            _ => None,
        };
        if script.is_some() {
            return script;
        }
    }
    Some(match normalized.split('-').next()? {
        "ja" => "Jpan",
        "ko" => "Kore",
        "zh" if normalized
            .split('-')
            .any(|part| matches!(part, "tw" | "hk" | "mo")) =>
        {
            "Hant"
        }
        "zh" => "Hans",
        "ar" | "fa" | "ur" | "ps" | "sd" => "Arab",
        "he" | "iw" | "yi" => "Hebr",
        "el" => "Grek",
        "ru" | "uk" | "bg" | "mk" | "be" | "kk" | "ky" | "sr" => "Cyrl",
        "hi" | "mr" | "ne" | "sa" => "Deva",
        "bn" | "as" => "Beng",
        "pa" => "Guru",
        "gu" => "Gujr",
        "or" | "od" => "Orya",
        "ta" => "Taml",
        "te" => "Telu",
        "kn" => "Knda",
        "ml" => "Mlym",
        "si" => "Sinh",
        "th" => "Thai",
        "lo" => "Laoo",
        "km" => "Khmr",
        "my" => "Mymr",
        "bo" => "Tibt",
        "am" | "ti" => "Ethi",
        "hy" => "Armn",
        "ka" => "Geor",
        "dv" => "Thaa",
        "syr" => "Syrc",
        "mn" => "Cyrl",
        "ii" => "Yiii",
        "en" | "pt" | "es" | "fr" | "de" | "it" | "nl" | "da" | "sv" | "no" | "nb" | "pl"
        | "cs" | "sk" | "ro" | "hu" | "tr" | "vi" | "id" | "ms" | "fi" | "et" | "lv" | "lt"
        | "az" | "uz" | "sw" | "af" => "Latn",
        _ => return None,
    })
}
fn character_script(character: char) -> Option<&'static str> {
    Some(match character as u32 {
        0x41..=0x5A | 0x61..=0x7A | 0xC0..=0x2AF | 0x1E00..=0x1EFF => "Latn",
        0x370..=0x3FF | 0x1F00..=0x1FFF => "Grek",
        0x400..=0x52F => "Cyrl",
        0x590..=0x5FF | 0xFB1D..=0xFB4F => "Hebr",
        0x600..=0x6FF | 0x750..=0x77F | 0x8A0..=0x8FF | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF => "Arab",
        0x700..=0x74F => "Syrc",
        0x780..=0x7BF => "Thaa",
        0x900..=0x97F => "Deva",
        0x980..=0x9FF => "Beng",
        0xA00..=0xA7F => "Guru",
        0xA80..=0xAFF => "Gujr",
        0xB00..=0xB7F => "Orya",
        0xB80..=0xBFF => "Taml",
        0xC00..=0xC7F => "Telu",
        0xC80..=0xCFF => "Knda",
        0xD00..=0xD7F => "Mlym",
        0xD80..=0xDFF => "Sinh",
        0xE00..=0xE7F => "Thai",
        0xE80..=0xEFF => "Laoo",
        0xF00..=0xFFF => "Tibt",
        0x1000..=0x109F => "Mymr",
        0x1100..=0x11FF | 0x3130..=0x318F | 0xAC00..=0xD7AF => "Kore",
        0x1200..=0x137F => "Ethi",
        0x1780..=0x17FF => "Khmr",
        0x3040..=0x30FF => "Jpan",
        0x2E80..=0x303F | 0x3400..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x323AF => "Hans",
        0xA000..=0xA4CF => "Yiii",
        _ => return None,
    })
}

#[cfg(test)]
#[path = "../tests/unit/theme_fonts.rs"]
mod tests;
