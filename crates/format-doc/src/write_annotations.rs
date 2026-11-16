//! Native bookmark and note/comment PLC encodings in physical UTF16 CP space.
use super::properties::{long, word};
use model::*;
use std::collections::BTreeMap;

pub(super) type Anchors = BTreeMap<NodeId, BTreeMap<usize, u32>>;
pub(super) fn cp(anchors: &Anchors, anchor: &TextAnchor) -> Option<u32> {
    anchors.get(&anchor.paragraph)?.get(&anchor.offset).copied()
}
pub(super) fn bookmarks(
    document: &Document,
    anchors: &Anchors,
    limit: u32,
) -> Vec<(usize, Vec<u8>)> {
    let mut records = document
        .review
        .bookmarks
        .values()
        .filter_map(|bookmark| {
            let (start, end) = bookmark
                .range
                .as_ref()
                .map(|range| Some((cp(anchors, &range.start)?, cp(anchors, &range.end)?)))
                .unwrap_or(Some((0, 0)))?;
            Some((start, end, bookmark.name.clone()))
        })
        .collect::<Vec<_>>();
    records.sort_by_key(|(start, end, _)| (*start, *end));
    if records.is_empty() {
        return Vec::new();
    }
    let mut names = Vec::new();
    word(&mut names, 0xFFFF);
    word(&mut names, records.len() as u16);
    word(&mut names, 0);
    for (_, _, name) in &records {
        word(&mut names, name.encode_utf16().count() as u16);
        for unit in name.encode_utf16() {
            word(&mut names, unit);
        }
    }
    let mut ends = records
        .iter()
        .enumerate()
        .map(|(index, (_, end, _))| (*end, index))
        .collect::<Vec<_>>();
    ends.sort_by_key(|(end, index)| (*end, *index));
    let end_indices = ends
        .iter()
        .enumerate()
        .map(|(index, (_, source))| (*source, index))
        .collect::<BTreeMap<_, _>>();
    let mut starts = Vec::new();
    for (start, _, _) in &records {
        long(&mut starts, *start);
    }
    long(&mut starts, limit);
    for index in 0..records.len() {
        word(&mut starts, end_indices[&index] as u16);
        word(&mut starts, 0);
    }
    let mut ending = Vec::new();
    for (end, _) in ends {
        long(&mut ending, end);
    }
    long(&mut ending, limit);
    vec![(21, names), (22, starts), (23, ending)]
}
pub(super) fn note_tables(
    references: &[(u32, NodeId)],
    text_cps: &[u32],
    kind: NoteKind,
    document: &Document,
    limit: u32,
) -> Vec<(usize, Vec<u8>)> {
    let refs = references
        .iter()
        .filter(|(_, id)| {
            document
                .review
                .footnotes
                .get(id)
                .is_some_and(|note| note.kind == kind)
        })
        .collect::<Vec<_>>();
    if refs.is_empty() {
        return Vec::new();
    }
    let mut reference_bytes = Vec::new();
    for (cp, _) in &refs {
        long(&mut reference_bytes, *cp);
    }
    long(&mut reference_bytes, limit);
    for _ in &refs {
        word(&mut reference_bytes, 1);
    }
    let mut text_bytes = Vec::new();
    for cp in text_cps {
        long(&mut text_bytes, *cp);
    }
    let index = if kind == NoteKind::Footnote { 2 } else { 46 };
    vec![(index, reference_bytes), (index + 1, text_bytes)]
}
pub(super) fn comments(
    document: &Document,
    anchors: &Anchors,
    references: &[(u32, NodeId)],
    text_cps: &[u32],
    limit: u32,
) -> Vec<(usize, Vec<u8>)> {
    if references.is_empty() {
        return Vec::new();
    }
    let mut owners = Vec::<String>::new();
    let mut refs = Vec::new();
    for (position, _) in references {
        long(&mut refs, *position);
    }
    long(&mut refs, limit);
    let mut ranges = Vec::new();
    for (index, (_, id)) in references.iter().enumerate() {
        let comment = &document.review.comments[id];
        let owner = owners
            .iter()
            .position(|owner| owner == &comment.author)
            .unwrap_or_else(|| {
                owners.push(comment.author.clone());
                owners.len() - 1
            });
        let mut record = [0u8; 30];
        let initials = comment
            .author
            .split_whitespace()
            .filter_map(|word| word.chars().next())
            .take(9)
            .collect::<String>();
        let units = initials.encode_utf16().take(9).collect::<Vec<_>>();
        record[0..2].copy_from_slice(&(units.len() as u16).to_le_bytes());
        for (offset, unit) in units.iter().enumerate() {
            record[2 + offset * 2..4 + offset * 2].copy_from_slice(&unit.to_le_bytes());
        }
        record[20..22].copy_from_slice(&(owner as u16).to_le_bytes());
        let range = comment
            .range
            .as_ref()
            .and_then(|range| Some((cp(anchors, &range.start)?, cp(anchors, &range.end)?)));
        if let Some((start, end)) = range {
            ranges.push((start, end, index));
            record[26..30].copy_from_slice(&(index as u32).to_le_bytes());
        } else {
            record[26..30].copy_from_slice(&u32::MAX.to_le_bytes());
        }
        refs.extend(record);
    }
    let mut text = Vec::new();
    for cp in text_cps {
        long(&mut text, *cp);
    }
    let mut owner_bytes = Vec::new();
    for owner in owners {
        word(&mut owner_bytes, owner.encode_utf16().count() as u16);
        for unit in owner.encode_utf16() {
            word(&mut owner_bytes, unit);
        }
    }
    let mut parts = vec![(4, refs), (5, text), (36, owner_bytes)];
    if ranges.is_empty() {
        return parts;
    }
    ranges.sort_by_key(|(start, end, index)| (*start, *end, *index));
    let mut names = Vec::new();
    word(&mut names, 0xFFFF);
    word(&mut names, ranges.len() as u16);
    word(&mut names, 10);
    for (_, _, index) in &ranges {
        word(&mut names, 0);
        word(&mut names, 0x0100);
        long(&mut names, *index as u32);
        long(&mut names, u32::MAX);
    }
    let mut ends = ranges
        .iter()
        .enumerate()
        .map(|(index, (_, end, _))| (*end, index))
        .collect::<Vec<_>>();
    ends.sort_by_key(|(end, index)| (*end, *index));
    let end_indices = ends
        .iter()
        .enumerate()
        .map(|(index, (_, source))| (*source, index))
        .collect::<BTreeMap<_, _>>();
    let mut starts = Vec::new();
    for (start, _, _) in &ranges {
        long(&mut starts, *start);
    }
    long(&mut starts, limit);
    for index in 0..ranges.len() {
        word(&mut starts, end_indices[&index] as u16);
        word(&mut starts, 0);
    }
    let mut ending = Vec::new();
    for (end, _) in ends {
        long(&mut ending, end);
    }
    long(&mut ending, limit);
    parts.extend([(37, names), (42, starts), (43, ending)]);
    parts
}
