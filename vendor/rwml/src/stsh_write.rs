//! Preserve native style slot identity, inheritance and local formatting in OOXML.
use super::{style_id, StyleSheet};
use crate::papx::{ParagraphJustification, ParagraphLineSpacing};
use crate::util::u16le;
use crate::write::esc_attr;

const WORD_NAMESPACE: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const MAX_STYLE_CHAIN: usize = 64;

impl StyleSheet {
    pub(crate) fn source_xml(&self, fonts: &[String]) -> String {
        let mut xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><w:styles xmlns:w="{WORD_NAMESPACE}">"#
        );
        for (index, source) in self.source.iter().enumerate() {
            let Some(source) = source else { continue };
            let kind = match source.description.sgc {
                1 => "paragraph",
                2 => "character",
                _ => continue,
            };
            let default = if index == 0 { r#" w:default="1""# } else { "" };
            xml.push_str(&format!(
                r#"<w:style w:type="{kind}" w:styleId="{}"{default}><w:name w:val="{}"/>"#,
                style_id(index as u16),
                esc_attr(&source.description.name)
            ));
            if self.valid_base(index) {
                xml.push_str(&format!(
                    r#"<w:basedOn w:val="{}"/>"#,
                    style_id(source.description.istd_base)
                ));
            }
            if source.description.sgc == 1
                && self
                    .source
                    .get(source.next as usize)
                    .is_some_and(|next| next.as_ref().is_some_and(|next| next.description.sgc == 1))
            {
                xml.push_str(&format!(r#"<w:next w:val="{}"/>"#, style_id(source.next)));
            }
            if let Some(paragraph) = &source.paragraph {
                let mut body =
                    paragraph_xml(paragraph.properties, self.heading_level(index as u16));
                if let Some(tabs) = &paragraph.tab_stop_changes {
                    body.push_str(&tabs.source_xml());
                }
                body.push_str(&paragraph.native_extra);
                if !body.is_empty() {
                    xml.push_str(&format!("<w:pPr>{body}</w:pPr>"));
                }
            }
            let run = self.style_character_xml(index, fonts);
            if !run.is_empty() {
                xml.push_str(&format!("<w:rPr>{run}</w:rPr>"));
            }
            xml.push_str("</w:style>");
        }
        if self.source.first().is_none_or(Option::is_none) {
            xml.push_str(r#"<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style>"#);
        }
        xml.push_str("</w:styles>");
        xml
    }

    fn valid_base(&self, index: usize) -> bool {
        let Some(Some(style)) = self.source.get(index) else {
            return false;
        };
        let mut current = style.description.istd_base as usize;
        if current == 0x0FFF {
            return false;
        }
        for _ in 0..MAX_STYLE_CHAIN {
            if current == index {
                return false;
            }
            let Some(Some(parent)) = self.source.get(current) else {
                return false;
            };
            if parent.description.sgc != style.description.sgc {
                return false;
            }
            current = parent.description.istd_base as usize;
            if current == 0x0FFF {
                return true;
            }
        }
        false
    }
}

pub(crate) fn paragraph_xml(
    properties: crate::papx::ParagraphStyleOverrides,
    heading: Option<u8>,
) -> String {
    let mut body = String::new();
    if let Some(bidi) = properties.layout.bidi {
        boolean(&mut body, "bidi", bidi);
    }
    if let Some(alignment) = properties.layout.justification {
        let value = match alignment {
            ParagraphJustification::PhysicalLeft => "left",
            ParagraphJustification::Center => "center",
            ParagraphJustification::PhysicalRight => "right",
            ParagraphJustification::Justify => "both",
            ParagraphJustification::LogicalEnd => "end",
            ParagraphJustification::LogicalStart | ParagraphJustification::UnsupportedIndented => {
                "start"
            }
        };
        value_element(&mut body, "jc", value);
    }
    let indent = properties.indent;
    let mut attributes = String::new();
    if let Some(value) = indent.logical_left_twips {
        attribute(&mut attributes, "start", value);
    }
    if let Some(value) = indent.logical_right_twips {
        attribute(&mut attributes, "end", value);
    }
    if let Some(value) = indent.first_line_twips {
        attribute(
            &mut attributes,
            if value < 0 { "hanging" } else { "firstLine" },
            i32::from(value).abs(),
        );
    }
    if !attributes.is_empty() {
        body.push_str(&format!("<w:ind{attributes}/>"));
    }
    attributes.clear();
    let spacing = properties.spacing;
    if let Some(value) = spacing.before_twips {
        attribute(&mut attributes, "before", value);
    }
    if let Some(value) = spacing.after_twips {
        attribute(&mut attributes, "after", value);
    }
    if let Some(line) = spacing.line {
        let pair = match line {
            ParagraphLineSpacing::ProportionalTwips(value) => Some((value, "auto")),
            ParagraphLineSpacing::ExactTwips(value) => Some((value, "exact")),
            ParagraphLineSpacing::AtLeastTwips(value) => Some((value, "atLeast")),
            ParagraphLineSpacing::Unrepresentable => None,
        };
        if let Some((value, rule)) = pair {
            attribute(&mut attributes, "line", value);
            attribute(&mut attributes, "lineRule", rule);
        }
    }
    if !attributes.is_empty() {
        body.push_str(&format!("<w:spacing{attributes}/>"));
    }
    let pagination = properties.pagination;
    for (name, value) in [
        ("keepNext", pagination.keep_next),
        ("keepLines", pagination.keep_lines),
        ("pageBreakBefore", pagination.page_break_before),
        ("widowControl", pagination.widow_control),
    ] {
        if let Some(value) = value {
            boolean(&mut body, name, value);
        }
    }
    if properties.shading == Some(crate::papx::ParagraphShading::Unrepresentable) {
        body.push_str(r#"<w:shd w:val="nil" w:fill="auto"/>"#);
    }
    if let Some(color) = properties.shading.and_then(|value| value.flat_color()) {
        body.push_str(&format!(
            r#"<w:shd w:val="clear" w:fill="{:02X}{:02X}{:02X}"/>"#,
            color.r, color.g, color.b
        ));
    }
    if let Some(level) = heading {
        value_element(&mut body, "outlineLvl", level - 1);
    }
    body
}

fn attribute(xml: &mut String, name: &str, value: impl std::fmt::Display) {
    xml.push_str(&format!(r#" w:{name}="{}""#, esc_attr(&value.to_string())));
}

fn value_element(xml: &mut String, name: &str, value: impl std::fmt::Display) {
    xml.push_str(&format!(
        r#"<w:{name} w:val="{}"/>"#,
        esc_attr(&value.to_string())
    ));
}

fn boolean(xml: &mut String, name: &str, value: bool) {
    value_element(xml, name, u8::from(value));
}

pub(crate) fn character_xml(bytes: &[u8], fonts: &[String]) -> String {
    let mut xml = String::new();
    let mut offset = 0;
    while let Some(opcode) = u16le(bytes, offset) {
        let operand = offset + 2;
        let length = match opcode >> 13 {
            0 | 1 => 1,
            2 | 4 | 5 => 2,
            3 => 4,
            7 => 3,
            6 => match bytes.get(operand) {
                Some(length) => usize::from(*length) + 1,
                None => break,
            },
            _ => break,
        };
        let Some(data) = bytes.get(operand..operand.saturating_add(length)) else {
            break;
        };
        let toggle = match opcode {
            0x0835 => Some("b"),
            0x0836 => Some("i"),
            0x0837 => Some("strike"),
            0x083A => Some("smallCaps"),
            0x083B => Some("caps"),
            0x083C => Some("vanish"),
            0x085A => Some("rtl"),
            _ => None,
        };
        if let Some(name) = toggle {
            if matches!(data[0], 0 | 1) {
                boolean(&mut xml, name, data[0] != 0);
            }
        } else {
            match opcode {
                0x2A0C => {
                    if let Some(name) = crate::chpx::highlight_name(data[0]) {
                        value_element(&mut xml, "highlight", name);
                    }
                }
                0xCA71 => {
                    if let Some(fill) = data.get(5..8) {
                        xml.push_str(&format!(
                            r#"<w:shd w:val="clear" w:fill="{:02X}{:02X}{:02X}"/>"#,
                            fill[0], fill[1], fill[2]
                        ));
                    }
                }
                0x4A30 => {
                    if let Some(style) = u16le(data, 0) {
                        value_element(&mut xml, "rStyle", super::style_id(style));
                    }
                }
                0x4A43 => {
                    if let Some(size) = u16le(data, 0) {
                        value_element(&mut xml, "sz", size);
                    }
                }
                0x4A4F => {
                    if let Some(font) = u16le(data, 0).and_then(|index| fonts.get(index as usize)) {
                        let font = esc_attr(font);
                        xml.push_str(&format!(r#"<w:rFonts w:ascii="{font}" w:hAnsi="{font}" w:eastAsia="{font}" w:cs="{font}"/>"#));
                    }
                }
                0x6870 => value_element(
                    &mut xml,
                    "color",
                    format!("{:02X}{:02X}{:02X}", data[0], data[1], data[2]),
                ),
                0x2A42 => {
                    if let Some(color) = crate::chpx::ico_color(data[0]) {
                        value_element(
                            &mut xml,
                            "color",
                            format!("{:02X}{:02X}{:02X}", color.r, color.g, color.b),
                        );
                    }
                }
                0x2A3E => {
                    value_element(&mut xml, "u", if data[0] == 0 { "none" } else { "single" })
                }
                0x2A48 => value_element(
                    &mut xml,
                    "vertAlign",
                    match data[0] {
                        1 => "superscript",
                        2 => "subscript",
                        _ => "baseline",
                    },
                ),
                _ => {}
            }
        }
        offset = operand + length;
    }
    xml
}
