//! Portable tracked changes. Before/after values are semantic model values, not
//! retained XML. The current document is the editable final view.
use crate::*;
use serde::{Deserialize, Serialize};

/// Limits prevent XML revision snapshots from exhausting editor memory.
pub const MAX_TRACKED_CHANGES: usize = 2000;
pub const MAX_TRACKED_EFFECT_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_TRACKED_HISTORY_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrackedChange {
    pub id: NodeId,
    pub author: String,
    pub date: Option<String>,
    pub source_id: String,
    #[serde(default)]
    pub order: u64,
    pub status: RevisionStatus,
    pub effects: Vec<RevisionEffect>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InlineRevisionKind {
    Insertion,
    Deletion,
    MoveFrom,
    MoveTo,
    Formatting,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum TableRevisionTarget {
    InsertCell {
        row: NodeId,
        cell: NodeId,
        index: usize,
    },
    DeleteCell {
        row: NodeId,
        cell: NodeId,
        index: usize,
    },
    CellMerge {
        row: NodeId,
        column: usize,
        before_merge: Option<String>,
        after_merge: Option<String>,
    },
    Properties,
    Grid,
    RowProperties {
        row: NodeId,
    },
    CellProperties {
        cell: NodeId,
    },
    InsertRow {
        row: NodeId,
        index: usize,
    },
    DeleteRow {
        row: NodeId,
        index: usize,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum RevisionEffect {
    ParagraphBoundary {
        container: NodeId,
        left: Paragraph,
        right: Paragraph,
        anchor: TextAnchor,
        inserted: bool,
    },
    Inline {
        range: AnchorRange,
        before: Vec<Inline>,
        after: Vec<Inline>,
        change: InlineRevisionKind,
        /// Shared name links the source and destination, even across paragraphs.
        move_name: Option<String>,
    },
    ParagraphProperties {
        paragraph: NodeId,
        before: Paragraph,
        after: Paragraph,
    },
    Table {
        table: NodeId,
        target: TableRevisionTarget,
        before: Table,
        after: Table,
    },
    SectionProperties {
        section: NodeId,
        before: SectionProperties,
        after: SectionProperties,
    },
}
impl TrackedChange {
    pub fn label(&self) -> &'static str {
        match self.effects.first() {
            Some(RevisionEffect::Inline {
                change: InlineRevisionKind::Formatting,
                ..
            }) => "Character formatting",
            Some(RevisionEffect::Inline {
                change: InlineRevisionKind::MoveFrom | InlineRevisionKind::MoveTo,
                ..
            }) => "Move text",
            Some(RevisionEffect::Inline {
                change: InlineRevisionKind::Insertion,
                ..
            }) => "Insert text",
            Some(RevisionEffect::Inline { .. }) => "Delete text",
            Some(RevisionEffect::ParagraphBoundary { .. }) => "Paragraph break",
            Some(RevisionEffect::ParagraphProperties { .. }) => "Paragraph formatting",
            Some(RevisionEffect::Table { .. }) => "Table change",
            Some(RevisionEffect::SectionProperties { .. }) => "Section formatting",
            None => "Change",
        }
    }
}

impl RevisionEffect {
    pub fn encoded_len(&self) -> Result<usize, serde_json::Error> {
        crate::revision_payload::count(self)
    }
}

pub fn block_id(block: &Block) -> &NodeId {
    match block {
        Block::Paragraph(value) => &value.id,
        Block::Table(value) => &value.id,
        Block::Equation(value) => &value.id,
        Block::Image(value) => &value.id,
        Block::PageBreak { id } | Block::ColumnBreak { id } => id,
        Block::Foreign(value) => &value.id,
    }
}
pub fn document_blocks<'a>(document: &'a Document, container: &NodeId) -> Option<&'a [Block]> {
    fn nested<'a>(blocks: &'a [Block], id: &NodeId) -> Option<&'a [Block]> {
        for block in blocks {
            if let Block::Table(table) = block {
                for row in &table.rows {
                    for cell in &row.cells {
                        if &cell.id == id {
                            return Some(&cell.blocks);
                        }
                        if let Some(found) = nested(&cell.blocks, id) {
                            return Some(found);
                        }
                    }
                }
            }
        }
        None
    }
    for section in &document.sections {
        if &section.id == container {
            return Some(&section.blocks);
        }
        if let Some(found) = nested(&section.blocks, container) {
            return Some(found);
        }
        for story in section.header_footers() {
            if &story.stream.id == container {
                return Some(&story.stream.blocks);
            }
            if let Some(found) = nested(&story.stream.blocks, container) {
                return Some(found);
            }
        }
    }
    for note in document.review.footnotes.values() {
        if &note.id == container {
            return Some(&note.blocks);
        }
        if let Some(found) = nested(&note.blocks, container) {
            return Some(found);
        }
    }
    None
}
pub fn document_blocks_mut<'a>(
    document: &'a mut Document,
    container: &NodeId,
) -> Option<&'a mut Shared<Vec<Block>>> {
    fn nested<'a>(blocks: &'a mut [Block], id: &NodeId) -> Option<&'a mut Shared<Vec<Block>>> {
        for block in blocks {
            if let Block::Table(table) = block {
                for row in &mut table.rows {
                    for cell in &mut row.cells {
                        if &cell.id == id {
                            return Some(&mut cell.blocks);
                        }
                        if let Some(found) = nested(&mut cell.blocks, id) {
                            return Some(found);
                        }
                    }
                }
            }
        }
        None
    }
    for section in &mut document.sections {
        if &section.id == container {
            return Some(&mut section.blocks);
        }
        if let Some(found) = nested(&mut section.blocks, container) {
            return Some(found);
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
            if &story.id == container {
                return Some(&mut story.blocks);
            }
            if let Some(found) = nested(&mut story.blocks, container) {
                return Some(found);
            }
        }
    }
    for note in document.review.footnotes.values_mut() {
        if &note.id == container {
            return Some(&mut note.blocks);
        }
        if let Some(found) = nested(&mut note.blocks, container) {
            return Some(found);
        }
    }
    None
}

/// Apply an identity-based paragraph mapping to every live semantic anchor.
pub fn map_document_anchors(document: &mut Document, mut map: impl FnMut(&mut TextAnchor, bool)) {
    for range in document
        .review
        .bookmarks
        .values_mut()
        .filter_map(|value| value.range.as_mut())
        .chain(
            document
                .review
                .comments
                .values_mut()
                .filter_map(|value| value.range.as_mut()),
        )
    {
        map(&mut range.start, false);
        map(&mut range.end, true);
    }
    for change in document.review.tracked_changes.values_mut() {
        if change.status != RevisionStatus::Pending {
            continue;
        }
        for effect in &mut change.effects {
            match effect {
                RevisionEffect::Inline { range, .. } => {
                    map(&mut range.start, false);
                    map(&mut range.end, true);
                }
                RevisionEffect::ParagraphBoundary { anchor, .. } => map(anchor, true),
                _ => {}
            }
        }
    }
    for control in document.content_controls.values_mut() {
        map(&mut control.range.start, false);
        map(&mut control.range.end, true);
    }
}

impl TrackedChange {
    pub fn encoded_len(&self) -> Result<usize, serde_json::Error> {
        crate::revision_payload::count(self)
    }
}
pub fn tracked_changes_bytes(
    changes: &std::collections::BTreeMap<NodeId, TrackedChange>,
) -> Result<usize, serde_json::Error> {
    let mut bytes = 0usize;
    for change in changes.values() {
        bytes = bytes.saturating_add(change.encoded_len()?);
    }
    Ok(bytes)
}
