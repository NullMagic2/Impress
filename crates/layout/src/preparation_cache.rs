//! Conservative worker-local cache of shaped/composed blocks. Keys are compared
//! exactly (not hash-only); immutable encoded resources are retained and compared
//! by Arc identity, so allocator address reuse cannot resurrect stale resources.
//! Unchanged blocks reuse typography/Typst preparation. Semantic checkpoint
//! validation and conservative note/float dependencies live in incremental.rs.
use super::*;
use std::{cell::RefCell,collections::VecDeque,sync::{Arc,atomic::{AtomicUsize,Ordering}}};
static HITS:AtomicUsize=AtomicUsize::new(0);static MISSES:AtomicUsize=AtomicUsize::new(0);
pub fn cache_totals()->(usize,usize){(HITS.load(Ordering::Relaxed),MISSES.load(Ordering::Relaxed))}
struct Entry{key:Vec<u8>,styles:StyleSheet,resources:Vec<(ResourceId,Arc<Vec<u8>>,Vec<model::EmbeddedFontFace>)>,block:Arc<PreparedBlock>,bytes:usize}
/// The context is taken out while a block is prepared, so no borrow of the
/// cache is held then: a table prepares its rows in parallel, and while it
/// waits rayon may hand this same thread another block to prepare.
struct Worker{context:Option<WorkerContext>,entries:VecDeque<Entry>,bytes:usize}
thread_local!{static LOCAL:RefCell<Option<Worker>>=const{RefCell::new(None)};}
fn new_context()->WorkerContext{WorkerContext{fonts:FontCatalog::system(),typst:TypstTypesetter::new()}}
fn worker<R>(f:impl FnOnce(&mut Worker)->R)->R{LOCAL.with(|cell|{let mut slot=cell.borrow_mut();let w=slot.get_or_insert_with(||Worker{context:None,entries:VecDeque::new(),bytes:0});f(w)})}
/// This thread's context, or a new one when it is in use further up the stack.
fn take_context()->WorkerContext{worker(|w|w.context.take()).unwrap_or_else(new_context)}
fn return_context(context:WorkerContext){worker(|w|{if w.context.is_none(){w.context=Some(context);}})}
pub(super)fn with_context<R>(resources:&BTreeMap<ResourceId,Resource>,f:impl FnOnce(&mut WorkerContext)->R)->R{let mut context=take_context();context.fonts=FontCatalog::for_resources(resources);let result=f(&mut context);return_context(context);result}
pub(super) fn commands_cost(cs:&[DrawCommand])->usize{cs.iter().map(|c|match c{
 DrawCommand::Clipped{command,..}=>64+commands_cost(std::slice::from_ref(command.as_ref())),
 DrawCommand::TextRun(r)=>256+r.actual_text.as_ref().map_or(0,String::len)+r.glyphs.len()*std::mem::size_of::<render::GlyphVector>(),
 DrawCommand::Path(p)=>128+p.path.verbs.len()*std::mem::size_of::<render::PathVerb>(),_=>128,
}).sum()}
pub(super) fn hits_cost(h:&[TextHitRegion])->usize{h.iter().map(|r|128+r.node_id.len()+r.caret_stops.len()*std::mem::size_of::<CaretStop>()).sum()}
pub(super) fn cost(b:&PreparedBlock)->usize{match b{
 PreparedBlock::Paragraph{lines,marker,..}=>256+lines.iter().map(|l|128+commands_cost(&l.commands)+hits_cost(&l.hit_regions)).sum::<usize>()+marker.as_ref().map_or(0,|m|commands_cost(&m.commands)),
 PreparedBlock::Equation{fragment,..}=>128+commands_cost(&fragment.commands),PreparedBlock::Image(i)=>128+commands_cost(&i.commands),
 PreparedBlock::Table(t)=>256+t.rows.iter().map(|r|128+r.cells.iter().map(|c|512+commands_cost(&c.commands)+hits_cost(&c.hit_regions)).sum::<usize>()).sum::<usize>(),_=>128}}
pub(super)fn prepare(block:&Block,width:f32,styles:&StyleSheet,resources:&BTreeMap<ResourceId,Resource>,marker:Option<&String>,fields:&FieldContext)->Result<Arc<PreparedBlock>,LayoutError>{
 let key=serde_json::to_vec(&(block,width,marker,(fields.page_number,fields.page_count,fields.section_number,fields.page_number_format))).map_err(|e|LayoutError::RenderPreparation(e.to_string()))?;
 let hit=worker(|w|{
  let i=w.entries.iter().position(|e|e.key==key && e.styles==*styles && e.resources.len()==resources.len() && e.resources.iter().zip(resources.iter()).all(|((id,bytes,faces),(other,r))|id==other&&faces==&r.font_faces&&Arc::ptr_eq(bytes,&r.data)))?;
  let e=w.entries.remove(i).expect("cache index");let result=Arc::clone(&e.block);w.entries.push_back(e);Some(result)
 });
 if let Some(result)=hit{HITS.fetch_add(1,Ordering::Relaxed);return Ok(result);}
 MISSES.fetch_add(1,Ordering::Relaxed);
 let prepared=with_context(resources,|context|prepare_block(block,width,styles,context,resources,marker,fields))?;
 let bytes=cost(&prepared)+key.len()+serde_json::to_vec(styles).map_or(0,|v|v.len())+resources.len()*128;
 let prepared=Arc::new(prepared);
 let budget=(128*1024*1024/rayon::current_num_threads().max(1)).max(256*1024);
 if bytes<=budget{worker(|w|{
  while (w.bytes+bytes>budget||w.entries.len()>=512)&&!w.entries.is_empty(){let e=w.entries.pop_front().expect("nonempty");w.bytes-=e.bytes;}
  w.bytes+=bytes;w.entries.push_back(Entry{key,styles:styles.clone(),resources:resources.iter().map(|(id,r)|(id.clone(),r.data.clone(),r.font_faces.clone())).collect(),block:Arc::clone(&prepared),bytes});
 });}
 Ok(prepared)
}



