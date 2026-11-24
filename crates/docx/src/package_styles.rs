//! Update semantic styles while retaining unsupported style metadata and properties.
use super::*;

const RUN_PROPERTIES: &[&str] = &[
    "b",
    "i",
    "u",
    "sz",
    "rFonts",
    "color",
    "lang",
    "highlight",
    "shd",
    "rtl",
    "cs",
    "ligatures",
];
const PARAGRAPH_PROPERTIES: &[&str] = &[
    "pStyle",
    "jc",
    "spacing",
    "ind",
    "tabs",
    "keepNext",
    "keepLines",
    "pageBreakBefore",
    "widowControl",
    "bidi",
    "suppressAutoHyphens",
    "numPr",
    "pBdr",
    "shd",
];

pub(super) struct Element<'a> {
    opening: &'a str,
    pub(super) children: Vec<(&'a str, &'a str)>,
    closing: String,
}
pub(super) fn element(xml: &str) -> Option<Element<'_>> {
    let mut reader = Reader::from_str(xml);
    let mut depth = 0usize;
    let mut opening = None;
    let mut children = Vec::new();
    let mut child_start = 0;
    let mut child_name = "";
    loop {
        let start = reader.buffer_position() as usize;
        let event = reader.read_event().ok()?;
        let end = reader.buffer_position() as usize;
        match event {
            Event::Start(_) => {
                if depth == 0 {
                    opening = Some(&xml[start..end]);
                } else if depth == 1 {
                    child_start = start;
                    child_name = tag_name(&xml[start..end]);
                }
                depth += 1;
            }
            Event::Empty(_) if depth == 0 => {
                let tag = &xml[start..end];
                let name = tag_name(tag);
                return Some(Element {
                    opening: &tag[..tag.len() - 2],
                    children,
                    closing: format!("</{name}>"),
                });
            }
            Event::Empty(_) if depth == 1 => {
                children.push((local_name(tag_name(&xml[start..end])), &xml[start..end]));
            }
            Event::End(_) => {
                depth = depth.checked_sub(1)?;
                if depth == 1 {
                    children.push((local_name(child_name), &xml[child_start..end]));
                }
                if depth == 0 {
                    return Some(Element {
                        opening: opening?,
                        children,
                        closing: xml[start..end].into(),
                    });
                }
            }
            Event::Comment(_) | Event::PI(_) if depth == 1 => {
                children.push(("", &xml[start..end]));
            }
            Event::Eof => return None,
            _ => {}
        }
    }
}
fn tag_name(tag: &str) -> &str {
    tag.trim_start_matches('<')
        .split(|c: char| c.is_whitespace() || c == '/' || c == '>')
        .next()
        .unwrap_or("")
}
fn local_name(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}
fn opening(element: &Element<'_>) -> String {
    if element.opening.ends_with('>') {
        element.opening.into()
    } else {
        format!("{}>", element.opening)
    }
}
const WORD_NAMESPACE: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
fn attributes(xml: &str) -> BTreeMap<String, String> {
    let mut reader = Reader::from_str(xml);
    match reader.read_event() {
        Ok(Event::Start(event)) | Ok(Event::Empty(event)) => event
            .attributes()
            .flatten()
            .filter_map(|attribute| {
                quick_xml::escape::unescape(attribute.value.as_ref())
                    .ok()
                    .map(|value| (attribute.key.as_ref().to_owned(), value.into_owned()))
            })
            .collect(),
        _ => BTreeMap::new(),
    }
}
fn namespace_scope(parent: &BTreeMap<String, String>, xml: &str) -> BTreeMap<String, String> {
    let mut scope = parent.clone();
    scope.extend(
        attributes(xml)
            .into_iter()
            .filter(|(key, _)| key == "xmlns" || key.starts_with("xmlns:")),
    );
    scope
}
fn namespace_opening(element: &Element<'_>, namespaces: &BTreeMap<String, String>) -> String {
    let mut attrs = namespaces.clone();
    attrs.extend(attributes(&opening(element)));
    format!(
        "<{}{}>",
        tag_name(element.opening),
        attrs
            .iter()
            .map(|(key, value)| format!(" {key}=\"{}\"", xml_escape(value)))
            .collect::<String>()
    )
}
fn preserve_scoped_child(xml: &str, scope: &BTreeMap<String, String>) -> String {
    let Some(child) = element(xml) else {
        return xml.into();
    };
    let inner = if child.opening.ends_with('>') {
        xml.get(child.opening.len()..xml.len().saturating_sub(child.closing.len()))
            .unwrap_or("")
    } else {
        ""
    };
    format!(
        "{}{}{}",
        namespace_opening(&child, scope),
        inner,
        child.closing
    )
}
fn merge_properties(
    original: Option<&str>,
    generated: &str,
    managed: &[&str],
    name: &str,
    parent_scope: &BTreeMap<String, String>,
) -> String {
    let original = original.and_then(element);
    let scope = original
        .as_ref()
        .map(|element| namespace_scope(parent_scope, &opening(element)))
        .unwrap_or_else(|| parent_scope.clone());
    let unknown = original
        .as_ref()
        .map(|element| {
            element
                .children
                .iter()
                .filter(|(name, xml)| {
                    if !managed.contains(name) {
                        return true;
                    }
                    let child_scope = namespace_scope(&scope, xml);
                    let tag = tag_name(xml);
                    let prefix = tag.split_once(':').map(|(prefix, _)| prefix).unwrap_or("");
                    let attribute = if prefix.is_empty() {
                        "xmlns".into()
                    } else {
                        format!("xmlns:{prefix}")
                    };
                    !child_scope.get(&attribute).is_some_and(|uri| {
                        uri == WORD_NAMESPACE
                            || uri == "http://purl.oclc.org/ooxml/wordprocessingml/main"
                            || *name == "ligatures"
                                && uri == "http://schemas.microsoft.com/office/word/2010/wordml"
                    })
                })
                .map(|(_, xml)| preserve_scoped_child(xml, &scope))
                .collect::<String>()
        })
        .unwrap_or_default();
    let generated = element(generated)
        .map(|element| {
            element
                .children
                .iter()
                .map(|(_, xml)| *xml)
                .collect::<String>()
        })
        .unwrap_or_default();
    if generated.is_empty() && unknown.is_empty() {
        return String::new();
    }
    if let Some(original) = original {
        // Keep the original container's attributes and local namespace scope;
        // generated w: properties have their own canonical namespace binding.
        let mut attrs = attributes(&opening(&original));
        attrs.insert("xmlns:w".into(), WORD_NAMESPACE.into());
        let start = format!(
            "<{}{}>",
            tag_name(original.opening),
            attrs
                .iter()
                .map(|(key, value)| format!(" {key}=\"{}\"", xml_escape(value)))
                .collect::<String>()
        );
        format!("{start}{generated}{unknown}{}", original.closing)
    } else {
        format!("<w:{name} xmlns:w=\"{WORD_NAMESPACE}\">{generated}{unknown}</w:{name}>")
    }
}

fn generated_style(
    style: &model::Style,
    document: &Document,
    context: &ExportContext,
    original: Option<&str>,
    parent_scope: &BTreeMap<String, String>,
) -> String {
    let old = original.and_then(element);
    let scope = old
        .as_ref()
        .map(|element| namespace_scope(parent_scope, &opening(element)))
        .unwrap_or_else(|| parent_scope.clone());
    let original_property = |name: &str| {
        old.as_ref().and_then(|e| {
            e.children
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, xml)| *xml)
        })
    };
    let kind = match style.kind {
        model::StyleKind::Paragraph => "paragraph",
        model::StyleKind::Character => "character",
    };
    let mut result=old.as_ref().map(opening).unwrap_or_else(||format!("<w:style xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" w:type=\"{kind}\" w:styleId=\"{}\">",xml_escape(&style.id.0)));
    // Word identifies styles by name as well as styleId. Imported Normal and
    // the native fallback can coexist internally, but must not share a name
    // in an exported stylesheet or Word replaces the imported definition.
    let name = if style.id.0.starts_with("builtin.") && document.styles.styles.values()
        .any(|other| other.id != style.id && other.name.eq_ignore_ascii_case(&style.name))
    {
        format!("{} ({})", style.name, style.id.0)
    } else {
        style.name.clone()
    };
    result.push_str(&format!(
        "<w:name xmlns:w=\"{WORD_NAMESPACE}\" w:val=\"{}\"/>",
        xml_escape(&name)
    ));
    if let Some(base) = &style.based_on {
        result.push_str(&format!(
            "<w:basedOn xmlns:w=\"{WORD_NAMESPACE}\" w:val=\"{}\"/>",
            xml_escape(&base.0)
        ));
    }
    if let Some(next) = &style.next_style {
        result.push_str(&format!(
            "<w:next xmlns:w=\"{WORD_NAMESPACE}\" w:val=\"{}\"/>",
            xml_escape(&next.0)
        ));
    }
    let linked = document.styles.linked_styles.get(&style.id).or_else(|| {
        document.styles.linked_styles.iter().find(|(_, character)| *character == &style.id).map(|(paragraph, _)| paragraph)
    });
    if let Some(link) = linked {
        result.push_str(&format!(r#"<w:link xmlns:w="{WORD_NAMESPACE}" w:val="{}"/>"#, xml_escape(&link.0)));
    }
    if let Some(old) = &old {
        for (name, xml) in &old.children {
            if !["name", "basedOn", "next", "link", "pPr", "rPr"].contains(name) {
                result.push_str(xml);
            }
        }
    }
    let mut paragraph = Paragraph::plain("");
    paragraph.style = None;
    paragraph.properties = style.paragraph.clone();
    if let Some((list, level)) = document.styles.linked_list(&style.id) {
        paragraph.properties.list_id = Some(list.clone());
        paragraph.properties.list_level = Some(level);
    }
    let inherited_tabs = document
        .styles
        .resolve_paragraph(style.based_on.as_ref())
        .tabs
        .unwrap_or_default();
    result.push_str(&merge_properties(
        original_property("pPr"),
        &paragraph_properties_xml(&paragraph, context, &inherited_tabs),
        PARAGRAPH_PROPERTIES,
        "pPr",
        &scope,
    ));
    let linked_character = (style.kind == model::StyleKind::Character && linked.is_some())
        .then(|| document.styles.resolve_character(Some(&style.id)));
    result.push_str(&merge_properties(
        original_property("rPr"),
        &run_props_xml(linked_character.as_ref().unwrap_or(&style.character)),
        RUN_PROPERTIES,
        "rPr",
        &scope,
    ));
    result.push_str(
        old.as_ref()
            .map(|e| e.closing.as_str())
            .unwrap_or("</w:style>"),
    );
    result
}
fn table_style_xml(id: &model::StyleId, style: &model::TableStyle, original: Option<&str>) -> String {
    fn fill(color: Option<&str>) -> String {
        color.map(|color| format!(r#"<w:shd w:val="clear" w:fill="{}"/>"#,
            xml_escape(color.trim_start_matches('#')))).unwrap_or_default()
    }
    let old = original.and_then(element);
    let property = |name: &str, original: Option<&Element<'_>>, generated: String, owns: &[&str]| {
        let previous = original.and_then(|e| e.children.iter().find(|(n, _)| *n == name))
            .and_then(|(_, xml)| element(xml));
        let mut content = generated;
        if let Some(p) = previous {
            for (tag, xml) in p.children { if !owns.contains(&tag) { content.push_str(xml); } }
        }
        if content.is_empty() { String::new() } else { format!("<w:{name}>{content}</w:{name}>") }
    };
    let mut result = old.as_ref().map(opening).unwrap_or_else(|| format!(r#"<w:style xmlns:w="{WORD_NAMESPACE}" w:type="table" w:styleId="{}">"#, xml_escape(&id.0)));
    result.push_str(&format!(r#"<w:name xmlns:w="{WORD_NAMESPACE}" w:val="{}"/>"#, xml_escape(&style.name)));
    let borders = style.borders.as_ref().map(|value| super::borders_xml("w:tblBorders", value)).unwrap_or_default();
    let mut other_regions = String::new();
    let mut regions = BTreeMap::new();
    if let Some(old) = &old {
        for (name, xml) in &old.children {
            match *name {
                "name" | "tblPr" | "tcPr" => (),
                "tblStylePr" => {
                    let mut reader = Reader::from_str(xml);
                    let kind = match reader.read_event() {
                        Ok(Event::Start(e)) | Ok(Event::Empty(e)) => e.attributes().flatten()
                            .find(|a| local_name(a.key.as_ref()) == "type").map(|a| a.value.into_owned()),
                        _ => None,
                    };
                    if let Some(kind @ ("firstRow" | "band1Horz")) = kind.as_deref() {
                        regions.insert(kind.to_owned(), element(xml));
                    } else {
                        other_regions.push_str(xml);
                    }
                }
                _ => result.push_str(xml),
            }
        }
    }
    result.push_str(&property("tblPr", old.as_ref(), borders, &["tblBorders"]));
    result.push_str(&property("tcPr", old.as_ref(), fill(style.body_fill.as_deref()), &["shd"]));
    for (name, color) in [("firstRow", &style.header_fill), ("band1Horz", &style.band_fill)] {
        let previous = regions.get(name).and_then(Option::as_ref);
        let mut properties = property("tcPr", previous, fill(color.as_deref()), &["shd"]);
        if let Some(old) = previous {
            for (tag, xml) in &old.children { if *tag != "tcPr" { properties.push_str(xml); } }
        }
        if !properties.is_empty() {
            result.push_str(&format!(r#"<w:tblStylePr w:type="{name}">{properties}</w:tblStylePr>"#));
        }
    }
    result.push_str(&other_regions);
    result.push_str(old.as_ref().map(|e| e.closing.as_str()).unwrap_or("</w:style>"));
    result
}

pub(super) fn styles_xml(document: &Document, context: &ExportContext) -> String {
    let original = preserved_text(document, "word/styles.xml");
    let root = original.as_deref().and_then(element);
    let mut result=root.as_ref().map(opening).unwrap_or_else(||"<w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:w14=\"http://schemas.microsoft.com/office/word/2010/wordml\">".into());
    let scope = root
        .as_ref()
        .map(|element| namespace_scope(&BTreeMap::new(), &opening(element)))
        .unwrap_or_default();
    let mut written = BTreeSet::new();
    let mut written_tables = BTreeSet::new();
    let generated_lists = document.styles.list_styles.keys().map(super::list_style_id).collect::<BTreeSet<_>>();
    if let Some(root) = &root {
        for (name, xml) in &root.children {
            if *name != "style" {
                result.push_str(xml);
                continue;
            }
            let mut reader = Reader::from_str(xml);
            let id = match reader.read_event() {
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) => e
                    .attributes()
                    .flatten()
                    .find(|a| local_name(a.key.as_ref()) == "styleId")
                    .and_then(|a| {
                        quick_xml::escape::unescape(a.value.as_ref())
                            .ok()
                            .map(|s| s.into_owned())
                    }),
                _ => None,
            };
            if let Some(style) = id
                .as_ref()
                .and_then(|id| document.styles.styles.get(&model::StyleId(id.clone())))
            {
                result.push_str(&generated_style(
                    style,
                    document,
                    context,
                    Some(xml),
                    &scope,
                ));
                written.insert(style.id.clone());
            } else if let Some((id, style)) = id.as_deref()
                .and_then(|id| document.styles.table_styles.get_key_value(&model::StyleId(id.to_owned()))) {
                result.push_str(&table_style_xml(id, style, Some(xml)));
                written_tables.insert(id.clone());
            } else if id.as_ref().is_none_or(|id| !generated_lists.contains(id)) {
                result.push_str(xml);
            }
        }
    }
    for style in document.styles.styles.values() {
        if !written.contains(&style.id) {
            result.push_str(&generated_style(style, document, context, None, &scope));
        }
    }
    for (id, style) in &document.styles.table_styles {
        if !written_tables.contains(id) { result.push_str(&table_style_xml(id, style, None)); }
    }
    for (id, list) in &document.styles.list_styles {
        if let Some(num) = context.list_num_id(id) {
            result.push_str(&format!(r#"<w:style xmlns:w="{WORD_NAMESPACE}" w:type="numbering" w:styleId="{}"><w:name w:val="{}"/><w:pPr><w:numPr><w:numId w:val="{num}"/></w:numPr></w:pPr></w:style>"#,
                super::list_style_id(id), xml_escape(&list.name)));
        }
    }
    result.push_str(
        root.as_ref()
            .map(|e| e.closing.as_str())
            .unwrap_or("</w:styles>"),
    );
    result
}
