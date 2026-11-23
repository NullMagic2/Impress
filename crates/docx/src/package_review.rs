//! Review-aware run serialization. Marker offsets follow the same UTF-8 stream
//! used by the editor, including atomic inline objects.
use super::*;
use model::AnchorRange;

pub(super) fn collect_links(document:&Document)->BTreeSet<String> {
    fn visit(items:&[Inline],out:&mut BTreeSet<String>){for i in items{if let Inline::Hyperlink(h)=i{if !h.target.starts_with('#'){out.insert(h.target.clone());}visit(&h.content,out);}}}
    let mut links=BTreeSet::new();for p in model::document_paragraphs(document){visit(&p.content,&mut links);}
    for note in document.review.footnotes.values(){let mut ps=Vec::new();model::visit_paragraphs(&note.blocks,&mut ps);for p in ps{visit(&p.content,&mut links);}}
    for r in document.review.revisions.values(){visit(&r.before.content,&mut links);}links
}
fn marker_map(p:&Paragraph,document:&Document,context:&ExportContext)->BTreeMap<usize,Vec<(u8,String)>> {
    let mut map=BTreeMap::<usize,Vec<(u8,String)>>::new();
    let mut add=|range:&AnchorRange,start:String,end:String|{
        let collapsed=range.start==range.end;
        if range.start.paragraph==p.id{map.entry(range.start.offset).or_default().push((1,start));}
        if range.end.paragraph==p.id{map.entry(range.end.offset).or_default().push((if collapsed{2}else{0},end));}
    };
    for b in document.review.bookmarks.values(){if let (Some(range),Some(id))=(&b.range,context.bookmark_ids.get(&b.id)) {
        add(range,format!("<w:bookmarkStart w:id=\"{id}\" w:name=\"{}\"/>",xml_escape(&b.name)),format!("<w:bookmarkEnd w:id=\"{id}\"/>"));
    }}
    for c in document.review.comments.values(){if let (Some(range),Some(id))=(&c.range,context.comment_ids.get(&c.id)) {
        add(range,format!("<w:commentRangeStart w:id=\"{id}\"/>"),format!("<w:commentRangeEnd w:id=\"{id}\"/><w:r><w:commentReference w:id=\"{id}\"/></w:r>"));
    }}
    controls::inline_markers(p,document,&mut map);
    for values in map.values_mut(){values.sort_by_key(|v|v.0);}map
}
pub(super) fn emit_marker(at:usize,map:&mut BTreeMap<usize,Vec<(u8,String)>>,out:&mut String){if let Some(values)=map.remove(&at){for (_,xml) in values{out.push_str(&xml);}}}
pub(super) fn serialize(items:&[Inline],document:&Document,context:&ExportContext,drawing:&mut u32,offset:&mut usize,map:&mut BTreeMap<usize,Vec<(u8,String)>>)->String {
    let mut out=String::new();
    for inline in items {
        emit_marker(*offset,map,&mut out);
        match inline {
            Inline::Text(t)=>{
                let end=*offset+t.text.len();let mut boundaries:Vec<usize>=map.range(*offset..end).map(|(k,_)|*k).filter(|k|t.text.is_char_boundary(*k-*offset)).collect();boundaries.push(end);
                let base=*offset;
                for boundary in boundaries {if boundary>*offset{let mut run=t.clone();run.text=t.text[(*offset-base)..(boundary-base)].into();out.push_str(&run_xml(&run));*offset=boundary;}emit_marker(*offset,map,&mut out);}
            }
            Inline::Hyperlink(h)=>{
                let hyperlink_end=*offset+model::inline_text(&h.content).len();
                let deferred=map.remove(&hyperlink_end);
                let inner=serialize(&h.content,document,context,drawing,offset,map);
                if let Some(markers)=deferred {map.insert(hyperlink_end,markers);}
                if let Some(anchor)=h.target.strip_prefix('#'){out.push_str(&format!("<w:hyperlink w:anchor=\"{}\">{inner}</w:hyperlink>",xml_escape(anchor)));}
                else if let Some(id)=context.links.get(&h.target){out.push_str(&format!("<w:hyperlink r:id=\"{id}\">{inner}</w:hyperlink>"));}
                else{out.push_str(&inner);}
            }
            Inline::Equation(e)=>{out.push_str(&format!("<m:oMath>{}</m:oMath>",node_to_omml(&e.root)));*offset+=3;}
            Inline::Image(i)=>{out.push_str(&format!("<w:r>{}</w:r>",drawing_xml(&i.resource,i.width_mm,i.height_mm,i.alt_text.as_deref(),&i.effects,document,context,drawing)));*offset+=3;}
            Inline::EmbeddedObject(object)=>{out.push_str(&format!("<w:r>{}</w:r>",embedded_object_xml(object,context,drawing)));*offset+=3;}
            Inline::TextBox(b)=>{out.push_str(&format!("<w:r>{}</w:r>",text_box_xml(b,document,context,drawing)));*offset+=3;}
            Inline::Field(f)=>{
                if let FieldKind::FootnoteReference{note}=&f.kind {
                    if let Some(id)=context.footnote_ids.get(note){let tag=if document.review.footnotes.get(note).is_some_and(|n|n.kind==model::NoteKind::Endnote){"endnoteReference"}else{"footnoteReference"};out.push_str(&format!("<w:r><w:rPr><w:vertAlign w:val=\"superscript\"/></w:rPr><w:{tag} w:id=\"{id}\"/></w:r>"));}
                    else{out.push_str("<w:r><w:t>[missing footnote]</w:t></w:r>");}
                }else{out.push_str(&field_xml(f));}*offset+=3;
            }
            Inline::PaginationBreak{break_kind,..}=>{let value=match break_kind{model::PaginationBreakKind::Page=>"page",model::PaginationBreakKind::Column=>"column"};out.push_str(&format!(r#"<w:r><w:br w:type="{value}"/></w:r>"#));*offset+=1;}
            Inline::LineBreak{..}=>{out.push_str("<w:r><w:br/></w:r>");*offset+=1;}
            Inline::Tab{..}=>{out.push_str("<w:r><w:tab/></w:r>");*offset+=1;}
        }
    }emit_marker(*offset,map,&mut out);out
}
// Slice a semantic inline stream at validated UTF-8 boundaries. Objects are
// indivisible U+FFFC markers and hyperlink containers keep their target.
pub(super) fn content_slice(items:&[Inline],start:usize,end:usize)->Vec<Inline>{
    let mut out=Vec::new();let mut offset=0;
    for item in items {
        let len=model::inline_text(std::slice::from_ref(item)).len();let a=start.saturating_sub(offset).min(len);let b=end.saturating_sub(offset).min(len);
        if a<b {let mut item=item.clone();match &mut item {
            Inline::Text(t)=>t.text=t.text.get(a..b).unwrap_or("").into(),
            Inline::Hyperlink(h)=>h.content=(content_slice(&h.content,a,b)).into(),_=>{}
        }out.push(item);}offset+=len;if offset>=end{break;}
    }out
}
pub(super) fn paragraph_with_annotations(p:&Paragraph,document:&Document,context:&ExportContext,drawing:&mut u32)->String {
    let mut markers=marker_map(p,document,context);let mut offset=0;let mut content=String::new();
    if super::tracked::has_inline(&p.id,document) {
        content=super::tracked::paragraph_content(p,document,context,drawing,&mut markers);
    }else if let Some(revision)=document.review.revisions.values().find(|r|r.paragraph==p.id) {
        let old=model::inline_text(&revision.before.content);let new=model::inline_text(&p.content);
        let author=xml_escape(&revision.author);let date=revision.date.as_ref().map(|d|format!(" w:date=\"{}\"",xml_escape(d))).unwrap_or_default();
        for change in model::text_changes(&old,&new) {
            let equal=content_slice(&p.content,offset,change.new.start);
            content.push_str(&serialize(&equal,document,context,drawing,&mut offset,&mut markers));
            if !change.old.is_empty(){
                let original=serialize(&content_slice(&revision.before.content,change.old.start,change.old.end),document,context,drawing,&mut 0,&mut BTreeMap::new())
                    .replace("<w:t ","<w:delText ").replace("<w:t>","<w:delText>").replace("</w:t>","</w:delText>").replace("<w:instrText ","<w:delInstrText ").replace("<w:instrText>","<w:delInstrText>").replace("</w:instrText>","</w:delInstrText>");
                let id=context.revision_serial.get();context.revision_serial.set(id.saturating_add(1));
                content.push_str(&format!("<w:del w:id=\"{id}\" w:author=\"{author}\"{date}>{original}</w:del>"));
            }
            if !change.new.is_empty(){
                let id=context.revision_serial.get();context.revision_serial.set(id.saturating_add(1));
                let mut boundaries=markers.range(change.new.start..change.new.end).map(|(offset,_)|*offset).filter(|at|*at>change.new.start).collect::<Vec<_>>();boundaries.push(change.new.end);
                for end in boundaries {
                    emit_marker(offset,&mut markers,&mut content);
                    let deferred=markers.remove(&end);
                    let inserted=serialize(&content_slice(&p.content,offset,end),document,context,drawing,&mut offset,&mut markers);
                    content.push_str(&format!("<w:ins w:id=\"{id}\" w:author=\"{author}\"{date}>{inserted}</w:ins>"));
                    if let Some(values)=deferred{markers.insert(end,values);}emit_marker(end,&mut markers,&mut content);
                }
            }
        }
        content.push_str(&serialize(&content_slice(&p.content,offset,new.len()),document,context,drawing,&mut offset,&mut markers));
    }else{content=serialize(&p.content,document,context,drawing,&mut offset,&mut markers);}
    let mut anchored=String::new();
    for image in document.sections.iter().flat_map(|s|&s.blocks).filter_map(|b|match b{Block::Image(i)if i.anchor.as_ref().is_some_and(|a|a.paragraph==p.id)&&i.wrap!=model::ImageWrap::Inline=>Some(i),_=>None}){
        anchored.push_str(&controls::floating_picture(image,format!("<w:r>{}</w:r>",floating_drawing_xml(image,document,context,drawing)),document));
    }
    format!("<w:p>{}{anchored}{content}</w:p>",super::tracked::paragraph_properties(p,document,context,paragraph_properties_xml(p,context,&document.styles.resolve_paragraph(p.style.as_ref()).tabs.unwrap_or_default())))
}
/// Word uses the final paragraph ID to associate reply and resolution metadata.
pub(super) fn comments_xml(document: &Document, context: &ExportContext) -> (String, String) {
    let mut body = String::new();
    let mut extended = String::new();
    let mut next_para = 1u32;
    let mut next_reply = context.comment_ids.len() + 1;
    for comment in document.review.comments.values() {
        let Some(id) = context.comment_ids.get(&comment.id) else { continue };
        let decision = match comment.decision {
            Some(model::CommentDecision::Accepted) => r#" impress:decision="accepted""#,
            Some(model::CommentDecision::Rejected) => r#" impress:decision="rejected""#,
            None => "",
        };
        let para = append_comment(&mut body, &mut next_para, *id, &comment.author,
            comment.date.as_deref(), &comment.text, decision);
        let done = u8::from(comment.is_closed());
        extended.push_str(&format!(r#"<w15:commentEx w15:paraId="{para}" w15:done="{done}"/>"#));
        for reply in &comment.replies {
            let date = (!reply.date.is_empty()).then_some(reply.date.as_str());
            let reply_para = append_comment(&mut body, &mut next_para, next_reply, &reply.author, date, &reply.text, "");
            next_reply += 1;
            extended.push_str(&format!(r#"<w15:commentEx w15:paraId="{reply_para}" w15:paraIdParent="{para}" w15:done="0"/>"#));
        }
    }
    (
        format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:comments xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml" xmlns:impress="urn:impress:comments:1" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" mc:Ignorable="w14 impress" mc:PreserveAttributes="impress:decision">{body}</w:comments>"#),
        format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w15:commentsEx xmlns:w15="http://schemas.microsoft.com/office/word/2012/wordml">{extended}</w15:commentsEx>"#),
    )
}

fn append_comment(body: &mut String, next_para: &mut u32, id: usize, author: &str, date: Option<&str>, text: &str, decision: &str) -> String {
    let date = date.map(|date| format!(r#" w:date="{}""#, xml_escape(date))).unwrap_or_default();
    body.push_str(&format!(r#"<w:comment w:id="{id}" w:author="{}"{date}{decision}>"#, xml_escape(author)));
    let mut para = String::new();
    for line in text.split('\n') {
        para = format!("{:08X}", *next_para);
        *next_para += 1;
        body.push_str(&format!(r#"<w:p w14:paraId="{para}"><w:r><w:t xml:space="preserve">{}</w:t></w:r></w:p>"#, xml_escape(line)));
    }
    body.push_str("</w:comment>");
    para
}
pub(super) fn styles_xml(document:&Document,context:&ExportContext)->String {
    super::styles::styles_xml(document,context)
}

pub(super) fn notes_xml(document:&Document,context:&ExportContext,drawing:&mut u32,kind:model::NoteKind)->String {
    let stem=if kind==model::NoteKind::Endnote{"endnote"}else{"footnote"};
    let mut body=format!("<w:{stem} w:type=\"separator\" w:id=\"-1\"><w:p><w:r><w:separator/></w:r></w:p></w:{stem}><w:{stem} w:type=\"continuationSeparator\" w:id=\"0\"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:{stem}>");
    for note in document.review.footnotes.values().filter(|n|n.kind==kind){let Some(id)=context.footnote_ids.get(&note.id) else{continue;};
        let mut content=String::new();for block in &note.blocks{content.push_str(&block_xml(block,document,context,drawing));}
        // The automatic note mark precedes the first run, after paragraph props.
        let mark=format!("<w:r><w:rPr><w:vertAlign w:val=\"superscript\"/></w:rPr><w:{stem}Ref/></w:r><w:r><w:t xml:space=\"preserve\"> </w:t></w:r>");
        if content.starts_with("<w:p>"){
            let position=if content[5..].starts_with("<w:pPr>"){content.find("</w:pPr>").map(|n|n+8).unwrap_or(5)}else{5};content.insert_str(position,&mark);
        }else{content=format!("<w:p>{mark}</w:p>{content}");}
        body.push_str(&format!("<w:{stem} w:id=\"{id}\">{content}</w:{stem}>"));
    }
    format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:{stem}s xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" mc:Ignorable="w14" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture">{body}</w:{stem}s>"#)
}

/// Request that Word refresh PAGE/PAGEREF fields on open while retaining other
/// settings, including the current revision-recording preference.
pub(super) fn settings_xml(document:&Document)->String {
    let default_mm = document.styles.resolve_paragraph(Some(&model::StyleId::from(model::STYLE_NORMAL))).default_tab_mm.unwrap_or(model::DEFAULT_TAB_INTERVAL_MM);
    let default_tab = format!("<w:defaultTabStop xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" w:val=\"{}\"/>", twips(default_mm));
    let embedding=if document.resources.values().any(|r|r.media_type==typography::EMBEDDED_FONT_MEDIA_TYPE){"<w:embedTrueTypeFonts xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" w:val=\"true\"/>"}else{""};
    let tracking = if document.review.track_changes {
        r#"<w:trackRevisions xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" w:val="true"/>"#
    } else {
        ""
    };
    if let Some(xml)=preserved_text(document,"word/settings.xml") {
        let mut reader=Reader::from_str(&xml);let mut out=String::new();let mut skip=0usize;
        loop {let start=reader.buffer_position() as usize;match reader.read_event(){
            Ok(Event::Eof)=>break,
            Ok(Event::Start(e)) if e.name().as_ref().rsplit(':').next() .is_some_and(|name|matches!(name,"updateFields"|"embedTrueTypeFonts"|"saveSubsetFonts"|"defaultTabStop"|"trackRevisions"))=>{skip=1;},
            Ok(Event::Empty(e)) if e.name().as_ref().rsplit(':').next() .is_some_and(|name|matches!(name,"updateFields"|"embedTrueTypeFonts"|"saveSubsetFonts"|"defaultTabStop"|"trackRevisions"))=>{},
            Ok(Event::Start(_)) if skip>0=>skip+=1,Ok(Event::End(_)) if skip>0=>skip-=1,
            Ok(_)=>{if skip==0{out.push_str(&xml[start..reader.buffer_position() as usize]);}},Err(_)=>{out.clear();break;}
        }}
        if let Some(close)=out.rfind("</"){if out[close..].contains("settings>"){out.insert_str(close,&default_tab);out.insert_str(close,embedding);out.insert_str(close,tracking);out.insert_str(close,"<w:updateFields xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" w:val=\"true\"/>");return out;}}
    }
    let mut result= "<?xml version=\"1.0\" encoding=\"UTF-8\"?><w:settings xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:updateFields w:val=\"true\"/></w:settings>".to_owned();
    let close=result.find("</w:settings>").expect("settings close");result.insert_str(close,&default_tab);result.insert_str(close,embedding);result.insert_str(close,tracking);result
}
