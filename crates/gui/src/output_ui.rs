//! Output jobs from a snapshot of the displayed pages: PDF export on a worker
//! thread, and printing through the platform's print system (wxWidgets
//! printing: GDI on Windows, GTK print on Linux, Cocoa on macOS). Completing an
//! export never marks the editable document as saved.
use super::*;
use std::sync::{mpsc::{self,Receiver,TryRecvError},atomic::{AtomicBool,AtomicUsize,Ordering}};
#[cfg(not(windows))] use wxdragon::printing::{PrintDialogData, Printer};
use wxdragon::printing::Printout;
use wxdragon::DeviceContext;

#[derive(Clone)]
pub(crate) struct ProgressState{
 pub cancel:Arc<AtomicBool>,
 pub completed:Arc<AtomicUsize>,
 pub total:Option<usize>,
 pub printing:bool,
}
pub struct Job{progress:ProgressState,receiver:Receiver<Result<String,String>>,open_after:Option<PathBuf>}
impl Drop for Job{fn drop(&mut self){self.progress.cancel.store(true,Ordering::Release);}}

/// A thread-safe snapshot for the GUI's modeless progress window.
pub(crate) fn progress(s:&CanvasState)->Option<ProgressState>{s.output_job.as_ref().map(|job|job.progress.clone())}

/// The pages as displayed now, if output can start.
pub fn snapshot(s:&mut CanvasState)->Result<(Arc<VectorDocument>,Arc<Document>),String>{
 if s.object_drag.is_some()||s.table_drag.is_some(){return Err("Finish or cancel dragging before output".into());}
 if s.display_generation!=Some(s.layout_generation)||s.layout_error.is_some()||s.vector_document.pages.is_empty(){return Err("Output needs a finished layout of the current document. Try again in a moment.".into());}
 if s.output_job.is_some(){return Err("Output is already running. Cancel it in the progress window.".into());}
 Ok((s.vector_document.clone(),document_snapshot(s)))
}

/// Everything the Export PDF dialog decided.
pub struct PdfRequest{pub path:PathBuf,pub pages:Vec<usize>,pub info:output::pdf::PdfInfo,pub links:bool,pub open:bool}

/// Writes the selected pages of the current snapshot in the background.
pub fn start_pdf_export(state:&Rc<RefCell<CanvasState>>,revision:u64,request:PdfRequest)->Result<(),String>{
 let mut s=state.borrow_mut();
 if s.editor.revision!=revision{return Err("The document changed while the Export PDF dialog was open. Export again.".into());}
 let(scene,doc)=snapshot(&mut s)?;
 if request.pages.iter().any(|&p|p>=scene.pages.len()){return Err("The selected pages are no longer in the document.".into());}
 let cancel=Arc::new(AtomicBool::new(false));let flag=cancel.clone();let(tx,receiver)=mpsc::channel();
 let PdfRequest{path,pages,info,links,open}=request;
 let open_after=open.then(||path.clone());
 let total=pages.len();
 let completed=Arc::new(AtomicUsize::new(0));let worker_progress=Arc::clone(&completed);
 std::thread::Builder::new().name("impress-pdf-output".into()).spawn(move||{
  let links=if links{output::links::collect(&doc,&scene)}else{Vec::new()};
  let result=format::atomic_write(&path,|file|{output::pdf::write_pdf_with_progress(&scene,&pages,&info,&links,&flag,file,|count|worker_progress.store(count,Ordering::Release))?;if flag.load(Ordering::Acquire){return Err("Export cancelled before the file was replaced".into());}Ok(())})
   .map(|()|format!("Exported {} page{} to {}",pages.len(),if pages.len()==1{""}else{"s"},path.display()));
  let _=tx.send(result);
 }).map_err(|e|e.to_string())?;
 s.output_job=Some(Job{progress:ProgressState{cancel,completed,total:Some(total),printing:false},receiver,open_after});s.action_status=Some("Exporting PDF… You can keep editing.".into());Ok(())
}

/// Prints `pages` (zero-based), `copies` times. The platform's print dialog
/// chooses the printer and paper next. Windows spools GDI bands on a worker
/// thread; elsewhere the platform print system (GTK, Cocoa) renders the job
/// with its own progress. Returns false when cancelled.
pub fn print_pages(parent:&impl WxWidget,state:&Rc<RefCell<CanvasState>>,revision:u64,pages:Vec<usize>,copies:usize,collate:bool,fit:bool)->Result<bool,String>{
 let scene={let mut s=state.borrow_mut();if s.editor.revision!=revision{return Err("The document changed while the Print dialog was open. Print again.".into());}snapshot(&mut s)?.0};
 if pages.is_empty()||pages.iter().any(|&p|p>=scene.pages.len()){return Err("Choose pages that exist in the document.".into());}
 let copies=copies.clamp(1,99);
 let sequence:Vec<usize>=if collate{(0..copies).flat_map(|_|pages.iter().copied()).collect()}else{pages.iter().flat_map(|&p|std::iter::repeat_n(p,copies)).collect()};
 #[cfg(windows)]{
  let Some(mut device)=crate::native_print::choose(parent.get_handle(),scene.pages.len())?else{return Ok(false);};
  device.pages=sequence;
  let mut s=state.borrow_mut();if s.output_job.is_some(){return Err("Another output job started while the printer dialog was open".into());}
  let cancel=Arc::new(AtomicBool::new(false));let flag=cancel.clone();let(tx,receiver)=mpsc::channel();
  std::thread::Builder::new().name("impress-print-spool".into()).spawn(move||{let result=device.spool(&scene,fit,&flag).map(|()|"Print job sent to the printer".into());let _=tx.send(result);}).map_err(|e|e.to_string())?;
  s.output_job=Some(Job{progress:ProgressState{cancel,completed:Arc::new(AtomicUsize::new(0)),total:None,printing:true},receiver,open_after:None});s.action_status=Some("Printing… You can keep editing.".into());
  return Ok(true);
 }
 #[cfg(not(windows))]{
  let title=state.borrow().current_path.as_ref().and_then(|p|p.file_stem()).map(|s|s.to_string_lossy().to_string()).unwrap_or_else(||"Impress document".into());
  let printout=PagesPrintout{document:scene,sequence,fit,renderer:output::raster::BandRenderer::default(),first:1};
  let mut printer=Printer::new(Some(&PrintDialogData::new()));
  Ok(printer.print(Some(parent),&title,printout,true))
 }
}

/// Draws each page as high-resolution raster bands from the final page
/// geometry (not a screen capture), scaled to the printer's resolution.
#[cfg_attr(windows,allow(dead_code))]
struct PagesPrintout{document:Arc<VectorDocument>,sequence:Vec<usize>,fit:bool,renderer:output::raster::BandRenderer,
 /// Number of the first page as the print system counts: wxGTK counts from
 /// the dialog's minimum page, which starts at 0; other ports from 1.
 first:i32}
impl Printout for PagesPrintout{
 fn get_page_info(&mut self)->(i32,i32,i32,i32){let n=self.sequence.len() as i32;(1,n,1,n)}
 fn on_begin_document(&mut self,_dc:&wxdragon::dc::GenericDC,start:i32,_end:i32)->bool{self.first=start.min(1).max(0);true}
 fn has_page(&mut self,page:i32)->bool{page>=self.first&&((page-self.first) as usize)<self.sequence.len()}
 fn on_print_page(&mut self,dc:&wxdragon::dc::GenericDC,number:i32)->bool{
  let Some(page)=usize::try_from(number-self.first).ok().and_then(|n|self.sequence.get(n)).and_then(|&i|self.document.pages.get(i)) else{return false;};
  let (w,h)=dc.get_size();let (px,py)=dc.get_ppi();
  if w<=0||h<=0{return false;}
  // Render at no more than 300 dpi and let the device scale up.
  let reduce=((px.max(py) as f32)/300.0).max(1.0);
  let (rw,rh)=(((w as f32)/reduce).round() as i32,((h as f32)/reduce).round() as i32);
  let (sx,sy)=(px as f32/72.0/reduce,py as f32/72.0/reduce);
  let factor=if self.fit{(rw as f32/(page.size.width*sx)).min(rh as f32/(page.size.height*sy))}else{1.0};
  let (tx,ty)=if self.fit{((rw as f32-page.size.width*sx*factor)/2.0,(rh as f32-page.size.height*sy*factor)/2.0)}else{(0.0,0.0)};
  dc.set_user_scale(reduce as f64,reduce as f64);
  let cancel=AtomicBool::new(false);
  let band=256;
  let mut y=0;
  while y<rh{
   let bh=(rh-y).min(band);
   let matrix=render::Affine2D{sx:sx*factor,sy:sy*factor,tx,ty:ty-y as f32,..render::Affine2D::IDENTITY};
   match self.renderer.render(page,rw as u32,bh as u32,matrix,&cancel){
    Ok(mut data)=>{for p in data.chunks_exact_mut(4){p.swap(0,2);}
     if let Some(bitmap)=wxdragon::Bitmap::from_rgba(&data,rw as u32,bh as u32){dc.draw_bitmap(&bitmap,0,y,false);}}
    Err(_)=>return false,
   }
   y+=bh;
  }
  true
 }
}

pub fn poll(s:&mut CanvasState){let result=s.output_job.as_ref().map(|j|j.receiver.try_recv());match result{
 Some(Ok(result))=>{let job=s.output_job.take();
  match result{
   Ok(message)=>{
    if let Some(path)=job.as_ref().and_then(|j|j.open_after.clone()){if !wxdragon::utils::launch_default_application(&path.to_string_lossy()){s.action_status=Some(format!("{message}. No application is set up to open PDF files."));return;}}
    s.action_status=Some(message);
   }
   Err(e)=>{
    let cancelled=job.as_ref().is_some_and(|job|job.progress.cancel.load(Ordering::Acquire))&&e.to_lowercase().contains("cancel");
    s.action_status=Some(if cancelled {if job.as_ref().is_some_and(|job|job.progress.printing){"Printing cancelled".into()}else{"Export cancelled".into()}}else{format!("Export failed: {e}")});
   },
  }},
 Some(Err(TryRecvError::Disconnected))=>{s.output_job=None;s.action_status=Some("The export stopped without finishing".into());},_=>{}}
}
