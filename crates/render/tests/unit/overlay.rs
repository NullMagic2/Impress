use super::*;
use crate::{CaretStop, Size, TextHitRegion, VectorPage};

fn region(node: &str, start: usize, end: usize, y: f32) -> TextHitRegion {
    TextHitRegion {
        node_id: node.into(),
        visual_line: 0,
        byte_range: start..end,
        rect: Rect::new(10.0, y, 40.0, 12.0),
        caret_stops: vec![
            CaretStop { byte_offset: start, x_pt: 10.0 },
            CaretStop { byte_offset: end, x_pt: 50.0 },
        ],
    }
}

#[test]
fn caret_line_affinity_disambiguates_soft_wrap_boundary() {
    let mut page = VectorPage::new(Size::new(100.0, 100.0));
    let mut first = region("p", 0, 5, 20.0);
    first.visual_line = 0;
    let mut second = region("p", 5, 10, 40.0);
    second.visual_line = 1;
    page.hit_regions.push(first);
    page.hit_regions.push(second);
    let doc = VectorDocument { pages: vec![page], ..Default::default() };
    let pos = SourcePosition { node_id: "p".into(), byte_offset: 5 };
    let selection = SourceSelection::collapsed(pos);
    let overlay = overlay_for_selection_with_caret_line(&doc, Some(&selection), 0.8, Some(1));
    assert_eq!(overlay.caret.unwrap().rect.y, 40.0);
}

#[test]
fn multi_paragraph_selection_paints_all_nodes() {
    let mut page = VectorPage::new(Size::new(100.0, 100.0));
    page.hit_regions.push(region("a", 0, 5, 20.0));
    page.hit_regions.push(region("b", 0, 5, 40.0));
    let doc = VectorDocument { pages: vec![page], ..Default::default() };
    let selection = SourceSelection {
        anchor: SourcePosition { node_id: "a".into(), byte_offset: 2 },
        focus: SourcePosition { node_id: "b".into(), byte_offset: 3 },
    };
    let overlay = overlay_for_selection(&doc, Some(&selection), 0.8);
    assert_eq!(overlay.selections.len(), 2);
}

