//! Heavy import/export and validation run off the event thread. A save result
//! can mark clean only the exact revision it wrote; later typing stays dirty.
use super::*;
use std::sync::mpsc::{self,Receiver,TryRecvError};
enum Completed{Saved(PathBuf,Option<String>),Opened(PathBuf,Document)}
pub struct Job{revision:u64,receiver:Receiver<Result<Completed,String>>}
pub fn queue_save(parent:&impl WxWidget,state:&Rc<RefCell<CanvasState>>,force:bool)->Result<(),String>{
 let(revision,current)={let s=state.borrow();if s.file_job.is_some()||s.clipboard_job.is_some(){return Err("A file or clipboard operation is already running".into());}(s.editor.revision,s.current_path.clone())};
 let settings=crate::save_options::settings_path()?;
 let previous=crate::save_options::load(&settings);
 let(path,embed)=if force||current.is_none(){
  let Some(choice)=choose_save_path(parent,current.as_deref(),previous)? else{return Ok(());};choice
 }else{(current.expect("known path"),previous)};
 let font_options=if state.borrow().formats.for_path(&path).is_some_and(|f|matches!(f.id(),"impress"|"docx")) {
  Some((settings,embed))
 }else{None};
 let mut s=state.borrow_mut();if s.editor.revision!=revision||s.file_job.is_some()||s.clipboard_job.is_some(){return Err("Document changed while the save dialog was open; save again".into());}let doc=document_snapshot(&mut s);let formats=s.formats.clone();let(tx,receiver)=mpsc::channel();
 let docx_contents = !doc.review.contents_paragraphs.is_empty() && formats.for_path(&path).is_some_and(|f| f.id() == "docx");
 let ready_scene = (docx_contents && s.layout_error.is_none()
     && s.display_generation == Some(s.layout_generation) && !s.typing_layout_pending
     && !equation_ui::previewing(&s)).then(|| Arc::clone(&s.vector_document));
 std::thread::Builder::new().name("impress-save".into()).spawn(move||{
  let mut doc = doc;
  if docx_contents {
   let scene = ready_scene.or_else(|| layout::LayoutEngine::default().layout_document(&doc).ok().map(Arc::new));
   if let Some(scene) = scene { layout::cache_page_field_labels(Arc::make_mut(&mut doc), &scene); }
  }
  let result=(if let Some((_,embed))=&font_options {crate::file_io::save_with_font_embedding(&path,&doc,&formats,*embed)}else{save_document(&path,&doc,&formats)}).map(|()|{
  let warning=font_options.and_then(|(settings,embed)|crate::save_options::store(&settings,embed).err()).map(|e|format!("Could not remember font embedding preference: {e}"));
  let warning=warning.or_else(|| formats.for_path(&path).filter(|format|format.id()=="doc").and_then(|format|{
   let count=format.export_warnings(&doc).len();
   (count>0).then(||format!("DOC export simplified {count} features. See Review → Compatibility."))
  }));
  Completed::Saved(path,warning)
 });let _=tx.send(result);}).map_err(|e|e.to_string())?;
 s.file_job=Some(Job{revision,receiver});s.action_status=Some("Saving a frozen revision… You may continue typing.".into());Ok(())
}
pub fn queue_open(path:PathBuf,state:&Rc<RefCell<CanvasState>>)->Result<(),String>{
 start_open(&mut state.borrow_mut(),path)
}
/// Opening adds a document to the window's stack, so the one shown need not be saved first.
pub fn start_open(s:&mut CanvasState,path:PathBuf)->Result<(),String>{
 if s.file_job.is_some()||s.clipboard_job.is_some()||s.pending_close{return Err("Finish the current file operation before opening another document".into());}
 let revision=s.editor.revision;let formats=s.formats.clone();let(tx,receiver)=mpsc::channel();std::thread::Builder::new().name("impress-open".into()).spawn(move||{let result=load_document(&path,&formats).map_err(|error| format!("{}: {error}", path.display())).map(|d|Completed::Opened(path,d));let _=tx.send(result);}).map_err(|e|e.to_string())?;
 s.file_job=Some(Job{revision,receiver});s.action_status=Some("Reading and validating document…".into());Ok(())
}
pub fn poll(s:&mut CanvasState){let result=s.file_job.as_ref().map(|j|j.receiver.try_recv());
 match result{Some(Ok(result))=>{let job=s.file_job.take().expect("completed job");match result{
  Ok(Completed::Saved(path,preference_warning))=>{s.current_path=Some(path.clone());s.editor.mark_snapshot_saved(job.revision);
   let close_requested=std::mem::take(&mut s.pending_close);
   s.close_ready=close_requested&&close_after_save(job.revision,s.editor.revision,s.editor.dirty,crate::equation_ui::previewing(s),s.input.active()||s.object_drag.is_some());if s.editor.revision==job.revision{s.recovery.mark_clean();s.action_status=Some(format!("Saved {}",path.display()));}else{s.action_status=Some(format!("Saved revision {} to {}; newer edits remain unsaved",job.revision,path.display()));}
   if let Some(warning)=preference_warning {s.action_status=Some(format!("{}; {warning}",s.action_status.as_deref().unwrap_or("Saved")));}
   crate::documents::saved(s,job.revision);
   remember_recent(s,&path);
   if let Some(open_path)=s.pending_open.take(){
    if s.editor.revision==job.revision&&!s.editor.dirty{
     if let Err(error)=start_open(s,open_path){s.action_status=Some(format!("Saved, but could not start Open: {error}"));}
    }else{s.action_status=Some("The document changed during Save, so Open was cancelled. Your changes remain here.".into());}
   }},
  Ok(Completed::Opened(path,document))=>{
   // A format Impress can read but not write (PDF) is converted, as in Word:
   // the result is a new, untitled document, so Save asks where to keep it.
   let converted=s.formats.for_path(&path).is_some_and(|f|!f.capabilities().export);
   let notes=crate::conversion_notes(&document);
   if let Err(error)=crate::documents::add(s,document,(!converted).then(||path.clone())){s.action_status=Some(format!("Could not show {}: {error}",path.display()));return;}
   s.action_status=Some(if converted{format!("Converted {} into an editable document. It may not look exactly like the original; save it to keep it.{notes}",path.file_name().map(|n|n.to_string_lossy().to_string()).unwrap_or_default())}else{format!("Opened {}",path.display())});
   remember_recent(s,&path);},
  Err(e)=>{s.pending_open=None;s.pending_close=false;s.close_ready=false;s.action_status=Some(format!("File operation failed: {e}; the window remains open"));},
 }},Some(Err(TryRecvError::Disconnected))=>{s.file_job=None;s.pending_open=None;s.pending_close=false;s.close_ready=false;s.action_status=Some("File worker stopped without a completion result; existing files were not marked saved".into());},_=>{}}
}

/// Closing is allowed only after the exact frozen revision was durably saved.
fn close_after_save(saved:u64,current:u64,dirty:bool,draft:bool,active_input:bool)->bool{
 saved==current&&!dirty&&!draft&&!active_input
}
#[cfg(test)] #[path = "../tests/unit/background_io.rs"] mod tests;

fn remember_recent(state:&mut CanvasState,path:&std::path::Path) {
 if let Err(error)=crate::recent_files::remember(path) {
  state.action_status=Some(format!("{}; could not remember recent file: {error}",state.action_status.as_deref().unwrap_or("File operation completed")));
 }
}
