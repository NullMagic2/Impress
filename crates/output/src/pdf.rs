//! PDF 1.7 writer. Type-3 outline subsets preserve the exact shaped appearance
//! without distributing font binaries. ToUnicode and ActualText preserve logical
//! prose. Text selection is at shaped-run granularity; equations stay vector paths.
//! This is not tagged PDF, PDF/A or an archival/color-managed export.
use render::*;
use std::{collections::BTreeMap,io::Write,sync::atomic::{AtomicBool,Ordering}};
use flate2::{Compression,write::ZlibEncoder};
use crate::links::{Link,Target};
const MAX_BYTES:usize=512*1024*1024;
const MAX_OBJECTS:usize=1_000_000;
type Result<T>=std::result::Result<T,String>;
#[derive(Default)] struct File{objects:Vec<Vec<u8>>,bytes:usize}
impl File{
 fn reserve(&mut self)->Result<usize>{if self.objects.len()>=MAX_OBJECTS{return Err("PDF object limit reached".into());}self.objects.push(Vec::new());Ok(self.objects.len())}
 fn put(&mut self,id:usize,data:Vec<u8>)->Result<()>{let old=self.objects.get(id-1).ok_or("Invalid PDF object")?.len();self.bytes=self.bytes.checked_sub(old).and_then(|n|n.checked_add(data.len())).ok_or("PDF size overflow")?;if self.bytes>MAX_BYTES{return Err("PDF output exceeds 512 MiB".into());}self.objects[id-1]=data;Ok(())}
 fn obj(&mut self,s:impl Into<Vec<u8>>)->Result<usize>{let id=self.reserve()?;self.put(id,s.into())?;Ok(id)}
 fn stream(&mut self,dict:&str,bytes:&[u8])->Result<usize>{if bytes.len()>MAX_BYTES{return Err("PDF stream limit".into());}let mut z=ZlibEncoder::new(Vec::new(),Compression::default());z.write_all(bytes).map_err(|e|e.to_string())?;let data=z.finish().map_err(|e|e.to_string())?;let mut out=format!("<< {dict} /Length {} /Filter /FlateDecode >>\nstream\n",data.len()).into_bytes();out.extend(data);out.extend_from_slice(b"\nendstream");self.obj(out)}
 fn finish(self,catalog:usize,info:usize,mut out:impl Write)->Result<()>{
  let mut pos=0u64;let mut offsets=vec![0u64];
  fn emit(out:&mut impl Write,pos:&mut u64,b:&[u8])->Result<()>{out.write_all(b).map_err(|e|e.to_string())?;*pos+=b.len()as u64;Ok(())}
  emit(&mut out,&mut pos,b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")?;
  for (i,o) in self.objects.iter().enumerate(){if o.is_empty(){return Err("Unresolved PDF object".into());}offsets.push(pos);emit(&mut out,&mut pos,format!("{} 0 obj\n",i+1).as_bytes())?;emit(&mut out,&mut pos,o)?;emit(&mut out,&mut pos,b"\nendobj\n")?;}
  let xref=pos;emit(&mut out,&mut pos,format!("xref\n0 {}\n0000000000 65535 f \n",offsets.len()).as_bytes())?;
  for n in offsets.iter().skip(1){if *n>9_999_999_999{return Err("PDF offset exceeds classic xref capacity".into());}emit(&mut out,&mut pos,format!("{n:010} 00000 n \n").as_bytes())?;}
  emit(&mut out,&mut pos,format!("trailer\n<< /Size {} /Root {catalog} 0 R /Info {info} 0 R >>\nstartxref\n{xref}\n%%EOF\n",offsets.len()).as_bytes())?;Ok(())
 }
}
fn n(v:f32)->Result<String>{if !v.is_finite()||v.abs()>1.0e8{return Err("Non-finite or excessive page coordinate".into());}Ok(format!("{v:.5}"))}
fn hex(bytes:&[u8])->String{bytes.iter().map(|b|format!("{b:02X}")).collect()}
fn unicode(s:&str,bom:bool)->String{let mut b=if bom{vec![0xfe,0xff]}else{vec![]};for c in s.encode_utf16(){b.extend(c.to_be_bytes());}hex(&b)}
fn rectangle(r:Rect)->Result<String>{Ok(format!("{} {} {} {} re\n",n(r.x)?,n(r.y)?,n(r.width)?,n(r.height)?))}
pub(crate) fn validate_page(page:&VectorPage)->Result<()>{if page.size.width<=0.0||page.size.height<=0.0||page.size.width>14_400.0||page.size.height>14_400.0{return Err("Invalid or excessive physical page dimensions".into());}n(page.size.width)?;n(page.size.height)?;if page.commands.len()>2_000_000{return Err("Page command limit reached".into());}Ok(())}
fn path(p:&VectorPath,t:Affine2D)->Result<String>{
 if p.verbs.len()>4_000_000{return Err("Vector path limit reached".into());}let mut s=String::new();let(mut at,mut start)=(Point::default(),Point::default());
 for verb in &p.verbs{match *verb{
  PathVerb::MoveTo(v)=>{at=t.map_point(v);start=at;s+=&format!("{} {} m\n",n(at.x)?,n(at.y)?);},
  PathVerb::LineTo(v)=>{at=t.map_point(v);s+=&format!("{} {} l\n",n(at.x)?,n(at.y)?);},
  PathVerb::QuadTo{ctrl,to}=>{let c=t.map_point(ctrl);let end=t.map_point(to);let a=Point::new(at.x+2.0*(c.x-at.x)/3.0,at.y+2.0*(c.y-at.y)/3.0);let b=Point::new(end.x+2.0*(c.x-end.x)/3.0,end.y+2.0*(c.y-end.y)/3.0);s+=&format!("{} {} {} {} {} {} c\n",n(a.x)?,n(a.y)?,n(b.x)?,n(b.y)?,n(end.x)?,n(end.y)?);at=end;},
  PathVerb::CubicTo{ctrl1,ctrl2,to}=>{let a=t.map_point(ctrl1);let b=t.map_point(ctrl2);at=t.map_point(to);s+=&format!("{} {} {} {} {} {} c\n",n(a.x)?,n(a.y)?,n(b.x)?,n(b.y)?,n(at.x)?,n(at.y)?);},PathVerb::Close=>{s+="h\n";at=start;}
 }}Ok(s)
}
fn matrix(t:Affine2D)->Result<String>{Ok(format!("{} {} {} {} {} {} cm\n",n(t.sx)?,n(t.ky)?,n(t.kx)?,n(t.sy)?,n(t.tx)?,n(t.ty)?))}
#[derive(Default)]struct Fonts{groups:Vec<FontGroup>}
struct FontGroup{id:usize,chars:Vec<(String,usize,f32)>,bbox:[f32;4]}
impl Fonts{
 fn text(&mut self,f:&mut File,run:&VectorTextRun)->Result<String>{
  let text=run.actual_text.as_deref().unwrap_or("");if text.encode_utf16().count()>16_000{return Err("PDF text run exceeds Unicode mapping bound".into());}
  let x=run.bounds.x;let y=run.bounds.y+run.font_size_pt;
  let tr=Affine2D{sx:1.0,sy:-1.0,tx:-x,ty:y,..Affine2D::IDENTITY};
  // Charproc uses y-up glyph coordinates; its text matrix cancels the y-down page CTM.
  let mut body=String::new();let mut box_= [0.0,-run.bounds.height,run.advance_pt.max(1.0),run.font_size_pt];
  for g in &run.glyphs{let t=g.transform.then(tr);for v in &g.outline.verbs{let pts:Vec<Point>=match *v{PathVerb::MoveTo(p)|PathVerb::LineTo(p)=>vec![p],PathVerb::QuadTo{ctrl,to}=>vec![ctrl,to],PathVerb::CubicTo{ctrl1,ctrl2,to}=>vec![ctrl1,ctrl2,to],PathVerb::Close=>vec![]};for p in pts{let p=t.map_point(p);box_[0]=box_[0].min(p.x);box_[1]=box_[1].min(p.y);box_[2]=box_[2].max(p.x);box_[3]=box_[3].max(p.y);}}body+=&path(&g.outline,t)?;body+="f\n";}
  let proc=format!("{} 0 {} {} {} {} d1\n{body}",n(run.advance_pt)?,n(box_[0])?,n(box_[1])?,n(box_[2])?,n(box_[3])?);
  if self.groups.last().is_none_or(|g|g.chars.len()==255){self.groups.push(FontGroup{id:f.reserve()?,chars:vec![],bbox:[0.0;4]});}
  let gi=self.groups.len()-1;let group=&mut self.groups[gi];let code=group.chars.len()+1;let obj=f.stream("",proc.as_bytes())?;
  group.chars.push((text.to_owned(),obj,run.advance_pt));for i in 0..2{group.bbox[i]=group.bbox[i].min(box_[i]);}for i in 2..4{group.bbox[i]=group.bbox[i].max(box_[i]);}
  Ok(format!("/Span << /ActualText <{}> >> BDC\nBT /F{} 1 Tf 1 0 0 -1 {} {} Tm <{:02X}> Tj ET\nEMC\n",unicode(text,true),gi,n(x)?,n(y)?,code))
 }
 fn finish(self,f:&mut File)->Result<String>{let mut res=String::new();for (gi,g) in self.groups.into_iter().enumerate(){
  let mut cmap=String::from("/CIDInit /ProcSet findresource begin\n12 dict begin begincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /ImpressUnicode def\n/CMapType 2 def\n1 begincodespacerange\n<01> <FF>\nendcodespacerange\n");
  for (ci,chunk) in g.chars.chunks(100).enumerate(){cmap+=&format!("{} beginbfchar\n",chunk.len());for (j,(text,_,_)) in chunk.iter().enumerate(){let u=if text.is_empty(){"200B".into()}else{unicode(text,false)};cmap+=&format!("<{:02X}> <{u}>\n",ci*100+j+1);}cmap+="endbfchar\n";}cmap+="endcmap CMapName currentdict /CMap defineresource pop end end\n";
  let map=f.stream("",cmap.as_bytes())?;let procs=g.chars.iter().enumerate().map(|(i,(_,id,_))|format!("/g{} {id} 0 R",i+1)).collect::<Vec<_>>().join(" ");let names=(1..=g.chars.len()).map(|i|format!("/g{i}")).collect::<Vec<_>>().join(" ");let widths=g.chars.iter().map(|(_,_,w)|n(*w)).collect::<Result<Vec<_>>>()?.join(" ");let bbox=g.bbox.into_iter().map(n).collect::<Result<Vec<_>>>()?.join(" ");
  f.put(g.id,format!("<< /Type /Font /Subtype /Type3 /Name /F{gi} /FontBBox [{bbox}] /FontMatrix [1 0 0 1 0 0] /CharProcs << {procs} >> /Encoding << /Type /Encoding /Differences [1 {names}] >> /FirstChar 1 /LastChar {} /Widths [{widths}] /Resources << >> /ToUnicode {map} 0 R >>",g.chars.len()).into_bytes())?;res+=&format!("/F{gi} {} 0 R ",g.id);
 }Ok(res)}
}
fn color(s:&mut String,paint:&Paint,stroke:bool,f:&mut File,states:&mut BTreeMap<u32,usize>)->Result<()>{let Paint::Solid(c)=paint;for v in [c.r,c.g,c.b,c.a]{if !v.is_finite()||!(0.0..=1.0).contains(&v){return Err("Invalid paint color".into());}}
 let key=c.a.to_bits();let id=if let Some(id)=states.get(&key){*id}else{let id=f.obj(format!("<< /Type /ExtGState /ca {} /CA {} >>",n(c.a)?,n(c.a)?))?;states.insert(key,id);id};let _=id;
 *s+=&format!("/G{key} gs {} {} {} {}\n",n(c.r)?,n(c.g)?,n(c.b)?,if stroke{"RG"}else{"rg"});Ok(())}
fn image(f:&mut File,image:&RasterImage)->Result<usize>{
 let rgba=crate::raster::straight_rgba(image)?;let count=(image.width as usize)*(image.height as usize);let mut rgb=Vec::with_capacity(count*3);let mut alpha=Vec::with_capacity(count);let mut translucent=false;
 for p in rgba.chunks_exact(4){rgb.extend_from_slice(&p[..3]);alpha.push(p[3]);translucent|=p[3]!=255;}
 let mask=if translucent{let id=f.stream(&format!("/Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceGray /BitsPerComponent 8",image.width,image.height),&alpha)?;format!("/SMask {id} 0 R")}else{String::new()};
 f.stream(&format!("/Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Interpolate true {mask}",image.width,image.height),&rgb)
}
/// Document information dictionary entries. Empty fields are omitted.
#[derive(Clone,Debug,Default,PartialEq)]
pub struct PdfInfo{pub title:String,pub author:String,pub subject:String,pub keywords:String}
pub fn write_pdf(document:&VectorDocument,pages:&[usize],title:&str,links:&[Link],cancel:&AtomicBool,out:impl Write)->Result<()>{
 write_pdf_with(document,pages,&PdfInfo{title:title.into(),..Default::default()},links,cancel,out)
}
pub fn write_pdf_with(document:&VectorDocument,pages:&[usize],meta:&PdfInfo,links:&[Link],cancel:&AtomicBool,out:impl Write)->Result<()>{
 write_pdf_with_progress(document,pages,meta,links,cancel,out,|_|{})
}
/// Export with the number of completed pages reported after each page.
pub fn write_pdf_with_progress(document:&VectorDocument,pages:&[usize],meta:&PdfInfo,links:&[Link],cancel:&AtomicBool,out:impl Write,mut progress:impl FnMut(usize))->Result<()>{
 let title=meta.title.as_str();
 if pages.is_empty()||pages.len()>10_000||[&meta.title,&meta.author,&meta.subject,&meta.keywords].iter().any(|v|v.len()>16_000){return Err("Invalid PDF page selection or title".into());}
 for &i in pages{validate_page(document.pages.get(i).ok_or("PDF page index is outside the document")?)?;}
 let mut f=File::default();let catalog=f.reserve()?;let tree=f.reserve()?;let ids=pages.iter().map(|_|f.reserve()).collect::<Result<Vec<_>>>()?;let dests:BTreeMap<usize,usize>=pages.iter().copied().zip(ids.iter().copied()).collect();let mut images=BTreeMap::new();let mut states=BTreeMap::new();
 for (completed,(&pi,&id)) in pages.iter().zip(&ids).enumerate(){if cancel.load(Ordering::Relaxed){return Err("Output cancelled".into());}let page=document.pages.get(pi).ok_or("Selected PDF page no longer exists")?;validate_page(page)?;
  let mut fonts=Fonts::default();let mut image_res=String::new();let mut used_images=std::collections::BTreeSet::new();let mut content=format!("q\n1 0 0 -1 0 {} cm\n",n(page.size.height)?);
  for source in &page.commands{let (command,clip)=source.unclipped();if cancel.load(Ordering::Relaxed){return Err("Output cancelled".into());}content+="q\n";if let Some(clip)=clip {content+=&rectangle(clip)?;content+="W n\n";}match command{
   DrawCommand::Clipped{..}=>unreachable!("unclipped command"),
   DrawCommand::FillRect{rect,paint}=>{color(&mut content,paint,false,&mut f,&mut states)?;content+=&rectangle(*rect)?;content+="f\n";},
   DrawCommand::Path(p)=>{content+=&matrix(p.transform)?;content+=&path(&p.path,Affine2D::IDENTITY)?;
    if let Some(paint)=&p.fill{color(&mut content,paint,false,&mut f,&mut states)?;content+="f\n";}
    if let Some((paint,stroke))=&p.stroke{color(&mut content,paint,true,&mut f,&mut states)?;content+=&path(&p.path,Affine2D::IDENTITY)?;content+=&format!("{} w S\n",n(stroke.width_pt)?);}},
   DrawCommand::TextRun(r)=>{color(&mut content,&r.fill,false,&mut f,&mut states)?;if r.actual_text.as_ref().is_some_and(|s|!s.is_empty()){content+=&fonts.text(&mut f,r)?;}else{for g in &r.glyphs{content+=&path(&g.outline,g.transform)?;content+="f\n";}}},
   DrawCommand::Image{rect,image:im}=>{let image_id=if let Some(id)=images.get(&im.key){*id}else{let id=image(&mut f,im)?;images.insert(im.key,id);id};if used_images.insert(im.key){image_res+=&format!("/I{} {image_id} 0 R ",im.key);}content+=&format!("{} 0 0 {} {} {} cm /I{} Do\n",n(rect.width)?,n(-rect.height)?,n(rect.x)?,n(rect.max_y())?,im.key);},
  }content+="Q\n";if content.len()>MAX_BYTES{return Err("PDF page stream exceeds size bound".into());}}
  content+="Q\n";let stream=f.stream("",content.as_bytes())?;let font_res=fonts.finish(&mut f)?;let state_res=states.iter().map(|(k,id)|format!("/G{k} {id} 0 R")).collect::<Vec<_>>().join(" ");
  let mut annotations=String::new();for link in links.iter().filter(|l|l.page==pi){let action=match &link.target{
   Target::Uri(uri) if uri.starts_with("https://")||uri.starts_with("http://")||uri.starts_with("mailto:")=>format!("/A << /S /URI /URI <{}> >>",hex(uri.as_bytes())),
   Target::Page{page,y}=>{let Some(dest)=dests.get(page)else{continue;};format!("/Dest [{dest} 0 R /XYZ null {} null]",n(document.pages[*page].size.height-*y)?)},_=>continue};
   let r=link.rect;let a=f.obj(format!("<< /Type /Annot /Subtype /Link /Rect [{} {} {} {}] /Border [0 0 0] {action} >>",n(r.x)?,n(page.size.height-r.max_y())?,n(r.max_x())?,n(page.size.height-r.y)?))?;annotations+=&format!("{a} 0 R ");}
  f.put(id,format!("<< /Type /Page /Parent {tree} 0 R /MediaBox [0 0 {} {}] /Resources << /Font << {font_res} >> /XObject << {image_res} >> /ExtGState << {state_res} >> >> /Contents {stream} 0 R /Annots [{annotations}] >>",n(page.size.width)?,n(page.size.height)?).into_bytes())?;
  progress(completed+1);
 }
 if cancel.load(Ordering::Acquire){return Err("Output cancelled".into());}
 f.put(tree,format!("<< /Type /Pages /Count {} /Kids [{}] >>",ids.len(),ids.iter().map(|id|format!("{id} 0 R")).collect::<Vec<_>>().join(" ")).into_bytes())?;
 f.put(catalog,format!("<< /Type /Catalog /Pages {tree} 0 R >>").into_bytes())?;let mut entries=format!("/Producer (Impress vector output) /Title <{}>",unicode(title,true));
 for (key,value) in [("Author",&meta.author),("Subject",&meta.subject),("Keywords",&meta.keywords)]{if !value.trim().is_empty(){entries+=&format!(" /{key} <{}>",unicode(value.trim(),true));}}
 let info=f.obj(format!("<< {entries} >>"))?;f.finish(catalog,info,out)
}
#[cfg(test)] #[path = "../tests/unit/pdf.rs"] mod tests;
