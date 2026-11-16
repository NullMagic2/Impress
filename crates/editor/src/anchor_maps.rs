//! Explicit paragraph identity maps. No substring matching: repeated words and
//! Unicode text cannot move a reference to the wrong occurrence.
use super::*;
use model::{AnchorRange, TextAnchor, ReviewState};
#[derive(Clone,Debug,serde::Serialize,serde::Deserialize)]
pub(super) enum AnchorMapping {
    Replace { paragraph:NodeId, start:usize, end:usize, inserted:usize },
    Offset { paragraph:NodeId, at:usize, delta:usize },
    Split { left:NodeId, right:NodeId, at:usize },
    Join { left:NodeId, right:NodeId, at:usize },
    Splice { start:TextAnchor, end:TextAnchor, interior:Vec<NodeId> },
}
impl AnchorMapping {
    pub(super) fn splice(document:&Document,start:&DocumentPosition,end:&DocumentPosition)->Self {
        let ps=paragraph_stream_for_node(document,&start.node_id)
            .filter(|stream|stream.iter().any(|p|p.id==end.node_id))
            .unwrap_or_else(||paragraph_positions(document).into_iter().map(|(_,_,p)|p).collect());
        let a=ps.iter().position(|p|p.id==start.node_id); let b=ps.iter().position(|p|p.id==end.node_id);
        let interior=match(a,b){(Some(a),Some(b)) if a<b=>ps[a+1..b].iter().map(|p|p.id.clone()).collect(),_=>Vec::new()};
        Self::Splice{start:TextAnchor{paragraph:start.node_id.clone(),offset:start.byte_offset},end:TextAnchor{paragraph:end.node_id.clone(),offset:end.byte_offset},interior}
    }
    fn anchor(&self,a:&mut TextAnchor,end_gravity:bool) {
        match self {
            Self::Replace{paragraph,start,end,inserted} if &a.paragraph==paragraph => {
                a.offset=if a.offset<*start {a.offset}
                    else if a.offset>*end {start.saturating_add(*inserted).saturating_add(a.offset-*end)}
                    else if a.offset==*start&&end_gravity {*start}
                    else {start.saturating_add(*inserted)};
            }
            Self::Offset{paragraph,at,delta} if &a.paragraph==paragraph && a.offset>=*at => {a.offset=a.offset.saturating_add(*delta);}
            Self::Split{left,right,at} if &a.paragraph==left => {
                if a.offset>*at || (a.offset==*at&&!end_gravity){a.paragraph=right.clone();a.offset-=*at;}
            }
            Self::Join{left,right,at} if &a.paragraph==right=>{a.paragraph=left.clone();a.offset+=*at;}
            Self::Splice{start,end,interior}=>{
                if a.paragraph==start.paragraph{a.offset=a.offset.min(start.offset);}
                else if a.paragraph==end.paragraph{a.paragraph=start.paragraph.clone();a.offset=start.offset+a.offset.saturating_sub(end.offset);}
                else if interior.contains(&a.paragraph){*a=start.clone();}
            }
            _=>{}
        }
    }
    fn range(&self,range:&mut Option<AnchorRange>){
        if let Some(r)=range {
            let collapsed=r.start==r.end;
            self.anchor(&mut r.start,false);self.anchor(&mut r.end,!collapsed);
            if r.start.paragraph==r.end.paragraph&&r.start.offset>r.end.offset{r.end=r.start.clone();}
        }
    }
    pub(super) fn control_range(&self,range:&mut AnchorRange){
        let split=matches!(self,Self::Split{..});
        self.anchor(&mut range.start,!split);self.anchor(&mut range.end,split);
        if range.start.paragraph==range.end.paragraph&&range.start.offset>range.end.offset{range.end=range.start.clone();}
    }
    pub(super) fn apply(&self,review:&mut ReviewState){
        for r in review.bookmarks.values_mut().map(|b|&mut b.range).chain(review.comments.values_mut().map(|c|&mut c.range)){self.range(r);}
        for change in review.tracked_changes.values_mut() {
            if change.status != model::RevisionStatus::Pending { continue; }
            for effect in &mut change.effects {
                if let model::RevisionEffect::ParagraphBoundary { anchor, .. } = effect { self.anchor(anchor,false); }
                if let model::RevisionEffect::Inline { range, .. } = effect {
                    let mut value = Some(range.clone()); self.range(&mut value);
                    if let Some(value) = value { *range = value; }
                }
            }
        }
    }
}
