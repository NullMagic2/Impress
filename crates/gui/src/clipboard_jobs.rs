//! One staged clipboard worker per window. Native capture/publication retains
//! HWND ownership on the UI thread. No worker mutates a live EditorState.
use super::*;
use std::sync::{mpsc::{self,Receiver,TryRecvError},atomic::{AtomicBool,Ordering}};
use objects_ui::SelectedObject;
#[derive(Clone)]
struct Target{revision:u64,selection:Option<Selection>,object:Option<SelectedObject>,typing:model::CharacterProperties,style:Option<model::StyleId>}
impl Target{
    fn from_editor(e:&EditorState,object:Option<SelectedObject>)->Self{
        Self{revision:e.revision,selection:e.selection.clone(),object,typing:e.typing_properties.clone(),style:e.typing_style.clone()}
    }
    fn capture(s:&CanvasState)->Self{Self::from_editor(&s.editor,s.selected_object.clone())}
    fn matches_editor(&self,e:&EditorState,object:Option<&SelectedObject>,blocked:bool)->bool{
        !blocked && self.revision==e.revision && self.selection==e.selection
            && self.object.as_ref()==object && self.typing==e.typing_properties && self.style==e.typing_style
    }
    fn matches(&self,s:&CanvasState)->bool{
        self.matches_editor(&s.editor,s.selected_object.as_ref(),s.object_drag.is_some()||crate::equation_ui::previewing(s)||s.input.active())
    }
}

enum Completed{Copy(system_clipboard::Prepared,Option<EditorState>),Paste(EditorState,Option<String>),Empty}
struct FollowingPaste {target:Target,command:AppCommand}
pub struct Job{target:Target,sequence:u32,command:AppCommand,cancel:Arc<AtomicBool>,receiver:Receiver<Result<Completed,String>>,following:Option<FollowingPaste>}
fn check(cancel:&AtomicBool)->Result<(),String>{if cancel.load(Ordering::Acquire){Err("Clipboard operation cancelled".into())}else{Ok(())}}
pub fn cancel(s:&mut CanvasState){if let Some(job)=&s.clipboard_job{job.cancel.store(true,Ordering::Release);s.action_status=Some("Clipboard operation cancelled; the document is unchanged.".into());}}
pub fn queue(s:&mut CanvasState,command:AppCommand)->Result<(),String>{
    if s.clipboard_job.is_some(){
        let target=Target::capture(s);
        let job=s.clipboard_job.as_mut().expect("pending clipboard job");
        return job.follow_with_paste(command,target);
    }
    if s.file_job.is_some()||s.pending_close||s.object_drag.is_some()||crate::equation_ui::previewing(s)||s.input.active(){return Err("Finish the file operation, drag or composition before using the clipboard.".into());}
    let captured=if matches!(command,AppCommand::Paste|AppCommand::PastePlain){Some(system_clipboard::capture(command==AppCommand::PastePlain)?)}else{None};
    let sequence=system_clipboard::sequence();let target=Target::capture(s);let object=target.object.clone();
    let mut staged=s.editor.clone();let cancel=Arc::new(AtomicBool::new(false));let signal=Arc::clone(&cancel);let(tx,receiver)=mpsc::channel();
    std::thread::Builder::new().name("impress-clipboard".into()).spawn(move||{
        let result=(||{
            check(&signal)?;
            if let Some(captured)=captured{
                let content=system_clipboard::decode_captured(captured)?;check(&signal)?;let mut notice=None;
                match content{
                    system_clipboard::Content::Rich(f)=>{
                        if !f.warnings.is_empty(){notice=Some(format!("Pasted with format limitations: {}",f.warnings.iter().take(4).cloned().collect::<Vec<_>>().join("; ")));}
                        if f.table.is_some(){if let Some(SelectedObject::Cells{table_id,cell_ids})=&object{staged.paste_table_cells(&f,table_id,cell_ids)?;}else{staged.paste_fragment(&f)?;}}
                        else{if matches!(&object,Some(SelectedObject::Cells{..})){return Err("Paste a matching table rectangle, or click inside one cell first".into());}staged.paste_fragment(&f)?;}
                    },
                    system_clipboard::Content::Text(t)=>{if matches!(&object,Some(SelectedObject::Cells{..})){return Err("Click inside a cell before pasting plain text".into());}staged.insert_text(&t)?;},
                    system_clipboard::Content::Empty=>return Ok(Completed::Empty),
                }
                check(&signal)?;Ok(Completed::Paste(staged,notice))
            }else{
                let fragment=match &object{
                    Some(SelectedObject::Cells{table_id,cell_ids})=>Some(staged.copy_table_cells(table_id,cell_ids)?),
                    Some(SelectedObject::Picture(id))=>Some(staged.copy_picture(id)?),_=>staged.copy_fragment(),
                };
                let Some(fragment)=fragment else{return Ok(Completed::Empty);};
                if command==AppCommand::Cut{match &object{
                    Some(SelectedObject::Cells{table_id,cell_ids})=>staged.clear_cells(table_id,cell_ids)?,
                    Some(SelectedObject::Picture(id))=>staged.delete_picture(id)?,_=>{staged.delete_selection()?;},
                }}
                check(&signal)?;let prepared=system_clipboard::prepare(&fragment)?;check(&signal)?;
                Ok(Completed::Copy(prepared,(command==AppCommand::Cut).then_some(staged)))
            }
        })();let _=tx.send(result);
    }).map_err(|e|e.to_string())?;
    s.clipboard_job=Some(Job{target,sequence,command,cancel,receiver,following:None});
    s.action_status=Some("Preparing clipboard operation… Escape cancels; editing elsewhere discards a pending Cut/Paste.".into());Ok(())
}
pub fn poll(s:&mut CanvasState){
    let Some(job)=s.clipboard_job.as_ref()else{return;};
    let result=match job.receiver.try_recv(){Ok(v)=>v,Err(TryRecvError::Empty)=>return,Err(TryRecvError::Disconnected)=>Err("Clipboard worker stopped before completion".into())};
    let job=s.clipboard_job.take().expect("completed job");
    if job.cancel.load(Ordering::Acquire){return;}
    if job.command!=AppCommand::Copy&&(!job.target.matches(s)||s.file_job.is_some()){
        s.action_status=Some("Pending Cut/Paste discarded because its editing target changed. No text was overwritten or deleted.".into());return;
    }
    // Validate the destination before applying Cut's staged deletion. A Paste
    // requested at the unchanged Cut selection follows its resulting caret.
    let following=job.following.filter(|request|request.target.matches(s));
    let old_revision=s.editor.revision;
    let mut published=false;
    let applied=(||{match result?{
        Completed::Copy(prepared,editor)=>{
            system_clipboard::publish(&prepared,job.sequence)?;
            published=true;
            // Source deletion follows complete native clipboard publication.
            if let Some(editor)=editor{s.editor=editor;s.selected_object=None;}
            s.action_status=Some(if job.command==AppCommand::Cut{"Cut"}else{"Copied"}.into());
        },
        Completed::Paste(editor,notice)=>{s.editor=editor;s.selected_object=None;s.action_status=notice.or_else(||Some("Pasted".into()));},
        Completed::Empty=>s.action_status=Some("Nothing to copy or paste".into()),
    }Ok::<(),String>(())})();
    if let Err(error)=applied{s.action_status=Some(error);}
    if s.editor.revision!=old_revision{s.preferred_caret_x_pt=None;s.caret_visual_line=None;submit_layout(s);refresh_overlay(s);wake_caret(s);}
    if published {
        if let Some(request)=following {
            if let Err(error)=queue(s,request.command) {s.action_status=Some(error);}
        }
    }
}

impl Job {
    fn follow_with_paste(&mut self,command:AppCommand,target:Target)->Result<(),String> {
        if matches!(self.command,AppCommand::Copy|AppCommand::Cut)
            && matches!(command,AppCommand::Paste|AppCommand::PastePlain)
            && self.following.is_none() && !self.cancel.load(Ordering::Acquire) {
            self.following=Some(FollowingPaste{target,command});
            Ok(())
        } else {Err("A clipboard operation is still finishing. Press Escape to cancel it.".into())}
    }
}

#[cfg(test)] #[path = "../tests/unit/clipboard_jobs/target_tests.rs"] mod target_tests;
