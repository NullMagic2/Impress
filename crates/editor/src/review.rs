use super::*;
use model::{AnchorRange,Bookmark,ReviewComment,ReviewState,TextAnchor,TextRevision,document_paragraphs,inline_text,valid_range};

impl EditorState {
    pub fn selection_anchor_range(&self)->Result<AnchorRange,String> {
        let selection=self.selection.as_ref().ok_or("Place the caret in the document first")?;
        let paragraphs=document_paragraphs(&self.document);
        let key=|p:&DocumentPosition|paragraphs.iter().position(|v|v.id==p.node_id).map(|i|(i,p.byte_offset));
        let a=key(&selection.anchor).ok_or("Selection start is no longer available")?;
        let b=key(&selection.focus).ok_or("Selection end is no longer available")?;
        let (first,last)=if a<=b{(&selection.anchor,&selection.focus)}else{(&selection.focus,&selection.anchor)};
        let same_body=self.locate_paragraph(&first.node_id).is_some()&&self.locate_paragraph(&last.node_id).is_some();
        let same_stream=paragraph_stream_for_node(&self.document,&first.node_id).is_some_and(|ps|ps.iter().any(|p|p.id==last.node_id));
        if !same_body&&!same_stream{return Err("Reference anchors must stay within one editing stream".into());}
        let range=AnchorRange{start:TextAnchor{paragraph:first.node_id.clone(),offset:first.byte_offset},end:TextAnchor{paragraph:last.node_id.clone(),offset:last.byte_offset}};
        if !valid_range(&self.document,&range){return Err("Invalid reference range".into());}Ok(range)
    }
    pub(super) fn commit_review(&mut self,label:&'static str,after:ReviewState)->Result<(),String> {
        let before=self.document.review.clone();self.document.review=after.clone();
        self.commit(label,vec![Mutation::ReviewChanged{before,after}],self.selection.clone(),None)
    }
    pub fn add_bookmark(&mut self,name:&str)->Result<(),String> {
        let name=name.trim();
        if name.is_empty() || name.len()>40 || !name.chars().next().is_some_and(|c|c.is_ascii_alphabetic()||c=='_')
            || !name.chars().all(|c|c.is_ascii_alphanumeric()||c=='_') {
            return Err("Use 1–40 letters, digits or underscores, starting with a letter or underscore".into());
        }
        if self.document.review.bookmarks.values().any(|b|b.name.eq_ignore_ascii_case(name)){return Err("That bookmark name already exists".into());}
        let id=NodeId::new();let range=self.selection_anchor_range()?;
        let mut after=self.document.review.clone();after.bookmarks.insert(id.clone(),Bookmark{id,name:name.into(),range:Some(range)});
        self.commit_review("Add bookmark",after)
    }
    pub fn delete_bookmark(&mut self,id:&NodeId)->Result<(),String> {
        let mut after=self.document.review.clone();after.bookmarks.remove(id).ok_or("Bookmark not found")?;
        self.commit_review("Delete bookmark",after)
    }
    pub fn go_to_range(&mut self,range:&AnchorRange)->Result<(),String> {
        if !valid_range(&self.document,range){return Err("The referenced text was removed".into());}
        self.set_caret(range.start.paragraph.clone(),range.start.offset);
        self.extend_selection(range.end.paragraph.clone(),range.end.offset);Ok(())
    }
    pub fn add_comment(&mut self,author:&str,text:&str)->Result<(),String> {
        if text.trim().is_empty() || text.len()>65536 {return Err("Enter a comment of at most 64 KiB".into());}
        let range=self.selection_anchor_range()?;let id=NodeId::new();let mut after=self.document.review.clone();
        after.comments.insert(id.clone(),ReviewComment{date:Some(model::utc_timestamp()),replies:Vec::new(),id,author:author.trim().chars().take(100).collect(),text:text.into(),range:Some(range),resolved:false});
        self.commit_review("Add comment",after)
    }
    pub fn update_comment(&mut self,id:&NodeId,text:Option<&str>,resolved:Option<bool>)->Result<(),String> {
        let mut after=self.document.review.clone();let comment=after.comments.get_mut(id).ok_or("Comment not found")?;
        if let Some(text)=text {if text.trim().is_empty()||text.len()>65536{return Err("Invalid comment length".into());}comment.text=text.into();}
        if let Some(resolved)=resolved{comment.resolved=resolved;}
        self.commit_review("Edit comment",after)
    }
    pub fn delete_comment(&mut self,id:&NodeId)->Result<(),String> {
        let mut after=self.document.review.clone();after.comments.remove(id).ok_or("Comment not found")?;self.commit_review("Delete comment",after)
    }
    pub fn reanchor_comment(&mut self,id:&NodeId)->Result<(),String>{let range=self.selection_anchor_range()?;let mut after=self.document.review.clone();after.comments.get_mut(id).ok_or("Comment not found")?.range=Some(range);self.commit_review("Reanchor comment",after)}
    pub fn reanchor_bookmark(&mut self,id:&NodeId)->Result<(),String>{let range=self.selection_anchor_range()?;let mut after=self.document.review.clone();after.bookmarks.get_mut(id).ok_or("Bookmark not found")?.range=Some(range);self.commit_review("Reanchor bookmark",after)}
    pub fn insert_page_reference(&mut self,id:&NodeId)->Result<(),String>{
        let bookmark=self.document.review.bookmarks.get(id).ok_or("Bookmark not found")?;let range=bookmark.range.as_ref().filter(|r|valid_range(&self.document,r)).ok_or("Bookmark has no valid target")?;let _=range;
        let field=Inline::Field(model::Field{id:NodeId::new(),kind:model::FieldKind::PageReference{bookmark:bookmark.name.clone()},display_text:None,color:None});
        let selection=self.selection.clone();let mut staged=self.clone();let mut changes=Vec::new();staged.delete_selection_into(&mut changes)?;let pos=staged.focus_position().ok_or("Place the caret first")?;let before=staged.paragraph(&pos.node_id).cloned().ok_or("Paragraph not found")?;let mut after=before.clone();insert_inline_into_paragraph(&mut after,pos.byte_offset,field)?;staged.replace_paragraph(&after)?;staged.set_caret(pos.node_id.clone(),pos.byte_offset+3);changes.push(Mutation::AnchorsTransformed{mapping:anchor_maps::AnchorMapping::Replace{paragraph:pos.node_id,start:pos.byte_offset,end:pos.byte_offset,inserted:3},suppress:1});changes.push(Mutation::ParagraphChanged{id:after.id.clone(),before,after});staged.commit("Insert page reference",changes,selection,None)?;*self=staged;Ok(())
    }
    pub fn set_text_tracking(&mut self,enabled:bool,author:&str)->Result<(),String> {
        let mut after=self.document.review.clone();after.track_text=enabled;if enabled{after.track_history=false;}
        if enabled {after.author=author.trim().chars().take(100).collect();if after.author.is_empty(){after.author="Author".into();}}
        self.commit_review("Track paragraph text",after)
    }
    pub fn resolve_revision(&mut self,id:&NodeId,accept:bool)->Result<(),String> {
        let revision=self.document.review.revisions.get(id).cloned().ok_or("Revision not found")?;
        let current=self.paragraph(&revision.paragraph).cloned();
        let before_review=self.document.review.clone();let mut after_review=before_review.clone();after_review.revisions.remove(id);
        let selection=self.selection.clone();let mut changes=Vec::new();
        if !accept {
            let current=current.ok_or("The revised paragraph was removed; undo the structural deletion before rejecting, or explicitly accept/discard the original")?;
            self.replace_paragraph(&revision.before)?;
            rebase_ranges(&mut after_review,&[(current.clone(),revision.before.clone())]);
            self.set_caret(revision.paragraph.clone(),0);
            changes.push(Mutation::ParagraphChanged{id:revision.paragraph,before:current,after:revision.before});
        }
        after_review.footnotes=self.document.review.footnotes.clone();
        self.document.review=after_review.clone();changes.push(Mutation::ReviewChanged{before:before_review,after:after_review});
        self.commit(if accept{"Accept text revision"}else{"Reject text revision"},changes,selection,None)
    }
    pub fn insert_footnote(&mut self,text:&str)->Result<(),String> { self.insert_note_with_text(text,model::NoteKind::Footnote) }
    pub fn insert_endnote(&mut self,text:&str)->Result<(),String> { self.insert_note_with_text(text,model::NoteKind::Endnote) }
    fn insert_note_with_text(&mut self,text:&str,kind:model::NoteKind)->Result<(),String> {
        if text.trim().is_empty(){return Err("Enter the note text".into());}
        self.insert_note(text,kind).map(|_|())
    }
    /// Word's Insert Footnote / Insert Endnote: a numbered reference at the
    /// caret and an empty note, with the caret moved into the note's text.
    pub fn insert_note_here(&mut self,kind:model::NoteKind)->Result<(),String> {
        let id=self.insert_note("",kind)?;
        self.enter_footnote(&id)
    }
    /// Adds a reference at the caret and a note holding `text` (one empty
    /// paragraph when `text` is empty); returns the note's id.
    fn insert_note(&mut self,text:&str,kind:model::NoteKind)->Result<NodeId,String> {
        if self.document.review.track_text{return Err("Turn off paragraph-text tracking before inserting a footnote".into());}
        if self.is_editing_header_footer()||self.focus_position().is_some_and(|p|self.note_for_paragraph(&p.node_id).is_some()){return Err("Footnotes belong in the body, not headers, footers or another note".into());}
        if text.len()>65536{return Err("Enter a footnote of at most 64 KiB".into());}
        let selection=self.selection.clone().ok_or("Place the caret at the footnote reference")?;
        if !selection.is_collapsed(){return Err("Place the caret without selecting text before inserting a footnote".into());}
        let position=selection.focus.clone();let before=self.paragraph(&position.node_id).cloned().ok_or("Paragraph not found")?;
        let mut after=before.clone();let id=NodeId::new();
        insert_inline_into_paragraph(&mut after,position.byte_offset,Inline::Field(model::Field{id:NodeId::new(),kind:model::FieldKind::FootnoteReference{note:id.clone()},display_text:None,color:None}))?;
        let review_before=self.document.review.clone();let mut review_after=review_before.clone();
        anchor_maps::AnchorMapping::Replace{paragraph:position.node_id.clone(),start:position.byte_offset,end:position.byte_offset,inserted:3}.apply(&mut review_after);
        review_after.footnotes.insert(id.clone(),model::Footnote{kind,id:id.clone(),blocks:(note_blocks(text)).into()});
        self.replace_paragraph(&after)?;self.document.review=review_after.clone();self.set_caret(position.node_id.clone(),position.byte_offset+3);
        self.commit("Insert footnote",vec![Mutation::ParagraphChanged{id:position.node_id,before,after},Mutation::ReviewChanged{before:review_before,after:review_after}],Some(selection),None)?;
        Ok(id)
    }
    pub fn edit_footnote_text(&mut self,id:&NodeId,text:&str)->Result<(),String> {
        if self.document.review.track_text{return Err("Turn off paragraph-text tracking before editing footnotes".into());}
        if text.trim().is_empty()||text.len()>65536{return Err("Enter a footnote of at most 64 KiB".into());}
        let mut after=self.document.review.clone();let mut removed=Vec::new();model::visit_paragraphs(&after.footnotes.get(id).ok_or("Footnote not found")?.blocks,&mut removed);let ids=removed.iter().map(|p|p.id.clone()).collect::<Vec<_>>();
        after.footnotes.get_mut(id).ok_or("Footnote not found")?.blocks=(note_blocks(text)).into();
        for range in after.bookmarks.values_mut().map(|b|&mut b.range).chain(after.comments.values_mut().map(|c|&mut c.range)){if range.as_ref().is_some_and(|r|ids.contains(&r.start.paragraph)||ids.contains(&r.end.paragraph)){*range=None;}}
        let selection=self.selection.clone();let before=self.document.review.clone();self.document.review=after.clone();
        if self.selection.as_ref().is_some_and(|s|ids.contains(&s.focus.node_id)){self.enter_footnote(id)?;}
        self.commit("Edit footnote",vec![Mutation::ReviewChanged{before,after}],selection,None)
    }
    pub fn delete_footnote(&mut self,id:&NodeId)->Result<(),String> {
        if self.document.review.track_text{return Err("Turn off paragraph-text tracking before deleting footnotes".into());}
        let refs=model::footnote_references(&self.document).into_iter().filter(|(_,note)|note==id).collect::<Vec<_>>();
        if refs.len()>1{return Err("This note has multiple references. Delete the references individually before removing it.".into());}
        let mut changes=Vec::new();let before_review=self.document.review.clone();let mut after_review=before_review.clone();
        let removed=after_review.footnotes.remove(id).ok_or("Footnote not found")?;let mut ps=Vec::new();model::visit_paragraphs(&removed.blocks,&mut ps);let ids=ps.iter().map(|p|p.id.clone()).collect::<Vec<_>>();
        for range in after_review.bookmarks.values_mut().map(|b|&mut b.range).chain(after_review.comments.values_mut().map(|c|&mut c.range)){if range.as_ref().is_some_and(|r|ids.contains(&r.start.paragraph)||ids.contains(&r.end.paragraph)){*range=None;}}
        let selection=self.selection.clone();
        if let Some((anchor,_))=refs.first(){let before=self.paragraph(&anchor.paragraph).cloned().ok_or("Reference paragraph not found")?;let mut after=before.clone();
            let mut content=slice_content(&after.content,0,anchor.offset)?;content.extend(slice_content(&after.content,anchor.offset+3,paragraph_flat_text(&after).len())?);after.content=(normalized_content(content)).into();
            self.replace_paragraph(&after)?;anchor_maps::AnchorMapping::Replace{paragraph:anchor.paragraph.clone(),start:anchor.offset,end:anchor.offset+3,inserted:0}.apply(&mut after_review);self.set_caret(anchor.paragraph.clone(),anchor.offset);
            changes.push(Mutation::ParagraphChanged{id:anchor.paragraph.clone(),before,after});
        }
        self.document.review=after_review.clone();changes.push(Mutation::ReviewChanged{before:before_review,after:after_review});self.commit("Delete footnote",changes,selection,None)
    }

    pub(super) fn prepare_review_transaction(&mut self,label:&str,mutations:&mut Vec<Mutation>,selection:&Option<Selection>)->Result<(),String> {
        if mutations.iter().any(|m|matches!(m,Mutation::ReviewChanged{..})){return Ok(());}
        let before=self.document.review.clone();
        // This is explicitly paragraph-text tracking. Reject unsupported edits
        // atomically rather than letting structural edits bypass tracking silently.
        if before.track_text && (!matches!(label,"Typing"|"Backspace"|"Delete"|"Delete selection"|"Paste"|"Replace"|"Replace all") || mutations.iter().any(|m|!matches!(m,Mutation::AnchorsTransformed{..})&&!matches!(m,Mutation::ParagraphChanged{before,after,..} if before.style==after.style && before.properties==after.properties && text_only(&before.content)&&text_only(&after.content)))) {
            for m in mutations.iter().rev(){apply_mutation(&mut self.document,m,false)?;}
            self.selection=selection.clone();self.reset_typing_properties_from_caret();
            return Err("Paragraph-text tracking supports text edits within existing text-only paragraphs. Turn it off before changing structure or objects.".into());
        }
        let mut after=before.clone();let mut pairs=Vec::new();
        for mutation in mutations.iter(){match mutation {
            Mutation::ParagraphChanged{before,after,..}=>pairs.push((before.clone(),after.clone())),
            Mutation::FootnoteChanged{before,after,..}=>note_pairs(before,after,&mut pairs),
            Mutation::SectionChanged{before,after,..}=>section_pairs(std::slice::from_ref(before),std::slice::from_ref(after),&mut pairs),
            Mutation::SectionsReplaced{before,after,..}=>section_pairs(before,after,&mut pairs),_=>{}
        }}
        let mut suppress=0usize;
        for mutation in mutations.iter() {
            if let Mutation::AnchorsTransformed{mapping,suppress:count}=mutation {
                mapping.apply(&mut after); suppress=*count; continue;
            }
            if suppress>0 { suppress-=1; continue; }
            let mut local=Vec::new();
            match mutation {
                Mutation::ParagraphChanged{before,after,..}=>local.push((before.clone(),after.clone())),
                Mutation::FootnoteChanged{before,after,..}=>note_pairs(before,after,&mut local),
                Mutation::SectionChanged{before,after,..}=>section_pairs(std::slice::from_ref(before),std::slice::from_ref(after),&mut local),
                Mutation::SectionsReplaced{before,after,..}=>section_pairs(before,after,&mut local),_=>{}
            }
            rebase_ranges(&mut after,&local);
            // Unknown structural replacements remain conservative. Explicit
            // split/join/splice maps above preserve identity without text search.
            if matches!(mutation,Mutation::SectionChanged{..}|Mutation::SectionsReplaced{..}|Mutation::FootnoteChanged{..}) {
                let changed=local.iter().filter(|(a,b)|inline_text(&a.content)!=inline_text(&b.content)).map(|(p,_)|p.id.clone()).collect::<Vec<_>>();
                for range in after.bookmarks.values_mut().map(|b|&mut b.range).chain(after.comments.values_mut().map(|c|&mut c.range)) {
                    if range.as_ref().is_some_and(|r|changed.contains(&r.start.paragraph)||changed.contains(&r.end.paragraph)){*range=None;}
                }
            }
        }
        if before.track_text&&pairs.iter().any(|(old,new)|old.content!=new.content&&before.revisions.values().any(|r|r.paragraph==old.id&&r.author!=before.author)){
            for m in mutations.iter().rev(){apply_mutation(&mut self.document,m,false)?;}self.selection=selection.clone();self.reset_typing_properties_from_caret();return Err("Resolve the pending author's changes in this paragraph before editing it under another author name".into());
        }
        if before.track_text {
            for (old,new) in &pairs {if old.content!=new.content && !after.revisions.values().any(|r|r.paragraph==old.id) {
                let id=NodeId::new();after.revisions.insert(id.clone(),TextRevision{id,paragraph:old.id.clone(),author:after.author.clone(),date:Some(model::utc_timestamp()),before:old.clone()});
            }}
        }
        // Never silently drop an orphaned comment. Keep its text in the review
        // list, with no live anchor, until the user deliberately deletes it.
        for range in after.bookmarks.values_mut().map(|b|&mut b.range).chain(after.comments.values_mut().map(|c|&mut c.range)) {
            if range.as_ref().is_some_and(|r|!valid_range(&self.document,r)){*range=None;}
        }
        if before!=after {self.document.review=after.clone();mutations.push(Mutation::ReviewChanged{before,after});}
        Ok(())
    }
}
fn text_only(items:&[Inline])->bool {items.iter().all(|i|match i {Inline::Text(_)|Inline::Tab{..}|Inline::LineBreak{..}|Inline::PaginationBreak{..}=>true,Inline::Hyperlink(h)=>text_only(&h.content),_=>false})}
fn note_pairs(before:&model::Footnote,after:&model::Footnote,out:&mut Vec<(Paragraph,Paragraph)>){let mut a=Vec::new();let mut b=Vec::new();model::visit_paragraphs(&before.blocks,&mut a);model::visit_paragraphs(&after.blocks,&mut b);for old in a{if let Some(new)=b.iter().find(|p|p.id==old.id){if old!=*new{out.push((old.clone(),(*new).clone()));}}}}
fn section_pairs(before:&[Section],after:&[Section],out:&mut Vec<(Paragraph,Paragraph)>) {
    let mut a=Document::default();a.sections=before.to_vec();let mut b=Document::default();b.sections=after.to_vec();
    let news=document_paragraphs(&b);
    for old in document_paragraphs(&a){if let Some(new)=news.iter().find(|p|p.id==old.id){if old!=*new{out.push((old.clone(),(*new).clone()));}}}
}
pub(super) fn rebase_ranges(review:&mut ReviewState,pairs:&[(Paragraph,Paragraph)]) {
    for (old,new) in pairs {
        let old_text=inline_text(&old.content);let new_text=inline_text(&new.content);
        // A whole-revision rejection can change both ends of a paragraph. Keep
        // anchors in its unchanged middle, rather than treating it as one edit.
        for change in model::text_changes(&old_text,&new_text).iter().rev(){
            anchor_maps::AnchorMapping::Replace{paragraph:old.id.clone(),start:change.old.start,end:change.old.end,inserted:change.new.len()}.apply(review);
        }
    }
}

#[cfg(test)] #[path = "../tests/unit/review.rs"] mod tests;

fn note_blocks(text:&str)->Vec<Block>{
    // An empty note still needs a paragraph for the caret.
    let lines:Vec<&str>=if text.is_empty(){vec![""]}else{text.lines().collect()};
    lines.into_iter().map(|line|{let mut p=Paragraph::plain(line);p.properties.line_spacing=Some(1.1);p.properties.space_after_pt=Some(2.0);for i in &mut p.content{if let Inline::Text(t)=i{t.properties.font_size_pt=Some(9.0);}}Block::Paragraph(p)}).collect()}

// Review individual text changes.
// Review independent original/final text differences without accepting an
// unrelated edit in the same paragraph. This is net-change review, not a log
// of every keystroke or reconstructed multi-author Word revision history.
#[derive(Clone,Debug)]
pub struct RevisionChange { pub index:usize, pub start:usize, pub end:usize, pub removed:String, pub inserted:String }
impl EditorState {
    pub fn revision_changes(&self,id:&NodeId)->Result<Vec<RevisionChange>,String>{
        let revision=self.document.review.revisions.get(id).ok_or("Revision not found")?;
        let current=self.paragraph(&revision.paragraph).ok_or("Revised paragraph no longer exists")?;
        let old=model::inline_text(&revision.before.content);let new=model::inline_text(&current.content);
        Ok(model::text_changes(&old,&new).into_iter().enumerate().map(|(index,c)|RevisionChange{index,start:c.new.start,end:c.new.end,removed:old[c.old].into(),inserted:new[c.new].into()}).collect())
    }
    pub fn resolve_revision_change(&mut self,id:&NodeId,index:usize,accept:bool,expected_revision:u64)->Result<(),String>{
        if self.revision!=expected_revision{return Err("The document changed; reopen the revision list".into());}
        let revision=self.document.review.revisions.get(id).cloned().ok_or("Revision not found")?;
        let current=self.paragraph(&revision.paragraph).cloned().ok_or("Revised paragraph not found")?;
        let old=model::inline_text(&revision.before.content);let new=model::inline_text(&current.content);
        let hunks=model::text_changes(&old,&new);let change=hunks.get(index).ok_or("Change no longer exists")?;
        let review_before=self.document.review.clone();let mut review_after=review_before.clone();let selection=self.selection.clone();let mut mutations=Vec::new();
        if accept {
            let original=&mut review_after.revisions.get_mut(id).ok_or("Revision not found")?.before;
            let mut content=slice_content(&original.content,0,change.old.start)?;
            content.extend(slice_content(&current.content,change.new.start,change.new.end)?);
            content.extend(slice_content(&original.content,change.old.end,old.len())?);original.content=(normalized_content(content)).into();
        }else{
            let mut after=current.clone();let mut content=slice_content(&current.content,0,change.new.start)?;
            content.extend(slice_content(&revision.before.content,change.old.start,change.old.end)?.into_iter().map(fresh_inline_ids));
            content.extend(slice_content(&current.content,change.new.end,new.len())?);after.content=(normalized_content(content)).into();
            super::anchor_maps::AnchorMapping::Replace{paragraph:current.id.clone(),start:change.new.start,end:change.new.end,inserted:change.old.len()}.apply(&mut review_after);
            self.replace_paragraph(&after)?;self.set_caret(after.id.clone(),change.new.start);
            mutations.push(Mutation::ParagraphChanged{id:after.id.clone(),before:current,after});
        }
        let remaining=review_after.revisions.get(id).is_some_and(|r|self.paragraph(&r.paragraph).is_some_and(|p|model::inline_text(&r.before.content)==model::inline_text(&p.content)));
        if remaining{review_after.revisions.remove(id);}
        review_after.footnotes=self.document.review.footnotes.clone();
        self.document.review=review_after.clone();mutations.push(Mutation::ReviewChanged{before:review_before,after:review_after});
        self.commit(if accept{"Accept text change"}else{"Reject text change"},mutations,selection,None)
    }
}

// Keep and resolve the native revision journal.
// Chronological native journal, deliberately distinct from OOXML net changes.
// Rejection is compare-and-replay guarded; it never rewinds unrelated new work.
use model::{CommentReply,RevisionEvent,RevisionStatus};
const MAX_EVENT:usize=4*1024*1024;
const MAX_HISTORY:usize=32*1024*1024;
fn stripped(mut review:ReviewState)->ReviewState {
    review.history.clear();review.track_history=false;review.track_text=false;review.author.clear();review
}
fn comparable(document:&Document)->Document {
    let mut d=document.clone();d.review=stripped(d.review);d.resources.clear();d
}
impl EditorState {
    pub fn set_revision_history(&mut self,enabled:bool,author:&str)->Result<(),String>{
        if author.trim().is_empty()&&enabled{return Err("Enter an author name for the native revision journal".into());}
        let mut after=self.document.review.clone();after.track_history=enabled;
        after.author=author.trim().chars().take(100).collect();if enabled{after.track_text=false;}
        self.commit_review("Native history settings",after)
    }
    pub fn reply_to_comment(&mut self,id:&NodeId,author:&str,text:&str)->Result<(),String>{
        if text.trim().is_empty()||text.len()>65536{return Err("Reply must contain 1–65536 bytes".into());}
        let mut after=self.document.review.clone();let comment=after.comments.get_mut(id).ok_or("Comment no longer exists")?;
        if comment.replies.len()>=256{return Err("Comment thread is limited to 256 replies".into());}
        comment.replies.push(CommentReply{id:NodeId::new(),author:author.trim().chars().take(100).collect(),date:model::utc_timestamp(),text:text.into()});
        comment.resolved=false;self.commit_review("Reply to comment",after)
    }
    pub fn delete_comment_reply(&mut self,id:&NodeId,reply:&NodeId)->Result<(),String>{
        let mut after=self.document.review.clone();let c=after.comments.get_mut(id).ok_or("Comment not found")?;
        let at=c.replies.iter().position(|r|&r.id==reply).ok_or("Reply not found")?;c.replies.remove(at);self.commit_review("Delete comment reply",after)
    }
    pub(super) fn record_native_revision(&mut self,label:&str,mutations:&mut Vec<Mutation>)->Result<(),String>{
        if !self.document.review.track_history||label.starts_with("Native history"){return Ok(());}
        let mut stored=Vec::new();
        for m in mutations.iter(){match m {
            Mutation::AnchorsTransformed{..}|Mutation::ResourceInserted{..}|Mutation::ListInserted{..}=>{},
            Mutation::ReviewChanged{before,after}=>{
                let before=stripped(before.clone());let after=stripped(after.clone());
                if before!=after{stored.push(Mutation::ReviewChanged{before,after});}
            },_=>stored.push(m.clone()),
        }}
        if stored.is_empty(){return Ok(());}
        let payload:model::RevisionPayload=serde_json::to_value(&stored).map_err(|e|e.to_string())?.into();
        if payload.encoded_len().map_err(|e|e.to_string())?>MAX_EVENT{return Err("This edit exceeds the 4 MiB native revision event limit; save a copy or disable tracking explicitly".into());}
        let before=self.document.review.clone();let mut after=before.clone();
        if after.history.len()>=2000{return Err("Native journal has 2000 events; save an archival copy before starting a fresh document".into());}
        after.history.push(RevisionEvent{id:NodeId::new(),author:after.author.clone(),date:model::utc_timestamp(),label:label.into(),status:RevisionStatus::Pending,payload: payload.into()});
        if model::revision_history_bytes(&after.history).map_err(|e|e.to_string())?>MAX_HISTORY{return Err("Native revision journal exceeds its 32 MiB limit; the edit was not committed".into());}
        self.document.review=after.clone();mutations.push(Mutation::ReviewChanged{before,after});Ok(())
    }
    pub fn resolve_history_event(&mut self,id:&NodeId,accept:bool)->Result<(),String>{
        let event=self.document.review.history.iter().find(|e|&e.id==id).cloned().ok_or("History event not found")?;
        if event.status!=RevisionStatus::Pending{return Err("History event was already resolved".into());}
        if accept{
            let mut after=self.document.review.clone();after.history.iter_mut().find(|e|&e.id==id).unwrap().status=RevisionStatus::Accepted;
            return self.commit_review("Native history accept",after);
        }
        let bytes=serde_json::to_vec(&event.payload).map_err(|e|e.to_string())?;
        if bytes.len()>MAX_EVENT{return Err("Invalid native history payload length".into());}
        let changes:Vec<Mutation>=serde_json::from_slice(&bytes).map_err(|e|format!("Invalid native history: {e}"))?;
        if changes.is_empty()||changes.len()>10000{return Err("Invalid native history operation count".into());}
        if changes.iter().any(|m|matches!(m,Mutation::ResourceInserted{..}|Mutation::ListInserted{..}|Mutation::AnchorsTransformed{..})){return Err("Unsupported native history payload".into());}
        let before=self.document.clone();let mut candidate=before.clone();
        // Review snapshot comparisons include anchors/notes/threads, but exclude
        // journal resolution and author settings. Conflicts fail conservatively.
        for m in changes.iter().rev(){
            if let Mutation::ReviewChanged{after,..}=m{if stripped(candidate.review.clone())!=*after{return Err("Later reference/review edits depend on this change. Resolve them first.".into());}}
            apply_mutation(&mut candidate,m,false)?;
        }
        let mut replay=candidate.clone();for m in &changes{apply_mutation(&mut replay,m,true)?;}
        if comparable(&replay)!=comparable(&before){return Err("Later edits overlap this change; rejecting it would overwrite newer work. Resolve dependent changes first.".into());}
        let saved=&before.review;candidate.review.history=saved.history.clone();candidate.review.author=saved.author.clone();candidate.review.track_history=saved.track_history;candidate.review.track_text=saved.track_text;
        candidate.review.history.iter_mut().find(|e|&e.id==id).unwrap().status=RevisionStatus::Rejected;
        let selection=self.selection.clone();let review_before=before.review.clone();let review_after=candidate.review.clone();let sections_before=before.sections.clone();let sections_after=candidate.sections.clone();
        let styles_before=before.styles.clone();let styles_after=candidate.styles.clone();
        self.document=candidate;
        if let Some(mut selected)=self.selection.clone(){
            let mut valid=true;
            for position in [&mut selected.anchor,&mut selected.focus]{
                if let Some(p)=self.paragraph(&position.node_id){let text=paragraph_flat_text(p);position.byte_offset=position.byte_offset.min(text.len());while !text.is_char_boundary(position.byte_offset){position.byte_offset-=1;}}
                else{valid=false;}
            }
            if valid{self.selection=Some(selected);}else if let Some(p)=model::document_paragraphs(&self.document).first(){self.set_caret(p.id.clone(),0);}
        }
        self.commit("Native history reject",vec![Mutation::StylesChanged{before:styles_before,after:styles_after},Mutation::SectionsReplaced{index:0,before:sections_before,after:sections_after},Mutation::ReviewChanged{before:review_before,after:review_after}],selection,None)
    }
}
