//! Plain-text newlines are paragraph boundaries. Explicit Inline::LineBreak
//! objects remain soft breaks. Older versions stored pasted prose in one run;
//! repair that structure on opening without changing run formatting or styles.
use super::*;
use anchor_maps::AnchorMapping;

impl EditorState {
    pub(super) fn insert_multiline_text(&mut self, text: &str) -> Result<(), String> {
        let position=self.focus_position().ok_or("no active caret")?;
        let paragraph=self.paragraph(&position.node_id).ok_or("caret paragraph not found")?;
        let normalized=text.replace("\r\n","\n").replace('\r',"\n");
        let fragment=ClipboardFragment {
            paragraphs: normalized.split('\n').map(|part| {
                let mut run=TextRun::plain(part);
                run.properties=self.typing_properties.clone();run.style=self.typing_style.clone();
                ClipboardParagraph {origin:None,style:paragraph.style.clone(),
                    properties:paragraph.properties.clone(),content:vec![Inline::Text(run)]}
            }).collect(),
            plain_text:normalized,
            ..Default::default()
        };
        // Reuse the semantic multi-paragraph paste transaction: this also
        // preserves suffix text, nested containers, references and one Undo.
        self.paste_fragment(&fragment)
    }
}

pub(super) fn normalize_document(document:&mut Document) {
    let mut mappings=Vec::new();
    for section in &mut document.sections {
        blocks(&mut section.blocks,&mut mappings);
        for stream in section.header_footers_mut() {blocks(&mut stream.blocks,&mut mappings);}
    }
    for note in document.review.footnotes.values_mut() {blocks(&mut note.blocks,&mut mappings);}
    for mapping in mappings {
        mapping.apply(&mut document.review);
        for control in document.content_controls.values_mut() {mapping.control_range(&mut control.range);}
    }
}

fn blocks(items:&mut model::Shared<Vec<Block>>, mappings:&mut Vec<AnchorMapping>) {
    let mut index=0;
    while index<items.len() {
        match &mut items[index] {
            Block::Paragraph(paragraph)=>{
                nested(&mut paragraph.content,mappings);
                if let Some((parts,changes))=split_legacy(paragraph) {
                    let count=parts.len();
                    items.splice(index..=index,parts.into_iter().map(Block::Paragraph));
                    mappings.extend(changes);index+=count;continue;
                }
            }
            Block::Table(table)=>for row in &mut table.rows {for cell in &mut row.cells {blocks(&mut cell.blocks,mappings);}},
            _=>{}
        }
        index+=1;
    }
}

fn nested(items:&mut [Inline],mappings:&mut Vec<AnchorMapping>) {
    for item in items {match item {
        Inline::Hyperlink(link)=>nested(&mut link.content,mappings),
        Inline::TextBox(boxed)=>blocks(&mut boxed.blocks,mappings),
        _=>{}
    }}
}

fn breaks(items:&[Inline],offset:&mut usize,out:&mut Vec<(usize,usize)>) {
    for item in items {match item {
        Inline::Text(run)=>{
            for (index,ch) in run.text.char_indices() {
                if ch=='\n' || ch=='\r' {out.push((*offset+index,1));}
            }
            *offset+=run.text.len();
        }
        Inline::Hyperlink(link)=>breaks(&link.content,offset,out),
        _=>*offset+=inline_flat_len(item)
    }}
}

fn split_legacy(paragraph:&Paragraph)->Option<(Vec<Paragraph>,Vec<AnchorMapping>)> {
    let mut separators=Vec::new();breaks(&paragraph.content,&mut 0,&mut separators);
    if separators.is_empty() {return None;}
    // CRLF is one boundary, including when its two bytes span text runs.
    let text=paragraph_flat_text(paragraph);
    let mut merged:Vec<(usize,usize)>=Vec::new();
    for (at,len) in separators {
        if let Some((previous,width))=merged.last_mut() {
            if *width==1 && *previous+*width==at && text.as_bytes()[*previous]==b'\r' && text.as_bytes()[at]==b'\n' {
                *width+=len;continue;
            }
        }
        merged.push((at,len));
    }
    let mut parts=Vec::new();let mut mappings=Vec::new();
    let mut remaining=paragraph.content.to_vec();let mut consumed=0;
    let mut id=paragraph.id.clone();
    for (at,width) in merged {
        let local=at-consumed;
        let (left,rest)=split_content(&remaining,local).ok()?;
        let (_,right)=split_content(&rest,width).ok()?;
        let mut part=paragraph.clone();part.id=id.clone();part.content=normalized_content(left).into();parts.push(part);
        let next=NodeId::new();
        mappings.push(AnchorMapping::Replace{paragraph:id.clone(),start:local,end:local+width,inserted:0});
        mappings.push(AnchorMapping::Split{left:id,right:next.clone(),at:local});
        id=next;remaining=right;consumed=at+width;
    }
    let mut last=paragraph.clone();last.id=id;last.content=normalized_content(remaining).into();parts.push(last);
    Some((parts,mappings))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compatibility_preserves_soft_breaks_run_formats_and_rebases_references() {
        let mut paragraph=Paragraph::plain("");let id=paragraph.id.clone();
        let mut first=TextRun::plain("τίτλος\r");first.properties.bold=Some(true);
        let mut second=TextRun::plain("\nσώμα");second.properties.language=Some("el-GR".into());
        paragraph.content=vec![Inline::Text(first),Inline::Text(second),
            Inline::LineBreak{id:NodeId::new()},Inline::Text(TextRun::plain("συνέχεια"))].into();
        let mut document=Document::default();document.sections[0].blocks=vec![Block::Paragraph(paragraph)].into();
        let bookmark=model::Bookmark{id:NodeId::new(),name:"Body".into(),range:Some(model::AnchorRange {
            start:model::TextAnchor{paragraph:id.clone(),offset:"τίτλος\r\n".len()},
            end:model::TextAnchor{paragraph:id,offset:"τίτλος\r\nσώμα".len()}
        })};
        document.review.bookmarks.insert(bookmark.id.clone(),bookmark);
        normalize_document(&mut document);
        let paragraphs=model::document_paragraphs(&document);
        assert_eq!(paragraphs.len(),2);
        assert_eq!(model::inline_text(&paragraphs[0].content),"τίτλος");
        assert_eq!(model::inline_text(&paragraphs[1].content),"σώμα\nσυνέχεια");
        assert!(matches!(&paragraphs[0].content[0],Inline::Text(t) if t.properties.bold==Some(true)));
        assert!(matches!(&paragraphs[1].content[0],Inline::Text(t) if t.properties.language.as_deref()==Some("el-GR")));
        assert!(paragraphs[1].content.iter().any(|i|matches!(i,Inline::LineBreak{..})));
        let range=document.review.bookmarks.values().next().unwrap().range.as_ref().unwrap();
        assert_eq!(range.start,model::TextAnchor{paragraph:paragraphs[1].id.clone(),offset:0});
        assert_eq!(range.end,model::TextAnchor{paragraph:paragraphs[1].id.clone(),offset:"σώμα".len()});
        let before=document.clone();normalize_document(&mut document);assert_eq!(document,before);
    }

    #[test]
    fn multiline_paste_in_table_header_and_note_keeps_styles_local_and_undoable() {
        for container in 0..3 {
            let mut document=Document::default();document.sections[0].blocks.push(Block::Paragraph(Paragraph::plain("body")));
            let mut editor=EditorState::new(document);let body=model::document_paragraphs(&editor.document)[0].id.clone();editor.set_caret(body.clone(),0);
            match container {
                0=>editor.insert_table(1,1).unwrap(),
                1=>editor.enter_header(HeaderFooterVariant::Default).unwrap(),
                _=>{
                    editor.insert_footnote("note").unwrap();
                    let note=editor.document.review.footnotes.keys().next().unwrap().clone();
                    editor.enter_footnote(&note).unwrap();
                    let pos=editor.focus_position().unwrap();
                    editor.extend_selection(pos.node_id,4);editor.delete_selection().unwrap();
                }
            }
            let before=editor.document.clone();let selection=editor.selection.clone();
            let untouched=editor.paragraph(&body).unwrap().clone();
            editor.insert_text("title\nbody").unwrap();
            assert_eq!(editor.paragraph_text(&editor.focus_position().unwrap().node_id).as_deref(),Some("body"));
            assert_eq!(editor.paragraph(&body).unwrap(),&untouched);
            assert!(editor.undo());assert_eq!(editor.document,before);assert_eq!(editor.selection,selection);
            assert!(editor.redo());
            // Existing files need the same repair in nested block containers.
            let caret=editor.focus_position().unwrap();let mut legacy=editor.paragraph(&caret.node_id).unwrap().clone();
            legacy.content=vec![Inline::Text(TextRun::plain("one\ntwo"))].into();editor.replace_paragraph(&legacy).unwrap();
            let normalized=EditorState::new(editor.document);
            let paragraphs=model::document_paragraphs(&normalized.document);
            assert!(paragraphs.iter().any(|p|model::inline_text(&p.content)=="one"));
            assert!(paragraphs.iter().any(|p|model::inline_text(&p.content)=="two"));
            assert!(!paragraphs.iter().any(|p|p.content.iter().any(|i|matches!(i,Inline::Text(t) if t.text.contains('\n')))));
        }
    }

    #[test]
    fn multiline_replacement_preserves_outside_text_and_typing_format() {
        let mut document=Document::default();let p=Paragraph::plain("before old after");let id=p.id.clone();document.sections[0].blocks.push(Block::Paragraph(p));
        let mut editor=EditorState::new(document);editor.set_caret(id.clone(),7);editor.extend_selection(id,10);
        editor.typing_properties.italic=Some(true);
        let before=editor.document.clone();let selection=editor.selection.clone();
        editor.insert_text("νέο\nκείμενο").unwrap();
        let ps=model::document_paragraphs(&editor.document);
        assert_eq!(ps.iter().map(|p|model::inline_text(&p.content)).collect::<Vec<_>>(),["before νέο","κείμενο after"]);
        assert!(ps.iter().all(|p|p.content.iter().any(|i|matches!(i,Inline::Text(t) if t.properties.italic==Some(true)))));
        assert!(editor.undo());assert_eq!(editor.document,before);assert_eq!(editor.selection,selection);
    }
}
