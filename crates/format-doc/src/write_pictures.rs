//! Native OfficeArt raster picture records stored in Word's Data stream.
use super::properties::{long, twips, word};
use format::{FormatError, FormatResult};
use model::{ImageEffects, Resource};
use std::io::Cursor;

const PICF_BYTES: usize = 68;
const SHAPE_PICTURE: u16 = 75;
const BLIP_PNG: u16 = 6;
const BLIP_JPEG: u16 = 5;

pub(super) fn append_inline(
    data: &mut Vec<u8>,
    resource: &Resource,
    width: f32,
    height: f32,
    effects: &ImageEffects,
    alt: Option<&str>,
) -> FormatResult<u32> {
    let (blip_type, bytes) = raster(resource)?;
    let uid = super::digest::md4(&bytes);
    let blip = blip(blip_type, &bytes, uid);
    let store = store_entry(blip_type, &blip, uid);
    let mut properties = vec![(0x4104, 1u32), (0x01BF, 0x00100010), (0x01FF, 0x00080000)];
    self::effects(&mut properties, effects);
    let shape = picture_shape(1025, &properties, alt);
    let mut picf = vec![0u8; PICF_BYTES];
    let length = PICF_BYTES + shape.len() + store.len();
    picf[..4].copy_from_slice(&(length as u32).to_le_bytes());
    picf[4..6].copy_from_slice(&(PICF_BYTES as u16).to_le_bytes());
    picf[6..8].copy_from_slice(&100u16.to_le_bytes());
    picf[28..30].copy_from_slice(&twips(width).to_le_bytes());
    picf[30..32].copy_from_slice(&twips(height).to_le_bytes());
    picf[32..34].copy_from_slice(&1000u16.to_le_bytes());
    picf[34..36].copy_from_slice(&1000u16.to_le_bytes());
    while data.len() % 4 != 0 {
        data.push(0);
    }
    let offset = data.len() as u32;
    data.extend(picf);
    data.extend(shape);
    data.extend(store);
    Ok(offset)
}
pub(super) fn record(flags: u16, kind: u16, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    word(&mut out, flags);
    word(&mut out, kind);
    long(&mut out, payload.len() as u32);
    out.extend_from_slice(payload);
    out
}
pub(super) fn picture_shape(id: u32, properties: &[(u16, u32)], alt: Option<&str>) -> Vec<u8> {
    let mut shape = Vec::new();
    let mut fsp = Vec::new();
    long(&mut fsp, id);
    long(&mut fsp, 0x00000A00);
    shape.extend(record((SHAPE_PICTURE << 4) | 2, 0xF00A, &fsp));
    let mut properties = properties.to_vec();
    let mut complex = Vec::new();
    if let Some(alt) = alt {
        for unit in alt.encode_utf16() {
            word(&mut complex, unit);
        }
        word(&mut complex, 0);
        properties.push((0x8381, complex.len() as u32));
    }
    properties.sort_by_key(|(id, _)| id & 0x3FFF);
    let mut fopt = Vec::new();
    for (code, value) in &properties {
        word(&mut fopt, *code);
        long(&mut fopt, *value);
    }
    fopt.extend(complex);
    shape.extend(record(((properties.len() as u16) << 4) | 3, 0xF00B, &fopt));
    shape.extend(record(0, 0xF010, &0x80000000u32.to_le_bytes()));
    record(15, 0xF004, &shape)
}
pub(super) fn raster(resource: &Resource) -> FormatResult<(u16, Vec<u8>)> {
    if resource.data.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Ok((BLIP_PNG, resource.data.as_ref().clone()));
    }
    if resource.data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Ok((BLIP_JPEG, resource.data.as_ref().clone()));
    }
    let image = image::load_from_memory(&resource.data).map_err(|error| FormatError::Invalid {
        format: "DOC",
        message: format!("Could not decode raster picture: {error}"),
    })?;
    let mut output = Cursor::new(Vec::new());
    image
        .write_to(&mut output, image::ImageFormat::Png)
        .map_err(|error| FormatError::Invalid {
            format: "DOC",
            message: format!("Could not encode raster picture: {error}"),
        })?;
    Ok((BLIP_PNG, output.into_inner()))
}
pub(super) fn blip(kind: u16, bytes: &[u8], uid: [u8; 16]) -> Vec<u8> {
    let mut payload = uid.to_vec();
    payload.push(0xFF);
    payload.extend_from_slice(bytes);
    record(
        if kind == BLIP_PNG { 0x6E00 } else { 0x46A0 },
        if kind == BLIP_PNG { 0xF01E } else { 0xF01D },
        &payload,
    )
}
pub(super) fn store_entry(kind: u16, blip: &[u8], uid: [u8; 16]) -> Vec<u8> {
    let mut payload = vec![0u8; 36];
    payload[0] = kind as u8;
    payload[1] = kind as u8;
    payload[2..18].copy_from_slice(&uid);
    payload[18..20].copy_from_slice(&0x00FFu16.to_le_bytes());
    payload[20..24].copy_from_slice(&(blip.len() as u32).to_le_bytes());
    payload[24..28].copy_from_slice(&1u32.to_le_bytes());
    payload.extend_from_slice(blip);
    record((kind << 4) | 2, 0xF007, &payload)
}

pub(super) fn effects(properties: &mut Vec<(u16, u32)>, effects: &ImageEffects) {
    if let Some(crop) = &effects.crop {
        for (code, value) in [
            (0x0100, crop.top),
            (0x0101, crop.bottom),
            (0x0102, crop.left),
            (0x0103, crop.right),
        ] {
            properties.push((code, (value * 65536.0).round() as u32));
        }
    }
}
