use super::*;
use std::collections::BTreeSet;

pub(super) fn load<R: Read + std::io::Seek>(
    zip: &mut ZipArchive<R>,
    relationships: &BTreeMap<String, Relationship>,
    context: &mut ImportContext,
) -> FormatResult<BTreeSet<String>> {
    let mut consumed = BTreeSet::new();
    for relation in relationships
        .values()
        .filter(|r| !r.external && r.kind.ends_with("/fontTable"))
    {
        let path = relationship_path(&relation.target);
        let Some(xml) = read_optional_text(zip, &path)? else {
            continue;
        };
        let rel_path = relationship_part_path(&path);
        let rels = parse_relationships(&read_optional_text(zip, &rel_path)?.unwrap_or_default())?;
        consumed.extend([path.clone(), rel_path]);
        let tree = parse_xml_tree(&xml)?;
        let Some(root) = find_descendant(&tree, "w:fonts") else {
            continue;
        };
        let mut loaded = BTreeMap::<(String, String), ResourceId>::new();
        for font in root.children_named("w:font") {
            let Some(family) = font.attr("w:name") else {
                continue;
            };
            for (tag, bold, italic) in [
                ("w:embedRegular", false, false),
                ("w:embedBold", true, false),
                ("w:embedItalic", false, true),
                ("w:embedBoldItalic", true, true),
            ] {
                let Some(binding) = font.child(tag) else {
                    continue;
                };
                let Some(rel) = binding
                    .attr("r:id")
                    .and_then(|id| rels.get(id))
                    .filter(|r| !r.external && r.kind.ends_with("/font"))
                else {
                    continue;
                };
                // Relationship targets are relative to the font table's folder.
                let base = path.rsplit_once('/').map_or("", |(dir, _)| dir);
                let mut parts = Vec::new();
                let joined = if rel.target.starts_with('/') {
                    rel.target.clone()
                } else {
                    format!("{base}/{}", rel.target)
                };
                for part in joined.split('/') {
                    match part {
                        "" | "." => {}
                        ".." => {
                            if parts.pop().is_none() {
                                return Err(invalid("Font relationship escapes package"));
                            }
                        }
                        _ => parts.push(part),
                    }
                }
                let target = parts.join("/");
                let key = binding
                    .attr("w:fontKey")
                    .unwrap_or("00000000-0000-0000-0000-000000000000")
                    .to_owned();
                let cache_key = (target.clone(), key.clone());
                let id = if let Some(id) = loaded.get(&cache_key) {
                    id.clone()
                } else {
                    let mut data = Vec::new();
                    zip.by_name(&target)
                        .map_err(zip_err)?
                        .take(format::MAX_PACKAGE_MEMBER + 1)
                        .read_to_end(&mut data)?;
                    format::check_member_len(data.len(), "DOCX")?;
                    crate::fonts::obfuscate(&mut data, &key).map_err(|e| invalid(&e))?;
                    let data = typography::standalone_font(&data, 0).map_err(|e| invalid(&e))?;
                    let id = ResourceId::new();
                    context.resources.insert(
                        id.clone(),
                        Resource {
                            id: id.clone(),
                            media_type: typography::EMBEDDED_FONT_MEDIA_TYPE.into(),
                            original_name: Some(target.clone()),
                            font_faces: Vec::new(),
                            data: data.into(),
                        },
                    );
                    loaded.insert(cache_key, id.clone());
                    id
                };
                let resource = context.resources.get_mut(&id).expect("loaded font");
                let face = model::EmbeddedFontFace {
                    family: family.into(),
                    bold,
                    italic,
                    face_index: 0,
                };
                if !resource.font_faces.contains(&face) {
                    resource.font_faces.push(face);
                }
                consumed.insert(target);
            }
        }
    }
    Ok(consumed)
}
