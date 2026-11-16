//! Rotating a picture bakes the turn into a new bitmap resource, so layout,
//! rendering and every exporter show the rotated picture without knowing
//! about rotation. The picture's frame grows to the rotated bitmap's bounds.
//!
//! Each turn starts again from the bitmap the picture had before its first
//! turn in this session (`RotationOrigin`), so turning repeatedly neither
//! blurs the picture nor grows its transparent corners. A saved document
//! keeps only the rotated bitmap.
use super::*;
use model::Inline;

/// Rotated bitmaps larger than this on either side are refused, so a turn
/// of a huge picture cannot exhaust memory.
const MAX_ROTATED_SIDE_PX: u32 = 16_384;
/// Turns smaller than this (in degrees) leave the picture unchanged.
const NEGLIGIBLE_TURN_DEG: f32 = 0.05;
const ROTATED_MEDIA_TYPE: &str = "image/png";

/// Where a rotated bitmap came from: the unrotated resource, the total turn
/// already applied to it, and the unrotated frame size.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RotationOrigin { resource: ResourceId, degrees: f32, width_mm: f32, height_mm: f32 }

/// Size of a `width` × `height` box turned by `degrees`.
pub fn rotated_bounds(width: f32, height: f32, degrees: f32) -> (f32, f32) {
    let (sin, cos) = degrees.to_radians().sin_cos();
    (width * cos.abs() + height * sin.abs(), width * sin.abs() + height * cos.abs())
}

/// `degrees` folded into (-180, 180].
pub fn normalized_degrees(degrees: f32) -> f32 {
    let folded = degrees.rem_euclid(360.0);
    if folded > 180.0 { folded - 360.0 } else { folded }
}

/// `source` turned clockwise by `degrees` on a canvas that just holds it;
/// the uncovered corners are transparent. Bilinear sampling.
pub(crate) fn rotate_rgba(source: &image::RgbaImage, degrees: f32) -> Result<image::RgbaImage, String> {
    let (width, height) = (source.width() as f32, source.height() as f32);
    let (out_width, out_height) = rotated_bounds(width, height, degrees);
    let (out_width, out_height) = (out_width.round().max(1.0) as u32, out_height.round().max(1.0) as u32);
    if out_width > MAX_ROTATED_SIDE_PX || out_height > MAX_ROTATED_SIDE_PX { return Err("The picture is too large to rotate".into()); }
    let (sin, cos) = degrees.to_radians().sin_cos();
    let (source_center_x, source_center_y) = (width * 0.5, height * 0.5);
    let (out_center_x, out_center_y) = (out_width as f32 * 0.5, out_height as f32 * 0.5);
    Ok(image::RgbaImage::from_fn(out_width, out_height, |x, y| {
        // Pixel centre, turned back into the source (y grows downwards, so a
        // positive angle turns clockwise on screen).
        let dx = x as f32 + 0.5 - out_center_x;
        let dy = y as f32 + 0.5 - out_center_y;
        let sx = cos * dx + sin * dy + source_center_x - 0.5;
        let sy = -sin * dx + cos * dy + source_center_y - 0.5;
        image::Rgba(bilinear(source, sx, sy))
    }))
}

/// Premultiplied bilinear sample at a pixel-centre coordinate; outside the
/// bitmap is transparent, so edges fade instead of smearing.
fn bilinear(source: &image::RgbaImage, x: f32, y: f32) -> [u8; 4] {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let mut sum = [0.0f32; 4];
    for (ox, oy, weight) in [(0.0, 0.0, (1.0 - fx) * (1.0 - fy)), (1.0, 0.0, fx * (1.0 - fy)), (0.0, 1.0, (1.0 - fx) * fy), (1.0, 1.0, fx * fy)] {
        let (px, py) = (x0 + ox, y0 + oy);
        if px < 0.0 || py < 0.0 || px >= source.width() as f32 || py >= source.height() as f32 { continue; }
        let pixel = source.get_pixel(px as u32, py as u32).0;
        let alpha = pixel[3] as f32 / 255.0 * weight;
        for channel in 0..3 { sum[channel] += pixel[channel] as f32 * alpha; }
        sum[3] += alpha;
    }
    if sum[3] <= 0.0 { return [0, 0, 0, 0]; }
    let colour = |channel: usize| (sum[channel] / sum[3]).round().clamp(0.0, 255.0) as u8;
    [colour(0), colour(1), colour(2), (sum[3] * 255.0).round().clamp(0.0, 255.0) as u8]
}

/// The picture's bitmap resource and frame size, wherever it lives.
fn picture_frame(blocks: &[Block], id: &NodeId) -> Option<(ResourceId, f32, f32)> {
    fn inline(items: &[Inline], id: &NodeId) -> Option<(ResourceId, f32, f32)> {
        items.iter().find_map(|item| match item {
            Inline::Image(p) if &p.id == id => Some((p.resource.clone(), p.width_mm, p.height_mm)),
            Inline::Hyperlink(link) => inline(&link.content, id),
            _ => None,
        })
    }
    blocks.iter().find_map(|block| match block {
        Block::Image(p) if &p.id == id => Some((p.resource.clone(), p.width_mm, p.height_mm)),
        Block::Paragraph(p) => inline(&p.content, id),
        Block::Table(t) => t.rows.iter().flat_map(|r| &r.cells).find_map(|c| picture_frame(&c.blocks, id)),
        _ => None,
    })
}

fn set_inline_bitmap(items: &mut [Inline], id: &NodeId, resource: &ResourceId, width_mm: f32, height_mm: f32) -> bool {
    items.iter_mut().any(|item| match item {
        Inline::Image(p) if &p.id == id => { p.resource = resource.clone(); p.width_mm = width_mm; p.height_mm = height_mm; true }
        Inline::Hyperlink(link) => set_inline_bitmap(&mut link.content, id, resource, width_mm, height_mm),
        _ => false,
    })
}

impl EditorState {
    fn find_picture_frame(&self, id: &NodeId) -> Option<(ResourceId, f32, f32)> {
        self.document.sections.iter().find_map(|section| picture_frame(&section.blocks, id)
            .or_else(|| section.header_footers().find_map(|hf| picture_frame(&hf.stream.blocks, id))))
    }

    /// Turns the picture clockwise by `degrees` (negative: anticlockwise) in
    /// one undoable step. The picture keeps its scale; its frame becomes the
    /// rotated bounds.
    pub fn rotate_picture(&mut self, id: &NodeId, degrees: f32) -> Result<(), String> {
        if !degrees.is_finite() { return Err("Enter a finite rotation angle".into()); }
        if normalized_degrees(degrees).abs() < NEGLIGIBLE_TURN_DEG { return Ok(()); }
        let (resource, width_mm, height_mm) = self.find_picture_frame(id).ok_or("The selected picture is no longer in the document")?;
        let origin = self.rotation_origins.get(&resource).filter(|o| self.document.resources.contains_key(&o.resource)).cloned()
            .unwrap_or(RotationOrigin { resource: resource.clone(), degrees: 0.0, width_mm, height_mm });
        let total = normalized_degrees(origin.degrees + degrees);
        // Keep any resize made since the last turn.
        let scale = width_mm / rotated_bounds(origin.width_mm, origin.height_mm, origin.degrees).0.max(0.001);
        let (new_width, new_height) = rotated_bounds(origin.width_mm * scale, origin.height_mm * scale, total);
        if total.abs() < NEGLIGIBLE_TURN_DEG {
            // Back upright: the original bitmap again.
            return self.edit_picture_bitmap(id, None, &origin.resource, new_width, new_height);
        }
        let source = self.document.resources.get(&origin.resource).ok_or("The picture's image data is missing")?;
        let decoded = image::load_from_memory(source.data.as_slice()).map_err(|_| "This picture's format cannot be rotated")?;
        let rotated = rotate_rgba(&decoded.to_rgba8(), total)?;
        let mut png = Vec::new();
        rotated.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).map_err(|e| format!("Could not store the rotated picture: {e}"))?;
        let new_id = ResourceId::new();
        let new_resource = Resource { font_faces: Vec::new(),  id: new_id.clone(), media_type: ROTATED_MEDIA_TYPE.into(), original_name: source.original_name.clone(), data: png.into() };
        self.edit_picture_bitmap(id, Some(new_resource), &new_id, new_width, new_height)?;
        self.rotation_origins.insert(new_id, RotationOrigin { degrees: total, ..origin });
        Ok(())
    }

    /// Points the picture at `resource` with a new frame size, adding
    /// `inserted` first, as one undo step.
    fn edit_picture_bitmap(&mut self, id: &NodeId, inserted: Option<Resource>, resource: &ResourceId, width_mm: f32, height_mm: f32) -> Result<(), String> {
        let mut extra = Vec::new();
        let added = inserted.as_ref().map(|r| r.id.clone());
        if let Some(new_resource) = inserted {
            self.document.resources.insert(new_resource.id.clone(), new_resource.clone());
            extra.push(Mutation::ResourceInserted { id: new_resource.id.clone(), resource: new_resource });
        }
        let result = self.edit_object_with("Rotate picture", extra, |blocks| blocks.iter_mut().any(|block| match block {
            Block::Image(p) if &p.id == id => { p.resource = resource.clone(); p.width_mm = width_mm; p.height_mm = height_mm; true }
            Block::Paragraph(p) => set_inline_bitmap(&mut p.content, id, resource, width_mm, height_mm),
            _ => false,
        }));
        if let (Err(_), Some(added)) = (&result, added) {
            // Nothing was committed: drop the unused bitmap again.
            self.document.resources.remove(&added);
        }
        result
    }
}

