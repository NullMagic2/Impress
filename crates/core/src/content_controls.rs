//! Editable structured controls, constraints and XPath-backed custom XML values.
use crate::{AnchorRange, Document, NodeId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Bounds on document-provided XML/XPath before recursive parsers run.
const MAX_BOUND_XML_BYTES: usize = 16 * 1024 * 1024;
const MAX_BINDING_XPATH_BYTES: usize = 4096;
const MAX_BINDING_XPATH_DEPTH: usize = 64;

/// An item has separate displayed text and its value in the XML data store.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentControlItem {
    pub display: String,
    pub value: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContentControlKind {
    #[default]
    RichText,
    PlainText {
        multiline: bool,
    },
    DropDown {
        items: Vec<ContentControlItem>,
    },
    ComboBox {
        items: Vec<ContentControlItem>,
    },
    CheckBox {
        checked: bool,
        checked_symbol: String,
        unchecked_symbol: String,
    },
    Date {
        format: String,
        language: Option<String>,
        calendar: Option<String>,
        full_date: Option<String>,
    },
    Picture,
    Group,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentControlLock {
    pub deletion: bool,
    pub content: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentBinding {
    pub store_id: String,
    pub xpath: String,
    #[serde(default)]
    pub namespaces: BTreeMap<String, String>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContentControlScope {
    #[default]
    Inline,
    Blocks,
    Cell,
    Row,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentControl {
    pub id: NodeId,
    #[serde(default)]
    pub source_id: Option<String>,
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default)]
    pub alias: Option<String>,
    pub range: AnchorRange,
    #[serde(default)]
    pub scope: ContentControlScope,
    #[serde(default)]
    pub members: Vec<NodeId>,
    #[serde(default)]
    pub kind: ContentControlKind,
    #[serde(default)]
    pub lock: ContentControlLock,
    #[serde(default)]
    pub binding: Option<ContentBinding>,
    #[serde(default)]
    pub placeholder: bool,
    #[serde(default)]
    pub temporary: bool,
    /// Unknown adapter properties survive alongside the semantic properties.
    #[serde(default)]
    pub properties_xml: Option<String>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomXmlData {
    pub id: String,
    pub xml: String,
    #[serde(default)]
    pub original_name: Option<String>,
}
impl ContentControl {
    pub fn validate_text(&self, text: &str) -> Result<(), String> {
        match &self.kind {
            ContentControlKind::PlainText { multiline: false }
                if text.contains(['\n', '\r', '\u{000C}', '\u{000E}']) =>
            {
                Err("This content control accepts one line of plain text".into())
            }
            ContentControlKind::DropDown { items }
                if !items
                    .iter()
                    .any(|item| item.display == text || item.value == text) =>
            {
                Err("Choose one of this content control's listed values".into())
            }
            ContentControlKind::CheckBox {
                checked_symbol,
                unchecked_symbol,
                ..
            } if text != checked_symbol && text != unchecked_symbol => {
                Err("Use the check box control to change its value".into())
            }
            ContentControlKind::Date {
                format,
                full_date,
                language,
                ..
            } if crate::control_date_parts(text).is_none()
                && !full_date.as_ref().is_some_and(|date| {
                    crate::control_date_display_language(date, format, language.as_deref())
                        .as_deref()
                        == Some(text)
                }) =>
            {
                Err("Choose a valid calendar date".into())
            }
            _ => Ok(()),
        }
    }
    pub fn stored_value(&self, text: &str) -> String {
        match &self.kind {
            ContentControlKind::Date {
                format,
                full_date,
                language,
                ..
            } => full_date
                .as_ref()
                .filter(|date| {
                    crate::control_date_display_language(date, format, language.as_deref())
                        .as_deref()
                        == Some(text)
                })
                .cloned()
                .unwrap_or_else(|| text.into()),
            ContentControlKind::DropDown { items } | ContentControlKind::ComboBox { items } => {
                items
                    .iter()
                    .find(|item| item.display == text)
                    .map(|item| item.value.clone())
                    .unwrap_or_else(|| text.into())
            }
            ContentControlKind::CheckBox { checked_symbol, .. } => if text == checked_symbol {
                "true"
            } else {
                "false"
            }
            .into(),
            _ => text.into(),
        }
    }
    pub fn displayed_value(&self, value: &str) -> String {
        match &self.kind {
            ContentControlKind::Date {
                format, language, ..
            } => crate::control_date_display_language(value, format, language.as_deref())
                .unwrap_or_else(|| value.into()),
            ContentControlKind::DropDown { items } | ContentControlKind::ComboBox { items } => {
                items
                    .iter()
                    .find(|item| item.value == value)
                    .map(|item| item.display.clone())
                    .unwrap_or_else(|| value.into())
            }
            ContentControlKind::CheckBox {
                checked_symbol,
                unchecked_symbol,
                ..
            } => {
                if matches!(value, "true" | "1") {
                    checked_symbol.clone()
                } else {
                    unchecked_symbol.clone()
                }
            }
            _ => value.into(),
        }
    }
}
impl Document {
    pub fn bound_content_value(&self, binding: &ContentBinding) -> Result<String, String> {
        let store = self
            .custom_xml
            .values()
            .find(|store| normalized_store_id(&store.id) == normalized_store_id(&binding.store_id))
            .ok_or("The content control's custom XML data store is missing")?;
        binding_xml(&store.xml, binding, None).map(|(value, _)| value)
    }
    pub fn set_bound_content_value(
        &mut self,
        binding: &ContentBinding,
        value: &str,
    ) -> Result<(), String> {
        let store = self
            .custom_xml
            .values_mut()
            .find(|store| normalized_store_id(&store.id) == normalized_store_id(&binding.store_id))
            .ok_or("The content control's custom XML data store is missing")?;
        let (_, updated) = binding_xml(&store.xml, binding, Some(value))?;
        if let Some(updated) = updated {
            store.xml = updated;
        }
        Ok(())
    }
}
pub fn normalized_store_id(id: &str) -> String {
    id.trim_matches(['{', '}']).to_ascii_lowercase()
}
fn binding_xml(
    xml: &str,
    binding: &ContentBinding,
    replacement: Option<&str>,
) -> Result<(String, Option<String>), String> {
    if xml.len() > MAX_BOUND_XML_BYTES || xml.to_ascii_uppercase().contains("<!DOCTYPE") {
        return Err(
            "Custom XML data exceeds its size limit or contains a forbidden document type".into(),
        );
    }
    if binding.xpath.len() > MAX_BINDING_XPATH_BYTES {
        return Err("The content-control XPath exceeds its size limit".into());
    }
    let mut depth = 0usize;
    let mut quote = None;
    for character in binding.xpath.chars() {
        if let Some(delimiter) = quote {
            if character == delimiter {
                quote = None;
            }
            continue;
        }
        match character {
            '\'' | '"' => quote = Some(character),
            '(' | '[' => {
                depth += 1;
                if depth > MAX_BINDING_XPATH_DEPTH {
                    return Err("The content-control XPath is nested too deeply".into());
                }
            }
            ')' | ']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    // sxd-xpath expects namespace prefixes to be registered before evaluation.
    // Validate lexical QName prefixes so malformed document bindings return an
    // error instead of triggering the library's unresolved-prefix assertion.
    let characters = binding.xpath.char_indices().collect::<Vec<_>>();
    let mut quote = None;
    for (index, (byte, character)) in characters.iter().enumerate() {
        if let Some(delimiter) = quote {
            if *character == delimiter {
                quote = None;
            }
            continue;
        }
        if matches!(character, '\'' | '"') {
            quote = Some(*character);
            continue;
        }
        if *character != ':'
            || characters.get(index + 1).is_some_and(|(_, c)| *c == ':')
            || index > 0 && characters[index - 1].1 == ':'
        {
            continue;
        }
        let start = binding.xpath[..*byte]
            .rfind(|c: char| !c.is_alphanumeric() && !matches!(c, '_' | '-' | '.'))
            .map_or(0, |at| at + 1);
        let prefix = &binding.xpath[start..*byte];
        if prefix != "xml" && !binding.namespaces.contains_key(prefix) {
            return Err(format!(
                "The content-control XPath uses an undeclared namespace prefix: {prefix}"
            ));
        }
    }
    let package = sxd_document::parser::parse(xml)
        .map_err(|error| format!("Invalid custom XML data: {error}"))?;
    let document = package.as_document();
    let expression = sxd_xpath::Factory::new()
        .build(&binding.xpath)
        .map_err(|error| format!("Invalid content-control XPath: {error}"))?
        .ok_or("The content control's XPath is empty")?;
    let mut context = sxd_xpath::Context::new();
    context.set_namespace("xml", "http://www.w3.org/XML/1998/namespace");
    for (prefix, uri) in &binding.namespaces {
        context.set_namespace(prefix, uri);
    }
    let value = expression
        .evaluate(&context, document.root())
        .map_err(|error| format!("Cannot evaluate content-control XPath: {error}"))?;
    let sxd_xpath::Value::Nodeset(nodes) = value else {
        return Err("A content-control binding must select an XML node".into());
    };
    let nodes = nodes.document_order();
    if nodes.len() != 1 {
        return Err("A content-control binding must select exactly one XML node".into());
    }
    let node = nodes[0];
    let value = node.string_value();
    if let Some(replacement) = replacement {
        match node {
            sxd_xpath::nodeset::Node::Element(element) => {
                element.clear_children();
                element.append_child(document.create_text(replacement));
            }
            sxd_xpath::nodeset::Node::Text(text) => text.set_text(replacement),
            sxd_xpath::nodeset::Node::Attribute(attribute) => {
                let parent = attribute
                    .parent()
                    .ok_or("The bound XML attribute has no parent")?;
                parent.set_attribute_value(attribute.name(), replacement);
            }
            _ => return Err("This XML node cannot hold a content-control value".into()),
        }
        let mut bytes = Vec::new();
        sxd_document::writer::format_document(&document, &mut bytes)
            .map_err(|error| error.to_string())?;
        return Ok((
            value,
            Some(String::from_utf8(bytes).map_err(|error| error.to_string())?),
        ));
    }
    Ok((value, None))
}
/// Extract the live control value using semantic UTF8 anchors, across paragraphs.
pub fn content_control_text(document: &Document, range: &AnchorRange) -> Option<String> {
    let paragraphs = crate::document_paragraphs(document);
    let first = paragraphs
        .iter()
        .position(|p| p.id == range.start.paragraph)?;
    let last = paragraphs
        .iter()
        .position(|p| p.id == range.end.paragraph)?;
    if first > last {
        return None;
    }
    paragraphs[first..=last]
        .iter()
        .enumerate()
        .map(|(index, p)| {
            let text = crate::inline_text(&p.content);
            let start = if index == 0 { range.start.offset } else { 0 };
            let end = if first + index == last {
                range.end.offset
            } else {
                text.len()
            };
            text.get(start..end).map(str::to_owned)
        })
        .collect::<Option<Vec<_>>>()
        .map(|parts| parts.join("\n"))
}

impl Document {
    /// Deleted table content keeps its controls until its pending revision is resolved.
    pub fn content_control_is_dormant(&self, control: &ContentControl) -> bool {
        if crate::valid_range(self, &control.range) {
            return false;
        }
        self.review
            .tracked_changes
            .values()
            .filter(|change| change.status == crate::RevisionStatus::Pending)
            .flat_map(|change| &change.effects)
            .any(|effect| {
                if let crate::RevisionEffect::Table { before, .. } = effect {
                    let mut paragraphs = Vec::new();
                    for row in &before.rows {
                        for cell in &row.cells {
                            crate::visit_paragraphs(&cell.blocks, &mut paragraphs);
                        }
                    }
                    paragraphs
                        .iter()
                        .any(|p| p.id == control.range.start.paragraph)
                        && paragraphs
                            .iter()
                            .any(|p| p.id == control.range.end.paragraph)
                } else {
                    false
                }
            })
    }
}
