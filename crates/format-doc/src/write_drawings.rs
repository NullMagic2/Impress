//! Word drawing groups and FSPA anchors for embedded floating raster pictures.
use super::pictures::{blip, picture_shape, raster, record, store_entry};
use super::properties::{long, word};
use format::{FormatError, FormatResult};
use model::*;

const FIRST_SHAPE_ID: u32 = 1025;
const TWIPS_PER_MM: f32 = 1440.0 / 25.4;
const EMUS_PER_MM: f32 = 36_000.0;
#[derive(Default)]
pub(super) struct Drawings {
    entries: Vec<Entry>,
}
struct Entry {
    cp: u32,
    store: Vec<u8>,
    shape: Vec<u8>,
    bounds: [i32; 4],
    flags: u16,
}
fn twips32(value: f32) -> i32 {
    (value * TWIPS_PER_MM).round() as i32
}
impl Drawings {
    pub(super) fn add(
        &mut self,
        cp: u32,
        image: &BlockImage,
        document: &Document,
    ) -> FormatResult<()> {
        let resource =
            document
                .resources
                .get(&image.resource)
                .ok_or_else(|| FormatError::Invalid {
                    format: "DOC",
                    message: "A floating picture resource is missing".into(),
                })?;
        let (kind, bytes) = raster(resource)?;
        let uid = super::digest::md4(&bytes);
        let store = store_entry(kind, &blip(kind, &bytes, uid), uid);
        let id = FIRST_SHAPE_ID + self.entries.len() as u32;
        let mut properties = vec![
            (0x4104, self.entries.len() as u32 + 1),
            (0x01BF, 0x00100010),
            (0x01FF, 0x00080000),
        ];
        super::pictures::effects(&mut properties, &image.effects);
        let (left, top, page, locked, gap) = image
            .anchor
            .as_ref()
            .map(|a| {
                (
                    a.x_mm,
                    a.y_mm,
                    a.reference == AnchorReference::Page,
                    a.locked,
                    a.gap_mm,
                )
            })
            .unwrap_or((0.0, 0.0, false, false, 3.2));
        for code in [0x0384, 0x0385, 0x0386, 0x0387] {
            properties.push((code, (gap * EMUS_PER_MM).round() as u32));
        }
        properties.push((
            0x03BF,
            if image.wrap == ImageWrap::BehindText {
                0x00200020
            } else {
                0x00200000
            },
        ));
        let shape = picture_shape(id, &properties, image.alt_text.as_deref());
        let wrap = match image.wrap {
            ImageWrap::TopAndBottom => 1,
            ImageWrap::Square => 2,
            ImageWrap::Tight => 4,
            ImageWrap::BehindText | ImageWrap::InFrontOfText => 3,
            ImageWrap::Inline => 2,
        };
        let flags = (if page {
            1 << 1 | 1 << 3
        } else {
            2 << 1 | 2 << 3
        }) | (wrap << 5)
            | if image.wrap == ImageWrap::BehindText {
                0x4000
            } else {
                0
            }
            | if locked { 0x8000 } else { 0 };
        let bounds = [
            twips32(left),
            twips32(top),
            twips32(left + image.width_mm),
            twips32(top + image.height_mm),
        ];
        self.entries.push(Entry {
            cp,
            store,
            shape,
            bounds,
            flags,
        });
        Ok(())
    }
    pub(super) fn tables(
        &self,
        body_end: u32,
        header_start: u32,
        header_end: u32,
    ) -> FormatResult<Vec<(usize, Vec<u8>)>> {
        if self.entries.is_empty() {
            return Ok(Vec::new());
        }
        if self
            .entries
            .iter()
            .any(|e| e.cp >= body_end && !(e.cp >= header_start && e.cp < header_end))
        {
            return Err(FormatError::Unsupported {format:"DOC",feature:"Floating pictures in note or comment stories cannot be anchored by the DOC format".into()});
        }
        let body_count = self
            .entries
            .iter()
            .filter(|entry| entry.cp < body_end)
            .count();
        let header_count = self.entries.len() - body_count;
        if body_count > 1023 || header_count > 1023 {
            return Err(FormatError::Unsupported {
                format: "DOC",
                feature: "A native DOC drawing story exceeds 1023 floating pictures".into(),
            });
        }
        let drawing_count = u32::from(body_count > 0) + u32::from(header_count > 0);
        let maximum = if header_count > 0 {
            2049 + header_count as u32
        } else {
            1025 + body_count as u32
        };
        let mut dgg = Vec::new();
        let mut info = Vec::new();
        for value in [
            maximum,
            drawing_count + 1,
            self.entries.len() as u32 + drawing_count,
            drawing_count,
        ] {
            long(&mut info, value);
        }
        for (id, count) in [(1, body_count), (2, header_count)] {
            if count > 0 {
                long(&mut info, id);
                long(&mut info, count as u32 + 1);
            }
        }
        dgg.extend(record(0, 0xF006, &info));
        let stores = self
            .entries
            .iter()
            .flat_map(|e| e.store.iter().copied())
            .collect::<Vec<_>>();
        dgg.extend(record(
            ((self.entries.len() as u16) << 4) | 15,
            0xF001,
            &stores,
        ));
        let mut content = record(15, 0xF000, &dgg);
        let mut parts = Vec::new();
        for (label, index, start, end) in [
            (0u8, 40usize, 0, body_end),
            (1, 41, header_start, header_end),
        ] {
            let mut entries = self
                .entries
                .iter()
                .filter(|e| e.cp >= start && e.cp < end)
                .collect::<Vec<_>>();
            if entries.is_empty() {
                continue;
            }
            entries.sort_by_key(|e| e.cp);
            let mut plc = Vec::new();
            for entry in &entries {
                long(&mut plc, entry.cp - start);
            }
            long(&mut plc, end - start);
            for (offset, entry) in entries.iter().enumerate() {
                long(&mut plc, (u32::from(label) + 1) * 1024 + offset as u32 + 1);
                for value in entry.bounds {
                    plc.extend_from_slice(&value.to_le_bytes());
                }
                word(&mut plc, entry.flags);
                long(&mut plc, 0);
            }
            parts.push((index, plc));
            let mut drawing = Vec::new();
            let mut fdg = Vec::new();
            long(&mut fdg, entries.len() as u32 + 1);
            long(
                &mut fdg,
                (u32::from(label) + 1) * 1024 + entries.len() as u32,
            );
            drawing.extend(record((label as u16 + 1) << 4, 0xF008, &fdg));
            let mut root = record(1, 0xF009, &[0u8; 16]);
            let mut fsp = Vec::new();
            long(&mut fsp, 1024 + u32::from(label) * 1024);
            long(&mut fsp, 5);
            root.extend(record(2, 0xF00A, &fsp));
            let mut group = record(15, 0xF004, &root);
            for (offset, entry) in entries.into_iter().enumerate() {
                let mut shape = entry.shape.clone();
                shape[16..20].copy_from_slice(
                    &((u32::from(label) + 1) * 1024 + offset as u32 + 1).to_le_bytes(),
                );
                group.extend(shape);
            }
            drawing.extend(record(15, 0xF003, &group));
            content.push(label);
            content.extend(record(15, 0xF002, &drawing));
        }
        parts.push((50, content));
        Ok(parts)
    }
}

pub(super) fn collect(document: &Document) -> Vec<&BlockImage> {
    fn visit<'a>(blocks: &'a [Block], out: &mut Vec<&'a BlockImage>) {
        for block in blocks {
            match block {
                Block::Image(image) if image.wrap != ImageWrap::Inline => out.push(image),
                Block::Table(table) => {
                    for row in &table.rows {
                        for cell in &row.cells {
                            visit(&cell.blocks, out);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    for section in &document.sections {
        visit(&section.blocks, &mut out);
        for stream in section.header_footers() {
            visit(&stream.stream.blocks, &mut out);
        }
    }
    for note in document.review.footnotes.values() {
        visit(&note.blocks, &mut out);
    }
    out
}
