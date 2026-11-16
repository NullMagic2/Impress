//! Splitting a paragraph's inline content at a byte offset of its flattened
//! text, where every inline object (picture, equation, field…) counts as one
//! object-replacement character. The editor splits paragraphs here, and the
//! layout engine re-fits the rest of a paragraph that continues in a column
//! of a different width.
use crate::{Inline, NodeId};

/// Stand-in for an inline object in the flattened paragraph text.
pub const INLINE_OBJECT_MARKER: &str = "\u{FFFC}";

/// Length of `inline` in the flattened paragraph text, in UTF-8 bytes.
pub fn inline_flat_len(inline: &Inline) -> usize {
    match inline {
        Inline::Text(run) => run.text.len(),
        Inline::Equation(_) | Inline::Image(_) | Inline::TextBox(_) | Inline::EmbeddedObject(_) | Inline::Field(_) => INLINE_OBJECT_MARKER.len(),
        Inline::LineBreak { .. } | Inline::PaginationBreak { .. } | Inline::Tab { .. } => 1,
        Inline::Hyperlink(link) => link.content.iter().map(inline_flat_len).sum(),
    }
}

/// `offset` moved back to the nearest character boundary of `text`.
pub fn clamp_char_boundary(text: &str, mut offset: usize) -> usize {
    while offset > 0 && !text.is_char_boundary(offset) { offset -= 1; }
    offset
}

/// The content before and after `offset` (clamped to the content's length).
/// Text runs and hyperlinks are split; other inline objects are atomic.
pub fn split_content(content: &[Inline], offset: usize) -> Result<(Vec<Inline>, Vec<Inline>), String> {
    let total: usize = content.iter().map(inline_flat_len).sum();
    let offset = offset.min(total);
    let mut left = Vec::new();
    let mut right = Vec::new();
    let mut cursor = 0usize;
    let mut split_done = false;

    for inline in content {
        if split_done {
            right.push(inline.clone());
            continue;
        }
        let len = inline_flat_len(inline);
        let end = cursor + len;
        if offset <= cursor {
            right.push(inline.clone());
            split_done = true;
        } else if offset >= end {
            left.push(inline.clone());
        } else {
            split_inline(inline, offset - cursor, &mut left, &mut right)?;
            split_done = true;
        }
        cursor = end;
    }
    Ok((left, right))
}

fn split_inline(inline: &Inline, local: usize, left: &mut Vec<Inline>, right: &mut Vec<Inline>) -> Result<(), String> {
    match inline {
        Inline::Text(run) => {
            let cut = clamp_char_boundary(&run.text, local.min(run.text.len()));
            if cut > 0 {
                let mut l = run.clone();
                l.text = run.text[..cut].to_string();
                left.push(Inline::Text(l));
            }
            if cut < run.text.len() {
                let mut r = run.clone();
                r.id = NodeId::new();
                r.text = run.text[cut..].to_string();
                right.push(Inline::Text(r));
            }
        }
        Inline::Hyperlink(link) => {
            let (l_content, r_content) = split_content(&link.content, local)?;
            if !l_content.is_empty() {
                let mut l = link.clone();
                l.content = (l_content).into();
                left.push(Inline::Hyperlink(l));
            }
            if !r_content.is_empty() {
                let mut r = link.clone();
                r.id = NodeId::new();
                r.content = (r_content).into();
                right.push(Inline::Hyperlink(r));
            }
        }
        _ => {
            // Equations/tabs/line breaks are atomic. A malformed offset inside
            // one snaps to the nearest edge rather than splitting the object.
            if local * 2 < inline_flat_len(inline) { right.push(inline.clone()); }
            else { left.push(inline.clone()); }
        }
    }
    Ok(())
}
