//! Executable native measurement, not precomputed benchmark results. It compares
//! a warmed edit against cold layout on the machine's installed fonts.
use model::{Block,Document,Inline,Paragraph};
use layout::{LayoutEngine,LayoutOptions,LayoutScheduler,PreparedLayout};
use std::{sync::Arc,time::{Instant,Duration}};
fn wait(s:&LayoutScheduler,revision:u64)->Result<PreparedLayout,String>{
    let start=Instant::now();loop{
        if let Some(result)=s.try_recv(){if result.revision==revision{return result.layout;}}
        if start.elapsed()>Duration::from_secs(180){return Err("Layout did not finish within the benchmark limit".into());}
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn main()->Result<(),Box<dyn std::error::Error>>{
    let n=std::env::args().nth(1).map(|s|s.parse::<usize>()).transpose()?.unwrap_or(300);
    if !(20..=5000).contains(&n){return Err("Pass a paragraph count between 20 and 5000".into());}
    let mut document=Document::default();
    for i in 0..n{document.sections[0].blocks.push(Block::Paragraph(Paragraph::plain(format!("Paragraph {i}: a stable line of text for pagination."))));}
    let scheduler=LayoutScheduler::new(LayoutOptions::default())?;
    let start=Instant::now();scheduler.submit(1,Arc::new(document.clone()));let first=wait(&scheduler,1)?;
    println!("initial: {:?}; {} pages; {:?}",start.elapsed(),first.document.pages.len(),first.work);
    let start=Instant::now();let snapshot=document.clone();println!("semantic snapshot capture: {:?}",start.elapsed());
    assert!(snapshot.sections[0].blocks.shares_storage(&document.sections[0].blocks));
    if let Block::Paragraph(p)=&mut document.sections[0].blocks[n/2]{if let Inline::Text(t)=&mut p.content[0]{t.text=t.text.replace("stable","edited");}}
    let start=Instant::now();scheduler.submit(2,Arc::new(document.clone()));let warm=wait(&scheduler,2)?;
    println!("edited: {:?}; {:?}; {} reused page meshes",start.elapsed(),warm.work,warm.reused_pages);
    let start=Instant::now();let cold=LayoutEngine::default().layout_document(&document)?;
    println!("cold reference: {:?}",start.elapsed());
    let comparison=Instant::now();
    if *warm.document!=cold{return Err("Incremental and cold scenes differ".into());}
    println!("full scene equality: {:?}",comparison.elapsed());
    println!("PASS: full commands, hit geometry and object regions equal cold layout. Timings are local observations, not a general speedup claim.");Ok(())
}
