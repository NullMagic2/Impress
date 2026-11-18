//! Format-neutral references and review data. Anchors use UTF-8 byte offsets
//! into the same inline stream as the editor, never screen coordinates.
use crate::{Block, Document, Inline, NodeId, Paragraph};
use serde::{Serialize,Deserialize};
use std::collections::BTreeMap;

#[derive(Clone,Debug,PartialEq,Eq,Serialize,Deserialize)]
pub struct TextAnchor { pub paragraph: NodeId, pub offset: usize }
#[derive(Clone,Debug,PartialEq,Eq,Serialize,Deserialize)]
pub struct AnchorRange { pub start: TextAnchor, pub end: TextAnchor }
#[derive(Clone,Debug,PartialEq,Eq,Serialize,Deserialize)]
pub struct Bookmark { pub id: NodeId, pub name: String, pub range: Option<AnchorRange> }
#[derive(Clone,Debug,PartialEq,Eq,Serialize,Deserialize)]
pub struct ReviewComment {
    pub id: NodeId, pub author: String, pub text: String, pub range: Option<AnchorRange>,
    #[serde(default)] pub resolved: bool,
    #[serde(default)] pub date: Option<String>,
    #[serde(default)] pub replies: Vec<CommentReply>,
}
#[derive(Clone,Debug,PartialEq,Eq,Serialize,Deserialize)]
pub struct CommentReply { pub id:NodeId, pub author:String, pub date:String, pub text:String }
#[derive(Clone,Copy,Debug,Default,PartialEq,Eq,Serialize,Deserialize)]
pub enum RevisionStatus { #[default] Pending, Accepted, Rejected }
/// Ordered native journal. Payloads are bounded editor mutations, not executable
/// code. OOXML net revisions remain a separate, explicitly limited interchange.
#[derive(Clone,Debug,PartialEq,Serialize,Deserialize)]
pub struct RevisionEvent {
    pub id:NodeId, pub author:String, pub date:String, pub label:String,
    pub status:RevisionStatus, pub payload:crate::RevisionPayload,
}
#[derive(Clone,Debug,PartialEq,Serialize,Deserialize)]
pub struct TextRevision {
    pub id: NodeId, pub paragraph: NodeId, pub author: String,
    #[serde(default)] pub date: Option<String>,
    /// Original paragraph before the pending text edit. Current content remains
    /// the editable final view. This alpha reviews one revision per paragraph.
    pub before: Paragraph,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentsTemplate { #[default] Classic, Compact, Modern, Book, Atlas, Technical, Formal, ReadingOutline, InlinePages, ChapterGuide }

#[derive(Clone,Debug,PartialEq,Serialize,Deserialize)]
#[serde(default)]
pub struct ContentsOptions {
    pub title: String,
    pub max_heading_level: u8,
    pub template: ContentsTemplate,
    pub show_page_numbers: bool,
    pub leader: crate::TabLeader,
    pub right_align_page_numbers: bool,
    pub max_numbered_level: u8,
}
impl Default for ContentsOptions {
    fn default() -> Self {
        Self { title: "Contents".into(), max_heading_level: 3, template: ContentsTemplate::Classic,
            show_page_numbers: true, right_align_page_numbers: true, max_numbered_level: 9, leader: crate::TabLeader::Dots }
    }
}
impl ContentsOptions {
    pub fn number_level(&self, level: u8) -> bool {
        self.show_page_numbers && level <= self.max_numbered_level
    }

    pub fn apply_template(&mut self, template: ContentsTemplate) {
        self.template = template;
        self.right_align_page_numbers = template != ContentsTemplate::InlinePages;
        self.max_numbered_level = if template == ContentsTemplate::ChapterGuide { 1 } else { 9 };
        self.show_page_numbers = template != ContentsTemplate::ReadingOutline;
        self.leader = match template {
            ContentsTemplate::Classic | ContentsTemplate::Compact | ContentsTemplate::Formal | ContentsTemplate::ChapterGuide => crate::TabLeader::Dots,
            ContentsTemplate::Technical => crate::TabLeader::Dashes,
            _ => crate::TabLeader::None,
        };
    }
}
#[derive(Clone,Debug,Default,PartialEq,Serialize,Deserialize)]
pub struct ReviewState {
    #[serde(default)] pub tracked_changes: BTreeMap<NodeId,crate::TrackedChange>,
    #[serde(default)] pub footnotes: BTreeMap<NodeId,Footnote>,
    #[serde(default)] pub bookmarks: BTreeMap<NodeId,Bookmark>,
    #[serde(default)] pub comments: BTreeMap<NodeId,ReviewComment>,
    #[serde(default)] pub revisions: BTreeMap<NodeId,TextRevision>,
    #[serde(default)] pub track_text: bool,
    #[serde(default)] pub author: String,
    /// IDs of the last generated editable contents snapshot, for replace/update.
    #[serde(default)] pub contents_paragraphs: Vec<NodeId>,
    #[serde(default)] pub contents_options:ContentsOptions,
    #[serde(default)] pub automatic_contents:bool,
    #[serde(default)] pub track_history:bool,
    #[serde(default)] pub history:crate::Shared<Vec<RevisionEvent>>,
}
#[derive(Clone,Debug,PartialEq,Eq,Serialize,Deserialize)]
pub struct CompatibilityIssue { pub code: String, pub part: String, pub message: String }

pub fn inline_text(content:&[Inline]) -> String {
    let mut out=String::new();
    for i in content {match i {
        Inline::Text(t)=>out.push_str(&t.text),Inline::Hyperlink(h)=>out.push_str(&inline_text(&h.content)),
        Inline::Tab{..}=>out.push('\t'),Inline::LineBreak{..}=>out.push('\n'),Inline::PaginationBreak{break_kind,..}=>out.push(break_kind.marker()),_=>out.push('\u{fffc}'),
    }} out
}
pub fn visit_paragraphs<'a>(blocks:&'a[Block],out:&mut Vec<&'a Paragraph>) {
    for block in blocks {match block {
        Block::Paragraph(p)=>{out.push(p);visit_box_paragraphs(&p.content,out);},Block::Table(t)=>for row in &t.rows {for cell in &row.cells {visit_paragraphs(&cell.blocks,out);}},_=>{}
    }}
}
fn visit_box_paragraphs<'a>(items:&'a[Inline],out:&mut Vec<&'a Paragraph>){
    for item in items{match item{
        Inline::TextBox(boxed)=>visit_paragraphs(&boxed.blocks,out),
        Inline::Hyperlink(link)=>visit_box_paragraphs(&link.content,out),
        _=>{}
    }}
}
pub fn document_paragraphs(document:&Document) -> Vec<&Paragraph> {
    let mut out=Vec::new();
    for section in &document.sections {
        visit_paragraphs(&section.blocks,&mut out);
        for h in section.header_footers() {
            visit_paragraphs(&h.stream.blocks,&mut out);
        }
    }
    for note in document.review.footnotes.values(){visit_paragraphs(&note.blocks,&mut out);}
    out
}
pub fn valid_range(document:&Document,range:&AnchorRange)->bool {
    let paragraphs=document_paragraphs(document);
    let find=|a:&TextAnchor|paragraphs.iter().position(|p|p.id==a.paragraph && inline_text(&p.content).is_char_boundary(a.offset));
    match (find(&range.start),find(&range.end)) { (Some(a),Some(b))=>a<b || (a==b&&range.start.offset<=range.end.offset),_=>false }
}
/// Rebase one boundary through a replacement computed at Unicode boundaries.
/// Start gravity excludes text inserted immediately before the anchored range;
/// end gravity excludes text inserted immediately after it.
pub fn rebase_offset(old:&str,new:&str,offset:usize,end:bool)->usize {
    if old==new {return offset.min(new.len());}
    let mut prefix=0;
    for (a,b) in old.chars().zip(new.chars()) {if a!=b {break;} prefix+=a.len_utf8();}
    let mut suffix=0;
    for (a,b) in old[prefix..].chars().rev().zip(new[prefix..].chars().rev()) {if a!=b {break;}suffix+=a.len_utf8();}
    let old_end=old.len()-suffix;let new_end=new.len()-suffix;
    let value=if offset<prefix {offset} else if offset>old_end {new_end+offset-old_end}
        else if offset==prefix && end {prefix} else {new_end};
    let mut value=value.min(new.len());while !new.is_char_boundary(value){value-=1;}value
}

#[derive(Clone,Debug,PartialEq,Serialize,Deserialize)]
pub struct Footnote { pub id:NodeId, pub blocks:crate::Shared<Vec<Block>>, #[serde(default)] pub kind:NoteKind }
#[derive(Clone,Copy,Debug,Default,PartialEq,Eq,Serialize,Deserialize)]
pub enum NoteKind { #[default] Footnote, Endnote }
/// Footnote references in body reading order (including table cells). Notes in
/// headers, in notes, and duplicated references are not generated by the UI.
pub fn footnote_references(document:&Document)->Vec<(TextAnchor,NodeId)> {
    fn walk(items:&[Inline],paragraph:&NodeId,offset:&mut usize,out:&mut Vec<(TextAnchor,NodeId)>){
        for inline in items { match inline {
            Inline::Hyperlink(h)=>walk(&h.content,paragraph,offset,out),
            Inline::Field(f)=>{if let crate::FieldKind::FootnoteReference{note}=&f.kind{out.push((TextAnchor{paragraph:paragraph.clone(),offset:*offset},note.clone()));}*offset+=3;},
            Inline::Text(t)=>*offset+=t.text.len(),Inline::Tab{..}|Inline::LineBreak{..}|Inline::PaginationBreak{..}=>*offset+=1,_=>*offset+=3,
        }}
    }
    let mut refs=Vec::new();for s in &document.sections{let mut ps=Vec::new();visit_paragraphs(&s.blocks,&mut ps);for p in ps{walk(&p.content,&p.id,&mut 0,&mut refs);}}refs
}
