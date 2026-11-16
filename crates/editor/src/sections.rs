//! Removing a section break keeps the following section's layout and header/
//! footer settings, but retains the surviving section's identity and incoming
//! break. Undo restores every removed section and its original identifiers.
use super::*;
impl EditorState {
    pub(super) fn delete_across_sections(&mut self, first: usize, first_block: usize, last: usize, last_block: usize,
        start: &DocumentPosition, end: &DocumentPosition, mutations: &mut Vec<Mutation>) -> Result<(), String> {
        if first >= last || last >= self.document.sections.len() { return Err("Invalid cross-section selection".into()); }
        let before = self.document.sections[first..=last].to_vec();
        let Block::Paragraph(head) = &before[0].blocks[first_block] else { return Err("Selection must start in body text".into()); };
        let tail_section = before.last().ok_or("Missing section")?;
        let Block::Paragraph(tail) = &tail_section.blocks[last_block] else { return Err("Selection must end in body text".into()); };
        // All fallible slicing is done before changing the live document.
        let (mut left, _) = split_content(&head.content, start.byte_offset)?;
        let (_, right) = split_content(&tail.content, end.byte_offset)?;
        left.extend(right);
        let mut paragraph = head.clone(); paragraph.content = (normalized_content(left)).into();
        let mut merged = before[0].clone();
        let incoming = merged.properties.start_on;
        merged.properties = tail_section.properties.clone(); merged.properties.start_on = incoming;
        merged.headers = tail_section.headers.clone(); merged.footers = tail_section.footers.clone();
        merged.blocks.truncate(first_block);
        merged.blocks.push(Block::Paragraph(paragraph));
        merged.blocks.extend(tail_section.blocks[last_block + 1..].iter().cloned());
        let after = vec![merged];
        let mapping=anchor_maps::AnchorMapping::splice(&self.document,start,end);
        self.document.sections.splice(first..=last, after.iter().cloned());
        mutations.push(Mutation::AnchorsTransformed { mapping, suppress: 1 });
        mutations.push(Mutation::SectionsReplaced { index: first, before, after });
        self.selection = Some(Selection::collapsed(start.clone()));
        Ok(())
    }

    pub(super) fn join_section_boundary(&mut self, first: usize, label: &'static str) -> Result<(), String> {
        if first + 1 >= self.document.sections.len() { return Ok(()); }
        let before_selection = self.selection.clone();
        let before = self.document.sections[first..=first + 1].to_vec();
        let mut merged = before[0].clone();
        let incoming = merged.properties.start_on;
        merged.properties = before[1].properties.clone(); merged.properties.start_on = incoming;
        merged.headers = before[1].headers.clone(); merged.footers = before[1].footers.clone();
        let mut tail = before[1].blocks.clone();
        let mut caret = self.focus_position();
        let mut mappings=Vec::new();
        if let (Some(Block::Paragraph(left)), Some(Block::Paragraph(right))) = (merged.blocks.last_mut(), tail.first()) {
            let at=paragraph_flat_text(left).len();
            mappings.push(Mutation::AnchorsTransformed { mapping: anchor_maps::AnchorMapping::Join {left:left.id.clone(),right:right.id.clone(),at},suppress:1 });
            caret = Some(DocumentPosition::new(left.id.clone(), at));
            left.content.extend(right.content.iter().cloned());
            left.content = (normalized_content(std::mem::take(&mut left.content))).into();
            tail.remove(0);
        }
        merged.blocks.extend(tail);
        let after = vec![merged];
        self.document.sections.splice(first..=first + 1, after.iter().cloned());
        self.selection = caret.map(Selection::collapsed);
        mappings.push(Mutation::SectionsReplaced {index:first,before,after});
        self.commit(label, mappings, before_selection, None)?;
        Ok(())
    }
}

