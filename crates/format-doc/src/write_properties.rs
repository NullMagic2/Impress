//! Encode the supported Word97 character, paragraph and section property subset.
use model::*;

const TWIPS_PER_MM: f32 = 1440.0 / 25.4;
/// BRC nil explicitly suppresses a border inherited from a row or table.
const NO_BORDER: [u8; 8] = [0xFF; 8];
pub(super) fn word(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}
pub(super) fn long(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}
pub(super) fn byte_property(out: &mut Vec<u8>, code: u16, value: u8) {
    word(out, code);
    out.push(value);
}
pub(super) fn word_property(out: &mut Vec<u8>, code: u16, value: u16) {
    word(out, code);
    word(out, value);
}
pub(super) fn twips(value: f32) -> u16 {
    if value.is_finite() {
        (value * TWIPS_PER_MM)
            .round()
            .clamp(i16::MIN as f32, i16::MAX as f32) as i16 as u16
    } else {
        0
    }
}
pub(super) fn rgb(value: &str) -> Option<[u8; 3]> {
    let value = value.strip_prefix('#').unwrap_or(value);
    if value.len() != 6 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    Some([
        u8::from_str_radix(&value[0..2], 16).ok()?,
        u8::from_str_radix(&value[2..4], 16).ok()?,
        u8::from_str_radix(&value[4..6], 16).ok()?,
    ])
}
pub(super) fn character(properties: &CharacterProperties, font: u16, special: bool) -> Vec<u8> {
    let mut out = Vec::new();
    for (code, value) in [
        (0x0835, properties.bold.unwrap_or(false)),
        (0x0836, properties.italic.unwrap_or(false)),
        (0x085A, properties.right_to_left.unwrap_or(false)),
        (0x0855, special),
    ] {
        byte_property(&mut out, code, u8::from(value));
    }
    byte_property(
        &mut out,
        0x2A3E,
        u8::from(properties.underline.unwrap_or(false)),
    );
    word_property(
        &mut out,
        0x4A43,
        (properties.font_size_pt.unwrap_or(11.0) * 2.0)
            .round()
            .clamp(2.0, 3276.0) as u16,
    );
    for code in [0x4A4F, 0x4A50, 0x4A51] {
        word_property(&mut out, code, font);
    }
    if let Some(color) = properties.color.as_deref().and_then(rgb) {
        word(&mut out, 0x6870);
        out.extend_from_slice(&[color[0], color[1], color[2], 0]);
    }
    if let Some(color) = properties.highlight.as_deref().and_then(rgb) {
        shading(&mut out, 0xCA71, color);
    }
    let language = match properties
        .language
        .as_deref()
        .unwrap_or("en-US")
        .split('-')
        .next()
        .unwrap_or("en")
    {
        "el" => 0x0408,
        "es" => 0x0C0A,
        "pt" if properties.language.as_deref() == Some("pt-BR") => 0x0416,
        "pt" => 0x0816,
        "fr" => 0x040C,
        "de" => 0x0407,
        "it" => 0x0410,
        "ru" => 0x0419,
        "ar" => 0x0401,
        "he" => 0x040D,
        _ => 0x0409,
    };
    word_property(&mut out, 0x486D, language);
    out
}
pub(super) fn paragraph(properties: &ParagraphProperties) -> Vec<u8> {
    let mut out = Vec::new();
    let alignment = match properties.alignment.unwrap_or(ParagraphAlignment::Left) {
        ParagraphAlignment::Left => 0,
        ParagraphAlignment::Center => 1,
        ParagraphAlignment::Right => 2,
        ParagraphAlignment::Justify => 3,
    };
    byte_property(&mut out, 0x2403, alignment);
    for (code, value) in [
        (0x2405, properties.keep_lines_together),
        (0x2406, properties.keep_with_next),
        (0x2407, properties.page_break_before),
        (0x2431, properties.widow_control),
    ] {
        if let Some(value) = value {
            byte_property(&mut out, code, u8::from(value));
        }
    }
    if let Some(direction) = properties.direction {
        byte_property(
            &mut out,
            0x2441,
            u8::from(direction == ParagraphDirection::RightToLeft),
        );
    }
    for (code, value) in [
        (0x845E, properties.left_indent_mm),
        (0x845D, properties.right_indent_mm),
        (0x8460, properties.first_line_indent_mm),
    ] {
        if let Some(value) = value {
            word_property(&mut out, code, twips(value));
        }
    }
    for (code, value) in [
        (0xA413, properties.space_before_pt),
        (0xA414, properties.space_after_pt),
    ] {
        if let Some(value) = value {
            word_property(
                &mut out,
                code,
                (value * 20.0).round().clamp(0.0, 65535.0) as u16,
            );
        }
    }
    if let Some(value) = properties.line_spacing {
        word(&mut out, 0x6412);
        word(&mut out, (value * 240.0).round().clamp(1.0, 32767.0) as u16);
        word(&mut out, 1);
    }
    if let Some(color) = properties.shading.as_deref().and_then(rgb) {
        shading(&mut out, 0xC64D, color);
    }
    if let Some(borders) = &properties.borders {
        for (code, border) in [
            (0xC64E, &borders.top),
            (0xC64F, &borders.left),
            (0xC650, &borders.bottom),
            (0xC651, &borders.right),
        ] {
            if let Some(border) = border {
                word(&mut out, code);
                out.push(8);
                out.extend_from_slice(&border_bytes(border));
            }
        }
    }
    if let Some(tabs) = &properties.tabs {
        let mut operand = vec![0, tabs.len().min(64) as u8];
        for tab in tabs.iter().take(64) {
            word(&mut operand, twips(tab.position_mm));
        }
        for tab in tabs.iter().take(64) {
            let alignment = match tab.alignment {
                TabAlignment::Left => 0,
                TabAlignment::Center => 1,
                TabAlignment::Right => 2,
                TabAlignment::Decimal => 3,
                TabAlignment::Bar => 4,
            };
            let leader = match tab.leader {
                TabLeader::None => 0,
                TabLeader::Dots => 1,
                TabLeader::Dashes => 2,
                TabLeader::Line => 3,
                TabLeader::Heavy => 4,
                TabLeader::MiddleDots => 5,
            };
            operand.push(alignment | (leader << 3));
        }
        word(&mut out, 0xC60D);
        out.push(operand.len() as u8);
        out.extend(operand);
    }
    out
}
fn shading(out: &mut Vec<u8>, code: u16, color: [u8; 3]) {
    word(out, code);
    out.push(10);
    out.extend_from_slice(&[0, 0, 0, 0xFF, color[0], color[1], color[2], 0, 0, 0]);
}
pub(super) fn border_bytes(border: &Border) -> [u8; 8] {
    if border.style == BorderStyle::None {
        return NO_BORDER;
    }
    let color = rgb(&border.color).unwrap_or([0; 3]);
    [
        color[0],
        color[1],
        color[2],
        0,
        (border.width_pt * 8.0).round().clamp(0.0, 255.0) as u8,
        match border.style {
            BorderStyle::None => 0,
            BorderStyle::Single => 1,
            BorderStyle::Double => 3,
            BorderStyle::Dotted => 6,
            BorderStyle::Dashed => 7,
        },
        0,
        0,
    ]
}
pub(super) fn section(properties: &SectionProperties) -> Vec<u8> {
    let mut out = Vec::new();
    for (code, value) in [
        (0xB01F, properties.page_width_mm),
        (0xB020, properties.page_height_mm),
        (0xB021, properties.margin_left_mm),
        (0xB022, properties.margin_right_mm),
        (0x9023, properties.margin_top_mm),
        (0x9024, properties.margin_bottom_mm),
        (0xB017, properties.header_distance_mm),
        (0xB018, properties.footer_distance_mm),
        (0x900C, properties.column_gap_mm),
    ] {
        word_property(&mut out, code, twips(value));
    }
    byte_property(
        &mut out,
        0x3009,
        match properties.start_on {
            SectionStart::Continuous => 0,
            SectionStart::NewColumn => 1,
            SectionStart::NewPage => 2,
            SectionStart::EvenPage => 3,
            SectionStart::OddPage => 4,
        },
    );
    byte_property(&mut out, 0x300A, u8::from(properties.different_first_page));
    word_property(&mut out, 0x500B, properties.column_count.saturating_sub(1));
    byte_property(&mut out, 0x3005, u8::from(properties.columns.is_empty()));
    if !properties.columns.is_empty() {
        word_property(
            &mut out,
            0x500B,
            properties.columns.len().saturating_sub(1) as u16,
        );
        for (index, column) in properties.columns.iter().enumerate() {
            word(&mut out, 0xF203);
            out.push(index as u8);
            word(&mut out, twips(column.width_mm));
            if index + 1 < properties.columns.len() {
                word(&mut out, 0xF204);
                out.push(index as u8);
                word(&mut out, twips(column.spacing_mm));
            }
        }
    }
    byte_property(&mut out, 0x3019, u8::from(properties.column_separator));
    if let Some(start) = properties.page_number_start {
        byte_property(&mut out, 0x3011, 1);
        word_property(&mut out, 0x501C, start.min(u16::MAX as u32) as u16);
    }
    out
}
