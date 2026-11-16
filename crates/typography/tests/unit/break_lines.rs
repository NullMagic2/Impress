use super::*;

#[test]
fn composition_reaches_paragraph_end() {
    let items = [
        LineItem::Box { width: 35.0 },
        LineItem::Glue { width: 5.0, stretch: 4.0, shrink: 2.0 },
        LineItem::Box { width: 35.0 },
        LineItem::Glue { width: 5.0, stretch: 4.0, shrink: 2.0 },
        LineItem::Box { width: 35.0 },
    ];
    let lines = compose(&items, ComposeOptions { line_width: 80.0, tolerance: 5.0, ..Default::default() });
    assert_eq!(lines.last().map(|b| b.item_index), Some(items.len()));
}

