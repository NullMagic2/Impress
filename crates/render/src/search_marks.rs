use crate::{ColoredOverlayRect,Color,OverlayRect,Rect,VectorDocument};
use std::{collections::BTreeMap,ops::Range};
pub fn search_marks(scene:&VectorDocument,ranges:&BTreeMap<String,Vec<Range<usize>>>,limit:usize)->Vec<ColoredOverlayRect>{
 let mut out=Vec::new();for(pi,p)in scene.pages.iter().enumerate(){for h in &p.hit_regions{let Some(rs)=ranges.get(&h.node_id)else{continue;};
  // Caller supplies non-overlapping ranges in ascending order.
  let begin=rs.partition_point(|r|r.end<=h.byte_range.start);
  for r in rs[begin..].iter().take_while(|r|r.start<h.byte_range.end){let a=r.start.max(h.byte_range.start);let b=r.end.min(h.byte_range.end);if a>=b{continue;}
   let x=|o:usize|h.caret_stops.iter().min_by_key(|s|s.byte_offset.abs_diff(o)).map_or(h.rect.x,|s|s.x_pt);let(x0,x1)=(x(a),x(b));
   if out.len()==limit{return out;}out.push(ColoredOverlayRect{area:OverlayRect{page_index:pi,rect:Rect::new(x0.min(x1),h.rect.y,(x1-x0).abs().max(0.5),h.rect.height)},color:Color::rgba(1.0,0.82,0.05,0.30)});
  }
 }}out
}
