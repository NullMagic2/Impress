//! Semantic editing for Impress.
//!
//! This crate owns keyboard-style editing, selections, formatting commands and
//! granular undo/redo. It deliberately has no GUI or renderer dependency.

mod formatting;
pub mod search;
mod styles;
mod style_selection;
mod multiline;
mod paragraph_break;
pub use styles::validate_styles;
mod review;
pub use review::RevisionChange;
mod contents_sync;
mod anchor_maps;
mod content_controls;
mod note_editing;
mod sections;
mod header_footer;
mod header_footer_color;
pub use header_footer_color::{HeaderFooterColor, HeaderFooterColors, HeaderFooterColorTarget};
pub use header_footer::{HeaderFooterContext, HeaderFooterTemplate, HeaderFooterTemplateText};
mod columns;
pub use columns::{ColumnLayout, ColumnPreset, ColumnScope, DEFAULT_COLUMN_SPACING_MM, MAX_COLUMNS, MIN_COLUMN_WIDTH_MM};
mod objects;
mod floating_pictures;
mod picture_rotation;
pub use picture_rotation::{normalized_degrees, rotated_bounds};
pub mod equation_input;
pub mod clipboard;
pub use clipboard::{ClipboardFragment, ClipboardParagraph, ClipboardReferences, ClipboardResource};
pub use formatting::FormattingSummary;

use model::{
    Block, CharacterProperties, Document, Inline, ListDefinition, NodeId, Paragraph, Resource,
    ResourceId, Section, StyleId, TextRun, STYLE_HEADING_1, STYLE_NORMAL, STYLE_TITLE,
};
use std::time::{Duration, Instant};
use unicode_segmentation::UnicodeSegmentation;

pub use model::{clamp_char_boundary, inline_flat_len, split_content, INLINE_OBJECT_MARKER};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentPosition {
    pub node_id: NodeId,
    /// UTF-8 byte offset in the paragraph's flattened inline stream.
    pub byte_offset: usize,
}

impl DocumentPosition {
    pub fn new(node_id: NodeId, byte_offset: usize) -> Self { Self { node_id, byte_offset } }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    pub anchor: DocumentPosition,
    pub focus: DocumentPosition,
}

/// Which semantic header/footer stream is being edited (defined by the model).
pub use model::HeaderFooterVariant;

/// Physical table-cell location containing the current caret paragraph.
/// The indices address Impress's semantic row/cell vectors, not DOCX grid IDs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableCellContext {
    pub section_index: usize,
    pub table_id: NodeId,
    pub row_index: usize,
    pub cell_index: usize,
    pub cell_id: NodeId,
}

impl Selection {
    pub fn collapsed(position: DocumentPosition) -> Self {
        Self { anchor: position.clone(), focus: position }
    }

    pub fn is_collapsed(&self) -> bool { self.anchor == self.focus }
}

/// IME/preedit state is kept outside the document until commit. This prevents
/// transient composition updates from polluting undo history or DOCX/native
/// serialization. Front ends may display `text` as a temporary preedit run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompositionState {
    pub replacement: Selection,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandId {
    Undo,
    Redo,
    Bold,
    Italic,
    Underline,
    StyleNormal,
    StyleHeading1,
    StyleTitle,
    SelectAll,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
enum Mutation {
    /// Anchor-only mapping; the resulting ReviewChanged mutation owns undo data.
    ContentControlsChanged { before: content_controls::ControlState, after: content_controls::ControlState },
    StylesChanged { before: model::StyleSheet, after: model::StyleSheet },
    AnchorsTransformed { mapping: anchor_maps::AnchorMapping, suppress: usize },
    FootnoteChanged { id:NodeId, before:model::Footnote, after:model::Footnote },
    ReviewChanged { before: model::ReviewState, after: model::ReviewState },
    SectionPropertiesChanged { section_id: NodeId, before: model::SectionProperties, after: model::SectionProperties },
    SectionChanged { section_id: NodeId, before: Section, after: Section },
    SectionInserted { index: usize, section: Section },
    SectionsReplaced { index: usize, before: Vec<Section>, after: Vec<Section> },
    ParagraphChanged { id: NodeId, before: Paragraph, after: Paragraph },
    BlockInserted { section_id: NodeId, index: usize, block: Block },
    BlockRemoved { section_id: NodeId, index: usize, block: Block },
    ListInserted { id: NodeId, definition: ListDefinition },
    ResourceInserted { id: ResourceId, resource: Resource },
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum CoalesceKey {
    Typing(NodeId),
    Backspace(NodeId),
    Delete(NodeId),
    /// Live edits of one equation from the equation pane.
    Equation(NodeId),
}

#[derive(Clone, Debug)]
struct UndoRecord {
    label: &'static str,
    mutations: Vec<Mutation>,
    before_selection: Option<Selection>,
    after_selection: Option<Selection>,
    coalesce: Option<CoalesceKey>,
    timestamp: Instant,
}

#[derive(Clone, Debug)]
pub struct EditorState {
    pub document: Document,
    pub revision: u64,
    pub dirty: bool,
    pub selection: Option<Selection>,
    /// Direct formatting used for newly typed text at a collapsed caret.
    pub typing_properties: CharacterProperties,
    /// Named character style for newly typed runs; kept semantic, not flattened.
    pub typing_style: Option<StyleId>,
    composition: Option<CompositionState>,
    undo: Vec<std::sync::Arc<UndoRecord>>,
    redo: Vec<std::sync::Arc<UndoRecord>>,
    /// Undo-stack depth corresponding to the last successful save. `None`
    /// means that saved state was on a redo branch which has since been
    /// replaced by new edits.
    clean_undo_depth: Option<usize>,
    /// Unrotated source of each bitmap made by `rotate_picture` this session.
    rotation_origins: std::collections::BTreeMap<ResourceId, picture_rotation::RotationOrigin>,
}

impl EditorState {
    pub fn new(mut document: Document) -> Self {
        multiline::normalize_document(&mut document);
        ensure_editable_document(&mut document);
        Self {
            document,
            revision: 0,
            dirty: false,
            selection: None,
            typing_properties: CharacterProperties::default(),
            typing_style: None,
            composition: None,
            undo: Vec::new(),
            redo: Vec::new(),
            clean_undo_depth: Some(0),
            rotation_origins: Default::default(),
        }
    }

    pub fn replace_document(&mut self, mut document: Document) {
        multiline::normalize_document(&mut document);
        ensure_editable_document(&mut document);
        self.document = document;
        self.revision = self.revision.wrapping_add(1);
        self.dirty = false;
        self.undo.clear();
        self.redo.clear();
        self.clean_undo_depth = Some(0);
        self.typing_properties = CharacterProperties::default();
        self.typing_style = None;
        self.composition = None;
        self.selection = first_paragraph_position(&self.document).map(Selection::collapsed);
    }

    pub fn mark_saved(&mut self) {
        // Saving establishes a transaction boundary. Without this, typing
        // immediately after Ctrl+S could merge into a pre-save undo record.
        self.break_coalescing();
        self.clean_undo_depth = Some(self.undo.len());
        self.dirty = false;
    }

    /// A worker saved an older snapshot. The previous clean checkpoint no longer
    /// describes the on-disk file, so even undoing to it must remain dirty.
    pub fn mark_snapshot_saved(&mut self,revision:u64) {if revision==self.revision{self.mark_saved();}else{self.clean_undo_depth=None;self.dirty=true;}}

    /// A restored snapshot has never been saved in this editing session.
    pub fn mark_recovered(&mut self) { self.clean_undo_depth = None; self.dirty = true; }

    pub fn can_undo(&self) -> bool { !self.undo.is_empty() }
    pub fn can_redo(&self) -> bool { !self.redo.is_empty() }

    pub fn set_caret(&mut self, node_id: NodeId, byte_offset: usize) {
        self.composition = None;
        let offset = self.clamp_offset(&node_id, byte_offset);
        self.selection = Some(Selection::collapsed(DocumentPosition::new(node_id, offset)));
        self.reset_typing_properties_from_caret();
        self.break_coalescing();
    }

    /// Select the Unicode word under a hit-tested caret, including accents.
    /// Whitespace/punctuation clicks select their corresponding boundary span.
    pub fn select_word_at(&mut self, node_id: NodeId, byte_offset: usize) -> bool {
        let Some(text) = self.paragraph_text(&node_id) else { return false; };
        let offset = self.clamp_offset(&node_id, byte_offset);
        let words: Vec<_> = text.unicode_word_indices().map(|(start, word)| start..start+word.len()).collect();
        let range = words.iter().find(|range| range.contains(&offset)).cloned()
            .or_else(|| words.iter().find(|range| range.end == offset).cloned())
            .or_else(|| text.split_word_bound_indices().find_map(|(start, span)| {
                let end = start + span.len();
                (start <= offset && (offset < end || offset == text.len())).then_some(start..end)
            }));
        let Some(range) = range else { return false; };
        self.set_caret(node_id.clone(), range.start);
        self.extend_selection(node_id, range.end);
        true
    }

    pub fn extend_selection(&mut self, node_id: NodeId, byte_offset: usize) {
        self.composition = None;
        let offset = self.clamp_offset(&node_id, byte_offset);
        let focus = DocumentPosition::new(node_id, offset);
        match &mut self.selection {
            Some(selection) => selection.focus = focus,
            None => self.selection = Some(Selection::collapsed(focus)),
        }
        self.break_coalescing();
    }

    pub fn execute(&mut self, command: CommandId) -> Result<bool, String> {
        match command {
            CommandId::Undo => Ok(self.undo()),
            CommandId::Redo => Ok(self.redo()),
            CommandId::Bold => { self.toggle_character_property(CharacterToggle::Bold)?; Ok(true) }
            CommandId::Italic => { self.toggle_character_property(CharacterToggle::Italic)?; Ok(true) }
            CommandId::Underline => { self.toggle_character_property(CharacterToggle::Underline)?; Ok(true) }
            CommandId::StyleNormal => { self.apply_paragraph_style(StyleId::from(STYLE_NORMAL))?; Ok(true) }
            CommandId::StyleHeading1 => { self.apply_paragraph_style(StyleId::from(STYLE_HEADING_1))?; Ok(true) }
            CommandId::StyleTitle => { self.apply_paragraph_style(StyleId::from(STYLE_TITLE))?; Ok(true) }
            CommandId::SelectAll => { self.select_all(); Ok(true) }
        }
    }

    pub fn insert_text(&mut self, text: &str) -> Result<(), String> {
        if text.is_empty() { return Ok(()); }
        if text.contains(['\r','\n']) { return self.insert_multiline_text(text); }
        let before_selection = self.selection.clone();
        let mut mutations = Vec::new();
        self.delete_selection_into(&mut mutations)?;
        let position = self.focus_position().ok_or("no active caret")?;
        let before = self.paragraph(&position.node_id).cloned().ok_or("caret paragraph not found")?;
        let mut after = before.clone();
        insert_text_into_paragraph(&mut after, position.byte_offset, text, &self.typing_properties, self.typing_style.as_ref())?;
        let new_offset = position.byte_offset + text.len();
        self.replace_paragraph(&after)?;
        mutations.push(Mutation::AnchorsTransformed{mapping:anchor_maps::AnchorMapping::Replace{paragraph:position.node_id.clone(),start:position.byte_offset,end:position.byte_offset,inserted:text.len()},suppress:1});
        mutations.push(Mutation::ParagraphChanged { id: after.id.clone(), before, after: after.clone() });
        self.selection = Some(Selection::collapsed(DocumentPosition::new(after.id.clone(), new_offset)));
        self.commit(
            "Typing",
            mutations,
            before_selection,
            Some(CoalesceKey::Typing(after.id.clone())),
        )?;
        Ok(())
    }

    pub fn insert_inline_equation(&mut self, equation: model::Equation) -> Result<(), String> {
        self.insert_inline_object(Inline::Equation(equation), "Insert equation")
    }

    /// Typst math typed into the text: when the caret follows the closing `$`
    /// of `$…$` in the same paragraph and the source is valid Typst math, the
    /// source becomes an equation (`$ x $`, with spaces inside, a display
    /// one), as one undoable step after the typing. `\$` stays a dollar sign.
    /// Returns whether an equation was made.
    pub fn convert_typed_math(&mut self) -> Result<bool, String> {
        if self.selection.as_ref().is_some_and(|s| !s.is_collapsed()) { return Ok(false); }
        let Some(position) = self.focus_position() else { return Ok(false); };
        let Some(text) = self.paragraph_text(&position.node_id) else { return Ok(false); };
        let end = position.byte_offset.min(text.len());
        let Some(before) = text.get(..end).and_then(|t| t.strip_suffix('$')) else { return Ok(false); };
        if before.ends_with('\\') { return Ok(false); }
        let Some(open) = before.rfind('$') else { return Ok(false); };
        if text[..open].ends_with('\\') { return Ok(false); }
        let source = &text[open..end];
        if source.contains(INLINE_OBJECT_MARKER) || source.contains('\n') { return Ok(false); }
        // Spaces inside both dollars or neither, so "$5 and $10" stays text
        // (Pandoc's rule for TeX math).
        let body = &source[1..source.len() - 1];
        if body.starts_with(char::is_whitespace) != body.ends_with(char::is_whitespace) { return Ok(false); }
        let Ok((root, display)) = equation_input::parse_with_display(source) else { return Ok(false); };
        let equation = match display {
            Some(model::EquationDisplay::Block) => model::Equation::block(root),
            _ => model::Equation::inline(root),
        };
        self.break_coalescing();
        let id = position.node_id.clone();
        self.selection = Some(Selection { anchor: DocumentPosition::new(id.clone(), open), focus: DocumentPosition::new(id, end) });
        self.insert_inline_object(Inline::Equation(equation), "Insert equation")?;
        Ok(true)
    }

    /// Insert a field (page number, page count, date…) at the caret. A
    /// `display_text` fixes the shown result; `None` lets layout compute it.
    pub fn insert_field(&mut self, kind: model::FieldKind, display_text: Option<String>) -> Result<(), String> {
        let field = model::Field { id: NodeId::new(), kind, display_text, color: None };
        self.insert_inline_object(Inline::Field(field), "Insert field")
    }

    /// Replace the selection with one inline object, as a single undo step.
    fn insert_inline_object(&mut self, inline: Inline, label: &'static str) -> Result<(), String> {
        let before_selection = self.selection.clone();
        let mut mutations = Vec::new();
        self.delete_selection_into(&mut mutations)?;
        let position = self.focus_position().ok_or("no active caret")?;
        let before = self.paragraph(&position.node_id).cloned().ok_or("caret paragraph not found")?;
        let mut after = before.clone();
        insert_inline_into_paragraph(&mut after, position.byte_offset, inline)?;
        let new_offset = position.byte_offset + INLINE_OBJECT_MARKER.len();
        self.replace_paragraph(&after)?;
        mutations.push(Mutation::AnchorsTransformed{mapping:anchor_maps::AnchorMapping::Replace{paragraph:position.node_id.clone(),start:position.byte_offset,end:position.byte_offset,inserted:INLINE_OBJECT_MARKER.len()},suppress:1});
        mutations.push(Mutation::ParagraphChanged { id: after.id.clone(), before, after: after.clone() });
        self.selection = Some(Selection::collapsed(DocumentPosition::new(after.id.clone(), new_offset)));
        self.commit(label, mutations, before_selection, None)?;
        Ok(())
    }

    pub fn backspace(&mut self) -> Result<(), String> {
        if self.selection.as_ref().is_some_and(|s| !s.is_collapsed()) {
            return self.delete_selection();
        }
        let position = self.focus_position().ok_or("no active caret")?;
        if position.byte_offset == 0 {
            return self.merge_with_previous_paragraph(position.node_id);
        }
        let text = self.paragraph_text(&position.node_id).ok_or("caret paragraph not found")?;
        let previous = previous_grapheme_boundary(&text, position.byte_offset);
        self.delete_range_same_paragraph(position.node_id.clone(), previous, position.byte_offset, "Backspace", Some(CoalesceKey::Backspace(position.node_id)))
    }

    pub fn delete_forward(&mut self) -> Result<(), String> {
        if self.selection.as_ref().is_some_and(|s| !s.is_collapsed()) {
            return self.delete_selection();
        }
        let position = self.focus_position().ok_or("no active caret")?;
        let text = self.paragraph_text(&position.node_id).ok_or("caret paragraph not found")?;
        if position.byte_offset >= text.len() {
            return self.merge_with_next_paragraph(position.node_id);
        }
        let next = next_grapheme_boundary(&text, position.byte_offset);
        self.delete_range_same_paragraph(position.node_id.clone(), position.byte_offset, next, "Delete", Some(CoalesceKey::Delete(position.node_id)))
    }

    /// Apply one timer tick's repeated deletions with one paragraph mutation
    /// and anchor/review/undo update, rather than cloning it for each character.
    pub fn delete_repeated(&mut self, backward: bool, mut count: usize) -> Result<(), String> {
        while count > 0 {
            let before_revision = self.revision;
            if self.selection.as_ref().is_some_and(|s| !s.is_collapsed()) {
                self.delete_selection()?;
                count -= 1;
            } else {
                let position = self.focus_position().ok_or("no active caret")?;
                let text = self.paragraph_text(&position.node_id).ok_or("caret paragraph not found")?;
                let offset = position.byte_offset;
                let (start, end, deleted) = if backward {
                    let mut start = offset; let mut deleted = 0;
                    for (index, _) in text[..offset].grapheme_indices(true).rev().take(count) {
                        start = index; deleted += 1;
                    }
                    (start, offset, deleted)
                } else {
                    let mut end = offset; let mut deleted = 0;
                    for grapheme in text[offset..].graphemes(true).take(count) {
                        end += grapheme.len(); deleted += 1;
                    }
                    (offset, end, deleted)
                };
                if deleted > 0 {
                    let id = position.node_id;
                    self.delete_range_same_paragraph(id.clone(), start, end,
                        if backward {"Backspace"} else {"Delete"},
                        Some(if backward {CoalesceKey::Backspace(id)} else {CoalesceKey::Delete(id)}))?;
                    count -= deleted;
                } else {
                    if backward { self.backspace()?; } else { self.delete_forward()?; }
                    count -= 1;
                }
            }
            if self.revision == before_revision { break; }
        }
        Ok(())
    }

    pub fn delete_selection(&mut self) -> Result<(), String> {
        let before_selection = self.selection.clone();
        let mut mutations = Vec::new();
        self.delete_selection_into(&mut mutations)?;
        if mutations.is_empty() { return Ok(()); }
        self.commit("Delete selection", mutations, before_selection, None)?;
        Ok(())
    }

    fn following_style(&self, current:Option<&StyleId>)->Option<StyleId>{
        current.and_then(|id|self.document.styles.get(id).and_then(|style|style.next_style.clone())).or_else(||current.cloned())
    }

    pub fn split_paragraph(&mut self) -> Result<(), String> {
        if self.focus_position().is_some_and(|p|self.note_for_paragraph(&p.node_id).is_some()){return self.split_note_paragraph();}
        let before_selection = self.selection.clone();
        let mut mutations = Vec::new();
        self.delete_selection_into(&mut mutations)?;
        let position = self.focus_position().ok_or("no active caret")?;

        // Mainstream word processors use Enter on an empty list item to leave
        // the list. Do this before creating a second paragraph so nested table
        // cells and header/footer streams get the same behaviour as body text.
        if let Some(before) = self.paragraph(&position.node_id).cloned() {
            if before.properties.list_id.is_some()
                && paragraph_flat_text(&before).is_empty()
                && position.byte_offset == 0
            {
                let mut after = before.clone();
                after.properties.list_id = None;
                after.properties.list_level = None;
                self.replace_paragraph(&after)?;
                mutations.push(Mutation::ParagraphChanged { id: after.id.clone(), before, after });
                self.selection = Some(Selection::collapsed(DocumentPosition::new(position.node_id, 0)));
                self.commit("End list", mutations, before_selection, None)?;
                return Ok(());
            }
        }
        if self.locate_paragraph(&position.node_id).is_none() {
            // Table-cell and header/footer paragraphs live in nested block
            // containers. Keep Enter fully functional there by snapshotting
            // only the containing section for this structural insertion.
            let section_index = self.section_index_for_paragraph(&position.node_id).ok_or("paragraph not found")?;
            let section_before = self.document.sections[section_index].clone();
            let before = self.paragraph(&position.node_id).cloned().ok_or("paragraph not found")?;
            let (current, new_paragraph) = self.split_paragraph_pair(&before, position.byte_offset)?;
            let changed_style = new_paragraph.style != before.style;
            self.replace_paragraph(&current)?;
            let inserted = insert_paragraph_after_in_section(
                &mut self.document.sections[section_index],
                &current.id,
                new_paragraph.clone(),
            );
            if !inserted { self.document.sections[section_index] = section_before; return Err("nested paragraph container not found".into()); }
            let split_mapping = anchor_maps::AnchorMapping::Split { left: current.id.clone(), right: new_paragraph.id.clone(), at: position.byte_offset };
            self.selection = Some(Selection::collapsed(DocumentPosition::new(new_paragraph.id, 0)));
            let section_after = self.document.sections[section_index].clone();
            mutations.push(Mutation::AnchorsTransformed { mapping: split_mapping, suppress: 1 });
            mutations.push(Mutation::SectionChanged { section_id: section_after.id.clone(), before: section_before, after: section_after });
            self.commit("New paragraph", mutations, before_selection, None)?;
            if changed_style { self.reset_typing_properties_from_caret(); }
            return Ok(());
        }
        let (section_index, block_index) = self.locate_paragraph(&position.node_id).ok_or("paragraph not found")?;
        let before = match &self.document.sections[section_index].blocks[block_index] {
            Block::Paragraph(p) => p.clone(),
            _ => return Err("paragraph not found".into()),
        };
        let (current, new_paragraph) = self.split_paragraph_pair(&before, position.byte_offset)?;
        let changed_style = new_paragraph.style != before.style;
        self.document.sections[section_index].blocks[block_index] = Block::Paragraph(current.clone());
        self.document.sections[section_index].blocks.insert(block_index + 1, Block::Paragraph(new_paragraph.clone()));
        mutations.push(Mutation::AnchorsTransformed { mapping: anchor_maps::AnchorMapping::Split { left: current.id.clone(), right: new_paragraph.id.clone(), at: position.byte_offset }, suppress: 2 });
        mutations.push(Mutation::ParagraphChanged { id: current.id.clone(), before, after: current });
        mutations.push(Mutation::BlockInserted {
            section_id: self.document.sections[section_index].id.clone(),
            index: block_index + 1,
            block: Block::Paragraph(new_paragraph.clone()),
        });
        self.selection = Some(Selection::collapsed(DocumentPosition::new(new_paragraph.id, 0)));
        self.commit("New paragraph", mutations, before_selection, None)?;
        if changed_style { self.reset_typing_properties_from_caret(); }
        Ok(())
    }

    pub fn move_left(&mut self, extend: bool) {
        if let Some(edge) = self.selection_edge(-1, extend) { self.move_selection_to(edge, false); return; }
        let Some(pos) = self.focus_position() else { return; };
        let Some(text) = self.paragraph_text(&pos.node_id) else { return; };
        let target = if pos.byte_offset > 0 {
            DocumentPosition::new(pos.node_id.clone(), previous_grapheme_boundary(&text, pos.byte_offset))
        } else if let Some(previous) = self.adjacent_paragraph(&pos.node_id, -1) {
            let end = self.paragraph_text(&previous).map(|s| s.len()).unwrap_or(0);
            DocumentPosition::new(previous, end)
        } else { pos };
        self.move_selection_to(target, extend);
    }

    pub fn move_right(&mut self, extend: bool) {
        if let Some(edge) = self.selection_edge(1, extend) { self.move_selection_to(edge, false); return; }
        let Some(pos) = self.focus_position() else { return; };
        let Some(text) = self.paragraph_text(&pos.node_id) else { return; };
        let target = if pos.byte_offset < text.len() {
            DocumentPosition::new(pos.node_id.clone(), next_grapheme_boundary(&text, pos.byte_offset))
        } else if let Some(next) = self.adjacent_paragraph(&pos.node_id, 1) {
            DocumentPosition::new(next, 0)
        } else { pos };
        self.move_selection_to(target, extend);
    }

    /// Ctrl+Left/Right navigation uses Unicode word boundaries rather than
    /// byte or grapheme steps. Shift keeps the original selection anchor.
    pub fn move_word_left(&mut self, extend: bool) {
        if let Some(edge) = self.selection_edge(-1, extend) { self.move_selection_to(edge, false); return; }
        let Some(pos) = self.focus_position() else { return; };
        let Some(text) = self.paragraph_text(&pos.node_id) else { return; };
        let target = if pos.byte_offset > 0 {
            DocumentPosition::new(pos.node_id.clone(), previous_word_start(&text, pos.byte_offset))
        } else if let Some(previous) = self.adjacent_paragraph(&pos.node_id, -1) {
            let text = self.paragraph_text(&previous).unwrap_or_default();
            DocumentPosition::new(previous, previous_word_start(&text, text.len()))
        } else { pos };
        self.move_selection_to(target, extend);
    }

    pub fn move_word_right(&mut self, extend: bool) {
        if let Some(edge) = self.selection_edge(1, extend) { self.move_selection_to(edge, false); return; }
        let Some(pos) = self.focus_position() else { return; };
        let Some(text) = self.paragraph_text(&pos.node_id) else { return; };
        let target = if pos.byte_offset < text.len() {
            DocumentPosition::new(pos.node_id.clone(), next_word_start(&text, pos.byte_offset))
        } else if let Some(next) = self.adjacent_paragraph(&pos.node_id, 1) {
            DocumentPosition::new(next, 0)
        } else { pos };
        self.move_selection_to(target, extend);
    }

    /// Ctrl+Up/Down moves by semantic paragraphs, independent of line wraps.
    pub fn move_paragraph_up(&mut self, extend: bool) {
        let Some(pos) = self.focus_position() else { return; };
        let target = if pos.byte_offset > 0 {
            DocumentPosition::new(pos.node_id.clone(), 0)
        } else if let Some(previous) = self.adjacent_paragraph(&pos.node_id, -1) {
            DocumentPosition::new(previous, 0)
        } else { pos };
        self.move_selection_to(target, extend);
    }

    pub fn move_paragraph_down(&mut self, extend: bool) {
        let Some(pos) = self.focus_position() else { return; };
        let target = if let Some(next) = self.adjacent_paragraph(&pos.node_id, 1) {
            DocumentPosition::new(next, 0)
        } else {
            let end = self.paragraph_text(&pos.node_id).map(|s| s.len()).unwrap_or(0);
            DocumentPosition::new(pos.node_id, end)
        };
        self.move_selection_to(target, extend);
    }

    fn selection_edge(&self, direction: isize, extend: bool) -> Option<DocumentPosition> {
        if extend { return None; }
        let selection = self.selection.as_ref().filter(|selection| !selection.is_collapsed())?;
        let (start, end) = self.ordered_positions(&selection.anchor, &selection.focus)?;
        Some(if direction < 0 { start } else { end })
    }

    pub fn move_home(&mut self, extend: bool) {
        if let Some(pos) = self.focus_position() {
            self.move_selection_to(DocumentPosition::new(pos.node_id, 0), extend);
        }
    }

    pub fn move_end(&mut self, extend: bool) {
        if let Some(pos) = self.focus_position() {
            let end = self.paragraph_text(&pos.node_id).map(|s| s.len()).unwrap_or(0);
            self.move_selection_to(DocumentPosition::new(pos.node_id, end), extend);
        }
    }

    /// Moves to the closest logical offset in the previous paragraph. Visual
    /// line navigation is refined by the app through hit-testing once the
    /// current layout is available, but this semantic fallback keeps keyboard
    /// navigation deterministic even while a background layout is pending.
    pub fn move_up(&mut self, extend: bool) {
        let Some(pos) = self.focus_position() else { return; };
        let Some(previous) = self.adjacent_paragraph(&pos.node_id, -1) else {
            self.move_home(extend);
            return;
        };
        let end = self.paragraph_text(&previous).map(|s| s.len()).unwrap_or(0);
        self.move_selection_to(DocumentPosition::new(previous, pos.byte_offset.min(end)), extend);
    }

    /// Moves to the closest logical offset in the next paragraph.
    pub fn move_down(&mut self, extend: bool) {
        let Some(pos) = self.focus_position() else { return; };
        let Some(next) = self.adjacent_paragraph(&pos.node_id, 1) else {
            self.move_end(extend);
            return;
        };
        let end = self.paragraph_text(&next).map(|s| s.len()).unwrap_or(0);
        self.move_selection_to(DocumentPosition::new(next, pos.byte_offset.min(end)), extend);
    }

    /// Whether `position` lies within a selection of text (its ends included).
    /// A right-click there keeps the selection, as in Word.
    pub fn selection_contains(&self, position: &DocumentPosition) -> bool {
        let Some(selection) = self.selection.as_ref().filter(|s| !s.is_collapsed()) else { return false; };
        let Some((start, end)) = self.ordered_positions(&selection.anchor, &selection.focus) else { return false; };
        self.ordered_positions(&start, position).is_some_and(|(first, _)| first == start)
            && self.ordered_positions(position, &end).is_some_and(|(_, last)| last == end)
    }

    pub fn select_all(&mut self) {
        if let Some(pos)=self.focus_position(){if self.locate_paragraph(&pos.node_id).is_none(){if let Some(stream)=paragraph_stream_for_node(&self.document,&pos.node_id){
            if let (Some(first),Some(last))=(stream.first(),stream.last()){let a=first.id.clone();let b=last.id.clone();let end=paragraph_flat_text(last).len();self.selection=Some(Selection{anchor:DocumentPosition::new(a,0),focus:DocumentPosition::new(b,end)});self.break_coalescing();return;}
        }}}
        let paragraphs = paragraph_positions(&self.document);
        let (Some(first), Some(last)) = (paragraphs.first(), paragraphs.last()) else { return; };
        let end = paragraph_flat_text(last.2).len();
        self.selection = Some(Selection {
            anchor: DocumentPosition::new(first.2.id.clone(), 0),
            focus: DocumentPosition::new(last.2.id.clone(), end),
        });
        self.break_coalescing();
    }

    pub fn selected_text(&self) -> Option<String> {
        let selection = self.selection.as_ref()?;
        if selection.is_collapsed() { return Some(String::new()); }
        let spans = self.selected_paragraph_spans();
        if spans.is_empty() { return None; }
        let mut pieces = Vec::with_capacity(spans.len());
        for (id, start, end) in spans {
            let text = self.paragraph_text(&id)?;
            pieces.push(text.get(start.min(text.len())..end.min(text.len())).unwrap_or("").to_string());
        }
        Some(pieces.join("\n"))
    }

    /// Copies the active selection as semantic inline content. The returned
    /// `plain_text` is suitable for mirroring to the OS clipboard while the
    /// paragraph payload can be retained by an Impress front end for rich paste.
    pub fn copy_fragment(&self) -> Option<ClipboardFragment> {
        let selection = self.selection.as_ref()?;
        if selection.is_collapsed() { return None; }
        let spans = self.selected_paragraph_spans();
        if spans.is_empty() { return None; }
        let mut paragraphs = Vec::with_capacity(spans.len());
        for (id, start, end) in spans {
            let paragraph = self.paragraph(&id)?;
            let content = slice_content(&paragraph.content, start, end).ok()?;
            paragraphs.push(ClipboardParagraph {
                origin: Some(id.clone()),
                style: paragraph.style.clone(),
                properties: paragraph.properties.clone(),
                content: content.into(),
            });
        }
        let mut fragment = ClipboardFragment {
            paragraphs, plain_text: self.selected_text().unwrap_or_default(),
            resources: Default::default(), lists: Default::default(), ..Default::default()
        };
        fragment.capture_dependencies(&self.document);
        let spans=self.selected_paragraph_spans();
        fragment.capture_references(&self.document,&spans);
        Some(fragment)
    }

    /// Cuts a semantic fragment in one undoable transaction.
    pub fn cut_fragment(&mut self) -> Result<Option<ClipboardFragment>, String> {
        let fragment = self.copy_fragment();
        if let Some(ref fragment) = fragment { fragment.validate()?; self.delete_selection()?; }
        Ok(fragment)
    }

    /// Pastes a semantic fragment, preserving inline objects and run formatting.
    /// Paragraph IDs and inline object IDs are regenerated on insertion.
    pub fn paste_fragment(&mut self, fragment: &ClipboardFragment) -> Result<(), String> {
        fragment.validate()?;
        if !fragment.blocks.is_empty() { return self.paste_block_fragment(fragment); }
        if fragment.table.is_some() { return self.paste_table_fragment(fragment, None); }
        if fragment.paragraphs.is_empty() { return Ok(()); }
        if self.focus_position().is_some_and(|p|self.note_for_paragraph(&p.node_id).is_some()){
            if !fragment.references.footnotes.is_empty(){return Err("Footnotes cannot contain nested footnote references".into());}
            return self.paste_note_fragment(fragment);
        }
        // Clipboard data is untrusted; stage imports and text edits together so
        // validation or an unsupported container never leaves a half-paste.
        let mut staged = self.clone();
        let (fragment, mutations) = staged.import_clipboard_dependencies(fragment)?;
        staged.paste_fragment_inner(&fragment, mutations)?;
        *self = staged;
        Ok(())
    }

    fn paste_fragment_inner(&mut self, fragment: &ClipboardFragment, mut mutations: Vec<Mutation>) -> Result<(), String> {
        let before_selection = self.selection.clone();
        self.delete_selection_into(&mut mutations)?;
        let position = self.focus_position().ok_or("no active caret")?;
        let before = self.paragraph(&position.node_id).cloned().ok_or("caret paragraph not found")?;
        let paste_change_start=mutations.len();
        let top_level = self.locate_paragraph(&position.node_id);
        let section_index = self.section_index_for_paragraph(&position.node_id).ok_or("caret section not found")?;
        let (mut left, right) = split_content(&before.content, position.byte_offset)?;
        let fresh: Vec<ClipboardParagraph> = fragment.paragraphs.iter().map(fresh_clipboard_paragraph).collect();
        let mut placements=std::collections::BTreeMap::new();
        if let Some(origin)=&fresh[0].origin {placements.insert(origin.clone(),(before.id.clone(),position.byte_offset));}

        if fresh.len() == 1 {
            left.extend(fresh[0].content.clone());
            left.extend(right);
            let mut after = before.clone();
            if flat_content_len(&before.content) == 0 { after.style=fresh[0].style.clone(); after.properties=fresh[0].properties.clone(); }
            after.content = (normalized_content(left)).into();
            self.replace_paragraph(&after)?;
            let caret = position.byte_offset + flat_content_len(&fresh[0].content);
            self.selection = Some(Selection::collapsed(DocumentPosition::new(after.id.clone(), caret)));
            mutations.push(Mutation::AnchorsTransformed{mapping:anchor_maps::AnchorMapping::Replace{paragraph:after.id.clone(),start:position.byte_offset,end:position.byte_offset,inserted:flat_content_len(&fresh[0].content)},suppress:1});
            mutations.push(Mutation::ParagraphChanged { id: after.id.clone(), before, after });
        } else if let Some((section_index, block_index)) = top_level {
            left.extend(fresh[0].content.clone());
            let mut first_after = before.clone();
            if flat_content_len(&before.content) == 0 { first_after.style=fresh[0].style.clone(); first_after.properties=fresh[0].properties.clone(); }
            first_after.content = (normalized_content(left)).into();
            self.document.sections[section_index].blocks[block_index] = Block::Paragraph(first_after.clone());
            mutations.push(Mutation::ParagraphChanged { id: first_after.id.clone(), before, after: first_after });

            let section_id = self.document.sections[section_index].id.clone();
            let mut insertion_index = block_index + 1;
            let last_index = fresh.len() - 1;
            let mut final_id = None;
            let mut final_offset = 0usize;
            for (i, part) in fresh.iter().enumerate().skip(1) {
                let mut content = part.content.clone();
                if i == last_index {
                    final_offset = flat_content_len(&content);
                    content.extend(right.clone());
                }
                let paragraph = Paragraph {
                    id: NodeId::new(), style: part.style.clone(), properties: part.properties.clone(),
                    content: (normalized_content(content)).into(),
                };
                if let Some(origin)=&part.origin {placements.insert(origin.clone(),(paragraph.id.clone(),0));}
                if i == last_index { final_id = Some(paragraph.id.clone()); }
                let block = Block::Paragraph(paragraph);
                self.document.sections[section_index].blocks.insert(insertion_index, block.clone());
                mutations.push(Mutation::BlockInserted { section_id: section_id.clone(), index: insertion_index, block });
                insertion_index += 1;
            }
            self.selection = final_id.map(|id| Selection::collapsed(DocumentPosition::new(id, final_offset)));
        } else {
            // A table cell or header/footer owns a nested block vector. Snapshot
            // only its containing section for structural paragraph insertion;
            // a single-paragraph paste above still uses a granular paragraph
            // mutation because it does not alter container structure.
            let section_before = self.document.sections[section_index].clone();
            left.extend(fresh[0].content.clone());
            let mut first_after = before.clone();
            if flat_content_len(&before.content) == 0 { first_after.style=fresh[0].style.clone(); first_after.properties=fresh[0].properties.clone(); }
            first_after.content = (normalized_content(left)).into();
            self.replace_paragraph(&first_after)?;

            let last_index = fresh.len() - 1;
            let mut previous_id = first_after.id.clone();
            let mut final_id = previous_id.clone();
            let mut final_offset = position.byte_offset + flat_content_len(&fresh[0].content);
            for (i, part) in fresh.iter().enumerate().skip(1) {
                let mut content = part.content.clone();
                if i == last_index {
                    final_offset = flat_content_len(&content);
                    content.extend(right.clone());
                }
                let paragraph = Paragraph {
                    id: NodeId::new(), style: part.style.clone(), properties: part.properties.clone(),
                    content: (normalized_content(content)).into(),
                };
                if let Some(origin)=&part.origin {placements.insert(origin.clone(),(paragraph.id.clone(),0));}
                final_id = paragraph.id.clone();
                if !insert_paragraph_after_in_section(&mut self.document.sections[section_index], &previous_id, paragraph.clone()) {
                    self.document.sections[section_index] = section_before;
                    return Err("nested paragraph container not found during paste".into());
                }
                previous_id = paragraph.id;
            }
            let section_after = self.document.sections[section_index].clone();
            mutations.push(Mutation::SectionChanged {
                section_id: section_after.id.clone(), before: section_before, after: section_after,
            });
            self.selection = Some(Selection::collapsed(DocumentPosition::new(final_id, final_offset)));
        }
        if fresh.len()>1{if let Some(tail)=self.focus_position(){
            let count=mutations.len()-paste_change_start;
            mutations.splice(paste_change_start..paste_change_start,[
                Mutation::AnchorsTransformed{mapping:anchor_maps::AnchorMapping::Split{left:position.node_id.clone(),right:tail.node_id.clone(),at:position.byte_offset},suppress:0},
                Mutation::AnchorsTransformed{mapping:anchor_maps::AnchorMapping::Offset{paragraph:tail.node_id,at:0,delta:tail.byte_offset},suppress:count},
            ]);
        }}
        self.finish_clipboard_references(fragment,&placements,&mut mutations,&before_selection)?;
        self.commit("Paste", mutations, before_selection, None)?;
        Ok(())
    }

    /// Begins an IME composition. Composition text is intentionally not written
    /// to the semantic document until `commit_composition`.
    pub fn begin_composition(&mut self) -> Result<(), String> {
        let replacement = self.selection.clone().ok_or("no active selection")?;
        self.break_coalescing();
        self.composition = Some(CompositionState { replacement, text: String::new() });
        Ok(())
    }

    pub fn update_composition(&mut self, text: impl Into<String>) -> Result<(), String> {
        if self.composition.is_none() { self.begin_composition()?; }
        if let Some(composition) = &mut self.composition { composition.text = text.into(); }
        Ok(())
    }

    pub fn composition(&self) -> Option<&CompositionState> { self.composition.as_ref() }

    /// Commits the current preedit as one ordinary semantic insertion, producing
    /// a single undo transaction regardless of how many IME updates occurred.
    pub fn commit_composition(&mut self) -> Result<(), String> {
        let Some(composition) = self.composition.take() else { return Ok(()); };
        self.selection = Some(composition.replacement);
        if !composition.text.is_empty() { self.insert_text(&composition.text)?; }
        Ok(())
    }

    pub fn cancel_composition(&mut self) {
        if let Some(composition) = self.composition.take() {
            self.selection = Some(composition.replacement);
        }
        self.break_coalescing();
    }

    /// Allows a layout-aware front end to move the semantic selection to an
    /// already-resolved visual caret target without exposing editor internals.
    pub fn move_to_position(&mut self, target: DocumentPosition, extend: bool) {
        let offset = self.clamp_offset(&target.node_id, target.byte_offset);
        self.move_selection_to(DocumentPosition::new(target.node_id, offset), extend);
    }

    pub fn apply_paragraph_style(&mut self, style: StyleId) -> Result<(), String> {
        self.apply_named_style(&style, true)
    }

    pub fn toggle_character_property(&mut self, property: CharacterToggle) -> Result<(), String> {
        let selection = self.selection.clone().ok_or("no active selection")?;
        if selection.is_collapsed() {
            let current = match property {
                CharacterToggle::Bold => self.typing_properties.bold.unwrap_or_else(|| self.effective_caret_properties().bold.unwrap_or(false)),
                CharacterToggle::Italic => self.typing_properties.italic.unwrap_or_else(|| self.effective_caret_properties().italic.unwrap_or(false)),
                CharacterToggle::Underline => self.typing_properties.underline.unwrap_or_else(|| self.effective_caret_properties().underline.unwrap_or(false)),
            };
            set_toggle(&mut self.typing_properties, property, !current);
            self.break_coalescing();
            return Ok(());
        }

        let target_value = !self.selection_has_toggle(property);
        let before_selection = self.selection.clone();
        let targets = self.selected_paragraph_spans();
        let mut mutations = Vec::new();
        for (id, start, end) in targets {
            let before = self.paragraph(&id).cloned().ok_or("paragraph not found")?;
            let mut after = before.clone();
            apply_format_range(&mut after, start, end, property, target_value)?;
            self.replace_paragraph(&after)?;
            mutations.push(Mutation::ParagraphChanged { id, before, after });
        }
        self.commit("Character formatting", mutations, before_selection, None)?;
        Ok(())
    }

    pub fn undo(&mut self) -> bool {
        let Some(record) = self.undo.last() else { return false; };
        // Validate/apply to a candidate first. A failed transaction must not
        // consume the undo record or leave half-applied structural mutations.
        let mut candidate=self.document.clone();
        for mutation in record.mutations.iter().rev() {
            if apply_mutation(&mut candidate, mutation, false).is_err() { return false; }
        }
        let record=self.undo.pop().expect("validated undo record");
        self.document=candidate;
        self.selection = record.before_selection.clone();
        self.redo.push(record);
        self.revision = self.revision.wrapping_add(1);
        self.refresh_dirty_from_history();
        self.reset_typing_properties_from_caret();
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(record) = self.redo.last() else { return false; };
        let mut candidate=self.document.clone();
        for mutation in &record.mutations {
            if apply_mutation(&mut candidate, mutation, true).is_err() { return false; }
        }
        let record=self.redo.pop().expect("validated redo record");
        self.document=candidate;
        self.selection = record.after_selection.clone();
        self.undo.push(record);
        self.revision = self.revision.wrapping_add(1);
        self.refresh_dirty_from_history();
        self.reset_typing_properties_from_caret();
        true
    }

    /// Joins the undo steps recorded since the stack held `depth` steps into
    /// one, so a compound edit undoes at once.
    pub(crate) fn group_undo_since(&mut self, depth: usize, label: &'static str) {
        if self.undo.len() <= depth + 1 { return; }
        let steps: Vec<_> = self.undo.drain(depth..).collect();
        let record = UndoRecord {
            label,
            mutations: steps.iter().flat_map(|r| r.mutations.iter().cloned()).collect(),
            before_selection: steps.first().and_then(|r| r.before_selection.clone()),
            after_selection: steps.last().and_then(|r| r.after_selection.clone()),
            coalesce: None,
            timestamp: Instant::now(),
        };
        self.undo.push(std::sync::Arc::new(record));
        if self.clean_undo_depth.is_some_and(|d| d > depth) { self.clean_undo_depth = None; }
        self.refresh_dirty_from_history();
    }

    pub fn undo_label(&self) -> Option<&'static str> { self.undo.last().map(|r| r.label) }
    pub fn redo_label(&self) -> Option<&'static str> { self.redo.last().map(|r| r.label) }

    pub fn paragraph_text(&self, id: &NodeId) -> Option<String> { self.paragraph(id).map(paragraph_flat_text) }

    pub fn focus_position(&self) -> Option<DocumentPosition> { self.selection.as_ref().map(|s| s.focus.clone()) }

    fn delete_range_same_paragraph(
        &mut self,
        id: NodeId,
        start: usize,
        end: usize,
        label: &'static str,
        coalesce: Option<CoalesceKey>,
    ) -> Result<(), String> {
        let before_selection = self.selection.clone();
        let before = self.paragraph(&id).cloned().ok_or("paragraph not found")?;
        let mut after = before.clone();
        delete_range_from_paragraph(&mut after, start, end)?;
        self.replace_paragraph(&after)?;
        self.selection = Some(Selection::collapsed(DocumentPosition::new(id.clone(), start)));
        self.commit(label, vec![Mutation::AnchorsTransformed{mapping:anchor_maps::AnchorMapping::Replace{paragraph:id.clone(),start,end,inserted:0},suppress:1},Mutation::ParagraphChanged { id, before, after }], before_selection, coalesce)?;
        Ok(())
    }

    fn delete_selection_into(&mut self, mutations: &mut Vec<Mutation>) -> Result<(), String> {
        let Some(selection) = self.selection.clone() else { return Ok(()); };
        if selection.is_collapsed() { return Ok(()); }
        let Some((start, end)) = self.ordered_positions(&selection.anchor, &selection.focus) else {
            return Err("selection is not in editable paragraphs".into());
        };

        if start.node_id == end.node_id {
            let before = self.paragraph(&start.node_id).cloned().ok_or("paragraph not found")?;
            let mut after = before.clone();
            delete_range_from_paragraph(&mut after, start.byte_offset, end.byte_offset)?;
            self.replace_paragraph(&after)?;
            mutations.push(Mutation::AnchorsTransformed{mapping:anchor_maps::AnchorMapping::Replace{paragraph:after.id.clone(),start:start.byte_offset,end:end.byte_offset,inserted:0},suppress:1});
            mutations.push(Mutation::ParagraphChanged { id: start.node_id.clone(), before, after });
            self.selection = Some(Selection::collapsed(start));
            return Ok(());
        }

        if self.note_for_paragraph(&start.node_id).is_some()||self.note_for_paragraph(&end.node_id).is_some(){return self.delete_note_range(&start,&end,mutations);}
        let top_level_start = self.locate_paragraph(&start.node_id);
        let top_level_end = self.locate_paragraph(&end.node_id);
        if top_level_start.is_none() || top_level_end.is_none() {
            let section_index = self.section_index_for_paragraph(&start.node_id).ok_or("selection start paragraph not found")?;
            if self.section_index_for_paragraph(&end.node_id) != Some(section_index) {
                return Err("Table cells and headers/footers are independent text streams; select within one stream".into());
            }
            let before = self.document.sections[section_index].clone();
            let mapping=anchor_maps::AnchorMapping::splice(&self.document,&start,&end);
            let target = delete_nested_range_in_section(
                &mut self.document.sections[section_index], &start, &end,
            )?.ok_or("selection endpoints are not in the same editable table/header/footer stream")?;
            let after = self.document.sections[section_index].clone();
            mutations.push(Mutation::AnchorsTransformed { mapping, suppress: 1 });
            mutations.push(Mutation::SectionChanged { section_id: after.id.clone(), before, after });
            self.selection = Some(Selection::collapsed(DocumentPosition::new(target, start.byte_offset)));
            return Ok(());
        }
        let (section_index, start_index) = top_level_start.expect("checked above");
        let (end_section, end_index) = top_level_end.expect("checked above");
        if section_index != end_section {
            return self.delete_across_sections(section_index, start_index, end_section, end_index, &start, &end, mutations);
        }

        let first_before = match &self.document.sections[section_index].blocks[start_index] {
            Block::Paragraph(p) => p.clone(),
            _ => return Err("selection start is not a paragraph".into()),
        };
        let last_before = match &self.document.sections[section_index].blocks[end_index] {
            Block::Paragraph(p) => p.clone(),
            _ => return Err("selection end is not a paragraph".into()),
        };
        let (first_left, _) = split_content(&first_before.content, start.byte_offset)?;
        let (_, last_right) = split_content(&last_before.content, end.byte_offset)?;
        let mut first_after = first_before.clone();
        let mut merged = first_left;
        merged.extend(last_right);
        first_after.content = (normalized_content(merged)).into();
        let mapping=anchor_maps::AnchorMapping::splice(&self.document,&start,&end);
        self.document.sections[section_index].blocks[start_index] = Block::Paragraph(first_after.clone());
        mutations.push(Mutation::AnchorsTransformed { mapping, suppress: end_index-start_index+1 });
        mutations.push(Mutation::ParagraphChanged {
            id: first_after.id.clone(), before: first_before, after: first_after.clone(),
        });

        let section_id = self.document.sections[section_index].id.clone();
        for index in (start_index + 1..=end_index).rev() {
            let block = self.document.sections[section_index].blocks.remove(index);
            mutations.push(Mutation::BlockRemoved { section_id: section_id.clone(), index, block });
        }
        self.selection = Some(Selection::collapsed(DocumentPosition::new(first_after.id, start.byte_offset)));
        Ok(())
    }

    fn merge_with_previous_paragraph(&mut self, current_id: NodeId) -> Result<(), String> {
        if self.note_for_paragraph(&current_id).is_some(){return self.merge_note_paragraph(current_id,-1);}
        let Some((section_index, current_index)) = self.locate_paragraph(&current_id) else {
            return self.merge_nested_adjacent_paragraph(current_id, -1);
        };
        if current_index == 0 {
            return if section_index > 0 { self.join_section_boundary(section_index - 1, "Backspace section break") } else { Ok(()) };
        }
        let previous_index = current_index - 1;
        if !matches!(self.document.sections[section_index].blocks[previous_index], Block::Paragraph(_)) { return Ok(()); }
        let before_selection = self.selection.clone();
        let previous_before = match &self.document.sections[section_index].blocks[previous_index] { Block::Paragraph(p) => p.clone(), _ => unreachable!() };
        let current = match &self.document.sections[section_index].blocks[current_index] { Block::Paragraph(p) => p.clone(), _ => unreachable!() };
        let caret = paragraph_flat_text(&previous_before).len();
        let mut previous_after = previous_before.clone();
        previous_after.content.extend(current.content.clone());
        previous_after.content = (normalized_content(previous_after.content)).into();
        self.document.sections[section_index].blocks[previous_index] = Block::Paragraph(previous_after.clone());
        let removed = self.document.sections[section_index].blocks.remove(current_index);
        let section_id = self.document.sections[section_index].id.clone();
        self.selection = Some(Selection::collapsed(DocumentPosition::new(previous_after.id.clone(), caret)));
        self.commit("Merge paragraphs", vec![
            Mutation::AnchorsTransformed { mapping: anchor_maps::AnchorMapping::Join { left: previous_after.id.clone(), right: current.id.clone(), at: caret }, suppress: 2 },
            Mutation::ParagraphChanged { id: previous_after.id.clone(), before: previous_before, after: previous_after },
            Mutation::BlockRemoved { section_id, index: current_index, block: removed },
        ], before_selection, None)?;
        Ok(())
    }

    fn merge_with_next_paragraph(&mut self, current_id: NodeId) -> Result<(), String> {
        if self.note_for_paragraph(&current_id).is_some(){return self.merge_note_paragraph(current_id,1);}
        let Some((section_index, current_index)) = self.locate_paragraph(&current_id) else {
            return self.merge_nested_adjacent_paragraph(current_id, 1);
        };
        let next_index = current_index + 1;
        if next_index >= self.document.sections[section_index].blocks.len() {
            return self.join_section_boundary(section_index, "Delete section break");
        }
        if !matches!(self.document.sections[section_index].blocks[next_index], Block::Paragraph(_)) { return Ok(()); }
        let before_selection = self.selection.clone();
        let current_before = match &self.document.sections[section_index].blocks[current_index] { Block::Paragraph(p) => p.clone(), _ => unreachable!() };
        let next = match &self.document.sections[section_index].blocks[next_index] { Block::Paragraph(p) => p.clone(), _ => unreachable!() };
        let caret = paragraph_flat_text(&current_before).len();
        let mut current_after = current_before.clone();
        current_after.content.extend(next.content.clone());
        current_after.content = (normalized_content(current_after.content)).into();
        self.document.sections[section_index].blocks[current_index] = Block::Paragraph(current_after.clone());
        let removed = self.document.sections[section_index].blocks.remove(next_index);
        let section_id = self.document.sections[section_index].id.clone();
        self.selection = Some(Selection::collapsed(DocumentPosition::new(current_after.id.clone(), caret)));
        self.commit("Merge paragraphs", vec![
            Mutation::AnchorsTransformed { mapping: anchor_maps::AnchorMapping::Join { left: current_after.id.clone(), right: next.id.clone(), at: caret }, suppress: 2 },
            Mutation::ParagraphChanged { id: current_after.id.clone(), before: current_before, after: current_after },
            Mutation::BlockRemoved { section_id, index: next_index, block: removed },
        ], before_selection, None)?;
        Ok(())
    }

    /// Paragraph formatting follows a half-open selection: an endpoint at
    /// byte zero of the following paragraph does not select that paragraph.
    /// Keep the raw spans for clipboard operations, which need the separator.
    fn paragraph_formatting_spans(&self) -> Vec<(NodeId, usize, usize)> {
        let mut spans = self.selected_paragraph_spans();
        if spans.len() > 1 && spans.last().is_some_and(|(_, start, end)| *start == 0 && *end == 0) {
            spans.pop();
        }
        spans
    }

    fn selected_paragraph_ids(&self) -> Vec<NodeId> {
        self.paragraph_formatting_spans().into_iter().map(|(id, _, _)| id).collect()
    }

    /// Selected UTF-8 byte ranges, in the editor's document-stream order.
    pub fn selected_paragraph_spans(&self) -> Vec<(NodeId, usize, usize)> {
        let Some(selection) = &self.selection else { return Vec::new(); };
        let Some((start, end)) = self.ordered_positions(&selection.anchor, &selection.focus) else { return Vec::new(); };
        let paragraphs = paragraph_stream_for_node(&self.document, &start.node_id)
            .filter(|stream| stream.iter().any(|paragraph| paragraph.id == end.node_id))
            .unwrap_or_else(|| paragraph_positions(&self.document).into_iter().map(|(_, _, p)| p).collect());
        let Some(start_index) = paragraphs.iter().position(|paragraph| paragraph.id == start.node_id) else { return Vec::new(); };
        let Some(end_index) = paragraphs.iter().position(|paragraph| paragraph.id == end.node_id) else { return Vec::new(); };
        if start_index > end_index { return Vec::new(); }
        paragraphs[start_index..=end_index].iter().map(|paragraph| {
            let len = paragraph_flat_text(paragraph).len();
            let a = if paragraph.id == start.node_id { start.byte_offset.min(len) } else { 0 };
            let b = if paragraph.id == end.node_id { end.byte_offset.min(len) } else { len };
            (paragraph.id.clone(), a, b)
        }).collect()
    }

    /// Returns true only when every selected text run has the toggle enabled.
    /// This matches conventional word-processor behaviour: a mixed selection
    /// becomes fully bold/italic/underlined on the first click, rather than
    /// turning the already-formatted part off.
    fn selection_has_toggle(&self, property: CharacterToggle) -> bool {
        let spans = self.selected_paragraph_spans();
        let mut saw_text = false;
        for (id, start, end) in spans {
            let Some(paragraph) = self.paragraph(&id) else { continue; };
            let (selected, _) = split_content(&paragraph.content, end).unwrap_or_default();
            let (_, selected) = split_content(&selected, start).unwrap_or_default();
            for inline in selected {
                if let Inline::Text(run) = inline {
                    if run.text.is_empty() { continue; }
                    saw_text = true;
                    let effective = self.document.styles.resolve_run(
                        paragraph.style.as_ref(),
                        run.style.as_ref(),
                        &run.properties,
                    );
                    let enabled = match property {
                        CharacterToggle::Bold => effective.bold.unwrap_or(false),
                        CharacterToggle::Italic => effective.italic.unwrap_or(false),
                        CharacterToggle::Underline => effective.underline.unwrap_or(false),
                    };
                    if !enabled { return false; }
                }
            }
        }
        saw_text
    }

    fn merge_nested_adjacent_paragraph(&mut self, current_id: NodeId, direction: isize) -> Result<(), String> {
        let Some(section_index) = self.section_index_for_paragraph(&current_id) else { return Ok(()); };
        let before_selection = self.selection.clone();
        let before = self.document.sections[section_index].clone();
        let Some((target, caret)) = merge_adjacent_in_section(
            &mut self.document.sections[section_index], &current_id, direction,
        ) else { return Ok(()); };
        let after = self.document.sections[section_index].clone();
        let mut old_ps=Vec::new(); collect_paragraph_refs(&before.blocks,&mut old_ps);
        for h in before.header_footers() { collect_paragraph_refs(&h.stream.blocks,&mut old_ps); }
        let removed=old_ps.iter().find(|p|self.paragraph(&p.id).is_none()).map(|p|p.id.clone());
        let mut changes=Vec::new();
        if let Some(right)=removed { changes.push(Mutation::AnchorsTransformed { mapping: anchor_maps::AnchorMapping::Join {left:target.clone(),right,at:caret},suppress:1 }); }
        self.selection = Some(Selection::collapsed(DocumentPosition::new(target, caret)));
        changes.push(Mutation::SectionChanged { section_id: after.id.clone(), before, after });
        self.commit(
            "Merge paragraphs",
            changes,
            before_selection,
            None,
        )?;
        Ok(())
    }

    fn effective_caret_properties(&self) -> CharacterProperties {
        let Some(pos) = self.focus_position() else { return CharacterProperties::default(); };
        let Some(paragraph) = self.paragraph(&pos.node_id) else { return CharacterProperties::default(); };
        let mut cursor = 0usize;
        let mut previous: Option<&TextRun> = None;
        for inline in &paragraph.content {
            match inline {
                Inline::Text(run) => {
                    let end = cursor + run.text.len();
                    if pos.byte_offset <= end {
                        return self.document.styles.resolve_run(paragraph.style.as_ref(), run.style.as_ref(), &run.properties);
                    }
                    previous = Some(run);
                    cursor = end;
                }
                _ => cursor += inline_flat_len(inline),
            }
        }
        previous.map(|run| self.document.styles.resolve_run(paragraph.style.as_ref(), run.style.as_ref(), &run.properties))
            .unwrap_or_else(|| self.document.styles.resolve_character(paragraph.style.as_ref()))
    }

    fn reset_typing_properties_from_caret(&mut self) {
        // Inherit the adjacent semantic run, not a flattened resolved style. At
        // an insertion boundary prefer the preceding character; at offset zero
        // use the following run. Explicit toolbar overrides remain until moving.
        fn at(items:&[Inline],probe:usize,offset:&mut usize)->Option<(Option<StyleId>,CharacterProperties)>{
            for item in items{let len=inline_flat_len(item);match item{
                Inline::Text(t) if probe>=*offset && (probe<*offset+len || (len==0&&probe==*offset))=>return Some((t.style.clone(),t.properties.clone())),
                Inline::Hyperlink(h) if probe>=*offset&&probe<*offset+len=>return at(&h.content,probe,offset),_=>{}}
                *offset+=len;
            }None
        }
        let value=self.focus_position().and_then(|p|self.paragraph(&p.node_id).and_then(|para|at(&para.content,p.byte_offset.saturating_sub(1),&mut 0)));
        let(style,properties)=value.unwrap_or_default();self.typing_style=style;self.typing_properties=properties;
    }

    fn move_selection_to(&mut self, target: DocumentPosition, extend: bool) {
        self.composition = None;
        if extend {
            match &mut self.selection {
                Some(selection) => selection.focus = target,
                None => self.selection = Some(Selection::collapsed(target)),
            }
        } else {
            self.selection = Some(Selection::collapsed(target));
            self.reset_typing_properties_from_caret();
        }
        self.break_coalescing();
    }

    fn commit(
        &mut self,
        label: &'static str,
        mutations: Vec<Mutation>,
        before_selection: Option<Selection>,
        coalesce: Option<CoalesceKey>,
    ) -> Result<(), String> {
        if mutations.is_empty() { return Ok(()); }
        let mut mutations = mutations;
        if let Err(error)=self.synchronize_content_controls(&mut mutations) {
            for mutation in mutations.iter().rev(){apply_mutation(&mut self.document,mutation,false)?;}
            self.selection=before_selection.clone();self.reset_typing_properties_from_caret();return Err(error);
        }
        self.prepare_review_transaction(label, &mut mutations, &before_selection)?;
        if let Err(error)=self.synchronize_picture_anchors(&mut mutations).and_then(|_|self.synchronize_contents_transaction(label,&mut mutations)).and_then(|_|self.record_native_revision(label,&mut mutations)) {
            for m in mutations.iter().rev(){apply_mutation(&mut self.document,m,false)?;}
            self.selection=before_selection.clone(); self.reset_typing_properties_from_caret();
            return Err(error);
        }
        let now = Instant::now();
        let after_selection = self.selection.clone();
        if self.clean_undo_depth.is_some_and(|depth| depth > self.undo.len()) {
            self.clean_undo_depth = None;
        }
        let can_coalesce = coalesce.as_ref().is_some_and(|key| {
            self.undo.last().is_some_and(|last| {
                last.coalesce.as_ref() == Some(key)
                    && (matches!(key, CoalesceKey::Equation(_)) || now.duration_since(last.timestamp) <= Duration::from_millis(900))
                    && last.after_selection == before_selection
                    && last.mutations.len() == 1
                    && mutations.len() == 1
            })
        });

        if can_coalesce {
            if let (Some(last), Some(new_mutation)) = (self.undo.last_mut(), mutations.into_iter().next()) {
                let last=std::sync::Arc::make_mut(last);
                match (&mut last.mutations[0], new_mutation) {
                    (Mutation::ParagraphChanged { after, .. }, Mutation::ParagraphChanged { after: new_after, .. }) => {
                        *after = new_after;
                        last.after_selection = after_selection;
                        last.timestamp = now;
                    }
                    (Mutation::SectionChanged { section_id, after, .. }, Mutation::SectionChanged { section_id: new_id, after: new_after, .. }) if *section_id == new_id => {
                        *after = new_after;
                        last.after_selection = after_selection;
                        last.timestamp = now;
                    }
                    (_, other) => {
                        last.mutations.push(other);
                    }
                }
            }
        } else {
            self.undo.push(std::sync::Arc::new(UndoRecord {
                label,
                mutations,
                before_selection,
                after_selection,
                coalesce,
                timestamp: now,
            }));
        }
        self.redo.clear();
        self.revision = self.revision.wrapping_add(1);
        self.refresh_dirty_from_history();
        Ok(())
    }

    fn refresh_dirty_from_history(&mut self) {
        self.dirty = self.clean_undo_depth != Some(self.undo.len());
    }

    fn break_coalescing(&mut self) {
        if let Some(last) = self.undo.last_mut() { std::sync::Arc::make_mut(last).coalesce = None; }
    }

    fn paragraph(&self, id: &NodeId) -> Option<&Paragraph> {
        for section in &self.document.sections {
            if let Some(paragraph) = find_paragraph_in_blocks(&section.blocks, id) { return Some(paragraph); }
            for header_footer in section.header_footers() {
                if let Some(paragraph) = find_paragraph_in_blocks(&header_footer.stream.blocks, id) { return Some(paragraph); }
            }
        }
        for note in self.document.review.footnotes.values(){if let Some(p)=find_paragraph_in_blocks(&note.blocks,id){return Some(p);}}
        None
    }

    fn replace_paragraph(&mut self, paragraph: &Paragraph) -> Result<(), String> {
        for section in &mut self.document.sections {
            if replace_paragraph_in_blocks(&mut section.blocks, paragraph) { return Ok(()); }
            for header_footer in section.header_footers_mut() {
                if replace_paragraph_in_blocks(&mut header_footer.blocks, paragraph) { return Ok(()); }
            }
        }
        for note in self.document.review.footnotes.values_mut(){if replace_paragraph_in_blocks(&mut note.blocks,paragraph){return Ok(());}}
        Err("paragraph not found".into())
    }

    fn locate_paragraph(&self, id: &NodeId) -> Option<(usize, usize)> {
        self.document.sections.iter().enumerate().find_map(|(section_index, section)| {
            section.blocks.iter().enumerate().find_map(|(block_index, block)| match block {
                Block::Paragraph(paragraph) if &paragraph.id == id => Some((section_index, block_index)),
                _ => None,
            })
        })
    }

    fn section_index_for_paragraph(&self, id: &NodeId) -> Option<usize> {
        self.document.sections.iter().enumerate().find_map(|(section_index, section)| {
            if find_paragraph_in_blocks(&section.blocks, id).is_some() { return Some(section_index); }
            let in_header_footer = section.header_footers()
                .any(|hf| find_paragraph_in_blocks(&hf.stream.blocks, id).is_some());
            in_header_footer.then_some(section_index)
        })
    }

    fn adjacent_paragraph(&self, id: &NodeId, direction: isize) -> Option<NodeId> {
        if let Some(stream) = paragraph_stream_for_node(&self.document, id) {
            let index = stream.iter().position(|paragraph| &paragraph.id == id)?;
            let target = index as isize + direction;
            if target >= 0 && target < stream.len() as isize {
                return Some(stream[target as usize].id.clone());
            }
            // Only ordinary top-level body paragraphs continue across section
            // boundaries. Table cells and headers/footers are independent text
            // streams and must not leak arrow navigation into neighbouring cells.
            if self.locate_paragraph(id).is_none() { return None; }
        }
        let positions = paragraph_positions(&self.document);
        let index = positions.iter().position(|(_, _, paragraph)| &paragraph.id == id)?;
        let target = index as isize + direction;
        if target < 0 || target >= positions.len() as isize { return None; }
        Some(positions[target as usize].2.id.clone())
    }

    fn ordered_positions(&self, a: &DocumentPosition, b: &DocumentPosition) -> Option<(DocumentPosition, DocumentPosition)> {
        if a.node_id == b.node_id {
            return if a.byte_offset <= b.byte_offset { Some((a.clone(), b.clone())) } else { Some((b.clone(), a.clone())) };
        }
        if let Some(stream) = paragraph_stream_for_node(&self.document, &a.node_id) {
            if let (Some(ia), Some(ib)) = (
                stream.iter().position(|p| p.id == a.node_id),
                stream.iter().position(|p| p.id == b.node_id),
            ) {
                return if ia <= ib { Some((a.clone(), b.clone())) } else { Some((b.clone(), a.clone())) };
            }
        }
        // Cross-section body selections still have a deterministic document
        // order. Nested streams deliberately do not fall through to this path.
        if self.locate_paragraph(&a.node_id).is_none() || self.locate_paragraph(&b.node_id).is_none() { return None; }
        let order = paragraph_positions(&self.document);
        let ia = order.iter().position(|(_, _, p)| p.id == a.node_id)?;
        let ib = order.iter().position(|(_, _, p)| p.id == b.node_id)?;
        if ia <= ib { Some((a.clone(), b.clone())) } else { Some((b.clone(), a.clone())) }
    }

    fn clamp_offset(&self, id: &NodeId, offset: usize) -> usize {
        let Some(text) = self.paragraph_text(id) else { return 0; };
        clamp_char_boundary(&text, offset.min(text.len()))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CharacterToggle { Bold, Italic, Underline }

fn set_toggle(properties: &mut CharacterProperties, property: CharacterToggle, value: bool) {
    match property {
        CharacterToggle::Bold => properties.bold = Some(value),
        CharacterToggle::Italic => properties.italic = Some(value),
        CharacterToggle::Underline => properties.underline = Some(value),
    }
}

fn apply_format_range(paragraph: &mut Paragraph, start: usize, end: usize, property: CharacterToggle, value: bool) -> Result<(), String> {
    if start >= end { return Ok(()); }
    let (before_and_selected, after) = split_content(&paragraph.content, end)?;
    let (before, mut selected) = split_content(&before_and_selected, start)?;
    apply_toggle_to_content(&mut selected, property, value);
    let mut content = before;
    content.extend(selected);
    content.extend(after);
    paragraph.content = (normalized_content(content)).into();
    Ok(())
}

fn apply_toggle_to_content(content: &mut [Inline], property: CharacterToggle, value: bool) {
    for inline in content {
        match inline {
            Inline::Text(run) => set_toggle(&mut run.properties, property, value),
            Inline::Hyperlink(link) => apply_toggle_to_content(&mut link.content, property, value),
            _ => {}
        }
    }
}

fn insert_text_into_paragraph(
    paragraph: &mut Paragraph,
    offset: usize,
    text: &str,
    typing_properties: &CharacterProperties,
    typing_style: Option<&StyleId>,
) -> Result<(), String> {
    let (mut left, right) = split_content(&paragraph.content, offset)?;
    let run = TextRun {
        id: NodeId::new(),
        text: text.to_string(),
        style: typing_style.cloned(),
        properties: typing_properties.clone(),
    };
    left.push(Inline::Text(run));
    left.extend(right);
    paragraph.content = (normalized_content(left)).into();
    Ok(())
}

fn insert_inline_into_paragraph(paragraph: &mut Paragraph, offset: usize, inline: Inline) -> Result<(), String> {
    let (mut left, right) = split_content(&paragraph.content, offset)?;
    left.push(inline);
    left.extend(right);
    paragraph.content = (normalized_content(left)).into();
    Ok(())
}

fn delete_range_from_paragraph(paragraph: &mut Paragraph, start: usize, end: usize) -> Result<(), String> {
    if start >= end { return Ok(()); }
    let (before_and_selected, after) = split_content(&paragraph.content, end)?;
    let (mut before, _) = split_content(&before_and_selected, start)?;
    before.extend(after);
    paragraph.content = (normalized_content(before)).into();
    Ok(())
}

fn normalized_content(content: impl Into<Vec<Inline>>) -> Vec<Inline> {
    let content=content.into();
    let mut out: Vec<Inline> = Vec::new();
    for inline in content {
        match inline {
            Inline::Text(run) if run.text.is_empty() => {}
            Inline::Text(run) => {
                if let Some(Inline::Text(previous)) = out.last_mut() {
                    if previous.style == run.style && previous.properties == run.properties {
                        previous.text.push_str(&run.text);
                        continue;
                    }
                }
                out.push(Inline::Text(run));
            }
            other => out.push(other),
        }
    }
    if out.is_empty() { out.push(Inline::Text(TextRun::plain(""))); }
    out
}

fn slice_content(content: &[Inline], start: usize, end: usize) -> Result<Vec<Inline>, String> {
    if start >= end { return Ok(Vec::new()); }
    let (before_and_selected, _) = split_content(content, end)?;
    let (_, selected) = split_content(&before_and_selected, start)?;
    Ok(selected)
}

fn flat_content_len(content: &[Inline]) -> usize {
    content.iter().map(inline_flat_len).sum()
}

fn fresh_clipboard_paragraph(source: &ClipboardParagraph) -> ClipboardParagraph {
    ClipboardParagraph {
        origin: source.origin.clone(),
        style: source.style.clone(),
        properties: source.properties.clone(),
        content: (source.content.iter().cloned().map(fresh_inline_ids).collect::<Vec<_>>()).into(),
    }
}

fn fresh_inline_ids(mut inline: Inline) -> Inline {
    match &mut inline {
        Inline::Text(run) => run.id = NodeId::new(),
        Inline::Equation(eq) => eq.id = NodeId::new(),
        Inline::Image(image) => image.id = NodeId::new(),
        Inline::EmbeddedObject(object) => object.id = NodeId::new(),
        Inline::TextBox(boxed) => {
            boxed.id = NodeId::new();
            crate::clipboard::fresh_blocks(&mut boxed.blocks, &mut std::collections::BTreeMap::new());
        },
        Inline::Field(field) => field.id = NodeId::new(),
        Inline::LineBreak { id } | Inline::PaginationBreak { id, .. } | Inline::Tab { id } => *id = NodeId::new(),
        Inline::Hyperlink(link) => {
            link.id = NodeId::new();
            link.content = (link.content.drain(..).map(fresh_inline_ids).collect::<Vec<_>>()).into();
        }
    }
    inline
}

pub fn paragraph_flat_text(paragraph: &Paragraph) -> String {
    let mut out = String::new();
    append_inline_text(&paragraph.content, &mut out);
    out
}

fn append_inline_text(content: &[Inline], out: &mut String) {
    for inline in content {
        match inline {
            Inline::Text(run) => out.push_str(&run.text),
            Inline::Equation(_) | Inline::Image(_) | Inline::TextBox(_) | Inline::EmbeddedObject(_) | Inline::Field(_) => out.push_str(INLINE_OBJECT_MARKER),
            Inline::PaginationBreak { break_kind, .. } => out.push(break_kind.marker()),
            Inline::LineBreak { .. } => out.push('\n'),
            Inline::Tab { .. } => out.push('\t'),
            Inline::Hyperlink(link) => append_inline_text(&link.content, out),
        }
    }
}

/// Return the independent editable paragraph stream containing `id`.
/// A table cell, header or footer is its own stream; this prevents ordinary
/// selection/navigation from silently crossing cell boundaries.
fn paragraph_stream_in_blocks<'a>(blocks: &'a [Block], id: &NodeId) -> Option<Vec<&'a Paragraph>> {
    if blocks.iter().any(|block| matches!(block, Block::Paragraph(paragraph) if &paragraph.id == id)) {
        return Some(blocks.iter().filter_map(|block| match block {
            Block::Paragraph(paragraph) => Some(paragraph),
            _ => None,
        }).collect());
    }
    for block in blocks {
        if let Block::Paragraph(paragraph) = block {
            if let Some(stream) = paragraph_stream_in_boxes(&paragraph.content, id) { return Some(stream); }
        }
        if let Block::Table(table) = block {
            for row in &table.rows {
                for cell in &row.cells {
                    if let Some(stream) = paragraph_stream_in_blocks(&cell.blocks, id) { return Some(stream); }
                }
            }
        }
    }
    None
}

fn paragraph_stream_in_boxes<'a>(items:&'a[Inline],id:&NodeId)->Option<Vec<&'a Paragraph>>{
    for item in items {match item {
        Inline::TextBox(boxed)=>if let Some(stream)=paragraph_stream_in_blocks(&boxed.blocks,id){return Some(stream);},
        Inline::Hyperlink(link)=>if let Some(stream)=paragraph_stream_in_boxes(&link.content,id){return Some(stream);},
        _=>{}
    }}None
}

fn paragraph_stream_for_node<'a>(document: &'a Document, id: &NodeId) -> Option<Vec<&'a Paragraph>> {
    for section in &document.sections {
        if let Some(stream) = paragraph_stream_in_blocks(&section.blocks, id) { return Some(stream); }
        for header_footer in section.header_footers() {
            if let Some(stream) = paragraph_stream_in_blocks(&header_footer.stream.blocks, id) { return Some(stream); }
        }
    }
    for note in document.review.footnotes.values(){if let Some(stream)=paragraph_stream_in_blocks(&note.blocks,id){return Some(stream);}}
    None
}

fn collect_paragraph_refs<'a>(blocks: &'a [Block], out: &mut Vec<&'a Paragraph>) {
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => {out.push(paragraph);collect_box_paragraph_refs(&paragraph.content,out);},
            Block::Table(table) => for row in &table.rows { for cell in &row.cells { collect_paragraph_refs(&cell.blocks, out); } },
            _ => {}
        }
    }
}

fn collect_box_paragraph_refs<'a>(items:&'a[Inline],out:&mut Vec<&'a Paragraph>){
    for item in items{match item{
        Inline::TextBox(boxed)=>collect_paragraph_refs(&boxed.blocks,out),
        Inline::Hyperlink(link)=>collect_box_paragraph_refs(&link.content,out),
        _=>{}
    }}
}

fn delete_range_in_same_block_list(
    blocks: &mut Vec<Block>, start: &DocumentPosition, end: &DocumentPosition,
) -> Result<Option<NodeId>, String> {
    let start_index = blocks.iter().position(|block| matches!(block, Block::Paragraph(p) if p.id == start.node_id));
    let end_index = blocks.iter().position(|block| matches!(block, Block::Paragraph(p) if p.id == end.node_id));
    if let (Some(start_index), Some(end_index)) = (start_index, end_index) {
        if start_index > end_index { return Ok(None); }
        let first = match &blocks[start_index] { Block::Paragraph(p) => p.clone(), _ => unreachable!() };
        let last = match &blocks[end_index] { Block::Paragraph(p) => p.clone(), _ => unreachable!() };
        let (first_left, _) = split_content(&first.content, start.byte_offset)?;
        let (_, last_right) = split_content(&last.content, end.byte_offset)?;
        let mut merged = first.clone();
        let mut content = first_left; content.extend(last_right); merged.content = (normalized_content(content)).into();
        let id = merged.id.clone();
        blocks[start_index] = Block::Paragraph(merged);
        blocks.drain(start_index + 1..=end_index);
        return Ok(Some(id));
    }
    for block in blocks.iter_mut() {
        if let Block::Paragraph(paragraph)=block {
            if let Some(result)=delete_range_in_boxes(&mut paragraph.content,start,end)? {return Ok(Some(result));}
        }
        if let Block::Table(table) = block {
            for row in &mut table.rows {
                for cell in &mut row.cells {
                    if find_paragraph_in_blocks(&cell.blocks, &start.node_id).is_some()
                        && find_paragraph_in_blocks(&cell.blocks, &end.node_id).is_some()
                    {
                        return delete_range_in_same_block_list(&mut cell.blocks, start, end);
                    }
                }
            }
        }
    }
    Ok(None)
}

fn delete_range_in_boxes(items:&mut[Inline],start:&DocumentPosition,end:&DocumentPosition)->Result<Option<NodeId>,String>{
    for item in items{match item{
        Inline::TextBox(boxed) if find_paragraph_in_blocks(&boxed.blocks,&start.node_id).is_some()
            && find_paragraph_in_blocks(&boxed.blocks,&end.node_id).is_some()=>{
                return delete_range_in_same_block_list(&mut boxed.blocks,start,end);
            },
        Inline::Hyperlink(link)=>if let Some(result)=delete_range_in_boxes(&mut link.content,start,end)?{return Ok(Some(result));},
        _=>{}
    }}Ok(None)
}

fn delete_nested_range_in_section(
    section: &mut Section, start: &DocumentPosition, end: &DocumentPosition,
) -> Result<Option<NodeId>, String> {
    if let Some(id) = delete_range_in_same_block_list(&mut section.blocks, start, end)? { return Ok(Some(id)); }
    for stream in section.header_footers_mut() {
        if find_paragraph_in_blocks(&stream.blocks, &start.node_id).is_some()
            && find_paragraph_in_blocks(&stream.blocks, &end.node_id).is_some()
        {
            return delete_range_in_same_block_list(&mut stream.blocks, start, end);
        }
    }
    Ok(None)
}

fn merge_adjacent_in_blocks(blocks: &mut Vec<Block>, current_id: &NodeId, direction: isize) -> Option<(NodeId, usize)> {
    if let Some(current_index) = blocks.iter().position(|block| matches!(block, Block::Paragraph(p) if &p.id == current_id)) {
        if direction < 0 {
            if current_index == 0 { return None; }
            let previous_index = current_index - 1;
            if !matches!(blocks[previous_index], Block::Paragraph(_)) { return None; }
            let previous = match &blocks[previous_index] { Block::Paragraph(p) => p.clone(), _ => unreachable!() };
            let current = match &blocks[current_index] { Block::Paragraph(p) => p.clone(), _ => unreachable!() };
            let caret = paragraph_flat_text(&previous).len();
            let mut merged = previous; merged.content.extend(current.content); merged.content = (normalized_content(merged.content)).into();
            let id = merged.id.clone(); blocks[previous_index] = Block::Paragraph(merged); blocks.remove(current_index);
            return Some((id, caret));
        }
        let next_index = current_index + 1;
        if next_index >= blocks.len() || !matches!(blocks[next_index], Block::Paragraph(_)) { return None; }
        let current = match &blocks[current_index] { Block::Paragraph(p) => p.clone(), _ => unreachable!() };
        let next = match &blocks[next_index] { Block::Paragraph(p) => p.clone(), _ => unreachable!() };
        let caret = paragraph_flat_text(&current).len();
        let mut merged = current; merged.content.extend(next.content); merged.content = (normalized_content(merged.content)).into();
        let id = merged.id.clone(); blocks[current_index] = Block::Paragraph(merged); blocks.remove(next_index);
        return Some((id, caret));
    }
    for block in blocks.iter_mut() {
        if let Block::Paragraph(paragraph)=block {
            if let Some(result)=merge_adjacent_in_boxes(&mut paragraph.content,current_id,direction){return Some(result);}
        }
        if let Block::Table(table) = block {
            for row in &mut table.rows { for cell in &mut row.cells {
                if find_paragraph_in_blocks(&cell.blocks, current_id).is_some() {
                    return merge_adjacent_in_blocks(&mut cell.blocks, current_id, direction);
                }
            }}
        }
    }
    None
}

fn merge_adjacent_in_boxes(items:&mut[Inline],id:&NodeId,direction:isize)->Option<(NodeId,usize)>{
    for item in items{match item{
        Inline::TextBox(boxed) if find_paragraph_in_blocks(&boxed.blocks,id).is_some()=>
            return merge_adjacent_in_blocks(&mut boxed.blocks,id,direction),
        Inline::Hyperlink(link)=>if let Some(result)=merge_adjacent_in_boxes(&mut link.content,id,direction){return Some(result);},
        _=>{}
    }}None
}

fn merge_adjacent_in_section(section: &mut Section, current_id: &NodeId, direction: isize) -> Option<(NodeId, usize)> {
    if let Some(result) = merge_adjacent_in_blocks(&mut section.blocks, current_id, direction) { return Some(result); }
    for stream in section.header_footers_mut() {
        if find_paragraph_in_blocks(&stream.blocks, current_id).is_some() {
            return merge_adjacent_in_blocks(&mut stream.blocks, current_id, direction);
        }
    }
    None
}

fn find_paragraph_in_blocks<'a>(blocks: &'a [Block], id: &NodeId) -> Option<&'a Paragraph> {
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) if &paragraph.id == id => return Some(paragraph),
            Block::Paragraph(paragraph) => if let Some(found)=find_paragraph_in_boxes(&paragraph.content,id){return Some(found);},
            Block::Table(table) => {
                for row in &table.rows {
                    for cell in &row.cells {
                        if let Some(paragraph) = find_paragraph_in_blocks(&cell.blocks, id) { return Some(paragraph); }
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn insert_paragraph_after_in_boxes(items:&mut[Inline],id:&NodeId,paragraph:Paragraph)->bool{
    for item in items{match item{
        Inline::TextBox(boxed) if find_paragraph_in_blocks(&boxed.blocks,id).is_some()=>
            return insert_paragraph_after_in_blocks(&mut boxed.blocks,id,paragraph),
        Inline::Hyperlink(link)=>if insert_paragraph_after_in_boxes(&mut link.content,id,paragraph.clone()){return true;},
        _=>{}
    }}false
}

fn find_paragraph_in_boxes<'a>(items:&'a[Inline],id:&NodeId)->Option<&'a Paragraph>{
    for item in items{match item{
        Inline::TextBox(boxed)=>if let Some(found)=find_paragraph_in_blocks(&boxed.blocks,id){return Some(found);},
        Inline::Hyperlink(link)=>if let Some(found)=find_paragraph_in_boxes(&link.content,id){return Some(found);},
        _=>{}
    }}None
}

fn replace_paragraph_in_blocks(blocks: &mut model::Shared<Vec<Block>>, replacement: &Paragraph) -> bool {
    if find_paragraph_in_blocks(blocks,&replacement.id).is_none(){return false;}
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) if paragraph.id == replacement.id => {
                *paragraph = replacement.clone();
                return true;
            }
            Block::Paragraph(paragraph) => if replace_paragraph_in_boxes(&mut paragraph.content,replacement){return true;},
            Block::Table(table) => {
                for row in &mut table.rows {
                    for cell in &mut row.cells {
                        if replace_paragraph_in_blocks(&mut cell.blocks, replacement) { return true; }
                    }
                }
            }
            _ => {}
        }
    }
    false
}

fn replace_paragraph_in_boxes(items:&mut[Inline],replacement:&Paragraph)->bool{
    for item in items{match item{
        Inline::TextBox(boxed)=>if replace_paragraph_in_blocks(&mut boxed.blocks,replacement){return true;},
        Inline::Hyperlink(link)=>if replace_paragraph_in_boxes(&mut link.content,replacement){return true;},
        _=>{}
    }}false
}

fn insert_paragraph_after_in_blocks(blocks: &mut Vec<Block>, id: &NodeId, paragraph: Paragraph) -> bool {
    let mut index = 0usize;
    while index < blocks.len() {
        match &mut blocks[index] {
            Block::Paragraph(existing) if &existing.id == id => {
                blocks.insert(index + 1, Block::Paragraph(paragraph));
                return true;
            }
            Block::Paragraph(existing) => if insert_paragraph_after_in_boxes(&mut existing.content,id,paragraph.clone()){return true;},
            Block::Table(table) => {
                for row in &mut table.rows {
                    for cell in &mut row.cells {
                        if insert_paragraph_after_in_blocks(&mut cell.blocks, id, paragraph.clone()) { return true; }
                    }
                }
            }
            _ => {}
        }
        index += 1;
    }
    false
}

fn insert_paragraph_after_in_section(section: &mut Section, id: &NodeId, paragraph: Paragraph) -> bool {
    if insert_paragraph_after_in_blocks(&mut section.blocks, id, paragraph.clone()) { return true; }
    for header_footer in section.header_footers_mut() {
        if insert_paragraph_after_in_blocks(&mut header_footer.blocks, id, paragraph.clone()) { return true; }
    }
    false
}

fn collect_body_paragraphs<'a>(
    blocks: &'a [Block],
    section_index: usize,
    top_block_index: usize,
    out: &mut Vec<(usize, usize, &'a Paragraph)>,
) {
    for (index, block) in blocks.iter().enumerate() {
        let root_index = if top_block_index == usize::MAX { index } else { top_block_index };
        match block {
            Block::Paragraph(paragraph) => out.push((section_index, root_index, paragraph)),
            Block::Table(table) => {
                for row in &table.rows {
                    for cell in &row.cells {
                        collect_body_paragraphs(&cell.blocks, section_index, root_index, out);
                    }
                }
            }
            _ => {}
        }
    }
}

fn paragraph_positions(document: &Document) -> Vec<(usize, usize, &Paragraph)> {
    let mut out = Vec::new();
    for (section_index, section) in document.sections.iter().enumerate() {
        collect_body_paragraphs(&section.blocks, section_index, usize::MAX, &mut out);
    }
    out
}

fn first_paragraph_position(document: &Document) -> Option<DocumentPosition> {
    paragraph_positions(document).first().map(|(_, _, p)| DocumentPosition::new(p.id.clone(), 0))
}

fn ensure_editable_document(document: &mut Document) {
    if document.sections.is_empty() { document.sections.push(Default::default()); }
    if paragraph_positions(document).is_empty() {
        document.sections[0].blocks.push(Block::Paragraph(Paragraph::plain("")));
    }
}

fn apply_mutation(document: &mut Document, mutation: &Mutation, forward: bool) -> Result<(), String> {
    match mutation {
        Mutation::ContentControlsChanged{before,after}=>{if forward{after}else{before}.apply(document);Ok(())},
        Mutation::StylesChanged { before, after } => { document.styles = if forward {after.clone()} else {before.clone()}; Ok(()) },
        Mutation::AnchorsTransformed { .. } => Ok(()),
        Mutation::FootnoteChanged{id,before,after}=>{
            let target=document.review.footnotes.get_mut(id).ok_or("Undo footnote no longer exists")?;*target=if forward{after.clone()}else{before.clone()};Ok(())
        },
        Mutation::ReviewChanged { before, after } => {
            document.review = if forward { after.clone() } else { before.clone() }; Ok(())
        }
        Mutation::SectionsReplaced { index, before, after } => {
            let (remove, insert) = if forward { (before, after) } else { (after, before) };
            let end = index.checked_add(remove.len()).ok_or("Invalid section transaction")?;
            if end > document.sections.len() || document.sections[*index..end].iter().map(|s| &s.id).ne(remove.iter().map(|s| &s.id)) {
                return Err("Section transaction no longer matches the document".into());
            }
            document.sections.splice(*index..end, insert.iter().cloned());
            Ok(())
        }
        Mutation::SectionPropertiesChanged { section_id, before, after } => {
            let section = document.sections.iter_mut().find(|s| &s.id == section_id).ok_or("undo section not found")?;
            section.properties = if forward { after.clone() } else { before.clone() };
            Ok(())
        }
        Mutation::SectionChanged { section_id, before, after } => {
            let replacement = if forward { after } else { before };
            let section = document.sections.iter_mut().find(|s| &s.id == section_id).ok_or("undo section not found")?;
            *section = replacement.clone();
            Ok(())
        }
        Mutation::SectionInserted { index, section } => {
            if forward {
                document.sections.insert((*index).min(document.sections.len()), section.clone());
            } else if *index < document.sections.len() && document.sections[*index].id == section.id {
                document.sections.remove(*index);
            } else if let Some(found) = document.sections.iter().position(|candidate| candidate.id == section.id) {
                document.sections.remove(found);
            } else {
                return Err("undo inserted section not found".into());
            }
            Ok(())
        }
        Mutation::ParagraphChanged { id, before, after } => {
            let replacement = if forward { after } else { before };
            for section in &mut document.sections {
                if replace_paragraph_in_blocks(&mut section.blocks, replacement) { return Ok(()); }
                for header_footer in section.header_footers_mut() {
                    if replace_paragraph_in_blocks(&mut header_footer.blocks, replacement) { return Ok(()); }
                }
            }
            for note in document.review.footnotes.values_mut(){if replace_paragraph_in_blocks(&mut note.blocks,replacement){return Ok(());}}
            Err(format!("undo paragraph not found: {id}"))
        }
        Mutation::BlockInserted { section_id, index, block } => {
            let section = document.sections.iter_mut().find(|s| &s.id == section_id).ok_or("undo section not found")?;
            if forward {
                let insertion=(*index).min(section.blocks.len());
                section.blocks.insert(insertion, block.clone());
            } else if *index < section.blocks.len() {
                section.blocks.remove(*index);
            }
            Ok(())
        }
        Mutation::BlockRemoved { section_id, index, block } => {
            let section = document.sections.iter_mut().find(|s| &s.id == section_id).ok_or("undo section not found")?;
            if forward {
                if *index < section.blocks.len() { section.blocks.remove(*index); }
            } else {
                let insertion=(*index).min(section.blocks.len());
                section.blocks.insert(insertion, block.clone());
            }
            Ok(())
        }
        Mutation::ListInserted { id, definition } => {
            if forward { document.lists.insert(id.clone(), definition.clone()); }
            else { document.lists.remove(id); }
            Ok(())
        }
        Mutation::ResourceInserted { id, resource } => {
            if forward { document.resources.insert(id.clone(), resource.clone()); }
            else { document.resources.remove(id); }
            Ok(())
        }
    }
}

fn previous_grapheme_boundary(text: &str, offset: usize) -> usize {
    unicode_segmentation::GraphemeCursor::new(offset, text.len(), true)
        .prev_boundary(text, 0).ok().flatten().unwrap_or(0)
}

fn next_grapheme_boundary(text: &str, offset: usize) -> usize {
    unicode_segmentation::GraphemeCursor::new(offset, text.len(), true)
        .next_boundary(text, 0).ok().flatten().unwrap_or(text.len())
}

fn previous_word_start(text: &str, offset: usize) -> usize {
    text.unicode_word_indices().map(|(start, _)| start)
        .take_while(|&start| start < offset).last().unwrap_or(0)
}

fn next_word_start(text: &str, offset: usize) -> usize {
    text.unicode_word_indices().map(|(start, _)| start)
        .find(|&start| start > offset).unwrap_or(text.len())
}

#[cfg(test)] #[path = "../tests/unit/lib.rs"] mod tests;

mod tracked_changes;
