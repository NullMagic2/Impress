//! Visible, conservative compatibility diagnostics. Keeping a package blob does
//! not imply its anchors, rendering, or behavior survived semantic editing.
use model::{Document,CompatibilityIssue,Block,Inline,FieldKind};
use crate::import::{parse_xml_tree,XmlNode};

pub(crate) fn audit_xml(xml:&str,part:&str,out:&mut Vec<CompatibilityIssue>) {
    let Ok(tree)=parse_xml_tree(xml)else{return;};
    fn scan(node:&XmlNode,part:&str,out:&mut Vec<CompatibilityIssue>){
        if node.name == "mc:AlternateContent" {
            if let Some(branch) = crate::import::alternate::selected_branch(node) {
                if branch.name == "mc:Fallback" {
                    push(out, "alternate-content-fallback", part, "A fallback representation was imported because the preferred Word content requires unsupported extensions; check its appearance.");
                }
                for child in &branch.children { scan(child, part, out); }
            } else {
                push(out, "alternate-content-unavailable", part, "Word alternate content has no supported choice or fallback; its visible content cannot be imported. Keep the original document.");
            }
            return;
        }
        let message=match node.name.as_str(){
            "w14:ligatures" if node.attrs.get("w14:val").is_none_or(|value| crate::ligatures::from_word(value).is_none()) =>
                Some(("ligature-combination", "This Word ligature combination is not representable by Impress's off/standard/standard+discretionary modes; inspect typography before exporting.")),
            "wp:anchor" if !supported_anchor(node)=>Some(("floating-drawing","This floating drawing uses an anchor or wrap configuration outside the supported page and paragraph offsets; check its placement.")),
            "a:srcRect" if ["l","t","r","b"].iter().any(|key| node.attrs.get(*key).is_some_and(|value| value.parse::<f32>().is_err()))
                =>Some(("picture-crop","This picture has invalid crop values; its crop may change when exported.")),
            "a:effectLst" if node.children.iter().any(|effect| !matches!(effect.name.as_str(),"a:outerShdw"|"a:glow"|"a:softEdge"|"a:reflection")
                || matches!(effect.name.as_str(),"a:outerShdw"|"a:glow") && !effect.children.iter().any(|child| child.name=="a:srgbClr"))
                =>Some(("drawing-effects","Some drawing effects are outside the supported shadow, glow, reflection, and soft-edge subset and may change when exported.")),
            "w:altChunk"=>Some(("alternative-content","Embedded alternative document content is not expanded.")),
            "w:object"=>Some(("embedded-object","The OLE payload and preview are imported and can be exported, but the embedded application cannot be activated or edited inside Impress.")),
            "w:pict" if !legacy_preview_supported(node)=>Some(("legacy-drawing","This legacy VML drawing has no supported text box or image preview; check its appearance.")),
            "w:pgNumType" if node.attrs.contains_key("w:chapStyle") || node.attrs.contains_key("w:chapSep")
                || node.attrs.get("w:fmt").is_some_and(|fmt| !matches!(fmt.as_str(),"decimal"|"upperRoman"|"lowerRoman"|"upperLetter"|"lowerLetter"))
                =>Some(("page-numbering","This page-number format or chapter-number prefix is not supported; decimal and Roman/letter page numbers are supported.")),
            "w:numRestart"|"w:numFmt" if part.ends_with("footnotes.xml")=>Some(("note-numbering","Footnotes use continuous decimal numbering in Impress.")),
            "w:highlight" if !node.attrs.get("w:val").is_some_and(|value|matches!(value.as_str(),"none"|"black"|"blue"|"cyan"|"green"|"magenta"|"red"|"yellow"|"white"|"darkBlue"|"darkCyan"|"darkGreen"|"darkMagenta"|"darkRed"|"darkYellow"|"darkGray"|"lightGray"))=>Some(("text-highlight","An unrecognized Word highlight color is outside the supported palette.")),
            "w:shd" if node.attrs.get("w:val").is_some_and(|value|!matches!(value.as_str(),"clear"|"nil"))=>Some(("shading-pattern","Patterned shading is imported as its background fill; the pattern is not regenerated.")),
            "w:footnoteReference" if part!="word/document.xml"=>Some(("note-scope","Footnotes inside headers, footers, or other footnotes are unsupported.")),
            "w:br" if node.attrs.get("w:type").is_some_and(|v|v=="page"||v=="column")=>None,
            _=>None,
        };
        if let Some((code,message))=message{push(out,code,part,message);}
        if node.name=="w:rPr"{
            for c in &node.children {if !matches!(c.name.as_str(),"w:rStyle"|"w:b"|"w:bCs"|"w:i"|"w:iCs"|"w:u"|"w:sz"|"w:szCs"|"w:rFonts"|"w:color"|"w:lang"|"w:vertAlign"|"w:noProof"|"w:rPrChange"|"w:ins"|"w:del"|"w:highlight"|"w:shd"|"w:rtl"|"w:cs"|"w14:ligatures") {push(out,"run-properties",part,"Some character properties are outside the supported DOCX subset and may change when saved.");break;}}
        }
        if node.name=="w:fldSimple"&&node.attrs.get("w:instr").is_some_and(|i|i.trim_start().starts_with("TOC")){push(out,"imported-toc",part,"A Word TOC field is retained as cached field text. Impress-generated contents uses editable entries and page-reference fields.");}
        if matches!(node.name.as_str(),"w:ins"|"w:del"|"w:moveFrom"|"w:moveTo")&&node.children.iter().any(|n|matches!(n.name.as_str(),"w:p"|"w:tbl")){push(out,"block-revisions",part,"Tracked block insertions/deletions are imported in their final view; structural revision history is not retained.");}
        for child in &node.children{scan(child,part,out);}
    }scan(&tree,part,out);
}
fn descendant(node:&XmlNode,name:&str)->bool{node.name==name||node.children.iter().any(|child|descendant(child,name))}
fn legacy_preview_supported(node:&XmlNode)->bool{descendant(node,"w:txbxContent")||descendant(node,"v:imagedata")}
fn supported_anchor(node:&XmlNode)->bool{
    let h=node.child("wp:positionH");let v=node.child("wp:positionV");
    let coords=match (h.and_then(|n|n.attr("relativeFrom")),v.and_then(|n|n.attr("relativeFrom"))){
        (Some("page"),Some("page"))|(Some("column"|"margin"),Some("paragraph"))=>true,_=>false};
    coords&&h.and_then(|n|n.child("wp:posOffset")).is_some_and(|n|n.text.parse::<i64>().is_ok())
        &&v.and_then(|n|n.child("wp:posOffset")).is_some_and(|n|n.text.parse::<i64>().is_ok())
        &&["wp:wrapSquare","wp:wrapTight","wp:wrapNone"].iter().any(|name|node.child(name).is_some())
        &&(descendant(node,"pic:pic")||descendant(node,"w:txbxContent"))
}

fn push(out:&mut Vec<CompatibilityIssue>,code:&str,part:&str,message:&str){if !out.iter().any(|w|w.code==code&&w.part==part){out.push(CompatibilityIssue{code:code.into(),part:part.into(),message:message.into()});}}

pub fn export_warnings(document:&Document)->Vec<String> {
    let mut warnings=document.compatibility.iter().map(|w|format!("{}: {}",w.part,w.message)).collect::<Vec<_>>();
    let paragraphs=model::document_paragraphs(document);
    for image in document.sections.iter().flat_map(|s|&s.blocks).filter_map(|b|if let Block::Image(i)=b{Some(i)}else{None}){
        if let Some(a)=&image.anchor{
            if !document.sections.iter().flat_map(|s|&s.blocks).any(|b|matches!(b,Block::Paragraph(p)if p.id==a.paragraph)){warnings.push("A floating picture has no surviving body anchor and cannot be placed in DOCX. Keep the native copy and reattach it before exporting.".into());}
            if image.wrap==model::ImageWrap::Tight{warnings.push("Tight alpha-silhouette wrapping exports as square wrapping in DOCX; native Impress retains the tight layout.".into());}
        }
    }
    if !document.review.history.is_empty(){warnings.push("Chronological native revision history (including formatting/structure and individual authors) is preserved only in .impress. DOCX exports the current document and separately supported net revisions, not the native journal. Keep a native copy.".into());}
    if document.review.comments.values().any(|c|!c.replies.is_empty()){warnings.push("Comment replies are exported as readable text within their parent comment, not as Word modern threaded-comment metadata.".into());}
    if document.review.comments.values().any(|c|c.resolved){warnings.push("Comment resolved status is native-only; DOCX comments will be exported as ordinary comments.".into());}
    if document.review.comments.values().any(|c|c.range.as_ref().is_none_or(|r|!model::valid_range(document,r))){warnings.push("Some comments have no live text anchor. Their text is retained in comments.xml, but Word may not display them in context.".into());}
    if document.review.bookmarks.values().any(|b|b.range.as_ref().is_none_or(|r|!model::valid_range(document,r))){warnings.push("Unanchored bookmarks cannot be placed in DOCX. Their metadata remains available in the native .impress file.".into());}
    if document.review.revisions.values().any(|r|!paragraphs.iter().any(|p|p.id==r.paragraph)){warnings.push("A pending revision belongs to a deleted paragraph and cannot be placed in DOCX. Save a native copy or resolve it first.".into());}
    if document.review.track_text{warnings.push("Paragraph-text tracking is an Impress editing mode. Pending revisions are exported, but Word's track-future-changes setting is not enabled by this mode.".into());}
    if document.review.revisions.values().any(|revision|!document.review.tracked_changes.values().any(|change|change.status==model::RevisionStatus::Pending && change.effects.iter().any(|effect|matches!(effect,model::RevisionEffect::Inline{range,..}if range.start.paragraph==revision.paragraph)))){warnings.push("Pending text revisions are serialized as bounded original/final text differences, not the original sequence of keystrokes, multiple-author events or formatting edits.".into());}
    if !document.review.contents_paragraphs.is_empty(){warnings.push("Contents entries and page-reference fields are exported, but Impress contents settings are native-only. Reconfigure heading depth after DOCX import.".into());}
    fn visit(items:&[Inline],document:&Document,warnings:&mut Vec<String>){for i in items{match i{
        Inline::Hyperlink(h)=>visit(&h.content,document,warnings),
        Inline::Field(f)=>match &f.kind{
            FieldKind::PageReference{bookmark} if !document.review.bookmarks.values().any(|b|&b.name==bookmark&&b.range.is_some())=>warnings.push(format!("Page reference has no valid bookmark: {bookmark}")),
            FieldKind::FootnoteReference{note} if !document.review.footnotes.contains_key(note)=>warnings.push("A footnote reference is missing its note content.".into()),
            FieldKind::Raw{instruction}=>warnings.push(format!("Field uses cached text rather than a live Impress calculation: {}",instruction.chars().take(100).collect::<String>())),_=>{}
        },_=>{}
    }}}
    for p in paragraphs{visit(&p.content,document,&mut warnings);}
    fn foreign(blocks:&[Block])->bool{blocks.iter().any(|b|match b{Block::Foreign(_)=>true,Block::Table(t)=>t.rows.iter().any(|r|r.cells.iter().any(|c|foreign(&c.blocks))),_=>false})}
    if document.sections.iter().any(|s|foreign(&s.blocks)){warnings.push("Foreign blocks are not emitted by the DOCX adapter. Keep a native copy.".into());}
    // Fonts and other package payloads are resources too, but they are not
    // raster images. Font payloads are decoded by the typography catalog.
    for resource in document.resources.values() {
        let media_type=resource.media_type.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
        if media_type.starts_with("image/") && !matches!(media_type.as_str(),"image/png"|"image/jpeg"|"image/jpg"|"image/gif"|"image/bmp") {
            warnings.push(format!("The canvas cannot decode every image of type {}. Its original bytes remain in the document.",resource.media_type));
        }
    }
    warnings.sort();warnings.dedup();warnings
}

