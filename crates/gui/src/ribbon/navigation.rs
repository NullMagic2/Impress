//! Navigation works with semantic offsets, never widget text offsets.
use model::{Block, Document, NodeId};
use editor::{paragraph_flat_text, INLINE_OBJECT_MARKER};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Debug)]
pub struct Entry {
    pub label: String,
    pub node: NodeId,
    pub start: usize,
    pub end: usize,
}

/// Native list population is capped; the search report retains every bounded match.
pub fn entries(document: &Document, query: &str, cap: usize) -> (Vec<Entry>, bool) {
    let mut all = if query.is_empty() {
        editor::search::outline(document).into_iter().map(|(node,level,text)| Entry {
            label:format!("{}{}", "  ".repeat(level.saturating_sub(1) as usize),preview(&text)),node,start:0,end:0,
        }).collect::<Vec<_>>()
    } else {
        editor::search::find(document,0,query,Default::default()).map(|r|r.matches.into_iter().map(|m|Entry {
            label:format!("{} · {}",m.story.label(),m.preview),node:m.node,start:m.range.start,end:m.range.end,
        }).collect()).unwrap_or_default()
    };
    let truncated=all.len()>cap;all.truncate(cap);(all,truncated)
}

fn preview(text: &str) -> String {
    let mut short = text.replace(INLINE_OBJECT_MARKER, "◊").replace(['\n', '\t'], " ").chars().take(90).collect::<String>();
    if text.chars().count() > 90 { short.push('…'); }
    if short.trim().is_empty() { "(Empty heading)".to_string() } else { short }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextStats { pub words: usize, pub characters: usize, pub paragraphs: usize }

pub fn count_words(text: &str) -> usize {
    text.unicode_words().count()
}

pub fn count_paragraph(paragraph: &model::Paragraph) -> TextStats {
    let text = paragraph_flat_text(paragraph);
    TextStats { words: count_words(&text),
        characters: text.chars().filter(|&c| c != '\u{fffc}').count(),
        paragraphs: 1 }
}

pub fn count_body(document: &Document) -> TextStats {
    fn visit(blocks: &[Block], result: &mut TextStats) {
        for block in blocks {
            match block {
                Block::Paragraph(p) => {
                    let stats = count_paragraph(p);
                    result.words += stats.words;
                    result.characters += stats.characters;
                    result.paragraphs += stats.paragraphs;
                }
                Block::Table(table) => for row in &table.rows { for cell in &row.cells { visit(&cell.blocks, result); } },
                _ => {}
            }
        }
    }
    let mut result = TextStats::default();
    for section in &document.sections { visit(&section.blocks, &mut result); }
    result
}

