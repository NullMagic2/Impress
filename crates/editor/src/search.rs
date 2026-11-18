//! Literal Unicode search over semantic editing streams. This module never uses
//! widget offsets or geometry. Reports are revision-tagged and replacements are
//! staged as ONE normal editor transaction (including review/anchor handling).
use super::*;
use model::{HeaderFooterSet, NoteKind};
use regex::RegexBuilder;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

pub const MAX_MATCHES: usize = 100_000;
const MAX_TEXT: usize = 64 * 1024 * 1024;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchScope { pub body: bool, pub tables: bool, pub headers: bool, pub footers: bool, pub notes: bool }
impl Default for SearchScope { fn default()->Self {Self{body:true,tables:true,headers:true,footers:true,notes:true}} }
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SearchOptions { pub match_case: bool, pub whole_word: bool, pub scope: SearchScope }
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Story { Body(usize), Header(usize,u8), Footer(usize,u8), Note(NodeId,NoteKind) }
impl Story { pub fn label(&self)->String {match self {
    Self::Body(s)=>format!("Section {}",s+1), Self::Header(s,_)=>format!("Header {}",s+1),
    Self::Footer(s,_)=>format!("Footer {}",s+1), Self::Note(_,NoteKind::Footnote)=>"Footnote".into(),
    Self::Note(_,NoteKind::Endnote)=>"Endnote".into(),
}} }
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchMatch { pub node: NodeId, pub range: Range<usize>, pub text: String, pub preview: String, pub story: Story }
#[derive(Clone, Debug)]
pub struct SearchReport { pub revision:u64, pub query:String, pub options:SearchOptions, pub matches:Vec<SearchMatch>, pub truncated:bool }

pub fn scoped_paragraphs<'a>(document:&'a Document, scope:SearchScope)->Vec<(&'a Paragraph,Story)> {
    fn walk<'a>(blocks:&'a[Block],story:&Story,inside_table:bool,scope:SearchScope,out:&mut Vec<(&'a Paragraph,Story)>){
        for block in blocks {match block {
            Block::Paragraph(p) if !inside_table || scope.tables => out.push((p,story.clone())),
            Block::Table(t) if scope.tables=>for row in &t.rows{for cell in &row.cells{walk(&cell.blocks,story,true,scope,out);}},_=>{}
        }}
    }
    fn hf<'a>(set:&'a HeaderFooterSet,section:usize,header:bool,scope:SearchScope,out:&mut Vec<(&'a Paragraph,Story)>){
        for (v,h) in [set.default.as_ref(),set.first.as_ref(),set.even.as_ref()].into_iter().enumerate(){if let Some(h)=h {
            let story=if header{Story::Header(section,v as u8)}else{Story::Footer(section,v as u8)};walk(&h.blocks,&story,false,scope,out);
        }}
    }
    let mut out=Vec::new();
    for (si,s) in document.sections.iter().enumerate(){
        if scope.body {walk(&s.blocks,&Story::Body(si),false,scope,&mut out);}
        if scope.headers{hf(&s.headers,si,true,scope,&mut out);}if scope.footers{hf(&s.footers,si,false,scope,&mut out);}
    }
    if scope.notes {for (id,note) in &document.review.footnotes{walk(&note.blocks,&Story::Note(id.clone(),note.kind),false,scope,&mut out);}}
    out
}

pub fn find(document:&Document, revision:u64, query:&str, options:SearchOptions)->Result<SearchReport,String> {
    if query.is_empty() || query.len()>8192 || query.contains(['\0','\u{fffc}']) {return Err("Enter 1–8192 bytes of searchable text; embedded objects are not text matches".into());}
    // Escaping makes this a literal search, not a hidden regex language. The
    // engine supplies Unicode SIMPLE case folding (not locale-specific folding).
    let re=RegexBuilder::new(&regex::escape(query)).case_insensitive(!options.match_case)
        .unicode(true).size_limit(4*1024*1024).build().map_err(|e|e.to_string())?;
    let mut report=SearchReport{revision,query:query.into(),options,matches:Vec::new(),truncated:false};
    let mut total=0usize;
    for (p,story) in scoped_paragraphs(document,options.scope){
        let text=paragraph_flat_text(p); total=total.saturating_add(text.len());
        if total>MAX_TEXT {report.truncated=true;break;}
        let boundaries: BTreeSet<usize>=if options.whole_word{text.split_word_bound_indices().flat_map(|(i,w)|[i,i+w.len()]).collect()}else{BTreeSet::new()};
        for m in re.find_iter(&text){
            if options.whole_word && (!boundaries.contains(&m.start())||!boundaries.contains(&m.end())){continue;}
            if report.matches.len()>=MAX_MATCHES {report.truncated=true;return Ok(report);}
            let mut start=m.start().saturating_sub(64);while !text.is_char_boundary(start){start+=1;}
            let mut end=(m.end()+96).min(text.len());while !text.is_char_boundary(end){end-=1;}
            report.matches.push(SearchMatch{node:p.id.clone(),range:m.range(),text:m.as_str().into(),
                preview:text[start..end].replace(['\n','\t']," ").replace(INLINE_OBJECT_MARKER,"◊"),story:story.clone()});
        }
    }
    Ok(report)
}

impl EditorState {
    pub fn search(&self,query:&str,options:SearchOptions)->Result<SearchReport,String>{find(&self.document,self.revision,query,options)}
    pub fn replace_search_match(&mut self,report:&SearchReport,index:usize,replacement:&str)->Result<usize,String>{
        let m=report.matches.get(index).ok_or("Search match no longer exists")?;
        self.replace_search_ranges(report,std::slice::from_ref(m),replacement,"Replace")
    }
    pub fn replace_all_search(&mut self,report:&SearchReport,replacement:&str)->Result<usize,String>{
        if report.truncated{return Err("Search reached its safety limit; narrow the scope before Replace All. Nothing was replaced.".into());}
        self.replace_search_ranges(report,&report.matches,replacement,"Replace all")
    }
    fn replace_search_ranges(&mut self,report:&SearchReport,matches:&[SearchMatch],replacement:&str,label:&'static str)->Result<usize,String>{
        if report.revision!=self.revision{return Err("The document changed; search again before replacing".into());}
        if replacement.len()>1024*1024 || replacement.contains(['\0','\u{fffc}','\r','\n']) {return Err("Replacement must be a single paragraph of plain text, at most 1 MiB".into());}
        if matches.len()>MAX_MATCHES || replacement.len().checked_mul(matches.len()).is_none_or(|n|n>MAX_TEXT){return Err("Replacement exceeds the 64 MiB transaction limit".into());}
        let mut grouped:BTreeMap<NodeId,Vec<&SearchMatch>>=BTreeMap::new();
        for m in matches{
            if self.document.review.automatic_contents && self.document.review.contents_paragraphs.contains(&m.node){return Err("Automatic contents are derived. Edit the heading or disable automatic contents before replacing inside them.".into());}
            grouped.entry(m.node.clone()).or_default().push(m);
        }
        // Validate every original range before touching the live document. Reverse
        // order within a paragraph preserves all original UTF-8 offsets.
        let mut mutations=Vec::new();let mut count=0;let mut work_budget=0usize;
        for (id,mut ms) in grouped{
            let mut before=self.paragraph(&id).cloned().ok_or("Search paragraph no longer exists")?;
            let text=paragraph_flat_text(&before);
            ms.sort_by_key(|m|m.range.start);
            let mut end=0;
            for m in &ms{if m.range.start<end || m.range.is_empty() || text.get(m.range.clone())!=Some(m.text.as_str()){
                return Err("Search ranges are stale or overlap; nothing was replaced".into());}end=m.range.end;}
            // Bound remaining split/copy work until a rope-backed stream exists.
            let work=text.len().checked_mul(ms.len()).ok_or("Replacement work overflow")?;
            work_budget=work_budget.checked_add(work).ok_or("Replacement work overflow")?;
            if work_budget>128*1024*1024{return Err("Replace All would require excessive work in large paragraphs; narrow the scope or replace individually".into());}
            let original=before.clone();let first=mutations.len();
            for m in ms.into_iter().rev(){
                if m.text==replacement{continue;}
                replace_preserving_runs(&mut before,m.range.clone(),replacement,&self.document.styles)?;
                mutations.push(Mutation::AnchorsTransformed{mapping:anchor_maps::AnchorMapping::Replace{paragraph:id.clone(),start:m.range.start,end:m.range.end,inserted:replacement.len()},suppress:1});count+=1;
            }
            if mutations.len()>first{mutations.push(Mutation::ParagraphChanged{id,before:original,after:before});}

        }
        if mutations.is_empty(){return Ok(0);}
        let selection=self.selection.clone();let mut staged=self.clone();
        for m in &mutations{apply_mutation(&mut staged.document,m,true)?;}
        // The edit is one transaction, including anchor transforms, contents
        // synchronization and native/net revision records. No partial Replace All.
        if let Some(m)=matches.first(){staged.set_caret(m.node.clone(),m.range.start+replacement.len());}
        staged.commit(label,mutations,selection,None)?;*self=staged;Ok(count)
    }
}
fn replace_preserving_runs(p:&mut Paragraph,range:Range<usize>,replacement:&str,styles:&model::StyleSheet)->Result<(),String>{
    fn replace(items:&mut Vec<Inline>,a:usize,b:usize,text:&str,paragraph_style:Option<&StyleId>,styles:&model::StyleSheet)->Result<(),String>{
        let mut offset=0;
        for item in items.iter_mut(){let len=inline_flat_len(item);if a>=offset&&b<=offset+len{
            if let Inline::Hyperlink(h)=item{return replace(&mut h.content,a-offset,b-offset,text,paragraph_style,styles);}
            break;
        }offset+=len;}
        let (prefix_selected,suffix)=split_content(items,b)?;let(mut prefix,selected)=split_content(&prefix_selected,a)?;
        fn first(items:&[Inline])->Option<&TextRun>{for i in items{match i{Inline::Text(t)=>return Some(t),Inline::Hyperlink(h)=>if let Some(t)=first(&h.content){return Some(t)},_=>{}}}None}
        let properties=first(&selected).map(|t|styles.resolve_run(paragraph_style,t.style.as_ref(),&t.properties)).unwrap_or_else(||styles.resolve_character(paragraph_style));
        if !text.is_empty(){prefix.push(Inline::Text(TextRun{id:NodeId::new(),text:text.into(),style:None,properties}));}
        prefix.extend(suffix);*items=normalized_content(prefix);Ok(())
    }
    replace(&mut p.content,range.start,range.end,replacement,p.style.as_ref(),styles)
}

/// The outline uses semantic style inheritance and all supported heading levels.
pub fn outline(document:&Document)->Vec<(NodeId,u8,String)>{
    fn level(document:&Document,p:&Paragraph)->Option<u8>{
        document.styles.paragraph_outline_level(p)
    }
    let scope=SearchScope{body:true,tables:true,headers:false,footers:false,notes:false};
    scoped_paragraphs(document,scope).into_iter().filter(|(p,_)|!document.review.contents_paragraphs.contains(&p.id))
        .filter_map(|(p,_)|level(document,p).map(|n|(p.id.clone(),n,paragraph_flat_text(p)))).collect()
}
