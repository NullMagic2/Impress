//! Screen-only All Markup text. Deleted runs participate in ordinary paragraph
//! and table pagination; their visual positions map back to live document bytes.
use model::{
    Block, Document, Inline, InlineRevisionKind, NodeId, Paragraph, RevisionEffect, RevisionStatus,
};
use render::{DrawCommand, ObjectKind, VectorDocument};
use std::{
    collections::{HashMap, HashSet},
    ops::Range,
    sync::Arc,
};

struct Deletion {
    offset: usize,
    order: u64,
    content: Vec<Inline>,
}

struct Gap {
    display: Range<usize>,
    source: usize,
}

pub(crate) struct ReviewProjection {
    pub document: Document,
    gaps: HashMap<String, Vec<Gap>>,
}

impl ReviewProjection {
    pub fn new(source: &Document) -> Self {
        if source.review.tracked_changes.is_empty()
            && source.review.revisions.is_empty()
            && source.review.history.is_empty()
        {
            return Self {
                document: source.clone(),
                gaps: HashMap::new(),
            };
        }
        let mut pending: HashMap<NodeId, Vec<Deletion>> = HashMap::new();
        let mut exact = HashSet::new();
        let mut changes: Vec<_> = source
            .review
            .tracked_changes
            .values()
            .filter(|change| change.status == RevisionStatus::Pending)
            .collect();
        changes.sort_by(|a, b| a.order.cmp(&b.order).then(a.id.cmp(&b.id)));
        for change in changes {
            for effect in &change.effects {
                if let RevisionEffect::Inline {
                    range,
                    before,
                    change: kind,
                    ..
                } = effect
                {
                    exact.insert(range.start.paragraph.clone());
                    if matches!(
                        kind,
                        InlineRevisionKind::Deletion | InlineRevisionKind::MoveFrom
                    ) {
                        pending
                            .entry(range.start.paragraph.clone())
                            .or_default()
                            .push(Deletion {
                                offset: range.start.offset,
                                order: change.order,
                                content: before.clone(),
                            });
                    }
                }
            }
        }

        // Exact Word changes supersede paragraph snapshots and journal mirrors.
        // Net snapshots use the same bounded grapheme diff as DOCX output.
        let paragraphs = model::document_paragraphs(source);
        let live: HashMap<_, _> = paragraphs.iter().map(|p| (&p.id, *p)).collect();
        let mut represented = exact;
        for revision in source.review.revisions.values() {
            if represented.insert(revision.paragraph.clone()) {
                if let Some(current) = live.get(&revision.paragraph) {
                    add_diff(
                        &mut pending,
                        &revision.paragraph,
                        &revision.before.content,
                        &current.content,
                    );
                }
            }
        }
        let mut history: HashMap<NodeId, (Vec<Inline>, Vec<Inline>)> = HashMap::new();
        for event in source
            .review
            .history
            .iter()
            .filter(|event| event.status == RevisionStatus::Pending)
        {
            let Some(operations) = event.payload.as_array() else {
                continue;
            };
            for operation in operations
                .iter()
                .filter_map(|op| op.get("ParagraphChanged"))
            {
                let Some(id) = operation
                    .get("id")
                    .and_then(|id| id.as_str())
                    .map(|id| NodeId(id.into()))
                else {
                    continue;
                };
                if represented.contains(&id) {
                    continue;
                }
                let (Some(before), Some(after)) = (
                    operation.get("before").and_then(|p| p.get("content")),
                    operation.get("after").and_then(|p| p.get("content")),
                ) else {
                    continue;
                };
                let (Ok(before), Ok(after)) = (
                    serde_json::from_value::<Vec<Inline>>(before.clone()),
                    serde_json::from_value::<Vec<Inline>>(after.clone()),
                ) else {
                    continue;
                };
                history
                    .entry(id)
                    .and_modify(|pair| pair.1 = after.clone())
                    .or_insert((before, after));
            }
        }
        for (id, (before, after)) in history {
            if let Some(current) = live
                .get(&id)
                .filter(|p| model::inline_text(&p.content) == model::inline_text(&after))
            {
                add_diff(&mut pending, &id, &before, &current.content);
            }
        }

        let mut projection = Self {
            document: source.clone(),
            gaps: HashMap::new(),
        };
        if pending.is_empty() {
            return projection;
        }
        let mut deleted_styles = DeletedStyles {
            sheet: &mut projection.document.styles,
            ids: HashMap::new(),
        };
        for section in &mut projection.document.sections {
            project_blocks(
                &mut section.blocks,
                &mut pending,
                &mut projection.gaps,
                &mut deleted_styles,
            );
            for stream in section.header_footers_mut() {
                project_blocks(
                    &mut stream.blocks,
                    &mut pending,
                    &mut projection.gaps,
                    &mut deleted_styles,
                );
            }
        }
        for note in projection.document.review.footnotes.values_mut() {
            project_blocks(
                &mut note.blocks,
                &mut pending,
                &mut projection.gaps,
                &mut deleted_styles,
            );
        }
        // PAGE/REF fields must find their anchors in the projected flow. Review
        // comments and revisions remain in semantic coordinates on the editor.
        for bookmark in projection.document.review.bookmarks.values_mut() {
            if let Some(range) = &mut bookmark.range {
                for anchor in [&mut range.start, &mut range.end] {
                    if let Some(gaps) = projection.gaps.get(&anchor.paragraph.0) {
                        anchor.offset += gaps
                            .iter()
                            .filter(|gap| gap.source <= anchor.offset)
                            .map(|gap| gap.display.len())
                            .sum::<usize>();
                    }
                }
            }
        }
        projection
    }

    pub fn remap_scene(&self, scene: &mut VectorDocument) {
        if self.gaps.is_empty() {
            return;
        }
        for page in &mut scene.pages {
            for hit in &mut page.hit_regions {
                if let Some(gaps) = self.gaps.get(&hit.node_id) {
                    hit.byte_range = source_offset(gaps, hit.byte_range.start)
                        ..source_offset(gaps, hit.byte_range.end);
                    for stop in &mut hit.caret_stops {
                        stop.byte_offset = source_offset(gaps, stop.byte_offset);
                    }
                }
            }
            for object in &mut page.object_regions {
                let source = match &mut object.kind {
                    ObjectKind::Picture { source }
                    | ObjectKind::Equation { source }
                    | ObjectKind::TextBox { source }
                    | ObjectKind::EmbeddedObject { source } => source,
                    _ => continue,
                };
                if let Some((id, offset)) = source {
                    if let Some(gaps) = self.gaps.get(id) {
                        *offset = source_offset(gaps, *offset);
                    }
                }
            }
            remap_commands(&mut page.commands, &self.gaps);
        }
    }
}

fn add_diff(
    pending: &mut HashMap<NodeId, Vec<Deletion>>,
    id: &NodeId,
    before: &[Inline],
    after: &[Inline],
) {
    for change in model::text_changes(&model::inline_text(before), &model::inline_text(after)) {
        if change.old.is_empty() {
            continue;
        }
        let Ok((_, tail)) = model::split_content(before, change.old.start) else {
            continue;
        };
        let Ok((content, _)) = model::split_content(&tail, change.old.len()) else {
            continue;
        };
        pending.entry(id.clone()).or_default().push(Deletion {
            offset: change.new.start,
            order: 0,
            content,
        });
    }
}

// Use the normal style resolver: visual_effects is derived data, deliberately
// ignored by direct-format overlays. These styles exist only in this view.
struct DeletedStyles<'a> {
    sheet: &'a mut model::StyleSheet,
    ids: HashMap<Option<model::StyleId>, model::StyleId>,
}

impl DeletedStyles<'_> {
    fn for_base(&mut self, base: Option<model::StyleId>) -> model::StyleId {
        if let Some(id) = self.ids.get(&base) {
            return id.clone();
        }
        let stem = match &base {
            Some(base) => format!("impress.screen.deleted:named:{}", base.0),
            None => "impress.screen.deleted:none".into(),
        };
        let mut id = model::StyleId::from(stem.as_str());
        let mut suffix = 0;
        while self.sheet.styles.contains_key(&id) {
            suffix += 1;
            id = model::StyleId::from(format!("{stem}:{suffix}").as_str());
        }
        let mut formatting = self.sheet.resolve_formatting(base.as_ref());
        formatting.insert("fnt_strike".into(), "true".into());
        formatting.insert("fnt_double_strike".into(), "false".into());
        self.sheet.styles.insert(
            id.clone(),
            model::Style {
                id: id.clone(),
                name: id.0.clone(),
                kind: model::StyleKind::Character,
                based_on: None,
                next_style: None,
                character: self.sheet.resolve_character(base.as_ref()),
                paragraph: Default::default(),
                formatting,
            },
        );
        self.ids.insert(base, id.clone());
        id
    }
}

fn mark_deleted(content: &mut [Inline], styles: &mut DeletedStyles<'_>) {
    for item in content {
        match item {
            Inline::Text(run) => {
                run.properties.color = Some("#CA383B".into());
                run.properties.underline = Some(false);
                run.style = Some(styles.for_base(run.style.clone()));
            }
            Inline::Hyperlink(link) => mark_deleted(&mut link.content, styles),
            _ => {}
        }
    }
}

fn project_paragraph(
    paragraph: &mut Paragraph,
    mut deletions: Vec<Deletion>,
    styles: &mut DeletedStyles<'_>,
) -> Vec<Gap> {
    deletions.sort_by_key(|deletion| (deletion.offset, deletion.order));
    let text = model::inline_text(&paragraph.content);
    let mut remaining = paragraph.content.to_vec();
    let mut content = Vec::new();
    let (mut cursor, mut added) = (0, 0);
    let mut gaps = Vec::new();
    for mut deletion in deletions {
        if deletion.offset > text.len() || !text.is_char_boundary(deletion.offset) {
            continue;
        }
        let size: usize = deletion.content.iter().map(model::inline_flat_len).sum();
        if size == 0 {
            continue;
        }
        let Ok((left, right)) = model::split_content(&remaining, deletion.offset - cursor) else {
            continue;
        };
        content.extend(left);
        mark_deleted(&mut deletion.content, styles);
        gaps.push(Gap {
            display: deletion.offset + added..deletion.offset + added + size,
            source: deletion.offset,
        });
        content.extend(deletion.content);
        remaining = right;
        cursor = deletion.offset;
        added += size;
    }
    content.extend(remaining);
    // split_content creates fresh fragment IDs. Keep view IDs stable so an
    // unrelated edit does not invalidate every prepared paragraph in All Markup.
    fn stable_ids(items: &mut [Inline], paragraph: &NodeId, index: &mut usize) {
        for item in items {
            match item {
                Inline::Text(run) => {
                    run.id = NodeId(format!("{}.review.text.{}", paragraph.0, *index));
                    *index += 1;
                }
                Inline::Hyperlink(link) => {
                    link.id = NodeId(format!("{}.review.link.{}", paragraph.0, *index));
                    *index += 1;
                    stable_ids(&mut link.content, paragraph, index);
                }
                _ => {}
            }
        }
    }
    stable_ids(&mut content, &paragraph.id, &mut 0);
    paragraph.content = content.into();
    gaps
}

fn project_blocks(
    blocks: &mut [Block],
    pending: &mut HashMap<NodeId, Vec<Deletion>>,
    gaps: &mut HashMap<String, Vec<Gap>>,
    styles: &mut DeletedStyles<'_>,
) {
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => {
                if paragraph.content.iter().any(contains_box) {
                    project_boxes(&mut paragraph.content, pending, gaps, styles);
                }
                if let Some(deletions) = pending.remove(&paragraph.id) {
                    let map = project_paragraph(paragraph, deletions, styles);
                    if !map.is_empty() {
                        gaps.insert(paragraph.id.0.clone(), map);
                    }
                }
            }
            Block::Table(table) => {
                for row in &mut table.rows {
                    for cell in &mut row.cells {
                        project_blocks(&mut cell.blocks, pending, gaps, styles);
                    }
                }
            }
            _ => {}
        }
    }
}

fn project_boxes(
    items: &mut [Inline],
    pending: &mut HashMap<NodeId, Vec<Deletion>>,
    gaps: &mut HashMap<String, Vec<Gap>>,
    styles: &mut DeletedStyles<'_>,
) {
    for item in items {
        match item {
            Inline::TextBox(boxed) => project_blocks(&mut boxed.blocks, pending, gaps, styles),
            Inline::Hyperlink(link) => project_boxes(&mut link.content, pending, gaps, styles),
            _ => {}
        }
    }
}

fn contains_box(item: &Inline) -> bool {
    match item {
        Inline::TextBox(_) => true,
        Inline::Hyperlink(link) => link.content.iter().any(contains_box),
        _ => false,
    }
}

fn source_offset(gaps: &[Gap], offset: usize) -> usize {
    let mut added = 0;
    for gap in gaps {
        if offset < gap.display.start {
            break;
        }
        if offset <= gap.display.end {
            return gap.source;
        }
        added += gap.display.len();
    }
    offset.saturating_sub(added)
}

fn remap_commands(commands: &mut [DrawCommand], maps: &HashMap<String, Vec<Gap>>) {
    for command in commands {
        match command {
            DrawCommand::Clipped { command, .. } => {
                remap_commands(std::slice::from_mut(Arc::make_mut(command)), maps)
            }
            DrawCommand::TextRun(run) => {
                let Some(gaps) = run
                    .source
                    .as_ref()
                    .and_then(|source| maps.get(&source.node_id))
                else {
                    continue;
                };
                let run = Arc::make_mut(run);
                if let Some(source) = &mut run.source {
                    source.byte_range = source_offset(gaps, source.byte_range.start)
                        ..source_offset(gaps, source.byte_range.end);
                }
                for glyph in &mut run.glyphs {
                    if let Some(offset) = &mut glyph.cluster {
                        *offset = source_offset(gaps, *offset);
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/review_projection.rs"]
mod tests;
