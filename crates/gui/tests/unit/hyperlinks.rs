use super::*;
use model::{Block,Hyperlink,Inline,Paragraph,TextRun};

fn document()->(Document,String,usize,usize) {
    let mut document=Document::default();
    let mut paragraph=Paragraph::plain("");let id=paragraph.id.0.clone();
    let prefix="Before ";let label="Ελληνικά";
    paragraph.content=vec![Inline::Text(TextRun::plain(prefix)),Inline::Hyperlink(Hyperlink {
        id:NodeId::new(),target:"https://example.org/real-target".into(),
        content:vec![Inline::Text(TextRun::plain(label))].into(),
    }),Inline::Text(TextRun::plain(" after"))].into();
    document.sections[0].blocks=vec![Block::Paragraph(paragraph)].into();
    (document,id,prefix.len(),prefix.len()+label.len())
}
fn point(scene:&render::VectorDocument,viewport:&ViewportState,id:&str,offset:usize,fraction:f32)->Point {
    for (page,p) in scene.pages.iter().enumerate(){for region in &p.hit_regions {
        if region.node_id!=id{continue;}
        if let Some(pair)=region.caret_stops.windows(2).find(|p|p[0].byte_offset==offset){
            let x=pair[0].x_pt+(pair[1].x_pt-pair[0].x_pt)*fraction;
            return viewport.page_point_to_view(scene,page,Point::new(x,region.rect.y+region.rect.height/2.0)).unwrap();
        }
    }}
    panic!("No rendered glyph at {offset}");
}
#[test]
fn greek_link_includes_the_trailing_half_of_its_last_character_but_not_neighbors() {
    let(doc,id,start,end)=document();
    let scene=layout::LayoutEngine::default().layout_document(&doc).unwrap();
    let viewport=ViewportState::default();
    for offset in [start,end-2] {
        let p=point(&scene,&viewport,&id,offset,0.85);
        assert_eq!(target_at(&doc,&scene,&viewport,p).as_deref(),Some("https://example.org/real-target"));
    }
    for offset in [start-1,end,end+1] {
        assert_eq!(target_at(&doc,&scene,&viewport,point(&scene,&viewport,&id,offset,0.85)),None);
    }
    let p=point(&scene,&viewport,&id,start,0.5);
    assert_eq!(target_at(&doc,&scene,&viewport,Point::new(p.x,p.y+50.0)),None,"blank paper must not snap to a URL");
}
#[test]
fn wrapped_links_in_table_cells_work_after_zoom_and_scrolling() {
    let(mut doc,id,start,_)=document();
    let paragraph=doc.sections[0].blocks[0].clone();
    let Block::Paragraph(mut p)=paragraph else{unreachable!()};
    let Inline::Hyperlink(h)=&mut p.content[1]else{unreachable!()};
    let label="Ελληνικά ".repeat(40);let label=label.trim_end();
    h.content=vec![Inline::Text(TextRun::plain(label))].into();
    let label_end=start+label.len();
    doc.sections[0].blocks=vec![Block::Table(model::Table{id:NodeId::new(),properties:Default::default(),rows:vec![model::TableRow{
        id:NodeId::new(),cells:vec![model::TableCell{id:NodeId::new(),blocks:vec![Block::Paragraph(p)].into(),column_span:1,row_span:1,width_mm:None,shading:None,borders:None,vertical_alignment:Default::default()}],repeat_as_header:false,allow_break_across_pages:true,height_mm:None,
    }]})].into();
    let scene=layout::LayoutEngine::default().layout_document(&doc).unwrap();
    let viewport=ViewportState{dpi:144.0,zoom:1.7,scroll_px:Point::new(17.0,40.0),..Default::default()};
    assert_eq!(target_at(&doc,&scene,&viewport,point(&scene,&viewport,&id,label_end-2,0.8)).as_deref(),Some("https://example.org/real-target"));
}
#[test]
fn url_validation_keeps_email_and_web_urls_and_rejects_commands() {
    for target in ["https://example.org/a?q=b#part","HTTP://example.org","mailto:person@example.org"] {assert_eq!(browser_url(target).unwrap(),target);}
    for target in ["javascript:alert(1)","data:text/html,test","file:///tmp/program","https://example.org/\n--flag",""] {assert!(browser_url(target).is_err());}
}
