use crate::{ColoredOverlayRect,Color,VectorDocument,text_range_rect};
use std::{collections::BTreeMap,ops::Range};
pub fn search_marks(scene:&VectorDocument,ranges:&BTreeMap<String,Vec<Range<usize>>>,limit:usize)->Vec<ColoredOverlayRect>{
 let mut out=Vec::new();for(pi,p)in scene.pages.iter().enumerate(){for h in &p.hit_regions{let Some(rs)=ranges.get(&h.node_id)else{continue;};
  // Caller supplies non-overlapping ranges in ascending order.
  let begin=rs.partition_point(|r|r.end<=h.byte_range.start);
  for r in rs[begin..].iter().take_while(|r|r.start<h.byte_range.end){let a=r.start.max(h.byte_range.start);let b=r.end.min(h.byte_range.end);if a>=b{continue;}
   if out.len()==limit{return out;}if let Some(area)=text_range_rect(pi,h,a..b){out.push(ColoredOverlayRect{area,color:Color::rgba(1.0,0.82,0.05,0.30)});}
  }
 }}out
}
