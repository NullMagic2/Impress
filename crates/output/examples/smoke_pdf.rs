//! Run with `cargo run -p output --example smoke_pdf -- output.pdf`.
//! Synthetic vector glyphs and pixels: no installed/distributed font is needed.
//! This exercises the Rust writer (unlike the separate Python wire-format oracle).
use render::*;use output::{pdf::write_pdf,links::{Link,Target}};
use std::{sync::{Arc,atomic::AtomicBool},io::Write};
fn main()->Result<(),Box<dyn std::error::Error>>{
 let path=std::env::args_os().nth(1).ok_or("Supply output.pdf")?;let mut page=VectorPage::new(Size::new(200.0,300.0));
 page.commands.push(DrawCommand::FillRect{rect:Rect::new(10.0,10.0,30.0,20.0),paint:Paint::Solid(Color::rgb8(220,30,30))});
 let outline=VectorPath{verbs:vec![PathVerb::MoveTo(Point::new(0.0,0.0)),PathVerb::LineTo(Point::new(10.0,-24.0)),PathVerb::LineTo(Point::new(20.0,0.0)),PathVerb::LineTo(Point::new(16.0,0.0)),PathVerb::LineTo(Point::new(13.0,-8.0)),PathVerb::LineTo(Point::new(7.0,-8.0)),PathVerb::LineTo(Point::new(4.0,0.0)),PathVerb::Close]};
 page.commands.push(DrawCommand::TextRun(std::sync::Arc::new(VectorTextRun{actual_text:Some("A".into()),source:None,font_family:"Synthetic test outline".into(),font_size_pt:24.0,fill:Paint::Solid(Color::BLACK),glyphs:vec![GlyphVector{glyph_id:1,outline:std::sync::Arc::new(outline),transform:Affine2D::translation(20.0,75.0),cluster:Some(0)}],bounds:Rect::new(20.0,51.0,24.0,30.0),advance_pt:24.0})));
 let image=Arc::new(RasterImage::from_rgba(2,2,&[255,0,0,255,0,255,0,255,0,0,255,255,255,255,0,128])?);
 page.commands.push(DrawCommand::Image{rect:Rect::new(80.0,50.0,80.0,80.0),image});
 let scene=VectorDocument{pages:vec![page,VectorPage::new(Size::new(300.0,200.0))], ..Default::default()};
 let links=vec![Link{page:0,rect:Rect::new(20.0,51.0,24.0,30.0),target:Target::Uri("https://example.com/".into())}];
 let mut file=std::fs::File::create(&path)?;write_pdf(&scene,&[0,1],"Impress alpha.14 output conformance",&links,&AtomicBool::new(false),&mut file).map_err(std::io::Error::other)?;file.flush()?;
 println!("Wrote {}: 2 pages, selectable synthetic A, vector rectangle, RGBA image, link. This is a conformance fixture, not an application screenshot.",std::path::Path::new(&path).display());Ok(())
}
