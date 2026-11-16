//! Write native paragraph/character style identities and sparse inheritance graphs.
use super::properties::{character, paragraph, word};
use format::{FormatError, FormatResult};
use model::{CharacterProperties, Document, StyleId, StyleKind, STYLE_NORMAL};
use std::collections::{BTreeMap, BTreeSet};

const NO_BASE_STYLE: u16 = 0x0FFF;
const USER_STYLE_ID: u16 = 0x0FFE;
const MAX_STYLE_COUNT: usize = 0x0FFE;
const MAX_STYLE_NAME_UNITS: usize = 255;
const STSH_HEADER_BYTES: usize = 20;
const STD_BASE_BYTES: usize = 10;

#[derive(Clone)]
pub(super) struct StyleIds {
    ordered: Vec<StyleId>,
    indexes: BTreeMap<StyleId, u16>,
}

impl StyleIds {
    pub(super) fn new(document: &Document) -> FormatResult<Self> {
        let normal = StyleId::from(STYLE_NORMAL);
        let mut ordered = vec![normal.clone()];
        ordered.extend(
            document
                .styles
                .styles
                .keys()
                .filter(|id| **id != normal)
                .cloned(),
        );
        if ordered.len() > MAX_STYLE_COUNT {
            return Err(unsupported("Too many styles for native DOC export"));
        }
        let indexes = ordered
            .iter()
            .enumerate()
            .map(|(index, id)| (id.clone(), index as u16))
            .collect();
        Ok(Self { ordered, indexes })
    }

    pub(super) fn index(&self, id: Option<&StyleId>) -> u16 {
        id.and_then(|id| self.indexes.get(id).copied()).unwrap_or(0)
    }

    pub(super) fn encode(
        &self,
        document: &Document,
        mut font: impl FnMut(&CharacterProperties) -> u16,
    ) -> FormatResult<Vec<u8>> {
        let mut output = vec![0; STSH_HEADER_BYTES];
        set_word(&mut output, 0, (STSH_HEADER_BYTES - 2) as u16);
        set_word(&mut output, 2, self.ordered.len() as u16);
        set_word(&mut output, 4, STD_BASE_BYTES as u16);
        set_word(&mut output, 6, 1);
        set_word(&mut output, 8, USER_STYLE_ID);
        for (index, id) in self.ordered.iter().enumerate() {
            let style = document.styles.styles.get(id);
            let name = style.map(|style| style.name.as_str()).unwrap_or("Normal");
            let name_units = name.encode_utf16().collect::<Vec<_>>();
            if name_units.len() > MAX_STYLE_NAME_UNITS {
                return Err(unsupported(
                    "A style name is too long for native DOC export",
                ));
            }
            let kind = style
                .map(|style| style.kind)
                .unwrap_or(StyleKind::Paragraph);
            let base = self.base(document, id)?;
            let next = style
                .and_then(|style| style.next_style.as_ref())
                .filter(|next| {
                    document
                        .styles
                        .styles
                        .get(*next)
                        .is_some_and(|style| style.kind == StyleKind::Paragraph)
                })
                .map(|next| self.index(Some(next)))
                .unwrap_or(index as u16);
            let mut data = vec![0; STD_BASE_BYTES];
            set_word(&mut data, 0, if index == 0 { 0 } else { USER_STYLE_ID });
            set_word(
                &mut data,
                2,
                (base << 4) | if kind == StyleKind::Paragraph { 1 } else { 2 },
            );
            set_word(
                &mut data,
                4,
                (next << 4) | if kind == StyleKind::Paragraph { 2 } else { 1 },
            );
            word(&mut data, name_units.len() as u16);
            for unit in name_units {
                word(&mut data, unit);
            }
            word(&mut data, 0);
            if kind == StyleKind::Paragraph {
                let mut properties = Vec::new();
                word(&mut properties, index as u16);
                if let Some(style) = style {
                    properties.extend(sparse_paragraph(&style.paragraph));
                }
                upx(&mut data, &properties)?;
            }
            let character = style
                .map(|style| &style.character)
                .cloned()
                .unwrap_or_default();
            let font_index = if character.font_family.is_some() {
                font(&character)
            } else {
                0
            };
            upx(&mut data, &sparse_character(&character, font_index))?;
            if data.len() > u16::MAX as usize {
                return Err(unsupported("A style is too large for native DOC export"));
            }
            let length = data.len() as u16;
            set_word(&mut data, 6, length);
            word(&mut output, length);
            output.extend(data);
        }
        Ok(output)
    }

    fn base(&self, document: &Document, id: &StyleId) -> FormatResult<u16> {
        let Some(style) = document.styles.styles.get(id) else {
            return Ok(NO_BASE_STYLE);
        };
        let Some(base) = &style.based_on else {
            return Ok(NO_BASE_STYLE);
        };
        let Some(parent) = document.styles.styles.get(base) else {
            return Err(unsupported("A style refers to a missing base style"));
        };
        if parent.kind != style.kind {
            return Err(unsupported("A style has a base of a different type"));
        }
        let mut seen = BTreeSet::new();
        seen.insert(id.clone());
        let mut current = Some(base);
        while let Some(id) = current {
            if !seen.insert(id.clone()) {
                return Err(unsupported("The style inheritance graph contains a cycle"));
            }
            current = document
                .styles
                .styles
                .get(id)
                .and_then(|style| style.based_on.as_ref());
        }
        Ok(self.index(Some(base)))
    }
}

fn upx(output: &mut Vec<u8>, bytes: &[u8]) -> FormatResult<()> {
    if bytes.len() > u16::MAX as usize {
        return Err(unsupported("Style formatting exceeds the native DOC limit"));
    }
    word(output, bytes.len() as u16);
    output.extend_from_slice(bytes);
    if bytes.len() % 2 != 0 {
        output.push(0);
    }
    Ok(())
}

pub(super) fn sparse_paragraph(properties: &model::ParagraphProperties) -> Vec<u8> {
    filter_properties(&paragraph(properties), |code| {
        code != 0x2403 || properties.alignment.is_some()
    })
}

pub(super) fn sparse_character(properties: &CharacterProperties, font: u16) -> Vec<u8> {
    filter_properties(&character(properties, font, false), |code| match code {
        0x0835 => properties.bold.is_some(),
        0x0836 => properties.italic.is_some(),
        0x085A => properties.right_to_left.is_some(),
        0x0855 => false,
        0x2A3E => properties.underline.is_some(),
        0x4A43 => properties.font_size_pt.is_some(),
        0x4A4F | 0x4A50 | 0x4A51 => properties.font_family.is_some(),
        0x486D => properties.language.is_some(),
        _ => true,
    })
}

fn filter_properties(bytes: &[u8], keep: impl Fn(u16) -> bool) -> Vec<u8> {
    let mut output = Vec::new();
    let mut offset = 0;
    while let Some(header) = bytes.get(offset..offset + 2) {
        let code = u16::from_le_bytes([header[0], header[1]]);
        let operand = offset + 2;
        let length = match code >> 13 {
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
        let end = operand + length;
        let Some(property) = bytes.get(offset..end) else {
            break;
        };
        if keep(code) {
            output.extend_from_slice(property);
        }
        offset = end;
    }
    output
}

fn set_word(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}
fn unsupported(message: &str) -> FormatError {
    FormatError::Unsupported {
        format: "DOC",
        feature: message.into(),
    }
}
