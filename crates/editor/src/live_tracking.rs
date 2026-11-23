//! Record independently reviewable text and formatting changes. Transactions
//! without a portable representation use the native revision journal.
use super::*;
use model::{
    AnchorRange, InlineRevisionKind, RevisionEffect, RevisionStatus, TextAnchor, TrackedChange,
};

const LOCAL_SOURCE: &str = "impress:";

impl EditorState {
    pub fn set_track_changes(&mut self, enabled: bool, author: &str) -> Result<(), String> {
        let mut review = self.document.review.clone();
        review.track_changes = enabled;
        review.track_history = false;
        review.track_text = false;
        if enabled {
            let author = author.trim();
            review.author = if author.is_empty() {
                "Author".into()
            } else {
                author.chars().take(100).collect()
            };
        }
        if review == self.document.review {
            return Ok(());
        }
        self.commit_review("Track changes", review)
    }

    pub(super) fn record_document_revision(
        &mut self,
        label: &str,
        mutations: &mut Vec<Mutation>,
    ) -> Result<(), String> {
        if label.starts_with("Native history")
            || matches!(
                label,
                "Track changes"
                    | "Track paragraph text"
                    | "Accept revision"
                    | "Reject revision"
                    | "Accept text revision"
                    | "Reject text revision"
                    | "Accept text change"
                    | "Reject text change"
            )
        {
            return Ok(());
        }
        if self.document.review.track_history {
            return self.record_native_revision(label, mutations);
        }
        if !self.document.review.track_changes {
            return Ok(());
        }

        let mut effects = Vec::new();
        let changes = mutations
            .iter()
            .filter(|mutation| !matches!(mutation, Mutation::ReviewChanged { .. }))
            .collect::<Vec<_>>();
        if let Some(effect) = paragraph_boundary(&changes) {
            effects.push(effect);
        } else {
            let mut text_range = None;
            for mutation in changes {
                match mutation {
                    Mutation::AnchorsTransformed {
                        mapping:
                            anchor_maps::AnchorMapping::Replace {
                                start,
                                end,
                                inserted,
                                ..
                            },
                        suppress: 1,
                    } => {
                        text_range = Some(model::TextChange {
                            old: *start..*end,
                            new: *start..start + inserted,
                        })
                    }
                    Mutation::AnchorsTransformed { .. } => {
                        return self.record_native_revision(label, mutations);
                    }
                    Mutation::ParagraphChanged { before, after, .. } => {
                        if !paragraph_effects(before, after, text_range.take(), &mut effects)? {
                            return self.record_native_revision(label, mutations);
                        }
                    }
                    Mutation::SectionPropertiesChanged {
                        section_id,
                        before,
                        after,
                    } => {
                        effects.push(RevisionEffect::SectionProperties {
                            section: section_id.clone(),
                            before: before.clone(),
                            after: after.clone(),
                        });
                    }
                    Mutation::ContentControlsChanged { .. } => {}
                    _ => return self.record_native_revision(label, mutations),
                }
            }
        }
        if effects.is_empty() {
            return Ok(());
        }
        for effect in &effects {
            if effect.encoded_len().map_err(|error| error.to_string())?
                > model::MAX_TRACKED_EFFECT_BYTES
            {
                return self.record_native_revision(label, mutations);
            }
        }

        // An edit crossing another pending revision can require a different
        // rejection order. Keep it in the native journal so its full inverse
        // is available even when Word ranges would overlap.
        let current = &self.document.review;
        let earlier = mutations
            .iter()
            .find_map(|mutation| match mutation {
                Mutation::ReviewChanged { before, .. } => Some(before),
                _ => None,
            })
            .unwrap_or(current);
        let mut review = current.clone();
        let consecutive_typing =
            label == "Typing" && self.undo.last().is_some_and(|last| last.label == "Typing");
        if effects.len() == 1
            && self.adjust_local_insertion(earlier, &mut review, &effects[0], consecutive_typing)?
        {
            self.commit_live_review(review, mutations);
            return Ok(());
        }
        if effects
            .iter()
            .any(|effect| overlaps_live_revision(&review, effect))
        {
            return self.record_native_revision(label, mutations);
        }

        let mut order = review
            .tracked_changes
            .values()
            .map(|change| change.order)
            .max()
            .unwrap_or(0);
        let author = if review.author.is_empty() {
            "Author"
        } else {
            &review.author
        };
        let date = model::utc_timestamp();
        for effect in effects {
            order = order.saturating_add(1);
            let id = NodeId::new();
            review.tracked_changes.insert(
                id.clone(),
                TrackedChange {
                    source_id: format!("{LOCAL_SOURCE}{id}"),
                    id,
                    author: author.to_owned(),
                    date: Some(date.clone()),
                    order,
                    status: RevisionStatus::Pending,
                    effects: vec![effect],
                },
            );
        }
        if review.tracked_changes.len() > model::MAX_TRACKED_CHANGES
            || model::tracked_changes_bytes(&review.tracked_changes)
                .map_err(|error| error.to_string())?
                > model::MAX_TRACKED_HISTORY_BYTES
        {
            return Err("Tracked changes exceed the supported history size limit".into());
        }
        self.commit_live_review(review, mutations);
        Ok(())
    }

    fn commit_live_review(&mut self, after: model::ReviewState, mutations: &mut Vec<Mutation>) {
        let before = std::mem::replace(&mut self.document.review, after.clone());
        mutations.push(Mutation::ReviewChanged { before, after });
    }
}

fn paragraph_boundary(changes: &[&Mutation]) -> Option<RevisionEffect> {
    use anchor_maps::AnchorMapping::{Join, Split};
    let [
        Mutation::AnchorsTransformed { mapping, .. },
        Mutation::ParagraphChanged { before, after, .. },
        block,
    ] = changes
    else {
        return None;
    };
    let (left, right, at, container, first, second, inserted) = match (mapping, block) {
        (
            Split { left, right, at },
            Mutation::BlockInserted {
                section_id,
                block: Block::Paragraph(paragraph),
                ..
            },
        ) => (left, right, at, section_id, after, paragraph, true),
        (
            Join { left, right, at },
            Mutation::BlockRemoved {
                section_id,
                block: Block::Paragraph(paragraph),
                ..
            },
        ) => (left, right, at, section_id, before, paragraph, false),
        _ => return None,
    };
    if left != &first.id || right != &second.id {
        return None;
    }
    Some(RevisionEffect::ParagraphBoundary {
        container: container.clone(),
        left: first.clone(),
        right: second.clone(),
        anchor: TextAnchor {
            paragraph: left.clone(),
            offset: *at,
        },
        inserted,
    })
}

fn span(paragraph: &NodeId, start: usize, end: usize) -> AnchorRange {
    AnchorRange {
        start: TextAnchor {
            paragraph: paragraph.clone(),
            offset: start,
        },
        end: TextAnchor {
            paragraph: paragraph.clone(),
            offset: end,
        },
    }
}

fn inline_text_only(content: &[Inline]) -> bool {
    content.iter().all(|inline| match inline {
        Inline::Text(_) | Inline::Tab { .. } | Inline::LineBreak { .. } => true,
        Inline::Hyperlink(link) => inline_text_only(&link.content),
        _ => false,
    })
}

fn paragraph_effects(
    before: &Paragraph,
    after: &Paragraph,
    exact: Option<model::TextChange>,
    effects: &mut Vec<RevisionEffect>,
) -> Result<bool, String> {
    let old = model::inline_text(&before.content);
    let new = model::inline_text(&after.content);
    if old != new {
        let changes = exact.map_or_else(|| model::text_changes(&old, &new), |change| vec![change]);
        for change in changes {
            let removed = slice_content(&before.content, change.old.start, change.old.end)?;
            let inserted = slice_content(&after.content, change.new.start, change.new.end)?;
            // Large tracked fragments and embedded objects retain their
            // complete transaction in the native history.
            if !inline_text_only(&removed) || !inline_text_only(&inserted) {
                return Ok(false);
            }
            if !removed.is_empty() {
                effects.push(RevisionEffect::Inline {
                    range: span(&before.id, change.new.start, change.new.start),
                    before: removed,
                    after: Vec::new(),
                    change: InlineRevisionKind::Deletion,
                    move_name: None,
                });
            }
            if !inserted.is_empty() {
                effects.push(RevisionEffect::Inline {
                    range: span(&before.id, change.new.start, change.new.end),
                    before: Vec::new(),
                    after: inserted,
                    change: InlineRevisionKind::Insertion,
                    move_name: None,
                });
            }
        }
    } else if !super::tracked_changes::same_revision_content(&before.content, &after.content) {
        if !inline_text_only(&before.content) || !inline_text_only(&after.content) {
            return Ok(false);
        }
        for range in formatting_ranges(&before.content, &after.content, new.len()) {
            effects.push(RevisionEffect::Inline {
                range: span(&before.id, range.start, range.end),
                before: slice_content(&before.content, range.start, range.end)?,
                after: slice_content(&after.content, range.start, range.end)?,
                change: InlineRevisionKind::Formatting,
                move_name: None,
            });
        }
    }
    if before.properties != after.properties || before.style != after.style {
        effects.push(RevisionEffect::ParagraphProperties {
            paragraph: before.id.clone(),
            before: before.clone(),
            after: after.clone(),
        });
    }
    Ok(true)
}

fn formatting_ranges(
    before: &[Inline],
    after: &[Inline],
    len: usize,
) -> Vec<std::ops::Range<usize>> {
    fn runs(content: &[Inline]) -> impl Iterator<Item = (usize, &TextRun)> {
        content
            .iter()
            .filter_map(|item| match item {
                Inline::Text(run) if !run.text.is_empty() => Some(run),
                _ => None,
            })
            .scan(0, |end, run| {
                *end += run.text.len();
                Some((*end, run))
            })
    }

    if before
        .iter()
        .chain(after)
        .any(|item| !matches!(item, Inline::Text(_)))
    {
        return std::iter::once(0..len).collect();
    }
    let mut left = runs(before).peekable();
    let mut right = runs(after).peekable();
    let mut start = 0;
    let mut ranges: Vec<std::ops::Range<usize>> = Vec::new();
    while let (Some(&(left_end, old)), Some(&(right_end, new))) = (left.peek(), right.peek()) {
        let end = left_end.min(right_end);
        if old.properties != new.properties || old.style != new.style {
            if let Some(previous) = ranges.last_mut().filter(|range| range.end == start) {
                previous.end = end;
            } else {
                ranges.push(start..end);
            }
        }
        start = end;
        if left_end == end {
            left.next();
        }
        if right_end == end {
            right.next();
        }
    }
    ranges
}

fn overlaps_live_revision(review: &model::ReviewState, effect: &RevisionEffect) -> bool {
    review
        .tracked_changes
        .values()
        .filter(|change| change.status == RevisionStatus::Pending)
        .flat_map(|change| &change.effects)
        .any(|pending| match (pending, effect) {
            (
                RevisionEffect::Inline { range: left, .. },
                RevisionEffect::Inline { range: right, .. },
            ) => {
                left.start.paragraph == right.start.paragraph
                    && left.start.offset < right.end.offset
                    && right.start.offset < left.end.offset
            }
            (
                RevisionEffect::ParagraphProperties {
                    paragraph: left, ..
                },
                RevisionEffect::ParagraphProperties {
                    paragraph: right, ..
                },
            ) => left == right,
            (
                RevisionEffect::SectionProperties { section: left, .. },
                RevisionEffect::SectionProperties { section: right, .. },
            ) => left == right,
            _ => false,
        })
}

/// Extend a just-written insertion or trim it when the writer backspaces in
/// their own pending text. The logical revision survives as one edit.
impl EditorState {
    fn adjust_local_insertion(
        &self,
        earlier: &model::ReviewState,
        review: &mut model::ReviewState,
        effect: &RevisionEffect,
        consecutive_typing: bool,
    ) -> Result<bool, String> {
        let RevisionEffect::Inline {
            range,
            before,
            change: kind,
            ..
        } = effect
        else {
            return Ok(false);
        };
        let at = range.start.offset;
        let removed = model::inline_text(before).len();
        let author = if review.author.is_empty() {
            "Author"
        } else {
            &review.author
        };
        let candidates = earlier.tracked_changes.values().filter(|change| {
            change.status == RevisionStatus::Pending
                && change.author == author
                && change.source_id.starts_with(LOCAL_SOURCE)
        });
        for revision in candidates {
            let [
                RevisionEffect::Inline {
                    range: old,
                    change: InlineRevisionKind::Insertion,
                    ..
                },
            ] = revision.effects.as_slice()
            else {
                continue;
            };
            if old.start.paragraph != range.start.paragraph
                || old.end.paragraph != range.start.paragraph
            {
                continue;
            }
            let end = match kind {
                InlineRevisionKind::Insertion
                    if consecutive_typing && at == old.end.offset && range.end.offset > at =>
                {
                    range.end.offset
                }
                InlineRevisionKind::Deletion
                    if at >= old.start.offset && at.saturating_add(removed) <= old.end.offset =>
                {
                    old.end.offset - removed
                }
                _ => continue,
            };
            let Some(paragraph) = self.paragraph(&old.start.paragraph) else {
                return Ok(false);
            };
            let after = slice_content(&paragraph.content, old.start.offset, end)?;
            if after.is_empty() {
                review.tracked_changes.remove(&revision.id);
            } else {
                let replacement = RevisionEffect::Inline {
                    range: span(&paragraph.id, old.start.offset, end),
                    before: Vec::new(),
                    after,
                    change: InlineRevisionKind::Insertion,
                    move_name: None,
                };
                if replacement
                    .encoded_len()
                    .map_err(|error| error.to_string())?
                    > model::MAX_TRACKED_EFFECT_BYTES
                {
                    return Ok(false);
                }
                let Some(next) = review.tracked_changes.get_mut(&revision.id) else {
                    return Ok(false);
                };
                next.effects = vec![replacement];
            }
            return Ok(true);
        }
        Ok(false)
    }
}
