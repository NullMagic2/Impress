//! Automatic managed contents participates in the originating undo transaction.
use super::*;
use model::inline_text;
use std::collections::BTreeMap;
fn desired(e:&EditorState)->Vec<(NodeId,String,u8)>{
    let r=&e.document.review;let mut out=Vec::new();
    for s in &e.document.sections{let mut ps=Vec::new();model::visit_paragraphs(&s.blocks,&mut ps);for p in ps{
        if r.contents_paragraphs.contains(&p.id){continue;}
        if let Some(level)=e.document.styles.outline_level(p.style.as_ref()) {
            if level<=r.contents_options.max_heading_level{out.push((p.id.clone(),inline_text(&p.content).replace(['\n','\t']," "),level));}
        }
    }}out
}
fn current(e:&EditorState)->bool{
    let review=&e.document.review;let wanted=desired(e);
    if review.contents_paragraphs.len()!=wanted.len()+1{return false;}
    let Some(title)=review.contents_paragraphs.first().and_then(|id|e.paragraph(id))else{return false;};
    if inline_text(&title.content)!=review.contents_options.title{return false;}
    for ((node,text,level),entry) in wanted.iter().zip(review.contents_paragraphs.iter().skip(1)){
        let Some(p)=e.paragraph(entry)else{return false;};
        let Some(Inline::Hyperlink(link))=p.content.first()else{return false;};
        let target=link.target.strip_prefix('#').and_then(|name|review.bookmarks.values().find(|b|b.name==name)).and_then(|b|b.range.as_ref());
        if target.is_none_or(|r|&r.start.paragraph!=node){return false;}
        let visible=link.content.iter().filter_map(|i|if let Inline::Text(t)=i{Some(t.text.as_str())}else{None}).collect::<String>();
        if &visible!=text||p.properties.left_indent_mm!=Some((*level-1)as f32*4.0){return false;}
    }true
}
impl EditorState{
    pub fn set_automatic_contents(&mut self,enabled:bool)->Result<(),String>{let mut after=self.document.review.clone();after.automatic_contents=enabled;self.commit_review("Automatic contents settings",after)}
    pub(super) fn synchronize_contents_transaction(&mut self,label:&str,mutations:&mut Vec<Mutation>)->Result<(),String>{
        if !self.document.review.automatic_contents||self.document.review.contents_paragraphs.is_empty()||label=="Update contents snapshot"||label.starts_with("Native history")||current(self){return Ok(());}
        let before_sections=self.document.sections.clone();let before_review=self.document.review.clone();
        let mut staged=self.clone();staged.document.review.automatic_contents=false;staged.document.review.track_history=false;staged.document.review.track_text=false;
        staged.update_contents(&BTreeMap::new())?;
        // The derived builder's local undo records are discarded. Only its exact
        // changes are appended to the user's one atomic document transaction.
        staged.document.review.automatic_contents=before_review.automatic_contents;staged.document.review.track_history=before_review.track_history;staged.document.review.track_text=before_review.track_text;
        self.document.sections=staged.document.sections;self.document.review=staged.document.review;self.selection=staged.selection;
        mutations.push(Mutation::SectionsReplaced{index:0,before:before_sections,after:self.document.sections.clone()});
        mutations.push(Mutation::ReviewChanged{before:before_review,after:self.document.review.clone()});Ok(())
    }
}
