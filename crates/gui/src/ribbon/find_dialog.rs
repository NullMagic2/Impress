//! Modeless native Find/Replace window. It delegates matching and edits to the
//! existing revision-tagged search worker/editor, rather than maintaining a
//! second search engine or modifying the native text widgets as document text.
use super::*;
use editor::search::{SearchOptions, SearchScope};

use crate::FindDialogUi;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Operation { Next, Previous, Replace, ReplaceAll }
#[derive(Clone)]
pub(super) struct Request {
    operation: Operation,
    revision: u64,
    selection: Option<Selection>,
}
pub(super) struct FindDialog {
    controls: FindDialogUi,
    replacing: Cell<bool>,
    syncing: Cell<bool>,
    status: RefCell<String>,
    /// Prevent poll from replacing a useful replacement/error message every tick.
    last_search_caption: RefCell<String>,
}
impl FindDialog {
    fn new(ribbon:&Rc<Ribbon>) -> Rc<Self> {
        let controls=crate::i18n::localized(FindDialogUi::new(Some(&ribbon.ui.main_frame),false));
        let window = controls.find_window;
        window.fit();
        let fitted = window.get_size();
        let minimum = Size::new(fitted.width.max(window.from_dip_int(610)), fitted.height.max(window.from_dip_int(410)));
        window.set_min_size(minimum);
        window.set_size(Size::new(minimum.width.max(window.from_dip_int(650)), minimum.height));
        controls.find_window.set_escape_id(FindDialogUi::xrc_id("find_close"));
        let this=Rc::new(Self {controls,replacing:Cell::new(false),syncing:Cell::new(false),
            status:RefCell::new(String::new()),last_search_caption:RefCell::new(String::new())});
        this.bind(ribbon);this
    }
    fn bind(self:&Rc<Self>,ribbon:&Rc<Ribbon>) {
        for control in [self.controls.find_query,self.controls.replace_query] {
            let weak=Rc::downgrade(ribbon);
            control.on_text_updated(move |_|{if let Some(r)=weak.upgrade(){r.popup_inputs_changed();}});
            let weak=Rc::downgrade(ribbon);
            control.on_enter_pressed(move |_|{if let Some(r)=weak.upgrade(){r.queue_popup_search(Operation::Next);}});
        }
        let weak=Rc::downgrade(ribbon);
        self.controls.replace_text.on_text_updated(move |_|{if let Some(r)=weak.upgrade(){r.popup_inputs_changed();}});
        let weak=Rc::downgrade(ribbon);
        self.controls.replace_text.on_enter_pressed(move |_|{if let Some(r)=weak.upgrade(){r.queue_popup_search(Operation::Replace);}});
        for check in [self.controls.find_match_case,self.controls.find_whole_word] {
            let weak=Rc::downgrade(ribbon);
            check.on_toggled(move |_|{if let Some(r)=weak.upgrade(){r.popup_inputs_changed();}});
        }
        let weak=Rc::downgrade(ribbon);
        self.controls.find_scope.on_selection_changed(move |_|{if let Some(r)=weak.upgrade(){r.popup_inputs_changed();}});
        let weak=Rc::downgrade(ribbon);
        let popup=Rc::downgrade(self);
        self.controls.find_tabs.on_page_changed(move |_|{
            if let (Some(r),Some(d))=(weak.upgrade(),popup.upgrade()) {
                if d.syncing.get(){return;}
                d.set_mode(d.controls.find_tabs.selection()==1);
                r.pending_search.borrow_mut().take();
            }
        });
        for (button,operation) in [(self.controls.find_next,Operation::Next),(self.controls.find_previous,Operation::Previous),
            (self.controls.find_replace_one,Operation::Replace),(self.controls.find_replace_all,Operation::ReplaceAll)] {
            let weak=Rc::downgrade(ribbon);
            button.on_click(move |_|{if let Some(r)=weak.upgrade(){r.queue_popup_search(operation);}});
        }
        let weak=Rc::downgrade(ribbon);
        self.controls.find_close.on_click(move |_|{if let Some(r)=weak.upgrade(){r.close_find_dialog();}});
        let weak=Rc::downgrade(ribbon);
        self.controls.find_window.on_close(move |event|{
            let can_veto=match &event {WindowEventData::General(e)=>e.can_veto(),WindowEventData::Session(e)=>e.can_veto(),_=>false};
            if can_veto {
                if let Some(r)=weak.upgrade(){r.close_find_dialog();}
                match &event{WindowEventData::General(e)=>e.veto(),WindowEventData::Session(e)=>e.veto(),_=>{}}
                event.skip(false);
            }else{event.skip(true);}
        });
        // Ctrl+F/H also works while the modeless dialog owns keyboard focus.
        for text in [self.controls.find_query,self.controls.replace_query,self.controls.replace_text] {
            let weak=Rc::downgrade(ribbon);
            text.on_key_down(move |event|{
                if let WindowEventData::Keyboard(key)=&event {
                    if key.get_key_code()==Some(WXK_ESCAPE){if let Some(r)=weak.upgrade(){r.close_find_dialog();}event.skip(false);return;}
                    if key.cmd_down()&&!key.alt_down(){if let Some(letter)=crate::shortcuts::letter(key){if matches!(letter,'F'|'H'){
                        if let Some(r)=weak.upgrade(){r.open_find_dialog(letter=='H');}event.skip(false);return;
                    }}}
                }event.skip(true);
            });
        }
    }
    fn set_mode(&self,replace:bool) {
        self.replacing.set(replace);self.syncing.set(true);
        self.controls.find_tabs.change_selection(if replace{1}else{0});self.syncing.set(false);
        // Keep geometry stable between tabs; unavailable Replace actions are disabled.
        self.controls.find_replace_one.enable(replace);self.controls.find_replace_all.enable(replace);
        let query=if replace{self.controls.replace_query}else{self.controls.find_query};
        query.set_focus();query.set_selection(0,-1);
    }
    fn set_inputs(&self,query:&str,replacement:&str,options:SearchOptions) {
        self.syncing.set(true);
        if self.controls.find_query.get_value()!=query { self.controls.find_query.change_value(query); }
        if self.controls.replace_query.get_value()!=query { self.controls.replace_query.change_value(query); }
        if self.controls.replace_text.get_value()!=replacement { self.controls.replace_text.change_value(replacement); }
        self.controls.find_match_case.set_value(options.match_case);self.controls.find_whole_word.set_value(options.whole_word);
        self.controls.find_scope.set_value(&crate::i18n::text(&(scope_label(options.scope))));
        self.syncing.set(false);
    }
    fn options(&self)->SearchOptions {
        SearchOptions{match_case:self.controls.find_match_case.is_checked(),whole_word:self.controls.find_whole_word.is_checked(),
            scope:scope_from_label(&crate::i18n::choice_source(&self.controls.find_scope.get_value(), &["Entire document", "Body, including tables", "Headers and footers", "Footnotes and endnotes"]))}
    }
    fn query(&self)->String {if self.replacing.get(){self.controls.replace_query.get_value()}else{self.controls.find_query.get_value()}}
    fn message(&self,text:&str) {
        if self.status.borrow().as_str()==text{return;}
        self.controls.find_status.set_label(&crate::i18n::text(&(&text.chars().take(180).collect::<String>())));
        self.controls.find_status.set_tooltip(&crate::i18n::text(&(text)));self.status.replace(text.to_owned());
    }
}
fn scope_label(scope:SearchScope)->&'static str {
    if scope==scope_from_label("Body, including tables"){"Body, including tables"}
    else if scope==scope_from_label("Headers and footers"){"Headers and footers"}
    else if scope==scope_from_label("Footnotes and endnotes"){"Footnotes and endnotes"}
    else{"Entire document"}
}
fn scope_from_label(label:&str)->SearchScope {
    match label {
        "Body, including tables"=>SearchScope{body:true,tables:true,headers:false,footers:false,notes:false},
        "Headers and footers"=>SearchScope{body:false,tables:true,headers:true,footers:true,notes:false},
        "Footnotes and endnotes"=>SearchScope{body:false,tables:true,headers:false,footers:false,notes:true},
        _=>SearchScope::default(),
    }
}
impl Ribbon {
    pub(super) fn handle_find_shortcut(&self,event:&WindowEventData)->bool {
        if let WindowEventData::Keyboard(key)=event {
            if key.cmd_down()&&!key.alt_down(){if let Some(letter)=crate::shortcuts::letter(key){
                if matches!(letter,'F'|'H'){self.open_find_dialog(letter=='H');event.skip(false);return true;}
            }}
        } false
    }
    pub(super) fn find_visible(&self)->bool {
        self.find_dialog.borrow().as_ref().is_some_and(|d|d.controls.find_window.is_shown())
    }
    pub(super) fn find_has_focus(&self)->bool {
        let popup=self.find_dialog.borrow();let Some(d)=popup.as_ref().filter(|d|d.controls.find_window.is_shown())else{return false;};
        let c=&d.controls;
        c.find_window.has_focus()||c.find_tabs.has_focus()||c.find_query.has_focus()||c.replace_query.has_focus()
            ||c.replace_text.has_focus()||c.find_match_case.has_focus()||c.find_whole_word.has_focus()||c.find_scope.has_focus()
            ||c.find_previous.has_focus()||c.find_next.has_focus()||c.find_replace_one.has_focus()||c.find_replace_all.has_focus()||c.find_close.has_focus()
    }
    pub(super) fn sync_find_from_navigation(&self) {
        self.pending_search.borrow_mut().take();
        let popup=self.find_dialog.borrow().clone();
        if let Some(d)=popup {d.set_inputs(&self.ui.navigation_search.get_value(),&self.ui.navigation_replace.get_value(),self.search_options.get());}
    }
    pub(super) fn search_visible(&self)->bool { self.find_visible()||self.ui.navigator_panel.is_shown() }
    pub(super) fn open_find_dialog(&self,replace:bool) {
        let existing=self.find_dialog.borrow().clone();
        let popup=if let Some(dialog)=existing{dialog}else{
            let Some(owner)=self.owner.upgrade()else{return;};
            let dialog=FindDialog::new(&owner);self.find_dialog.replace(Some(Rc::clone(&dialog)));dialog
        };
        // A short one-paragraph selection is a useful initial search term. Do
        // not flatten a large/multi-story selection merely to open a dialog.
        if !popup.controls.find_window.is_shown() {
            let selected={let s=self.state.borrow();s.editor.selection.as_ref().filter(|v|
                v.anchor.node_id==v.focus.node_id&&v.anchor.byte_offset.abs_diff(v.focus.byte_offset)<=8192)
                .and_then(|_|s.editor.selected_text())};
            if let Some(text)=selected.filter(|s|!s.is_empty()&&!s.contains(['\n','\r','\0','\u{fffc}'])) {
                self.ui.navigation_search.change_value(&text);
            }
        }
        popup.set_inputs(&self.ui.navigation_search.get_value(),&self.ui.navigation_replace.get_value(),self.search_options.get());
        if !popup.controls.find_window.is_shown(){crate::placement::centre(&popup.controls.find_window);}popup.controls.find_window.show(true);popup.controls.find_window.raise();popup.set_mode(replace);
        self.pending_search.borrow_mut().take();
        popup.message(if self.ui.navigation_search.get_value().is_empty(){"Enter the text to find."}else{"Ready. Press Enter or Find Next."});
        self.search_due.set(Instant::now());self.navigation_revision.set(None);
        self.populate_navigation();
    }
    pub(super) fn close_find_dialog(&self) {
        let popup=self.find_dialog.borrow().clone();
        if let Some(d)=popup{d.controls.find_window.hide();}
        self.pending_search.borrow_mut().take();
        if !self.ui.navigator_panel.is_shown(){let mut s=self.state.borrow_mut();s.search_marks=None;refresh_overlay(&mut s);}
        self.focus_document();
    }
    fn popup_inputs_changed(&self) {
        let Some(d)=self.find_dialog.borrow().clone()else{return;};
        if d.syncing.get(){return;}
        let query=d.query();let replacement=d.controls.replace_text.get_value();let options=d.options();
        let search_changed=query!=self.ui.navigation_search.get_value()||options!=self.search_options.get();
        d.set_inputs(&query,&replacement,options);
        self.pending_search.borrow_mut().take();
        self.ui.navigation_search.change_value(&query);self.ui.navigation_replace.change_value(&replacement);
        self.search_options.set(options);
        if search_changed {
            self.navigation_revision.set(None);self.search_report.borrow_mut().take();self.search_cursor.set(None);
            self.search_due.set(Instant::now()+Duration::from_millis(180));
            let mut s=self.state.borrow_mut();s.search_marks=None;refresh_overlay(&mut s);
            d.message(if query.is_empty(){"Enter the text to find."}else{"Searching…"});
        }
    }
    fn queue_popup_search(&self,operation:Operation) {
        if matches!(operation,Operation::Replace|Operation::ReplaceAll)&&!self.find_dialog.borrow().as_ref().is_some_and(|d|d.replacing.get()){return;}
        if self.ui.navigation_search.get_value().is_empty(){if let Some(d)=self.find_dialog.borrow().as_ref(){d.message("Enter the text to find.");}return;}
        let request={let s=self.state.borrow();
            if matches!(operation,Operation::Replace|Operation::ReplaceAll)&&(crate::equation_ui::previewing(&s)||s.input.active()||s.object_drag.is_some()||s.dragging_selection){
                if let Some(d)=self.find_dialog.borrow().as_ref(){d.message("Finish text composition, object dragging, or the equation draft before replacing.");}
                return;
            }
            Request{operation,revision:s.editor.revision,selection:s.editor.selection.clone()}};
        self.pending_search.replace(Some(request));self.search_due.set(Instant::now());
        if !self.current_search_ready(){self.populate_navigation();}
    }
    fn current_search_ready(&self)->bool {
        let revision=self.state.borrow().editor.revision;
        self.navigation_revision.get()==Some(revision)&&self.search_report.borrow().as_ref().is_some_and(|r|
            r.revision==revision&&r.query==self.ui.navigation_search.get_value()&&r.options==self.search_options.get())
    }
    pub(super) fn poll_find_dialog(&self) {
        let Some(d)=self.find_dialog.borrow().clone().filter(|d|d.controls.find_window.is_shown())else{return;};
        let caption=self.ui.navigation_hint.get_label();
        if *d.last_search_caption.borrow()!=caption {
            d.last_search_caption.replace(caption.clone());
            if !self.ui.navigation_search.get_value().is_empty(){d.message(&caption);}
        }
        let Some(request)=self.pending_search.borrow().clone()else{return;};
        let stale={let s=self.state.borrow();request.revision!=s.editor.revision
            ||request.selection!=s.editor.selection
            ||(matches!(request.operation,Operation::Replace|Operation::ReplaceAll)
                &&(s.input.active()||crate::equation_ui::previewing(&s)||s.object_drag.is_some()||s.dragging_selection))};
        if stale{self.pending_search.borrow_mut().take();d.message("The document or selection changed. Press Find or Replace again.");return;}
        if !self.current_search_ready(){
            if self.navigation_revision.get()==Some(request.revision)&&self.search_job.borrow().is_none(){
                self.pending_search.borrow_mut().take();d.message(&caption);
            }return;
        }
        self.pending_search.borrow_mut().take();
        if self.search_report.borrow().as_ref().is_none_or(|r|r.matches.is_empty()){d.message("No matches found in the selected scope.");return;}
        let result=match request.operation {
            Operation::Next=>{self.search_step(true);Ok(())},Operation::Previous=>{self.search_step(false);Ok(())},
            Operation::ReplaceAll=>self.replace_found(true),
            Operation::Replace=>{
                // Replace means the currently selected result. First find/select a
                // result when the caret has moved; never replace index zero blindly.
                let selected={let report=self.search_report.borrow();let s=self.state.borrow();
                    report.as_ref().and_then(|r|self.search_cursor.get().and_then(|i|r.matches.get(i)))
                        .is_some_and(|m|selection_matches(s.editor.selection.as_ref(),&m.node,m.range.start,m.range.end))};
                if selected{self.replace_found(false)}else{self.search_step(true);d.message(&self.ui.navigation_hint.get_label());return;}
            },
        };
        if let Err(error)=result {d.message(&error);self.state.borrow_mut().action_status=Some(error);}
        else if matches!(request.operation,Operation::Next|Operation::Previous){d.message(&self.ui.navigation_hint.get_label());}
        else if let Some(status)=self.state.borrow().action_status.as_ref(){d.message(status);}
    }
}
pub(super) fn selection_matches(selection:Option<&Selection>,node:&model::NodeId,start:usize,end:usize)->bool {
    selection.is_some_and(|s|s.anchor.node_id==*node&&s.focus.node_id==*node
        &&s.anchor.byte_offset.min(s.focus.byte_offset)==start&&s.anchor.byte_offset.max(s.focus.byte_offset)==end)
}
