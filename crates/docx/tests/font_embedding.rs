use docx::DocxFormat;
use format::DocumentFormat;
use model::{Document, EmbeddedFontFace, Resource, ResourceId};
use std::io::{Cursor, Read};
fn fixture() -> Document {
    let mut doc = Document::default();
    for (index, (bold, italic)) in [(false, false), (true, false), (false, true), (true, true)]
        .into_iter()
        .enumerate()
    {
        let id = ResourceId::new();
        doc.resources.insert(
            id.clone(),
            Resource {
                id,
                media_type: typography::EMBEDDED_FONT_MEDIA_TYPE.into(),
                original_name: None,
                font_faces: vec![EmbeddedFontFace {
                    family: "Test & Portable".into(),
                    bold,
                    italic,
                    face_index: 0,
                }],
                data: typst_assets::fonts().nth(index).unwrap().to_vec().into(),
            },
        );
    }
    doc
}
fn export(doc: &Document) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    DocxFormat.export(doc, &mut out).unwrap();
    out.into_inner()
}
fn text(zip: &mut zip::ZipArchive<Cursor<Vec<u8>>>, name: &str) -> String {
    let mut out = String::new();
    zip.by_name(name).unwrap().read_to_string(&mut out).unwrap();
    out
}
#[test]
fn all_variants_have_parts_relationships_and_reversible_payloads() {
    let doc = fixture();
    let bytes = export(&doc);
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes.clone())).unwrap();
    let table = text(&mut zip, "word/fontTable.xml");
    assert!(table.contains("Test &amp; Portable"));
    for tag in [
        "embedRegular",
        "embedBold",
        "embedItalic",
        "embedBoldItalic",
    ] {
        assert!(table.contains(tag));
    }
    assert!(text(&mut zip, "word/_rels/document.xml.rels").contains("relationships/fontTable"));
    assert!(text(&mut zip, "word/_rels/fontTable.xml.rels").contains("relationships/font"));
    assert!(
        text(&mut zip, "[Content_Types].xml")
            .contains("application/vnd.openxmlformats-officedocument.obfuscatedFont")
    );
    assert!(text(&mut zip, "word/settings.xml").contains("embedTrueTypeFonts"));
    let mut payload = Vec::new();
    zip.by_name("word/fonts/impressFont1.odttf")
        .unwrap()
        .read_to_end(&mut payload)
        .unwrap();
    assert_ne!(&payload[..32], &typst_assets::fonts().next().unwrap()[..32]);
    assert!(
        typst_assets::fonts()
            .take(4)
            .any(|font| &payload[32..] == &font[32..])
    );
    let imported = DocxFormat.import(&mut Cursor::new(bytes)).unwrap();
    assert_eq!(imported.resources.len(), 4);
    let catalog = typography::FontCatalog::for_resources(&imported.resources);
    for (index, (bold, italic)) in [(false, false), (true, false), (false, true), (true, true)]
        .into_iter()
        .enumerate()
    {
        let font = catalog.resolve("Test & Portable", bold, italic).unwrap();
        assert_eq!(
            catalog
                .with_font_data(font, |data, _| data.to_vec())
                .unwrap(),
            typst_assets::fonts().nth(index).unwrap()
        );
    }
    let again = DocxFormat
        .import(&mut Cursor::new(export(&imported)))
        .unwrap();
    assert_eq!(again.resources.len(), 4);
}
#[test]
fn opting_out_removes_payloads_and_old_package_declarations() {
    let mut doc = DocxFormat
        .import(&mut Cursor::new(export(&fixture())))
        .unwrap();
    typography::remove_embedded_fonts(&mut doc);
    let bytes = export(&doc);
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    assert!(!zip.file_names().any(|name| name.ends_with(".odttf")));
    assert!(!text(&mut zip, "word/fontTable.xml").contains("embedRegular"));
    assert!(!text(&mut zip, "word/settings.xml").contains("embedTrueTypeFonts"));
}

#[test]
fn font_aliases_share_one_obfuscated_payload() {
    let mut doc = fixture();
    doc.resources
        .retain(|_, r| !r.font_faces[0].bold && !r.font_faces[0].italic);
    let resource = doc.resources.values_mut().next().unwrap();
    let mut alias = resource.font_faces[0].clone();
    alias.family = "Another Alias".into();
    resource.font_faces.push(alias);
    let bytes = export(&doc);
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes.clone())).unwrap();
    assert_eq!(
        zip.file_names().filter(|n| n.ends_with(".odttf")).count(),
        1
    );
    let table = text(&mut zip, "word/fontTable.xml");
    assert!(table.contains("w:name=\"Test &amp; Portable\""));
    assert!(table.contains("w:name=\"Another Alias\""));
    // The genuine Windows family also shares the payload with both aliases.
    assert_eq!(table.matches("r:id=\"rIdFont1\"").count(), 3);
    let loaded = DocxFormat.import(&mut Cursor::new(bytes)).unwrap();
    assert_eq!(loaded.resources.len(), 1);
    assert_eq!(
        loaded.resources.values().next().unwrap().font_faces.len(),
        3
    );
}

#[test]
fn embedded_neo_aegean_fonts_decode_after_native_and_docx_round_trips_without_image_warnings() {
    for bytes in [
        include_bytes!("../../typography/tests/fixtures/neo-aegean/NeoAegean-Regular.otf").as_slice(),
        include_bytes!("../../typography/tests/fixtures/neo-aegean/NeoAegean-Regular.ttf").as_slice(),
    ] {
        let mut document=Document::default();
        let family="Impress Portable Neo Aegean";
        let id=ResourceId::new();
        document.resources.insert(id.clone(),Resource {
            id,media_type:typography::EMBEDDED_FONT_MEDIA_TYPE.into(),original_name:None,
            font_faces:vec![EmbeddedFontFace{family:family.into(),bold:false,italic:false,face_index:0}],
            data:bytes.to_vec().into(),
        });
        let sample="Από πότε που δημιουργήθηκαν";
        let mut paragraph=model::Paragraph::plain(sample);
        if let model::Inline::Text(run)=&mut paragraph.content[0] {run.properties.font_family=Some(family.into());}
        document.sections[0].blocks.push(model::Block::Paragraph(paragraph));
        assert!(docx::export_warnings(&document).is_empty(),"fonts must never be diagnosed as images");
        let mut native=Cursor::new(Vec::new());
        format_native::NativeImpressFormat.export(&document,&mut native).unwrap();native.set_position(0);
        let restored=format_native::NativeImpressFormat.import(&mut native).unwrap();
        let restored_docx=DocxFormat.import(&mut Cursor::new(export(&restored))).unwrap();
        for decoded in [&restored,&restored_docx] {
            assert!(!docx::export_warnings(decoded).iter().any(|w|w.contains("cannot decode every image")));
            let catalog=typography::FontCatalog::for_resources(&decoded.resources);
            let font=catalog.resolve(family,false,false).unwrap();
            let shaped=catalog.with_face(font,|face|typography::shape_with_face(face,sample,&typography::ShapeOptions::default())).unwrap().unwrap();
            assert!(shaped.advance_pt>0.0);
            assert!(shaped.glyphs.iter().all(|glyph|glyph.glyph_id!=0));
            assert_eq!(catalog.with_font_data(font,|data,_|data.to_vec()).unwrap(),bytes);
            assert!(catalog.with_face(font,|face|face.glyph_index('Α').and_then(|id|face.glyph_bounding_box(id)).is_some()).unwrap());
        }
    }
}

#[test]
fn image_diagnostics_are_scoped_to_images_not_generic_binary_resources() {
    let mut document=Document::default();
    for media_type in [typography::EMBEDDED_FONT_MEDIA_TYPE,"application/octet-stream","application/vnd.impress.ole","image/svg+xml"] {
        let id=ResourceId::new();
        document.resources.insert(id.clone(),Resource{id,media_type:media_type.into(),original_name:None,font_faces:Vec::new(),data:vec![0u8].into()});
    }
    let warnings=docx::export_warnings(&document);
    assert_eq!(warnings.len(),1);
    assert!(warnings[0].contains("image/svg+xml"));
}
