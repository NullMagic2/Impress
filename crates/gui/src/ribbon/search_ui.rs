//! Background search with revision/query/options validation. At most one search
//! worker is live; a newer request replaces the pending request, not the editor.
use super::*;
use editor::search::{self,SearchOptions,SearchReport};
pub(super)struct ResultSet{revision:u64,query:String,options:SearchOptions,result:Result<(Vec<navigation::Entry>,Option<Arc<SearchReport>>),String>}
impl Ribbon{
 pub(super)fn start_search(&self){
  if self.search_job.borrow().is_some()||Instant::now()<self.search_due.get(){return;}
  let query=self.ui.navigation_search.get_value();let options=self.search_options.get();let(revision,document)={let mut s=self.state.borrow_mut();(s.editor.revision,document_snapshot(&mut s))};
  let(tx,rx)=mpsc::channel();let spawn=std::thread::Builder::new().name("impress-search".into()).spawn(move||{
   let result=if query.is_empty(){Ok((navigation::entries(&document,"",100_000).0,None))}else{search::find(&document,revision,&query,options).map(|report|{
    let entries=report.matches.iter().map(|m|navigation::Entry{label:format!("{} · {}",m.story.label(),m.preview),node:m.node.clone(),start:m.range.start,end:m.range.end}).collect();(entries,Some(Arc::new(report)))})};
   let _=tx.send(ResultSet{revision,query,options,result});
  });match spawn{Ok(_)=>{*self.search_job.borrow_mut()=Some(rx);self.ui.navigation_hint.set_label(&crate::i18n::text(&("Searching…")));},Err(e)=>{self.navigation_revision.set(Some(revision));self.state.borrow_mut().action_status=Some(format!("Could not start search: {e}"));}}
 }
 pub(super)fn poll_search(&self){
  let result=self.search_job.borrow().as_ref().map(|rx|rx.try_recv());
  if let Some(Ok(found))=result{
   self.search_job.borrow_mut().take();let current=self.state.borrow().editor.revision;
   if found.revision==current && found.query==self.ui.navigation_search.get_value() && found.options==self.search_options.get(){
    match found.result{Ok((entries,report))=>{
     self.ui.navigation_list.freeze();self.ui.navigation_list.clear();for entry in entries.iter().take(500){self.ui.navigation_list.append(&entry.label);}self.ui.navigation_list.thaw();
     let suffix=if report.as_ref().is_some_and(|r|r.truncated){" · incomplete; narrow scope before Replace All"}else if entries.len()>500{" · first 500 listed; Next reaches later matches"}else{""};
     self.ui.navigation_hint.set_label(&crate::i18n::text(&(&format!("{} {}{}",entries.len(),if found.query.is_empty(){"headings"}else{"matches"},suffix))));
     self.navigation.replace(entries);self.search_report.replace(report);self.search_cursor.set(None);
    },Err(e)=>{self.navigation.borrow_mut().clear();self.search_report.borrow_mut().take();self.search_cursor.set(None);self.ui.navigation_list.clear();self.ui.navigation_hint.set_label(&crate::i18n::text(&(&e)));}}
    self.navigation_revision.set(Some(current));self.state.borrow_mut().search_marks=None;
   }else{self.navigation_revision.set(None);}
  }else if matches!(result,Some(Err(TryRecvError::Disconnected))){self.search_job.borrow_mut().take();self.navigation_revision.set(Some(self.state.borrow().editor.revision));self.ui.navigation_hint.set_label(&crate::i18n::text(&("Search worker stopped unexpectedly")));}
  if !self.search_visible(){return;}
  let mut s=self.state.borrow_mut();if s.display_generation!=Some(s.layout_generation){return;}
  let report=self.search_report.borrow();let Some(report)=report.as_ref().filter(|r|r.revision==s.editor.revision&&r.query==self.ui.navigation_search.get_value()&&r.options==self.search_options.get())else{return;};
  if s.search_marks.as_ref().is_some_and(|(rev,g,_)|*rev==report.revision&&*g==s.layout_generation){return;}
  let mut ranges=std::collections::BTreeMap::new();for m in &report.matches{ranges.entry(m.node.0.clone()).or_insert_with(Vec::new).push(m.range.clone());}
  for rs in ranges.values_mut(){rs.sort_by_key(|r|r.start);}
  let marks=render::search_marks(&s.vector_document,&ranges,10_000);s.search_marks=Some((report.revision,s.layout_generation,marks));refresh_overlay(&mut s);
 }
 pub(super)fn search_step(&self,forward:bool){
  let revision=self.state.borrow().editor.revision;
  if self.navigation_revision.get()!=Some(revision){self.populate_navigation();return;}
  let selection=self.state.borrow().editor.selection.clone();
  let entries=self.navigation.borrow();if entries.is_empty(){return;}
  // IDs are not ordered document positions. Build an order in the same story
  // traversal used by the search engine, including paragraphs with no matches.
  let order={let state=self.state.borrow();search::scoped_paragraphs(&state.editor.document,self.search_options.get().scope)
   .into_iter().enumerate().map(|(i,(p,_))|(p.id.clone(),i)).collect::<std::collections::BTreeMap<_,_>>()};
  let i=step_index(&entries,self.search_cursor.get(),selection.as_ref(),forward,&order).unwrap_or(0);
  let entry=entries[i].clone();let total=entries.len();drop(entries);self.search_cursor.set(Some(i));self.navigate_entry(entry);
  if i<500 {self.ui.navigation_list.set_selection(i as u32,true);self.ui.navigation_list.ensure_visible(i as i32);}
  self.ui.navigation_hint.set_label(&crate::i18n::text(&(&format!("Match/heading {} of {}",i+1,total))));
 }
 pub(super)fn search_options_dialog(&self){self.open_find_dialog(false);}
 pub(super)fn replace_found(&self,all:bool)->Result<(),String>{
  let report=self.search_report.borrow().clone().ok_or("Find text first")?;
  if report.query!=self.ui.navigation_search.get_value()||report.options!=self.search_options.get(){return Err("Search options changed; search again before replacing".into());}
  if report.revision!=self.state.borrow().editor.revision{return Err("The document changed; search again before replacing".into());}
  let replacement=self.ui.navigation_replace.get_value();
  if all {
   if report.truncated{return Err("Search reached its safety limit; narrow the scope before Replace All. Nothing was replaced.".into());}
   let choices=["Cancel".into(),format!("Replace {} matches",report.matches.len())];
   if self.choose_review("Replace All","Every replacement is staged as one undoable transaction. Text outside each match retains its formatting.",&choices)!=Some(1){
    self.state.borrow_mut().action_status=Some("Replace All cancelled. No text changed.".into());return Ok(());
   }
  }
  // Confirmation dispatches a modal loop, so recheck before committing.
  if report.query!=self.ui.navigation_search.get_value()||report.options!=self.search_options.get()
   ||replacement!=self.ui.navigation_replace.get_value(){return Err("Search or replacement changed; run the command again".into());}
  {let state=self.state.borrow();if crate::equation_ui::previewing(&state)||state.input.active()||state.object_drag.is_some()||state.dragging_selection{
    return Err("Finish composition, object dragging, or the equation draft before replacing".into());}}
  let index=if all{0}else{
   let state=self.state.borrow();report.matches.iter().position(|m|find_dialog::selection_matches(state.editor.selection.as_ref(),&m.node,m.range.start,m.range.end))
    .ok_or("Select a current match with Find Next before replacing")?
  };
  let mut count=0;
  self.edit(|e|{count=if all{e.replace_all_search(&report,&replacement)?}else{e.replace_search_match(&report,index,&replacement)?};Ok(())})?;
  self.state.borrow_mut().action_status=Some(format!("Replaced {count} match(es)"));
  self.navigation_revision.set(None);self.search_cursor.set(None);self.search_due.set(Instant::now());self.populate_navigation();Ok(())
 }
}
/// Start at the current caret, not always at the first result. A current selected
/// result advances; a manual selection/caret move invalidates the remembered index.
fn step_index(entries:&[navigation::Entry],cursor:Option<usize>,selection:Option<&Selection>,forward:bool,
 order:&std::collections::BTreeMap<NodeId,usize>)->Option<usize>{
 if entries.is_empty(){return None;}
 if let Some(i)=cursor.filter(|&i|entries.get(i).is_some_and(|e|find_dialog::selection_matches(selection,&e.node,e.start,e.end))){
  return Some(if forward{(i+1)%entries.len()}else{(i+entries.len()-1)%entries.len()});
 }
 if let Some(selection)=selection {
  let focus=&selection.focus;
  if let Some(&rank)=order.get(&focus.node_id){
   let current=(rank,focus.byte_offset);
   if forward {
    if let Some(i)=entries.iter().position(|e|order.get(&e.node).is_some_and(|&rank|(rank,e.start)>=current)){return Some(i);}
   }else if let Some(i)=entries.iter().rposition(|e|order.get(&e.node).is_some_and(|&rank|(rank,e.end)<=current)){return Some(i);}
  }
 }
 Some(if forward{0}else{entries.len()-1})
}
#[cfg(test)] #[path = "../../tests/unit/ribbon/search_ui.rs"] mod tests;
