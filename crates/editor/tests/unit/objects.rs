use super::*;
#[test] fn live_equation_edits_are_one_undo_step() {
    let mut d=Document::default();let mut p=Paragraph::plain("");let id=p.id.clone();
    let eq=Equation::inline(model::EquationNode::ident("x"));let eid=eq.id.clone();p.content=(vec![Inline::Equation(eq.clone())]).into();d.sections[0].blocks.push(Block::Paragraph(p));
    let mut e=EditorState::new(d);e.set_caret(id,0);
    for n in ["1","12","123"] { e.update_equation_live(&eid,Equation::block(model::EquationNode::number(n))).unwrap(); }
    let now=e.equation_by_id(&eid).unwrap();assert_eq!(now.root,model::EquationNode::number("123"));assert_eq!(now.display,model::EquationDisplay::Block);
    assert!(e.undo());assert_eq!(e.equation_by_id(&eid),Some(eq.clone()));assert!(!e.can_undo());
    assert!(e.redo());e.end_equation_edit();
    e.update_equation_live(&eid,Equation::inline(model::EquationNode::number("4"))).unwrap();
    assert!(e.undo());assert_eq!(e.equation_by_id(&eid).unwrap().root,model::EquationNode::number("123"),"a new editing session is its own step");
}

#[test]
fn crop_shape_edits_inside_table_cells_preserve_other_effects_and_undo() {
    use model::{ImageEffects, ImageShape, InlineImage, Resource, ResourceId, TableCell, TableProperties, TableRow};
    let mut document = Document::default();
    let resource = ResourceId::new();
    document.resources.insert(resource.clone(), Resource { id: resource.clone(), media_type: "image/png".into(), data: vec![137,80,78,71,13,10,26,10].into(), original_name: None, font_faces: Vec::new() });
    let picture = NodeId::new();
    let effects = ImageEffects { soft_edge_mm: Some(1.0), ..Default::default() };
    let mut paragraph = Paragraph::plain("");
    paragraph.content = vec![Inline::Image(InlineImage { id: picture.clone(), resource, width_mm: 30.0, height_mm: 30.0, alt_text: None, effects: effects.clone() })].into();
    document.sections[0].blocks = vec![Block::Table(Table {
        id: NodeId::new(), properties: TableProperties::default(), rows: vec![TableRow {
            id: NodeId::new(), repeat_as_header: false, allow_break_across_pages: true, height_mm: None,
            cells: vec![TableCell { id: NodeId::new(), blocks: vec![Block::Paragraph(paragraph)].into(), column_span: 1, row_span: 1, width_mm: None, shading: None, borders: None, vertical_alignment: Default::default(), margins: None, text_direction: None }],
        }],
    })].into();
    let mut editor = EditorState::new(document);
    let shape = ImageShape { preset: "heart".into(), ..Default::default() };
    editor.set_picture_shape(&picture, shape.clone()).unwrap();
    assert_eq!(editor.picture_shape(&picture), Some(&shape));
    assert!(editor.undo());
    assert_eq!(editor.picture_shape(&picture), Some(&ImageShape::default()));
    assert!(editor.redo());
    assert_eq!(editor.picture_shape(&picture), Some(&shape));
    assert!(editor.set_picture_shape(&picture, ImageShape { preset: "invalid shape".into(), ..Default::default() }).is_err());
}
