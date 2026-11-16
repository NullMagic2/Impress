//! Bounded raster values for picture controls bound to scalar XML base64 data.
use crate::*;
use base64::Engine;
use std::{collections::BTreeSet, io::Cursor};
pub const MAX_CONTENT_CONTROL_IMAGE_BYTES: usize = 16 * 1024 * 1024;
const MAX_IMAGE_DIMENSION: u32 = 20_000;
const MAX_IMAGE_ALLOC_BYTES: u64 = 64 * 1024 * 1024;
/// Decode and validate a complete image before it can replace a document resource.
pub fn decode_content_picture(value: &str) -> Result<Resource, String> {
    let value = value
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>();
    if value.len() > MAX_CONTENT_CONTROL_IMAGE_BYTES * 4 / 3 + 4 {
        return Err("The bound picture exceeds its size limit".into());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(value)
        .map_err(|_| "The bound picture is not valid base64 image data")?;
    let mut reader = image::ImageReader::new(Cursor::new(&bytes))
        .with_guessed_format()
        .map_err(|_| "The bound picture format cannot be read")?;
    let media_type = match reader.format() {
        Some(image::ImageFormat::Png) => "image/png",
        Some(image::ImageFormat::Jpeg) => "image/jpeg",
        Some(image::ImageFormat::Gif) => "image/gif",
        Some(image::ImageFormat::Bmp) => "image/bmp",
        _ => return Err("The bound picture must be PNG, JPEG, GIF or BMP".into()),
    };
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_DIMENSION);
    limits.max_image_height = Some(MAX_IMAGE_DIMENSION);
    limits.max_alloc = Some(MAX_IMAGE_ALLOC_BYTES);
    reader.limits(limits);
    reader
        .decode()
        .map_err(|_| "The bound picture is damaged or exceeds its decoded size limit")?;
    Ok(Resource {
        id: ResourceId::new(),
        media_type: media_type.into(),
        original_name: None,
        font_faces: Vec::new(),
        data: bytes.into(),
    })
}
pub fn encode_content_picture(resource: &Resource) -> String {
    base64::engine::general_purpose::STANDARD.encode(resource.data.as_slice())
}
fn picture_objects(document: &Document, control: &ContentControl) -> Vec<(NodeId, ResourceId)> {
    let paragraphs = document_paragraphs(document);
    let first = paragraphs
        .iter()
        .position(|p| p.id == control.range.start.paragraph);
    let last = paragraphs
        .iter()
        .position(|p| p.id == control.range.end.paragraph);
    let (Some(first), Some(last)) = (first, last) else {
        return Vec::new();
    };
    if first > last {
        return Vec::new();
    }
    let ids = paragraphs[first..=last]
        .iter()
        .map(|p| p.id.clone())
        .collect::<BTreeSet<_>>();
    let mut objects = Vec::new();
    fn inlines(
        items: &[Inline],
        offset: &mut usize,
        start: usize,
        end: usize,
        out: &mut Vec<(NodeId, ResourceId)>,
    ) {
        for item in items {
            let length = inline_text(std::slice::from_ref(item)).len();
            match item {
                Inline::Image(image) if *offset >= start && *offset + length <= end => {
                    out.push((image.id.clone(), image.resource.clone()))
                }
                Inline::Hyperlink(link) => {
                    inlines(&link.content, offset, start, end, out);
                    continue;
                }
                _ => {}
            }
            *offset += length;
        }
    }
    for (index, p) in paragraphs[first..=last].iter().enumerate() {
        let start = if index == 0 {
            control.range.start.offset
        } else {
            0
        };
        let end = if first + index == last {
            control.range.end.offset
        } else {
            inline_text(&p.content).len()
        };
        inlines(&p.content, &mut 0, start, end, &mut objects);
    }
    fn blocks(
        items: &[Block],
        paragraphs: &BTreeSet<NodeId>,
        members: &[NodeId],
        out: &mut Vec<(NodeId, ResourceId)>,
    ) {
        for item in items {
            match item {
                Block::Image(image)
                    if members.contains(&image.id)
                        || members.is_empty()
                            && image
                                .anchor
                                .as_ref()
                                .is_some_and(|anchor| paragraphs.contains(&anchor.paragraph)) =>
                {
                    out.push((image.id.clone(), image.resource.clone()))
                }
                Block::Table(table) => {
                    for row in &table.rows {
                        for cell in &row.cells {
                            blocks(&cell.blocks, paragraphs, members, out);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    for section in &document.sections {
        blocks(&section.blocks, &ids, &control.members, &mut objects);
        for story in section.header_footers() {
            blocks(&story.stream.blocks, &ids, &control.members, &mut objects);
        }
    }
    for note in document.review.footnotes.values() {
        blocks(&note.blocks, &ids, &control.members, &mut objects);
    }
    objects
}
impl Document {
    pub fn content_control_picture(&self, control: &ContentControl) -> Result<&Resource, String> {
        let objects = picture_objects(self, control);
        if objects.len() != 1 {
            return Err("A picture content control must contain exactly one picture".into());
        }
        self.resources
            .get(&objects[0].1)
            .ok_or_else(|| "The content control's picture resource is missing".into())
    }
    /// Replace only the controlled object, preserving its size, anchor and effects.
    pub fn replace_content_control_picture(
        &mut self,
        control: &ContentControl,
        resource: &ResourceId,
    ) -> Result<(), String> {
        let objects = picture_objects(self, control);
        if objects.len() != 1 {
            return Err("A picture content control must contain exactly one picture".into());
        }
        let object = &objects[0].0;
        fn inline(items: &mut [Inline], object: &NodeId, resource: &ResourceId) {
            for item in items {
                match item {
                    Inline::Image(image) if &image.id == object => {
                        image.resource = resource.clone()
                    }
                    Inline::Hyperlink(link) => inline(&mut link.content, object, resource),
                    Inline::TextBox(text) => blocks(&mut text.blocks, object, resource),
                    _ => {}
                }
            }
        }
        fn blocks(items: &mut [Block], object: &NodeId, resource: &ResourceId) {
            for item in items {
                match item {
                    Block::Image(image) if &image.id == object => image.resource = resource.clone(),
                    Block::Paragraph(paragraph) => inline(&mut paragraph.content, object, resource),
                    Block::Table(table) => {
                        for row in &mut table.rows {
                            for cell in &mut row.cells {
                                blocks(&mut cell.blocks, object, resource);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        for section in &mut self.sections {
            blocks(&mut section.blocks, object, resource);
            for story in section.header_footers_mut() {
                blocks(&mut story.blocks, object, resource);
            }
        }
        for note in self.review.footnotes.values_mut() {
            blocks(&mut note.blocks, object, resource);
        }
        Ok(())
    }
}
