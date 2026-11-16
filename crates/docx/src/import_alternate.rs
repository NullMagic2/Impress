//! Select one namespace-supported OOXML alternate representation before import.
use super::XmlNode;
use std::collections::BTreeMap;

pub(super) const MARKUP_COMPATIBILITY_NAMESPACE: &str =
    "http://schemas.openxmlformats.org/markup-compatibility/2006";

/// Extensions whose visible content has a semantic importer. Recognizing an
/// arbitrary namespace prefix alone must never opt into a newer Word feature.
const SUPPORTED_NAMESPACES: &[&str] = &[
    "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
    "http://purl.oclc.org/ooxml/wordprocessingml/main",
    "http://schemas.openxmlformats.org/officeDocument/2006/math",
    "http://purl.oclc.org/ooxml/officeDocument/math",
    "http://schemas.openxmlformats.org/drawingml/2006/main",
    "http://purl.oclc.org/ooxml/drawingml/main",
    "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing",
    "http://purl.oclc.org/ooxml/drawingml/wordprocessingDrawing",
    "http://schemas.openxmlformats.org/drawingml/2006/picture",
    "http://purl.oclc.org/ooxml/drawingml/picture",
    "http://schemas.microsoft.com/office/word/2010/wordprocessingShape",
    "urn:schemas-microsoft-com:vml",
];

pub(super) fn requirements_supported(
    requirements: Option<&str>,
    namespaces: &BTreeMap<String, String>,
) -> bool {
    let Some(requirements) = requirements else {
        return false;
    };
    let mut prefixes = requirements.split_whitespace().peekable();
    prefixes.peek().is_some()
        && prefixes.all(|prefix| {
            namespaces
                .get(prefix)
                .is_some_and(|uri| SUPPORTED_NAMESPACES.contains(&uri.as_str()))
        })
}

pub(crate) fn selected_branch(node: &XmlNode) -> Option<&XmlNode> {
    node.children
        .iter()
        .find(|child| child.name == "mc:Choice" && child.choice_supported)
        .or_else(|| {
            node.children
                .iter()
                .find(|child| child.name == "mc:Fallback")
        })
}

/// Splice only the first understood choice, or the single fallback, into its
/// parent. Recursing after selection prevents discarded branches contributing
/// duplicate text, drawings, annotations or relationships to semantic content.
pub(super) fn resolve(node: &mut XmlNode) {
    let children = std::mem::take(&mut node.children);
    for mut child in children {
        if child.name == "mc:AlternateContent" {
            let selected = child
                .children
                .iter()
                .position(|branch| branch.name == "mc:Choice" && branch.choice_supported)
                .or_else(|| {
                    child
                        .children
                        .iter()
                        .position(|branch| branch.name == "mc:Fallback")
                });
            if let Some(index) = selected {
                let mut branch = child.children.remove(index);
                resolve(&mut branch);
                node.children.extend(branch.children);
            }
        } else {
            resolve(&mut child);
            node.children.push(child);
        }
    }
}
