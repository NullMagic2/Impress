//! Review ribbon and nonmodal pane. The editor remains the owner of every
//! revision decision and its undo record.
use super::*;
use crate::i18n::text;
use model::{
    AnchorRange, InlineRevisionKind, RevisionEffect, RevisionStatus, TableRevisionTarget,
    TextAnchor,
};
use std::{
    collections::HashSet,
    hash::{DefaultHasher, Hash, Hasher},
};

const MARKUP_FIRST: i32 = 32_710;
const MARKUP_MODE_FIRST: i32 = 32_714;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Markup {
    #[default]
    All,
    Simple,
    None,
}

impl Markup {
    const MODES: [(Self, &'static str); 3] = [
        (Self::All, "All markup"),
        (Self::Simple, "Simple markup"),
        (Self::None, "No markup"),
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Category {
    Text,
    Formatting,
    Structure,
}

impl Category {
    const MENU: [(Self, &'static str); 3] = [
        (Self::Text, "Text changes"),
        (Self::Formatting, "Formatting"),
        (Self::Structure, "Structural changes"),
    ];

    fn bit(self) -> u8 {
        1 << self as u8
    }

    fn color(self, deletion: bool) -> [f32; 3] {
        match self {
            Self::Text if deletion => [0.79, 0.22, 0.23],
            Self::Text => [0.13, 0.55, 0.36],
            Self::Formatting => [0.24, 0.40, 0.77],
            Self::Structure => [0.63, 0.36, 0.12],
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum ChangeId {
    Word(NodeId),
    Paragraph(NodeId),
    History(NodeId),
}

impl ChangeId {
    fn resolve(&self, editor: &mut EditorState, accept: bool) -> Result<(), String> {
        match self {
            Self::Word(id) => editor.resolve_tracked_change(id, accept),
            Self::Paragraph(id) => editor.resolve_revision(id, accept),
            Self::History(id) => editor.resolve_history_event(id, accept),
        }
    }
}

#[derive(Clone)]
struct Mark {
    range: AnchorRange,
    category: Category,
    deletion: bool,
}

struct Entry {
    id: ChangeId,
    label: String,
    author: String,
    date: Option<String>,
    ranges: Vec<Mark>,
    order: u64,
}

impl Entry {
    fn caption(&self) -> String {
        if self.author.is_empty() {
            text(&self.label)
        } else {
            format!("{} · {}", text(&self.label), self.author)
        }
    }
}

struct ActiveChange {
    id: ChangeId,
    selection: Option<Selection>,
}

pub(crate) struct ReviewView {
    revision: Option<u64>,
    epoch: u64,
    order: HashMap<NodeId, usize>,
    entries: Vec<Entry>,
    markers: HashMap<String, Vec<(std::ops::Range<usize>, Category, bool)>>,
    history_marks: HashMap<NodeId, (model::RevisionPayload, Option<Vec<JournalMark>>)>,
    active: Option<ActiveChange>,
    markup: Markup,
    hidden: u8,
}

impl Default for ReviewView {
    fn default() -> Self {
        Self {
            revision: None,
            epoch: 0,
            order: HashMap::new(),
            entries: Vec::new(),
            markers: HashMap::new(),
            history_marks: HashMap::new(),
            active: None,
            markup: Markup::All,
            hidden: 0,
        }
    }
}

fn point(id: &NodeId, offset: usize) -> AnchorRange {
    let anchor = TextAnchor {
        paragraph: id.clone(),
        offset,
    };
    AnchorRange {
        start: anchor.clone(),
        end: anchor,
    }
}

fn paragraph_mark(id: &NodeId, category: Category, deletion: bool) -> Mark {
    Mark {
        range: point(id, 0),
        category,
        deletion,
    }
}

fn table_paragraph(table: &model::Table, target: &TableRevisionTarget) -> Option<NodeId> {
    let row_id = match target {
        TableRevisionTarget::InsertCell { row, .. }
        | TableRevisionTarget::DeleteCell { row, .. }
        | TableRevisionTarget::CellMerge { row, .. }
        | TableRevisionTarget::RowProperties { row }
        | TableRevisionTarget::InsertRow { row, .. }
        | TableRevisionTarget::DeleteRow { row, .. } => Some(row),
        _ => None,
    };
    let rows = row_id
        .and_then(|id| table.rows.iter().find(|row| &row.id == id))
        .map(std::slice::from_ref)
        .unwrap_or(&table.rows);
    let cell_id = match target {
        TableRevisionTarget::InsertCell { cell, .. }
        | TableRevisionTarget::DeleteCell { cell, .. }
        | TableRevisionTarget::CellProperties { cell } => Some(cell),
        _ => None,
    };
    let all_cells = rows.iter().flat_map(|row| &row.cells);
    let cells = cell_id.and_then(|id| all_cells.clone().find(|cell| &cell.id == id));
    let mut paragraphs = Vec::new();
    if let Some(cell) = cells {
        model::visit_paragraphs(&cell.blocks, &mut paragraphs);
    } else {
        for cell in all_cells {
            model::visit_paragraphs(&cell.blocks, &mut paragraphs);
            if !paragraphs.is_empty() {
                break;
            }
        }
    }
    paragraphs.first().map(|paragraph| paragraph.id.clone())
}

fn word_mark(effect: &RevisionEffect, document: &Document) -> Option<Mark> {
    let mark = match effect {
        RevisionEffect::Inline { range, change, .. } => Mark {
            range: range.clone(),
            category: if *change == InlineRevisionKind::Formatting {
                Category::Formatting
            } else {
                Category::Text
            },
            deletion: matches!(
                change,
                InlineRevisionKind::Deletion | InlineRevisionKind::MoveFrom
            ),
        },
        RevisionEffect::ParagraphProperties { paragraph, .. } => {
            paragraph_mark(paragraph, Category::Formatting, false)
        }
        RevisionEffect::ParagraphBoundary { anchor, .. } => Mark {
            range: point(&anchor.paragraph, anchor.offset),
            category: Category::Structure,
            deletion: false,
        },
        RevisionEffect::SectionProperties { section, .. } => {
            let section = document.sections.iter().find(|item| &item.id == section)?;
            let mut paragraphs = Vec::new();
            model::visit_paragraphs(&section.blocks, &mut paragraphs);
            paragraph_mark(&paragraphs.first()?.id, Category::Formatting, false)
        }
        RevisionEffect::Table {
            target,
            after,
            before,
            ..
        } => {
            let paragraph =
                table_paragraph(after, target).or_else(|| table_paragraph(before, target))?;
            paragraph_mark(&paragraph, Category::Structure, false)
        }
    };
    Some(mark)
}

#[derive(Clone)]
struct JournalMark {
    mark: Mark,
    final_text_hash: u64,
}

fn text_hash(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

fn history_marks(event: &model::RevisionEvent) -> Option<Vec<JournalMark>> {
    let operations = event.payload.as_array()?;
    // A comment, reply or bookmark is review metadata. The history menu can
    // still show its journal entry, but the document ribbon reviews edits.
    if !operations
        .iter()
        .any(|op| op.get("ReviewChanged").is_none())
    {
        return None;
    }
    let mut marks = Vec::new();
    for change in operations
        .iter()
        .filter_map(|op| op.get("ParagraphChanged"))
    {
        let Some(id) = change.get("id").and_then(|id| id.as_str()) else {
            continue;
        };
        let Some(before) = change.get("before").and_then(|value| value.get("content")) else {
            continue;
        };
        let Some(after) = change.get("after").and_then(|value| value.get("content")) else {
            continue;
        };
        let (Ok(before), Ok(after)) = (
            serde_json::from_value::<Vec<model::Inline>>(before.clone()),
            serde_json::from_value::<Vec<model::Inline>>(after.clone()),
        ) else {
            continue;
        };
        let before = model::inline_text(&before);
        let after = model::inline_text(&after);
        let id = NodeId(id.to_owned());
        let mark = if before == after {
            paragraph_mark(&id, Category::Formatting, false)
        } else {
            text_mark(&id, &before, &after)
        };
        marks.push(JournalMark {
            mark,
            final_text_hash: text_hash(&after),
        });
    }
    Some(marks)
}

fn text_mark(id: &NodeId, before: &str, after: &str) -> Mark {
    let prefix: usize = before
        .chars()
        .zip(after.chars())
        .take_while(|(a, b)| a == b)
        .map(|(ch, _)| ch.len_utf8())
        .sum();
    let suffix: usize = before[prefix..]
        .chars()
        .rev()
        .zip(after[prefix..].chars().rev())
        .take_while(|(a, b)| a == b)
        .map(|(ch, _)| ch.len_utf8())
        .sum();
    let end = after.len() - suffix;
    Mark {
        range: AnchorRange {
            start: TextAnchor {
                paragraph: id.clone(),
                offset: prefix,
            },
            end: TextAnchor {
                paragraph: id.clone(),
                offset: end,
            },
        },
        category: Category::Text,
        deletion: end == prefix,
    }
}

impl ReviewView {
    fn refresh(&mut self, editor: &EditorState, epoch: u64) {
        if self.revision == Some(editor.revision) && self.epoch == epoch {
            return;
        }
        self.revision = Some(editor.revision);
        self.epoch = epoch;
        self.entries.clear();
        self.markers.clear();
        self.order.clear();
        let document = &editor.document;
        let review = &document.review;
        if review.tracked_changes.is_empty()
            && review.revisions.is_empty()
            && review.history.is_empty()
        {
            self.active = None;
            return;
        }
        let paragraphs = model::document_paragraphs(document);
        let lookup: HashMap<_, _> = paragraphs
            .iter()
            .enumerate()
            .map(|(index, p)| (&p.id, index))
            .collect();
        let mut exact_text_paragraphs = HashSet::new();
        for change in review
            .tracked_changes
            .values()
            .filter(|change| change.status == RevisionStatus::Pending)
        {
            for effect in &change.effects {
                if let RevisionEffect::Inline { range, .. } = effect {
                    exact_text_paragraphs.insert(&range.start.paragraph);
                }
            }
            self.entries.push(Entry {
                id: ChangeId::Word(change.id.clone()),
                label: change.label().into(),
                author: change.author.clone(),
                date: change.date.clone(),
                order: change.order,
                ranges: change
                    .effects
                    .iter()
                    .filter_map(|effect| word_mark(effect, document))
                    .collect(),
            });
        }
        for change in review.revisions.values() {
            // DOCX imports also keep a compatibility snapshot of the whole
            // paragraph. The individual Word changes already cover that text.
            if exact_text_paragraphs.contains(&change.paragraph) {
                continue;
            }

            let range = lookup.get(&change.paragraph).map(|&index| {
                text_mark(
                    &change.paragraph,
                    &model::inline_text(&change.before.content),
                    &model::inline_text(&paragraphs[index].content),
                )
            });
            self.entries.push(Entry {
                id: ChangeId::Paragraph(change.id.clone()),
                label: "Text changes".into(),
                author: change.author.clone(),
                date: change.date.clone(),
                order: 0,
                ranges: range.into_iter().collect(),
            });
        }
        let mut cached = std::mem::take(&mut self.history_marks);
        let mut current_hashes = HashMap::new();
        for (index, event) in review
            .history
            .iter()
            .enumerate()
            .filter(|(_, event)| event.status == RevisionStatus::Pending)
        {
            let data = cached
                .remove(&event.id)
                .filter(|(payload, _)| payload.shares_storage(&event.payload))
                .unwrap_or_else(|| (event.payload.clone(), history_marks(event)));
            let ranges = data.1.clone();
            self.history_marks.insert(event.id.clone(), data);
            let Some(ranges) = ranges else { continue };
            let ranges = ranges
                .into_iter()
                .filter_map(|item| {
                    let JournalMark {
                        mut mark,
                        final_text_hash,
                    } = item;
                    let paragraph = &mark.range.start.paragraph;
                    let &index = lookup.get(paragraph)?;
                    let hash = current_hashes.entry(index).or_insert_with(|| {
                        text_hash(&model::inline_text(&paragraphs[index].content))
                    });
                    // A historical byte offset can point at unrelated text after
                    // another edit. Keep a paragraph-level anchor in that case.
                    if *hash != final_text_hash {
                        mark.range = point(paragraph, 0);
                    }
                    Some(mark)
                })
                .collect();
            self.entries.push(Entry {
                id: ChangeId::History(event.id.clone()),
                label: event.label.clone(),
                author: event.author.clone(),
                date: Some(event.date.clone()),
                order: index as u64,
                ranges,
            });
        }
        for entry in &mut self.entries {
            entry.ranges.retain(|mark| {
                lookup
                    .get(&mark.range.start.paragraph)
                    .zip(lookup.get(&mark.range.end.paragraph))
                    .is_some_and(|(start, end)| start <= end)
            });
            entry
                .ranges
                .sort_by_key(|mark| (lookup[&mark.range.start.paragraph], mark.range.start.offset));
            for mark in &entry.ranges {
                let first = lookup[&mark.range.start.paragraph];
                let last = lookup[&mark.range.end.paragraph];
                for (index, paragraph) in (first..=last).zip(&paragraphs[first..=last]) {
                    let start = if index == first {
                        mark.range.start.offset
                    } else {
                        0
                    };
                    let end = if index == last {
                        mark.range.end.offset
                    } else {
                        usize::MAX
                    };
                    self.markers
                        .entry(paragraph.id.0.clone())
                        .or_default()
                        .push((start..end, mark.category, mark.deletion));
                }
            }
        }
        self.entries.sort_by_key(|entry| {
            let position = entry.ranges.first().map(|mark| &mark.range.start);
            (
                position
                    .map(|anchor| lookup[&anchor.paragraph])
                    .unwrap_or(usize::MAX),
                position.map_or(0, |anchor| anchor.offset),
                entry.order,
            )
        });
        self.order = lookup
            .into_iter()
            .map(|(id, index)| (id.clone(), index))
            .collect();
        if self
            .active
            .as_ref()
            .is_some_and(|active| !self.entries.iter().any(|entry| entry.id == active.id))
        {
            self.active = None;
        }
    }

    fn position(&self, id: &NodeId, offset: usize) -> Option<(usize, usize)> {
        Some((*self.order.get(id)?, offset))
    }

    fn current(&self, selection: Option<&Selection>) -> Option<usize> {
        if let Some(active) = self
            .active
            .as_ref()
            .filter(|active| active.selection.as_ref() == selection)
        {
            if let Some(index) = self.entries.iter().position(|entry| entry.id == active.id) {
                return Some(index);
            }
        }
        let selection = selection?;
        let a = self.position(&selection.anchor.node_id, selection.anchor.byte_offset)?;
        let b = self.position(&selection.focus.node_id, selection.focus.byte_offset)?;
        let (lo, hi) = (a.min(b), a.max(b));
        self.entries.iter().position(|entry| {
            entry.ranges.iter().any(|mark| {
                let range = &mark.range;
                let start = self.position(&range.start.paragraph, range.start.offset);
                let end = self.position(&range.end.paragraph, range.end.offset);
                start.zip(end).is_some_and(|(start, end)| {
                    if start == end {
                        lo <= start && start <= hi
                    } else if lo == hi {
                        start <= lo && lo < end
                    } else {
                        start < hi && lo < end
                    }
                })
            })
        })
    }

    fn step(&self, selection: Option<&Selection>, forward: bool) -> Option<usize> {
        let count = self.entries.len();
        if count == 0 {
            return None;
        }
        if let Some(index) = self.current(selection) {
            return Some(if forward {
                (index + 1) % count
            } else {
                (index + count - 1) % count
            });
        }
        let caret = selection.and_then(|selection| {
            self.position(&selection.focus.node_id, selection.focus.byte_offset)
        });
        let eligible = |entry: &Entry| {
            entry
                .ranges
                .first()
                .and_then(|mark| {
                    self.position(&mark.range.start.paragraph, mark.range.start.offset)
                })
                .zip(caret)
                .is_some_and(|(at, caret)| if forward { at >= caret } else { at <= caret })
        };
        Some(if forward {
            self.entries.iter().position(eligible).unwrap_or(0)
        } else {
            self.entries.iter().rposition(eligible).unwrap_or(count - 1)
        })
    }
}

pub(crate) fn adorn(state: &CanvasState, overlay: &mut CanvasOverlay) {
    let view = &state.revisions;
    if view.markup == Markup::None || state.display_generation != Some(state.layout_generation) {
        return;
    }
    if view.revision != Some(state.editor.revision)
        || view.epoch != state.documents.epoch
        || view.markers.is_empty()
    {
        return;
    }
    let doc = &state.vector_document;
    let mode = view.markup;
    let window = state.viewport.visible_pages(doc);
    let mut margins = HashSet::new();
    for page_index in window {
        let page = &doc.pages[page_index];
        let left = page.margin_rect.map_or(0.0, |rect| rect.x);
        for hit in &page.hit_regions {
            let Some(ranges) = view.markers.get(&hit.node_id) else {
                continue;
            };
            for (range, category, deletion) in ranges {
                if view.hidden & category.bit() != 0 {
                    continue;
                }
                let marker = if range.is_empty() {
                    (hit.byte_range.contains(&range.start)
                        || hit.caret_stops.iter().any(|p| p.byte_offset == range.start))
                    .then(|| render::OverlayRect {
                        page_index,
                        rect: hit.inline_rect(hit.coordinate_for_offset(range.start), 2.0),
                    })
                } else {
                    render::text_range_rect(page_index, hit, range.clone())
                };
                let Some(mut area) = marker else { continue };
                if mode == Markup::Simple {
                    if !margins.insert((page_index, area.rect.y.round() as i32)) {
                        continue;
                    }
                    area.rect = render::Rect::new(
                        (left - 8.0).max(0.0),
                        area.rect.y,
                        2.0,
                        area.rect.height.max(2.0),
                    );
                }
                let [r, g, b] = category.color(*deletion);
                let a = if mode == Markup::Simple || range.is_empty() {
                    0.9
                } else {
                    0.22
                };
                overlay.review_marks.push(render::ColoredOverlayRect {
                    area,
                    color: render::Color { r, g, b, a },
                });
            }
        }
    }
}

#[derive(Default)]
pub(super) struct PaneView {
    mode_key: Option<(u64, Markup)>,
    list_key: Option<(u64, u64, crate::i18n::Language)>,
    detail_key: Option<(u64, u64, Option<usize>, crate::i18n::Language)>,
}

pub(super) fn bind(ribbon: &Rc<Ribbon>) {
    let weak = Rc::downgrade(ribbon);
    let dropdown = ribbon.ui.markup_mode_combo;
    dropdown.on_selection_changed(move |_| {
        let Some(ribbon) = weak.upgrade() else { return };
        let Some(&(mode, _)) = dropdown
            .get_selection()
            .and_then(|i| Markup::MODES.get(i as usize))
        else {
            return;
        };
        ribbon.set_markup_mode(mode);
    });
    let weak = Rc::downgrade(ribbon);
    let list = ribbon.ui.review_list;
    list.on_selection_changed(move |_| {
        let (Some(ribbon), Some(index)) = (weak.upgrade(), list.get_selection()) else {
            return;
        };
        // A background edit or a tab switch can invalidate a displayed row.
        let (key, id) = {
            let state = ribbon.state.borrow();
            let key = (state.documents.epoch, state.editor.revision);
            let id = state
                .revisions
                .entries
                .get(index as usize)
                .map(|entry| entry.id.clone());
            (key, id)
        };
        if !ribbon
            .review_pane
            .borrow()
            .list_key
            .as_ref()
            .is_some_and(|(epoch, rev, _)| (*epoch, *rev) == key)
        {
            return;
        }
        if let Some(id) = id {
            if let Err(error) = ribbon.go_to_revision(&id, false) {
                ribbon.state.borrow_mut().action_status = Some(error);
            }
            ribbon.sync_revision_controls();
        }
    });
}

impl Ribbon {
    pub(super) fn show_markup_menu(&self) {
        let (mode, hidden) = {
            let state = self.state.borrow();
            (state.revisions.markup, state.revisions.hidden)
        };
        let mut builder = Menu::builder();
        for (offset, (_, label)) in Markup::MODES.iter().enumerate() {
            builder =
                builder.append_radio_item(MARKUP_MODE_FIRST + offset as i32, &text(label), "");
        }
        builder = builder.append_separator();
        for (offset, (_, label)) in Category::MENU.iter().enumerate() {
            builder = builder.append_check_item(MARKUP_FIRST + offset as i32, &text(label), "");
        }
        let mut menu = builder.build();
        menu.check_item(MARKUP_MODE_FIRST + mode as i32, true);
        for (offset, (category, _)) in Category::MENU.iter().enumerate() {
            menu.check_item(MARKUP_FIRST + offset as i32, hidden & category.bit() == 0);
        }
        let button = self.ui.show_markup_button;
        let anchor = crate::gtk_native::to_screen(
            &button,
            wxdragon::geometry::Point::new(0, button.get_size().height),
        );
        self.ui.main_frame.popup_menu(&mut menu, Some(anchor));
    }

    fn set_markup_mode(&self, mode: Markup) {
        let mut state = self.state.borrow_mut();
        state.revisions.markup = mode;
        state.needs_present = true;
    }

    pub(super) fn handle_markup_menu(&self, id: i32) -> bool {
        if let Some(&(mode, _)) = Markup::MODES.get(id.wrapping_sub(MARKUP_MODE_FIRST) as usize) {
            self.set_markup_mode(mode);
            self.sync_revision_controls();
            return true;
        }
        let Some((category, _)) = Category::MENU.get(id.wrapping_sub(MARKUP_FIRST) as usize) else {
            return false;
        };
        let mut state = self.state.borrow_mut();
        state.revisions.hidden ^= category.bit();
        state.needs_present = true;
        true
    }

    pub(super) fn toggle_review_pane(&self) {
        self.set_review_pane(!self.ui.review_panel.is_shown());
    }

    pub(super) fn set_review_pane(&self, show: bool) {
        if show {
            self.install_faces_where(|spec| {
                matches!(
                    spec.name,
                    "review_close_button" | "review_accept_button" | "review_reject_button"
                )
            });
        }
        self.ui.review_panel.show(show);
        self.ui.workspace_panel.layout();
        self.sync_revision_controls();
    }

    pub(super) fn sync_revision_controls(&self) {
        let show = self.ui.review_panel.is_shown();
        let mut state = self.state.borrow_mut();
        let CanvasState {
            revisions,
            editor,
            documents,
            ..
        } = &mut *state;
        let invalidated =
            revisions.revision != Some(editor.revision) || revisions.epoch != documents.epoch;
        revisions.refresh(editor, documents.epoch);
        if revisions
            .active
            .as_ref()
            .is_some_and(|active| active.selection != editor.selection)
        {
            revisions.active = None;
        }
        let current = revisions.current(editor.selection.as_ref());
        let can_step = !revisions.entries.is_empty();
        let mode = revisions.markup;
        let mut pane = self.review_pane.borrow_mut();
        let mode_key = (documents.epoch, mode);
        let update_mode = pane.mode_key.replace(mode_key) != Some(mode_key);
        let mut rows = None;
        let mut details = None;
        if show {
            let language = crate::i18n::language();
            let key = (documents.epoch, editor.revision, language);
            let detail_key = (documents.epoch, editor.revision, current, language);
            if pane.list_key.as_ref() != Some(&key) {
                rows = Some(
                    revisions
                        .entries
                        .iter()
                        .map(Entry::caption)
                        .collect::<Vec<_>>(),
                );
                pane.list_key = Some(key);
            }
            if pane.detail_key.as_ref() != Some(&detail_key) {
                details = Some(match current {
                    Some(index) => detail(&revisions.entries[index], editor),
                    None if can_step => text("Choose a change."),
                    None => text("No pending changes."),
                });
                pane.detail_key = Some(detail_key);
            }
        }
        drop(pane);
        if invalidated && can_step {
            state.needs_present = true;
        }
        drop(state);
        let dropdown = self.ui.markup_mode_combo;
        if update_mode && dropdown.get_selection() != Some(mode as u32) {
            dropdown.set_selection(mode as u32);
        }
        if let Some(rows) = rows {
            let list = self.ui.review_list;
            list.freeze();
            list.clear();
            for label in rows {
                list.append(&label);
            }
            list.thaw();
        }
        if let Some(details) = details {
            self.ui.review_details.change_value(&details);
            let list = self.ui.review_list;
            match (current, list.get_selection()) {
                (Some(index), previous) if previous != Some(index as u32) => {
                    list.set_selection(index as u32, true)
                }
                (None, Some(previous)) => list.set_selection(previous, false),
                _ => {}
            }
        }
        let mut states = self.button_states.borrow_mut();
        for (index, spec) in self.buttons.iter().enumerate() {
            let desired = match spec.action {
                Action::ReviewPane => (show && spec.name == "reviewing_pane_button", true),
                Action::RevisionStep(_) => (false, can_step),
                Action::RevisionResolve(_) => (false, current.is_some()),
                _ => continue,
            };
            if states[index] != Some(desired) {
                spec.button.enable(desired.1);
                skin::selected(&spec.button, desired.0);
                states[index] = Some(desired);
            }
        }
    }

    fn go_to_revision(&self, id: &ChangeId, focus: bool) -> Result<(), String> {
        let result = self.edit_canvas(|state| {
            let view = &mut state.revisions;
            view.refresh(&state.editor, state.documents.epoch);
            let entry = view
                .entries
                .iter()
                .find(|entry| &entry.id == id)
                .ok_or("This change is no longer pending.")?;
            // A deleted structural target can have no surviving text anchor.
            // It still has a list entry and can be resolved from the pane.
            for mark in &entry.ranges {
                if state.editor.go_to_range(&mark.range).is_ok() {
                    break;
                }
            }
            state.caret_page_hint = None;
            view.active = Some(ActiveChange {
                id: id.clone(),
                selection: state.editor.selection.clone(),
            });
            objects_ui::deselect(state);
            Ok(())
        });
        if focus {
            self.focus_document();
        }
        result
    }

    pub(super) fn step_revision(&self, forward: bool) -> Result<(), String> {
        let id = {
            let mut state = self.state.borrow_mut();
            let CanvasState {
                revisions,
                editor,
                documents,
                ..
            } = &mut *state;
            revisions.refresh(editor, documents.epoch);
            revisions
                .step(editor.selection.as_ref(), forward)
                .map(|index| revisions.entries[index].id.clone())
        };
        if let Some(id) = id {
            self.go_to_revision(&id, true)?;
        }
        self.sync_revision_controls();
        Ok(())
    }

    pub(super) fn resolve_current_revision(&self, accept: bool) -> Result<(), String> {
        let (id, following) = {
            let mut state = self.state.borrow_mut();
            let CanvasState {
                revisions,
                editor,
                documents,
                ..
            } = &mut *state;
            revisions.refresh(editor, documents.epoch);
            let Some(index) = revisions.current(editor.selection.as_ref()) else {
                return Ok(());
            };
            let next: Vec<_> = revisions.entries[index + 1..]
                .iter()
                .chain(&revisions.entries[..index])
                .map(|entry| entry.id.clone())
                .collect();
            (revisions.entries[index].id.clone(), next)
        };
        self.edit_without_focus(|editor| id.resolve(editor, accept))?;
        let next = {
            let mut state = self.state.borrow_mut();
            let CanvasState {
                revisions,
                editor,
                documents,
                ..
            } = &mut *state;
            revisions.refresh(editor, documents.epoch);
            let surviving: HashSet<_> = revisions.entries.iter().map(|entry| &entry.id).collect();
            following.into_iter().find(|id| surviving.contains(id))
        };
        if let Some(next) = next {
            self.go_to_revision(&next, false)?;
            // If the decision invalidated the current layout, reveal the new
            // target again when the async layout for it is installed.
            let mut state = self.state.borrow_mut();
            if state.display_generation != Some(state.layout_generation) {
                state.reveal_caret_at = Some(state.layout_generation);
            }
        }
        self.sync_revision_controls();
        Ok(())
    }
}

fn excerpt(source: String) -> String {
    const MAX: usize = 280;
    let end = source.char_indices().nth(MAX).map(|(offset, _)| offset);
    let mut preview = source[..end.unwrap_or(source.len())].replace('\0', "�");
    if end.is_some() {
        preview.push('…');
    }
    preview
}

fn detail(entry: &Entry, editor: &EditorState) -> String {
    let mut out = entry.caption();
    if let Some(date) = &entry.date {
        out.push('\n');
        out.push_str(date);
    }
    let review = &editor.document.review;
    let difference = match &entry.id {
        ChangeId::Word(id) => review.tracked_changes.get(id).and_then(|change| {
            change.effects.iter().find_map(|effect| match effect {
                RevisionEffect::Inline { before, after, .. } => {
                    Some((model::inline_text(before), model::inline_text(after)))
                }
                _ => None,
            })
        }),
        ChangeId::Paragraph(id) => review.revisions.get(id).map(|change| {
            (
                model::inline_text(&change.before.content),
                editor.paragraph_text(&change.paragraph).unwrap_or_default(),
            )
        }),
        ChangeId::History(_) => None,
    };
    if let Some((before, after)) = difference.filter(|(before, after)| before != after) {
        for (label, source) in [("Removed", before), ("Inserted", after)] {
            if !source.is_empty() {
                out.push_str(&format!("\n\n{}\n{}", text(label), excerpt(source)));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::{Inline, Paragraph, TextRun, TrackedChange};

    #[test]
    fn decisions_follow_document_order_and_keep_unselected_revisions_pending() {
        let mut doc = Document::default();
        let first_insert = Inline::Text(TextRun::plain("reviewed"));
        let second_insert = Inline::Text(TextRun::plain("paragraph"));
        let mut first = Paragraph::plain("before ");
        let mut second = Paragraph::plain("other ");
        first.content.push(first_insert.clone());
        second.content.push(second_insert.clone());
        let first_id = first.id.clone();
        let second_id = second.id.clone();
        doc.sections[0].blocks = vec![
            model::Block::Paragraph(first),
            model::Block::Paragraph(second),
        ]
        .into();
        let first_change = NodeId::new();
        let second_change = NodeId::new();
        for (id, paragraph, pos, len, inserted, order) in [
            (&first_change, &first_id, 7, 8, first_insert, 2),
            (&second_change, &second_id, 6, 9, second_insert, 1),
        ] {
            doc.review.tracked_changes.insert(
                id.clone(),
                TrackedChange {
                    id: id.clone(),
                    author: "Reviewer".into(),
                    date: None,
                    order,
                    source_id: order.to_string(),
                    status: RevisionStatus::Pending,
                    effects: vec![RevisionEffect::Inline {
                        range: AnchorRange {
                            start: point(paragraph, pos).start,
                            end: point(paragraph, pos + len).end,
                        },
                        before: Vec::new(),
                        after: vec![inserted],
                        change: InlineRevisionKind::Insertion,
                        move_name: None,
                    }],
                },
            );
        }
        let net = NodeId::new();
        let mut old_second = Paragraph::plain("other ");
        old_second.id = second_id.clone();
        doc.review.revisions.insert(
            net.clone(),
            model::TextRevision {
                id: net,
                paragraph: second_id.clone(),
                author: "Reviewer".into(),
                date: None,
                before: old_second,
            },
        );
        let mut editor = EditorState::new(doc);
        let mut view = ReviewView::default();
        view.refresh(&editor, 1);
        editor.set_caret(first_id.clone(), 1);
        assert_eq!(view.entries.len(), 2);
        assert_eq!(view.current(editor.selection.as_ref()), None);
        assert_eq!(view.step(editor.selection.as_ref(), true), Some(0));
        assert_eq!(view.step(editor.selection.as_ref(), false), Some(1));

        editor
            .go_to_range(&view.entries[0].ranges[0].range)
            .unwrap();
        let current = view.current(editor.selection.as_ref()).unwrap();
        assert_eq!(
            view.entries[current].id,
            ChangeId::Word(first_change.clone())
        );
        view.entries[current].id.resolve(&mut editor, true).unwrap();
        view.refresh(&editor, 1);
        assert_eq!(view.entries.len(), 1);
        assert_eq!(view.entries[0].id, ChangeId::Word(second_change.clone()));
        assert_eq!(
            editor.paragraph_text(&first_id).as_deref(),
            Some("before reviewed")
        );

        view.entries[0].id.resolve(&mut editor, false).unwrap();
        assert_eq!(editor.paragraph_text(&second_id).as_deref(), Some("other "));
        assert!(editor.undo());
        assert_eq!(
            editor.paragraph_text(&second_id).as_deref(),
            Some("other paragraph")
        );
        view.refresh(&editor, 1);
        assert_eq!(view.entries[0].id, ChangeId::Word(second_change));
    }

    #[test]
    fn native_text_edits_use_utf8_offsets_and_comments_stay_out_of_the_revision_list() {
        let mut doc = Document::default();
        let paragraph = Paragraph::plain("");
        let id = paragraph.id.clone();
        doc.sections[0]
            .blocks
            .push(model::Block::Paragraph(paragraph));
        let mut editor = EditorState::new(doc);
        editor.set_caret(id.clone(), 0);
        editor.set_revision_history(true, "Reviewer").unwrap();
        editor.insert_text("ελληνικά").unwrap();
        editor.add_comment("Reviewer", "Check this").unwrap();
        assert_eq!(editor.document.review.history.len(), 2);

        let mut view = ReviewView::default();
        view.refresh(&editor, 1);
        assert_eq!(view.entries.len(), 1);
        let range = &view.entries[0].ranges[0].range;
        assert_eq!(range.start.offset, 0);
        assert_eq!(range.end.offset, "ελληνικά".len());

        let older = view.entries[0].id.clone();
        editor.set_caret(id, 0);
        editor.insert_text("Start ").unwrap();
        view.refresh(&editor, 1);
        let old_entry = view.entries.iter().find(|entry| entry.id == older).unwrap();
        assert_eq!(
            old_entry.ranges[0].range.start,
            old_entry.ranges[0].range.end
        );

        // A new document can start with the same editor revision counter.
        let mut other = EditorState::new(Document::default());
        other.revision = editor.revision;
        view.refresh(&other, 2);
        assert!(view.entries.is_empty());
    }

    #[test]
    fn a_deleted_multibyte_character_marks_a_point_in_final_text() {
        let mark = text_mark(&NodeId::new(), "αβγ", "αγ");
        assert!(mark.deletion);
        assert_eq!(mark.range.start.offset, "α".len());
        assert_eq!(mark.range.start, mark.range.end);
    }
}
