use super::*;
fn prepared(spec:&[(NodeId,Vec<f32>)])->PreparedNotes{
    let fragments=spec.iter().map(|(id,heights)|{
        let marker=PreparedMarker{commands:Vec::new(),width:0.0,gap:0.0};
        (id.clone(),PreparedNote{atoms:heights.iter().map(|h|PreparedCellFragment{height:*h,commands:Vec::new(),hit_regions:Vec::new(),object_regions:Vec::new()}).collect(),first:marker.clone(),continued:marker})
    }).collect();PreparedNotes{references:Vec::new(),fragments}
}
#[test]fn long_note_places_every_line_once_in_order(){
    let id=NodeId::new();let p=prepared(&[(id.clone(),vec![20.0;100])]);let g=SectionGeometry::new(&Section::default());
    let plan=p.plan(&[vec![id.clone()]],&g).unwrap();assert!(plan.pages.len()>1);
    let lines=plan.pages.iter().flatten().flat_map(|piece|piece.start..piece.end).collect::<Vec<_>>();assert_eq!(lines,(0..100).collect::<Vec<_>>());
    assert!(plan.reserves.iter().all(|h|*h<=g.column_height()*0.60+0.01));
}
