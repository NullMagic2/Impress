//! Semantic clipboard payload, copy/paste operations, and bounded interchange formats.
//! The editor owns one clipboard API; format handling stays private to this module.
use crate::*;

/// A semantic clipboard payload. Unlike the system plain-text clipboard this
/// preserves mixed formatting, hyperlinks, tabs, explicit line breaks and
/// inline equations. Node identifiers are regenerated when the fragment is
/// pasted so copied objects remain independent.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ClipboardFragment {
    /// Ordered mixed prose/table/image content; exclusive with table/paragraphs.
    #[serde(default)] pub blocks: Vec<Block>,
    /// Format import losses, shown by the UI rather than silently discarded.
    #[serde(default)] pub warnings: Vec<String>,
    #[serde(default)] pub table: Option<model::Table>,
    #[serde(default)] pub references: ClipboardReferences,
    pub paragraphs: Vec<ClipboardParagraph>,
    pub plain_text: String,
    #[serde(default)] pub resources: std::collections::BTreeMap<ResourceId, ClipboardResource>,
    #[serde(default)] pub lists: std::collections::BTreeMap<NodeId, ListDefinition>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ClipboardParagraph {
    /// Fragment-local identity used to rebase copied bookmark/comment anchors.
    #[serde(default)] pub origin: Option<NodeId>,
    pub style: Option<StyleId>,
    pub properties: model::ParagraphProperties,
    pub content: Vec<Inline>,
}

pub use data::{ClipboardReferences, ClipboardResource};
pub(crate) use data::fresh_blocks;

mod data {
//! A self-contained, bounded semantic clipboard. Dependencies are imported into
//! a staged editor with fresh IDs; destination styles cannot change appearance.
use super::*;
use model::{Bookmark,ReviewComment,Footnote,FieldKind,AnchorRange,TextAnchor};
use std::collections::{BTreeMap,BTreeSet};
#[derive(Clone,Debug,PartialEq,serde::Serialize,serde::Deserialize)]
pub struct ClipboardResource { pub media_type:String,pub original_name:Option<String>,pub data:Vec<u8> }
#[derive(Clone,Debug,Default,PartialEq,serde::Serialize,serde::Deserialize)]
pub struct ClipboardReferences {
    #[serde(default)] pub footnotes:BTreeMap<NodeId,Footnote>,
    #[serde(default)] pub bookmarks:BTreeMap<NodeId,Bookmark>,
    #[serde(default)] pub comments:BTreeMap<NodeId,ReviewComment>,
}
fn capture_inlines(items:&mut[Inline],style:Option<&StyleId>,doc:&Document,out:&mut BTreeMap<ResourceId,ClipboardResource>,lists:&mut BTreeMap<NodeId,ListDefinition>){
    for i in items {match i {
        Inline::Text(t)=>{t.properties=doc.styles.resolve_run(style,t.style.as_ref(),&t.properties);t.style=None;},
        Inline::Hyperlink(h)=>capture_inlines(&mut h.content,style,doc,out,lists),
        Inline::Image(p)=>capture_resource(&p.resource,doc,out),
        Inline::EmbeddedObject(object)=>{capture_resource(&object.payload,doc,out);if let Some(preview)=&object.preview{capture_resource(preview,doc,out);}},
        Inline::TextBox(boxed)=>capture_blocks(&mut boxed.blocks,doc,out,lists),
        _=>{}
    }}
}
fn capture_resource(id:&ResourceId,doc:&Document,out:&mut BTreeMap<ResourceId,ClipboardResource>){if let Some(r)=doc.resources.get(id){out.entry(id.clone()).or_insert_with(||ClipboardResource{media_type:r.media_type.clone(),original_name:r.original_name.clone(),data:r.data.as_ref().clone()});}}
fn capture_blocks(blocks:&mut[Block],doc:&Document,resources:&mut BTreeMap<ResourceId,ClipboardResource>,lists:&mut BTreeMap<NodeId,ListDefinition>){
    for block in blocks{match block {
        Block::Paragraph(p)=>{
            capture_inlines(&mut p.content,p.style.as_ref(),doc,resources,lists);
            p.properties=doc.styles.resolve_paragraph_with_direct(p.style.as_ref(),&p.properties);p.style=None;
            if let Some(id)=&p.properties.list_id{if let Some(l)=doc.styles.list_definition(&doc.lists, id){lists.insert(id.clone(),l.clone());}}
        },
        Block::Table(t) => {
            let fills = t.rows.iter().enumerate().map(|(index, _)| doc.styles.table_style_fill(t, index)).collect::<Vec<_>>();
            t.properties.borders = doc.styles.table_borders(t).cloned();
            t.properties.style = None;
            t.properties.style_first_row = None;
            t.properties.style_row_banding = None;
            t.properties.style_overrides_table_borders = None;
            for (row, fill) in t.rows.iter_mut().zip(fills) {
                for cell in &mut row.cells {
                    if cell.shading.is_none() {
                        cell.shading = fill.map(str::to_owned);
                    }
                    capture_blocks(&mut cell.blocks, doc, resources, lists);
                }
            }
        },
        Block::Image(i)=>capture_resource(&i.resource,doc,resources),_=>{}
    }}
}
fn inline_notes(items:&[Inline],notes:&mut BTreeSet<NodeId>){for i in items{match i{Inline::Field(f)=>if let FieldKind::FootnoteReference{note}=&f.kind{notes.insert(note.clone());},Inline::Hyperlink(h)=>inline_notes(&h.content,notes),Inline::TextBox(b)=>block_notes(&b.blocks,notes),_=>{}}}}
fn block_notes(blocks:&[Block],notes:&mut BTreeSet<NodeId>){for b in blocks{match b{Block::Paragraph(p)=>inline_notes(&p.content,notes),Block::Table(t)=>for r in &t.rows{for c in &r.cells{block_notes(&c.blocks,notes);}},_=>{}}}}
impl ClipboardFragment {
    pub(crate) fn capture_dependencies(&mut self,document:&Document){
        capture_blocks(&mut self.blocks,document,&mut self.resources,&mut self.lists);
        for p in &mut self.paragraphs {
            capture_inlines(&mut p.content,p.style.as_ref(),document,&mut self.resources,&mut self.lists);
            p.properties=document.styles.resolve_paragraph_with_direct(p.style.as_ref(),&p.properties);p.style=None;
            if let Some(id)=&p.properties.list_id{if let Some(l)=document.styles.list_definition(&document.lists, id){self.lists.insert(id.clone(),l.clone());}}
        }
        if let Some(t)=self.table.take(){let mut bs=vec![Block::Table(t)];capture_blocks(&mut bs,document,&mut self.resources,&mut self.lists);if let Some(Block::Table(t))=bs.pop(){self.table=Some(t);}}
    }
    pub(crate) fn capture_references(&mut self,doc:&Document,spans:&[(NodeId,usize,usize)]){
        let mut included=spans.iter().map(|(id,a,b)|(id.clone(),(*a,*b))).collect::<BTreeMap<_,_>>();
        let mut wanted=BTreeSet::new();block_notes(&self.blocks,&mut wanted);for p in &self.paragraphs{inline_notes(&p.content,&mut wanted);}
        if let Some(t)=&self.table{for row in &t.rows{for c in &row.cells{block_notes(&c.blocks,&mut wanted);}}}
        // Nested footnotes are unsupported. Validation rejects any nested
        // reference; never follow arbitrary cycles through an untrusted graph.
        for id in wanted {
            if let Some(note)=doc.review.footnotes.get(&id){let mut note=note.clone();
                let mut ps=Vec::new();model::visit_paragraphs(&note.blocks,&mut ps);for p in ps{included.insert(p.id.clone(),(0,paragraph_flat_text(p).len()));}
                capture_blocks(&mut note.blocks,doc,&mut self.resources,&mut self.lists);self.references.footnotes.insert(id,note);
            }
        }
        let convert=|range:&Option<AnchorRange>|->Option<AnchorRange>{
            let range=range.as_ref()?;let a=included.get(&range.start.paragraph)?;let b=included.get(&range.end.paragraph)?;
            if !(a.0..=a.1).contains(&range.start.offset)||!(b.0..=b.1).contains(&range.end.offset){return None;}
            Some(AnchorRange{start:TextAnchor{paragraph:range.start.paragraph.clone(),offset:range.start.offset-a.0},end:TextAnchor{paragraph:range.end.paragraph.clone(),offset:range.end.offset-b.0}})
        };
        for b in doc.review.bookmarks.values(){if let Some(range)=convert(&b.range){let mut b=b.clone();b.range=Some(range);self.references.bookmarks.insert(b.id.clone(),b);}}
        for c in doc.review.comments.values(){if let Some(range)=convert(&c.range){let mut c=c.clone();c.range=Some(range);self.references.comments.insert(c.id.clone(),c);}}
    }
    pub fn validate(&self)->Result<(),String>{
        if self.warnings.len()>256||self.warnings.iter().map(String::len).sum::<usize>()>65536{return Err("Clipboard warnings exceed their size limit".into());}
        if (!self.blocks.is_empty()&&(self.table.is_some()||!self.paragraphs.is_empty()))||(self.table.is_some()&&!self.paragraphs.is_empty()){return Err("Clipboard has conflicting table and text payloads".into());}
        if self.paragraphs.len()>10000||self.plain_text.len()>16*1024*1024||self.resources.len()>256||self.references.footnotes.len()>256||self.references.bookmarks.len()>10000||self.references.comments.len()>10000{return Err("Clipboard selection is too large".into());}
        let mut bytes=0usize;for r in self.resources.values(){bytes=bytes.checked_add(r.data.len()).ok_or("Clipboard size overflow")?;if bytes>32*1024*1024{return Err("Clipboard images exceed 32 MiB".into());}}
        for (id,b) in &self.references.bookmarks{
            if id!=&b.id||b.name.is_empty()||b.name.len()>40||!b.name.chars().next().is_some_and(|c|c.is_ascii_alphabetic()||c=='_')||!b.name.chars().all(|c|c.is_ascii_alphanumeric()||c=='_')||b.range.is_none(){return Err("Invalid clipboard bookmark identity or target".into());}
        }
        for (id,c) in &self.references.comments{if id!=&c.id||c.range.is_none(){return Err("Invalid clipboard comment identity or target".into());}if c.text.len()>65536||c.replies.len()>256||c.replies.iter().any(|r|r.text.len()>65536||r.author.len()>1024){return Err("Clipboard comment thread exceeds supported limits".into());}}
        let names=self.references.bookmarks.values().map(|b|b.name.as_str()).collect::<BTreeSet<_>>();
        if names.len()!=self.references.bookmarks.len()||self.references.bookmarks.values().map(|b|b.name.to_ascii_lowercase()).collect::<BTreeSet<_>>().len()!=names.len(){return Err("Clipboard contains duplicate bookmark names".into());}
        let mut count=0usize;let mut text_bytes=0usize;let mut origins=BTreeMap::new();let mut notes_seen=BTreeSet::new();
        fn picture(id:&ResourceId,w:f32,h:f32,f:&ClipboardFragment)->Result<(),String>{
            if !w.is_finite()||!h.is_finite()||!(0.1..=1000.0).contains(&w)||!(0.1..=1000.0).contains(&h){return Err("Invalid clipboard picture dimensions".into());}
            if !f.resources.contains_key(id){return Err("Clipboard picture bytes are missing".into());}Ok(())
        }
        fn check_items(items:&[Inline],f:&ClipboardFragment,names:&BTreeSet<&str>,depth:usize,count:&mut usize,bytes:&mut usize,seen:&mut BTreeSet<NodeId>,note:bool)->Result<(),String>{
            if depth>64{return Err("Clipboard nesting exceeds 64 levels".into());}
            for i in items{*count+=1;if *count>100000{return Err("Clipboard has too many objects".into());}
                match i {
                    Inline::Text(t)=>{*bytes=bytes.checked_add(t.text.len()).ok_or("Clipboard text overflow")?;if *bytes>16*1024*1024{return Err("Clipboard text exceeds 16 MiB".into());}},
                    Inline::Image(p)=>picture(&p.resource,p.width_mm,p.height_mm,f)?,
                    Inline::EmbeddedObject(object)=>{picture(&object.payload,object.width_mm,object.height_mm,f)?;if let Some(preview)=&object.preview{picture(preview,object.width_mm,object.height_mm,f)?;}},
                    Inline::TextBox(boxed)=>{
                        if !boxed.width_mm.is_finite()||!boxed.height_mm.is_finite()||!(1.0..=1000.0).contains(&boxed.width_mm)||!(1.0..=1000.0).contains(&boxed.height_mm){return Err("Invalid clipboard text-box dimensions".into());}
                        check_box_blocks(&boxed.blocks,f,names,depth+1,count,bytes,seen,note)?;
                    },
                    Inline::Field(field)=>match &field.kind {
                        FieldKind::FootnoteReference{note:id}=>{if note{return Err("Nested clipboard footnotes are unsupported".into());}if !f.references.footnotes.contains_key(id){return Err("Clipboard footnote content is missing".into());}if !seen.insert(id.clone()){return Err("Copy one reference per footnote; duplicate references are not supported".into());}},
                        FieldKind::PageReference{bookmark} if !names.contains(bookmark.as_str())=>return Err(format!("Include the target of bookmark ‘{bookmark}’ in the selection, or use Text only. No source content has been removed.")),_=>{}
                    },
                    Inline::Hyperlink(h)=>{if let Some(name)=h.target.strip_prefix('#'){if !names.contains(name){return Err(format!("The selection does not include internal link target ‘{name}’. Use Text only or include the target."));}}check_items(&h.content,f,names,depth+1,count,bytes,seen,note)?;},_=>{}
                }
            }Ok(())
        }
        fn check_box_blocks(blocks:&[Block],f:&ClipboardFragment,names:&BTreeSet<&str>,depth:usize,count:&mut usize,bytes:&mut usize,seen:&mut BTreeSet<NodeId>,note:bool)->Result<(),String>{
            if depth>32{return Err("Clipboard text-box nesting exceeds 32 levels".into());}
            for block in blocks {match block {
                Block::Paragraph(p)=>check_items(&p.content,f,names,depth+1,count,bytes,seen,note)?,
                Block::Table(t)=>for row in &t.rows{for cell in &row.cells{check_box_blocks(&cell.blocks,f,names,depth+1,count,bytes,seen,note)?;}},
                Block::Image(i)=>picture(&i.resource,i.width_mm,i.height_mm,f)?,
                _=>{}
            }}Ok(())
        }
        fn origin(origins:&mut BTreeMap<NodeId,(String,usize)>,id:&NodeId,content:&[Inline])->Result<(),String>{let index=origins.len();if origins.insert(id.clone(),(model::inline_text(content),index)).is_some(){return Err("Duplicate paragraph identity in clipboard".into());}Ok(())}
        fn blocks(bs:&[Block],f:&ClipboardFragment,names:&BTreeSet<&str>,depth:usize,count:&mut usize,bytes:&mut usize,origins:&mut BTreeMap<NodeId,(String,usize)>,seen:&mut BTreeSet<NodeId>,note:bool)->Result<(),String>{
            if depth>32{return Err("Clipboard table nesting exceeds 32 levels".into());}
            for b in bs{*count+=1;if *count>100000{return Err("Clipboard has too many blocks".into());}match b{
                Block::Paragraph(p)=>{origin(origins,&p.id,&p.content)?;check_items(&p.content,f,names,depth,count,bytes,seen,note)?;},
                Block::Image(p)=>{picture(&p.resource,p.width_mm,p.height_mm,f)?;if p.wrap!=model::ImageWrap::Inline {let a=p.anchor.as_ref().ok_or("Floating clipboard picture has no anchor")?;a.validate()?;if depth!=0||note{return Err("Floating clipboard pictures require a top-level body paragraph".into());}if !f.blocks.iter().any(|b|matches!(b,Block::Paragraph(p)if p.id==a.paragraph)){return Err("Copy the floating picture together with its anchor paragraph, or copy the picture alone as an inline image".into());}}},
                Block::Table(t)=>{super::tables::grid(t)?;for row in &t.rows{for c in &row.cells{blocks(&c.blocks,f,names,depth+1,count,bytes,origins,seen,note)?;}}},
                Block::Foreign(_)=>return Err("Unsupported foreign block in semantic clipboard".into()),_=>{}
            }}Ok(())
        }
        blocks(&self.blocks,self,&names,0,&mut count,&mut text_bytes,&mut origins,&mut notes_seen,false)?;
        for p in &self.paragraphs{if let Some(id)=&p.origin{origin(&mut origins,id,&p.content)?;}check_items(&p.content,self,&names,0,&mut count,&mut text_bytes,&mut notes_seen,false)?;}
        if let Some(t)=&self.table{blocks(&[Block::Table(t.clone())],self,&names,0,&mut count,&mut text_bytes,&mut origins,&mut notes_seen,false)?;}
        for (id,n) in &self.references.footnotes{if id!=&n.id{return Err("Footnote ID mismatch".into());}blocks(&n.blocks,self,&names,0,&mut count,&mut text_bytes,&mut origins,&mut notes_seen,true)?;}
        if notes_seen.len()!=self.references.footnotes.len(){return Err("Clipboard contains unreferenced note dependencies".into());}
        for range in self.references.bookmarks.values().map(|b|&b.range).chain(self.references.comments.values().map(|c|&c.range)).flatten(){
            if range.start.paragraph==range.end.paragraph&&range.start.offset>range.end.offset{return Err("Reversed clipboard reference range".into());}
            if let (Some((_,a)),Some((_,b)))=(origins.get(&range.start.paragraph),origins.get(&range.end.paragraph)){if a>b{return Err("Reversed clipboard paragraph range".into());}}
            for a in [&range.start,&range.end]{if !origins.get(&a.paragraph).is_some_and(|(t,_)|t.is_char_boundary(a.offset)){return Err("Invalid clipboard reference anchor".into());}}
        }
        for c in self.references.comments.values(){if c.text.len()>65536||c.author.len()>1024{return Err("Clipboard comment exceeds limits".into());}}
        Ok(())
    }
}
// One remapping pass for all copied block identities, including note bodies.
pub(crate) fn fresh_blocks(blocks:&mut[Block],map:&mut BTreeMap<NodeId,(NodeId,usize)>){
    for b in blocks{match b{
        Block::Paragraph(p)=>{let id=NodeId::new();map.insert(p.id.clone(),(id.clone(),0));p.id=id;p.content=(p.content.drain(..).map(fresh_inline_ids).collect::<Vec<_>>()).into();},
        Block::Table(t)=>{t.id=NodeId::new();for r in &mut t.rows{r.id=NodeId::new();for c in &mut r.cells{c.id=NodeId::new();fresh_blocks(&mut c.blocks,map);}}},
        Block::Image(i)=>i.id=NodeId::new(),Block::Equation(e)=>e.id=NodeId::new(),Block::PageBreak{id}|Block::ColumnBreak{id}=>*id=NodeId::new(),Block::Foreign(f)=>f.id=NodeId::new(),
    }}
}
fn map_range(range:&mut Option<AnchorRange>,map:&BTreeMap<NodeId,(NodeId,usize)>)->Result<(),String>{
    if let Some(range)=range{for a in [&mut range.start,&mut range.end]{let(id,offset)=map.get(&a.paragraph).ok_or("Copied reference target was not placed")?;a.paragraph=id.clone();a.offset=a.offset.checked_add(*offset).ok_or("Clipboard anchor overflow")?;}}Ok(())
}
impl EditorState {
    pub(crate) fn import_clipboard_dependencies(&mut self,source:&ClipboardFragment)->Result<(ClipboardFragment,Vec<Mutation>),String>{
        if self.document.review.track_text && (source.table.is_some()||!source.blocks.is_empty()||!source.references.footnotes.is_empty()||!source.references.bookmarks.is_empty()||!source.references.comments.is_empty()) {return Err("Turn off text tracking before pasting structures or reference dependencies".into());}
        let mut f=source.clone();let mut mutations=Vec::new();let mut resources=BTreeMap::new();let mut lists=BTreeMap::new();
        for(old,r)in &source.resources{let id=ResourceId::new();let resource=Resource { font_faces: Vec::new(), id:id.clone(),media_type:r.media_type.clone(),original_name:r.original_name.clone(),data:r.data.clone().into()};self.document.resources.insert(id.clone(),resource.clone());resources.insert(old.clone(),id.clone());mutations.push(Mutation::ResourceInserted{id,resource});}
        for(old,l)in &source.lists{let id=NodeId::new();let mut definition=l.clone();definition.id=id.clone();self.document.lists.insert(id.clone(),definition.clone());lists.insert(old.clone(),id.clone());mutations.push(Mutation::ListInserted{id,definition});}
        let note_ids=f.references.footnotes.keys().map(|id|(id.clone(),NodeId::new())).collect::<BTreeMap<_,_>>();
        let mut used=self.document.review.bookmarks.values().map(|b|b.name.to_ascii_lowercase()).collect::<BTreeSet<_>>();let mut names=BTreeMap::new();
        for b in f.references.bookmarks.values(){let base=b.name.chars().take(30).collect::<String>();let mut name=b.name.clone();let mut suffix=1usize;while used.contains(&name.to_ascii_lowercase()){name=format!("{base}_Copy{suffix}");suffix+=1;}used.insert(name.to_ascii_lowercase());names.insert(b.name.clone(),name);}
        fn remap(items:&mut[Inline],resources:&BTreeMap<ResourceId,ResourceId>,lists:&BTreeMap<NodeId,NodeId>,notes:&BTreeMap<NodeId,NodeId>,names:&BTreeMap<String,String>){for i in items{match i{
            Inline::Image(p)=>if let Some(id)=resources.get(&p.resource){p.resource=id.clone();},
            Inline::EmbeddedObject(object)=>{if let Some(id)=resources.get(&object.payload){object.payload=id.clone();}if let Some(preview)=&mut object.preview{if let Some(id)=resources.get(preview){*preview=id.clone();}}},
            Inline::TextBox(boxed)=>bs(&mut boxed.blocks,resources,lists,notes,names),
            Inline::Field(f)=>match &mut f.kind{FieldKind::FootnoteReference{note}=>if let Some(id)=notes.get(note){*note=id.clone();},FieldKind::PageReference{bookmark}=>if let Some(name)=names.get(bookmark){*bookmark=name.clone();},_=>{}},
            Inline::Hyperlink(h)=>{if let Some(name)=h.target.strip_prefix('#').and_then(|v|names.get(v)){h.target=format!("#{name}");}remap(&mut h.content,resources,lists,notes,names);},_=>{}
        }}}
        fn bs(blocks:&mut[Block],resources:&BTreeMap<ResourceId,ResourceId>,lists:&BTreeMap<NodeId,NodeId>,notes:&BTreeMap<NodeId,NodeId>,names:&BTreeMap<String,String>){for b in blocks{match b{
            Block::Paragraph(p)=>{remap(&mut p.content,resources,lists,notes,names);if let Some(id)=&p.properties.list_id{p.properties.list_id=lists.get(id).cloned();}},
            Block::Table(t)=>for r in &mut t.rows{for c in &mut r.cells{bs(&mut c.blocks,resources,lists,notes,names);}},Block::Image(i)=>if let Some(id)=resources.get(&i.resource){i.resource=id.clone();},_=>{}
        }}}
        bs(&mut f.blocks,&resources,&lists,&note_ids,&names);
        for p in &mut f.paragraphs{remap(&mut p.content,&resources,&lists,&note_ids,&names);if let Some(id)=&p.properties.list_id{p.properties.list_id=lists.get(id).cloned();}}
        if let Some(t)=&mut f.table{for r in &mut t.rows{for c in &mut r.cells{bs(&mut c.blocks,&resources,&lists,&note_ids,&names);}}}
        let mut note_map=BTreeMap::new();let mut fresh_notes=BTreeMap::new();
        for(old,mut note)in std::mem::take(&mut f.references.footnotes){note.id=note_ids[&old].clone();bs(&mut note.blocks,&resources,&lists,&note_ids,&names);fresh_blocks(&mut note.blocks,&mut note_map);fresh_notes.insert(note.id.clone(),note);}
        f.references.footnotes=fresh_notes;
        for range in f.references.bookmarks.values_mut().map(|b|&mut b.range).chain(f.references.comments.values_mut().map(|c|&mut c.range)).flatten(){for a in [&mut range.start,&mut range.end]{if let Some((id,at))=note_map.get(&a.paragraph){a.paragraph=id.clone();a.offset+=at;}}}
        let mut fresh_bookmarks=BTreeMap::new();for(_,mut b)in std::mem::take(&mut f.references.bookmarks){b.id=NodeId::new();b.name=names[&b.name].clone();fresh_bookmarks.insert(b.id.clone(),b);}f.references.bookmarks=fresh_bookmarks;
        let mut fresh_comments=BTreeMap::new();for(_,mut c)in std::mem::take(&mut f.references.comments){c.id=NodeId::new();for reply in &mut c.replies{reply.id=NodeId::new();}fresh_comments.insert(c.id.clone(),c);}f.references.comments=fresh_comments;
        Ok((f,mutations))
    }
    pub(crate) fn finish_clipboard_references(&mut self,f:&ClipboardFragment,placements:&BTreeMap<NodeId,(NodeId,usize)>,mutations:&mut Vec<Mutation>,selection:&Option<Selection>)->Result<(),String>{
        // First rebase existing destination references and enforce tracking.
        self.prepare_review_transaction("Paste",mutations,selection)?;
        let before=self.document.review.clone();let mut after=before.clone();let mut map=placements.clone();
        for note in f.references.footnotes.values(){let mut ps=Vec::new();model::visit_paragraphs(&note.blocks,&mut ps);for p in ps{map.insert(p.id.clone(),(p.id.clone(),0));}after.footnotes.insert(note.id.clone(),note.clone());}
        for b in f.references.bookmarks.values(){let mut b=b.clone();map_range(&mut b.range,&map)?;after.bookmarks.insert(b.id.clone(),b);}
        for c in f.references.comments.values(){let mut c=c.clone();map_range(&mut c.range,&map)?;after.comments.insert(c.id.clone(),c);}
        if after!=before{self.document.review=after.clone();mutations.push(Mutation::ReviewChanged{before,after});}Ok(())
    }
}
}

mod tables {
//! Rectangular table clipboard operations. Merges are copied as structure, not
//! flattened paragraphs. Selections cutting a merge boundary are rejected.
use super::*;
use model::Table;
use std::collections::{BTreeMap,BTreeSet};
#[derive(Clone,Debug)]
pub(crate) struct CellSlot {pub row:usize,pub cell:usize,pub column:usize,pub rows:usize,pub columns:usize,pub id:NodeId}
pub(crate) struct TableGrid {pub columns:usize,pub rows:usize,pub cells:Vec<CellSlot>}
pub(crate) fn grid(table:&Table)->Result<TableGrid,String>{
    if table.rows.is_empty()||table.rows.len()>4096{return Err("Clipboard table must have 1–4096 rows".into());}
    let mut cells=Vec::new();let mut occupied=BTreeSet::new();let mut ids=BTreeSet::new();let mut width=0;
    for (row,r) in table.rows.iter().enumerate(){if r.height_mm.is_some_and(|v|!v.is_finite()||v<0.0||v>1000.0){return Err("Invalid clipboard row height".into());}let mut col=0;
        for (cell,c) in r.cells.iter().enumerate(){
            if c.width_mm.is_some_and(|v|!v.is_finite()||v<=0.0||v>1000.0){return Err("Invalid clipboard cell width".into());}
            if !ids.insert(c.id.clone()){return Err("Duplicate clipboard cell ID".into());}
            let (rs,cs)=(usize::from(c.row_span),usize::from(c.column_span));
            if rs==0||cs==0||rs>table.rows.len()-row||cs>256{return Err("Invalid clipboard cell span".into());}
            while col<256&&(col..col+cs).any(|v|occupied.contains(&(row,v))){col+=1;}
            if col+cs>256{return Err("Clipboard table exceeds 256 columns".into());}
            for y in row..row+rs{for x in col..col+cs{if !occupied.insert((y,x)){return Err("Clipboard cell spans overlap".into());}}}
            cells.push(CellSlot{row,cell,column:col,rows:rs,columns:cs,id:c.id.clone()});col+=cs;width=width.max(col);
        }
    }
    if width==0||occupied.len()!=table.rows.len()*width{return Err("Clipboard table has holes in its logical grid".into());}
    if !table.properties.grid_column_widths_mm.is_empty()&&(table.properties.grid_column_widths_mm.len()!=width||table.properties.grid_column_widths_mm.iter().any(|v|!v.is_finite()||*v<=0.0||*v>1000.0)){return Err("Invalid clipboard column widths".into());}
    Ok(TableGrid{columns:width,rows:table.rows.len(),cells})
}
#[derive(Clone,Copy)]struct Rectangle{top:usize,left:usize,bottom:usize,right:usize}
fn rectangle(g:&TableGrid,ids:&[NodeId])->Result<Rectangle,String>{
    let selected=ids.iter().collect::<BTreeSet<_>>();if selected.is_empty(){return Err("Select at least one table cell".into());}
    let cells=g.cells.iter().filter(|c|selected.contains(&c.id)).collect::<Vec<_>>();if cells.len()!=selected.len(){return Err("Selected table cells no longer exist".into());}
    let rect=Rectangle{top:cells.iter().map(|c|c.row).min().unwrap(),left:cells.iter().map(|c|c.column).min().unwrap(),bottom:cells.iter().map(|c|c.row+c.rows).max().unwrap(),right:cells.iter().map(|c|c.column+c.columns).max().unwrap()};
    for c in &g.cells{let intersects=c.row<rect.bottom&&c.row+c.rows>rect.top&&c.column<rect.right&&c.column+c.columns>rect.left;
        if intersects&&(!selected.contains(&c.id)||c.row<rect.top||c.column<rect.left||c.row+c.rows>rect.bottom||c.column+c.columns>rect.right){return Err("Select a complete rectangle including all merged cells it intersects".into());}}
    Ok(rect)
}
pub(crate) fn table_text(table:&Table)->String{
    let Ok(g)=grid(table)else{return String::new();};let mut rows=vec![vec![String::new();g.columns];g.rows];
    for c in &g.cells{let mut ps=Vec::new();collect_paragraph_refs(&table.rows[c.row].cells[c.cell].blocks,&mut ps);rows[c.row][c.column]=ps.iter().map(|p|paragraph_flat_text(p)).collect::<Vec<_>>().join("\n");}
    rows.into_iter().map(|r|r.join("\t")).collect::<Vec<_>>().join("\n")
}
fn table_mut<'a>(blocks:&'a mut[Block],id:&NodeId)->Option<&'a mut Table>{for b in blocks{if let Block::Table(t)=b{if &t.id==id{return Some(t);}for r in &mut t.rows{for c in &mut r.cells{if let Some(t)=table_mut(&mut c.blocks,id){return Some(t);}}}}}None}
fn section_table_mut<'a>(section:&'a mut Section,id:&NodeId)->Option<&'a mut Table>{
    let Section{blocks,headers,footers,..}=section;
    if let Some(t)=table_mut(blocks,id){return Some(t);}
    for (_,h) in headers.iter_mut().chain(footers.iter_mut()){if let Some(t)=table_mut(&mut h.blocks,id){return Some(t);}}None
}
pub(super) fn insert_blocks(blocks:&mut Vec<Block>,id:&NodeId,replacement:&[Block])->bool{
    if let Some(i)=blocks.iter().position(|b|matches!(b,Block::Paragraph(p) if &p.id==id)){blocks.splice(i..=i,replacement.iter().cloned());return true;}
    for b in blocks{if let Block::Table(t)=b{for r in &mut t.rows{for c in &mut r.cells{if insert_blocks(&mut c.blocks,id,replacement){return true;}}}}}false
}
fn expanded_cells(g:&TableGrid,mut area:Rectangle)->Result<Vec<NodeId>,String>{
    for _ in 0..128{
        let before=(area.top,area.left,area.bottom,area.right);
        for c in &g.cells{
            if c.row<area.bottom&&c.row+c.rows>area.top&&c.column<area.right&&c.column+c.columns>area.left{
                area.top=area.top.min(c.row);area.left=area.left.min(c.column);
                area.bottom=area.bottom.max(c.row+c.rows);area.right=area.right.max(c.column+c.columns);
            }
        }
        if before==(area.top,area.left,area.bottom,area.right){
            return Ok(g.cells.iter().filter(|c|c.row>=area.top&&c.row<area.bottom&&c.column>=area.left&&c.column<area.right).map(|c|c.id.clone()).collect());
        }
    }
    Err("Merged-cell selection is too complex; select the whole table instead".into())
}
impl EditorState {
    /// Extend a cell rectangle around whole merges. The semantic grid, not screen
    /// coordinates or only the currently visible page, determines membership.
    pub fn table_rectangle_between(&self,id:&NodeId,first:&NodeId,last:&NodeId)->Result<Vec<NodeId>,String>{
        let g=grid(self.table_by_id(id).ok_or("Table not found")?)?;
        let a=g.cells.iter().find(|c|&c.id==first).ok_or("Starting cell not found")?;
        let b=g.cells.iter().find(|c|&c.id==last).ok_or("Ending cell not found")?;
        expanded_cells(&g,Rectangle{top:a.row.min(b.row),left:a.column.min(b.column),bottom:(a.row+a.rows).max(b.row+b.rows),right:(a.column+a.columns).max(b.column+b.columns)})
    }
    pub fn table_selection_band(&self,id:&NodeId,cell:&NodeId,mode:u8)->Result<Vec<NodeId>,String>{
        let g=grid(self.table_by_id(id).ok_or("Table not found")?)?;let c=g.cells.iter().find(|c|&c.id==cell).ok_or("Cell not found")?;
        let area=match mode{0=>Rectangle{top:c.row,left:c.column,bottom:c.row+c.rows,right:c.column+c.columns},1=>Rectangle{top:c.row,left:0,bottom:c.row+c.rows,right:g.columns},2=>Rectangle{top:0,left:c.column,bottom:g.rows,right:c.column+c.columns},_=>Rectangle{top:0,left:0,bottom:g.rows,right:g.columns}};
        expanded_cells(&g,area)
    }

    pub fn copy_table_cells(&self,id:&NodeId,cells:&[NodeId])->Result<ClipboardFragment,String>{
        let source=self.table_by_id(id).ok_or("Table not found")?;let g=grid(source)?;let rect=rectangle(&g,cells)?;
        let selected=cells.iter().collect::<BTreeSet<_>>();let mut table=source.clone();table.rows=source.rows[rect.top..rect.bottom].to_vec();
        for row in &mut table.rows{row.cells.retain(|c|selected.contains(&c.id));}
        if !table.properties.grid_column_widths_mm.is_empty(){table.properties.grid_column_widths_mm=table.properties.grid_column_widths_mm[rect.left..rect.right].to_vec();table.properties.preferred_width_mm=Some(table.properties.grid_column_widths_mm.iter().sum());}
        let mut spans=Vec::new();for row in &table.rows{for c in &row.cells{let mut ps=Vec::new();collect_paragraph_refs(&c.blocks,&mut ps);for p in ps{spans.push((p.id.clone(),0,paragraph_flat_text(p).len()));}}}
        let mut f=ClipboardFragment{plain_text:table_text(&table),table:Some(table),..Default::default()};f.capture_dependencies(&self.document);f.capture_references(&self.document,&spans);f.validate()?;Ok(f)
    }
    pub fn copy_picture(&self,id:&NodeId)->Result<ClipboardFragment,String>{
        fn find(blocks:&[Block],id:&NodeId)->Option<model::InlineImage>{
            fn inline(items:&[Inline],id:&NodeId)->Option<model::InlineImage>{for i in items{match i{Inline::Image(p)if &p.id==id=>return Some(p.clone()),Inline::Hyperlink(h)=>if let Some(p)=inline(&h.content,id){return Some(p);},_=>{}}}None}
            for b in blocks{match b{Block::Paragraph(p)=>if let Some(i)=inline(&p.content,id){return Some(i);},Block::Image(p)if &p.id==id=>return Some(model::InlineImage{id:p.id.clone(),resource:p.resource.clone(),width_mm:p.width_mm,height_mm:p.height_mm,alt_text:p.alt_text.clone(),effects:p.effects.clone()}),Block::Table(t)=>for r in &t.rows{for c in &r.cells{if let Some(i)=find(&c.blocks,id){return Some(i);}}},_=>{}}}None
        }
        let mut picture=None;for s in &self.document.sections{picture=picture.or_else(||find(&s.blocks,id));for h in s.header_footers(){picture=picture.or_else(||find(&h.stream.blocks,id));}}
        let picture=picture.ok_or("Picture not found")?;let mut f=ClipboardFragment{plain_text:picture.alt_text.clone().unwrap_or_else(||"[Picture]".into()),paragraphs:vec![ClipboardParagraph{origin:None,style:None,properties:Default::default(),content:(vec![Inline::Image(picture)]).into()}],..Default::default()};f.capture_dependencies(&self.document);f.validate()?;Ok(f)
    }
    pub fn paste_table_cells(&mut self,f:&ClipboardFragment,table:&NodeId,cells:&[NodeId])->Result<(),String>{self.paste_table_fragment(f,Some((table,cells)))}
    pub(crate) fn paste_table_fragment(&mut self,source:&ClipboardFragment,target:Option<(&NodeId,&[NodeId])>)->Result<(),String>{
        source.validate()?;if source.table.is_none(){return Err("The clipboard does not contain a table".into());}
        if self.document.review.track_text{return Err("Turn off text tracking before pasting a table".into());}
        let mut staged=self.clone();let selection=staged.selection.clone();let(f,mut mutations)=staged.import_clipboard_dependencies(source)?;
        let mut blocks=vec![Block::Table(f.table.clone().ok_or("Missing table")?)];let mut placements=BTreeMap::new();super::data::fresh_blocks(&mut blocks,&mut placements);
        let Block::Table(mut incoming)=blocks.remove(0)else{unreachable!()};
        let before=staged.document.sections.clone();
        if let Some((id,cells))=target {
            let dest=staged.document.sections.iter_mut().find_map(|s|section_table_mut(s,id)).ok_or("Target table not found")?;
            let g=grid(dest)?;let area=rectangle(&g,cells)?;let ig=grid(&incoming)?;
            if ig.rows!=area.bottom-area.top||ig.columns!=area.right-area.left{return Err("Copied and selected table regions must have the same logical rows and columns. Paste at a text caret to insert a new table.".into());}
            let selected=cells.iter().collect::<BTreeSet<_>>();
            let widths=if dest.properties.grid_column_widths_mm.len()==g.columns{dest.properties.grid_column_widths_mm.clone()}else{vec![dest.properties.preferred_width_mm.unwrap_or(160.0)/g.columns as f32;g.columns]};
            for (row,source_row) in incoming.rows.iter_mut().enumerate(){
                for slot in ig.cells.iter().filter(|c|c.row==row){source_row.cells[slot.cell].width_mm=Some(widths[area.left+slot.column..area.left+slot.column+slot.columns].iter().sum());}
                let y=area.top+row;let at=g.cells.iter().filter(|c|c.row==y&&c.column<area.left).count();
                dest.rows[y].cells.retain(|c|!selected.contains(&c.id));dest.rows[y].cells.splice(at..at,source_row.cells.iter().cloned());if area.left==0&&area.right==g.columns{dest.rows[y].height_mm=source_row.height_mm;}
            }
            grid(dest)?;
            let mut ps=Vec::new();for row in &incoming.rows{for c in &row.cells{collect_paragraph_refs(&c.blocks,&mut ps);}}
            if let Some(p)=ps.first(){staged.set_caret(p.id.clone(),0);}
        }else{
            if !staged.selection.as_ref().is_some_and(Selection::is_collapsed){return Err("Place the caret before inserting a copied table, or select a destination cell rectangle".into());}
            let pos=staged.focus_position().ok_or("Place the caret first")?;let p=staged.paragraph(&pos.node_id).cloned().ok_or("Caret paragraph not found")?;
            let (left,right)=split_content(&p.content,pos.byte_offset)?;let mut head=p.clone();head.content=(normalized_content(left)).into();let mut tail=p;tail.id=NodeId::new();tail.content=(normalized_content(right)).into();let tail_id=tail.id.clone();
            let replacement=vec![Block::Paragraph(head),Block::Table(incoming),Block::Paragraph(tail)];let mut inserted=false;
            for s in &mut staged.document.sections{if insert_blocks(&mut s.blocks,&pos.node_id,&replacement){inserted=true;break;}for h in s.header_footers_mut(){if insert_blocks(&mut h.blocks,&pos.node_id,&replacement){inserted=true;break;}}if inserted{break;}}
            if !inserted{return Err("This container cannot receive a table yet".into());}
            mutations.push(Mutation::AnchorsTransformed{mapping:super::anchor_maps::AnchorMapping::Split{left:pos.node_id,right:tail_id.clone(),at:pos.byte_offset},suppress:1});staged.set_caret(tail_id,0);
        }
        mutations.push(Mutation::SectionsReplaced{index:0,before,after:staged.document.sections.clone()});
        staged.finish_clipboard_references(&f,&placements,&mut mutations,&selection)?;staged.commit("Paste table",mutations,selection,None)?;*self=staged;Ok(())
    }
}
#[cfg(test)] mod tests { include!("../tests/unit/clipboard/tables.rs"); }
}

mod blocks {
//! Atomic mixed block insertion. Existing table and prose paths are retained for
//! their richer destination behavior; mixed fragments preserve ordering at a caret.
use super::*;
use std::collections::BTreeMap;
impl EditorState {
    pub(crate) fn paste_block_fragment(&mut self,source:&ClipboardFragment)->Result<(),String>{
        source.validate()?;
        if !self.selection.as_ref().is_some_and(Selection::is_collapsed){return Err("Place a text caret before pasting mixed paragraphs and tables".into());}
        let mut staged=self.clone();let selection=staged.selection.clone();let(f,mut mutations)=staged.import_clipboard_dependencies(source)?;
        let pos=staged.focus_position().ok_or("Place a caret first")?;let p=staged.paragraph(&pos.node_id).cloned().ok_or("Caret paragraph not found")?;
        let before=staged.document.sections.clone();let(mut left,right)=split_content(&p.content,pos.byte_offset)?;
        let has_floating=f.blocks.iter().any(|b|matches!(b,Block::Image(i)if i.wrap!=model::ImageWrap::Inline));
        if has_floating&&!staged.document.sections.iter().any(|s|s.properties.column_count==1&&s.blocks.iter().any(|b|matches!(b,Block::Paragraph(p)if p.id==pos.node_id))){return Err("Paste floating pictures at a top-level single-column body paragraph".into());}
        let mut blocks=f.blocks.clone();let mut placements=BTreeMap::new();super::data::fresh_blocks(&mut blocks,&mut placements);
        for b in &mut blocks{if let Block::Image(i)=b{if let Some(a)=&mut i.anchor{a.paragraph=placements.get(&a.paragraph).ok_or("Floating picture anchor was not copied")?.0.clone();}}}
        let mut head=p.clone();head.content=(normalized_content(std::mem::take(&mut left))).into();let mut tail=p;tail.id=NodeId::new();tail.content=(normalized_content(right)).into();let tail_id=tail.id.clone();
        let mut replacement=vec![Block::Paragraph(head)];replacement.extend(blocks);replacement.push(Block::Paragraph(tail));
        let mut inserted=false;for s in &mut staged.document.sections{
            if super::tables::insert_blocks(&mut s.blocks,&pos.node_id,&replacement){inserted=true;break;}
            for h in s.header_footers_mut(){if super::tables::insert_blocks(&mut h.blocks,&pos.node_id,&replacement){inserted=true;break;}}
            if inserted{break;}
        }
        if !inserted{return Err("This editing stream cannot receive a mixed block fragment".into());}
        mutations.push(Mutation::AnchorsTransformed{mapping:anchor_maps::AnchorMapping::Split{left:pos.node_id,right:tail_id.clone(),at:pos.byte_offset},suppress:1});
        mutations.push(Mutation::SectionsReplaced{index:0,before,after:staged.document.sections.clone()});staged.set_caret(tail_id,0);
        staged.finish_clipboard_references(&f,&placements,&mut mutations,&selection)?;staged.commit("Paste mixed content",mutations,selection,None)?;*self=staged;Ok(())
    }
}
}

pub mod html {
//! Interoperable CF_HTML subset: paragraphs, run styling, hyperlinks and embedded
//! PNG/JPEG/GIF/BMP images. No script execution, stylesheet fetching or file/network
//! image loading. Unknown presentation is reduced to text rather than guessed.
use super::*;
use model::FieldKind;
use std::collections::BTreeMap;
const MAX:usize=32*1024*1024;
fn escape(s:&str)->String{s.replace('&',"&amp;").replace('<',"&lt;").replace('>',"&gt;").replace('"',"&quot;")}
fn entities(s:&str)->String{
    let mut out=String::new();let mut rest=s;
    while let Some(pos)=rest.find('&'){
        out.push_str(&rest[..pos]);rest=&rest[pos..];
        if let Some(end)=rest.find(';').filter(|n|*n<16){
            let name=&rest[1..end];let decoded=match name{"amp"=>Some('&'),"lt"=>Some('<'),"gt"=>Some('>'),"quot"=>Some('"'),"apos"=>Some('\''),"nbsp"=>Some('\u{a0}'),_=>{
                name.strip_prefix("#x").or_else(||name.strip_prefix("#X")).and_then(|n|u32::from_str_radix(n,16).ok()).or_else(||name.strip_prefix('#').and_then(|n|n.parse().ok())).and_then(char::from_u32)
            }};
            if let Some(c)=decoded{out.push(c);rest=&rest[end+1..];continue;}
        }
        out.push('&');rest=&rest[1..];
    }out.push_str(rest);out
}
pub fn wrap_cf_html(fragment:&str)->Vec<u8>{
    let prefix="<html><body><!--StartFragment-->";let suffix="<!--EndFragment--></body></html>";
    let header=|a:usize,b:usize,c:usize,d:usize|format!("Version:1.0\r\nStartHTML:{a:010}\r\nEndHTML:{b:010}\r\nStartFragment:{c:010}\r\nEndFragment:{d:010}\r\n");
    let start=header(0,0,0,0).len();let fs=start+prefix.len();let fe=fs+fragment.len();
    format!("{}{}{}{}",header(start,fe+suffix.len(),fs,fe),prefix,fragment,suffix).into_bytes()
}
pub fn unwrap_cf_html(bytes: &[u8]) -> Result<&str, String> {
    if bytes.len() > MAX { return Err("HTML clipboard exceeds 32 MiB".into()); }
    // Read offsets from an ASCII header before interpreting the UTF-8 body:
    // GlobalSize may include non-UTF-8 allocator padding after EndHTML.
    let header = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]);
    let field = |key: &str| header.lines().find_map(|line| line.strip_prefix(key).and_then(|v| v.trim().parse::<usize>().ok()));
    if let (Some(a), Some(b)) = (field("StartFragment:"), field("EndFragment:")) {
        let fragment = bytes.get(a..b).ok_or("Invalid CF_HTML fragment byte offsets")?;
        return std::str::from_utf8(fragment).map_err(|_| "Clipboard HTML fragment is not UTF-8".into());
    }
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    let text = std::str::from_utf8(&bytes[..end]).map_err(|_| "Clipboard HTML is not UTF-8")?;
    if let (Some(a), Some(b)) = (text.find("<!--StartFragment-->"), text.find("<!--EndFragment-->")) {
        return text.get(a + 20..b).ok_or_else(|| "Invalid HTML fragment markers".into());
    }
    if text.trim_start().starts_with('<') { Ok(text) } else { Err("No HTML fragment was found".into()) }
}

fn safe_link(s:&str)->bool{let s=s.trim().to_ascii_lowercase();["https://","http://","mailto:","#"].iter().any(|p|s.starts_with(p))}
fn css(p:&CharacterProperties)->String{
    let mut s=String::new();
    if let Some(b)=p.bold{s.push_str(if b{"font-weight:bold;"}else{"font-weight:normal;"});}
    if let Some(i)=p.italic{s.push_str(if i{"font-style:italic;"}else{"font-style:normal;"});}
    if p.underline==Some(true){s.push_str("text-decoration:underline;");}
    if let Some(c)=&p.color{if color(c).is_some(){s.push_str(&format!("color:{};",escape(c)));}}
    if let Some(c)=&p.highlight{if c=="none"{s.push_str("background-color:transparent;");}else if color(c).is_some(){s.push_str(&format!("background-color:{};",escape(c)));}}
    if let Some(f)=&p.font_family{s.push_str(&format!("font-family:'{}';",escape(&f.replace('\'',""))));}
    if let Some(size)=p.font_size_pt{if size.is_finite(){s.push_str(&format!("font-size:{size}pt;"));}}s
}
pub fn export(fragment:&ClipboardFragment)->String{
    fn inlines(items:&[Inline],fragment:&ClipboardFragment,out:&mut String){for i in items{match i{
        Inline::Text(r)=>out.push_str(&format!("<span style=\"{}\">{}</span>",css(&r.properties),escape(&r.text))),
        Inline::Hyperlink(h)=>{if safe_link(&h.target){out.push_str(&format!("<a href=\"{}\">",escape(&h.target)));inlines(&h.content,fragment,out);out.push_str("</a>");}else{inlines(&h.content,fragment,out);}},
        Inline::Tab{..}=>out.push_str("&#9;"),Inline::LineBreak{..}=>out.push_str("<br>"),Inline::PaginationBreak{break_kind,..}=>out.push_str(match break_kind {model::PaginationBreakKind::Page=>"<br style=\"break-before:page\">",model::PaginationBreakKind::Column=>"<br style=\"break-before:column\">"}),
        Inline::Equation(e)=>out.push_str(&escape(&e.alt_text.clone().unwrap_or_else(||equation_input::format(&e.root)))),
        Inline::Field(f)=>match &f.kind {
            FieldKind::FootnoteReference{note}=>out.push_str(&format!("<sup>[{}]</sup>",fragment.references.footnotes.keys().position(|id|id==note).map(|i|i+1).unwrap_or(0))),
            _=>out.push_str(&escape(f.display_text.as_deref().unwrap_or("?"))),
        },
        Inline::Image(p)=>{if let Some(r)=fragment.resources.get(&p.resource){out.push_str(&format!("<img src=\"data:{};base64,{}\" width=\"{}\" height=\"{}\" alt=\"{}\">",escape(&r.media_type),b64(&r.data),p.width_mm*96.0/25.4,p.height_mm*96.0/25.4,escape(p.alt_text.as_deref().unwrap_or("Picture"))));}},
        Inline::EmbeddedObject(object)=>{out.push_str(&format!("<span>[Embedded object: {}]</span>",escape(object.program.as_deref().unwrap_or("Package"))));},
        Inline::TextBox(boxed)=>{out.push_str("<div>");let mut paragraphs=Vec::new();model::visit_paragraphs(&boxed.blocks,&mut paragraphs);for p in paragraphs{out.push_str(&format!("<p>{}</p>",escape(&model::inline_text(&p.content))));}out.push_str("</div>");},
    }}}
    fn paragraph(p:&ClipboardParagraph,f:&ClipboardFragment,out:&mut String){
        let align=match p.properties.alignment{Some(model::ParagraphAlignment::Center)=>"center",Some(model::ParagraphAlignment::Right)=>"right",Some(model::ParagraphAlignment::Justify)=>"justify",_=>"left"};
        out.push_str(&format!("<p style=\"margin:0;text-align:{align}\">"));
        // HTML has point destinations, not the semantic range markers used by
        // the private clipboard. Put each destination at its real UTF-8 start.
        let mut markers:BTreeMap<usize,Vec<&str>>=BTreeMap::new();
        let text=model::inline_text(&p.content);
        for bookmark in f.references.bookmarks.values(){
            if let Some(range)=&bookmark.range{
                if p.origin.as_ref()==Some(&range.start.paragraph)&&text.is_char_boundary(range.start.offset){
                    markers.entry(range.start.offset).or_default().push(&bookmark.name);
                }
            }
        }
        if markers.is_empty(){inlines(&p.content,f,out);}else{
            let mut boundaries=markers.keys().copied().collect::<Vec<_>>();
            boundaries.push(0);boundaries.push(text.len());boundaries.sort_unstable();boundaries.dedup();
            let parts=boundaries.windows(2).map(|pair|slice_content(&p.content,pair[0],pair[1])).collect::<Result<Vec<_>,_>>();
            if let Ok(parts)=parts{
                for(index,start)in boundaries.iter().enumerate(){
                    if let Some(names)=markers.get(start){for name in names{out.push_str(&format!("<span id=\"{}\"></span>",escape(name)));}}
                    if let Some(part)=parts.get(index){inlines(part,f,out);}
                }
            }else{inlines(&p.content,f,out);}
        }
        out.push_str("</p>");
    }
    fn blocks(bs:&[Block],f:&ClipboardFragment,out:&mut String){for b in bs{match b{
        Block::Paragraph(p)=>paragraph(&ClipboardParagraph{origin:Some(p.id.clone()),style:p.style.clone(),properties:p.properties.clone(),content:(p.content.clone()).into()},f,out),
        Block::Table(t)=>table(t,f,out),Block::Image(i)=>inlines(&[Inline::Image(model::InlineImage{id:i.id.clone(),resource:i.resource.clone(),width_mm:i.width_mm,height_mm:i.height_mm,alt_text:i.alt_text.clone(),effects:i.effects.clone()})],f,out),
        Block::Equation(e)=>out.push_str(&escape(&equation_input::format(&e.root))),_=>{}
    }}}
    fn table(t:&model::Table,f:&ClipboardFragment,out:&mut String){
        out.push_str("<table style=\"border-collapse:collapse\" border=\"1\">");
        if !t.properties.grid_column_widths_mm.is_empty(){out.push_str("<colgroup>");for w in &t.properties.grid_column_widths_mm{out.push_str(&format!("<col style=\"width:{}mm\">",w));}out.push_str("</colgroup>");}
        for row in &t.rows{out.push_str("<tr>");for c in &row.cells{
            let tag=if row.repeat_as_header{"th"}else{"td"};let mut style=String::from("padding:2pt;");
            if let Some(shading)=c.shading.as_deref().and_then(color){style.push_str(&format!("background-color:{shading};"));}
            if let Some(width)=c.width_mm{style.push_str(&format!("width:{width}mm;"));}
            style.push_str(match c.vertical_alignment{model::CellVerticalAlignment::Top=>"vertical-align:top;",model::CellVerticalAlignment::Center=>"vertical-align:middle;",model::CellVerticalAlignment::Bottom=>"vertical-align:bottom;"});
            out.push_str(&format!("<{tag} rowspan=\"{}\" colspan=\"{}\" style=\"{}\">",c.row_span,c.column_span,escape(&style)));blocks(&c.blocks,f,out);out.push_str(&format!("</{tag}>"));
        }out.push_str("</tr>");}out.push_str("</table>");
    }
    let mut out=String::from("<div style=\"white-space:pre-wrap\">");
    if !fragment.blocks.is_empty(){blocks(&fragment.blocks,fragment,&mut out);}else if let Some(t)=&fragment.table{table(t,fragment,&mut out);}else{for p in &fragment.paragraphs{paragraph(p,fragment,&mut out);}}
    if !fragment.references.footnotes.is_empty(){out.push_str("<hr><div class=\"impress-footnotes\">");for (index,n)in fragment.references.footnotes.values().enumerate(){out.push_str(&format!("<p>[{}]</p>",index+1));blocks(&n.blocks,fragment,&mut out);}out.push_str("</div>");}
    out.push_str("</div>");out
}

#[derive(Clone,Default)]struct Style{properties:CharacterProperties,link:Option<String>,pre:bool}
fn color(value:&str)->Option<String>{
    let value=value.trim();if value.starts_with('#')&&matches!(value.len(),4|7)&&value[1..].bytes().all(|b|b.is_ascii_hexdigit()){
        return Some(if value.len()==4{format!("#{}{}{}{}{}{}",&value[1..2],&value[1..2],&value[2..3],&value[2..3],&value[3..4],&value[3..4])}else{value.to_string()});
    }
    if let Some(v)=value.strip_prefix("rgb(").and_then(|s|s.strip_suffix(')')){let n=v.split(',').map(|n|n.trim().parse::<u8>()).collect::<Result<Vec<_>,_>>().ok()?;if n.len()==3{return Some(format!("#{:02x}{:02x}{:02x}",n[0],n[1],n[2]));}}
    match value.to_ascii_lowercase().as_str(){"black"=>Some("#000000".into()),"white"=>Some("#ffffff".into()),"red"=>Some("#ff0000".into()),"blue"=>Some("#0000ff".into()),_=>None}
}
fn attrs(tag:&str)->BTreeMap<String,String>{
    let mut out=BTreeMap::new();let mut rest=tag.trim();
    while !rest.is_empty(){rest=rest.trim_start();if rest.is_empty(){break;}
        let n=rest.find(|c:char|c.is_whitespace()||c=='='||c=='/').unwrap_or(rest.len());if n==0{rest=&rest[1..];continue;}
        let key=rest[..n].to_ascii_lowercase();rest=rest[n..].trim_start();if !rest.starts_with('='){continue;}rest=rest[1..].trim_start();
        let(value,next)=if let Some(q)=rest.chars().next().filter(|c|*c=='\''||*c=='"'){
            let tail=&rest[1..];let n=tail.find(q).unwrap_or(tail.len());(&tail[..n],tail.get(n+1..).unwrap_or(""))
        }else{let n=rest.find(char::is_whitespace).unwrap_or(rest.len());(&rest[..n],&rest[n..])};
        out.insert(key,entities(value));rest=next;
    }out
}
fn apply_css(style:&mut Style,text:&str){for decl in text.split(';'){let Some((key,value))=decl.split_once(':')else{continue;};let v=value.trim();match key.trim().to_ascii_lowercase().as_str(){
    "font-weight"=>style.properties.bold=Some(v.eq_ignore_ascii_case("bold")||v.parse::<u16>().is_ok_and(|n|n>=600)),
    "font-style"=>style.properties.italic=Some(v.eq_ignore_ascii_case("italic")),
    "text-decoration"=>style.properties.underline=Some(v.contains("underline")),
    "color"=>style.properties.color=color(v),
    "background"|"background-color"=>{if v.eq_ignore_ascii_case("transparent"){style.properties.highlight=Some("none".into());}else if let Some(c)=color(v){style.properties.highlight=Some(c);}},
    "font-family"=>style.properties.font_family=Some(v.split(',').next().unwrap_or(v).trim().trim_matches(['\'','"']).to_string()),
    "font-size"=>{let size=v.strip_suffix("pt").and_then(|n|n.parse::<f32>().ok()).or_else(||v.strip_suffix("px").and_then(|n|n.parse::<f32>().ok()).map(|n|n*0.75));if size.is_some_and(|n|n.is_finite()&&(1.0..=500.0).contains(&n)){style.properties.font_size_pt=size;}},
    "white-space"=>style.pre=v.starts_with("pre"),_=>{}
}}}
fn push_text(p:&mut ClipboardParagraph,text:&str,style:&Style){
    if text.is_empty(){return;}let mut run=TextRun::plain(text);run.properties=style.properties.clone();let item=Inline::Text(run);
    if let Some(link)=&style.link{p.content.push(Inline::Hyperlink(model::Hyperlink{id:NodeId::new(),target:link.clone(),content:(vec![item]).into()}));}else{p.content.push(item);}
}
fn empty()->ClipboardParagraph{ClipboardParagraph{origin:None,style:None,properties:Default::default(),content:(Vec::new()).into()}}
mod tables {
//! Bounded HTML table subset. A table remains a table (including nested tables),
//! never a guessed sequence of text paragraphs. No stylesheets or links loaded.
use super::*;
use model::{Table,TableCell,TableRow,TableProperties,CellVerticalAlignment};
struct Tag {start:usize,end:usize,name:String,closing:bool,attrs:BTreeMap<String,String>}
fn tag(html:&str,from:usize)->Result<Option<Tag>,String>{
    let mut from=from;
    loop{let Some(at)=html[from..].find('<')else{return Ok(None);};let at=at+from;let rest=&html[at..];
        if rest.starts_with("<!--"){from=at+rest.find("-->").map(|n|n+3).ok_or("Unterminated HTML comment")?;continue;}
        let mut quote=None;let mut end=None;for(i,c)in rest.char_indices().skip(1){if let Some(q)=quote{if c==q{quote=None;}}else if c=='\''||c=='"'{quote=Some(c);}else if c=='>'{end=Some(at+i+1);break;}}
        let end=end.ok_or("Unterminated HTML tag")?;let body=html[at+1..end-1].trim();let closing=body.starts_with('/');let body=body.trim_start_matches('/').trim();let n=body.find(|c:char|c.is_whitespace()||c=='/').unwrap_or(body.len());let name=body[..n].to_ascii_lowercase();
        if !closing&&matches!(name.as_str(),"script"|"style"|"head"|"iframe"|"object") {
            let lower=html[end..].to_ascii_lowercase();let stop=lower.find(&format!("</{name}")).ok_or("Unclosed inactive HTML element")?+end;
            from=html[stop..].find('>').ok_or("Unclosed inactive HTML element")?+stop+1;continue;
        }
        return Ok(Some(Tag{start:at,end,name,closing,attrs:attrs(&body[n..])}));
    }
}
fn close(html:&str,opening:&Tag)->Result<(usize,usize),String>{
    let mut depth=1usize;let mut pos=opening.end;let mut count=0;
    while let Some(t)=tag(html,pos)?{pos=t.end;count+=1;if count>200000{return Err("Too many HTML tags".into());}
        if t.name==opening.name{if t.closing{depth-=1;if depth==0{return Ok((t.start,t.end));}}else{depth+=1;if depth>32{return Err("HTML tables are too deeply nested".into());}}}
    }Err(format!("Unclosed HTML {} element",opening.name))
}
fn length(v:&str)->Option<f32>{let v=v.trim();let n=if let Some(n)=v.strip_suffix("mm"){n.trim().parse::<f32>().ok()}else if let Some(n)=v.strip_suffix("pt"){n.trim().parse::<f32>().ok().map(|n|n*25.4/72.0)}else{v.strip_suffix("px").unwrap_or(v).trim().parse::<f32>().ok().map(|n|n*25.4/96.0)};n.filter(|n|n.is_finite()&&*n>0.0&&*n<=1000.0)}
fn style(a:&BTreeMap<String,String>,key:&str)->Option<String>{a.get("style")?.split(';').filter_map(|v|v.split_once(':')).find(|(k,_)|k.trim().eq_ignore_ascii_case(key)).map(|(_,v)|v.trim().to_string())}
fn prose(html:&str,f:&mut ClipboardFragment,out:&mut Vec<Block>)->Result<(),String>{
    match import_prose(html){Ok(p)=>{f.resources.extend(p.resources);f.lists.extend(p.lists);for p in p.paragraphs{out.push(Block::Paragraph(Paragraph{id:NodeId::new(),style:p.style,properties:p.properties,content:(p.content).into()}));}Ok(())},Err(e)if e=="HTML contains no editable content"=>Ok(()),Err(e)=>Err(e)}
}
fn cell_blocks(html:&str,f:&mut ClipboardFragment,depth:usize)->Result<Vec<Block>,String>{
    let mut out=Vec::new();let mut pos=0;let mut last=0;
    while let Some(t)=tag(html,pos)?{pos=t.end;if t.name=="table"&&!t.closing{let(end,after)=close(html,&t)?;prose(&html[last..t.start],f,&mut out)?;out.push(Block::Table(parse_table(&html[t.end..end],&t.attrs,f,depth+1)?));pos=after;last=after;}}
    prose(&html[last..],f,&mut out)?;if out.is_empty(){out.push(Block::Paragraph(Paragraph::plain("")));}Ok(out)
}
fn parse_table(html:&str,a:&BTreeMap<String,String>,f:&mut ClipboardFragment,depth:usize)->Result<Table,String>{
    if depth>16{return Err("HTML table nesting exceeds 16 levels".into());}
    let mut rows=Vec::new();let mut widths=Vec::new();let mut pos=0;let mut count=0;
    while let Some(t)=tag(html,pos)?{pos=t.end;count+=1;if count>200000{return Err("Too many HTML table tags".into());}
        if t.name=="table"&&!t.closing{return Err("Nested table must be inside a cell".into());}
        if t.name=="col"&&!t.closing{if let Some(width)=style(&t.attrs,"width").as_deref().and_then(length).or_else(||t.attrs.get("width").and_then(|v|length(v))){widths.push(width);}continue;}
        if t.name!="tr"||t.closing{continue;}
        if rows.len()>=4096{return Err("HTML table exceeds 4096 rows".into());}
        let(end,after)=close(html,&t)?;let body=&html[t.end..end];pos=after;let mut cursor=0;let mut cells=Vec::new();let mut header=true;
        while let Some(c)=tag(body,cursor)?{cursor=c.end;if c.closing||!matches!(c.name.as_str(),"td"|"th"){continue;}
            if cells.len()>=256{return Err("HTML table row exceeds 256 cells".into());}header&=c.name=="th";
            let(end,after)=close(body,&c)?;cursor=after;
            let span=|key:&str|->Result<u16,String>{let n=c.attrs.get(key).map(|v|v.parse::<u16>().map_err(|_|"Invalid HTML cell span".to_string())).transpose()?.unwrap_or(1);if n==0||n>4096{return Err("Invalid HTML cell span".into());}Ok(n)};
            let width=style(&c.attrs,"width").as_deref().and_then(length).or_else(||c.attrs.get("width").and_then(|v|length(v)));
            let shading=style(&c.attrs,"background-color").as_deref().and_then(color).or_else(||c.attrs.get("bgcolor").and_then(|v|color(v)));
            let vertical=match style(&c.attrs,"vertical-align").as_deref().or_else(||c.attrs.get("valign").map(String::as_str)){Some("middle")=>CellVerticalAlignment::Center,Some("bottom")=>CellVerticalAlignment::Bottom,_=>CellVerticalAlignment::Top};
            let mut blocks=cell_blocks(&body[c.end..end],f,depth)?;
            if c.name=="th"{for b in &mut blocks{if let Block::Paragraph(p)=b{for i in &mut p.content{if let Inline::Text(r)=i{if r.properties.bold.is_none(){r.properties.bold=Some(true);}}}}}}
            cells.push(TableCell{id:NodeId::new(),blocks: blocks.into(),column_span:span("colspan")?,row_span:span("rowspan")?,width_mm:width,shading,borders:None,vertical_alignment:vertical, margins: None, text_direction: None});
        }
        rows.push(TableRow{id:NodeId::new(),cells,repeat_as_header:header,allow_break_across_pages:true,height_mm:style(&t.attrs,"height").as_deref().and_then(length)});
    }
    let mut table=Table{id:NodeId::new(),rows,properties:TableProperties{preferred_width_mm:style(a,"width").as_deref().and_then(length).or_else(||a.get("width").and_then(|v|length(v))),..Default::default()}};
    let grid=super::super::tables::grid(&table)?;if widths.len()==grid.columns{table.properties.grid_column_widths_mm=widths;}
    Ok(table)
}
pub(super) fn import_table(html:&str)->Result<Option<ClipboardFragment>,String>{
    if html.len()>MAX{return Err("HTML clipboard exceeds 32 MiB".into());}let mut pos=0;
    while let Some(t)=tag(html,pos)?{pos=t.end;if t.name!="table"||t.closing{continue;}
        let(end,after)=close(html,&t)?;
        // Ordered block fragments preserve all prose and tables, not merely the
        // first table (Office commonly places explanatory prose around tables).
        let mixed = [&html[..t.start],&html[after..]].iter().any(|outside|
            import_prose(outside).is_ok_and(|f| !f.plain_text.trim().is_empty()))
            || html[after..].to_ascii_lowercase().contains("<table");
        if mixed {
            let mut f=ClipboardFragment::default();
            f.blocks=(cell_blocks(html,&mut f,0)?).into();
            let mut ps=Vec::new();model::visit_paragraphs(&f.blocks,&mut ps);
            f.plain_text=ps.iter().map(|p|model::inline_text(&p.content)).collect::<Vec<_>>().join("\n");
            f.validate()?;return Ok(Some(f));
        }
        let mut f=ClipboardFragment::default();let table=parse_table(&html[t.end..end],&t.attrs,&mut f,0)?;f.plain_text=super::super::tables::table_text(&table);f.table=Some(table);f.validate()?;return Ok(Some(f));
    }Ok(None)
}
}
pub fn import(html:&str)->Result<ClipboardFragment,String>{
    if let Some(result)=tables::import_table(html)? { return Ok(result); }
    import_prose(html)
}
fn import_prose(html:&str)->Result<ClipboardFragment,String>{
    if html.len()>MAX{return Err("HTML clipboard exceeds 32 MiB".into());}
    let mut result=ClipboardFragment{paragraphs:Vec::new(),plain_text:String::new(),resources:BTreeMap::new(),lists:BTreeMap::new(),..Default::default()};
    let mut p=empty();let mut style=Style::default();let mut stack:Vec<(String,Style)>=Vec::new();let mut pos=0;let mut skip:Option<String>=None;
    while pos<html.len(){
        if result.paragraphs.len()>10000||stack.len()>64{return Err("HTML clipboard is too deeply nested or has too many paragraphs".into());}
        let rest=&html[pos..];if rest.starts_with("<!--"){pos+=rest.find("-->").map(|n|n+3).unwrap_or(rest.len());continue;}
        if !rest.starts_with('<'){
            let len=rest.find('<').unwrap_or(rest.len());let mut text=entities(&rest[..len]);pos+=len;
            if skip.is_some(){continue;}
            if !style.pre{let mut s=String::new();let mut space=false;for c in text.chars(){if c.is_ascii_whitespace(){if !space{s.push(' ');}space=true;}else{s.push(c);space=false;}}text=s;if p.content.is_empty(){text=text.trim_start_matches(' ').to_string();}}
            push_text(&mut p,&text,&style);continue;
        }
        let mut quoted=None;let mut end=None;for(i,c)in rest.char_indices().skip(1){if let Some(q)=quoted{if c==q{quoted=None;}}else if c=='\''||c=='"'{quoted=Some(c);}else if c=='>'{end=Some(i);break;}}
        let Some(end)=end else{return Err("Unterminated HTML tag".into());};let tag=rest[1..end].trim();pos+=end+1;
        let closing=tag.starts_with('/');let body=tag.trim_start_matches('/').trim();let n=body.find(|c:char|c.is_whitespace()||c=='/').unwrap_or(body.len());let name=body[..n].to_ascii_lowercase();let a=attrs(&body[n..]);
        if let Some(s)=&skip{if closing&&s==&name{skip=None;}continue;}
        if matches!(name.as_str(),"script"|"style"|"head"|"iframe"|"object"){if !closing{skip=Some(name);}continue;}
        let block=matches!(name.as_str(),"p"|"div"|"li"|"h1"|"h2"|"h3"|"h4"|"h5"|"h6"|"tr");
        if block&&(!p.content.is_empty()||(closing&&name=="p")){result.paragraphs.push(p);p=empty();}
        if closing{if let Some(i)=stack.iter().rposition(|(n,_)|n==&name){style=stack[i].1.clone();stack.truncate(i);}continue;}
        if name=="br"{p.content.push(Inline::LineBreak{id:NodeId::new()});continue;}
        if name=="img"{
            if let Some(src)=a.get("src"){
                if let Some((mime,data))=src.strip_prefix("data:").and_then(|s|s.split_once(";base64,")){
                    if matches!(mime,"image/png"|"image/jpeg"|"image/gif"|"image/bmp"){
                        let data=unb64(data)?;let resource=ResourceId::new();result.resources.insert(resource.clone(),ClipboardResource{media_type:mime.into(),original_name:None,data});
                        let dimension=|name|a.get(name).and_then(|v|v.parse::<f32>().ok()).filter(|n|n.is_finite()&&*n>0.0).map(|n|(n*25.4/96.0).clamp(0.1,1000.0)).unwrap_or(25.4);
                        p.content.push(Inline::Image(model::InlineImage{id:NodeId::new(),resource,width_mm:dimension("width"),height_mm:dimension("height"),alt_text:a.get("alt").cloned(),effects:Default::default()}));continue;
                    }
                }
            }
            push_text(&mut p,a.get("alt").map(String::as_str).unwrap_or("[Picture]"),&style);continue;
        }
        if matches!(name.as_str(),"meta"|"link"|"hr"|"input"|"!doctype"){continue;}
        stack.push((name.clone(),style.clone()));
        match name.as_str(){"b"|"strong"=>style.properties.bold=Some(true),"i"|"em"=>style.properties.italic=Some(true),"u"=>style.properties.underline=Some(true),"pre"=>style.pre=true,"a"=>style.link=a.get("href").filter(|s|safe_link(s)).cloned(),_=>{}}
        if let Some(v)=a.get("style"){apply_css(&mut style,v);if block{for d in v.split(';'){if let Some((k,v))=d.split_once(':'){if k.trim().eq_ignore_ascii_case("text-align"){p.properties.alignment=Some(match v.trim(){"center"=>model::ParagraphAlignment::Center,"right"=>model::ParagraphAlignment::Right,"justify"=>model::ParagraphAlignment::Justify,_=>model::ParagraphAlignment::Left});}}}}}
        if let Some(c)=a.get("color"){style.properties.color=color(c);}if let Some(f)=a.get("face"){style.properties.font_family=Some(f.clone());}
    }
    if !p.content.is_empty(){result.paragraphs.push(p);}if result.paragraphs.is_empty(){return Err("HTML contains no editable content".into());}
    result.plain_text=result.paragraphs.iter().map(|p|paragraph_flat_text(&Paragraph { id: NodeId::new(), style: p.style.clone(), properties: p.properties.clone(), content: (p.content.clone()).into() })).collect::<Vec<_>>().join("\n");result.validate()?;Ok(result)
}
const BASE:&[u8;64]=b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
fn b64(data:&[u8])->String{let mut s=String::new();for c in data.chunks(3){let n=((c[0]as u32)<<16)|((c.get(1).copied().unwrap_or(0)as u32)<<8)|c.get(2).copied().unwrap_or(0)as u32;s.push(BASE[(n>>18)as usize]as char);s.push(BASE[((n>>12)&63)as usize]as char);s.push(if c.len()>1{BASE[((n>>6)&63)as usize]as char}else{'='});s.push(if c.len()>2{BASE[(n&63)as usize]as char}else{'='});}s}
fn unb64(s:&str)->Result<Vec<u8>,String>{let mut out=Vec::new();let mut n=0u32;let mut bits=0;for c in s.bytes(){if c==b'='{break;}if c.is_ascii_whitespace(){continue;}let value=BASE.iter().position(|b|*b==c).ok_or("Invalid embedded image base64")? as u32;n=(n<<6)|value;bits+=6;if bits>=8{bits-=8;out.push((n>>bits)as u8);if out.len()>MAX{return Err("Embedded image is too large".into());}}}Ok(out)}
#[cfg(test)] mod tests { include!("../tests/unit/clipboard/html.rs"); }

}

pub mod rtf {
//! Clipboard adapter only. RTF tokenization, decoding and writing are isolated
//! in format-rtf and shared with the file Open/Save adapter.
use super::*;
impl ClipboardFragment {
    pub fn from_document(document:&Document)->Result<Self,String>{
        let blocks=document.sections.iter().flat_map(|s|s.blocks.clone()).collect::<Vec<_>>();
        let mut ps=Vec::new();model::visit_paragraphs(&blocks,&mut ps);
        let spans=ps.iter().map(|p|(p.id.clone(),0,model::inline_text(&p.content).len())).collect::<Vec<_>>();
        let plain_text=ps.iter().map(|p|model::inline_text(&p.content)).collect::<Vec<_>>().join("\n");
        let mut f=Self{blocks: blocks.into(),plain_text,warnings:document.compatibility.iter().map(|i|i.message.clone()).collect(),..Default::default()};
        if f.blocks.iter().all(|b|matches!(b,Block::Paragraph(_))){
            for b in std::mem::take(&mut f.blocks){if let Block::Paragraph(p)=b{f.paragraphs.push(ClipboardParagraph{origin:Some(p.id),style:p.style,properties:p.properties,content:(p.content).into()});}}
        }else if f.blocks.len()==1&&matches!(f.blocks[0],Block::Table(_)){if let Block::Table(t)=f.blocks.remove(0){f.table=Some(t);}}
        f.capture_dependencies(document);f.capture_references(document,&spans);f.validate()?;Ok(f)
    }
    pub fn to_document(&self)->Result<Document,String>{
        self.validate()?;let mut d=Document::default();d.sections[0].blocks=(if !self.blocks.is_empty(){self.blocks.clone()}else if let Some(t)=&self.table{vec![Block::Table(t.clone())]}else{self.paragraphs.iter().map(|p|Block::Paragraph(Paragraph{id:p.origin.clone().unwrap_or_else(NodeId::new),style:p.style.clone(),properties:p.properties.clone(),content:(p.content.clone()).into()})).collect()}).into();
        d.resources=self.resources.iter().map(|(id,r)|(id.clone(),Resource { font_faces: Vec::new(), id:id.clone(),media_type:r.media_type.clone(),original_name:r.original_name.clone(),data:r.data.clone().into()})).collect();
        d.lists=self.lists.clone();d.review.footnotes=self.references.footnotes.clone();d.review.bookmarks=self.references.bookmarks.clone();d.review.comments=self.references.comments.clone();Ok(d)
    }
}
pub fn decode(bytes:&[u8])->Result<ClipboardFragment,String>{ClipboardFragment::from_document(&format_rtf::decode(bytes)?)}
pub fn encode(fragment:&ClipboardFragment)->Result<Vec<u8>,String>{format_rtf::encode(&fragment.to_document()?)}
}

pub mod bitmap {
//! Bounded PNG and packed Windows DIB clipboard interchange. No HBITMAP is
//! treated as global memory, and no linked color-profile file is ever opened.
use super::*;
use std::io::Cursor;
const MAX_BYTES:usize=64*1024*1024;
#[derive(Clone,Debug,PartialEq,Eq)]pub struct Bitmap{pub width:u32,pub height:u32,pub rgba:Vec<u8>}
fn size(width:u32,height:u32)->Result<usize,String>{
    if width==0||height==0||width>16384||height>16384{return Err("Clipboard picture dimensions are unsupported".into());}
    let len=(width as usize).checked_mul(height as usize).and_then(|v|v.checked_mul(4)).ok_or("Picture size overflow")?;
    if len>MAX_BYTES{return Err("Clipboard picture exceeds 64 MiB decoded".into());}Ok(len)
}
pub fn decode_image(encoded:&[u8])->Result<Bitmap,String>{
    if encoded.len()>32*1024*1024{return Err("Encoded clipboard image exceeds 32 MiB".into());}
    let mut reader=image::ImageReader::new(Cursor::new(encoded)).with_guessed_format().map_err(|e|e.to_string())?;
    let mut limits=image::Limits::default();limits.max_image_width=Some(16384);limits.max_image_height=Some(16384);limits.max_alloc=Some(MAX_BYTES as u64);reader.limits(limits);
    let image=reader.decode().map_err(|e|e.to_string())?.into_rgba8();size(image.width(),image.height())?;
    Ok(Bitmap{width:image.width(),height:image.height(),rgba:image.into_raw()})
}
impl Bitmap {
    pub fn png(&self)->Result<Vec<u8>,String>{
        if self.rgba.len()!=size(self.width,self.height)?{return Err("Invalid RGBA length".into());}
        let image=image::RgbaImage::from_raw(self.width,self.height,self.rgba.clone()).ok_or("Invalid RGBA image")?;let mut encoded=Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image).write_to(&mut encoded,image::ImageFormat::Png).map_err(|e|e.to_string())?;Ok(encoded.into_inner())
    }
    pub fn fragment(&self)->Result<ClipboardFragment,String>{
        let data=self.png()?;let id=ResourceId::new();let mut f=ClipboardFragment::default();
        // 96-DPI physical size, scaled to a normal text column without altering
        // pixel data. Later resizing remains lossless.
        let natural=self.width as f32*25.4/96.0;let scale=(150.0/natural).min(180.0/(self.height as f32*25.4/96.0)).min(1.0);
        f.plain_text="[Picture]".into();f.resources.insert(id.clone(),ClipboardResource{media_type:"image/png".into(),original_name:Some("Clipboard.png".into()),data});
        f.paragraphs.push(ClipboardParagraph{origin:None,style:None,properties:Default::default(),content:(vec![Inline::Image(model::InlineImage{id:NodeId::new(),resource:id,width_mm:(natural*scale).max(0.1),height_mm:(self.height as f32*25.4/96.0*scale).max(0.1),alt_text:Some("Clipboard picture".into()),effects:Default::default()})]).into()});f.validate()?;Ok(f)
    }
    pub fn dib_v5(&self)->Result<Vec<u8>,String>{
        if self.rgba.len()!=size(self.width,self.height)?{return Err("Invalid RGBA length".into());}
        let mut bytes=vec![0;124];let set=|bytes:&mut Vec<u8>,at:usize,n:u32|bytes[at..at+4].copy_from_slice(&n.to_le_bytes());
        set(&mut bytes,0,124);set(&mut bytes,4,self.width);set(&mut bytes,8,(-(self.height as i32))as u32);bytes[12..14].copy_from_slice(&1u16.to_le_bytes());bytes[14..16].copy_from_slice(&32u16.to_le_bytes());
        set(&mut bytes,16,3);set(&mut bytes,20,self.rgba.len() as u32);
        for(at,value)in [(40,0x00ff0000),(44,0x0000ff00),(48,0x000000ff),(52,0xff000000),(56,0x73524742),(108,4)]{set(&mut bytes,at,value);}
        for p in self.rgba.chunks_exact(4){bytes.extend_from_slice(&[p[2],p[1],p[0],p[3]]);}Ok(bytes)
    }
}
fn u16le(b:&[u8],at:usize)->Result<u16,String>{Ok(u16::from_le_bytes(b.get(at..at+2).ok_or("Truncated DIB header")?.try_into().map_err(|_|"Invalid DIB word")?))}
fn u32le(b:&[u8],at:usize)->Result<u32,String>{Ok(u32::from_le_bytes(b.get(at..at+4).ok_or("Truncated DIB header")?.try_into().map_err(|_|"Invalid DIB integer")?))}
pub fn decode_dib(bytes:&[u8])->Result<Bitmap,String>{
    if bytes.len()>MAX_BYTES+2048{return Err("DIB clipboard exceeds the size limit".into());}
    let header=u32le(bytes,0)? as usize;if ![40,108,124].contains(&header)||bytes.len()<header{return Err("Unsupported or truncated DIB header".into());}
    let w=u32le(bytes,4)? as i32;let h=u32le(bytes,8)? as i32;if w<=0||h==0||h==i32::MIN{return Err("Invalid DIB dimensions".into());}
    let width=w as u32;let height=h.unsigned_abs();let len=size(width,height)?;
    if u16le(bytes,12)?!=1{return Err("Invalid DIB plane count".into());}
    let bits=u16le(bytes,14)? as usize;let compression=u32le(bytes,16)?;
    if ![1,4,8,16,24,32].contains(&bits)||!(compression==0||(compression==3&&[16,32].contains(&bits))){return Err("Compressed or unsupported clipboard bitmap; copy it as PNG instead".into());}
    if header==124&&(u32le(bytes,112)?!=0||u32le(bytes,116)?!=0){return Err("Profile-bearing DIB clipboard is unsupported; copy it as PNG instead".into());}
    let mut offset=header;let mut masks=if bits==16{[0x7c00,0x03e0,0x001f,0]}else{[0x00ff0000,0x0000ff00,0x000000ff,0]};
    if compression==3{if header==40{for i in 0..3{masks[i]=u32le(bytes,offset+i*4)?;}offset+=12;}else{for i in 0..4{masks[i]=u32le(bytes,40+i*4)?;}}
        let mut used=0;for(i,mask)in masks.iter().copied().enumerate(){if i<3&&mask==0{return Err("Missing DIB color mask".into());}if mask!=0{let shifted=u64::from(mask>>mask.trailing_zeros());if !(shifted+1).is_power_of_two()||used&mask!=0||(bits<32&&mask>>bits!=0){return Err("Overlapping or noncontiguous DIB masks".into());}used|=mask;}}
    }
    let used=u32le(bytes,32)? as usize;let entries=if bits<=8{if used==0{1usize<<bits}else{used}}else{used};
    if entries>256||(bits<=8&&entries>1usize<<bits){return Err("Invalid DIB palette".into());}
    let palette=bytes.get(offset..offset+entries*4).ok_or("Truncated DIB palette")?;offset+=entries*4;
    let stride=((width as usize*bits+31)/32)*4;let end=offset.checked_add(stride.checked_mul(height as usize).ok_or("DIB size overflow")?).ok_or("DIB offset overflow")?;
    if end>bytes.len(){return Err("Truncated DIB pixels".into());}
    let component=|value:u32,mask:u32,default:u8|{if mask==0{default}else{let shift=mask.trailing_zeros();let maximum=u64::from(mask>>shift);((u64::from((value&mask)>>shift)*255+maximum/2)/maximum) as u8}};
    let mut rgba=vec![0;len];for y in 0..height as usize{let sy=if h<0{y}else{height as usize-1-y};let row=&bytes[offset+sy*stride..offset+(sy+1)*stride];
        for x in 0..width as usize{let p=&mut rgba[(y*width as usize+x)*4..(y*width as usize+x+1)*4];
            if bits<=8{let index=match bits{8=>row[x]as usize,4=>((row[x/2]>>(if x%2==0{4}else{0}))&15)as usize,_=>((row[x/8]>>(7-x%8))&1)as usize};let q=palette.get(index*4..index*4+4).ok_or("Invalid palette index")?;p.copy_from_slice(&[q[2],q[1],q[0],255]);}
            else if bits==24{let q=&row[x*3..x*3+3];p.copy_from_slice(&[q[2],q[1],q[0],255]);}
            else{let at=x*bits/8;let value=if bits==16{u16le(row,at)? as u32}else{u32le(row,at)?};p.copy_from_slice(&[component(value,masks[0],0),component(value,masks[1],0),component(value,masks[2],0),component(value,masks[3],255)]);}
        }
    }Ok(Bitmap{width,height,rgba})
}
pub fn single_picture(fragment:&ClipboardFragment)->Option<&ClipboardResource>{
    if fragment.table.is_some()||fragment.paragraphs.len()!=1||fragment.paragraphs[0].content.len()!=1{return None;}
    if let Inline::Image(i)=&fragment.paragraphs[0].content[0]{fragment.resources.get(&i.resource)}else{None}
}
#[cfg(test)] mod tests { include!("../tests/unit/clipboard/bitmap.rs"); }
}

