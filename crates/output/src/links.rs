//! Resolve output hyperlinks through semantic ranges and final hit geometry.
use model::{Document,Inline,NodeId,document_paragraphs,inline_text};
use render::{Rect,VectorDocument,TextHitRegion};
#[derive(Clone,Debug)]pub enum Target{Uri(String),Page{page:usize,y:f32}}
#[derive(Clone,Debug)]pub struct Link{pub page:usize,pub rect:Rect,pub target:Target}
fn x_at(hit:&TextHitRegion,offset:usize)->f32{hit.caret_stops.iter().min_by_key(|s|s.byte_offset.abs_diff(offset)).map_or(hit.rect.x,|s|s.x_pt)}
pub fn collect(document:&Document,scene:&VectorDocument)->Vec<Link>{
 let mut ranges=std::collections::BTreeMap::<String,Vec<(usize,usize,Target)>>::new();
 let mut anchors=std::collections::BTreeMap::new();
 for b in document.review.bookmarks.values(){if let Some(r)=&b.range{if let Some((pi,hit))=scene.pages.iter().enumerate().find_map(|(pi,p)|p.hit_regions.iter().find(|h|h.node_id==r.start.paragraph.0&&h.byte_range.end>=r.start.offset).map(|h|(pi,h))){anchors.insert(b.name.clone(),Target::Page{page:pi,y:hit.rect.y});}}}
 fn walk(items:&[Inline],id:&NodeId,offset:&mut usize,anchors:&std::collections::BTreeMap<String,Target>,ranges:&mut std::collections::BTreeMap<String,Vec<(usize,usize,Target)>>){
  for i in items{if let Inline::Hyperlink(h)=i{let a=*offset;walk(&h.content,id,offset,anchors,ranges);let target=if let Some(name)=h.target.strip_prefix('#'){anchors.get(name).cloned()}else if h.target.len()<=8192{Some(Target::Uri(h.target.clone()))}else{None};if let Some(t)=target{ranges.entry(id.0.clone()).or_default().push((a,*offset,t));}}
   else{*offset+=inline_text(std::slice::from_ref(i)).len();}}
 }
 for p in document_paragraphs(document){walk(&p.content,&p.id,&mut 0,&anchors,&mut ranges);}
 let mut out=Vec::new();for (pi,page) in scene.pages.iter().enumerate(){for h in &page.hit_regions{if let Some(rs)=ranges.get(&h.node_id){for(a,b,target)in rs{let start=(*a).max(h.byte_range.start);let end=(*b).min(h.byte_range.end);if start<end{let x0=x_at(h,start);let x1=x_at(h,end);out.push(Link{page:pi,rect:Rect::new(x0.min(x1),h.rect.y,(x1-x0).abs().max(0.5),h.rect.height),target:target.clone()});}}}}}out
}
