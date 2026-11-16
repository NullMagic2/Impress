//! Floating-picture operations are one semantic transaction and preserve IDs.
//! This implementation deliberately scopes anchors to top-level body paragraphs.
use super::*;
use model::{ImageAnchor,ImageWrap,AnchorReference,BlockImage,ImageAlignment};
impl EditorState{
    /// Paragraph anchors follow explicit joins/splices, never substring guesses.
    /// Deleting an anchor through an unknown structural operation is rejected.
    pub(super) fn synchronize_picture_anchors(&mut self,changes:&mut Vec<Mutation>)->Result<(),String>{
        let mut updates=Vec::new();
        for section in &self.document.sections{for block in &section.blocks{if let Block::Image(image)=block{if let Some(anchor)=&image.anchor{
            if image.wrap==ImageWrap::Inline{continue;}
            let mut target=anchor.paragraph.clone();
            for change in changes.iter(){if let Mutation::AnchorsTransformed{mapping,..}=change{match mapping{
                anchor_maps::AnchorMapping::Join{left,right,..} if &target==right=>target=left.clone(),
                anchor_maps::AnchorMapping::Splice{start,end,interior} if target==end.paragraph||interior.contains(&target)=>target=start.paragraph.clone(),_=>{}
            }}}
            let owner=self.document.sections.iter().find(|s|s.blocks.iter().any(|b|matches!(b,Block::Paragraph(p)if p.id==target))).ok_or("This edit removes a floating picture's anchor. Move the anchor or remove the picture first.")?;
            if owner.properties.column_count!=1{return Err("Move floating pictures to a one-column section before changing columns".into());}
            if target!=anchor.paragraph||owner.id!=section.id{updates.push((image.id.clone(),target));}
        }}}}
        if updates.is_empty(){return Ok(());}
        let before=self.document.sections.clone();let mut after=before.clone();
        for (id,target) in updates{
            let mut moved=None;for section in &mut after{if let Some(at)=section.blocks.iter().position(|b|matches!(b,Block::Image(i)if i.id==id)){if let Block::Image(mut image)=section.blocks.remove(at){image.anchor.as_mut().unwrap().paragraph=target.clone();moved=Some(image);}break;}}
            let image=moved.ok_or("Floating picture disappeared during anchor rebasing")?;
            let section=after.iter_mut().find(|s|s.blocks.iter().any(|b|matches!(b,Block::Paragraph(p)if p.id==target))).ok_or("Floating anchor destination disappeared")?;
            let at=section.blocks.iter().position(|b|matches!(b,Block::Paragraph(p)if p.id==target)).unwrap();section.blocks.insert(at,Block::Image(image));
        }
        self.document.sections=after.clone();changes.push(Mutation::SectionsReplaced{index:0,before,after});Ok(())
    }
    pub fn picture_anchor(&self,id:&NodeId)->Option<&ImageAnchor>{
        self.document.sections.iter().flat_map(|s|&s.blocks).find_map(|b|match b{Block::Image(i)if &i.id==id=>i.anchor.as_ref(),_=>None})
    }
    pub fn picture_wrap(&self,id:&NodeId)->ImageWrap{
        self.document.sections.iter().flat_map(|s|&s.blocks).find_map(|b|match b{Block::Image(i)if &i.id==id=>Some(i.wrap),_=>None}).unwrap_or(ImageWrap::Inline)
    }
    pub fn picture_body_paragraph(&self,id:&NodeId)->Option<NodeId>{
        fn has(items:&[Inline],id:&NodeId)->bool{items.iter().any(|i|match i{Inline::Image(p)=>&p.id==id,Inline::Hyperlink(h)=>has(&h.content,id),_=>false})}
        let found=self.document.sections.iter().flat_map(|s|&s.blocks).find_map(|b|match b{Block::Paragraph(p)if has(&p.content,id)=>Some(p.id.clone()),Block::Image(p)if &p.id==id=>p.anchor.as_ref().map(|a|a.paragraph.clone()),_=>None});
        // A picture standing on its own between paragraphs (as imported) goes
        // with the paragraph after it, or before it at the end of a section.
        found.or_else(||self.document.sections.iter().find_map(|s|{
            let at=s.blocks.iter().position(|b|matches!(b,Block::Image(p)if &p.id==id))?;
            let paragraph=|b:&Block|match b{Block::Paragraph(p)=>Some(p.id.clone()),_=>None};
            s.blocks[at+1..].iter().find_map(paragraph).or_else(||s.blocks[..at].iter().rev().find_map(paragraph))
        }))
    }
    /// Width and height (mm) of a top-level body picture, inline or floating.
    pub fn picture_size(&self,id:&NodeId)->Option<(f32,f32)>{
        fn inline(items:&[Inline],id:&NodeId)->Option<(f32,f32)>{items.iter().find_map(|i|match i{Inline::Image(p)if &p.id==id=>Some((p.width_mm,p.height_mm)),Inline::Hyperlink(h)=>inline(&h.content,id),_=>None})}
        self.document.sections.iter().flat_map(|s|s.blocks.iter()).find_map(|b|match b{Block::Image(i)if &i.id==id=>Some((i.width_mm,i.height_mm)),Block::Paragraph(p)=>inline(&p.content,id),_=>None})
    }
    /// Page setup of the section holding a picture (or its anchor paragraph).
    pub fn picture_section(&self,id:&NodeId)->Option<&model::SectionProperties>{
        let paragraph=self.picture_body_paragraph(id)?;
        self.document.sections.iter().find(|s|s.blocks.iter().any(|b|matches!(b,Block::Paragraph(p)if p.id==paragraph))).map(|s|&s.properties)
    }
    /// The Layout dialog's OK: size, wrapping and position in one undo step.
    /// `anchor` is needed unless the picture goes in line with the text.
    pub fn lay_out_picture(&mut self,id:&NodeId,wrap:ImageWrap,anchor:Option<ImageAnchor>,size:Option<(f32,f32)>)->Result<(),String>{
        let depth=self.undo.len();
        let result=(||{
            if let Some((width,height))=size.filter(|s|Some(*s)!=self.picture_size(id)){self.resize_picture(id,width,height)?;}
            if wrap==ImageWrap::Inline{self.make_picture_inline(id)}
            else{self.position_picture(id,anchor.ok_or("The picture needs a position on the page")?,wrap)}
        })();
        if result.is_err(){while self.undo.len()>depth{self.undo();}return result;}
        self.group_undo_since(depth,"Picture layout");
        Ok(())
    }
    pub fn position_picture(&mut self,id:&NodeId,anchor:ImageAnchor,wrap:ImageWrap)->Result<(),String>{
        anchor.validate()?;
        if wrap==ImageWrap::Inline{return self.make_picture_inline(id);}
        if self.document.review.track_text{return Err("Disable net paragraph-text tracking before floating a picture; native transaction history supports this edit".into());}
        if self.picture_anchor(id).is_some_and(|old|old.locked&&old.paragraph!=anchor.paragraph){return Err("Unlock this picture's anchor before attaching it to another paragraph".into());}
        let target=self.document.sections.iter().position(|s|s.blocks.iter().any(|b|matches!(b,Block::Paragraph(p)if p.id==anchor.paragraph))).ok_or("A floating picture needs a top-level body paragraph anchor")?;
        if self.document.sections[target].properties.column_count!=1{return Err("Floating picture layout currently requires a one-column section".into());}
        let selection=self.selection.clone();let before=self.document.sections.clone();let mut sections=before.clone();let mut found=None;let mut offset_map=None;
        fn remove(items:&mut Vec<Inline>,id:&NodeId,offset:&mut usize)->Option<model::InlineImage>{
            let mut i=0;while i<items.len(){
                if matches!(&items[i],Inline::Image(p)if &p.id==id){if let Inline::Image(p)=items.remove(i){return Some(p);}}
                if let Inline::Hyperlink(h)=&mut items[i]{if let Some(p)=remove(&mut h.content,id,offset){return Some(p);}}
                else{*offset+=flat_content_len(std::slice::from_ref(&items[i]));}i+=1;
            }None
        }
        for s in &mut sections{
            if let Some(i)=s.blocks.iter().position(|b|matches!(b,Block::Image(p)if &p.id==id)){if let Block::Image(p)=s.blocks.remove(i){found=Some(p);}break;}
            for block in &mut s.blocks{if let Block::Paragraph(p)=block{let mut offset=0;if let Some(image)=remove(&mut p.content,id,&mut offset){
                offset_map=Some(anchor_maps::AnchorMapping::Replace{paragraph:p.id.clone(),start:offset,end:offset+3,inserted:0});
                found=Some(BlockImage{id:image.id,resource:image.resource,width_mm:image.width_mm,height_mm:image.height_mm,alt_text:image.alt_text,effects:image.effects,wrap,alignment:ImageAlignment::Left,anchor:None});break;
            }}}
            if found.is_some(){break;}
        }
        let mut image=found.ok_or("Only top-level body pictures can float; pictures inside tables, headers or notes stay inline")?;
        image.wrap=wrap;image.anchor=Some(anchor.clone());
        let at=sections[target].blocks.iter().position(|b|matches!(b,Block::Paragraph(p)if p.id==anchor.paragraph)).ok_or("Picture anchor disappeared")?;
        sections[target].blocks.insert(at,Block::Image(image));
        self.document.sections=sections.clone();let mut changes=Vec::new();
        if let Some(mapping)=offset_map{changes.push(Mutation::AnchorsTransformed{mapping,suppress:1});}
        changes.push(Mutation::SectionsReplaced{index:0,before,after:sections});
        self.set_caret(anchor.paragraph,0);self.commit("Position floating picture",changes,selection,None)
    }
    pub fn move_floating_picture(&mut self,id:&NodeId,dx_mm:f32,dy_mm:f32)->Result<(),String>{
        let mut anchor=self.picture_anchor(id).cloned().ok_or("Picture is not floating")?;anchor.x_mm+=dx_mm;anchor.y_mm+=dy_mm;
        self.position_picture(id,anchor,self.picture_wrap(id))
    }
    pub fn make_picture_inline(&mut self,id:&NodeId)->Result<(),String>{
        let Some(anchor)=self.picture_anchor(id).cloned()else{return Ok(());};
        let selection=self.selection.clone();let before=self.document.sections.clone();let mut sections=before.clone();let mut image=None;
        for s in &mut sections{if let Some(at)=s.blocks.iter().position(|b|matches!(b,Block::Image(i)if &i.id==id)){if let Block::Image(i)=s.blocks.remove(at){image=Some(i);}break;}}
        let image=image.ok_or("Picture disappeared")?;let target=sections.iter_mut().flat_map(|s|&mut s.blocks).find_map(|b|match b{Block::Paragraph(p)if p.id==anchor.paragraph=>Some(p),_=>None}).ok_or("Anchor paragraph no longer exists; attach the picture to a surviving body paragraph first")?;
        target.content.insert(0,Inline::Image(model::InlineImage{id:image.id,resource:image.resource,width_mm:image.width_mm,height_mm:image.height_mm,alt_text:image.alt_text,effects:image.effects}));
        self.document.sections=sections.clone();self.set_caret(anchor.paragraph.clone(),3);
        self.commit("Make picture inline",vec![Mutation::AnchorsTransformed{mapping:anchor_maps::AnchorMapping::Replace{paragraph:anchor.paragraph,start:0,end:0,inserted:3},suppress:1},Mutation::SectionsReplaced{index:0,before,after:sections}],selection,None)
    }
    pub fn set_floating_wrap(&mut self,id:&NodeId,wrap:ImageWrap)->Result<(),String>{
        if wrap==ImageWrap::Inline{return self.make_picture_inline(id);}
        let anchor=self.picture_anchor(id).cloned().or_else(||self.picture_body_paragraph(id).map(|paragraph|ImageAnchor{paragraph,reference:AnchorReference::Paragraph,x_mm:0.0,y_mm:0.0,gap_mm:3.0,locked:false})).ok_or("Select an inline picture in a top-level body paragraph")?;
        self.position_picture(id,anchor,wrap)
    }
}
