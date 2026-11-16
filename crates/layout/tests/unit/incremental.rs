use super::*;
fn paragraph(height:f32,tag:u8,keep:bool)->PreparedBlock {
    PreparedBlock::Paragraph{lines:vec![PreparedLine{pagination_break:None,height,ascent:height*0.75,
        commands:vec![DrawCommand::FillRect{rect:Rect::new(0.0,0.0,tag as f32+1.0,1.0),paint:Paint::Solid(Color::BLACK)}],hit_regions:vec![]}],
        space_before:0.0,space_after:0.0,left_indent:0.0,marker:None,decoration:None,keep_with_next:keep,keep_lines_together:false,page_break_before:false,widow_control:false}
}
fn fixture(blocks:&[PreparedBlock],keys:Vec<Vec<u8>>,g:&SectionGeometry)->Entry {
    let (pages,cursor,boundaries)=resume(blocks,&keys,g,None,true,None,None,&mut LayoutWork::default()).unwrap();
    Entry{environment:Environment{key:vec![],resources:vec![]},keys,pages,cursor,boundaries:Some(boundaries),prepared:None,bytes:0,last_used:0}
}
fn verify(blocks:&[PreparedBlock],keys:&[Vec<u8>],old:&Entry,g:&SectionGeometry)->LayoutWork {
    let mut work=LayoutWork::default();let (pages,cursor,_)=resume(blocks,keys,g,None,true,Some(old),None,&mut work).unwrap();
    let (expected,expected_cursor)=paginate_prepared(blocks,g,None,true,None).unwrap();
    assert_eq!(pages,expected,"incremental geometry differs from cold pagination");assert_eq!(cursor,expected_cursor);work
}
#[test] fn seeded_edit_sequence_matches_cold_commands_hits_and_objects(){
    fn rand(seed:&mut u64)->u64{*seed=seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);*seed}
    fn block(seed:&mut u64,serial:usize)->PreparedBlock{
        let r=rand(seed);let height=12.0+(r%35)as f32;
        if r%13==0{return PreparedBlock::PageBreak;}
        if r%11==0{return PreparedBlock::Image(PreparedImage{id:format!("image-{serial}"),width:50.0,height,alignment:ImageAlignment::Left,
            commands:vec![DrawCommand::FillRect{rect:Rect::new(0.0,0.0,50.0,height),paint:Paint::Solid(Color::BLACK)}]});}
        let mut p=paragraph(height,(serial%250)as u8,r%7==0);
        if let PreparedBlock::Paragraph{lines,widow_control,page_break_before,keep_lines_together,..}=&mut p{
            *widow_control=r%3==0;*page_break_before=r%17==0;*keep_lines_together=r%5==0;
            let mut line=lines[0].clone();line.hit_regions=vec![TextHitRegion{node_id:format!("p-{serial}"),visual_line:0,byte_range:0..2,rect:Rect::new(0.0,0.0,12.0,height),caret_stops:vec![CaretStop{byte_offset:0,x_pt:0.0},CaretStop{byte_offset:2,x_pt:12.0}]}];
            *lines=(0..1+(r%5)).map(|i|{let mut l=line.clone();l.hit_regions[0].visual_line=i as usize;l}).collect();
        }p
    }
    let g=SectionGeometry::new(&Section::default());let mut seed=42;
    let mut blocks=(0..60).map(|i|block(&mut seed,i)).collect::<Vec<_>>();
    let mut keys=(0usize..60).map(|i|i.to_le_bytes().to_vec()).collect::<Vec<_>>();let mut old=fixture(&blocks,keys.clone(),&g);
    for serial in 60usize..360{
        let at=rand(&mut seed)as usize%blocks.len();
        match rand(&mut seed)%3{
            0 if blocks.len()>20=>{blocks.remove(at);keys.remove(at);},
            1 if blocks.len()<100=>{blocks.insert(at,block(&mut seed,serial));keys.insert(at,serial.to_le_bytes().to_vec());},
            _=>{blocks[at]=block(&mut seed,serial);keys[at]=serial.to_le_bytes().to_vec();}
        }
        verify(&blocks,&keys,&old,&g);
        let(pages,cursor,bounds)=resume(&blocks,&keys,&g,None,true,Some(&old),None,&mut LayoutWork::default()).unwrap();
        old=Entry{environment:old.environment,keys:keys.clone(),pages,cursor,boundaries:Some(bounds),prepared:None,bytes:0,last_used:0};
    }
}
