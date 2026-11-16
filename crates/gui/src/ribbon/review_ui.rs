//! Native reference/review dialogs. Clone the small dialog model, release the
//! RefCell borrow, then show a modal dialog (wx timers continue during dialogs).
use super::*;
use model::{AnchorRange,TextAnchor,inline_text};
use wxdragon::dialogs::text_entry_dialog::{TextEntryDialog,TextEntryDialogStyle};

pub(super) struct SpellJob { pub generation:u64, pub revision:u64, pub selection_only:bool, pub dictionary:crate::spelling::Dictionary, pub receiver:Receiver<Result<crate::spelling::Report,String>> }
impl Ribbon {
    pub(super) fn choose_review(&self,title:&str,message:&str,items:&[String])->Option<usize>{
        if items.is_empty(){MessageDialog::builder(&self.ui.main_frame,&crate::i18n::text(&("No entries yet.")),&crate::i18n::text(&(title))).build().show_modal();return None;}
        let refs=items.iter().map(String::as_str).collect::<Vec<_>>();
        let d=SingleChoiceDialog::builder(&self.ui.main_frame,&crate::i18n::text(&(message)),&crate::i18n::text(&(title)),&refs).build();
        d.set_selection(0);if ({crate::placement::centre(&d);d.show_modal()})!=ID_OK{return None;}usize::try_from(d.get_selection()).ok().filter(|&i|i<items.len())
    }
    pub(super) fn text_review(&self,title:&str,message:&str,initial:&str,multiline:bool)->Option<String>{
        // wxTE_MULTILINE is 0x0020; wxDragon's text-entry style wrapper does not
        // name the generic text-control flag, but forwards retained bits to wx.
        let style=if multiline{TextEntryDialogStyle::from_bits_retain(TextEntryDialogStyle::Default.bits()|0x0020)}else{TextEntryDialogStyle::Default};
        let d=TextEntryDialog::builder(&self.ui.main_frame,&crate::i18n::text(&(message)),&crate::i18n::text(&(title))).with_default_value(initial).with_style(style).build();
        if ({crate::placement::centre(&d);d.show_modal()})!=ID_OK{return None;}d.get_value()
    }
    pub(super) fn review_info(&self,title:&str,text:&str){MessageDialog::builder(&self.ui.main_frame,&crate::i18n::text(&(text)),&crate::i18n::text(&(title))).build().show_modal();}
    pub(super) fn bookmarks(&self)->Result<(),String>{
        let entries=self.state.borrow().editor.document.review.bookmarks.values().cloned().collect::<Vec<_>>();
        let mut items=vec!["Add bookmark at selection…".into()];items.extend(entries.iter().map(|b|format!("{}{}",b.name,if b.range.is_none(){" [unanchored]"}else{""})));
        let Some(index)=self.choose_review("Bookmarks","Choose a bookmark, or add one.",&items)else{return Ok(());};
        if index==0{if let Some(name)=self.text_review("Add bookmark","Name: 1–40 ASCII letters, digits or underscores.","",false){self.edit(|e|e.add_bookmark(&name))?;}return Ok(());}
        let b=&entries[index-1];let actions=["Go to selection".into(),"Delete bookmark".into(),"Insert page reference at current caret".into(),"Attach bookmark to current selection".into()];
        match self.choose_review("Bookmark",&b.name,&actions){Some(0)=>{let range=b.range.as_ref().ok_or("This bookmark no longer has a text anchor")?;self.edit(|e|e.go_to_range(range))?;},Some(1)=>self.edit(|e|e.delete_bookmark(&b.id))?,Some(2)=>self.edit(|e|e.insert_page_reference(&b.id))?,Some(3)=>self.edit(|e|e.reanchor_bookmark(&b.id))?,_=>{}}Ok(())
    }
    pub(super) fn text_tracking(&self)->Result<(),String>{
        let modes=["Word revisions…".into(),"Chronological native history (all edit kinds)…".into(),"DOCX-compatible net paragraph-text changes…".into()];
        match self.choose_review("Review changes","Choose a review mode.",&modes){Some(0)=>return self.word_revisions(),Some(1)=>return self.native_history(),Some(2)=>{},_=>return Ok(())}

        let (review,current)={let state=self.state.borrow();(state.editor.document.review.clone(),model::document_paragraphs(&state.editor.document).iter().map(|p|(p.id.clone(),inline_text(&p.content))).collect::<std::collections::BTreeMap<_,_>>())};
        let revisions=review.revisions.values().collect::<Vec<_>>();
        let mut items=vec![if review.track_text{"Turn OFF paragraph-text tracking".into()}else{"Turn ON paragraph-text tracking…".into()}];
        items.extend(revisions.iter().map(|r|format!("{}: {}",r.author,current.get(&r.paragraph).map(|s|preview(s)).unwrap_or_else(||"[paragraph removed]".into()))));
        let Some(index)=self.choose_review("Text changes","Tracks original/final text per paragraph, not formatting, moves or structural changes. Unsupported structural edits are blocked while tracking is on.",&items)else{return Ok(());};
        if index==0{let author=if review.track_text{review.author.clone()}else{let Some(author)=self.text_review("Track text changes","Author name:",&review.author,false)else{return Ok(());};author};self.edit(|e|e.set_text_tracking(!review.track_text,&author))?;return Ok(());}
        let r=revisions[index-1];let detail=format!("Author: {}\nDate: {}\n\nOriginal:\n{}\n\nCurrent:\n{}",r.author,r.date.as_deref().unwrap_or("Not recorded"),inline_text(&r.before.content),current.get(&r.paragraph).map(String::as_str).unwrap_or("[paragraph removed]"));
        let actions=["Go to paragraph".into(),"Accept current text / discard original".into(),"Reject and restore original text".into(),"Review individual text changes…".into()];
        match self.choose_review("Text revision",&detail,&actions){Some(0)=>self.edit(|e|e.go_to_range(&AnchorRange{start:TextAnchor{paragraph:r.paragraph.clone(),offset:0},end:TextAnchor{paragraph:r.paragraph.clone(),offset:0}}))?,Some(1)=>self.edit(|e|e.resolve_revision(&r.id,true))?,Some(2)=>self.edit(|e|e.resolve_revision(&r.id,false))?,Some(3)=>self.review_text_changes(&r.id)?,_=>{}}Ok(())
    }
    fn word_revisions(&self)->Result<(),String>{
        let mut changes=self.state.borrow().editor.document.review.tracked_changes.values().filter(|change|change.status==model::RevisionStatus::Pending).cloned().collect::<Vec<_>>();
        changes.sort_by_key(|change|change.order);
        let items=changes.iter().map(|change|format!("{} · {} · {}",crate::i18n::text(change.label()),change.author,change.date.as_deref().unwrap_or(""))).collect::<Vec<_>>();
        let Some(index)=self.choose_review("Word revisions","Choose a change.",&items)else{return Ok(());};
        let change=&changes[index];let actions=["Go to selection".into(),"Accept this change".into(),"Reject this change".into()];
        match self.choose_review("Word revisions",&format!("{} · {}",crate::i18n::text(change.label()),change.author),&actions){
            Some(0)=>if let Some(range)=change.effects.iter().find_map(|effect|match effect{model::RevisionEffect::Inline{range,..}=>Some(range.clone()),model::RevisionEffect::ParagraphProperties{paragraph,..}=>{let anchor=TextAnchor{paragraph:paragraph.clone(),offset:0};Some(AnchorRange{start:anchor.clone(),end:anchor})},_=>None}){self.edit(|editor|editor.go_to_range(&range))?;},
            Some(1)=>self.edit(|editor|editor.resolve_tracked_change(&change.id,true))?,
            Some(2)=>self.edit(|editor|editor.resolve_tracked_change(&change.id,false))?,_=>{}
        }Ok(())
    }
    fn native_history(&self)->Result<(),String>{
        let review=self.state.borrow().editor.document.review.clone();
        let mut items=vec![if review.track_history{"Turn OFF native tracking".into()}else{"Turn ON native tracking…".into()},"Change current author…".into()];
        items.extend(review.history.iter().rev().map(|e|format!("{:?} · {} · {} · {}",e.status,e.date,e.author,e.label)));
        let Some(index)=self.choose_review("Chronological native history","Newest first. Accept retains the event. Reject is conflict-checked and undoable; overlapping dependent edits must be resolved first. Keep a native .impress file to preserve this journal.",&items)else{return Ok(());};
        if index<2{let enabled=if index==0{!review.track_history}else{review.track_history};let author=if enabled||index==1{let Some(author)=self.text_review("Revision author","Author name:",&review.author,false)else{return Ok(());};author}else{review.author};return self.edit(|e|e.set_revision_history(enabled,&author));}
        let event=&review.history[review.history.len()-(index-2)-1];let choices=["Accept this event".into(),"Reject this event (conflict checked)".into()];
        if let Some(action)=self.choose_review("Resolve native revision",&format!("{}\n{} · {}\nStatus: {:?}",event.label,event.author,event.date,event.status),&choices){self.edit(|e|e.resolve_history_event(&event.id,action==0))?;}Ok(())
    }
    fn review_text_changes(&self,id:&NodeId)->Result<(),String>{
        loop {
            let (revision,changes)={let s=self.state.borrow();if !s.editor.document.review.revisions.contains_key(id){return Ok(());}(s.editor.revision,s.editor.revision_changes(id)?)};
            if changes.is_empty(){self.review_info("Text changes","No text differences remain. Formatting history is not reconstructed.");return Ok(());}
            let items=changes.iter().map(|c|format!("{} → {}",if c.removed.is_empty(){"[insertion]".into()}else{preview(&c.removed)},if c.inserted.is_empty(){"[deletion]".into()}else{preview(&c.inserted)})).collect::<Vec<_>>();
            let Some(index)=self.choose_review("Individual text changes","Choose a difference. Other changes in this paragraph remain pending.",&items)else{return Ok(());};
            let c=&changes[index];let actions=["Accept this change".into(),"Reject this change".into(),"Back to changes".into()];
            match self.choose_review("Review change",&format!("Removed:\n{}\n\nInserted:\n{}",c.removed,c.inserted),&actions){
                Some(choice @ 0..=1)=>self.edit(|e|e.resolve_revision_change(id,c.index,choice==0,revision))?,_=>{}
            }
        }
    }
    pub(super) fn contents(&self)->Result<(),String>{
        let options=self.state.borrow().editor.document.review.contents_options.clone();let automatic=self.state.borrow().editor.document.review.automatic_contents;let choices=["Update contents using saved settings".into(),"Configure title and heading depth…".into(),if automatic{"Turn OFF automatic synchronization".into()}else{"Turn ON automatic synchronization".into()}];
        let Some(choice)=self.choose_review("Contents","Contents can follow heading edits automatically; updates share the originating undo transaction. Right-aligned dotted leaders are generated at layout time.",&choices)else{return Ok(());};
        if choice==0{return self.edit(|e|e.update_contents(&std::collections::BTreeMap::new()));}if choice==2{return self.edit(|e|e.set_automatic_contents(!automatic));}
        let Some(title)=self.text_review("Contents title","Title:",&options.title,false)else{return Ok(());};let levels=(1..=9).map(|n|format!("Include heading levels 1 through {n}")).collect::<Vec<_>>();let Some(level)=self.choose_review("Contents depth","Heading levels:",&levels)else{return Ok(());};
        self.edit(|e|e.update_contents_options(model::ContentsOptions{title,max_heading_level:(level+1)as u8}))
    }
    pub(super) fn compatibility(&self){
        let native_issues={
            let state=self.state.borrow();
            state.current_path.as_deref().and_then(|path|state.formats.for_path(path))
                .filter(|format|format.id()=="doc").map(|format|{
                    let mut issues=format.export_warnings(&state.editor.document);
                    issues.extend(state.editor.document.compatibility.iter().map(|issue|issue.message.clone()));
                    issues.sort();issues.dedup();issues.into_iter().map(|issue|crate::i18n::text(&issue)).collect::<Vec<_>>()
                })
        };
        if let Some(issues)=native_issues {
            let text=if issues.is_empty(){"No known DOC compatibility issues were detected.".into()}else{format!("Known DOC limitations (keep an Impress or DOCX copy):\n\n{}",issues.join("\n\n"))};
            self.review_info("DOC compatibility",&text);return;
        }
        let issues=docx::export_warnings(&self.state.borrow().editor.document);
        let text=if issues.is_empty(){"No known DOCX compatibility issues were detected. This is not a guarantee of full Word fidelity; keep a native .impress copy.".into()}else{format!("Known DOCX limitations (keep a native .impress copy):\n\n{}",issues.join("\n\n"))};
        self.review_info("DOCX compatibility",&text);
    }
    pub(super) fn start_spelling(&self)->Result<(),String>{
        if self.spell_job.borrow().is_some() { return Err("A spelling check is already running".into()); }
        let selected = self.state.borrow().editor.selection.as_ref().is_some_and(|selection| !selection.is_collapsed());
        self.start_spelling_scope(selected)
    }
    pub(super) fn start_spelling_scope(&self, selection_only: bool) -> Result<(), String> {
        if self.spell_job.borrow().is_some() { return Err("A spelling check is already running".into()); }
        let dictionary=crate::spelling::Dictionary::load()?;
        let (generation, revision, input) = {
            let state = self.state.borrow();
            (state.layout_generation, state.editor.revision, crate::proofing::snapshot(&state.editor, &state.inline_spelling.language, selection_only)?)
        };
        let (tx, receiver) = mpsc::channel();
        let worker_dictionary = dictionary.clone();
        std::thread::Builder::new().name("impress-spelling".into()).spawn(move || {
            let _ = tx.send(crate::proofing::check(input, &worker_dictionary));
        }).map_err(|error| error.to_string())?;
        self.spell_job.replace(Some(SpellJob { generation, revision, selection_only, dictionary, receiver }));
        self.state.borrow_mut().action_status = Some(if selection_only { "Checking spelling in the selected text…" } else { "Checking spelling throughout the document…" }.into());
        Ok(())
    }
    pub(super) fn poll_spelling(&self) {
        let completed = {
            let slot = self.spell_job.borrow();
            slot.as_ref().and_then(|job| match job.receiver.try_recv() {
                Ok(result) => Some(result),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => Some(Err("Spelling check interrupted.".into())),
            })
        };
        let Some(result) = completed else { return; };
        let Some(job) = self.spell_job.borrow_mut().take() else { return; };
        let stale = {
            let state = self.state.borrow();
            state.layout_generation != job.generation || state.editor.revision != job.revision
        };
        if stale {
            self.state.borrow_mut().action_status = Some("The document changed. Check spelling again.".into());
            return;
        }
        match result {
            Ok(report) => spelling_dialog::show(self, job, report),
            Err(error) => self.state.borrow_mut().action_status = Some(error),
        }
    }


}
fn preview(text:&str)->String{text.replace(['\n','\t']," ").chars().take(80).collect()}
