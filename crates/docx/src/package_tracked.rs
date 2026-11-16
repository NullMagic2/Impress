//! Standard Word revision markup generated from portable semantic snapshots.
use super::*;
use crate::import::{parse_xml_tree, XmlNode};
use model::{
    InlineRevisionKind, RevisionEffect, RevisionStatus, TableRevisionTarget, TrackedChange,
};
fn xml(node: &XmlNode) -> String {
    if node.name == "#root" {
        return node.children.iter().map(xml).collect();
    }
    let attrs = node
        .attrs
        .iter()
        .map(|(key, value)| format!(" {key}=\"{}\"", xml_escape(value)))
        .collect::<String>();
    format!(
        "<{}{attrs}>{}{}</{}>",
        node.name,
        xml_escape(&node.text),
        node.children.iter().map(xml).collect::<String>(),
        node.name
    )
}
fn parsed(xml: &str) -> Option<XmlNode> {
    parse_xml_tree(xml).ok()?.children.into_iter().next()
}
fn next_id(context: &ExportContext) -> u32 {
    let id = context.revision_serial.get();
    context.revision_serial.set(id.saturating_add(1));
    id
}
fn attributes(change: &TrackedChange, id: u32) -> String {
    format!("w:id=\"{id}\" w:author=\"{}\"{} xmlns:impress=\"urn:impress:revisions:1\" xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\" mc:Ignorable=\"impress\" impress:group=\"{}\"",xml_escape(&change.author),change.date.as_ref().map(|value|format!(" w:date=\"{}\"",xml_escape(value))).unwrap_or_default(),xml_escape(&change.id.0))
}
fn property_change(
    name: &str,
    old: XmlNode,
    change: &TrackedChange,
    context: &ExportContext,
) -> XmlNode {
    parsed(&format!(
        "<{name} {}>{}</{name}>",
        attributes(change, next_id(context)),
        xml(&old)
    ))
    .unwrap()
}
fn append_property(node: &mut XmlNode, name: &str, change: XmlNode) {
    if let Some(properties) = node.children.iter_mut().find(|child| child.name == name) {
        properties.children.push(change);
    } else {
        let index = if node.name == "w:pPr" && name == "w:rPr" {
            node.children
                .iter()
                .position(|child| matches!(child.name.as_str(), "w:sectPr" | "w:pPrChange"))
                .unwrap_or(node.children.len())
        } else {
            0
        };
        node.children.insert(
            index,
            XmlNode {
                name: name.into(),
                children: vec![change],
                ..Default::default()
            },
        );
    }
}
fn properties(xml: &str, name: &str) -> XmlNode {
    parsed(xml)
        .and_then(|node| {
            if node.name == name {
                Some(node)
            } else {
                node.child(name).cloned()
            }
        })
        .unwrap_or_else(|| XmlNode {
            name: name.into(),
            ..Default::default()
        })
}
fn format_runs(
    node: &mut XmlNode,
    old: &model::TextRun,
    change: &TrackedChange,
    context: &ExportContext,
) {
    if node.name == "w:r" {
        let mut previous = properties(&run_props_xml(&old.properties), "w:rPr");
        if let Some(style) = &old.style {
            previous.children.insert(
                0,
                parsed(&format!("<w:rStyle w:val=\"{}\"/>", xml_escape(&style.0))).unwrap(),
            );
        }
        append_property(
            node,
            "w:rPr",
            property_change("w:rPrChange", previous, change, context),
        );
        return;
    }
    for child in &mut node.children {
        format_runs(child, old, change, context);
    }
}
fn cell_at_column(row: &XmlNode, column: usize) -> Option<&XmlNode> {
    let mut start = 0;
    for cell in row.children.iter().filter(|node| node.name == "w:tc") {
        if start == column {
            return Some(cell);
        }
        start += cell
            .child("w:tcPr")
            .and_then(|pr| pr.child("w:gridSpan"))
            .and_then(|span| span.attr("w:val"))
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(1);
    }
    None
}
fn text_properties(items: &[Inline]) -> Option<&model::TextRun> {
    items.iter().find_map(|item| match item {
        Inline::Text(run) => Some(run),
        Inline::Hyperlink(link) => text_properties(&link.content),
        _ => None,
    })
}
pub(super) fn has_inline(paragraph: &NodeId, document: &Document) -> bool {
    document.review.tracked_changes.values().filter(|change|change.status==RevisionStatus::Pending).flat_map(|change|&change.effects).any(|effect|matches!(effect,RevisionEffect::Inline{range,..}if &range.start.paragraph==paragraph))
}
pub(super) fn paragraph_content(
    p: &Paragraph,
    document: &Document,
    context: &ExportContext,
    drawing: &mut u32,
    map: &mut BTreeMap<usize, Vec<(u8, String)>>,
) -> String {
    let mut events = Vec::new();
    let mut boundaries = BTreeSet::from([0, model::inline_text(&p.content).len()]);
    for change in document
        .review
        .tracked_changes
        .values()
        .filter(|change| change.status == RevisionStatus::Pending)
    {
        for effect in &change.effects {
            if let RevisionEffect::Inline { range, .. } = effect {
                if range.start.paragraph != p.id || range.end.paragraph != p.id {
                    continue;
                }
                let id = next_id(context);
                boundaries.insert(range.start.offset);
                boundaries.insert(range.end.offset);
                events.push((change, effect, id));
            }
        }
    }
    boundaries.extend(map.keys().copied());
    events.sort_by_key(|(change, _, _)| change.order);
    let mut out = String::new();
    let points = boundaries.into_iter().collect::<Vec<_>>();
    for (index, start) in points.iter().copied().enumerate() {
        // Deletions have no bytes in the final view. Their original runs remain
        // reviewable at the exact collapsed anchor, including hyperlinks/objects.
        for (change, effect, id) in &events {
            if let RevisionEffect::Inline {
                range,
                before,
                change: kind,
                ..
            } = effect
            {
                if range.start.offset == start
                    && matches!(
                        kind,
                        InlineRevisionKind::Deletion | InlineRevisionKind::MoveFrom
                    )
                {
                    review::emit_marker(start, map, &mut out);
                    if *kind == InlineRevisionKind::MoveFrom {
                        let name = if let RevisionEffect::Inline { move_name, .. } = effect {
                            move_name.as_deref().unwrap_or(&change.source_id)
                        } else {
                            &change.source_id
                        };
                        out.push_str(&format!(
                            "<w:moveFromRangeStart {} w:name=\"{}\"/>",
                            attributes(change, *id),
                            xml_escape(name)
                        ));
                    }
                    let old = review::serialize(
                        before,
                        document,
                        context,
                        drawing,
                        &mut 0,
                        &mut BTreeMap::new(),
                    )
                    .replace("<w:t ", "<w:delText ")
                    .replace("<w:t>", "<w:delText>")
                    .replace("</w:t>", "</w:delText>")
                    .replace("<w:instrText ", "<w:delInstrText ")
                    .replace("<w:instrText>", "<w:delInstrText>")
                    .replace("</w:instrText>", "</w:delInstrText>");
                    let tag = if *kind == InlineRevisionKind::MoveFrom {
                        "moveFrom"
                    } else {
                        "del"
                    };
                    out.push_str(&format!(
                        "<w:{tag} {}>{old}</w:{tag}>",
                        attributes(change, next_id(context))
                    ));
                    if *kind == InlineRevisionKind::MoveFrom {
                        out.push_str(&format!("<w:moveFromRangeEnd w:id=\"{id}\"/>"));
                    }
                }
            }
        }
        let Some(end) = points.get(index + 1).copied() else {
            break;
        };
        if start >= end {
            continue;
        }
        for (change, effect, id) in &events {
            if let RevisionEffect::Inline {
                range,
                change: InlineRevisionKind::MoveTo,
                move_name,
                ..
            } = effect
            {
                if range.start.offset == start {
                    out.push_str(&format!(
                        "<w:moveToRangeStart {} w:name=\"{}\"/>",
                        attributes(change, *id),
                        xml_escape(move_name.as_deref().unwrap_or(&change.source_id))
                    ));
                }
            }
        }
        review::emit_marker(start, map, &mut out);
        let deferred = map.remove(&end);
        let segment = review::content_slice(&p.content, start, end);
        let mut offset = start;
        let mut value = review::serialize(&segment, document, context, drawing, &mut offset, map);
        for (change, effect, _id) in &events {
            if let RevisionEffect::Inline {
                range,
                before,
                change: kind,
                ..
            } = effect
            {
                if range.start.offset > start || range.end.offset < end {
                    continue;
                }
                match kind {
                    InlineRevisionKind::Formatting => {
                        if let Some(old) = text_properties(before) {
                            if let Some(mut fragment) =
                                parsed(&format!("<fragment>{value}</fragment>"))
                            {
                                format_runs(&mut fragment, old, change, context);
                                value = fragment.children.iter().map(xml).collect();
                            }
                        }
                    }
                    InlineRevisionKind::Insertion | InlineRevisionKind::MoveTo => {
                        let tag = if *kind == InlineRevisionKind::MoveTo {
                            "moveTo"
                        } else {
                            "ins"
                        };
                        value = format!(
                            "<w:{tag} {}>{value}</w:{tag}>",
                            attributes(change, next_id(context))
                        );
                    }
                    _ => {}
                }
            }
        }
        out.push_str(&value);
        if let Some(markers) = deferred {
            map.insert(end, markers);
        }
        review::emit_marker(end, map, &mut out);
        for (_, effect, id) in &events {
            if let RevisionEffect::Inline {
                range,
                change: InlineRevisionKind::MoveTo,
                ..
            } = effect
            {
                if range.end.offset == end {
                    out.push_str(&format!("<w:moveToRangeEnd w:id=\"{id}\"/>"));
                }
            }
        }
    }
    review::emit_marker(model::inline_text(&p.content).len(), map, &mut out);
    out
}
pub(super) fn paragraph_properties(
    p: &Paragraph,
    document: &Document,
    context: &ExportContext,
    mut current: String,
) -> String {
    for change in document
        .review
        .tracked_changes
        .values()
        .filter(|change| change.status == RevisionStatus::Pending)
    {
        for effect in &change.effects {
            if let RevisionEffect::ParagraphProperties {
                paragraph, before, ..
            } = effect
            {
                if paragraph == &p.id {
                    let old = properties(
                        &paragraph_properties_xml(
                            before,
                            context,
                            &document
                                .styles
                                .resolve_paragraph(before.style.as_ref())
                                .tabs
                                .unwrap_or_default(),
                        ),
                        "w:pPr",
                    );
                    let revision = xml(&property_change("w:pPrChange", old, change, context));
                    if let Some(index) = current.rfind("</w:pPr>") {
                        current.insert_str(index, &revision);
                    } else {
                        current = format!("<w:pPr>{revision}</w:pPr>");
                    }
                }
            }
        }
    }
    for change in document
        .review
        .tracked_changes
        .values()
        .filter(|change| change.status == RevisionStatus::Pending)
    {
        for effect in &change.effects {
            if let RevisionEffect::ParagraphBoundary { left, inserted, .. } = effect {
                if left.id == p.id {
                    let marker = parsed(&format!(
                        "<w:{} {}/>",
                        if *inserted { "ins" } else { "del" },
                        attributes(change, next_id(context))
                    ))
                    .unwrap();
                    let mut tree = properties(&current, "w:pPr");
                    append_property(&mut tree, "w:rPr", marker);
                    current = xml(&tree);
                }
            }
        }
    }
    current
}
pub(super) fn section_properties(
    section: &Section,
    index: usize,
    document: &Document,
    context: &ExportContext,
) -> String {
    let mut current = section_properties_xml(section, index, context);
    for change in document
        .review
        .tracked_changes
        .values()
        .filter(|change| change.status == RevisionStatus::Pending)
    {
        for effect in &change.effects {
            if let RevisionEffect::SectionProperties {
                section: target,
                before,
                ..
            } = effect
            {
                if target == &section.id {
                    let mut previous = section.clone();
                    previous.properties = before.clone();
                    let mut old = properties(
                        &section_properties_xml(&previous, index, context),
                        "w:sectPr",
                    );
                    // PreviousSectionProperties is CT_SectPrBase: header/footer
                    // references are not legal children of a sectPrChange snapshot.
                    old.children.retain(|node| {
                        !matches!(
                            node.name.as_str(),
                            "w:headerReference" | "w:footerReference"
                        )
                    });
                    if let Some(index) = current.rfind("</w:sectPr>") {
                        current.insert_str(
                            index,
                            &xml(&property_change("w:sectPrChange", old, change, context)),
                        );
                    }
                }
            }
        }
    }
    current
}
pub(super) fn table(
    table: &Table,
    document: &Document,
    context: &ExportContext,
    drawing: &mut u32,
) -> String {
    let current = table_xml(table, document, context, drawing);
    if !document
        .review
        .tracked_changes
        .values()
        .filter(|change| change.status == RevisionStatus::Pending)
        .flat_map(|change| &change.effects)
        .any(|effect| matches!(effect,RevisionEffect::Table{table:target,..}if target==&table.id))
    {
        return current;
    }
    let Some(mut tree) = parsed(&current) else {
        return current;
    };
    let mut changes = document
        .review
        .tracked_changes
        .values()
        .filter(|change| change.status == RevisionStatus::Pending)
        .flat_map(|change| change.effects.iter().map(move |effect| (change, effect)))
        .collect::<Vec<_>>();
    changes.sort_by_key(|(_, effect)| match effect {
        RevisionEffect::Table {
            target: TableRevisionTarget::DeleteRow { .. },
            ..
        } => 2,
        RevisionEffect::Table {
            target: TableRevisionTarget::DeleteCell { .. },
            ..
        } => 1,
        _ => 0,
    });
    for (change, effect) in changes {
        let RevisionEffect::Table {
            table: target,
            target: part,
            before,
            ..
        } = effect
        else {
            continue;
        };
        if target != &table.id {
            continue;
        }
        let old_xml = table_xml(before, document, context, drawing);
        let Some(old) = parsed(&old_xml) else {
            continue;
        };
        match part {
            TableRevisionTarget::InsertCell { row, cell, .. } => {
                if let Some(row_index) = table.rows.iter().position(|value| &value.id == row) {
                    if let Some(cell_index) = table.rows[row_index]
                        .cells
                        .iter()
                        .position(|value| &value.id == cell)
                    {
                        if let Some(current) = tree
                            .children
                            .iter_mut()
                            .filter(|n| n.name == "w:tr")
                            .nth(row_index)
                            .and_then(|row| {
                                row.children
                                    .iter_mut()
                                    .filter(|n| n.name == "w:tc")
                                    .nth(cell_index)
                            })
                        {
                            append_property(
                                current,
                                "w:tcPr",
                                parsed(&format!(
                                    "<w:cellIns {}/>",
                                    attributes(change, next_id(context))
                                ))
                                .unwrap(),
                            );
                        }
                    }
                }
            }
            TableRevisionTarget::DeleteCell { row, cell, index } => {
                if let Some(row_index) = table.rows.iter().position(|value| &value.id == row) {
                    if let Some(old_row_index) =
                        before.rows.iter().position(|value| &value.id == row)
                    {
                        if let Some(old_cell_index) = before.rows[old_row_index]
                            .cells
                            .iter()
                            .position(|value| &value.id == cell)
                        {
                            if let (Some(current), Some(mut previous)) = (
                                tree.children
                                    .iter_mut()
                                    .filter(|n| n.name == "w:tr")
                                    .nth(row_index),
                                old.children
                                    .iter()
                                    .filter(|n| n.name == "w:tr")
                                    .nth(old_row_index)
                                    .and_then(|row| {
                                        row.children
                                            .iter()
                                            .filter(|n| n.name == "w:tc")
                                            .nth(old_cell_index)
                                    })
                                    .cloned(),
                            ) {
                                append_property(
                                    &mut previous,
                                    "w:tcPr",
                                    parsed(&format!(
                                        "<w:cellDel {}/>",
                                        attributes(change, next_id(context))
                                    ))
                                    .unwrap(),
                                );
                                let position = current
                                    .children
                                    .iter()
                                    .enumerate()
                                    .filter(|(_, n)| n.name == "w:tc")
                                    .nth(*index)
                                    .map(|(index, _)| index)
                                    .unwrap_or(current.children.len());
                                current.children.insert(position, previous);
                            }
                        }
                    }
                }
            }
            TableRevisionTarget::CellMerge {
                row,
                column,
                before_merge,
                after_merge,
            } => {
                if let Some(row_index) = table.rows.iter().position(|value| &value.id == row) {
                    if let Some(current) = tree
                        .children
                        .iter_mut()
                        .filter(|n| n.name == "w:tr")
                        .nth(row_index)
                    {
                        let mut start = 0;
                        for cell in current.children.iter_mut().filter(|n| n.name == "w:tc") {
                            let span = cell
                                .child("w:tcPr")
                                .and_then(|pr| pr.child("w:gridSpan"))
                                .and_then(|span| span.attr("w:val"))
                                .and_then(|value| value.parse::<usize>().ok())
                                .unwrap_or(1);
                            if start == *column {
                                if after_merge.as_deref() == Some("cont")
                                    && before_merge.as_deref() != Some("cont")
                                {
                                    if let Some(previous) = old
                                        .children
                                        .iter()
                                        .filter(|node| node.name == "w:tr")
                                        .nth(row_index)
                                        .and_then(|row| cell_at_column(row, *column))
                                    {
                                        cell.children.retain(|node| node.name == "w:tcPr");
                                        cell.children.extend(
                                            previous
                                                .children
                                                .iter()
                                                .filter(|node| node.name != "w:tcPr")
                                                .cloned(),
                                        );
                                    }
                                }
                                let mut attrs = String::new();
                                for (name, value) in
                                    [("vMergeOrig", before_merge), ("vMerge", after_merge)]
                                {
                                    if let Some(value) = value {
                                        attrs.push_str(&format!(
                                            " w:{name}=\"{}\"",
                                            xml_escape(value)
                                        ));
                                    }
                                }
                                append_property(
                                    cell,
                                    "w:tcPr",
                                    parsed(&format!(
                                        "<w:cellMerge {}{attrs}/>",
                                        attributes(change, next_id(context))
                                    ))
                                    .unwrap(),
                                );
                                break;
                            }
                            start += span;
                        }
                    }
                }
            }
            TableRevisionTarget::Properties | TableRevisionTarget::Grid => {
                let (name, revision) = if matches!(part, TableRevisionTarget::Grid) {
                    ("w:tblGrid", "w:tblGridChange")
                } else {
                    ("w:tblPr", "w:tblPrChange")
                };
                if let Some(previous) = old.child(name) {
                    append_property(
                        &mut tree,
                        name,
                        property_change(revision, previous.clone(), change, context),
                    );
                }
            }
            TableRevisionTarget::RowProperties { row } => {
                if let Some(index) = table.rows.iter().position(|value| &value.id == row) {
                    if let (Some(current), Some(previous)) = (
                        tree.children
                            .iter_mut()
                            .filter(|n| n.name == "w:tr")
                            .nth(index),
                        old.children
                            .iter()
                            .filter(|n| n.name == "w:tr")
                            .nth(index)
                            .and_then(|n| n.child("w:trPr")),
                    ) {
                        append_property(
                            current,
                            "w:trPr",
                            property_change("w:trPrChange", previous.clone(), change, context),
                        );
                    }
                }
            }
            TableRevisionTarget::CellProperties { cell } => {
                for (row_index, row) in table.rows.iter().enumerate() {
                    if let Some(cell_index) = row.cells.iter().position(|value| &value.id == cell) {
                        let current = tree
                            .children
                            .iter_mut()
                            .filter(|n| n.name == "w:tr")
                            .nth(row_index)
                            .and_then(|row| {
                                row.children
                                    .iter_mut()
                                    .filter(|n| n.name == "w:tc")
                                    .nth(cell_index)
                            });
                        let previous = old
                            .children
                            .iter()
                            .filter(|n| n.name == "w:tr")
                            .nth(row_index)
                            .and_then(|row| {
                                row.children
                                    .iter()
                                    .filter(|n| n.name == "w:tc")
                                    .nth(cell_index)
                            })
                            .and_then(|cell| cell.child("w:tcPr"));
                        if let (Some(current), Some(previous)) = (current, previous) {
                            append_property(
                                current,
                                "w:tcPr",
                                property_change("w:tcPrChange", previous.clone(), change, context),
                            );
                        }
                    }
                }
            }
            TableRevisionTarget::InsertRow { row, .. } => {
                if let Some(index) = table.rows.iter().position(|value| &value.id == row) {
                    if let Some(current) = tree
                        .children
                        .iter_mut()
                        .filter(|n| n.name == "w:tr")
                        .nth(index)
                    {
                        let marker = parsed(&format!(
                            "<w:ins {}/>",
                            attributes(change, next_id(context))
                        ))
                        .unwrap();
                        append_property(current, "w:trPr", marker);
                    }
                }
            }
            TableRevisionTarget::DeleteRow { row, index } => {
                if let Some(old_index) = before.rows.iter().position(|value| &value.id == row) {
                    if let Some(mut previous) = old
                        .children
                        .iter()
                        .filter(|n| n.name == "w:tr")
                        .nth(old_index)
                        .cloned()
                    {
                        let marker = parsed(&format!(
                            "<w:del {}/>",
                            attributes(change, next_id(context))
                        ))
                        .unwrap();
                        append_property(&mut previous, "w:trPr", marker);
                        let position = tree
                            .children
                            .iter()
                            .enumerate()
                            .filter(|(_, n)| n.name == "w:tr")
                            .nth(*index)
                            .map(|(index, _)| index)
                            .unwrap_or(tree.children.len());
                        tree.children.insert(position, previous);
                    }
                }
            }
        }
    }
    xml(&tree)
}

/// Reconstruct physical paragraphs for Word while retaining Impress's final
/// view (a deleted paragraph mark joins text in the editor).
pub(super) fn project(document: &Document) -> Option<Document> {
    let mut boundaries = document
        .review
        .tracked_changes
        .values()
        .filter(|change| change.status == RevisionStatus::Pending)
        .flat_map(|change| change.effects.iter())
        .filter_map(|effect| {
            if let RevisionEffect::ParagraphBoundary {
                container,
                right,
                anchor,
                inserted: false,
                ..
            } = effect
            {
                Some((container.clone(), right.clone(), anchor.clone()))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    if boundaries.is_empty() {
        return None;
    }
    boundaries.sort_by(|a, b| b.2.offset.cmp(&a.2.offset));
    let mut projected = document.clone();
    for (container, right, anchor) in boundaries {
        let Some(blocks) = model::document_blocks_mut(&mut projected, &container) else {
            continue;
        };
        let Some(index) = blocks
            .iter()
            .position(|block| matches!(block,Block::Paragraph(p)if p.id==anchor.paragraph))
        else {
            continue;
        };
        let Block::Paragraph(current) = &blocks[index] else {
            continue;
        };
        let Ok((first, second)) = model::split_content(&current.content, anchor.offset) else {
            continue;
        };
        let mut left = current.clone();
        left.content = first.into();
        let mut next = right.clone();
        next.content = second.into();
        blocks.splice(
            index..index + 1,
            [Block::Paragraph(left), Block::Paragraph(next)],
        );
        model::map_document_anchors(&mut projected, |value, end| {
            if value.paragraph == anchor.paragraph
                && (value.offset > anchor.offset || (value.offset == anchor.offset && !end))
            {
                value.paragraph = right.id.clone();
                value.offset = value.offset.saturating_sub(anchor.offset);
            }
        });
    }
    Some(projected)
}
