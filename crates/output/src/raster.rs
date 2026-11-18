//! Printer bands are rendered from authoring paths at output DPI, not captured
//! from the monitor. Each band includes the full compositing order, then is
//! flattened onto white. A band is limited to 64 MiB; images retain source detail.
use render::*;
use std::{collections::BTreeMap,sync::atomic::{AtomicBool,Ordering}};
fn byte(v:f32)->u8{(v.clamp(0.0,1.0)*255.0).round()as u8}
fn gamma(v:f32)->f32{if v<=0.0031308{12.92*v}else{1.055*v.powf(1.0/2.4)-0.055}}
pub fn straight_rgba(image:&RasterImage)->Result<Vec<u8>,String>{
 let size=(image.width as usize).checked_mul(image.height as usize).and_then(|v|v.checked_mul(4)).ok_or("Image size overflow")?;
 if size>128*1024*1024||image.pixels.len()<size{return Err("Invalid output raster size".into());}let mut out=Vec::with_capacity(size);
 for p in image.pixels[..size].chunks_exact(4){let a=p[3]as f32/255.0;for c in &p[..3]{out.push(if a==0.0{0}else{byte(gamma(srgb_to_linear(*c as f32/255.0)/a))});}out.push(p[3]);}Ok(out)
}
fn transform(a:Affine2D)->tiny_skia::Transform{tiny_skia::Transform::from_row(a.sx,a.ky,a.kx,a.sy,a.tx,a.ty)}
fn path(p:&VectorPath)->Option<tiny_skia::Path>{let mut b=tiny_skia::PathBuilder::new();for v in &p.verbs{match *v{PathVerb::MoveTo(p)=>b.move_to(p.x,p.y),PathVerb::LineTo(p)=>b.line_to(p.x,p.y),PathVerb::QuadTo{ctrl,to}=>b.quad_to(ctrl.x,ctrl.y,to.x,to.y),PathVerb::CubicTo{ctrl1,ctrl2,to}=>b.cubic_to(ctrl1.x,ctrl1.y,ctrl2.x,ctrl2.y,to.x,to.y),PathVerb::Close=>b.close()}}b.finish()}
fn paint(p:&Paint)->Result<tiny_skia::Paint<'static>,String>{let Paint::Solid(c)=p;let mut p=tiny_skia::Paint::default();p.set_color(tiny_skia::Color::from_rgba(c.r,c.g,c.b,c.a).ok_or("Invalid output color")?);p.anti_alias=true;Ok(p)}
#[derive(Default)]pub struct BandRenderer{images:BTreeMap<u64,tiny_skia::Pixmap>,image_bytes:usize}
impl BandRenderer{
 pub fn render(&mut self,page:&VectorPage,width:u32,height:u32,matrix:Affine2D,cancel:&AtomicBool)->Result<Vec<u8>,String>{
  crate::pdf::validate_page(page)?;let bytes=(width as usize).checked_mul(height as usize).and_then(|v|v.checked_mul(4)).ok_or("Printer band overflow")?;
  if bytes>64*1024*1024{return Err("Printer band exceeds 64 MiB".into());}let mut dest=tiny_skia::Pixmap::new(width,height).ok_or("Could not allocate printer band")?;dest.fill(tiny_skia::Color::WHITE);
  let mut mask_cache:Option<(Rect,tiny_skia::Mask)>=None;
  for source in &page.commands{let (command,clip)=source.unclipped();
   if clip.is_some_and(Rect::is_empty){continue;}
   if let Some(rect)=clip {
    if mask_cache.as_ref().is_none_or(|(r,_)|*r!=rect){
     let mut mask=tiny_skia::Mask::new(width,height).ok_or("Could not allocate clipping mask")?;
     let r=tiny_skia::Rect::from_xywh(rect.x,rect.y,rect.width,rect.height).ok_or("Invalid clip")?;
     let path=tiny_skia::PathBuilder::from_rect(r);
     mask.fill_path(&path,tiny_skia::FillRule::Winding,true,transform(matrix));mask_cache=Some((rect,mask));
    }
   }
   let mask=clip.and_then(|_|mask_cache.as_ref().map(|(_,m)|m));
if cancel.load(Ordering::Relaxed){return Err("Printing cancelled".into());}match command{
   DrawCommand::Clipped{..} | DrawCommand::EncodedImage{..} => unreachable!("resolved scene command"),
   DrawCommand::FillRect{rect,paint:p}=>{if let Some(r)=tiny_skia::Rect::from_xywh(rect.x,rect.y,rect.width,rect.height){dest.fill_rect(r,&paint(p)?,transform(matrix),mask);}},
   DrawCommand::Path(obj)=>{if let Some(p)=path(&obj.path){let t=transform(obj.transform.then(matrix));if let Some(fill)=&obj.fill{dest.fill_path(&p,&paint(fill)?,tiny_skia::FillRule::Winding,t,mask);}if let Some((color,style))=&obj.stroke{dest.stroke_path(&p,&paint(color)?,&tiny_skia::Stroke{width:style.width_pt,..Default::default()},t,mask);}}},
   DrawCommand::TextRun(run)=>{let p=paint(&run.fill)?;for g in &run.glyphs{if let Some(path)=path(&g.outline){dest.fill_path(&path,&p,tiny_skia::FillRule::Winding,transform(g.transform.then(matrix)),mask);}}},
   DrawCommand::Image{rect,image}=>{
    if !self.images.contains_key(&image.key){let rgba=straight_rgba(image)?;if self.image_bytes+rgba.len()>256*1024*1024{self.images.clear();self.image_bytes=0;}
     let mut pix=tiny_skia::Pixmap::new(image.width,image.height).ok_or("Could not allocate print image")?;for (dst,src) in pix.data_mut().chunks_exact_mut(4).zip(rgba.chunks_exact(4)){for c in 0..3{dst[c]=((src[c]as u32*src[3]as u32+127)/255)as u8;}dst[3]=src[3];}self.image_bytes+=rgba.len();self.images.insert(image.key,pix);
    }let pix=&self.images[&image.key];let t=Affine2D::scale(rect.width/image.width as f32,rect.height/image.height as f32).then(Affine2D::translation(rect.x,rect.y)).then(matrix);
    dest.draw_pixmap(0,0,pix.as_ref(),&tiny_skia::PixmapPaint{quality:tiny_skia::FilterQuality::Bilinear,..Default::default()},transform(t),mask);
   }
  }}let mut out=dest.take();for p in out.chunks_exact_mut(4){p.swap(0,2);}Ok(out) // BGRA, opaque top-down DIB
 }
}

