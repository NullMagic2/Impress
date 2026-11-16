//! Run on the build machine; no precomputed timings or bundled font files.
use model::{Block,Document,NodeId,Paragraph,Table,TableProperties,TableRow,TableCell,CellVerticalAlignment};
use layout::{LayoutEngine,LayoutOptions,LayoutScheduler,PreparedLayout};
use std::{sync::Arc,time::{Instant,Duration}};
fn wait(s:&LayoutScheduler,revision:u64)->Result<PreparedLayout,String>{let start=Instant::now();loop{if let Some(r)=s.try_recv(){if r.revision==revision{return r.layout;}}if start.elapsed()>Duration::from_secs(180){return Err("Layout timed out".into());}std::thread::sleep(Duration::from_millis(1));}}
fn main()->Result<(),Box<dyn std::error::Error>>{
 let long="werrrrqqwwwwwwqqqqqqssssssaaaaaa".repeat(80);
 let mut d=Document::default();d.sections[0].blocks=vec![Block::Paragraph(Paragraph::plain(&long)),Block::Table(Table{id:NodeId::new(),properties:TableProperties::default(),rows:vec![TableRow{id:NodeId::new(),repeat_as_header:false,allow_break_across_pages:true,height_mm:None,cells:vec![TableCell{id:NodeId::new(),blocks:vec![Block::Paragraph(Paragraph::plain(&long))].into(),column_span:1,row_span:1,width_mm:None,shading:None,borders:None,vertical_alignment:CellVerticalAlignment::Top}]}]})].into();
 let worker=LayoutScheduler::new(LayoutOptions::default())?;let start=Instant::now();worker.submit(1,Arc::new(d.clone()));let first=wait(&worker,1)?;println!("Cold: {:?}, {} pages",start.elapsed(),first.document.pages.len());
 for revision in 2..=11 {if let Block::Paragraph(p)=&mut d.sections[0].blocks[0]{*p=Paragraph{content:vec![model::Inline::Text(model::TextRun::plain(format!("{long}{}","s".repeat(revision as usize))))].into(),..p.clone()};}let start=Instant::now();worker.submit(revision,Arc::new(d.clone()));let warm=wait(&worker,revision)?;println!("Edit {revision}: {:?}, {} reused meshes",start.elapsed(),warm.reused_pages);if revision==11 {let cold=LayoutEngine::default().layout_document(&d)?;assert_eq!(*warm.document,cold,"warm and cold layout differ");for page in &cold.pages{for hit in &page.hit_regions{assert!(hit.rect.x>=-0.02&&hit.rect.max_x()<=page.size.width+0.02);assert!(hit.rect.y>=-0.02&&hit.rect.max_y()<=page.size.height+0.02);}}}}
 println!("PASS: all text hit bounds on paper and final warm scene equals cold. Compare timings locally; these are not Windows results supplied with the archive.");Ok(())
}
