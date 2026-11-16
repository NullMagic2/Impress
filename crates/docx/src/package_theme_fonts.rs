//! Write regional fonts without flattening the editable theme and style choices.
use super::*;
use model::{FontHint, FontRegion, ThemeFontSet};

fn attribute(xml: &mut String, name: &str, value: &str) {
    xml.push_str(&format!(" w:{name}=\"{}\"", xml_escape(value)));
}
pub(super) fn properties(properties: &CharacterProperties) -> String {
    let mut fonts = String::new();
    if let Some(slots) = &properties.font_slots {
        for (region, direct, theme) in [
            (FontRegion::Ascii, "ascii", "asciiTheme"),
            (FontRegion::HighAnsi, "hAnsi", "hAnsiTheme"),
            (FontRegion::EastAsia, "eastAsia", "eastAsiaTheme"),
            (FontRegion::ComplexScript, "cs", "cstheme"),
        ] {
            let Some(slot) = slots.slot(region) else {
                continue;
            };
            if let Some(family) = slot.family.as_deref().or(slot.fallback_family.as_deref()) {
                attribute(&mut fonts, direct, family);
            }
            if let Some(reference) = slot.theme {
                let set = match reference.set {
                    ThemeFontSet::Major => "major",
                    ThemeFontSet::Minor => "minor",
                };
                let suffix = match reference.region {
                    FontRegion::Ascii => "Ascii",
                    FontRegion::HighAnsi => "HAnsi",
                    FontRegion::EastAsia => "EastAsia",
                    FontRegion::ComplexScript => "Bidi",
                };
                attribute(&mut fonts, theme, &format!("{set}{suffix}"));
            }
        }
        if let Some(hint) = slots.hint {
            attribute(
                &mut fonts,
                "hint",
                match hint {
                    FontHint::Default => "default",
                    FontHint::EastAsia => "eastAsia",
                    FontHint::ComplexScript => "cs",
                },
            );
        }
    } else if let Some(family) = &properties.font_family {
        for name in ["ascii", "hAnsi", "eastAsia", "cs"] {
            attribute(&mut fonts, name, family);
        }
    }
    let mut result = if fonts.is_empty() {
        String::new()
    } else {
        format!("<w:rFonts{fonts}/>")
    };
    if let Some(force) = properties
        .font_slots
        .as_ref()
        .and_then(|slots| slots.force_complex_script)
    {
        result.push_str(if force {
            "<w:cs/>"
        } else {
            "<w:cs w:val=\"0\"/>"
        });
    }
    let mut languages = String::new();
    if let Some(language) = &properties.language {
        attribute(&mut languages, "val", language);
    }
    if let Some(slots) = &properties.font_slots {
        if let Some(language) = &slots.east_asia_language {
            attribute(&mut languages, "eastAsia", language);
        }
        if let Some(language) = &slots.complex_script_language {
            attribute(&mut languages, "bidi", language);
        }
    }
    if !languages.is_empty() {
        result.push_str(&format!("<w:lang{languages}/>"));
    }
    result
}
