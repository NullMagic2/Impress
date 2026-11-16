//! Preserve native paragraph borders and style-assigned list references.
use crate::util::u16le;
use std::collections::BTreeMap;

const MODERN_BORDER_FIRST: u16 = 0xC64E;
const LEGACY_BORDER_FIRST: u16 = 0x6424;
const BORDER_SIDES: [&str; 6] = ["top", "left", "bottom", "right", "between", "bar"];
const MODERN_BORDER_BYTES: usize = 8;

pub(crate) fn supplementary_xml(bytes: &[u8], include_numbering: bool) -> String {
    let mut borders = BTreeMap::new();
    let mut list = None;
    let mut level = None;
    let mut offset = 0usize;
    while let Some(code) = u16le(bytes, offset) {
        let start = offset + 2;
        let length = match code >> 13 {
            0 | 1 => 1,
            2 | 4 | 5 => 2,
            3 => 4,
            7 => 3,
            6 => match bytes.get(start) {
                Some(value) => usize::from(*value) + 1,
                None => break,
            },
            _ => break,
        };
        let Some(data) = bytes.get(start..start.saturating_add(length)) else {
            break;
        };
        if (MODERN_BORDER_FIRST..MODERN_BORDER_FIRST + BORDER_SIDES.len() as u16).contains(&code) {
            if let Some(border) = data.get(1..).and_then(modern_border) {
                borders.insert(usize::from(code - MODERN_BORDER_FIRST), border);
            }
        } else if (LEGACY_BORDER_FIRST..LEGACY_BORDER_FIRST + BORDER_SIDES.len() as u16)
            .contains(&code)
        {
            if let Some(border) = legacy_border(data) {
                borders.insert(usize::from(code - LEGACY_BORDER_FIRST), border);
            }
        } else if code == 0x460B {
            list = u16le(data, 0);
        } else if code == 0x260A {
            level = data.first().copied();
        }
        offset = start + length;
    }
    let mut xml = String::new();
    if !borders.is_empty() {
        xml.push_str("<w:pBdr>");
        for (index, attributes) in borders {
            xml.push_str(&format!("<w:{}{attributes}/>", BORDER_SIDES[index]));
        }
        xml.push_str("</w:pBdr>");
    }
    if include_numbering {
        if let Some(id) = list {
            xml.push_str(&format!(
                r#"<w:numPr><w:ilvl w:val="{}"/><w:numId w:val="{id}"/></w:numPr>"#,
                level.unwrap_or(0).min(8)
            ));
        }
    }
    xml
}

fn border_attributes(kind: u8, size: u8, color: String, space: u8) -> String {
    let style = match kind {
        0 | 0xFF => "nil",
        3 => "double",
        6 => "dotted",
        7 => "dashed",
        _ => "single",
    };
    format!(
        r#" w:val="{style}" w:sz="{size}" w:color="{color}" w:space="{}""#,
        space & 31
    )
}

fn modern_border(bytes: &[u8]) -> Option<String> {
    let bytes = bytes.get(..MODERN_BORDER_BYTES)?;
    let color = if bytes[3] == 0xFF {
        "auto".into()
    } else {
        format!("{:02X}{:02X}{:02X}", bytes[0], bytes[1], bytes[2])
    };
    Some(border_attributes(bytes[5], bytes[4], color, bytes[6]))
}

fn legacy_border(bytes: &[u8]) -> Option<String> {
    let bytes = bytes.get(..4)?;
    let color = crate::chpx::ico_color(bytes[2])
        .map(|color| format!("{:02X}{:02X}{:02X}", color.r, color.g, color.b))
        .unwrap_or_else(|| "auto".into());
    Some(border_attributes(bytes[1], bytes[0], color, bytes[3]))
}
