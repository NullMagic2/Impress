//! Build font tables and shared payloads with portable style-linked aliases.
use super::*;

pub(super) struct FontPackage {
    pub table: String,
    pub relationships: String,
    pub payloads: Vec<(String, Vec<u8>)>,
}
impl FontPackage {
    pub fn new(document: &Document) -> FormatResult<Self> {
        let mut families = BTreeMap::<String, BTreeMap<(bool, bool), (String, String)>>::new();
        let mut payloads = Vec::new();
        let mut relationships = String::new();
        let mut written = BTreeMap::<(ResourceId, u32), (String, String)>::new();
        let catalogue = typography::FontCatalog::for_resources(&document.resources);
        let mut bindings = document
            .resources
            .values()
            .filter(|r| r.media_type == typography::EMBEDDED_FONT_MEDIA_TYPE)
            .flat_map(|resource| {
                let mut bindings = resource.font_faces.clone();
                for index in resource.font_faces.iter().map(|face| face.face_index) {
                    for binding in typography::embedded_face_bindings(&resource.data, index) {
                        if !bindings.contains(&binding) {
                            bindings.push(binding);
                        }
                    }
                }
                bindings.into_iter().map(move |binding| (resource, binding))
            })
            .collect::<Vec<_>>();
        bindings.sort_by_key(|(resource, binding)| {
            !typography::font_family_matches(&resource.data, binding.face_index, &binding.family)
        });
        for (resource, binding) in bindings {
            let variants = families.entry(binding.family.clone()).or_default();
            if variants.contains_key(&(binding.bold, binding.italic)) {
                continue;
            }
            let payload_key = (resource.id.clone(), binding.face_index);
            if let Some((id, key)) = written.get(&payload_key) {
                variants.insert((binding.bold, binding.italic), (id.clone(), key.clone()));
                continue;
            }
            let mut data = typography::standalone_font(&resource.data, binding.face_index)
                .map_err(|e| FormatError::Invalid {
                    format: "DOCX",
                    message: e,
                })?;
            typography::check_embedding_permission(&data, 0).map_err(|e| FormatError::Invalid {
                format: "DOCX",
                message: e,
            })?;
            if let Some(compatible) = typography::compatible_truetype_font(&catalogue, &data, 0) {
                data = compatible;
            }
            let number = payloads.len() + 1;
            let key = model::NodeId::new().0.to_uppercase();
            crate::fonts::obfuscate(&mut data, &key).map_err(|e| FormatError::Invalid {
                format: "DOCX",
                message: e,
            })?;
            let id = format!("rIdFont{number}");
            let target = format!("fonts/impressFont{number}.odttf");
            relationships.push_str(&format!("<Relationship Id=\"{id}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/font\" Target=\"{target}\"/>"));
            payloads.push((format!("word/{target}"), data));
            written.insert(payload_key, (id.clone(), key.clone()));
            variants.insert((binding.bold, binding.italic), (id, key));
        }
        let mut table = String::new();
        for (family, variants) in families {
            table.push_str(&format!("<w:font w:name=\"{}\">", xml_escape(&family)));
            // Schema order: regular, bold, italic, boldItalic.
            for (variant, tag) in [
                ((false, false), "embedRegular"),
                ((true, false), "embedBold"),
                ((false, true), "embedItalic"),
                ((true, true), "embedBoldItalic"),
            ] {
                if let Some((id, key)) = variants.get(&variant) {
                    table.push_str(&format!(
                        "<w:{tag} r:id=\"{id}\" w:fontKey=\"{{{key}}}\" w:subsetted=\"false\"/>"
                    ));
                }
            }
            table.push_str("</w:font>");
        }
        Ok(Self {
            table: format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?><w:fonts xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\">{table}</w:fonts>"
            ),
            relationships: format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">{relationships}</Relationships>"
            ),
            payloads,
        })
    }
    pub fn write<W: Write + std::io::Seek>(
        &self,
        zip: &mut ZipWriter<W>,
        options: SimpleFileOptions,
    ) -> FormatResult<()> {
        write_member(zip, "word/fontTable.xml", &self.table, options)?;
        write_member(
            zip,
            "word/_rels/fontTable.xml.rels",
            &self.relationships,
            options,
        )?;
        for (name, data) in &self.payloads {
            zip.start_file(name, options).map_err(zip_err)?;
            zip.write_all(data)?;
        }
        Ok(())
    }
}
