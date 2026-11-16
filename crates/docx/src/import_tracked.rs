//! Decode structural revision snapshots through the same semantic parsers as
//! current values. Old property trees never become an opaque XML payload.
use super::*;
use model::{RevisionEffect, TableRevisionTarget};
fn clean(node: &mut XmlNode) {
    node.children.retain(|child| {
        !child.name.ends_with("PrChange")
            && child.name != "w:tblGridChange"
            && !(matches!(node.name.as_str(), "w:trPr" | "w:tcPr")
                && matches!(
                    child.name.as_str(),
                    "w:ins" | "w:del" | "w:cellIns" | "w:cellDel" | "w:cellMerge"
                ))
    });
    for child in &mut node.children {
        clean(child);
    }
}
fn match_ids(old: &mut Table, current: &Table) {
    fn columns(table: &Table) -> Vec<BTreeMap<usize, &TableCell>> {
        let mut occupied = BTreeMap::<usize, usize>::new();
        let mut out = Vec::new();
        for (row_index, row) in table.rows.iter().enumerate() {
            let mut column = 0;
            let mut cells = BTreeMap::new();
            for cell in &row.cells {
                while occupied
                    .get(&column)
                    .is_some_and(|until| *until > row_index)
                {
                    column += 1;
                }
                cells.insert(column, cell);
                let span = usize::from(cell.column_span.max(1));
                for slot in column..column + span {
                    occupied.insert(slot, row_index + usize::from(cell.row_span.max(1)));
                }
                column += span;
            }
            out.push(cells);
        }
        out
    }
    old.id = current.id.clone();
    let current_columns = columns(current);
    let old_columns = columns(old)
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(|(column, cell)| (cell.id.clone(), column))
                .collect::<BTreeMap<_, _>>()
        })
        .collect::<Vec<_>>();
    for (index, row) in old.rows.iter_mut().enumerate() {
        if let Some(current_row) = current.rows.get(index) {
            row.id = current_row.id.clone();
            for cell in &mut row.cells {
                if let Some(current) = old_columns[index]
                    .get(&cell.id)
                    .and_then(|column| current_columns[index].get(column))
                {
                    cell.id = current.id.clone();
                    cell.blocks = current.blocks.clone();
                }
            }
        }
    }
}
fn nth_child_mut<'a>(node: &'a mut XmlNode, name: &str, index: usize) -> Option<&'a mut XmlNode> {
    fn find<'a>(node: &'a mut XmlNode, name: &str, left: &mut usize) -> Option<&'a mut XmlNode> {
        for child in &mut node.children {
            if child.name == name {
                if *left == 0 {
                    return Some(child);
                }
                *left -= 1;
            } else if matches!(
                child.name.as_str(),
                "w:sdt" | "w:sdtContent" | "w:customXml" | "w:smartTag"
            ) {
                if let Some(found) = find(child, name, left) {
                    return Some(found);
                }
            }
        }
        None
    }
    {
        let mut left = index;
        find(node, name, &mut left)
    }
}
fn previous(node: &XmlNode, current: &Table, context: &ImportContext) -> Table {
    let mut node = node.clone();
    clean(&mut node);
    let isolated = snapshot_context(context);
    let mut old = parse_table(&node, &isolated);
    match_ids(&mut old, current);
    old
}
fn replace_properties(node: &mut XmlNode, name: &str, old: &XmlNode) {
    node.children.retain(|child| child.name != name);
    node.children.insert(0, old.clone());
}
pub(super) fn table(node: &XmlNode, table: &mut Table, context: &ImportContext) {
    let all = table.clone();
    let rows = controls::children(node, "w:tr").collect::<Vec<_>>();
    let deleted = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| {
            row.child("w:trPr")
                .and_then(|pr| pr.child("w:del"))
                .is_some()
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    table.rows = table
        .rows
        .iter()
        .enumerate()
        .filter(|(index, _)| !deleted.contains(index))
        .map(|(_, row)| row.clone())
        .collect();
    // Deleted cells are retained in the revision snapshot, not in final layout.
    for (index, row_node) in rows.iter().enumerate() {
        if let Some(original) = all.rows.get(index) {
            if let Some(current) = table.rows.iter_mut().find(|row| row.id == original.id) {
                for (cell_index, cell_node) in controls::children(row_node, "w:tc").enumerate() {
                    if cell_node
                        .child("w:tcPr")
                        .and_then(|pr| pr.child("w:cellDel"))
                        .is_some()
                    {
                        if let Some(cell) = original.cells.get(cell_index) {
                            current.cells.retain(|value| value.id != cell.id);
                        }
                    }
                }
            }
        }
    }
    for (name, change_name, target) in [
        ("w:tblPr", "w:tblPrChange", TableRevisionTarget::Properties),
        ("w:tblGrid", "w:tblGridChange", TableRevisionTarget::Grid),
    ] {
        if let Some(change) = node.child(name).and_then(|pr| pr.child(change_name)) {
            let mut old_node = node.clone();
            if let Some(old) = change.child(name) {
                replace_properties(&mut old_node, name, old);
            }
            let mut old = previous(&old_node, &all, context);
            old.rows
                .retain(|row| table.rows.iter().any(|current| current.id == row.id));
            context.annotations.borrow_mut().record(
                change,
                &context.part,
                RevisionEffect::Table {
                    table: table.id.clone(),
                    target,
                    before: old,
                    after: table.clone(),
                },
            );
        }
    }
    for (index, row_node) in rows.iter().enumerate() {
        let Some(row) = all.rows.get(index) else {
            continue;
        };
        if let Some(pr) = row_node.child("w:trPr") {
            for (tag, inserted) in [("w:ins", true), ("w:del", false)] {
                if let Some(change) = pr.child(tag) {
                    let mut before = table.clone();
                    let target = if inserted {
                        before.rows.retain(|current| current.id != row.id);
                        TableRevisionTarget::InsertRow {
                            row: row.id.clone(),
                            index,
                        }
                    } else {
                        before
                            .rows
                            .insert(index.min(before.rows.len()), row.clone());
                        TableRevisionTarget::DeleteRow {
                            row: row.id.clone(),
                            index,
                        }
                    };
                    context.annotations.borrow_mut().record(
                        change,
                        &context.part,
                        RevisionEffect::Table {
                            table: table.id.clone(),
                            target,
                            before,
                            after: table.clone(),
                        },
                    );
                }
            }
            if let Some(change) = pr.child("w:trPrChange") {
                let mut old_node = node.clone();
                if let Some(old) = change.child("w:trPr") {
                    if let Some(target) = nth_child_mut(&mut old_node, "w:tr", index) {
                        replace_properties(target, "w:trPr", old);
                    }
                }
                let mut before = previous(&old_node, &all, context);
                before
                    .rows
                    .retain(|row| table.rows.iter().any(|current| current.id == row.id));
                context.annotations.borrow_mut().record(
                    change,
                    &context.part,
                    RevisionEffect::Table {
                        table: table.id.clone(),
                        target: TableRevisionTarget::RowProperties {
                            row: row.id.clone(),
                        },
                        before,
                        after: table.clone(),
                    },
                );
            }
        }
        if deleted.contains(&index) {
            continue;
        }
        for (cell_index, cell_node) in controls::children(row_node, "w:tc").enumerate() {
            if let Some(change) = cell_node
                .child("w:tcPr")
                .and_then(|pr| pr.child("w:cellMerge"))
            {
                let mut old_node = node.clone();
                if let Some(old_pr) = nth_child_mut(&mut old_node, "w:tr", index)
                    .and_then(|row| nth_child_mut(row, "w:tc", cell_index))
                    .and_then(|cell| cell.children.iter_mut().find(|n| n.name == "w:tcPr"))
                {
                    old_pr.children.retain(|n| n.name != "w:vMerge");
                    if let Some(value) = change.attr("w:vMergeOrig") {
                        old_pr.children.push(XmlNode {
                            name: "w:vMerge".into(),
                            attrs: BTreeMap::from([(
                                "w:val".into(),
                                if value == "rest" {
                                    "restart"
                                } else {
                                    "continue"
                                }
                                .into(),
                            )]),
                            ..Default::default()
                        });
                    }
                }
                let mut before = previous(&old_node, &all, context);
                before
                    .rows
                    .retain(|row| table.rows.iter().any(|current| current.id == row.id));
                let column = controls::children(row_node, "w:tc")
                    .take(cell_index)
                    .map(|cell| {
                        cell.child("w:tcPr")
                            .and_then(|pr| pr.child("w:gridSpan"))
                            .and_then(|span| span.attr("w:val"))
                            .and_then(|v| v.parse::<usize>().ok())
                            .unwrap_or(1)
                    })
                    .sum();
                context.annotations.borrow_mut().record(
                    change,
                    &context.part,
                    RevisionEffect::Table {
                        table: table.id.clone(),
                        target: TableRevisionTarget::CellMerge {
                            row: row.id.clone(),
                            column,
                            before_merge: change.attr("w:vMergeOrig").map(str::to_owned),
                            after_merge: change.attr("w:vMerge").map(str::to_owned),
                        },
                        before,
                        after: table.clone(),
                    },
                );
            }
            let Some(cell) = row.cells.get(cell_index) else {
                continue;
            };
            if let Some(pr) = cell_node.child("w:tcPr") {
                for (tag, inserted) in [("w:cellIns", true), ("w:cellDel", false)] {
                    if let Some(change) = pr.child(tag) {
                        let mut before = table.clone();
                        let Some(current_row) =
                            before.rows.iter_mut().find(|value| value.id == row.id)
                        else {
                            continue;
                        };
                        let target = if inserted {
                            current_row.cells.retain(|value| value.id != cell.id);
                            TableRevisionTarget::InsertCell {
                                row: row.id.clone(),
                                cell: cell.id.clone(),
                                index: cell_index,
                            }
                        } else {
                            current_row
                                .cells
                                .insert(cell_index.min(current_row.cells.len()), cell.clone());
                            TableRevisionTarget::DeleteCell {
                                row: row.id.clone(),
                                cell: cell.id.clone(),
                                index: cell_index,
                            }
                        };
                        context.annotations.borrow_mut().record(
                            change,
                            &context.part,
                            RevisionEffect::Table {
                                table: table.id.clone(),
                                target,
                                before,
                                after: table.clone(),
                            },
                        );
                    }
                }
            }

            if let Some(change) = cell_node
                .child("w:tcPr")
                .and_then(|pr| pr.child("w:tcPrChange"))
            {
                let mut old_node = node.clone();
                if let Some(old) = change.child("w:tcPr") {
                    if let Some(target) = nth_child_mut(&mut old_node, "w:tr", index)
                        .and_then(|row| nth_child_mut(row, "w:tc", cell_index))
                    {
                        replace_properties(target, "w:tcPr", old);
                    }
                }
                let mut before = previous(&old_node, &all, context);
                before
                    .rows
                    .retain(|row| table.rows.iter().any(|current| current.id == row.id));
                context.annotations.borrow_mut().record(
                    change,
                    &context.part,
                    RevisionEffect::Table {
                        table: table.id.clone(),
                        target: TableRevisionTarget::CellProperties {
                            cell: cell.id.clone(),
                        },
                        before,
                        after: table.clone(),
                    },
                );
            }
        }
    }
}
pub(super) fn section(node: &XmlNode, section: &Section, context: &ImportContext) {
    if let Some(change) = node.child("w:sectPrChange") {
        let mut before = Section::default();
        if let Some(old) = change.child("w:sectPr") {
            apply_section_properties(&mut before, old, context);
        }
        context.annotations.borrow_mut().record(
            change,
            &context.part,
            RevisionEffect::SectionProperties {
                section: section.id.clone(),
                before: before.properties,
                after: section.properties.clone(),
            },
        );
    }
}

pub(super) fn finish(document: &mut Document, context: &ImportContext, default: &model::StyleId) {
    let paragraphs = model::document_paragraphs(document)
        .into_iter()
        .map(|paragraph| (paragraph.id.clone(), paragraph.clone()))
        .collect::<BTreeMap<_, _>>();
    let previous_tabs = &context.annotations.borrow().previous_tabs;
    fn defaults(blocks: &mut [Block], style: &model::StyleId) {
        for block in blocks {
            match block {
                Block::Paragraph(p) => {
                    if p.style.is_none() {
                        p.style = Some(style.clone());
                    }
                }
                Block::Table(table) => {
                    for row in &mut table.rows {
                        for cell in &mut row.cells {
                            defaults(&mut cell.blocks, style);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    for change in document.review.tracked_changes.values_mut() {
        for effect in &mut change.effects {
            match effect {
                model::RevisionEffect::ParagraphProperties {
                    paragraph,
                    before,
                    after,
                } => {
                    if let Some(current) = paragraphs.get(paragraph) {
                        if before.style.is_none() {
                            before.style = Some(default.clone());
                        }
                        if let Some(edits) = previous_tabs.get(paragraph) {
                            before.properties.tabs = Some(merge_tab_edits(
                                document
                                    .styles
                                    .resolve_paragraph(before.style.as_ref())
                                    .tabs
                                    .unwrap_or_default(),
                                edits,
                            ));
                        }
                        after.style = current.style.clone();
                        after.properties = current.properties.clone();
                    }
                }
                model::RevisionEffect::Table { before, after, .. } => {
                    for table in [before, after] {
                        for row in &mut table.rows {
                            for cell in &mut row.cells {
                                defaults(&mut cell.blocks, default);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

fn block_id(block: &Block) -> &NodeId {
    match block {
        Block::Paragraph(p) => &p.id,
        Block::Table(t) => &t.id,
        Block::Equation(e) => &e.id,
        Block::Image(i) => &i.id,
        Block::PageBreak { id } | Block::ColumnBreak { id } => id,
        Block::Foreign(f) => &f.id,
    }
}
fn location(document: &Document, id: &NodeId) -> Option<(NodeId, usize, Block)> {
    fn blocks(items: &[Block], container: &NodeId, id: &NodeId) -> Option<(NodeId, usize, Block)> {
        for (index, block) in items.iter().enumerate() {
            if block_id(block) == id {
                return Some((container.clone(), index, block.clone()));
            }
            if let Block::Table(table) = block {
                for row in &table.rows {
                    for cell in &row.cells {
                        if let Some(found) = blocks(&cell.blocks, &cell.id, id) {
                            return Some(found);
                        }
                    }
                }
            }
        }
        None
    }
    for section in &document.sections {
        if let Some(found) = blocks(&section.blocks, &section.id, id) {
            return Some(found);
        }
        for story in section.header_footers() {
            if let Some(found) = blocks(&story.stream.blocks, &story.stream.id, id) {
                return Some(found);
            }
        }
    }
    for note in document.review.footnotes.values() {
        if let Some(found) = blocks(&note.blocks, &note.id, id) {
            return Some(found);
        }
    }
    None
}
pub(super) fn paragraph_boundaries(document: &mut Document, context: &ImportContext) {
    let boundaries = std::mem::take(&mut context.annotations.borrow_mut().boundaries);
    for (id, source, inserted, part) in boundaries.into_iter().rev() {
        let Some((container, index, _)) = location(document, &id) else {
            continue;
        };
        let Some(blocks) = model::document_blocks(document, &container) else {
            continue;
        };
        let (Some(Block::Paragraph(left)), Some(Block::Paragraph(right))) =
            (blocks.get(index), blocks.get(index + 1))
        else {
            continue;
        };
        let left = left.clone();
        let right = right.clone();
        let offset = model::inline_text(&left.content).len();
        let mut source = source;
        source.name = if inserted {
            "w:paragraphBoundaryIns"
        } else {
            "w:paragraphBoundaryDel"
        }
        .into();
        let id = NodeId::new();
        let change = model::TrackedChange {
            id: id.clone(),
            source_id: format!(
                "{}:{}:{}",
                part,
                source.name,
                source.attr("w:id").unwrap_or("")
            ),
            order: document.review.tracked_changes.len() as u64,
            author: source.attr("w:author").unwrap_or("Imported author").into(),
            date: source.attr("w:date").map(str::to_owned),
            status: model::RevisionStatus::Pending,
            effects: vec![RevisionEffect::ParagraphBoundary {
                container: container.clone(),
                left: left.clone(),
                right: right.clone(),
                anchor: model::TextAnchor {
                    paragraph: left.id.clone(),
                    offset,
                },
                inserted,
            }],
        };
        if !inserted {
            let mut joined = left.clone();
            joined.content.extend(right.content.iter().cloned());
            model::document_blocks_mut(document, &container)
                .unwrap()
                .splice(index..index + 2, [Block::Paragraph(joined)]);
            model::map_document_anchors(document, |anchor, _| {
                if anchor.paragraph == right.id {
                    anchor.paragraph = left.id.clone();
                    anchor.offset += offset;
                }
            });
            document.review.revisions.retain(|_, revision| {
                revision.paragraph != left.id && revision.paragraph != right.id
            });
        }
        document.review.tracked_changes.insert(id, change);
    }
}
