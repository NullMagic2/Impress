//! Live body/selection statistics. Unchanged paragraphs retain their counts;
//! scrolling, caret blinking and selection changes never recount the body.
use super::navigation::{self, TextStats};
use editor::{EditorState, Selection};
use model::{Block, Inline, NodeId, Paragraph, Shared};
use std::collections::{HashMap, HashSet};

struct ParagraphCount {
    content: Shared<Vec<Inline>>,
    stats: TextStats,
    bytes: usize,
    index: usize,
}

type SelectionKey = (Option<Selection>, Option<Vec<NodeId>>);

#[derive(Default)]
pub(super) struct State {
    revision: Option<(u64, u64)>,
    paragraphs: HashMap<NodeId, ParagraphCount>,
    selection: Option<SelectionKey>,
    pub stats: TextStats,
    pub selected: Option<usize>,
}

fn paragraphs(blocks: &[Block], visit: &mut impl FnMut(&Paragraph)) {
    for block in blocks {
        match block {
            Block::Paragraph(p) => visit(p),
            Block::Table(t) => for row in &t.rows { for cell in &row.cells { paragraphs(&cell.blocks, visit); } },
            _ => {}
        }
    }
}

impl State {
    pub fn update(&mut self, editor: &EditorState, epoch: u64, cells: Option<&[NodeId]>) {
        if self.revision != Some((epoch, editor.revision)) {
            let mut live = HashSet::new();
            self.stats = TextStats::default();
            for section in &editor.document.sections {
                paragraphs(&section.blocks, &mut |p| {
                    live.insert(p.id.clone());
                    let cached = self.paragraphs.entry(p.id.clone()).or_insert_with(|| ParagraphCount {
                        content: p.content.clone(), stats: navigation::count_paragraph(p),
                        bytes: p.content.iter().map(editor::inline_flat_len).sum(), index: 0,
                    });
                    if !cached.content.shares_storage(&p.content) {
                        *cached = ParagraphCount { content: p.content.clone(), stats: navigation::count_paragraph(p),
                            bytes: p.content.iter().map(editor::inline_flat_len).sum(), index: 0 };
                    }
                    cached.index = self.stats.paragraphs;
                    self.stats.words += cached.stats.words;
                    self.stats.characters += cached.stats.characters;
                    self.stats.paragraphs += cached.stats.paragraphs;
                });
            }
            self.paragraphs.retain(|id, _| live.contains(id));
            self.revision = Some((epoch, editor.revision));
            self.selection = None;
        }
        let key = (editor.selection.as_ref().filter(|s| !s.is_collapsed()).cloned(), cells.map(<[_]>::to_vec));
        if self.selection.as_ref() == Some(&key) { return; }
        self.selected = if let Some(cells) = cells {
            let selected: HashSet<_> = cells.iter().collect();
            let mut words = 0;
            fn walk(blocks: &[Block], selected: &HashSet<&NodeId>, counts: &HashMap<NodeId, ParagraphCount>, words: &mut usize) {
                for block in blocks { if let Block::Table(t) = block {
                    for row in &t.rows { for cell in &row.cells {
                        if selected.contains(&cell.id) {
                            paragraphs(&cell.blocks, &mut |p| { *words += counts.get(&p.id).map_or(0, |c| c.stats.words); });
                        } else { walk(&cell.blocks, selected, counts, words); }
                    } }
                } }
            }
            for section in &editor.document.sections { walk(&section.blocks, &selected, &self.paragraphs, &mut words); }
            Some(words)
        } else if let Some(selection) = &key.0 {
            let a = self.paragraphs.get(&selection.anchor.node_id);
            let b = self.paragraphs.get(&selection.focus.node_id);
            if let (Some(a), Some(b)) = (a, b) {
                let a = (a.index, selection.anchor.byte_offset);
                let b = (b.index, selection.focus.byte_offset);
                let (start, end) = (a.min(b), a.max(b));
                Some(self.paragraphs.iter().filter(|(_, p)| p.index >= start.0 && p.index <= end.0)
                    .map(|(id, p)| {
                        let lo = if p.index == start.0 { start.1 } else { 0 };
                        let hi = if p.index == end.0 { end.1 } else { p.bytes };
                        if lo == 0 && hi == p.bytes { p.stats.words }
                        else { editor.paragraph_text(id).and_then(|text| text.get(lo..hi).map(navigation::count_words)).unwrap_or(0) }
                    }).sum())
            } else {
                // Headers/footnotes are independent editing streams, outside
                // the body total. Their selections still have useful counts.
                editor.selected_text().map(|s| navigation::count_words(&s))
            }
        } else { None };
        self.selection = Some(key);
    }

    pub fn label(&self) -> String {
        match self.selected {
            Some(selected) if self.stats.words == 1 => format!("{selected} of 1 word"),
            Some(selected) => format!("{selected} of {} words", self.stats.words),
            None if self.stats.words == 1 => "1 word".into(),
            None => format!("{} words", self.stats.words),
        }
    }
}

#[cfg(test)] #[path = "../../tests/unit/ribbon/word_count.rs"] mod tests;
