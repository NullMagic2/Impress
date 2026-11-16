//! Native Word97 list definitions and format-override tables.
use super::properties::{long, word, word_property};
use model::*;
use std::collections::BTreeMap;

pub(super) struct Lists {
    ids: BTreeMap<NodeId, u16>,
}
impl Lists {
    pub(super) fn new(document: &Document) -> Self {
        Self {
            ids: document
                .lists
                .keys()
                .enumerate()
                .map(|(index, id)| (id.clone(), index as u16 + 1))
                .collect(),
        }
    }
    pub(super) fn paragraph(&self, properties: &ParagraphProperties, out: &mut Vec<u8>) {
        let Some(index) = properties.list_id.as_ref().and_then(|id| self.ids.get(id)) else {
            return;
        };
        super::properties::byte_property(out, 0x260A, properties.list_level.unwrap_or(0).min(8));
        word_property(out, 0x460B, *index);
    }
    pub(super) fn tables(&self, document: &Document) -> (Vec<u8>, usize, Vec<u8>) {
        let mut definitions = Vec::new();
        word(&mut definitions, self.ids.len() as u16);
        for index in self.ids.values() {
            long(&mut definitions, *index as u32);
            long(&mut definitions, *index as u32);
            for _ in 0..9 {
                word(&mut definitions, 0x0FFF);
            }
            definitions.extend_from_slice(&[0, 0]);
        }
        let fixed_len = definitions.len();
        for id in self.ids.keys() {
            let list = &document.lists[id];
            for index in 0..9 {
                let level = list
                    .levels
                    .iter()
                    .find(|level| level.level == index)
                    .or_else(|| list.levels.first());
                let fallback = ListLevel {
                    level: index,
                    format: ListFormat::Decimal,
                    text: format!("%{}.", index + 1),
                    start_at: 1,
                    left_indent_mm: 12.7,
                    hanging_indent_mm: 6.35,
                };
                append_level(&mut definitions, level.unwrap_or(&fallback));
            }
        }
        let mut overrides = Vec::new();
        long(&mut overrides, self.ids.len() as u32);
        for index in self.ids.values() {
            long(&mut overrides, *index as u32);
            overrides.extend_from_slice(&[0; 12]);
        }
        for _ in self.ids.values() {
            long(&mut overrides, u32::MAX);
        }
        (definitions, fixed_len, overrides)
    }
}
fn append_level(out: &mut Vec<u8>, level: &ListLevel) {
    let mut template = Vec::new();
    let mut offsets = [0u8; 9];
    let mut chars = level.text.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '%' && chars.peek().is_some_and(|next| ('1'..='9').contains(next)) {
            let index = chars.next().unwrap_or('1') as usize - '1' as usize;
            offsets[index] = (template.len() + 1) as u8;
            template.push(index as u16);
        } else {
            let mut encoded = [0; 2];
            template.extend_from_slice(character.encode_utf16(&mut encoded));
        }
    }
    let mut paragraph = Vec::new();
    word_property(
        &mut paragraph,
        0x845E,
        super::properties::twips(level.left_indent_mm),
    );
    word_property(
        &mut paragraph,
        0x8460,
        super::properties::twips(-level.hanging_indent_mm),
    );
    let mut header = vec![0u8; 28];
    header[..4].copy_from_slice(&level.start_at.to_le_bytes());
    header[4] = match level.format {
        ListFormat::Decimal => 0,
        ListFormat::UpperRoman => 1,
        ListFormat::LowerRoman => 2,
        ListFormat::UpperLetter => 3,
        ListFormat::LowerLetter => 4,
        ListFormat::Bullet => 23,
    };
    header[6..15].copy_from_slice(&offsets);
    header[15] = 0;
    header[25] = paragraph.len() as u8;
    out.extend(header);
    out.extend(paragraph);
    word(out, template.len() as u16);
    for character in template {
        word(out, character);
    }
}
