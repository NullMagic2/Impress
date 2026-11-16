//! Unicode bidirectional helpers (UAX #9).

use unicode_bidi::{get_base_direction, BidiInfo, Direction};

use crate::TextDirection;

/// Resolved embedding levels indexed by UTF-8 byte offset.
pub fn bidi_levels(text: &str) -> Vec<u8> {
    if text.is_empty() { return Vec::new(); }
    let info = BidiInfo::new(text, None);
    info.levels.iter().map(|level| level.number()).collect()
}

pub fn bidi_level_at(text: &str, byte_offset: usize) -> u8 {
    let levels = bidi_levels(text);
    if levels.is_empty() { return 0; }
    let index = byte_offset.min(levels.len().saturating_sub(1));
    levels[index]
}

pub fn base_text_direction(text: &str) -> TextDirection {
    match get_base_direction(text) {
        Direction::Rtl => TextDirection::RightToLeft,
        Direction::Ltr | Direction::Mixed => TextDirection::LeftToRight,
    }
}

pub fn direction_for_level(level: u8) -> TextDirection {
    if level % 2 == 1 { TextDirection::RightToLeft } else { TextDirection::LeftToRight }
}

/// UAX #9 rule L2 reordering for a sequence whose resolved level is already
/// known. Returns logical indices in visual order.
pub fn visual_order(levels: &[u8]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..levels.len()).collect();
    let Some(max_level) = levels.iter().copied().max() else { return order; };
    let Some(min_odd) = levels.iter().copied().filter(|level| level % 2 == 1).min() else { return order; };

    for threshold in (min_odd..=max_level).rev() {
        let mut cursor = 0usize;
        while cursor < order.len() {
            while cursor < order.len() && levels[order[cursor]] < threshold { cursor += 1; }
            let start = cursor;
            while cursor < order.len() && levels[order[cursor]] >= threshold { cursor += 1; }
            if start < cursor { order[start..cursor].reverse(); }
        }
    }
    order
}

#[cfg(test)] #[path = "../tests/unit/bidi.rs"] mod tests;
