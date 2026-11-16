//! Import live SDT boundaries and custom XML stores without flattening constraints.
use super::*;
use model::{
    AnchorRange, ContentBinding, ContentControl, ContentControlItem, ContentControlKind,
    ContentControlLock, ContentControlScope, CustomXmlData, TextAnchor,
};

fn value<'a>(node: &'a XmlNode, name: &str) -> Option<&'a str> {
    node.child(name).and_then(|n| n.attr("w:val"))
}
fn items(node: &XmlNode) -> Vec<ContentControlItem> {
    node.children_named("w:listItem")
        .map(|n| {
            let value = n.attr("w:value").unwrap_or("").to_owned();
            ContentControlItem {
                display: n.attr("w:displayText").unwrap_or(&value).into(),
                value,
            }
        })
        .collect()
}
fn symbol(node: Option<&XmlNode>, fallback: &str) -> String {
    node.and_then(|n| n.attr("w14:val"))
        .and_then(|v| u32::from_str_radix(v, 16).ok())
        .and_then(char::from_u32)
        .map(|v| v.to_string())
        .unwrap_or_else(|| fallback.into())
}
fn properties(
    node: &XmlNode,
    range: AnchorRange,
    scope: ContentControlScope,
) -> Option<ContentControl> {
    let props = node.child("w:sdtPr")?;
    if value(props, "w:tag") == Some("Impress.Contents.v1") {
        return None;
    }
    let kind = if let Some(n) = props.child("w:text") {
        ContentControlKind::PlainText {
            multiline: n
                .attr("w:multiLine")
                .is_some_and(|v| matches!(v, "1" | "true" | "on")),
        }
    } else if let Some(n) = props.child("w:dropDownList") {
        ContentControlKind::DropDown { items: items(n) }
    } else if let Some(n) = props.child("w:comboBox") {
        ContentControlKind::ComboBox { items: items(n) }
    } else if let Some(n) = props.child("w14:checkbox") {
        ContentControlKind::CheckBox {
            checked: n
                .child("w14:checked")
                .and_then(|n| n.attr("w14:val"))
                .is_some_and(|v| matches!(v, "1" | "true" | "on")),
            checked_symbol: symbol(n.child("w14:checkedState"), "☒"),
            unchecked_symbol: symbol(n.child("w14:uncheckedState"), "☐"),
        }
    } else if let Some(n) = props.child("w:date") {
        ContentControlKind::Date {
            format: value(n, "w:dateFormat").unwrap_or("yyyy-MM-dd").into(),
            language: value(n, "w:lid").map(str::to_owned),
            calendar: value(n, "w:calendar").map(str::to_owned),
            full_date: n.attr("w:fullDate").map(str::to_owned),
        }
    } else if props.child("w:picture").is_some() {
        ContentControlKind::Picture
    } else if props.child("w:group").is_some() {
        ContentControlKind::Group
    } else {
        ContentControlKind::RichText
    };
    let lock = value(props, "w:lock");
    let binding = props.child("w:dataBinding").map(|binding| ContentBinding {
        store_id: binding.attr("w:storeItemID").unwrap_or("").into(),
        xpath: binding.attr("w:xpath").unwrap_or("").into(),
        namespaces: namespaces(binding.attr("w:prefixMappings").unwrap_or("")),
    });
    Some(ContentControl {
        id: NodeId::new(),
        source_id: value(props, "w:id").map(str::to_owned),
        tag: value(props, "w:tag").map(str::to_owned),
        alias: value(props, "w:alias").map(str::to_owned),
        range,
        scope,
        members: Vec::new(),
        kind,
        lock: ContentControlLock {
            deletion: matches!(lock, Some("sdtLocked" | "sdtContentLocked")),
            content: matches!(lock, Some("contentLocked" | "sdtContentLocked")),
        },
        binding,
        placeholder: props.child("w:showingPlcHdr").is_some(),
        temporary: on_off(props.child("w:temporary")).unwrap_or(false),
        properties_xml: Some(node_xml(props)),
    })
}
fn namespaces(text: &str) -> BTreeMap<String, String> {
    // Prefix mappings use XML namespace declaration syntax, including either quote style.
    let xml = format!("<bindings {text}/>");
    parse_xml_tree(&xml)
        .ok()
        .map(|tree| {
            tree.children
                .into_iter()
                .next()
                .unwrap_or_default()
                .attrs
                .into_iter()
                .filter_map(|(name, value)| {
                    name.strip_prefix("xmlns:")
                        .map(|prefix| (prefix.into(), value))
                })
                .collect()
        })
        .unwrap_or_default()
}
fn node_xml(node: &XmlNode) -> String {
    let attrs = node
        .attrs
        .iter()
        .map(|(key, value)| format!(" {key}=\"{}\"", escape(value)))
        .collect::<String>();
    format!(
        "<{}{attrs}>{}{}</{}>",
        node.name,
        escape(&node.text),
        node.children.iter().map(node_xml).collect::<String>(),
        node.name
    )
}
fn escape(text: &str) -> String {
    quick_xml::escape::escape(text).into_owned()
}
pub(super) fn begin_inline(
    node: &XmlNode,
    paragraph: &Paragraph,
    context: &ImportContext,
) -> Option<ContentControl> {
    let anchor = TextAnchor {
        paragraph: paragraph.id.clone(),
        offset: model::inline_text(&paragraph.content).len(),
    };
    let mut control = properties(
        node,
        AnchorRange {
            start: anchor.clone(),
            end: anchor,
        },
        ContentControlScope::Inline,
    )?;
    control.members = context
        .anchored
        .borrow()
        .iter()
        .map(|image| image.id.clone())
        .collect();
    Some(control)
}
pub(super) fn end_inline(
    control: Option<ContentControl>,
    paragraph: &Paragraph,
    context: &ImportContext,
) {
    if let Some(mut control) = control {
        control.range.end.offset = model::inline_text(&paragraph.content).len();
        control.members = context
            .anchored
            .borrow()
            .iter()
            .filter(|image| !control.members.contains(&image.id))
            .map(|image| image.id.clone())
            .collect();
        context
            .content_controls
            .borrow_mut()
            .insert(control.id.clone(), control);
    }
}
pub(super) fn project_blocks(source: &XmlNode, _blocks: &[Block], context: &ImportContext) {
    fn members(node: &XmlNode, context: &ImportContext, out: &mut Vec<NodeId>) {
        for child in &node.children {
            if matches!(child.name.as_str(), "w:p" | "w:tbl" | "w:tr" | "w:tc") {
                if let Some(id) = context
                    .source_members
                    .borrow()
                    .get(&(child as *const XmlNode as usize))
                {
                    out.push(id.clone());
                }
            } else if matches!(
                child.name.as_str(),
                "w:sdt" | "w:sdtContent" | "w:customXml" | "w:smartTag"
            ) {
                members(child, context, out);
            }
        }
    }
    fn walk(node: &XmlNode, context: &ImportContext) -> Vec<Paragraph> {
        if node.name == "w:p" {
            return context
                .source_paragraphs
                .borrow()
                .get(&(node as *const XmlNode as usize))
                .cloned()
                .into_iter()
                .collect();
        }
        if matches!(node.name.as_str(), "w:del" | "w:moveFrom") {
            return Vec::new();
        }
        let paragraphs = node
            .children
            .iter()
            .filter(|child| child.name != "w:sdtPr")
            .flat_map(|child| walk(child, context))
            .collect::<Vec<_>>();
        if node.name == "w:sdt" {
            if let (Some(first), Some(last), Some(content)) = (
                paragraphs.first(),
                paragraphs.last(),
                node.child("w:sdtContent"),
            ) {
                let scope = if children(content, "w:tr").next().is_some() {
                    ContentControlScope::Row
                } else if children(content, "w:tc").next().is_some() {
                    ContentControlScope::Cell
                } else {
                    ContentControlScope::Blocks
                };
                let range = AnchorRange {
                    start: TextAnchor {
                        paragraph: first.id.clone(),
                        offset: 0,
                    },
                    end: TextAnchor {
                        paragraph: last.id.clone(),
                        offset: model::inline_text(&last.content).len(),
                    },
                };
                if let Some(mut control) = properties(node, range, scope) {
                    members(content, context, &mut control.members);
                    context
                        .content_controls
                        .borrow_mut()
                        .insert(control.id.clone(), control);
                }
            }
        }
        paragraphs
    }
    walk(source, context);
}
pub(super) fn load_stores<R: Read + std::io::Seek>(
    zip: &mut ZipArchive<R>,
) -> FormatResult<BTreeMap<String, CustomXmlData>> {
    let names = zip
        .file_names()
        .filter(|name| {
            name.starts_with("customXml/")
                && name.ends_with(".xml")
                && !name.contains("/rels/")
                && !name.contains("/_rels/")
        })
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mut stores = BTreeMap::new();
    for name in names {
        let rel_path = relationship_part_path(&name);
        let Some(xml) = read_optional_text(zip, &rel_path)? else {
            continue;
        };
        let relations = parse_relationships(&xml)?;
        for relation in relations
            .values()
            .filter(|relation| !relation.external && relation.kind.ends_with("/customXmlProps"))
        {
            let directory = name.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");
            let path = if relation.target.starts_with('/') {
                relation.target.trim_start_matches('/').into()
            } else {
                format!("{directory}/{}", relation.target)
            };
            let Some(props) = read_optional_text(zip, &path)? else {
                continue;
            };
            let tree = parse_xml_tree(&props)?;
            fn id(node: &XmlNode) -> Option<&str> {
                node.attrs
                    .iter()
                    .find(|(key, _)| key.rsplit(':').next() == Some("itemID"))
                    .map(|(_, value)| value.as_str())
                    .or_else(|| node.children.iter().find_map(id))
            }
            let Some(id) = id(&tree) else {
                continue;
            };
            if let Some(xml) = read_optional_text(zip, &name)? {
                parse_xml_tree(&xml)?;
                stores.insert(
                    model::normalized_store_id(id),
                    CustomXmlData {
                        id: id.into(),
                        xml,
                        original_name: Some(name.clone()),
                    },
                );
            }
        }
    }
    Ok(stores)
}
pub(super) fn finish(document: &mut Document, context: &ImportContext) {
    for control in document.content_controls.values() {
        let unsupported = control
            .properties_xml
            .as_ref()
            .and_then(|xml| parse_xml_tree(xml).ok())
            .and_then(|tree| tree.children.into_iter().next())
            .is_some_and(|props| {
                props.children.iter().any(|node| {
                    !matches!(
                        node.name.as_str(),
                        "w:rPr"
                            | "w:id"
                            | "w:alias"
                            | "w:tag"
                            | "w:lock"
                            | "w:placeholder"
                            | "w:showingPlcHdr"
                            | "w:dataBinding"
                            | "w:text"
                            | "w:dropDownList"
                            | "w:comboBox"
                            | "w14:checkbox"
                            | "w:date"
                            | "w:picture"
                            | "w:group"
                            | "w:richText"
                            | "w:temporary"
                            | "w15:appearance"
                            | "w15:color"
                    )
                })
            });
        if unsupported {
            document.compatibility.push(model::CompatibilityIssue{code:"content-control-constraint".into(),part:context.part.clone(),message:"This content control uses an unsupported constraint; its visible content and metadata are retained.".into()});
        }
        if matches!(&control.kind,ContentControlKind::Date{calendar:Some(calendar),..}if calendar!="gregorian")
        {
            document.compatibility.push(model::CompatibilityIssue{code:"content-control-calendar".into(),part:context.part.clone(),message:"This date control uses a non-Gregorian calendar; its calendar metadata is retained.".into()});
        }
    }
    let ids = document
        .content_controls
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    for id in ids {
        let Some(control) = document.content_controls.get(&id).cloned() else {
            continue;
        };
        if document.content_control_is_dormant(&control) {
            continue;
        }
        if let Some(binding) = &control.binding {
            let result = document.bound_content_value(binding).and_then(|value| {
                if matches!(control.kind, ContentControlKind::Picture) {
                    let resource = model::decode_content_picture(&value)?;
                    if document
                        .content_control_picture(&control)
                        .is_ok_and(|current| current.data == resource.data)
                    {
                        return Ok(());
                    }
                    let id = resource.id.clone();
                    document.resources.insert(id.clone(), resource);
                    document.replace_content_control_picture(&control, &id)?;
                    return Ok(());
                }
                let display = control.displayed_value(&value);
                if model::content_control_text(document, &control.range).as_deref()
                    != Some(&display)
                {
                    refresh_bound_text(document, &control, &display)?;
                }
                if let Some(current) = document.content_controls.get_mut(&id) {
                    current.placeholder = false;
                    if let ContentControlKind::CheckBox { checked, .. } = &mut current.kind {
                        *checked = matches!(value.as_str(), "true" | "1");
                    }
                }
                Ok(())
            });
            if let Err(message) = result {
                document.compatibility.push(model::CompatibilityIssue {
                    code: "content-binding".into(),
                    part: context.part.clone(),
                    message,
                });
            }
        }
    }
}

/// Direct row/cell children can be carried by a structured-control wrapper.
pub(super) fn children<'a>(node: &'a XmlNode, name: &str) -> std::vec::IntoIter<&'a XmlNode> {
    fn collect<'a>(node: &'a XmlNode, name: &str, out: &mut Vec<&'a XmlNode>) {
        for child in &node.children {
            if child.name == name {
                out.push(child);
            } else if matches!(
                child.name.as_str(),
                "w:sdt" | "w:sdtContent" | "w:customXml"
            ) {
                collect(child, name, out);
            }
        }
    }
    let mut out = Vec::new();
    collect(node, name, &mut out);
    out.into_iter()
}

fn refresh_bound_text(
    document: &mut Document,
    control: &ContentControl,
    value: &str,
) -> Result<(), String> {
    let paragraphs = model::document_paragraphs(document);
    let first = paragraphs
        .iter()
        .position(|p| p.id == control.range.start.paragraph)
        .ok_or("The content control's start paragraph is missing")?;
    let last = paragraphs
        .iter()
        .position(|p| p.id == control.range.end.paragraph)
        .ok_or("The content control's end paragraph is missing")?;
    if first > last {
        return Err("The content control's range is invalid".into());
    }
    let originals = paragraphs[first..=last]
        .iter()
        .map(|p| (*p).clone())
        .collect::<Vec<_>>();
    if first != last {
        fn containers(blocks: &[Block], out: &mut Vec<NodeId>) {
            for block in blocks {
                if let Block::Table(table) = block {
                    for row in &table.rows {
                        for cell in &row.cells {
                            out.push(cell.id.clone());
                            containers(&cell.blocks, out);
                        }
                    }
                }
            }
        }
        let mut ids = Vec::new();
        for section in &document.sections {
            ids.push(section.id.clone());
            containers(&section.blocks, &mut ids);
            for story in section.header_footers() {
                ids.push(story.stream.id.clone());
                containers(&story.stream.blocks, &mut ids);
            }
        }
        for note in document.review.footnotes.values() {
            ids.push(note.id.clone());
            containers(&note.blocks, &mut ids);
        }
        let found = ids
            .into_iter()
            .find_map(|id| {
                let blocks = model::document_blocks(document, &id)?;
                let start = blocks
                    .iter()
                    .position(|b| model::block_id(b) == &control.range.start.paragraph)?;
                let end = blocks
                    .iter()
                    .position(|b| model::block_id(b) == &control.range.end.paragraph)?;
                (start <= end
                    && blocks[start..=end]
                        .iter()
                        .all(|b| matches!(b, Block::Paragraph(_))))
                .then_some((id, start, end))
            })
            .ok_or("The bound text spans incompatible structural containers")?;
        let (mut prefix, _) =
            model::split_content(&originals[0].content, control.range.start.offset)?;
        let (_, suffix) = model::split_content(
            &originals[originals.len() - 1].content,
            control.range.end.offset,
        )?;
        let mut run = originals[0]
            .content
            .iter()
            .find_map(|inline| {
                if let Inline::Text(run) = inline {
                    Some(run.clone())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| TextRun::plain(""));
        run.text = value.into();
        if !run.text.is_empty() {
            prefix.push(Inline::Text(run));
        }
        prefix.extend(suffix);
        let mut paragraph = originals[0].clone();
        paragraph.content = prefix.into();
        model::document_blocks_mut(document, &found.0)
            .ok_or("The bound text container is missing")?
            .splice(found.1..=found.2, [Block::Paragraph(paragraph)]);
        let start = &control.range.start;
        let end = &control.range.end;
        let removed = originals[1..]
            .iter()
            .map(|p| p.id.clone())
            .collect::<Vec<_>>();
        model::map_document_anchors(document, |anchor, end_gravity| {
            if anchor.paragraph == start.paragraph && anchor.offset >= start.offset {
                anchor.offset = if anchor.offset == start.offset && end_gravity {
                    start.offset
                } else {
                    start.offset + value.len()
                };
            } else if anchor.paragraph == end.paragraph {
                anchor.paragraph = start.paragraph.clone();
                anchor.offset =
                    start.offset + value.len() + anchor.offset.saturating_sub(end.offset);
            } else if removed.contains(&anchor.paragraph) {
                anchor.paragraph = start.paragraph.clone();
                anchor.offset = start.offset;
            }
        });
        for current in document.content_controls.values_mut() {
            current.members.retain(|id| !removed.contains(id));
        }
        return Ok(());
    }
    let lines = value.split('\n').collect::<Vec<_>>();
    fn replace(blocks: &mut [Block], paragraph: &Paragraph) -> bool {
        for block in blocks {
            match block {
                Block::Paragraph(current) if current.id == paragraph.id => {
                    *current = paragraph.clone();
                    return true;
                }
                Block::Table(table) => {
                    for row in &mut table.rows {
                        for cell in &mut row.cells {
                            if replace(&mut cell.blocks, paragraph) {
                                return true;
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        false
    }
    for (index, old) in originals.iter().enumerate() {
        let start = if index == 0 {
            control.range.start.offset
        } else {
            0
        };
        let end = if first + index == last {
            control.range.end.offset
        } else {
            model::inline_text(&old.content).len()
        };
        let replacement = if first + index == last {
            lines.get(index..).unwrap_or(&[]).join("\n")
        } else {
            lines.get(index).copied().unwrap_or("").into()
        };
        let (mut prefix, _) = model::split_content(&old.content, start)?;
        let (_, suffix) = model::split_content(&old.content, end)?;
        let mut run = old
            .content
            .iter()
            .find_map(|inline| {
                if let Inline::Text(run) = inline {
                    Some(run.clone())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| TextRun::plain(""));
        run.text = replacement;
        if !run.text.is_empty() {
            prefix.push(Inline::Text(run));
        }
        prefix.extend(suffix);
        let mut paragraph = old.clone();
        paragraph.content = prefix.into();
        let old_text = model::inline_text(&old.content);
        let new_text = model::inline_text(&paragraph.content);
        for section in &mut document.sections {
            replace(&mut section.blocks, &paragraph);
            for story in section.header_footers_mut() {
                replace(&mut story.blocks, &paragraph);
            }
        }
        for note in document.review.footnotes.values_mut() {
            replace(&mut note.blocks, &paragraph);
        }
        let controls = document.content_controls.clone();
        model::map_document_anchors(document, |anchor, end| {
            if anchor.paragraph == old.id {
                anchor.offset = model::rebase_offset(&old_text, &new_text, anchor.offset, end);
            }
        });
        for current in document.content_controls.values_mut() {
            if let Some(old_control) = controls.get(&current.id) {
                if old_control.range.start.paragraph == old.id {
                    current.range.start.offset = model::rebase_offset(
                        &old_text,
                        &new_text,
                        old_control.range.start.offset,
                        true,
                    );
                }
                if old_control.range.end.paragraph == old.id {
                    current.range.end.offset = model::rebase_offset(
                        &old_text,
                        &new_text,
                        old_control.range.end.offset,
                        false,
                    );
                }
            }
        }
    }
    Ok(())
}
