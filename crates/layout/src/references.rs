//! Page-bound footnotes and generated page fields. Preparation is immutable;
//! bounded repeat passes never modify the user's document or undo history.
use super::*;
use model::{Footnote,TextAnchor,footnote_references,visit_paragraphs};
use std::collections::BTreeSet;

pub(super) fn section_has_notes(section:&Section)->bool {
    fn has(items:&[Inline])->bool{items.iter().any(|i|match i{Inline::Field(f)=>matches!(&f.kind,FieldKind::FootnoteReference{..}),Inline::Hyperlink(h)=>has(&h.content),_=>false})}
    let mut ps=Vec::new();visit_paragraphs(&section.blocks,&mut ps);ps.iter().any(|p|has(&p.content))
}
/// Read-only dependency probe before entering a copy-on-write branch.
pub(super) fn dynamic_inline(items:&[Inline])->bool {
    items.iter().any(|i|match i {
        Inline::Field(f)=>matches!(&f.kind,FieldKind::PageNumber|FieldKind::PageCount|FieldKind::PageReference{..}|FieldKind::FootnoteReference{..}),
        Inline::Hyperlink(h)=>dynamic_inline(&h.content),_=>false,
    })
}
pub(super) fn dynamic_blocks(blocks:&[Block])->bool {
    blocks.iter().any(|b|match b {Block::Paragraph(p)=>dynamic_inline(&p.content),
        Block::Table(t)=>t.rows.iter().any(|r|r.cells.iter().any(|c|dynamic_blocks(&c.blocks))),_=>false})
}
pub(super) fn has_dynamic_fields(document:&Document)->bool {
    fn has(items:&[Inline])->bool{items.iter().any(|i|match i{Inline::Field(f)=>matches!(&f.kind,FieldKind::PageNumber|FieldKind::PageCount|FieldKind::PageReference{..}|FieldKind::FootnoteReference{..}),Inline::Hyperlink(h)=>has(&h.content),_=>false})}
    model::document_paragraphs(document).iter().any(|p|has(&p.content))
}
fn inline_source_len(items:&[Inline])->usize { items.iter().map(|i|match i {
    Inline::Text(t)=>t.text.len(),Inline::LineBreak{..}|Inline::PaginationBreak{..}|Inline::Tab{..}=>1,
    Inline::Hyperlink(h)=>inline_source_len(&h.content),_=>3,
}).sum() }
fn page_at(pages:&VectorDocument,node:&NodeId,offset:usize)->Option<usize>{
    for (index,page) in pages.pages.iter().enumerate(){if page.hit_regions.iter().any(|r|r.node_id==node.0&&r.byte_range.contains(&offset)){return Some(index+1);}}
    for (index,page) in pages.pages.iter().enumerate(){if page.hit_regions.iter().any(|r|r.node_id==node.0&&r.caret_stops.iter().any(|s|s.byte_offset==offset)){return Some(index+1);}}None
}
fn page_label(pages:Option<&VectorDocument>,page_number:usize)->String {
    pages.and_then(|pages|pages.pages.get(page_number.saturating_sub(1)))
        .and_then(|page|page.page_number_label.clone())
        .unwrap_or_else(||page_number.to_string())
}
pub(super) fn update_fields(document:&mut Document,pages:Option<&VectorDocument>)->bool {
    let mut numbers=BTreeMap::new();let refs=footnote_references(document);
    let mut counts=[0usize;2];
    for (_,note) in &refs {if !numbers.contains_key(note){let k=usize::from(document.review.footnotes.get(note).is_some_and(|n|n.kind==model::NoteKind::Endnote));counts[k]+=1;numbers.insert(note.clone(),counts[k]);}}
    let mut bookmark_pages = BTreeMap::new();
    if let Some(pages) = pages.filter(|_| !document.review.bookmarks.is_empty()) {
        // Index just the targeted paragraphs in one pass. A long TOC should
        // not rescan all the laid-out text once per entry on every field pass.
        let nodes: BTreeSet<_> = document.review.bookmarks.values()
            .filter_map(|b| b.range.as_ref().map(|r| r.start.paragraph.0.as_str())).collect();
        let mut hits: HashMap<&str, Vec<(usize, &TextHitRegion)>> = HashMap::new();
        for (page, value) in pages.pages.iter().enumerate() {
            for region in &value.hit_regions {
                let node = region.node_id.as_str();
                if nodes.contains(node) { hits.entry(node).or_default().push((page + 1, region)); }
            }
        }
        for bookmark in document.review.bookmarks.values() {
            let Some(range) = &bookmark.range else { continue; };
            let Some(regions) = hits.get(range.start.paragraph.0.as_str()) else { continue; };
            let offset = range.start.offset;
            let target = regions.iter().find(|(_, hit)| hit.byte_range.contains(&offset))
                .or_else(|| regions.iter().find(|(_, hit)| hit.caret_stops.iter().any(|stop| stop.byte_offset == offset)));
            if let Some((page, _)) = target { bookmark_pages.insert(bookmark.name.clone(), *page); }
        }
    }
    let mut changed=false;
    fn walk(items:&mut[Inline],node:&NodeId,offset:&mut usize,header:bool,pages:Option<&VectorDocument>,numbers:&BTreeMap<NodeId,usize>,bookmarks:&BTreeMap<String,usize>,changed:&mut bool){
        for i in items{match i {
            Inline::Text(t)=>*offset+=t.text.len(),Inline::LineBreak{..}|Inline::PaginationBreak{..}|Inline::Tab{..}=>*offset+=1,
            Inline::Hyperlink(h)=>{
                if dynamic_inline(&h.content){walk(&mut h.content,node,offset,header,pages,numbers,bookmarks,changed);}
                else {*offset+=inline_source_len(&h.content);}
            },
            Inline::Field(f)=>{
                let text=match &f.kind{
                    FieldKind::FootnoteReference{note}=>Some(numbers.get(note).map(usize::to_string).unwrap_or_else(||"?".into())),
                    FieldKind::PageReference{bookmark}=>Some(bookmarks.get(bookmark).map(|page|page_label(pages,*page)).unwrap_or_else(||"?".into())),
                    FieldKind::PageNumber if !header=>Some(page_label(pages,pages.and_then(|p|page_at(p,node,*offset)).unwrap_or(1))),
                    FieldKind::PageCount if !header=>Some(pages.map(|p|p.pages.len()).unwrap_or(1).to_string()),
                    FieldKind::PageNumber|FieldKind::PageCount|FieldKind::SectionNumber if header=>None,
                    _=>{*offset+=3;continue;},
                };
                if f.display_text!=text{f.display_text=text;*changed=true;}*offset+=3;
            },_=>*offset+=3,
        }}
    }
    fn blocks(bs:&mut model::Shared<Vec<Block>>,header:bool,pages:Option<&VectorDocument>,numbers:&BTreeMap<NodeId,usize>,bookmarks:&BTreeMap<String,usize>,changed:&mut bool){
        if !dynamic_blocks(bs){return;}
        for b in bs{match b{Block::Paragraph(p) if dynamic_inline(&p.content)=>walk(&mut p.content,&p.id,&mut 0,header,pages,numbers,bookmarks,changed),Block::Table(t)=>for row in &mut t.rows{for c in &mut row.cells{blocks(&mut c.blocks,header,pages,numbers,bookmarks,changed);}},_=>{}}}
    }
    for s in &mut document.sections {
        blocks(&mut s.blocks,false,pages,&numbers,&bookmark_pages,&mut changed);
        for h in s.header_footers_mut(){blocks(&mut h.blocks,true,pages,&numbers,&bookmark_pages,&mut changed);}
    }
    for note in document.review.footnotes.values_mut(){blocks(&mut note.blocks,false,pages,&numbers,&bookmark_pages,&mut changed);}
    changed
}

const NOTE_LABEL_HEIGHT:f32=12.0;
const NOTE_GAP:f32=3.0;
struct PreparedNote { atoms:Vec<PreparedCellFragment>, first:PreparedMarker, continued:PreparedMarker }
pub(super) struct NotePiece { note:NodeId, start:usize, end:usize }
pub(super) struct NotePlan { pub(super) pages:Vec<Vec<NotePiece>>, pub(super) reserves:Vec<f32> }
pub(super) struct PreparedNotes { references:Vec<(TextAnchor,NodeId)>, fragments:BTreeMap<NodeId,PreparedNote> }
impl PreparedNotes {
    pub(super) fn new(section:&Section,document:&Document,geometry:&SectionGeometry,fields:&FieldContext)->Result<Self,LayoutError>{
        let mut ps=Vec::new();visit_paragraphs(&section.blocks,&mut ps);let ids:BTreeSet<_>=ps.iter().map(|p|p.id.clone()).collect();
        let all_refs=footnote_references(document).into_iter().filter(|(_,id)|document.review.footnotes.get(id).is_none_or(|n|n.kind==model::NoteKind::Footnote)).collect::<Vec<_>>();let mut numbers=BTreeMap::new();
        for (_,note) in &all_refs{if !numbers.contains_key(note){numbers.insert(note.clone(),numbers.len()+1);}}
        let references=all_refs.into_iter().filter(|(anchor,_)|ids.contains(&anchor.paragraph)).collect::<Vec<_>>();
        if references.len()>4096{return Err(LayoutError::RenderPreparation("Too many footnote references in one section".into()));}
        let mut fragments=BTreeMap::new();
        if references.is_empty(){return Ok(Self{references,fragments});}
        let width=geometry.text_width();
        let label_style=CharacterProperties{font_size_pt:Some(9.0),..Default::default()};
        for (_,id) in &references {
            if fragments.contains_key(id){continue;}
            let note=document.review.footnotes.get(id).ok_or_else(||LayoutError::RenderPreparation("Footnote reference has no note content".into()))?;validate_note(note)?;
            let mut atoms=Vec::new();
            for block in &note.blocks{
                if let Block::Paragraph(p)=block{
                    let prepared=preparation_cache::prepare(block,width,&document.styles,&document.resources,None,fields)?;
                    if let PreparedBlock::Paragraph{lines,space_before,space_after,left_indent,..}=prepared.as_ref(){
                        let len=lines.len();for (index,line)in lines.iter().enumerate(){
                            let before=if index==0{space_before.max(0.0)}else{0.0};let after=if index+1==len{space_after.max(0.0)}else{0.0};let baseline=before+line.ascent;
                            atoms.push(PreparedCellFragment{height:before+line.height+after,commands:line.commands.iter().map(|c|c.translated(*left_indent,baseline)).collect(),hit_regions:line.hit_regions.iter().map(|r|r.translated(*left_indent,baseline)).collect(),object_regions:Vec::new()});
                        }
                        if len==0{let _=p;}
                    }
                }else{
                    // Tables, pictures and display equations are indivisible note
                    // atoms. Oversized atoms fail explicitly, never get clipped.
                    atoms.push(preparation_cache::with_context(&document.resources,|ctx|prepare_block_fragment(std::slice::from_ref(block),width,&document.styles,ctx,&document.resources,fields))?);
                }
                if atoms.len()>100000{return Err(LayoutError::RenderPreparation("Footnote exceeds 100,000 prepared lines/objects".into()));}
            }
            if atoms.is_empty(){atoms.push(PreparedCellFragment{height:1.0,commands:Vec::new(),hit_regions:Vec::new(),object_regions:Vec::new()});}
            if atoms.iter().any(|a|!a.height.is_finite()||a.height<=0.0){return Err(LayoutError::RenderPreparation("Invalid note line/object height".into()));}
            let number=numbers.get(id).copied().unwrap_or(0);
            let (first,continued)=preparation_cache::with_context(&document.resources,|ctx|Ok::<_,LayoutError>((
                prepare_list_marker(&format!("{number}."),&label_style,&ctx.fonts)?,
                prepare_list_marker(&format!("{number}. (continued)"),&label_style,&ctx.fonts)?)))?;
            fragments.insert(id.clone(),PreparedNote{atoms,first,continued});
        }
        Ok(Self{references,fragments})
    }
    pub(super) fn assign(&self,pages:&[VectorPage])->Result<Vec<Vec<NodeId>>,LayoutError>{
        let mut result=vec![Vec::new();pages.len()];let mut assigned=BTreeSet::new();
        for (anchor,note) in &self.references{
            if !assigned.insert(note.clone()){return Err(LayoutError::RenderPreparation("One footnote is referenced more than once; duplicate the note instead".into()));}
            let page=pages.iter().position(|p|p.hit_regions.iter().any(|r|r.node_id==anchor.paragraph.0&&r.byte_range.contains(&anchor.offset))).ok_or_else(||LayoutError::RenderPreparation("A footnote reference could not be located on a page".into()))?;
            result[page].push(note.clone());
        }Ok(result)
    }
    pub(super) fn plan(&self,assignment:&[Vec<NodeId>],geometry:&SectionGeometry)->Result<NotePlan,LayoutError>{
        let capacity=geometry.column_height()*0.60-8.0;
        let mut plan=NotePlan{pages:Vec::new(),reserves:Vec::new()};let mut pending=Vec::<(NodeId,usize)>::new();let mut index=0;
        while index<assignment.len()||!pending.is_empty(){
            if index>=10000{return Err(LayoutError::RenderPreparation("Footnote continuation exceeds 10,000 pages".into()));}
            if let Some(ids)=assignment.get(index){pending.extend(ids.iter().cloned().map(|id|(id,0)));}
            if pending.is_empty(){plan.pages.push(Vec::new());plan.reserves.push(0.0);index+=1;continue;}
            let minimum=pending.iter().map(|(id,start)|self.fragments.get(id).and_then(|n|n.atoms.get(*start)).map(|a|a.height+NOTE_LABEL_HEIGHT+NOTE_GAP).ok_or_else(||LayoutError::RenderPreparation("Invalid footnote continuation cursor".into()))).collect::<Result<Vec<_>,_>>()?;
            if minimum.iter().any(|h|!h.is_finite()||*h<=0.0)||minimum.iter().sum::<f32>()>capacity{
                return Err(LayoutError::RenderPreparation("The first line/object of the concurrent footnotes cannot fit in the reserved note area. Reduce the oversized note object or the number of same-page note references.".into()));
            }
            let mut available=capacity;let mut remaining_minimum=minimum.iter().sum::<f32>();let mut used=0.0;let mut pieces=Vec::new();let mut next=Vec::new();
            for ((id,start),minimum)in pending.into_iter().zip(minimum){
                remaining_minimum-=minimum;let limit=available-remaining_minimum;let atoms=&self.fragments[&id].atoms;
                let mut end=start;let mut height=NOTE_LABEL_HEIGHT+NOTE_GAP;
                while end<atoms.len()&&height+atoms[end].height<=limit+0.01{height+=atoms[end].height;end+=1;}
                if end==start{return Err(LayoutError::RenderPreparation("Footnote continuation could not make progress".into()));}
                available-=height;used+=height;pieces.push(NotePiece{note:id.clone(),start,end});if end<atoms.len(){next.push((id,end));}
            }
            plan.pages.push(pieces);plan.reserves.push(used+8.0);pending=next;index+=1;
        }
        Ok(plan)
    }
    pub(super) fn paint(&self,pages:&mut[VectorPage],plan:&NotePlan,geometry:&SectionGeometry)->Result<(),LayoutError>{
        for (index,pieces)in plan.pages.iter().enumerate(){if pieces.is_empty(){continue;}
            let mut y=geometry.content_bottom-plan.reserves[index];
            if pages[index].hit_regions.iter().any(|r|r.rect.y+r.rect.height>y+1.0&&r.rect.y>=geometry.content_top)||pages[index].object_regions.iter().any(|r|r.rect.y+r.rect.height>y+1.0&&r.rect.y>=geometry.content_top){return Err(LayoutError::RenderPreparation("An oversized body object overlaps the footnote area".into()));}
            let page = &mut pages[index];
            if page.has_paint {
                page.commands.push(DrawCommand::FillRect{rect:Rect::new(geometry.content_left,y,geometry.column_width.min(140.0),0.5),paint:Paint::Solid(Color::BLACK)});
            }
            y += 8.0;
            for piece in pieces{
                let note=&self.fragments[&piece.note];let label=if piece.start==0{&note.first}else{&note.continued};
                if page.has_paint { page.commands.extend(label.commands.iter().map(|c|c.translated(geometry.content_left,y+9.0))); }
                y += NOTE_LABEL_HEIGHT;
                for atom in &note.atoms[piece.start..piece.end]{
                    if page.has_paint { page.commands.extend(atom.commands.iter().map(|c|c.translated(geometry.content_left,y))); }
                    page.hit_regions.extend(atom.hit_regions.iter().map(|r|r.translated(geometry.content_left,y)));
                    // Object manipulation inside notes remains restricted. Text
                    // regions use the original semantic offsets, without labels.
                    y+=atom.height;
                }y+=NOTE_GAP;
            }
        }Ok(())
    }
}

fn validate_note(note:&Footnote)->Result<(),LayoutError>{
    fn nested(items:&[Inline])->bool{items.iter().any(|i|match i{Inline::Field(f)=>matches!(&f.kind,FieldKind::FootnoteReference{..}),Inline::Hyperlink(h)=>nested(&h.content),_=>false})}
    let mut ps=Vec::new();visit_paragraphs(&note.blocks,&mut ps);
    if ps.iter().any(|p|nested(&p.content)){return Err(LayoutError::RenderPreparation("Nested footnotes are not supported".into()));}Ok(())
}

#[cfg(test)] #[path = "../tests/unit/references/continuation_tests.rs"] mod continuation_tests;

/// Endnotes are a derived final section. IDs of authored note paragraphs remain
/// unchanged for selection/editing; generated labels never alter note contents.
pub(super) struct DerivedEndnotes{section:NodeId,title:Paragraph,labels:BTreeMap<NodeId,Paragraph>}
impl Default for DerivedEndnotes{fn default()->Self{Self{section:NodeId::new(),title:Paragraph::plain("Endnotes"),labels:BTreeMap::new()}}}
pub(super) fn append_endnotes(document:&mut Document,derived:&mut DerivedEndnotes)->Result<(),LayoutError>{
    let mut seen=BTreeSet::new();let mut notes=Vec::new();
    for (_,id) in footnote_references(document){
        if let Some(note)=document.review.footnotes.get(&id){if note.kind==model::NoteKind::Endnote {
            if !seen.insert(id){return Err(LayoutError::RenderPreparation("Duplicate endnote references are not supported".into()));}
            validate_note(note)?;notes.push(note.clone());
        }}
    }
    if notes.is_empty(){return Ok(());}
    if notes.len()>4096{return Err(LayoutError::RenderPreparation("Document exceeds 4096 endnotes".into()));}
    let mut section=document.sections.last().cloned().unwrap_or_default();section.id=derived.section.clone();section.blocks.clear();
    section.properties.start_on=model::SectionStart::NewPage;section.properties.column_count=1;
    let mut title=derived.title.clone();title.properties.keep_with_next=Some(true);section.blocks.push(Block::Paragraph(title));
    derived.labels.retain(|id,_|seen.contains(id));
    for (i,note) in notes.into_iter().enumerate(){let template=derived.labels.entry(note.id.clone()).or_insert_with(||Paragraph::plain(""));
        let mut label=template.clone();if let Some(Inline::Text(t))=label.content.first_mut(){t.text=format!("{}.",i+1);}label.properties.keep_with_next=Some(true);label.properties.space_after_pt=Some(2.0);section.blocks.push(Block::Paragraph(label));section.blocks.extend(note.blocks);}
    document.sections.push(section);Ok(())
}
