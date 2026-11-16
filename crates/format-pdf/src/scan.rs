//! Pages without a text layer whose pictures the text reader cannot decode
//! (fax, JBIG2 and JPEG 2000 scans) are rendered whole and kept as page
//! pictures, so no page goes missing; OCR may then read their text.
use crate::extract::{Area, PageText, Picture};
use crate::render::{each_page, png};
use std::sync::Arc;

/// Resolution of rendered page pictures.
const RENDER_DPI: f32 = 150.0;

pub(crate) fn render_blank_pages(bytes: &[u8], pages: &mut [PageText]) {
    let blank: Vec<usize> = pages.iter().enumerate().filter(|(_, p)| p.lines.is_empty() && p.pictures.is_empty()).map(|(i, _)| i).collect();
    for (index, data) in each_page(bytes, &blank, RENDER_DPI, |_, image| png(image)) {
        let Some(data) = data else { continue; };
        let page = &mut pages[index];
        page.pictures.push(Picture { area: Area { x: 0.0, y: 0.0, width: page.width, height: page.height }, media_type: "image/png", data: Arc::new(data) });
    }
}
