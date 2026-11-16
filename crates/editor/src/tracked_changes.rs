//! Accept/reject portable Word revisions without discarding unrelated edits.
use super::*;
use model::{RevisionEffect, RevisionStatus, TableRevisionTarget};

fn table_mut<'a>(blocks: &'a mut [Block], id: &NodeId) -> Option<&'a mut model::Table> {
    for block in blocks {
        if let Block::Table(table) = block {
            if &table.id == id {
                return Some(table);
            }
            for row in &mut table.rows {
                for cell in &mut row.cells {
                    if let Some(found) = table_mut(&mut cell.blocks, id) {
                        return Some(found);
                    }
                }
            }
        }
    }
    None
}
fn document_table_mut<'a>(document: &'a mut Document, id: &NodeId) -> Option<&'a mut model::Table> {
    for section in &mut document.sections {
        if let Some(table) = table_mut(&mut section.blocks, id) {
            return Some(table);
        }
        for story in [
            section.headers.default.as_mut(),
            section.headers.first.as_mut(),
            section.headers.even.as_mut(),
            section.footers.default.as_mut(),
            section.footers.first.as_mut(),
            section.footers.even.as_mut(),
        ]
        .into_iter()
        .flatten()
        {
            if let Some(table) = table_mut(&mut story.blocks, id) {
                return Some(table);
            }
        }
    }
    for note in document.review.footnotes.values_mut() {
        if let Some(table) = table_mut(&mut note.blocks, id) {
            return Some(table);
        }
    }
    None
}
fn cell_properties(cell: &model::TableCell) -> model::TableCell {
    let mut value = cell.clone();
    value.blocks.clear();
    value
}
const CONFLICT:&str="Later edits overlap this change; rejecting it would overwrite newer work. Resolve dependent changes first.";

impl EditorState {
    pub fn resolve_tracked_change(&mut self, id: &NodeId, accept: bool) -> Result<(), String> {
        let change = self
            .document
            .review
            .tracked_changes
            .get(id)
            .cloned()
            .ok_or("Revision not found")?;
        if change.status != RevisionStatus::Pending {
            return Err("History event was already resolved".into());
        }
        let move_names = change
            .effects
            .iter()
            .filter_map(|effect| {
                if let RevisionEffect::Inline {
                    move_name: Some(name),
                    ..
                } = effect
                {
                    Some(name.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        let related=self.document.review.tracked_changes.values().filter(|candidate|candidate.status==RevisionStatus::Pending && (candidate.id==change.id || candidate.effects.iter().any(|effect|matches!(effect,RevisionEffect::Inline{move_name:Some(name),..}if move_names.contains(name))))).cloned().collect::<Vec<_>>();
        let mut effects = related
            .iter()
            .flat_map(|change| change.effects.clone())
            .collect::<Vec<_>>();
        let paragraphs = model::document_paragraphs(&self.document);
        effects.sort_by_key(|effect| match effect {
            RevisionEffect::Inline { range, .. } => (
                paragraphs
                    .iter()
                    .position(|p| p.id == range.start.paragraph)
                    .unwrap_or(0),
                range.start.offset,
                range.end.offset,
            ),
            _ => (0, 0, 0),
        });
        let mut staged = self.clone();
        let before = self.document.clone();
        let selection = self.selection.clone();
        let mut mutations = Vec::new();
        for change in &related {
            staged
                .document
                .review
                .tracked_changes
                .get_mut(&change.id)
                .unwrap()
                .status = if accept {
                RevisionStatus::Accepted
            } else {
                RevisionStatus::Rejected
            };
        }
        if !accept {
            for effect in effects.iter().rev() {
                match effect {
                    RevisionEffect::ParagraphBoundary {
                        container,
                        left,
                        right,
                        anchor,
                        inserted,
                    } => {
                        let blocks = model::document_blocks_mut(&mut staged.document, container)
                            .ok_or(CONFLICT)?;
                        let index=blocks.iter().position(|block|matches!(block,Block::Paragraph(p)if p.id==anchor.paragraph)).ok_or(CONFLICT)?;
                        let Block::Paragraph(current) = &blocks[index] else {
                            return Err(CONFLICT.into());
                        };
                        if *inserted {
                            let Some(Block::Paragraph(next)) = blocks.get(index + 1) else {
                                return Err(CONFLICT.into());
                            };
                            if next.id != right.id {
                                return Err(CONFLICT.into());
                            }
                            let offset = model::inline_text(&current.content).len();
                            let mut joined = current.clone();
                            joined.content.extend(next.content.iter().cloned());
                            blocks.splice(index..index + 2, [Block::Paragraph(joined)]);
                            model::map_document_anchors(&mut staged.document, |value, _| {
                                if value.paragraph == right.id {
                                    value.paragraph = left.id.clone();
                                    value.offset += offset;
                                }
                            });
                        } else {
                            let (first, second) =
                                model::split_content(&current.content, anchor.offset)?;
                            let mut first_p = current.clone();
                            first_p.content = first.into();
                            let mut second_p = right.clone();
                            second_p.content = second.into();
                            blocks.splice(
                                index..index + 1,
                                [Block::Paragraph(first_p), Block::Paragraph(second_p)],
                            );
                            model::map_document_anchors(&mut staged.document, |value, end| {
                                if value.paragraph == left.id
                                    && (value.offset > anchor.offset
                                        || (value.offset == anchor.offset && !end))
                                {
                                    value.paragraph = right.id.clone();
                                    value.offset = value.offset.saturating_sub(anchor.offset);
                                }
                            });
                        }
                    }
                    RevisionEffect::Inline {
                        range,
                        before,
                        after,
                        ..
                    } => {
                        if range.start.paragraph != range.end.paragraph {
                            return Err("Invalid reference range".into());
                        }
                        let old = staged
                            .paragraph(&range.start.paragraph)
                            .cloned()
                            .ok_or(CONFLICT)?;
                        let (left, tail) = model::split_content(&old.content, range.start.offset)?;
                        let (current, right) = model::split_content(
                            &tail,
                            range.end.offset.saturating_sub(range.start.offset),
                        )?;
                        if current != *after {
                            return Err(CONFLICT.into());
                        }
                        let mut new = old.clone();
                        new.content = left
                            .into_iter()
                            .chain(before.clone())
                            .chain(right)
                            .collect::<Vec<_>>()
                            .into();
                        staged.replace_paragraph(&new)?;
                        anchor_maps::AnchorMapping::Replace {
                            paragraph: old.id.clone(),
                            start: range.start.offset,
                            end: range.end.offset,
                            inserted: model::inline_text(before).len(),
                        }
                        .apply(&mut staged.document.review);
                        mutations.push(Mutation::ParagraphChanged {
                            id: old.id.clone(),
                            before: old,
                            after: new,
                        });
                    }
                    RevisionEffect::ParagraphProperties {
                        paragraph,
                        before,
                        after,
                    } => {
                        let old = staged.paragraph(paragraph).cloned().ok_or(CONFLICT)?;
                        if old.style != after.style || old.properties != after.properties {
                            return Err(CONFLICT.into());
                        }
                        let mut new = old.clone();
                        new.style = before.style.clone();
                        new.properties = before.properties.clone();
                        staged.replace_paragraph(&new)?;
                        mutations.push(Mutation::ParagraphChanged {
                            id: old.id.clone(),
                            before: old,
                            after: new,
                        });
                    }
                    RevisionEffect::SectionProperties {
                        section,
                        before,
                        after,
                    } => {
                        let target = staged
                            .document
                            .sections
                            .iter_mut()
                            .find(|value| &value.id == section)
                            .ok_or(CONFLICT)?;
                        if target.properties != *after {
                            return Err(CONFLICT.into());
                        }
                        target.properties = before.clone();
                    }
                    RevisionEffect::Table {
                        table,
                        target,
                        before,
                        after,
                    } => {
                        let current =
                            document_table_mut(&mut staged.document, table).ok_or(CONFLICT)?;
                        match target {
                            TableRevisionTarget::InsertCell { row, cell, .. } => {
                                let actual = current
                                    .rows
                                    .iter_mut()
                                    .find(|value| &value.id == row)
                                    .ok_or(CONFLICT)?;
                                let index = actual
                                    .cells
                                    .iter()
                                    .position(|value| &value.id == cell)
                                    .ok_or(CONFLICT)?;
                                let expected = after
                                    .rows
                                    .iter()
                                    .flat_map(|row| &row.cells)
                                    .find(|value| &value.id == cell)
                                    .ok_or(CONFLICT)?;
                                if actual.cells[index] != *expected {
                                    return Err(CONFLICT.into());
                                }
                                actual.cells.remove(index);
                            }
                            TableRevisionTarget::DeleteCell { row, cell, index } => {
                                let actual = current
                                    .rows
                                    .iter_mut()
                                    .find(|value| &value.id == row)
                                    .ok_or(CONFLICT)?;
                                if actual.cells.iter().any(|value| &value.id == cell) {
                                    return Err(CONFLICT.into());
                                }
                                let previous = before
                                    .rows
                                    .iter()
                                    .flat_map(|row| &row.cells)
                                    .find(|value| &value.id == cell)
                                    .ok_or(CONFLICT)?;
                                actual
                                    .cells
                                    .insert((*index).min(actual.cells.len()), previous.clone());
                            }
                            TableRevisionTarget::CellMerge { .. } => {
                                if current.rows != after.rows {
                                    return Err(CONFLICT.into());
                                }
                                current.rows = before.rows.clone();
                            }
                            TableRevisionTarget::Properties => {
                                let grid = current.properties.grid_column_widths_mm.clone();
                                let mut expected = after.properties.clone();
                                expected.grid_column_widths_mm = grid.clone();
                                if current.properties != expected {
                                    return Err(CONFLICT.into());
                                }
                                current.properties = before.properties.clone();
                                current.properties.grid_column_widths_mm = grid;
                            }
                            TableRevisionTarget::Grid => {
                                if current.properties.grid_column_widths_mm
                                    != after.properties.grid_column_widths_mm
                                {
                                    return Err(CONFLICT.into());
                                }
                                current.properties.grid_column_widths_mm =
                                    before.properties.grid_column_widths_mm.clone();
                            }
                            TableRevisionTarget::RowProperties { row } => {
                                let old = before
                                    .rows
                                    .iter()
                                    .find(|value| &value.id == row)
                                    .ok_or(CONFLICT)?;
                                let expected = after
                                    .rows
                                    .iter()
                                    .find(|value| &value.id == row)
                                    .ok_or(CONFLICT)?;
                                let actual = current
                                    .rows
                                    .iter_mut()
                                    .find(|value| &value.id == row)
                                    .ok_or(CONFLICT)?;
                                if (
                                    actual.repeat_as_header,
                                    actual.allow_break_across_pages,
                                    actual.height_mm,
                                ) != (
                                    expected.repeat_as_header,
                                    expected.allow_break_across_pages,
                                    expected.height_mm,
                                ) {
                                    return Err(CONFLICT.into());
                                }
                                actual.repeat_as_header = old.repeat_as_header;
                                actual.allow_break_across_pages = old.allow_break_across_pages;
                                actual.height_mm = old.height_mm;
                            }
                            TableRevisionTarget::CellProperties { cell } => {
                                let old = before
                                    .rows
                                    .iter()
                                    .flat_map(|row| &row.cells)
                                    .find(|value| &value.id == cell)
                                    .ok_or(CONFLICT)?;
                                let expected = after
                                    .rows
                                    .iter()
                                    .flat_map(|row| &row.cells)
                                    .find(|value| &value.id == cell)
                                    .ok_or(CONFLICT)?;
                                let actual = current
                                    .rows
                                    .iter_mut()
                                    .flat_map(|row| &mut row.cells)
                                    .find(|value| &value.id == cell)
                                    .ok_or(CONFLICT)?;
                                if cell_properties(actual) != cell_properties(expected) {
                                    return Err(CONFLICT.into());
                                }
                                let blocks = actual.blocks.clone();
                                *actual = old.clone();
                                actual.blocks = blocks;
                            }
                            TableRevisionTarget::InsertRow { row, .. } => {
                                let index = current
                                    .rows
                                    .iter()
                                    .position(|value| &value.id == row)
                                    .ok_or(CONFLICT)?;
                                let expected = after
                                    .rows
                                    .iter()
                                    .find(|value| &value.id == row)
                                    .ok_or(CONFLICT)?;
                                if current.rows[index] != *expected {
                                    return Err(CONFLICT.into());
                                }
                                current.rows.remove(index);
                            }
                            TableRevisionTarget::DeleteRow { row, index } => {
                                if current.rows.iter().any(|value| &value.id == row) {
                                    return Err(CONFLICT.into());
                                }
                                let old = before
                                    .rows
                                    .iter()
                                    .find(|value| &value.id == row)
                                    .cloned()
                                    .ok_or(CONFLICT)?;
                                current.rows.insert((*index).min(current.rows.len()), old);
                            }
                        }
                    }
                }
            }
        }
        // Exact imported records supersede the compatibility net view.
        let affected = effects
            .iter()
            .filter_map(|effect| match effect {
                RevisionEffect::Inline { range, .. } => Some(range.start.paragraph.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        staged
            .document
            .review
            .revisions
            .retain(|_, revision| !affected.contains(&revision.paragraph));
        // One snapshot covers body, table and section edits. Footnotes live in ReviewState.
        let controls_before = content_controls::ControlState::read(&before);
        let controls_after = content_controls::ControlState::read(&staged.document);
        mutations.clear();
        mutations.push(Mutation::SectionsReplaced {
            index: 0,
            before: before.sections.clone(),
            after: staged.document.sections.clone(),
        });
        mutations.push(Mutation::ReviewChanged {
            before: before.review,
            after: staged.document.review.clone(),
        });
        if controls_before != controls_after {
            mutations.push(Mutation::ContentControlsChanged {
                before: controls_before,
                after: controls_after,
            });
        }
        staged.commit(
            if accept {
                "Accept revision"
            } else {
                "Reject revision"
            },
            mutations,
            selection,
            None,
        )?;
        *self = staged;
        Ok(())
    }
}
