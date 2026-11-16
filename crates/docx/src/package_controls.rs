//! Rebuild live structured controls and write their current custom XML stores.
use super::*;
use model::{ContentControl, ContentControlKind, ContentControlScope};
fn val(name: &str, value: &str) -> String {
    format!("<w:{name} w:val=\"{}\"/>", xml_escape(value))
}
pub(super) fn opening(control: &ContentControl) -> String {
    let mut props = String::new();
    if let Some(id) = &control.source_id {
        props.push_str(&val("id", id));
    }
    if let Some(alias) = &control.alias {
        props.push_str(&val("alias", alias));
    }
    if let Some(tag) = &control.tag {
        props.push_str(&val("tag", tag));
    }
    let lock = match (control.lock.deletion, control.lock.content) {
        (true, true) => Some("sdtContentLocked"),
        (true, false) => Some("sdtLocked"),
        (false, true) => Some("contentLocked"),
        _ => None,
    };
    if let Some(lock) = lock {
        props.push_str(&val("lock", lock));
    }
    if control.temporary {
        props.push_str("<w:temporary/>");
    }
    if control.placeholder {
        props.push_str("<w:showingPlcHdr/>");
    }
    if let Some(binding) = &control.binding {
        let mappings = binding
            .namespaces
            .iter()
            .map(|(prefix, uri)| format!("xmlns:{prefix}='{}'", uri.replace('\'', "&apos;")))
            .collect::<Vec<_>>()
            .join(" ");
        props.push_str(&format!(
            "<w:dataBinding w:storeItemID=\"{}\" w:xpath=\"{}\" w:prefixMappings=\"{}\"/>",
            xml_escape(&binding.store_id),
            xml_escape(&binding.xpath),
            xml_escape(&mappings)
        ));
    }
    match &control.kind {
        ContentControlKind::RichText => {}
        ContentControlKind::Group => props.push_str("<w:group/>"),
        ContentControlKind::Picture => props.push_str("<w:picture/>"),
        ContentControlKind::PlainText { multiline } => props.push_str(if *multiline {
            "<w:text w:multiLine=\"1\"/>"
        } else {
            "<w:text/>"
        }),
        ContentControlKind::DropDown { items } | ContentControlKind::ComboBox { items } => {
            let kind = if matches!(control.kind, ContentControlKind::DropDown { .. }) {
                "dropDownList"
            } else {
                "comboBox"
            };
            props.push_str(&format!(
                "<w:{kind}>{}</w:{kind}>",
                items
                    .iter()
                    .map(|item| format!(
                        "<w:listItem w:displayText=\"{}\" w:value=\"{}\"/>",
                        xml_escape(&item.display),
                        xml_escape(&item.value)
                    ))
                    .collect::<String>()
            ));
        }
        ContentControlKind::CheckBox {
            checked,
            checked_symbol,
            unchecked_symbol,
        } => {
            let checked_symbol = checked_symbol.chars().next().unwrap_or('☒') as u32;
            let unchecked_symbol = unchecked_symbol.chars().next().unwrap_or('☐') as u32;
            props.push_str(&format!("<w14:checkbox><w14:checked w14:val=\"{}\"/><w14:checkedState w14:val=\"{checked_symbol:04X}\"/><w14:uncheckedState w14:val=\"{unchecked_symbol:04X}\"/></w14:checkbox>",u8::from(*checked)));
        }
        ContentControlKind::Date {
            format,
            language,
            calendar,
            full_date,
        } => {
            props.push_str("<w:date");
            if let Some(date) = full_date {
                props.push_str(&format!(" w:fullDate=\"{}\"", xml_escape(date)));
            }
            props.push('>');
            props.push_str(&val("dateFormat", format));
            if let Some(language) = language {
                props.push_str(&val("lid", language));
            }
            if let Some(calendar) = calendar {
                props.push_str(&val("calendar", calendar));
            }
            props.push_str("</w:date>");
        }
    }
    if let Some(original) = &control.properties_xml {
        props.push_str(&unknown_properties(original));
    }
    format!("<w:sdt xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:w14=\"http://schemas.microsoft.com/office/word/2010/wordml\"><w:sdtPr>{props}</w:sdtPr><w:sdtContent>")
}
fn unknown_properties(xml: &str) -> String {
    let Some(root) = super::styles::element(xml) else {
        return String::new();
    };
    root.children
        .into_iter()
        .filter(|(name, _)| {
            ![
                "id",
                "alias",
                "tag",
                "lock",
                "showingPlcHdr",
                "temporary",
                "dataBinding",
                "text",
                "dropDownList",
                "comboBox",
                "checkbox",
                "date",
                "picture",
                "group",
            ]
            .contains(name)
        })
        .map(|(_, xml)| xml)
        .collect()
}
pub(super) fn inline_markers(
    paragraph: &Paragraph,
    document: &Document,
    map: &mut BTreeMap<usize, Vec<(u8, String)>>,
) {
    let mut controls = document
        .content_controls
        .values()
        .filter(|c| {
            c.scope == ContentControlScope::Inline
                && c.range.start.paragraph == paragraph.id
                && c.range.end.paragraph == paragraph.id
        })
        .collect::<Vec<_>>();
    // Outer starts precede inner starts; inner ends precede outer ends.
    controls.sort_by_key(|c| (c.range.start.offset, std::cmp::Reverse(c.range.end.offset)));
    controls.retain(|control| {
        !(matches!(control.kind, ContentControlKind::Picture)
            && control.range.start == control.range.end
            && !control.members.is_empty())
    });
    for control in &controls {
        map.entry(control.range.start.offset)
            .or_default()
            .push((1, opening(control)));
    }
    for control in controls.iter().rev() {
        map.entry(control.range.end.offset).or_default().push((
            if control.range.start == control.range.end {
                2
            } else {
                0
            },
            "</w:sdtContent></w:sdt>".into(),
        ));
    }
}
pub(super) fn wrap_member(
    id: &NodeId,
    scope: ContentControlScope,
    xml: String,
    document: &Document,
) -> String {
    let mut result = xml;
    for control in document.content_controls.values().filter(|c| {
        c.scope == scope && c.members.first() == Some(id) && c.members.last() == Some(id)
    }) {
        result = format!("{}{result}</w:sdtContent></w:sdt>", opening(control));
    }
    let starts = document
        .content_controls
        .values()
        .filter(|c| c.scope == scope && c.members.len() > 1 && c.members.first() == Some(id))
        .map(opening)
        .collect::<String>();
    let ends = document
        .content_controls
        .values()
        .filter(|c| c.scope == scope && c.members.len() > 1 && c.members.last() == Some(id))
        .map(|_| "</w:sdtContent></w:sdt>")
        .collect::<String>();
    format!("{starts}{result}{ends}")
}

pub(super) fn floating_picture(
    image: &model::BlockImage,
    xml: String,
    document: &Document,
) -> String {
    if let Some(control) = document.content_controls.values().find(|control| {
        matches!(control.kind, ContentControlKind::Picture)
            && control.scope == ContentControlScope::Inline
            && control.members.contains(&image.id)
    }) {
        format!("{}{xml}</w:sdtContent></w:sdt>", opening(control))
    } else {
        xml
    }
}
