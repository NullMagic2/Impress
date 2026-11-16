//! Read bounded native OfficeArt picture stores and FSPA floating anchors.
use crate::model::{Block, Image};
use crate::util::{u16le, u32le};
use crate::{DocState, FloatingShape, ShapeDistance, ShapeExtent, ShapePosition, ShapeWrapping};
use std::collections::{BTreeMap, BTreeSet};

const RECORD_HEADER: usize = 8;
const MAX_DRAWING_DEPTH: usize = 64;
const MAX_RECORDS: usize = 65536;
const MAX_IMAGE_BYTES: usize = 512 << 20;
const FSPA_BYTES: usize = 26;
const EMU_PER_TWIP: i64 = 635;
const FIXED_ONE: i64 = 65536;

#[derive(Default)]
struct Shape {
    properties: BTreeMap<u16, u32>,
    text: BTreeMap<u16, String>,
}
#[derive(Default)]
struct DrawingData<'a> {
    stores: Vec<&'a [u8]>,
    shapes: BTreeMap<u32, Shape>,
    records: usize,
}

impl<'a> DrawingData<'a> {
    fn read(&mut self, bytes: &'a [u8], depth: usize) {
        if depth > MAX_DRAWING_DEPTH {
            return;
        }
        let mut offset = 0usize;
        while offset + RECORD_HEADER <= bytes.len() && self.records < MAX_RECORDS {
            let Some(flags) = u16le(bytes, offset) else {
                break;
            };
            let Some(kind) = u16le(bytes, offset + 2) else {
                break;
            };
            if !(0xF000..=0xF122).contains(&kind) {
                offset += 1;
                continue;
            }
            let Some(length) = u32le(bytes, offset + 4).map(|value| value as usize) else {
                break;
            };
            let Some(end) = offset
                .checked_add(RECORD_HEADER)
                .and_then(|start| start.checked_add(length))
            else {
                break;
            };
            let Some(payload) = bytes.get(offset + RECORD_HEADER..end) else {
                break;
            };
            self.records += 1;
            if kind == 0xF007 {
                self.stores.push(payload);
            }
            if kind == 0xF004 {
                if let Some((id, shape)) = read_shape(payload) {
                    self.shapes.insert(id, shape);
                }
            }
            if flags & 15 == 15 {
                self.read(payload, depth + 1);
            }
            offset = end;
        }
    }
}

fn read_shape(bytes: &[u8]) -> Option<(u32, Shape)> {
    let mut offset = 0usize;
    let mut id = None;
    let mut shape = Shape::default();
    while offset + RECORD_HEADER <= bytes.len() {
        let flags = u16le(bytes, offset)?;
        let kind = u16le(bytes, offset + 2)?;
        let end = offset
            .checked_add(RECORD_HEADER)?
            .checked_add(u32le(bytes, offset + 4)? as usize)?;
        let payload = bytes.get(offset + RECORD_HEADER..end)?;
        match kind {
            0xF00A => id = u32le(payload, 0),
            0xF00B | 0xF121 | 0xF122 => {
                let count = usize::from(flags >> 4);
                let mut complex = count.checked_mul(6)?;
                if complex > payload.len() {
                    return None;
                }
                for index in 0..count {
                    let code = u16le(payload, index * 6)?;
                    let value = u32le(payload, index * 6 + 2)?;
                    if code & 0x8000 != 0 {
                        let end = complex.checked_add(value as usize)?;
                        let data = payload.get(complex..end)?;
                        complex = end;
                        if matches!(code & 0x3FFF, 0x0380 | 0x0381) {
                            let units = data
                                .chunks_exact(2)
                                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                                .take_while(|unit| *unit != 0)
                                .collect::<Vec<_>>();
                            if let Ok(text) = String::from_utf16(&units) {
                                shape.text.insert(code & 0x3FFF, text);
                            }
                        }
                    } else {
                        shape.properties.insert(code & 0x3FFF, value);
                    }
                }
            }
            _ => {}
        }
        offset = end;
    }
    Some((id?, shape))
}

fn effects(shape: &Shape, image: &mut Image) {
    if let Some(text) = shape.text.get(&0x0381) {
        image.alt = Some(text.clone());
    }
    if let Some(value) = shape.properties.get(&4) {
        image.rotation_degrees = Some((*value as i32 as i64 / FIXED_ONE) as i32);
    }
    let mut crop = [0i32; 4];
    let mut has_crop = false;
    for (index, code) in [(0, 0x0102), (1, 0x0100), (2, 0x0103), (3, 0x0101)] {
        if let Some(value) = shape.properties.get(&code) {
            crop[index] = ((*value as i32 as i64) * 100000 / FIXED_ONE) as i32;
            has_crop = true;
        }
    }
    if has_crop {
        image.crop = Some(crop);
    }
}

pub(crate) fn inline_metadata(bytes: &[u8], image: &mut Image) {
    let mut drawing = DrawingData::default();
    drawing.read(bytes, 0);
    if let Some(shape) = drawing.shapes.values().next() {
        effects(shape, image);
    }
}

fn store_image(payload: &[u8], data: &[u8]) -> Option<Image> {
    let name = usize::from(*payload.get(33)?);
    let embedded = payload.get(36usize.checked_add(name)?..)?;
    if let Some(image) = crate::image::extract_art(embedded) {
        return Some(image);
    }
    let offset = u32le(payload, 28)? as usize;
    let length = u32le(data, offset.checked_add(4)?)? as usize;
    let end = offset.checked_add(RECORD_HEADER)?.checked_add(length)?;
    crate::image::extract_art(data.get(offset..end)?)
}

/// Attach raster shapes and return the number of unprojected source shapes.
pub(crate) fn attach(state: &DocState, model: &mut crate::model::DocModel) -> usize {
    let Some(bytes) = crate::legacy_conversion::pair(&state.word, &state.table, 50) else {
        return 0;
    };
    let mut drawing = DrawingData::default();
    drawing.read(bytes, 0);
    let mut images = BTreeMap::<usize, Vec<Image>>::new();
    let mut budget = MAX_IMAGE_BYTES;
    let mut source_shapes = 0usize;
    for (index, base) in [
        (40, 0usize),
        (41, (state.fib.ccp_text + state.fib.ccp_ftn) as usize),
    ] {
        let Some(plc) = crate::legacy_conversion::pair(&state.word, &state.table, index) else {
            continue;
        };
        if plc.len() < 4 || (plc.len() - 4) % (4 + FSPA_BYTES) != 0 {
            continue;
        }
        let count = (plc.len() - 4) / (4 + FSPA_BYTES);
        let records = (count + 1) * 4;
        source_shapes += count;
        for index in 0..count {
            let Some(cp) = u32le(plc, index * 4).map(|cp| cp as usize + base) else {
                continue;
            };
            let Some(record) =
                plc.get(records + index * FSPA_BYTES..records + (index + 1) * FSPA_BYTES)
            else {
                continue;
            };
            let Some(shape) = u32le(record, 0).and_then(|id| drawing.shapes.get(&id)) else {
                continue;
            };
            let Some(store) = shape
                .properties
                .get(&0x0104)
                .and_then(|index| index.checked_sub(1))
                .and_then(|index| drawing.stores.get(index as usize))
            else {
                continue;
            };
            let Some(mut image) = store_image(store, &state.data) else {
                continue;
            };
            let size = image.bytes.as_ref().map_or(0, Vec::len);
            if size > budget {
                continue;
            }
            budget -= size;
            let signed = |offset| u32le(record, offset).map(|value| i64::from(value as i32));
            let (Some(left), Some(top), Some(right), Some(bottom), Some(flags)) = (
                signed(4),
                signed(8),
                signed(12),
                signed(16),
                u16le(record, 20),
            ) else {
                continue;
            };
            if right <= left || bottom <= top {
                continue;
            }
            let horizontal = match (flags >> 1) & 3 {
                1 => "page",
                2 => "column",
                _ => "margin",
            };
            let vertical = match (flags >> 3) & 3 {
                1 => "page",
                2 => "paragraph",
                _ => "margin",
            };
            let wrapping = match (flags >> 5) & 15 {
                1 => "topAndBottom",
                2 => "square",
                4 => "tight",
                5 => "through",
                _ => "none",
            };
            let distance = ShapeDistance {
                left_emu: shape.properties.get(&0x0384).map(|value| i64::from(*value)),
                top_emu: shape.properties.get(&0x0385).map(|value| i64::from(*value)),
                right_emu: shape.properties.get(&0x0386).map(|value| i64::from(*value)),
                bottom_emu: shape.properties.get(&0x0387).map(|value| i64::from(*value)),
            };
            let metadata = FloatingShape {
                id: u32le(record, 0).unwrap_or(0).to_string(),
                name: shape.text.get(&0x0380).cloned(),
                description: shape.text.get(&0x0381).cloned(),
                horizontal_position: Some(ShapePosition {
                    relative_from: Some(horizontal.into()),
                    offset_emu: Some(left * EMU_PER_TWIP),
                    align: None,
                }),
                vertical_position: Some(ShapePosition {
                    relative_from: Some(vertical.into()),
                    offset_emu: Some(top * EMU_PER_TWIP),
                    align: None,
                }),
                extent: Some(ShapeExtent {
                    cx_emu: (right - left) * EMU_PER_TWIP,
                    cy_emu: (bottom - top) * EMU_PER_TWIP,
                }),
                behind_doc: Some(flags & 0x4000 != 0),
                locked: Some(flags & 0x8000 != 0),
                layout_in_cell: Some(true),
                allow_overlap: Some(true),
                wrapping: Some(ShapeWrapping {
                    kind: wrapping.into(),
                    text: Some("bothSides".into()),
                    distance,
                    polygon: Vec::new(),
                }),
                distance,
                ..FloatingShape::default()
            };
            effects(shape, &mut image);
            image.display_size_emu =
                Some(((right - left) * EMU_PER_TWIP, (bottom - top) * EMU_PER_TWIP));
            image.floating_offset_emu = Some((left * EMU_PER_TWIP, top * EMU_PER_TWIP));
            image.floating_metadata = Some(Box::new(metadata));
            images.entry(cp).or_default().push(image);
        }
    }
    fn visit(
        blocks: &mut [Block],
        images: &BTreeMap<usize, Vec<Image>>,
        budget: &mut usize,
        matched: &mut BTreeSet<usize>,
    ) -> usize {
        let mut attached = 0;
        for block in blocks {
            match block {
                Block::Paragraph(paragraph) => {
                    for run in &mut paragraph.runs {
                        let cp = run
                            .bookmark
                            .as_deref()
                            .and_then(|name| name.strip_prefix("ImpressCp"))
                            .and_then(|cp| cp.parse::<usize>().ok());
                        if let Some(image) = cp
                            .and_then(|cp| images.get(&cp))
                            .and_then(|images| images.first())
                        {
                            let size = image.bytes.as_ref().map_or(0, Vec::len);
                            if size <= *budget {
                                *budget -= size;
                                run.image = Some(image.clone());
                                if let Some(cp) = cp {
                                    matched.insert(cp);
                                }
                                attached += 1;
                            }
                        }
                    }
                }
                Block::Table(table) => {
                    for row in &mut table.rows {
                        for cell in &mut row.cells {
                            attached += visit(&mut cell.blocks, images, budget, matched);
                        }
                    }
                }
                Block::SectionBreak(section) => {
                    for blocks in [
                        &mut section.header,
                        &mut section.footer,
                        &mut section.first_header,
                        &mut section.first_footer,
                        &mut section.even_header,
                        &mut section.even_footer,
                    ] {
                        attached += visit(blocks, images, budget, matched);
                    }
                }
                _ => {}
            }
        }
        attached
    }
    let mut matched = BTreeSet::new();
    visit(&mut model.blocks, &images, &mut budget, &mut matched);
    for blocks in [
        &mut model.setup.header,
        &mut model.setup.footer,
        &mut model.setup.first_header,
        &mut model.setup.first_footer,
        &mut model.setup.even_header,
        &mut model.setup.even_footer,
    ] {
        visit(blocks, &images, &mut budget, &mut matched);
    }
    source_shapes.saturating_sub(matched.len())
}
