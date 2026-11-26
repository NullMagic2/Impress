//! Proofing changes use the word's runs, never the current typing format.
use super::*;
use std::ops::Range;

impl EditorState {
    /// Replace a checked word without resetting its character style or direct
    /// formatting. UTF-8 ranges and the expected revision must still match.
    pub fn replace_spelling_word(
        &mut self,
        node: &NodeId,
        range: Range<usize>,
        expected: &str,
        replacement: &str,
        revision: u64,
    ) -> Result<(), String> {
        if revision != self.revision {
            return Err("The text changed. Close this window and check spelling again.".into());
        }
        if replacement.len() > 1024 || replacement.contains(['\0', '\r', '\n', '\u{fffc}']) {
            return Err("Spelling replacements must be single-line text, at most 1024 bytes.".into());
        }
        let before = self.paragraph(node).cloned().ok_or("Spelling paragraph no longer exists")?;
        let text = paragraph_flat_text(&before);
        if range.is_empty() || text.get(range.clone()) != Some(expected) {
            return Err("The text changed. Close this window and check spelling again.".into());
        }
        if self.document.review.automatic_contents && self.document.review.contents_paragraphs.contains(node) {
            return Err("Automatic contents are derived. Correct the source heading instead.".into());
        }
        if expected == replacement { return Ok(()); }

        // Keep unchanged letters in their original runs. Compare graphemes so
        // accents and joined characters are never split by the minimal edit.
        let old: Vec<&str> = expected.graphemes(true).collect();
        let new: Vec<&str> = replacement.graphemes(true).collect();
        let prefix = old.iter().zip(&new).take_while(|(a,b)| a == b).count();
        let suffix = old[prefix..].iter().rev().zip(new[prefix..].iter().rev())
            .take_while(|(a,b)| a == b).count();
        let start = range.start + old[..prefix].iter().map(|g|g.len()).sum::<usize>();
        let end = range.end - old[old.len()-suffix..].iter().map(|g|g.len()).sum::<usize>();
        let inserted = new[prefix..new.len()-suffix].concat();
        // For an insertion, inherit the left letter within this word, or its
        // first letter when inserting at the start. Never use a preceding word.
        let at = if start == end && start > range.start { start-1 } else { start };
        let template = run_at(&before.content, at).cloned().ok_or("The spelling range contains no text")?;
        let mut after = before.clone();
        replace_runs(&mut after.content, start, end, &inserted, &template)?;
        let mutations = vec![
            Mutation::AnchorsTransformed { mapping: anchor_maps::AnchorMapping::Replace {
                paragraph: node.clone(), start, end, inserted: inserted.len(),
            }, suppress: 1 },
            Mutation::ParagraphChanged { id: node.clone(), before, after },
        ];
        // Stage through the normal transaction machinery, including locked and
        // bound controls, review records, anchor transforms and undo/redo.
        let selection = self.selection.clone();
        let mut staged = self.clone();
        for mutation in &mutations { apply_mutation(&mut staged.document, mutation, true)?; }
        staged.set_caret(node.clone(), range.start + replacement.len());
        staged.commit("Correct spelling", mutations, selection, None)?;
        *self = staged;
        Ok(())
    }
}

fn run_at(items: &[Inline], mut at: usize) -> Option<&TextRun> {
    for item in items {
        let length = inline_flat_len(item);
        if at < length {
            return match item {
                Inline::Text(run) => Some(run),
                Inline::Hyperlink(link) => run_at(&link.content, at),
                _ => None,
            };
        }
        at -= length;
    }
    None
}

fn replace_runs(items: &mut Vec<Inline>, start: usize, end: usize, text: &str, template: &TextRun) -> Result<(), String> {
    fn contains_run(item: &Inline, id: &NodeId) -> bool {
        match item {
            Inline::Text(run) => run.id == *id,
            Inline::Hyperlink(link) => link.content.iter().any(|item| contains_run(item, id)),
            _ => false,
        }
    }
    let mut offset = 0;
    for item in items.iter_mut() {
        let length = inline_flat_len(item);
        if start >= offset && end <= offset + length
            && (start != end || contains_run(item, &template.id)) {
            if let Inline::Hyperlink(link) = item {
                return replace_runs(&mut link.content, start-offset, end-offset, text, template);
            }
            break;
        }
        offset += length;
    }
    let (left, suffix) = split_content(items, end)?;
    let (mut prefix, _) = split_content(&left, start)?;
    if !text.is_empty() {
        let mut run = template.clone();
        run.id = NodeId::new();
        run.text = text.into();
        prefix.push(Inline::Text(run));
    }
    prefix.extend(suffix);
    *items = normalized_content(prefix);
    Ok(())
}
