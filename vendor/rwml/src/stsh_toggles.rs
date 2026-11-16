//! Resolve Word's relative character toggles against their actual style base.
use super::StyleSheet;
use crate::util::u16le;
use std::collections::BTreeMap;

const MAX_BASE_STYLES: usize = 64;
const CHARACTER_STYLE: u16 = 0x4A30;
const TOGGLE_PROPERTIES: [u16; 7] = [0x0835, 0x0836, 0x0837, 0x083A, 0x083B, 0x083C, 0x085A];
type ToggleValues = BTreeMap<u16, bool>;

impl StyleSheet {
    fn apply_style_toggles(&self, index: u16, values: &mut ToggleValues) {
        let mut chain = Vec::new();
        let mut current = index;
        for _ in 0..MAX_BASE_STYLES {
            if chain.contains(&current) {
                break;
            }
            let Some(Some(style)) = self.source.get(usize::from(current)) else {
                break;
            };
            chain.push(current);
            current = style.description.istd_base;
        }
        for index in chain.into_iter().rev() {
            if let Some(Some(style)) = self.source.get(usize::from(index)) {
                visit(&style.character, |code, data, _| {
                    if TOGGLE_PROPERTIES.contains(&code) {
                        match data.first() {
                            Some(0) => {
                                values.insert(code, false);
                            }
                            Some(1) => {
                                values.insert(code, true);
                            }
                            Some(0x81) => {
                                values.insert(code, !values.get(&code).copied().unwrap_or(false));
                            }
                            _ => {}
                        }
                    }
                });
            }
        }
    }

    pub(super) fn style_character_xml(&self, index: usize, fonts: &[String]) -> String {
        let Some(Some(style)) = self.source.get(index) else {
            return String::new();
        };
        let mut base = ToggleValues::new();
        if style.description.istd_base != index as u16 {
            self.apply_style_toggles(style.description.istd_base, &mut base);
        }
        super::character_xml(&normalize(&style.character, &base), fonts)
    }

    pub(crate) fn run_character_xml(
        &self,
        bytes: &[u8],
        fonts: &[String],
        paragraph_style: u16,
    ) -> String {
        let mut base = ToggleValues::new();
        self.apply_style_toggles(paragraph_style, &mut base);
        let mut character_style = None;
        visit(bytes, |code, data, _| {
            if code == CHARACTER_STYLE {
                character_style = u16le(data, 0);
            }
        });
        if let Some(index) = character_style {
            self.apply_style_toggles(index, &mut base);
        }
        super::character_xml(&normalize(bytes, &base), fonts)
    }
}

fn normalize(bytes: &[u8], base: &ToggleValues) -> Vec<u8> {
    let mut normalized = bytes.to_vec();
    visit(bytes, |code, data, offset| {
        if TOGGLE_PROPERTIES.contains(&code) && data.first() == Some(&0x81) {
            normalized[offset] = u8::from(!base.get(&code).copied().unwrap_or(false));
        }
    });
    normalized
}

fn visit(bytes: &[u8], mut visit: impl FnMut(u16, &[u8], usize)) {
    let mut offset = 0;
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
        visit(code, data, start);
        offset = start + length;
    }
}
