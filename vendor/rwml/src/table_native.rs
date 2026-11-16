//! Retain native per-cell shading/borders, exact grids and floating placement.
use super::*;
use crate::util::u16le;

const CELL_SHADING: u16 = 0xD62D;
const CELL_BORDER: u16 = 0xD62F;
const SHADING_BYTES: usize = 10;

impl TableDef {
    pub(crate) fn apply_native_property(&mut self, code: u16, operand: &[u8]) {
        match code {
            0x5624 => {
                if let (Some(&start), Some(&end)) = (operand.first(), operand.get(1)) {
                    let start = usize::from(start).min(self.tcgrf.len());
                    let end = usize::from(end).min(self.tcgrf.len());
                    for index in start..end {
                        self.tcgrf[index] =
                            (self.tcgrf[index] & !3) | if index == start { 1 } else { 2 };
                    }
                }
            }
            0xD62B => {
                if let (Some(&column), Some(&flags)) = (operand.get(1), operand.get(2)) {
                    if let Some(value) = self.tcgrf.get_mut(usize::from(column)) {
                        *value = (*value & !0x60)
                            | if flags & 2 != 0 {
                                0x60
                            } else if flags & 1 != 0 {
                                0x20
                            } else {
                                0
                            };
                    }
                }
            }
            0xD612 | 0xD616 | 0xD60C => {
                let base = match code {
                    0xD612 => 0,
                    0xD616 => 22,
                    _ => 44,
                };
                for (index, bytes) in operand
                    .get(1..)
                    .unwrap_or_default()
                    .chunks_exact(SHADING_BYTES)
                    .enumerate()
                {
                    if let Some(target) = self.cell_shading.get_mut(base + index) {
                        *target = if bytes[7] == 0
                            && u16le(bytes, 8).is_some_and(|pattern| pattern <= 1)
                        {
                            Some(Color::rgb(bytes[4], bytes[5], bytes[6]))
                        } else {
                            None
                        };
                    }
                }
            }
            CELL_SHADING | CELL_BORDER => {
                let Some(body) = operand.get(1..) else { return };
                let (Some(&first), Some(&limit)) = (body.first(), body.get(1)) else {
                    return;
                };
                let first = usize::from(first).min(self.tcgrf.len());
                let limit = usize::from(limit).min(self.tcgrf.len());
                if first >= limit {
                    return;
                }
                if code == CELL_SHADING {
                    let Some(bytes) = body.get(2..2 + SHADING_BYTES) else {
                        return;
                    };
                    let pattern = u16le(bytes, 8).unwrap_or(u16::MAX);
                    let color = if pattern <= 1 && bytes[7] == 0 {
                        Some(Color::rgb(bytes[4], bytes[5], bytes[6]))
                    } else {
                        None
                    };
                    for index in first..limit {
                        self.cell_shading[index] = color;
                    }
                } else {
                    let Some(&mask) = body.get(2) else { return };
                    let Some(border) = body.get(3..3 + BRC_LEN).and_then(parse_brc) else {
                        return;
                    };
                    for cell in &mut self.cell_borders[first..limit] {
                        if mask & 1 != 0 {
                            cell.top = border;
                        }
                        if mask & 2 != 0 {
                            cell.left = border;
                        }
                        if mask & 4 != 0 {
                            cell.bottom = border;
                        }
                        if mask & 8 != 0 {
                            cell.right = border;
                        }
                    }
                }
            }
            0xD633 => {
                if let Some(value) = operand.get(1..).and_then(|body| u16le(body, 4)) {
                    self.native_position.insert(code, u32::from(value));
                }
            }
            0xF614 => {
                if operand.first() == Some(&3) {
                    if let Some(value) = u16le(operand, 1) {
                        self.native_position.insert(code, u32::from(value));
                    }
                }
            }
            0x360D | 0x3465 => {
                if let Some(&value) = operand.first() {
                    self.native_position.insert(code, u32::from(value));
                }
            }
            0x5400 | 0x9407 | 0x940E | 0x940F | 0x9410 | 0x9411 | 0x941E | 0x941F | 0x3615 => {
                if let Some(value) = u16le(operand, 0) {
                    self.native_position.insert(code, u32::from(value));
                }
            }
            _ => {}
        }
    }

    pub(super) fn cell_properties_xml(&self, index: usize) -> String {
        let Some(cell) = self.cell_borders.get(index) else {
            return String::new();
        };
        let mut xml = String::new();
        for (name, border) in [
            ("top", cell.top),
            ("left", cell.left),
            ("bottom", cell.bottom),
            ("right", cell.right),
        ] {
            match border {
                LegacyBorderState::Nil => xml.push_str(&format!(r#"<w:{name} w:val="nil"/>"#)),
                LegacyBorderState::Positive(border) => {
                    let color = match border.color {
                        LegacyBorderColor::Auto => "auto".into(),
                        LegacyBorderColor::Explicit(color) => {
                            format!("{:02X}{:02X}{:02X}", color.r, color.g, color.b)
                        }
                    };
                    let style = match border.style {
                        TableBorderStyle::Single => "single",
                        TableBorderStyle::Double => "double",
                        TableBorderStyle::Dotted => "dotted",
                        TableBorderStyle::Dashed => "dashed",
                    };
                    xml.push_str(&format!(
                        r#"<w:{name} w:val="{style}" w:sz="{}" w:color="{color}"/>"#,
                        border.size_eighths
                    ));
                }
                _ => {}
            }
        }
        if xml.is_empty() {
            xml
        } else {
            format!("<w:tcBorders>{xml}</w:tcBorders>")
        }
    }

    pub(super) fn row_properties_xml(&self) -> String {
        self.native_position
            .get(&0x9407)
            .map(|value| {
                let height = *value as u16 as i16;
                format!(
                    r#"<w:trHeight w:val="{}" w:hRule="{}"/>"#,
                    i32::from(height).abs(),
                    if height < 0 { "exact" } else { "atLeast" }
                )
            })
            .unwrap_or_default()
    }

    pub(super) fn table_properties_xml(&self) -> String {
        let mut xml = String::new();
        if let (Some(first), Some(last)) = (self.rgdxa.first(), self.rgdxa.last()) {
            let width = self
                .native_position
                .get(&0xF614)
                .copied()
                .map(|width| width as i32)
                .unwrap_or_else(|| i32::from(*last) - i32::from(*first));
            if width > 0 {
                xml.push_str(&format!(r#"<w:tblW w:w="{width}" w:type="dxa"/>"#));
            }
            if *first != 0 {
                xml.push_str(&format!(r#"<w:tblInd w:w="{first}" w:type="dxa"/>"#));
            }
        }
        if let Some(value) = self.native_position.get(&0xD633) {
            xml.push_str(&format!(
                r#"<w:tblCellSpacing w:w="{value}" w:type="dxa"/>"#
            ));
        }
        if let Some(value) = self.native_position.get(&0x5400) {
            let alignment = match value {
                1 => "center",
                2 => "right",
                _ => "left",
            };
            xml.push_str(&format!(r#"<w:jc w:val="{alignment}"/>"#));
        }
        let Some(&position) = self.native_position.get(&0x360D) else {
            return xml;
        };
        let horizontal = match (position >> 6) & 3 {
            1 => "margin",
            2 => "page",
            _ => "text",
        };
        let vertical = match (position >> 4) & 3 {
            1 => "page",
            2 => "text",
            _ => "margin",
        };
        let mut attributes = format!(r#" w:horzAnchor="{horizontal}" w:vertAnchor="{vertical}""#);
        for (code, offset_name, alignment_name) in [
            (0x940E, "tblpX", "tblpXSpec"),
            (0x940F, "tblpY", "tblpYSpec"),
        ] {
            if let Some(value) = self.native_position.get(&code) {
                let value = *value as u16 as i16;
                let alignment = if code == 0x940E {
                    match value {
                        -4 => Some("center"),
                        -8 => Some("right"),
                        -12 => Some("inside"),
                        -16 => Some("outside"),
                        _ => None,
                    }
                } else {
                    match value {
                        -4 => Some("top"),
                        -8 => Some("center"),
                        -12 => Some("bottom"),
                        _ => None,
                    }
                };
                if let Some(alignment) = alignment {
                    attributes.push_str(&format!(r#" w:{alignment_name}="{alignment}""#));
                } else {
                    attributes.push_str(&format!(r#" w:{offset_name}="{value}""#));
                }
            }
        }
        for (code, name) in [
            (0x9410, "leftFromText"),
            (0x941E, "rightFromText"),
            (0x9411, "topFromText"),
            (0x941F, "bottomFromText"),
        ] {
            if let Some(value) = self.native_position.get(&code) {
                attributes.push_str(&format!(r#" w:{name}="{value}""#));
            }
        }
        xml.push_str(&format!("<w:tblpPr{attributes}/>"));
        if self.native_position.get(&0x3465) == Some(&1) {
            xml.push_str(r#"<w:tblOverlap w:val="never"/>"#);
        }
        xml
    }
}
