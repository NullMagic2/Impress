use super::*;
#[test]
fn snapped_preview_is_the_move_and_resize_that_will_be_committed() {
    let mut drag = TableDrag {
        table: "t".into(), start_page: 0,
        resize: false,
        start: Point::new(100.0, 80.0),
        current: Point::new(104.0, 80.0),
        moved: true,
        bounds: (100.0, 80.0, 300.0, 180.0),
        indent_mm: 0.0,
        widths_pt: vec![100.0, 100.0],
        heights_pt: vec![100.0],
        declared: vec![None],
        horizontal_only: true,
        proportional: false,
        free: None,
        snapped: None,
    };
    drag.snapped = Some(crate::table_snap::moving(
        drag.ghost(),
        Rect::new(100.0, 80.0, 500.0, 700.0),
        6.0,
        false,
    ));
    assert_eq!(new_indent(&drag, 1.0), 0.0);
    assert_eq!(drag.ghost().x, 100.0);
    drag.resize = true;
    drag.snapped = Some(crate::table_snap::Snapped {
        rect: Rect::new(100.0, 80.0, 500.0, 150.0),
        x: Some(600.0),
        y: None,
        margins: Rect::new(100.0, 80.0, 500.0, 700.0),
    });
    assert_eq!(drag.factors(), (2.5, 1.5));
    assert_eq!(drag.ghost().width, 500.0);
}


#[test]
fn selected_outer_table_takes_priority_over_a_caret_inside_a_nested_table() {
    use model::{Block, Document, Paragraph, Table, TableProperties, TableRow, TableCell};
    fn table(blocks: Vec<Block>) -> Table {
        Table { id: NodeId::new(), properties: TableProperties::default(), rows: vec![TableRow {
            id: NodeId::new(), repeat_as_header: false, allow_break_across_pages: true, height_mm: None,
            cells: vec![TableCell { id: NodeId::new(), blocks: blocks.into(), column_span: 1, row_span: 1,
                width_mm: None, shading: None, borders: None, vertical_alignment: Default::default(), margins: None, text_direction: None }],
        }] }
    }
    let inside = Paragraph::plain("Inside nested table"); let caret = inside.id.clone();
    let inner = table(vec![Block::Paragraph(inside)]); let inner_id = inner.id.clone();
    let outer = table(vec![Block::Table(inner), Block::Paragraph(Paragraph::plain("Outer anchor"))]);
    let selected = objects_ui::SelectedObject::Cells { table_id: outer.id.clone(), cell_ids: vec![outer.rows[0].cells[0].id.clone()] };
    let mut document = Document::default(); document.sections[0].blocks = vec![Block::Table(outer.clone())].into();
    let mut editor = EditorState::new(document); editor.set_caret(caret, 0);
    assert_eq!(selected_context(&editor, None).unwrap().table_id, inner_id);
    assert_eq!(selected_context(&editor, Some(&selected)).unwrap().table_id, outer.id);
    assert!(editor.table_can_move(&inner_id));
    assert!(editor.table_can_move(&outer.id));
}

#[test]
fn nested_drag_uses_cell_coordinates_and_keeps_shift_vertical_position() {
    use model::{TablePosition, HorizontalFrom, HorizontalPlacement, VerticalFrom, VerticalPlacement, TableWrap};
    let original = TablePosition { vertical: VerticalPlacement::Offset(7.0), vertical_from: VerticalFrom::Paragraph,
        wrap: TableWrap::TopAndBottom, distance_left_mm: 2.5, ..Default::default() };
    let bounds = Rect::new(80.0, 100.0, 200.0, 300.0);
    let result = position_in_cell(original.clone(), bounds, Point::new(-20.0, 999.0), render::Size::new(100.0, 80.0), false);
    assert_eq!(result.horizontal_from, HorizontalFrom::Margin);
    assert_eq!(result.horizontal, HorizontalPlacement::Offset(0.0));
    assert_eq!(result.vertical_from, VerticalFrom::Margin);
    assert!(matches!(result.vertical, VerticalPlacement::Offset(y) if (y * MM_TO_PT - 220.0).abs()<0.15));
    assert_eq!(result.wrap, original.wrap); assert_eq!(result.distance_left_mm, 2.5);
    let shifted = position_in_cell(original.clone(), bounds, Point::new(120.0, 999.0), render::Size::new(100.0, 80.0), true);
    assert_eq!(shifted.vertical, original.vertical); assert_eq!(shifted.vertical_from, original.vertical_from);
    assert!(matches!(shifted.horizontal, HorizontalPlacement::Offset(x) if (x * MM_TO_PT - 40.0).abs()<0.15));
}

#[test]
fn positioning_keeps_cell_selection_and_selects_the_table_from_a_caret() {
    let table = NodeId::new(); let a = NodeId::new(); let b = NodeId::new(); let removed = NodeId::new();
    let selected = objects_ui::SelectedObject::Cells { table_id: table.clone(), cell_ids: vec![b.clone(), removed] };
    assert_eq!(positioned_selection(&table, &[a.clone(), b.clone()], Some(&selected)),
        objects_ui::SelectedObject::Cells { table_id: table.clone(), cell_ids: vec![b.clone()] });
    assert_eq!(positioned_selection(&table, &[a.clone(), b.clone()], None),
        objects_ui::SelectedObject::Cells { table_id: table, cell_ids: vec![a, b] });
}
