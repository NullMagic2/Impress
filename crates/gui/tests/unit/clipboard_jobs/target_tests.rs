use super::*;
fn fixture()->EditorState{
    let mut d=Document::default();let p=model::Paragraph::plain("αβ");let id=p.id.clone();
    d.sections[0].blocks.push(model::Block::Paragraph(p));let mut e=EditorState::new(d);e.set_caret(id,0);e
}
#[test]fn unrelated_new_edits_never_accept_the_old_staged_editor(){
    let mut e=fixture();let guard=Target::from_editor(&e,None);
    assert!(guard.matches_editor(&e,None,false));e.insert_text("new").unwrap();
    assert!(!guard.matches_editor(&e,None,false));
}

fn pending(command:AppCommand,editor:&EditorState)->Job {
    let (_sender,receiver)=mpsc::channel();
    Job{target:Target::from_editor(editor,None),sequence:0,command,cancel:Arc::new(AtomicBool::new(false)),receiver,following:None}
}

#[test]
fn immediate_paste_waits_for_copy_and_keeps_the_requested_destination() {
    let mut e=fixture();let mut job=pending(AppCommand::Copy,&e);
    e.move_right(false);let destination=e.selection.clone();
    job.follow_with_paste(AppCommand::Paste,Target::from_editor(&e,None)).unwrap();
    let request=job.following.as_ref().unwrap();
    assert_eq!(request.command,AppCommand::Paste);assert_eq!(request.target.selection,destination);
    assert!(request.target.matches_editor(&e,None,false));
    e.insert_text("changed").unwrap();assert!(!request.target.matches_editor(&e,None,false));
}

#[test]
fn cut_and_plain_paste_preserve_the_guard_and_cancel_drops_the_followup() {
    let mut e=fixture();e.select_all();let mut job=pending(AppCommand::Cut,&e);
    job.follow_with_paste(AppCommand::PastePlain,Target::from_editor(&e,None)).unwrap();
    assert!(job.target.matches_editor(&e,None,false));
    assert!(job.following.as_ref().unwrap().target.matches_editor(&e,None,false));
    job.cancel.store(true,Ordering::Release);
    assert!(job.follow_with_paste(AppCommand::Paste,Target::from_editor(&e,None)).is_err());
    assert_eq!(e.selected_text().as_deref(),Some("αβ"));
}

#[test]
fn a_pending_paste_does_not_accept_an_overlapping_clipboard_operation() {
    let e=fixture();let mut job=pending(AppCommand::Paste,&e);
    assert!(job.follow_with_paste(AppCommand::Paste,Target::from_editor(&e,None)).is_err());
    assert!(job.following.is_none());
}
