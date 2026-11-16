//! Object edits address stable semantic IDs, not the caret's nearest neighbor.
//! Dragging changes an overlay only; release calls one of these transactions.
use super::*;
use model::{Equation, ImageAlignment, ImageWrap, Table};

fn inline_equation(content: &[Inline], id: &NodeId) -> Option<Equation> {
    for item in content { match item {
        Inline::Equation(eq) if &eq.id == id => return Some(eq.clone()),
        Inline::Hyperlink(link) => if let Some(eq)=inline_equation(&link.content,id) { return Some(eq); },
        _=>{}
    }} None
}
fn equation_in_blocks(blocks: &[Block], id: &NodeId) -> Option<Equation> {
    for block in blocks { match block {
        Block::Equation(eq) if &eq.id==id => return Some(eq.clone()),
        Block::Paragraph(p)=>if let Some(eq)=inline_equation(&p.content,id) { return Some(eq); },
        Block::Table(t)=>for r in &t.rows { for c in &r.cells { if let Some(eq)=equation_in_blocks(&c.blocks,id) {return Some(eq);} } },
        _=>{}
    }} None
}
fn update_inline_equation(content: &mut [Inline], id: &NodeId, replacement: &Equation) -> bool {
    for item in content { match item {
        Inline::Equation(eq) if &eq.id==id => { *eq=replacement.clone(); return true; },
        Inline::Hyperlink(link) => if update_inline_equation(&mut link.content,id,replacement) { return true; },
        _=>{}
    }} false
}
fn visit_blocks(blocks: &mut Vec<Block>, op: &mut impl FnMut(&mut Vec<Block>)->bool) -> bool {
    if op(blocks) { return true; }
    for b in blocks { if let Block::Table(t)=b { for r in &mut t.rows { for c in &mut r.cells {
        if visit_blocks(&mut c.blocks,op) { return true; }
    }}}} false
}
fn visit_section(section: &mut Section, op: &mut impl FnMut(&mut Vec<Block>)->bool)->bool {
    if visit_blocks(&mut section.blocks,op) {return true;}
    for hf in section.header_footers_mut() {
        if visit_blocks(&mut hf.blocks,op) {return true;}
    } false
}
fn table_in_blocks<'a>(blocks: &'a [Block], id: &NodeId)->Option<&'a Table> {
    for b in blocks { if let Block::Table(t)=b {
        if &t.id==id {return Some(t);}
        for row in &t.rows {for cell in &row.cells {if let Some(t)=table_in_blocks(&cell.blocks,id) {return Some(t);}}}
    }} None
}
fn resize_inline(content:&mut [Inline], id:&NodeId, width:f32,height:f32)->bool {
    for i in content {match i {
        Inline::Image(p) if &p.id==id=>{p.width_mm=width;p.height_mm=height;return true;},
        Inline::Hyperlink(h)=>if resize_inline(&mut h.content,id,width,height){return true;},_=>{}
    }}false
}
fn remove_inline(content:&mut Vec<Inline>,id:&NodeId)->bool {
    if let Some(index)=content.iter().position(|i| matches!(i,Inline::Image(p) if &p.id==id)) {content.remove(index);return true;}
    for i in content {if let Inline::Hyperlink(h)=i {if remove_inline(&mut h.content,id){return true;}}}false
}
impl EditorState {
    fn edit_object(&mut self,label:&'static str,operation:impl FnMut(&mut Vec<Block>)->bool)->Result<(),String> {
        self.edit_object_with(label,Vec::new(),operation)
    }
    /// `edit_object` that also records `extra` mutations (such as a new
    /// resource) in the same undo step, before the section change.
    pub(crate) fn edit_object_with(&mut self,label:&'static str,extra:Vec<Mutation>,operation:impl FnMut(&mut Vec<Block>)->bool)->Result<(),String> {
        self.edit_object_coalesced(label,extra,None,operation)
    }
    fn edit_object_coalesced(&mut self,label:&'static str,mut extra:Vec<Mutation>,coalesce:Option<CoalesceKey>,mut operation:impl FnMut(&mut Vec<Block>)->bool)->Result<(),String> {
        for index in 0..self.document.sections.len() {
            let before=self.document.sections[index].clone();let mut after=before.clone();
            if visit_section(&mut after,&mut operation) {
                if before==after && extra.is_empty() {return Ok(());}
                let selection=self.selection.clone();self.document.sections[index]=after.clone();
                extra.push(Mutation::SectionChanged{section_id:after.id.clone(),before,after});
                self.commit(label,extra,selection,coalesce)?;
                return Ok(());
            }
        } Err("The selected object is no longer in the document".into())
    }
    pub fn equation_by_id(&self,id:&NodeId)->Option<Equation> {
        for section in &self.document.sections {
            if let Some(eq)=equation_in_blocks(&section.blocks,id){return Some(eq);}
            for hf in section.header_footers() {
                if let Some(eq)=equation_in_blocks(&hf.stream.blocks,id){return Some(eq);}
            }
        }None
    }
    pub fn equation_at_caret(&self)->Option<Equation> {
        let pos=self.focus_position()?;let p=self.paragraph(&pos.node_id)?;
        fn find(items:&[Inline],offset:usize,start:&mut usize)->Option<Equation> {
            for i in items {
                if let Inline::Hyperlink(h)=i {if let Some(e)=find(&h.content,offset,start){return Some(e);}continue;}
                let len=flat_content_len(std::slice::from_ref(i));
                if let Inline::Equation(eq)=i {if offset>=*start&&offset<=*start+len{return Some(eq.clone());}}
                *start+=len;
            }None
        }find(&p.content,pos.byte_offset,&mut 0)
    }
    pub fn replace_equation(&mut self,id:&NodeId,mut replacement:Equation)->Result<(),String> {
        let existing=self.equation_by_id(id).ok_or("Equation not found")?;
        replacement.id=id.clone();replacement.display=existing.display;
        self.edit_object("Edit equation",|blocks|{
            for b in blocks {match b {
                Block::Equation(eq) if &eq.id==id=>{*eq=replacement.clone();return true;},
                Block::Paragraph(p)=>if update_inline_equation(&mut p.content,id,&replacement){return true;},_=>{}
            }}false
        })
    }
    /// Live edit from the equation pane: the equation, including its display,
    /// becomes `replacement`. Successive edits of the same equation merge into
    /// one undo step until `end_equation_edit`.
    pub fn update_equation_live(&mut self,id:&NodeId,mut replacement:Equation)->Result<(),String> {
        self.equation_by_id(id).ok_or("Equation not found")?;
        replacement.id=id.clone();
        self.edit_object_coalesced("Edit equation",Vec::new(),Some(CoalesceKey::Equation(id.clone())),|blocks|{
            for b in blocks {match b {
                Block::Equation(eq) if &eq.id==id=>{*eq=replacement.clone();return true;},
                Block::Paragraph(p)=>if update_inline_equation(&mut p.content,id,&replacement){return true;},_=>{}
            }}false
        })
    }
    /// Ends the undo step that live equation edits merge into.
    pub fn end_equation_edit(&mut self) { self.break_coalescing(); }
    pub fn resize_picture(&mut self,id:&NodeId,width_mm:f32,height_mm:f32)->Result<(),String> {
        if !width_mm.is_finite()||!height_mm.is_finite()||!(1.0..=1000.0).contains(&width_mm)||!(1.0..=1000.0).contains(&height_mm){return Err("Picture dimensions must be between 1 and 1000 mm".into());}
        self.edit_object("Resize picture",|blocks|{for b in blocks {match b {
            Block::Image(p) if &p.id==id=>{p.width_mm=width_mm;p.height_mm=height_mm;return true;},
            Block::Paragraph(p)=>if resize_inline(&mut p.content,id,width_mm,height_mm){return true;},_=>{}
        }}false})
    }

    /// Change only the crop geometry, keeping source pixels, edge crop and effects.
    pub fn set_picture_shape(&mut self, id: &NodeId, shape: model::ImageShape) -> Result<(), String> {
        if shape.preset.is_empty() || !shape.preset.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Err("Invalid picture shape".into());
        }
        fn inline(items: &mut [Inline], id: &NodeId, shape: &model::ImageShape) -> bool {
            for item in items {
                match item {
                    Inline::Image(image) if &image.id == id => { image.effects.shape = shape.clone(); return true; }
                    Inline::Hyperlink(link) => if inline(&mut link.content, id, shape) { return true; },
                    _ => {}
                }
            }
            false
        }
        self.edit_object("Crop picture to shape", |blocks| {
            for block in blocks {
                match block {
                    Block::Image(image) if &image.id == id => { image.effects.shape = shape.clone(); return true; }
                    Block::Paragraph(paragraph) => if inline(&mut paragraph.content, id, &shape) { return true; },
                    _ => {}
                }
            }
            false
        })
    }

    pub fn picture_shape(&self, id: &NodeId) -> Option<&model::ImageShape> {
        fn inline<'a>(items: &'a [Inline], id: &NodeId) -> Option<&'a model::ImageShape> {
            items.iter().find_map(|item| match item {
                Inline::Image(image) if &image.id == id => Some(&image.effects.shape),
                Inline::Hyperlink(link) => inline(&link.content, id), _ => None,
            })
        }
        fn blocks<'a>(items: &'a [Block], id: &NodeId) -> Option<&'a model::ImageShape> {
            items.iter().find_map(|block| match block {
                Block::Image(image) if &image.id == id => Some(&image.effects.shape),
                Block::Paragraph(paragraph) => inline(&paragraph.content, id),
                Block::Table(table) => table.rows.iter().flat_map(|row| &row.cells).find_map(|cell| blocks(&cell.blocks, id)),
                _ => None,
            })
        }
        self.document.sections.iter().find_map(|section| blocks(&section.blocks, id)
            .or_else(|| section.header_footers().find_map(|stream| blocks(&stream.stream.blocks, id))))
    }
    fn object_text_position(&self, id: &NodeId) -> Option<DocumentPosition> {
        fn find(items: &[Inline], id: &NodeId, offset: &mut usize) -> bool {
            for item in items {
                if matches!(item, Inline::Image(i) if &i.id == id) || matches!(item, Inline::Equation(e) if &e.id == id) { return true; }
                if let Inline::Hyperlink(link) = item { if find(&link.content, id, offset) { return true; } }
                else { *offset += flat_content_len(std::slice::from_ref(item)); }
            }
            false
        }
        for section in &self.document.sections {
            let mut paragraphs = Vec::new(); collect_paragraph_refs(&section.blocks, &mut paragraphs);
            for hf in section.header_footers() {
                collect_paragraph_refs(&hf.stream.blocks, &mut paragraphs);
            }
            for p in paragraphs { let mut offset = 0; if find(&p.content, id, &mut offset) { return Some(DocumentPosition::new(p.id.clone(), offset)); } }
        }
        None
    }
    fn finish_object_delete(&mut self, position: Option<DocumentPosition>) {
        if let Some(position) = position {
            self.set_caret(position.node_id, position.byte_offset);
            // edit_object has already made the single transaction. Its after-
            // selection must be the repaired caret, not a removed object range.
            if let Some(record) = self.undo.last_mut() { std::sync::Arc::make_mut(record).after_selection = self.selection.clone(); }
        }
    }
    pub fn delete_picture(&mut self, id: &NodeId) -> Result<(), String> {
        let position = self.object_text_position(id);
        self.edit_object("Delete picture", |blocks| {
            if let Some(i) = blocks.iter().position(|b| matches!(b, Block::Image(p) if &p.id == id)) { blocks.remove(i); return true; }
            for b in blocks { if let Block::Paragraph(p) = b { if remove_inline(&mut p.content, id) {
                p.content = (normalized_content(std::mem::take(&mut p.content))).into(); return true;
            } } }
            false
        })?;
        self.finish_object_delete(position); Ok(())
    }
    pub fn delete_equation(&mut self, id: &NodeId) -> Result<(), String> {
        fn remove(items: &mut Vec<Inline>, id: &NodeId) -> bool {
            if let Some(i) = items.iter().position(|item| matches!(item, Inline::Equation(e) if &e.id == id)) { items.remove(i); return true; }
            for item in items { if let Inline::Hyperlink(link) = item { if remove(&mut link.content, id) { return true; } } }
            false
        }
        let position = self.object_text_position(id);
        self.edit_object("Delete equation", |blocks| {
            if let Some(i) = blocks.iter().position(|b| matches!(b, Block::Equation(e) if &e.id == id)) { blocks.remove(i); return true; }
            for block in blocks { if let Block::Paragraph(p) = block { if remove(&mut p.content, id) { p.content=(normalized_content(std::mem::take(&mut p.content))).into(); return true; } } }
            false
        })?;
        self.finish_object_delete(position); Ok(())
    }
    /// Align block pictures directly, or an inline picture's owning paragraph.
    pub fn align_picture(&mut self,id:&NodeId,alignment:ImageAlignment)->Result<(),String> {
        fn has(items:&[Inline],id:&NodeId)->bool {items.iter().any(|i|match i {Inline::Image(p)=>&p.id==id,Inline::Hyperlink(h)=>has(&h.content,id),_=>false})}
        self.edit_object("Align picture",|blocks|{for b in blocks {match b {
            Block::Image(p) if &p.id==id=>{p.alignment=alignment;return true;},
            Block::Paragraph(p) if has(&p.content,id)=>{p.properties.alignment=Some(match alignment {ImageAlignment::Left=>model::ParagraphAlignment::Left,ImageAlignment::Center=>model::ParagraphAlignment::Center,ImageAlignment::Right=>model::ParagraphAlignment::Right});return true;},_=>{}
        }}false})
    }
    pub fn set_block_picture_wrap(&mut self,id:&NodeId,wrap:ImageWrap)->Result<(),String>{self.set_floating_wrap(id,wrap)}
    pub fn table_by_id(&self,id:&NodeId)->Option<&Table> {
        for s in &self.document.sections {
            if let Some(t)=table_in_blocks(&s.blocks,id){return Some(t);}
            for hf in s.header_footers(){if let Some(t)=table_in_blocks(&hf.stream.blocks,id){return Some(t);}}
        }None
    }
    pub fn select_cell(&mut self,table_id:&NodeId,row:usize,cell:usize)->Result<(),String> {
        let cell=self.table_by_id(table_id).and_then(|t|t.rows.get(row)).and_then(|r|r.cells.get(cell)).ok_or("Cell not found")?;
        let mut paragraphs=Vec::new();collect_paragraph_refs(&cell.blocks,&mut paragraphs);
        let id=paragraphs.first().map(|p|p.id.clone()).ok_or("Cell has no editable paragraph")?;
        self.set_caret(id,0);Ok(())
    }
    pub fn resize_table_grid(&mut self,id:&NodeId,widths_mm:Vec<f32>)->Result<(),String> {
        if widths_mm.is_empty()||widths_mm.len()>256||widths_mm.iter().any(|v|!v.is_finite()||!(2.0..=500.0).contains(v)){return Err("Invalid table column widths".into());}
        self.edit_object("Resize table columns",|blocks|{for b in blocks {if let Block::Table(t)=b {if &t.id==id {
            t.properties.preferred_width_mm=Some(widths_mm.iter().sum());t.properties.grid_column_widths_mm=widths_mm.clone();
            for r in &mut t.rows {for c in &mut r.cells {c.width_mm=None;}}return true;
        }}}false})
    }
    /// Resizes the whole table in one step: new grid column widths and row
    /// minimum heights (None keeps a row automatic).
    pub fn scale_table(&mut self,id:&NodeId,widths_mm:Vec<f32>,heights_mm:Vec<Option<f32>>)->Result<(),String> {
        if widths_mm.is_empty()||widths_mm.len()>256||widths_mm.iter().any(|v|!v.is_finite()||!(2.0..=500.0).contains(v)){return Err("Invalid table column widths".into());}
        if heights_mm.iter().flatten().any(|v|!v.is_finite()||!(2.0..=500.0).contains(v)){return Err("Invalid row height".into());}
        self.edit_object("Resize table",|blocks|{for b in blocks {if let Block::Table(t)=b {if &t.id==id {
            t.properties.preferred_width_mm=Some(widths_mm.iter().sum());t.properties.grid_column_widths_mm=widths_mm.clone();
            for r in &mut t.rows {for c in &mut r.cells {c.width_mm=None;}}
            for (row,height) in t.rows.iter_mut().zip(&heights_mm) {row.height_mm=*height;}
            return true;
        }}}false})
    }
    /// Moves a main-text table. `target` is a paragraph in the main text and
    /// whether the table goes before it; `left_indent_mm` places the table
    /// horizontally (left aligned). Either may be None to keep it.
    pub fn move_table(&mut self,id:&NodeId,target:Option<(NodeId,bool)>,left_indent_mm:Option<f32>)->Result<(),String>{
        if left_indent_mm.is_some_and(|v|!v.is_finite()||!(-200.0..=500.0).contains(&v)){return Err("Invalid table position".into());}
        self.relocate_table(id,target,"Move table",|p|{if let Some(indent)=left_indent_mm{p.left_indent_mm=Some((indent*10.0).round()/10.0);p.alignment=None;}})
    }
    /// Makes a table float at `position` (or back inline with None), in one
    /// step. A floating table is anchored to the paragraph after it, so
    /// `anchor` (a main-text paragraph) moves the table in front of it.
    pub fn set_table_position(&mut self,id:&NodeId,anchor:Option<NodeId>,position:Option<model::TablePosition>)->Result<(),String>{
        if let Some(p)=&position{p.validate()?;}
        let label=if position.is_none(){"Table in line with text"}else if anchor.is_some(){"Move floating table"}else{"Table position"};
        self.relocate_table(id,anchor.map(|a|(a,true)),label,|p|{p.position=position;})
    }
    fn relocate_table(&mut self,id:&NodeId,target:Option<(NodeId,bool)>,label:&'static str,change:impl FnOnce(&mut model::TableProperties))->Result<(),String>{
        let (si,bi)=self.document.sections.iter().enumerate().find_map(|(si,s)|s.blocks.iter().position(|b|matches!(b,Block::Table(t) if &t.id==id)).map(|bi|(si,bi)))
            .ok_or("Only tables in the main text can be moved.")?;
        let destination=match &target {
            Some((paragraph,before))=>{
                let (ti,tb)=self.locate_paragraph(paragraph).ok_or("Drop the table between paragraphs of the main text.")?;
                Some((ti,tb+usize::from(!*before)))
            }
            None=>None,
        };
        let selection=self.selection.clone();
        let sections:Vec<usize>=match destination{Some((ti,_)) if ti!=si=>vec![si,ti],_=>vec![si]};
        let before:Vec<_>=sections.iter().map(|&i|self.document.sections[i].clone()).collect();
        {
            let Block::Table(table)=&mut self.document.sections[si].blocks[bi] else{return Err("table not found".into());};
            change(&mut table.properties);
        }
        if let Some((ti,mut index))=destination {
            if !(ti==si&&(index==bi||index==bi+1)) {
                let block=self.document.sections[si].blocks.remove(bi);
                if ti==si&&index>bi {index-=1;}
                self.document.sections[ti].blocks.insert(index,block);
            }
        }
        let mutations:Vec<_>=sections.iter().zip(before).filter_map(|(&i,before)|{
            let after=self.document.sections[i].clone();
            (before!=after).then(||Mutation::SectionChanged{section_id:after.id.clone(),before,after})
        }).collect();
        if mutations.is_empty(){return Ok(());}
        self.commit(label,mutations,selection,None)?;
        Ok(())
    }
    pub fn resize_table_row(&mut self,id:&NodeId,row:usize,height_mm:f32)->Result<(),String> {
        if !height_mm.is_finite()||!(2.0..=500.0).contains(&height_mm){return Err("Invalid row height".into());}
        if self.table_by_id(id).and_then(|t|t.rows.get(row)).is_none(){return Err("Table row not found".into());}
        self.edit_object("Resize table row",|blocks|{for b in blocks {if let Block::Table(t)=b {if &t.id==id {t.rows[row].height_mm=Some(height_mm);return true;}}}false})
    }
    pub fn clear_cells(&mut self, id: &NodeId, cells: &[NodeId]) -> Result<(), String> {
        // Preserve each cell's first paragraph identity and repair a caret in a
        // later paragraph. Otherwise Delete followed by typing targets a node
        // that no longer exists, even though the cell itself is still selected.
        let focus = self.focus_position();
        let mut caret = None;
        let table = self.table_by_id(id).ok_or("Table not found")?;
        for row in &table.rows { for cell in &row.cells {
            if !cells.contains(&cell.id) { continue; }
            let mut paragraphs = Vec::new(); collect_paragraph_refs(&cell.blocks, &mut paragraphs);
            if focus.as_ref().is_some_and(|position| paragraphs.iter().any(|p| p.id == position.node_id)) {
                caret = paragraphs.first().map(|p| DocumentPosition::new(p.id.clone(), 0));
            }
        } }
        let revision = self.revision;
        self.edit_object("Clear table cells", |blocks| {
            for block in blocks { if let Block::Table(table) = block { if &table.id == id {
                for row in &mut table.rows { for cell in &mut row.cells {
                    if cells.contains(&cell.id) {
                        let mut replacement = Paragraph::plain("");
                        let mut paragraphs = Vec::new(); collect_paragraph_refs(&cell.blocks, &mut paragraphs);
                        if let Some(first) = paragraphs.first() { replacement.id = first.id.clone(); }
                        cell.blocks = (vec![Block::Paragraph(replacement)]).into();
                    }
                } }
                return true;
            } } }
            false
        })?;
        if self.revision != revision { self.finish_object_delete(caret); }
        Ok(())
    }
}

impl EditorState {
    /// Relocate an inline picture to a semantic insertion point. The original
    /// ID, resource, dimensions and alternative text survive; drop is one undo.
    pub fn move_inline_picture(&mut self, id: &NodeId, target: DocumentPosition) -> Result<(), String> {
        fn locate(items: &[Inline], id: &NodeId, base: usize) -> Option<(usize, model::InlineImage)> {
            let mut offset = base;
            for item in items {
                match item {
                    Inline::Image(image) if &image.id == id => return Some((offset, image.clone())),
                    Inline::Hyperlink(link) => if let Some(found) = locate(&link.content, id, offset) { return Some(found); },
                    _ => {}
                }
                offset += flat_content_len(std::slice::from_ref(item));
            }
            None
        }
        let mut paragraphs = Vec::new();
        for section in &self.document.sections {
            collect_paragraph_refs(&section.blocks, &mut paragraphs);
            for hf in section.header_footers() {
                collect_paragraph_refs(&hf.stream.blocks, &mut paragraphs);
            }
        }
        let (source, at, image) = paragraphs.iter().find_map(|p| locate(&p.content, id, 0).map(|(at, image)| (p.id.clone(), at, image)))
            .ok_or("Only inline pictures can be moved into text. Use alignment controls for a block picture.")?;
        if source == target.node_id && (at..=at + INLINE_OBJECT_MARKER.len()).contains(&target.byte_offset) { return Ok(()); }
        let mut staged = self.clone();
        staged.delete_picture(id)?;
        let mut paragraph = staged.paragraph(&target.node_id).cloned().ok_or("The drop target no longer exists")?;
        let offset = if source == target.node_id && target.byte_offset > at { target.byte_offset - INLINE_OBJECT_MARKER.len() } else { target.byte_offset };
        let (mut left, right) = split_content(&paragraph.content, offset)?;
        left.push(Inline::Image(image)); left.extend(right); paragraph.content = (normalized_content(left)).into();
        staged.replace_paragraph(&paragraph)?;
        let before = self.document.sections.clone(); let after = staged.document.sections;
        if before == after { return Ok(()); }
        let selection = self.selection.clone(); self.document.sections = after.clone();
        self.selection = Some(Selection::collapsed(DocumentPosition::new(target.node_id, offset + INLINE_OBJECT_MARKER.len())));
        self.commit("Move picture", vec![Mutation::SectionsReplaced { index: 0, before, after }], selection, None)?;
        Ok(())
    }
}

#[cfg(test)] #[path = "../tests/unit/objects.rs"] mod tests;

