//! Preserve native list definitions and instance overrides as editable OOXML.
use super::{Level, Lists};
use crate::write::esc_attr;

const WORD_NAMESPACE: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
impl Lists {
    pub(crate) fn source_xml(&self, fonts: &[String]) -> String {
        let mut xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><w:numbering xmlns:w="{WORD_NAMESPACE}">"#
        );
        for (index, definition) in self.defs.iter().enumerate() {
            xml.push_str(&format!(
                r#"<w:abstractNum w:abstractNumId="{}"><w:multiLevelType w:val="{}"/>"#,
                index + 1,
                if definition.simple {
                    "singleLevel"
                } else {
                    "multilevel"
                }
            ));
            for (level_index, level) in definition.levels.iter().enumerate() {
                xml.push_str(&level_xml(level, level_index, fonts));
            }
            xml.push_str("</w:abstractNum>");
        }
        for (index, instance) in self.lfos.iter().enumerate() {
            let Some(definition) = self
                .defs
                .iter()
                .position(|definition| definition.lsid == instance.lsid)
            else {
                continue;
            };
            xml.push_str(&format!(
                r#"<w:num w:numId="{}"><w:abstractNumId w:val="{}"/>"#,
                index + 1,
                definition + 1
            ));
            for level_index in 0..9 {
                let Some(override_) = instance.level(level_index) else {
                    continue;
                };
                xml.push_str(&format!(r#"<w:lvlOverride w:ilvl="{level_index}">"#));
                if let Some(start) = override_.start_at {
                    xml.push_str(&format!(r#"<w:startOverride w:val="{start}"/>"#));
                }
                if let Some(level) = &override_.formatting {
                    xml.push_str(&level_xml(level, level_index, fonts));
                }
                xml.push_str("</w:lvlOverride>");
            }
            xml.push_str("</w:num>");
        }
        xml.push_str("</w:numbering>");
        xml
    }
}
fn level_xml(level: &Level, index: usize, fonts: &[String]) -> String {
    let format = match level.nfc {
        1 => "upperRoman",
        2 => "lowerRoman",
        3 => "upperLetter",
        4 => "lowerLetter",
        23 => "bullet",
        255 => "none",
        _ => "decimal",
    };
    let mut template = String::new();
    let mut literal = Vec::new();
    for unit in &level.xst {
        if *unit <= 8 {
            template.push_str(&String::from_utf16_lossy(&literal));
            literal.clear();
            template.push('%');
            template.push(char::from(b'1' + *unit as u8));
        } else {
            literal.push(*unit);
        }
    }
    template.push_str(&String::from_utf16_lossy(&literal));
    let suffix = match level.ixch_follow {
        1 => "space",
        2 => "nothing",
        _ => "tab",
    };
    let mut xml = format!(
        r#"<w:lvl w:ilvl="{index}"><w:start w:val="{}"/><w:numFmt w:val="{format}"/><w:suff w:val="{suffix}"/><w:lvlText w:val="{}"/>"#,
        level.start.max(0),
        esc_attr(&template)
    );
    if level.legal {
        xml.push_str("<w:isLgl/>");
    }
    if let Some(limit) = level.restart_limit {
        xml.push_str(&format!(r#"<w:lvlRestart w:val="{limit}"/>"#));
    }
    if let Some(properties) =
        crate::papx::scan_paragraph_style_overrides(&level.paragraph_modifiers)
    {
        let body = crate::stsh::paragraph_xml(properties, None);
        if !body.is_empty() {
            xml.push_str(&format!("<w:pPr>{body}</w:pPr>"));
        }
    }
    let run = crate::stsh::character_xml(&level.character_modifiers, fonts);
    if !run.is_empty() {
        xml.push_str(&format!("<w:rPr>{run}</w:rPr>"));
    }
    xml.push_str("</w:lvl>");
    xml
}
