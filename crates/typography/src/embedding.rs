//! Document-local font payloads. Full faces preserve all glyphs for later editing.
use crate::FontCatalog;
use model::{
    CharacterProperties, Document, EmbeddedFontFace, Inline, Paragraph, Resource, ResourceId,
};
use std::collections::BTreeMap;

pub const EMBEDDED_FONT_MEDIA_TYPE: &str = "application/vnd.impress.font";

pub fn remove_embedded_fonts(document: &mut Document) {
    document
        .resources
        .retain(|_, r| r.media_type != EMBEDDED_FONT_MEDIA_TYPE);
}

/// Resolve inherited formatting and actual script fallbacks before collecting.
/// The save worker operates on a clone; editor state and undo remain untouched.
pub fn embed_document_fonts(document: &mut Document) -> Result<(), String> {
    let fonts = FontCatalog::for_resources(&document.resources);
    let mut requests = BTreeMap::<(String, bool, bool), String>::new();
    fn request(
        out: &mut BTreeMap<(String, bool, bool), String>,
        p: &CharacterProperties,
        text: &str,
    ) {
        for span in p.font_spans(text) {
            let key = (
                span.family.unwrap_or("Linux Libertine G").to_owned(),
                p.bold.unwrap_or(false),
                p.italic.unwrap_or(false),
            );
            let sample = out.entry(key).or_default();
            sample.push_str(&text[span.range]);
            sample.push(' ');
        }
    }

    fn inlines(
        document: &Document,
        paragraph: &Paragraph,
        items: &[Inline],
        out: &mut BTreeMap<(String, bool, bool), String>,
    ) {
        for item in items {
            match item {
                Inline::Text(run) => request(
                    out,
                    &document.styles.resolve_run(
                        paragraph.style.as_ref(),
                        run.style.as_ref(),
                        &run.properties,
                    ),
                    &run.text,
                ),
                Inline::Hyperlink(link) => inlines(document, paragraph, &link.content, out),
                Inline::Field(field) => request(
                    out,
                    &document.styles.resolve_run(
                        paragraph.style.as_ref(),
                        None,
                        &CharacterProperties::default(),
                    ),
                    field.display_text.as_deref().unwrap_or("0123456789IVXivx"),
                ),
                _ => {}
            }
        }
    }
    for paragraph in model::document_paragraphs(document) {
        request(
            &mut requests,
            &document.styles.resolve_run(
                paragraph.style.as_ref(),
                None,
                &CharacterProperties::default(),
            ),
            "0123456789",
        );
        let properties = document
            .styles
            .resolve_paragraph_with_direct(paragraph.style.as_ref(), &paragraph.properties);
        if let Some(list) = properties
            .list_id
            .as_ref()
            .and_then(|id| document.lists.get(id))
        {
            for level in &list.levels {
                request(
                    &mut requests,
                    &document.styles.resolve_run(
                        paragraph.style.as_ref(),
                        None,
                        &CharacterProperties::default(),
                    ),
                    &level.text,
                );
            }
        }
        inlines(document, paragraph, &paragraph.content, &mut requests);
    }
    let mut resources = Vec::<Resource>::new();
    for ((family, bold, italic), text) in requests {
        // Match layout's per-word resolution; mixed scripts may use several faces.
        let mut resolved = Vec::new();
        for word in text.split_whitespace() {
            let font = fonts
                .resolve_for_text(&family, bold, italic, word)
                .ok_or_else(|| format!("No font is available for {family}"))?;
            if !resolved.contains(&font) {
                resolved.push(font);
            }
        }
        let preferred = fonts.resolve(&family, bold, italic);
        resolved.sort_by_key(|font| Some(*font) != preferred);
        for font in resolved {
            let data = fonts
                .with_font_data(font, |bytes, index| standalone_font(bytes, index))
                .ok_or_else(|| format!("Could not read font {family}"))??;
            check_embedding_permission(&data, 0).map_err(|e| {
                format!("Cannot embed {family}: {e}. Save without embedding to continue.")
            })?;
            let actual_family = fonts
                .database()
                .face(font.id)
                .and_then(|face| face.families.first())
                .map(|(name, _)| name.clone());
            let face = EmbeddedFontFace {
                family: family.clone(),
                bold,
                italic,
                face_index: 0,
            };
            let mut bindings = vec![face];
            if let Some(actual_family) =
                actual_family.filter(|actual| !actual.eq_ignore_ascii_case(&family))
            {
                bindings.push(EmbeddedFontFace {
                    family: actual_family,
                    bold,
                    italic,
                    face_index: 0,
                });
            }
            if let Some(resource) = resources
                .iter_mut()
                .find(|r| r.data.as_slice() == data.as_slice())
            {
                for binding in bindings {
                    if !resource.font_faces.contains(&binding) {
                        resource.font_faces.push(binding);
                    }
                }
            } else {
                resources.push(Resource {
                    id: ResourceId::new(),
                    media_type: EMBEDDED_FONT_MEDIA_TYPE.into(),
                    original_name: Some(family.clone()),
                    font_faces: bindings,
                    data: data.into(),
                });
            }
        }
    }
    remove_embedded_fonts(document);
    for resource in resources {
        document.resources.insert(resource.id.clone(), resource);
    }
    Ok(())
}

/// Whether a payload is the requested family, rather than a script fallback.
/// DOCX has only one embedding slot per family/style, so prefer this face.
pub fn font_family_matches(data: &[u8], index: u32, family: &str) -> bool {
    font_family_names(data, index)
        .iter()
        .any(|name| name.eq_ignore_ascii_case(family))
}

/// Genuine family names, including Windows' legacy style-linked family.
pub fn font_family_names(data: &[u8], index: u32) -> Vec<String> {
    let Ok(face) = ttf_parser::Face::parse(data, index) else {
        return Vec::new();
    };
    let mut names = Vec::<String>::new();
    for name in face.names() {
        if !matches!(
            name.name_id,
            ttf_parser::name_id::FAMILY
                | ttf_parser::name_id::TYPOGRAPHIC_FAMILY
                | ttf_parser::name_id::WWS_FAMILY
        ) {
            continue;
        }
        if let Some(name) = name.to_string().filter(|name| !name.trim().is_empty()) {
            if !names.iter().any(|old| old.eq_ignore_ascii_case(&name)) {
                names.push(name);
            }
        }
    }
    names
}

/// Bind actual style-linked names as well as the document's requested aliases.
/// Older Windows consumers only activate a face under its legacy family name.
pub fn embedded_face_bindings(data: &[u8], index: u32) -> Vec<EmbeddedFontFace> {
    let Ok(face) = ttf_parser::Face::parse(data, index) else {
        return Vec::new();
    };
    font_family_names(data, index)
        .into_iter()
        .map(|family| EmbeddedFontFace {
            family,
            bold: face.is_bold(),
            italic: face.is_italic(),
            face_index: index,
        })
        .collect()
}

/// This is an editable document, so preview/print-only licenses are insufficient.
pub fn check_embedding_permission(data: &[u8], index: u32) -> Result<(), String> {
    let face = ttf_parser::Face::parse(data, index).map_err(|_| "invalid OpenType font")?;
    if let Some(os2) = face.tables().os2 {
        if !matches!(
            os2.permissions(),
            Some(ttf_parser::Permissions::Installable | ttf_parser::Permissions::Editable)
        ) || !os2.is_outline_embedding_allowed()
        {
            return Err(
                "the font's embedding permissions do not allow editable outline embedding".into(),
            );
        }
    }
    Ok(())
}

/// Extract a single sfnt face from a TTC, relocating table offsets. DOCX font
/// parts contain standalone OpenType fonts, not a platform font collection.
pub fn standalone_font(data: &[u8], index: u32) -> Result<Vec<u8>, String> {
    fn u32_at(data: &[u8], at: usize) -> Option<u32> {
        Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?))
    }
    if !data.starts_with(b"ttcf") {
        ttf_parser::Face::parse(data, index).map_err(|_| "Invalid font")?;
        return Ok(data.to_vec());
    }
    let fail = || "Invalid font collection".to_owned();
    let count = u32_at(data, 8).ok_or_else(fail)?;
    if index >= count {
        return Err(fail());
    }
    let offset = u32_at(data, 12 + index as usize * 4).ok_or_else(fail)? as usize;
    let header = data.get(offset..offset + 12).ok_or_else(fail)?;
    let n = u16::from_be_bytes([header[4], header[5]]) as usize;
    let directory = data.get(offset..offset + 12 + n * 16).ok_or_else(fail)?;
    let mut out = directory.to_vec();
    let mut head = None;
    for i in 0..n {
        let record = 12 + i * 16;
        let source = u32_at(directory, record + 8).ok_or_else(fail)? as usize;
        let len = u32_at(directory, record + 12).ok_or_else(fail)? as usize;
        let table = data
            .get(source..source.checked_add(len).ok_or_else(fail)?)
            .ok_or_else(fail)?;
        let dest = out.len();
        out[record + 8..record + 12].copy_from_slice(&(dest as u32).to_be_bytes());
        out.extend_from_slice(table);
        if &directory[record..record + 4] == b"head" && len >= 12 {
            head = Some(dest);
            out[dest + 8..dest + 12].fill(0);
        }
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }
    if let Some(head) = head {
        let sum = out.chunks_exact(4).fold(0u32, |sum, b| {
            sum.wrapping_add(u32::from_be_bytes(b.try_into().unwrap()))
        });
        out[head + 8..head + 12].copy_from_slice(&0xB1B0AFBAu32.wrapping_sub(sum).to_be_bytes());
    }
    ttf_parser::Face::parse(&out, 0).map_err(|_| fail())?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(family: &str) -> Resource {
        Resource {
            id: ResourceId::new(),
            media_type: EMBEDDED_FONT_MEDIA_TYPE.into(),
            original_name: None,
            font_faces: vec![EmbeddedFontFace {
                family: family.into(),
                bold: false,
                italic: false,
                face_index: 0,
            }],
            data: typst_assets::fonts().next().unwrap().to_vec().into(),
        }
    }
    #[test]
    fn aliases_are_document_local_and_save_deduplicates_payloads() {
        let mut d = Document::default();
        let resource = fixture("Impress Test Alias");
        let original = resource.data.clone();
        d.resources.insert(resource.id.clone(), resource);
        let mut p = Paragraph::plain("ABC ABC");
        if let Inline::Text(run) = &mut p.content[0] {
            run.properties.font_family = Some("Impress Test Alias".into());
        }
        d.sections[0].blocks.push(model::Block::Paragraph(p));
        let catalog = FontCatalog::for_resources(&d.resources);
        let f = catalog.resolve("Impress Test Alias", false, false).unwrap();
        assert_eq!(
            catalog.with_font_data(f, |data, _| data.to_vec()).unwrap(),
            *original
        );
        assert!(
            FontCatalog::system()
                .resolve("Impress Test Alias", false, false)
                .is_none()
        );
        embed_document_fonts(&mut d).unwrap();
        let payloads: Vec<_> = d
            .resources
            .values()
            .filter(|r| r.media_type == EMBEDDED_FONT_MEDIA_TYPE)
            .collect();
        assert_eq!(payloads.iter().filter(|r| r.data == original).count(), 1);
        remove_embedded_fonts(&mut d);
        assert!(d.resources.is_empty());
    }
    #[test]
    fn respects_editable_embedding_permissions() {
        let mut data = typst_assets::fonts().next().unwrap().to_vec();
        check_embedding_permission(&data, 0).unwrap();
        let face = ttf_parser::Face::parse(&data, 0).unwrap();
        let table = face
            .raw_face()
            .table_records
            .into_iter()
            .find(|t| t.tag == ttf_parser::Tag::from_bytes(b"OS/2"))
            .unwrap();
        let offset = table.offset as usize + 8;
        for bits in [2u16, 4, 0x0200] {
            data[offset..offset + 2].copy_from_slice(&bits.to_be_bytes());
            assert!(check_embedding_permission(&data, 0).is_err());
        }
        for bits in [0u16, 8, 0x0108] {
            data[offset..offset + 2].copy_from_slice(&bits.to_be_bytes());
            assert!(check_embedding_permission(&data, 0).is_ok());
        }
    }
    #[test]
    fn extracts_collection_face_with_correct_checksum() {
        let original = typst_assets::fonts().next().unwrap();
        let mut ttc = b"ttcf\0\x01\0\0\0\0\0\x01\0\0\0\x10".to_vec();
        ttc.extend_from_slice(original);
        let n = u16::from_be_bytes([ttc[20], ttc[21]]) as usize;
        for i in 0..n {
            let at = 16 + 12 + i * 16 + 8;
            let offset = u32::from_be_bytes(ttc[at..at + 4].try_into().unwrap()) + 16;
            ttc[at..at + 4].copy_from_slice(&offset.to_be_bytes());
        }
        let extracted = standalone_font(&ttc, 0).unwrap();
        let a = ttf_parser::Face::parse(original, 0).unwrap();
        let b = ttf_parser::Face::parse(&extracted, 0).unwrap();
        assert_eq!(a.number_of_glyphs(), b.number_of_glyphs());
        assert_eq!(a.glyph_index('A'), b.glyph_index('A'));
        let sum = extracted.chunks_exact(4).fold(0u32, |sum, b| {
            sum.wrapping_add(u32::from_be_bytes(b.try_into().unwrap()))
        });
        assert_eq!(sum, 0xB1B0AFBA);
        assert!(standalone_font(&ttc, 1).is_err());
    }
    #[test]
    fn binding_change_invalidates_private_catalogue() {
        let mut d = Document::default();
        let resource = fixture("First Alias");
        let id = resource.id.clone();
        d.resources.insert(id.clone(), resource);
        assert!(
            FontCatalog::for_resources(&d.resources)
                .resolve("First Alias", false, false)
                .is_some()
        );
        d.resources.get_mut(&id).unwrap().font_faces[0].family = "Second Alias".into();
        let catalog = FontCatalog::for_resources(&d.resources);
        assert!(catalog.resolve("First Alias", false, false).is_none());
        assert!(catalog.resolve("Second Alias", false, false).is_some());
    }
}
