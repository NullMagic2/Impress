//! Conservative semantic pagination reuse. Entries contain BODY pages only:
//! headers/footers, object annotations and comment furniture are regenerated.
//! Equality is exact, with retained resource owners (never hash/address-only).
use super::*;
use std::{collections::BTreeMap, io::Write, sync::Arc};

const CACHE_BUDGET: usize = 128 * 1024 * 1024;
const KEY_BUDGET: usize = 8 * 1024 * 1024;
const MAX_BOUNDARIES: usize = 100_000;

/// Work performed by the last request, including all field-settling passes.
/// These are counts, not estimates of saved milliseconds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayoutWork {
    pub field_passes: usize,
    pub sections_reused: usize,
    pub sections_rebuilt: usize,
    pub blocks_paginated: usize,
    pub blocks_skipped: usize,
    pub prefix_pages_reused: usize,
    pub suffix_pages_reused: usize,
    pub checkpoint_resumes: usize,
    pub conservative_sections: usize,
}

struct LimitedBytes { bytes: Vec<u8>, remaining: usize }
impl Write for LimitedBytes {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.remaining { return Err(std::io::Error::other("cache key budget")); }
        self.bytes.extend_from_slice(bytes); self.remaining -= bytes.len(); Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}
fn key<T: serde::Serialize>(value: &T, limit: usize) -> Option<Vec<u8>> {
    let mut out = LimitedBytes { bytes: Vec::new(), remaining: limit };
    serde_json::to_writer(&mut out, value).ok()?; Some(out.bytes)
}

struct ResourceOwner { id: ResourceId, media_type: String, font_faces:Vec<model::EmbeddedFontFace>, data: Arc<Vec<u8>> }
pub(super) struct Environment { key: Vec<u8>, resources: Vec<ResourceOwner> }
impl Environment {
    fn same(&self, other: &Self) -> bool {
        self.key == other.key && self.resources.len() == other.resources.len()
            && self.resources.iter().zip(&other.resources).all(|(a,b)|
                a.id == b.id && a.media_type == b.media_type && a.font_faces==b.font_faces && Arc::ptr_eq(&a.data,&b.data))
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Boundary {
    cursor: FlowCursor,
    commands: usize,
    hits: usize,
    objects: usize,
}
impl Boundary {
    pub(super) fn capture(pages: &[VectorPage], cursor: FlowCursor) -> Self {
        let p = &pages[cursor.page_index];
        Self { cursor, commands:p.commands.len(), hits:p.hit_regions.len(), objects:p.object_regions.len() }
    }
    pub(super) fn restore(self, pages: &mut Vec<VectorPage>, cursor: &mut FlowCursor) {
        *cursor = self.cursor;
        pages.truncate(cursor.page_index + 1);
        self.truncate(&mut pages[cursor.page_index]);
    }
    fn truncate(self, page: &mut VectorPage) {
        page.commands.truncate(self.commands); page.hit_regions.truncate(self.hits);
        page.object_regions.truncate(self.objects);
    }
}
struct Entry {
    environment: Environment,
    keys: Vec<Vec<u8>>,
    pages: Vec<VectorPage>,
    cursor: FlowCursor,
    boundaries: Option<Vec<Boundary>>,
    prepared: Option<Vec<Arc<PreparedBlock>>>,
    bytes: usize,
    last_used: u64,
}

#[derive(Default)]
pub(crate) struct LayoutCache {
    entries: BTreeMap<NodeId, Entry>,
    retained_bytes: usize,
    clock: u64,
    fields: BTreeMap<NodeId, (FieldKind, Option<String>)>,
    field_inputs: Option<Environment>,
    pub derived_notes: references::DerivedEndnotes,
    pub work: LayoutWork,
}
impl LayoutCache {
    pub fn begin_request(&mut self) { self.work = LayoutWork::default(); }
    pub fn clear_fields(&mut self) { self.fields.clear(); self.field_inputs=None; }

    pub(super) fn field_inputs(document:&Document,background:bool)->Option<Environment>{
        // Warm-starting a changed field-dependent document can select a different
        // fixed point (for example 9 versus 10 pages). Seed only identical layout
        // inputs, excluding review furniture/journal that cannot change pagination.
        let bytes=key(&(&document.sections,&document.styles,&document.lists,
            &document.review.footnotes,&document.review.bookmarks,background),KEY_BUDGET)?;
        // A signature retains resource owners too. Do not keep an unbounded
        // document alive solely to warm-start its next field evaluation.
        let retained=document.resources.values().try_fold(0usize,|n,r|n.checked_add(r.data.len()))?;
        if retained>CACHE_BUDGET{return None;}
        Some(Environment{key:bytes,resources:document.resources.iter().map(|(id,r)|ResourceOwner{id:id.clone(),media_type:r.media_type.clone(),font_faces:r.font_faces.clone(),data:Arc::clone(&r.data)}).collect()})
    }
    pub(super) fn seed_fields(&self, document: &mut Document,inputs:Option<&Environment>) {
        if !inputs.zip(self.field_inputs.as_ref()).is_some_and(|(a,b)|a.same(b)){return;}
        walk_fields(document, &mut |field| {
            if let Some((kind,display)) = self.fields.get(&field.id) {
                if *kind == field.kind { field.display_text = display.clone(); }
            }
        });
    }
    pub(super) fn remember_fields(&mut self, document: &Document,inputs:Option<Environment>) {
        let mut fields = BTreeMap::new();
        fn visit(items:&[Inline], fields:&mut BTreeMap<NodeId,(FieldKind,Option<String>)>) {
            for i in items { match i {
                Inline::Field(f) if seeded(&f.kind) => { if fields.len()<MAX_BOUNDARIES { fields.insert(f.id.clone(),(f.kind.clone(),f.display_text.clone())); } },
                Inline::Hyperlink(h) => visit(&h.content,fields), _=>{}
            }}
        }
        for p in model::document_paragraphs(document) { visit(&p.content,&mut fields); }
        self.field_inputs=if fields.len()<MAX_BOUNDARIES{inputs}else{None};self.fields=fields;
    }

    pub(super) fn layout_section(&mut self, engine:&LayoutEngine, section:&Section, document:&Document,
        section_index:usize, global_start:usize, initial:Option<(usize,f32)>, extents:HeaderFooterExtents, cancel:Option<(&AtomicU64,u64)>)
        ->Result<(Vec<VectorPage>,FlowCursor),LayoutError>
    {
        check_cancel(cancel)?;
        self.clock=self.clock.wrapping_add(1);
        let has_notes=references::section_has_notes(section);
        // Global note-reference order affects numbering, even when a note lives
        // in a different section. Conservative dependency capture is intentional.
        let notes=has_notes.then(||(&document.review.footnotes,model::footnote_references(document)));
        let env_key=key(&(&section.properties,&document.styles,&document.lists,notes,
            section_index,global_start,initial,extents,engine.options.show_page_background),KEY_BUDGET);
        let markers=compute_list_markers(section,&document.lists,&document.styles);
        let mut remaining=KEY_BUDGET;
        let keys=section.blocks.iter().map(|b| { let marker=block_node_id(b).and_then(|id|markers.get(id));let bytes=key(&(b,marker),remaining)?;remaining-=bytes.len();Some(bytes) })
            .collect::<Option<Vec<_>>>();
        let (Some(env_key),Some(keys))=(env_key,keys) else {
            self.work.conservative_sections+=1;self.work.sections_rebuilt+=1;self.work.blocks_paginated+=section.blocks.len();
            return engine.layout_section(section,document,section_index,global_start,initial,extents,cancel);
        };
        let dependencies=resource_dependencies(section,document,has_notes);
        let environment=Environment { key:env_key, resources:dependencies.into_iter().filter_map(|id|document.resources.get(&id).map(|r|
            ResourceOwner{id,media_type:r.media_type.clone(),font_faces:r.font_faces.clone(),data:Arc::clone(&r.data)})).collect() };
        if let Some(old)=self.entries.get_mut(&section.id) {
            if old.environment.same(&environment) && old.keys==keys {
                old.last_used=self.clock;
                self.work.sections_reused+=1; self.work.blocks_skipped+=keys.len();
                return Ok((old.pages.clone(),old.cursor));
            }
        }
        let geometry=SectionGeometry::with_extents(section,extents);
        let eligible=geometry.column_count==1 && !has_notes && !floating::has_floats(section)
            && keys.len() < MAX_BOUNDARIES;
        let (pages,cursor,boundaries,prepared_cache)=if eligible {
            let fields=FieldContext { page_number:global_start+1,page_count:global_start+1,section_number:section_index+1 , page_number_format: model::PageNumberFormat::Decimal};
            let old=self.entries.get(&section.id).filter(|old|old.environment.same(&environment));
            let reusable=old.and_then(|entry|entry.prepared.as_ref().filter(|blocks|blocks.len()==entry.keys.len()).map(|blocks|(entry.keys.as_slice(),blocks.as_slice())));
            let (prefix,suffix)=reusable.map_or((0,0),|(old_keys,_)|{
                (keys.iter().zip(old_keys).take_while(|(a,b)|a==b).count(),
                 keys.iter().rev().zip(old_keys.iter().rev()).take_while(|(a,b)|a==b).count())
            });
            let prepared=section.blocks.par_iter().enumerate().map(|(index,b)| {
                check_cancel(cancel)?;
                if let Some((old_keys,blocks))=reusable {
                    if index<prefix {return Ok(Arc::clone(&blocks[index]));}
                    if index>=keys.len().saturating_sub(suffix) {
                        let old_index=old_keys.len()-(keys.len()-index);
                        return Ok(Arc::clone(&blocks[old_index]));
                    }
                }
                let marker=block_node_id(b).and_then(|id|markers.get(id));
                preparation_cache::prepare(b,geometry.column_width,&document.styles,&document.resources,marker,&fields)
            }).collect::<Vec<_>>().into_iter().collect::<Result<Vec<_>,_>>()?;
            let (pages,cursor,bounds)=resume(&prepared,&keys,&geometry,initial,engine.options.show_page_background,
                old,cancel,&mut self.work)?;
            (pages,cursor,Some(bounds),Some(prepared))
        } else {
            self.work.conservative_sections+=1;self.work.blocks_paginated+=section.blocks.len();
            let (pages,cursor)=engine.layout_section(section,document,section_index,global_start,initial,extents,cancel)?;
            (pages,cursor,None,None)
        };
        check_cancel(cancel)?;
        self.work.sections_rebuilt+=1;
        let bytes=environment.key.len()+environment.resources.iter().map(|r|160+r.data.len()).sum::<usize>()+keys.iter().map(Vec::len).sum::<usize>()
            +boundaries.as_ref().map_or(0,|b|b.len()*std::mem::size_of::<Boundary>())+page_cost(&pages)
            +prepared_cache.as_ref().map_or(0,|blocks:&Vec<Arc<PreparedBlock>>|blocks.iter().map(|block|preparation_cache::cost(block)).sum::<usize>());
        if let Some(old)=self.entries.remove(&section.id){self.retained_bytes=self.retained_bytes.saturating_sub(old.bytes);}
        if bytes<=CACHE_BUDGET {
            while self.retained_bytes+bytes>CACHE_BUDGET || self.entries.len()>=128 {
                let Some(id)=self.entries.iter().min_by_key(|(_,e)|e.last_used).map(|(id,_)|id.clone()) else{break;};
                let old=self.entries.remove(&id).expect("selected entry"); self.retained_bytes-=old.bytes;
            }
            self.retained_bytes+=bytes;
            self.entries.insert(section.id.clone(),Entry { environment,keys,pages:pages.clone(),cursor,boundaries,prepared:prepared_cache,bytes,last_used:self.clock });
        }
        Ok((pages,cursor))
    }
}

fn resource_dependencies(section:&Section,document:&Document,has_notes:bool)->std::collections::BTreeSet<ResourceId>{
    fn items(content:&[Inline],ids:&mut std::collections::BTreeSet<ResourceId>){for i in content{match i{
        Inline::Image(p)=>{ids.insert(p.resource.clone());},Inline::EmbeddedObject(o)=>{ids.insert(o.payload.clone());if let Some(preview)=&o.preview{ids.insert(preview.clone());}},Inline::Hyperlink(h)=>items(&h.content,ids),_=>{}
    }}}
    fn blocks(bs:&[Block],ids:&mut std::collections::BTreeSet<ResourceId>){for b in bs{match b{
        Block::Image(p)=>{ids.insert(p.resource.clone());},Block::Paragraph(p)=>items(&p.content,ids),
        Block::Table(t)=>for row in &t.rows{for c in &row.cells{blocks(&c.blocks,ids);}},_=>{}
    }}}
    let mut ids=std::collections::BTreeSet::new();blocks(&section.blocks,&mut ids);
    if has_notes{for note in document.review.footnotes.values(){blocks(&note.blocks,&mut ids);}}
    ids
}

fn seeded(kind:&FieldKind)->bool { matches!(kind,FieldKind::PageNumber|FieldKind::PageCount|FieldKind::PageReference{..}) }
fn walk_fields(document:&mut Document, f:&mut impl FnMut(&mut model::Field)) {
    fn items(is:&mut [Inline], f:&mut impl FnMut(&mut model::Field)) { for i in is {match i {
        Inline::Field(field) if seeded(&field.kind)=>f(field), Inline::Hyperlink(h) if references::dynamic_inline(&h.content)=>items(&mut h.content,f),_=>{}
    }}}
    fn blocks(bs:&mut model::Shared<Vec<Block>>, f:&mut impl FnMut(&mut model::Field)){
        if !references::dynamic_blocks(bs){return;}
        for b in bs{match b{
        Block::Paragraph(p) if references::dynamic_inline(&p.content)=>items(&mut p.content,f),Block::Table(t)=>for row in &mut t.rows{for cell in &mut row.cells{blocks(&mut cell.blocks,f);}},_=>{}
    }}}
    for s in &mut document.sections {
        blocks(&mut s.blocks,f);
        for h in s.header_footers_mut(){blocks(&mut h.blocks,f);}
    }
    for note in document.review.footnotes.values_mut(){blocks(&mut note.blocks,f);}
}

fn page_cost(pages:&[VectorPage])->usize {
    let mut rasters=std::collections::BTreeSet::new();
    let image_bytes=pages.iter().flat_map(|p|p.commands.iter()).filter_map(|c|match c.unclipped().0{DrawCommand::Image{image,..}=>Some(image),_=>None})
        .filter(|image|rasters.insert(image.key)).map(|image|image.pixels.len()+image.levels.len()*std::mem::size_of::<render::RasterMip>()).sum::<usize>();
    image_bytes+pages.iter().map(|p|256+preparation_cache::commands_cost(&p.commands)+preparation_cache::hits_cost(&p.hit_regions)
        +p.object_regions.iter().map(|o|192+o.node_id.len()+match &o.kind{ObjectKind::TableCell(c)=>c.column_widths_pt.len()*4,_=>0}).sum::<usize>()).sum::<usize>()
}

/// Resume only at a complete block boundary. Restoring the active page's prefix
/// is essential: its old suffix must not survive when line heights change.
fn resume<T:AsRef<PreparedBlock>>(prepared:&[T],keys:&[Vec<u8>],geometry:&SectionGeometry,initial:Option<(usize,f32)>,
    background:bool,old:Option<&Entry>,cancel:Option<(&AtomicU64,u64)>,work:&mut LayoutWork)
    ->Result<(Vec<VectorPage>,FlowCursor,Vec<Boundary>),LayoutError>
{
    let old=old.filter(|e|e.boundaries.as_ref().is_some_and(|b|b.len()==e.keys.len()+1));
    let mut prefix=old.map_or(0,|e|keys.iter().zip(&e.keys).take_while(|(a,b)|a==b).count());
    // Keep-with-next is a LOOK-AHEAD dependency. A changed follower invalidates
    // every preceding block in its keep chain, not merely the changed block.
    while prefix>0 && matches!(prepared[prefix-1].as_ref(),PreparedBlock::Paragraph{keep_with_next:true,..}){prefix-=1;}
    let suffix=old.map_or(0,|e|keys.iter().rev().zip(e.keys.iter().rev()).take_while(|(a,b)|a==b).count());
    let mut pages=vec![new_page(geometry.page_width,geometry.page_height,background)];
    let (col,y)=initial.unwrap_or((0,geometry.content_top));
    let mut cursor=FlowCursor{page_index:0,column_index:col.min(geometry.column_count.saturating_sub(1)),y:y.clamp(geometry.content_top,geometry.content_bottom)};
    let mut boundaries=Vec::with_capacity(prepared.len()+1);
    if prefix>0 {
        let old=old.expect("prefix has baseline");let bs=old.boundaries.as_ref().expect("checked");let boundary=bs[prefix];
        pages=old.pages[..=boundary.cursor.page_index].to_vec(); boundary.truncate(pages.last_mut().expect("current page"));
        cursor=boundary.cursor;boundaries.extend_from_slice(&bs[..prefix]);
        work.checkpoint_resumes+=1;work.blocks_skipped+=prefix;work.prefix_pages_reused+=cursor.page_index;
    }
    for index in prefix..=prepared.len() {
        check_cancel(cancel)?;
        let now=Boundary::capture(&pages,cursor);
        if index<keys.len() && index>=keys.len().saturating_sub(suffix) {
            if let Some(old)=old {
                let old_index=old.keys.len()-(keys.len()-index);
                let bs=old.boundaries.as_ref().expect("checked");let before=bs[old_index];
                if before.cursor==cursor {
                    let current=&mut pages[cursor.page_index];let prior=&old.pages[cursor.page_index];
                    current.commands.extend_from_slice(&prior.commands[before.commands..]);
                    current.hit_regions.extend_from_slice(&prior.hit_regions[before.hits..]);
                    current.object_regions.extend_from_slice(&prior.object_regions[before.objects..]);
                    pages.extend_from_slice(&old.pages[cursor.page_index+1..]);
                    for boundary in &bs[old_index..] {
                        let mut boundary=*boundary;
                        if boundary.cursor.page_index==cursor.page_index {
                            boundary.commands=now.commands+(boundary.commands-before.commands);
                            boundary.hits=now.hits+(boundary.hits-before.hits);
                            boundary.objects=now.objects+(boundary.objects-before.objects);
                        }
                        boundaries.push(boundary);
                    }
                    work.blocks_skipped+=keys.len()-index;work.suffix_pages_reused+=old.pages.len()-cursor.page_index-1;
                    return Ok((pages,old.cursor,boundaries));
                }
            }
        }
        boundaries.push(now);
        if index==prepared.len(){break;}
        paginate_block(prepared,index,&mut pages,&mut cursor,geometry,background)?;
        work.blocks_paginated+=1;
    }
    Ok((pages,cursor,boundaries))
}

#[cfg(test)] #[path = "../tests/unit/incremental.rs"] mod tests;


