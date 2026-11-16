//! Footnote paragraphs use normal inline editing without flattening imported
//! formatting. Each structural note change remains one reversible transaction.
use super::*;
use std::collections::BTreeMap;
impl EditorState {
    pub fn note_for_paragraph(&self,id:&NodeId)->Option<NodeId>{self.document.review.footnotes.values().find(|n|find_paragraph_in_blocks(&n.blocks,id).is_some()).map(|n|n.id.clone())}
    pub fn enter_footnote(&mut self,id:&NodeId)->Result<(),String>{let note=self.document.review.footnotes.get(id).ok_or("Footnote not found")?;let mut ps=Vec::new();collect_paragraph_refs(&note.blocks,&mut ps);let p=ps.first().ok_or("This note has no editable paragraph")?.id.clone();self.set_caret(p,0);Ok(())}
    pub fn leave_footnote(&mut self)->Result<(),String>{let pos=self.focus_position().ok_or("No active caret")?;let note=self.note_for_paragraph(&pos.node_id).ok_or("The caret is not in a footnote")?;let(a,_)=model::footnote_references(&self.document).into_iter().find(|(_,id)|id==&note).ok_or("Footnote has no body reference")?;self.set_caret(a.paragraph,a.offset);Ok(())}
    pub(super) fn split_note_paragraph(&mut self)->Result<(),String>{
        let mut staged=self.clone();let selection=staged.selection.clone();let mut changes=Vec::new();staged.delete_selection_into(&mut changes)?;
        let pos=staged.focus_position().ok_or("No caret")?;let id=staged.note_for_paragraph(&pos.node_id).ok_or("Footnote stream not found")?;
        let before=staged.document.review.footnotes[&id].clone();let mut after=before.clone();let p=staged.paragraph(&pos.node_id).cloned().ok_or("Note paragraph not found")?;
        let(head,tail)=staged.split_paragraph_pair(&p,pos.byte_offset)?;
        if !replace_paragraph_in_blocks(&mut after.blocks,&head)||!insert_paragraph_after_in_blocks(&mut after.blocks,&head.id,tail.clone()){return Err("Note paragraph container not found".into());}
        changes.push(Mutation::AnchorsTransformed{mapping:anchor_maps::AnchorMapping::Split{left:head.id,right:tail.id.clone(),at:pos.byte_offset},suppress:1});
        staged.document.review.footnotes.insert(id.clone(),after.clone());changes.push(Mutation::FootnoteChanged{id,before,after});staged.set_caret(tail.id,0);staged.commit("New note paragraph",changes,selection,None)?;*self=staged;Ok(())
    }
    pub(super) fn merge_note_paragraph(&mut self,current:NodeId,direction:isize)->Result<(),String>{
        let selection=self.selection.clone();let id=self.note_for_paragraph(&current).ok_or("Note not found")?;let before=self.document.review.footnotes[&id].clone();let mut after=before.clone();
        let Some((left,at))=merge_adjacent_in_blocks(&mut after.blocks,&current,direction)else{return Ok(());};
        let mut old=Vec::new();let mut new=Vec::new();collect_paragraph_refs(&before.blocks,&mut old);collect_paragraph_refs(&after.blocks,&mut new);let right=old.iter().find(|p|!new.iter().any(|q|q.id==p.id)).ok_or("Missing merged note paragraph")?.id.clone();
        self.document.review.footnotes.insert(id.clone(),after.clone());self.set_caret(left.clone(),at);
        self.commit("Merge note paragraphs",vec![Mutation::AnchorsTransformed{mapping:anchor_maps::AnchorMapping::Join{left,right,at},suppress:1},Mutation::FootnoteChanged{id,before,after}],selection,None)
    }
    pub(super) fn delete_note_range(&mut self,start:&DocumentPosition,end:&DocumentPosition,changes:&mut Vec<Mutation>)->Result<(),String>{
        let id=self.note_for_paragraph(&start.node_id).ok_or("Selection starts outside the note")?;if self.note_for_paragraph(&end.node_id).as_ref()!=Some(&id){return Err("Body, notes, and independent cells are separate editing streams".into());}
        let before=self.document.review.footnotes[&id].clone();let mut after=before.clone();let mapping=anchor_maps::AnchorMapping::splice(&self.document,start,end);
        let left=delete_range_in_same_block_list(&mut after.blocks,start,end)?.ok_or("Select within one note text stream")?;
        self.document.review.footnotes.insert(id.clone(),after.clone());changes.push(Mutation::AnchorsTransformed{mapping,suppress:1});changes.push(Mutation::FootnoteChanged{id,before,after});self.set_caret(left,start.byte_offset);Ok(())
    }
    pub(super) fn paste_note_fragment(&mut self,source:&ClipboardFragment)->Result<(),String>{
        let mut staged=self.clone();let selection=staged.selection.clone();let(f,mut changes)=staged.import_clipboard_dependencies(source)?;staged.delete_selection_into(&mut changes)?;
        let pos=staged.focus_position().ok_or("No caret")?;let id=staged.note_for_paragraph(&pos.node_id).ok_or("Note not found")?;let before=staged.document.review.footnotes[&id].clone();let mut after=before.clone();let p=staged.paragraph(&pos.node_id).cloned().ok_or("Note paragraph not found")?;
        let (left,right)=split_content(&p.content,pos.byte_offset)?;let mut places=BTreeMap::new();let fresh=f.paragraphs.iter().map(fresh_clipboard_paragraph).collect::<Vec<_>>();let mut previous=p.id.clone();let mut end=pos.byte_offset;
        let mut mappings=Vec::new();
        for(index,part)in fresh.iter().enumerate(){let last=index+1==fresh.len();let mut para=if index==0{p.clone()}else{Paragraph{id:NodeId::new(),style:part.style.clone(),properties:part.properties.clone(),content:(Vec::new()).into()}};let mut content=if index==0{left.clone()}else{Vec::new()};content.extend(part.content.clone());end=flat_content_len(&content);if last{content.extend(right.clone());}para.content=(normalized_content(content)).into();
            if let Some(origin)=&part.origin{places.insert(origin.clone(),(para.id.clone(),if index==0{pos.byte_offset}else{0}));}
            if index==0{replace_paragraph_in_blocks(&mut after.blocks,&para);}else if !insert_paragraph_after_in_blocks(&mut after.blocks,&previous,para.clone()){return Err("Note paragraph container not found".into());}
            if last&&index>0{mappings.push(anchor_maps::AnchorMapping::Split{left:p.id.clone(),right:para.id.clone(),at:pos.byte_offset});mappings.push(anchor_maps::AnchorMapping::Offset{paragraph:para.id.clone(),at:0,delta:end});}
            previous=para.id;
        }
        if fresh.len()==1{let new=find_paragraph_in_blocks(&after.blocks,&p.id).ok_or("Note paragraph missing")?.clone();staged.replace_paragraph(&new)?;changes.push(Mutation::AnchorsTransformed{mapping:anchor_maps::AnchorMapping::Replace{paragraph:p.id.clone(),start:pos.byte_offset,end:pos.byte_offset,inserted:flat_content_len(&fresh[0].content)},suppress:1});changes.push(Mutation::ParagraphChanged{id:p.id.clone(),before:p,after:new});}
        else{for mapping in mappings{changes.push(Mutation::AnchorsTransformed{mapping,suppress:1});}staged.document.review.footnotes.insert(id.clone(),after.clone());changes.push(Mutation::FootnoteChanged{id,before,after});}
        staged.set_caret(previous,end);staged.finish_clipboard_references(&f,&places,&mut changes,&selection)?;staged.commit("Paste",changes,selection,None)?;*self=staged;Ok(())
    }
}
/// Word-style note navigation: reference marks and their notes.
impl EditorState {
    /// The note whose reference mark touches `offset` in `paragraph`: the mark
    /// just after or just before that caret position.
    pub fn note_reference_at(&self, paragraph: &NodeId, offset: usize) -> Option<NodeId> {
        let mark = INLINE_OBJECT_MARKER.len();
        model::footnote_references(&self.document).into_iter()
            .find(|(anchor, _)| &anchor.paragraph == paragraph && (anchor.offset == offset || anchor.offset + mark == offset))
            .map(|(_, note)| note)
    }

    /// A note's text as plain lines, for a tooltip.
    pub fn note_text(&self, id: &NodeId) -> Option<String> {
        let note = self.document.review.footnotes.get(id)?;
        let mut paragraphs = Vec::new();
        model::visit_paragraphs(&note.blocks, &mut paragraphs);
        Some(paragraphs.iter().map(|p| model::inline_text(&p.content)).collect::<Vec<_>>().join("\n"))
    }

    /// Word's Next / Previous Footnote. In the body the caret moves to the
    /// next (or previous) reference mark; inside a note it moves to the next
    /// (or previous) note's text.
    pub fn go_to_adjacent_note(&mut self, forward: bool) -> Result<(), String> {
        let position = self.focus_position().ok_or("No active caret")?;
        let references = model::footnote_references(&self.document);
        if references.is_empty() { return Err("There are no footnotes".into()); }
        let order: BTreeMap<NodeId, usize> = model::document_paragraphs(&self.document).iter().enumerate()
            .map(|(index, paragraph)| (paragraph.id.clone(), index)).collect();
        let in_note = self.note_for_paragraph(&position.node_id);
        // Where we are, in reading order: the caret, or the reference of the note holding it.
        let here = match &in_note {
            Some(note) => references.iter().find(|(_, id)| id == note).map(|(anchor, _)| (order.get(&anchor.paragraph).copied(), anchor.offset)),
            None => Some((order.get(&position.node_id).copied(), position.byte_offset)),
        }.and_then(|(index, offset)| index.map(|index| (index, offset))).ok_or("The caret is not in the document body")?;
        let key = |anchor: &model::TextAnchor| (order.get(&anchor.paragraph).copied().unwrap_or(usize::MAX), anchor.offset);
        let target = if forward {
            references.iter().find(|(anchor, _)| key(anchor) > here)
        } else {
            references.iter().rev().find(|(anchor, _)| key(anchor) < here)
        }.cloned().ok_or(if forward { "This is the last footnote" } else { "This is the first footnote" })?;
        let (anchor, note) = target;
        if in_note.is_some() { self.enter_footnote(&note) } else { self.set_caret(anchor.paragraph, anchor.offset); Ok(()) }
    }

    /// Word's Show Notes: from a note back to its reference; from a reference
    /// mark (or anywhere in the body) to the nearest following note.
    pub fn toggle_note_view(&mut self) -> Result<(), String> {
        let position = self.focus_position().ok_or("No active caret")?;
        if self.note_for_paragraph(&position.node_id).is_some() { return self.leave_footnote(); }
        if let Some(note) = self.note_reference_at(&position.node_id, position.byte_offset) { return self.enter_footnote(&note); }
        let caret = self.selection.clone();
        match self.go_to_adjacent_note(true) {
            Ok(()) => {
                let position = self.focus_position().ok_or("No active caret")?;
                let note = self.note_reference_at(&position.node_id, position.byte_offset).ok_or("Footnote not found")?;
                self.enter_footnote(&note)
            }
            Err(_) => {
                self.selection = caret;
                let first = model::footnote_references(&self.document).into_iter().next().ok_or("There are no footnotes")?;
                self.enter_footnote(&first.1)
            }
        }
    }
}


#[cfg(test)] #[path = "../tests/unit/note_editing.rs"] mod tests;
