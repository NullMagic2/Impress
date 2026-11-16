//! Cached fields use regional theme fonts while remaining one atomic editing slot.
use super::*;
#[test]
fn mixed_cached_field_fonts_split_paint_commands_but_keep_one_hit_region() {
    let mut document = Document::default();
    let normal = document
        .styles
        .styles
        .get_mut(&model::StyleId::from(model::STYLE_NORMAL))
        .unwrap();
    normal.character.font_slots = Some(model::FontSlots {
        ascii: Some(model::FontSlot::explicit("Linux Libertine G")),
        h_ansi: Some(model::FontSlot::explicit("DejaVu Sans")),
        ..Default::default()
    });
    let mut paragraph = Paragraph::plain("");
    let id = paragraph.id.0.clone();
    paragraph.content = vec![Inline::Field(model::Field {
        id: NodeId::new(),
        kind: FieldKind::Raw {
            instruction: "CACHE".into(),
        },
        display_text: Some("Latin αβ".into()),
        color: None,
    })]
    .into();
    document.sections[0].blocks = vec![Block::Paragraph(paragraph)].into();
    let vector = LayoutEngine::default().layout_document(&document).unwrap();
    let fields = vector.pages[0]
        .commands
        .iter()
        .filter_map(|command| {
            if let DrawCommand::TextRun(run) = command.unclipped().0 {
                run.actual_text
                    .as_ref()
                    .map(|text| (text.as_str(), run.font_family.as_str()))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert!(
        fields
            .iter()
            .any(|(text, family)| text.contains("Latin") && *family == "Linux Libertine G"),
        "{fields:?}"
    );
    assert!(fields
        .iter()
        .any(|(text, family)| text.contains("αβ") && *family == "DejaVu Sans"));
    let hits = vector.pages[0]
        .hit_regions
        .iter()
        .filter(|hit| hit.node_id == id)
        .collect::<Vec<_>>();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].byte_range, 0..3);
    assert_eq!(hits[0].caret_stops.len(), 2);
}

#[test]
fn overwide_cached_field_wraps_inside_its_cell_and_keeps_atomic_source_positions() {
    let display="00:05:00 - 00:06:00 fi e\u{301} αβ";
    let mut p=Paragraph::plain("");let paragraph_id=p.id.0.clone();
    p.content=vec![Inline::Field(model::Field{id:NodeId::new(),kind:FieldKind::Raw{instruction:"CACHE".into()},display_text:Some(display.into()),color:Some("#1155CC".into())})].into();
    let cell=model::TableCell{id:NodeId::new(),blocks:vec![Block::Paragraph(p)].into(),column_span:1,row_span:1,width_mm:None,shading:None,borders:None,vertical_alignment:Default::default()};
    let cell_id=cell.id.0.clone();let mut document=Document::default();
    document.sections[0].blocks=vec![Block::Table(Table{id:NodeId::new(),properties:model::TableProperties{preferred_width_mm:Some(24.0),..Default::default()},rows:vec![model::TableRow{id:NodeId::new(),cells:vec![cell],repeat_as_header:false,allow_break_across_pages:true,height_mm:None}]})].into();
    let before=document.clone();let vector=LayoutEngine::default().layout_document(&document).unwrap();assert_eq!(document,before);
    let page=&vector.pages[0];let cell=page.object_regions.iter().find(|r|r.node_id==cell_id).unwrap();
    let painted=page.commands.iter().filter_map(|c|if let DrawCommand::TextRun(run)=c.unclipped().0 {Some(run)}else{None}).collect::<Vec<_>>();
    assert_eq!(painted.iter().filter_map(|r|r.actual_text.as_deref()).collect::<String>(),display);
    for run in painted {assert!(run.bounds.x>=cell.rect.x-0.01 && run.bounds.max_x()<=cell.rect.max_x()+0.01,"field text must wrap, not be hidden by clipping");}
    let hits=page.hit_regions.iter().filter(|h|h.node_id==paragraph_id).collect::<Vec<_>>();
    assert!(hits.iter().map(|h|h.visual_line).max().unwrap()>0);
    for hit in hits {assert_eq!(hit.byte_range,0..3);assert!(hit.caret_stops.iter().all(|s|s.byte_offset==0 || s.byte_offset==3));}
}
