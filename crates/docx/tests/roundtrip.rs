//! DOCX import and export round trips, run with Cargo.
use model::*;
use editor::EditorState;
use format::DocumentFormat;
use docx::DocxFormat;
use std::io::{Cursor, Read, Write};
const MIXED:&[u8]=include_bytes!("fixtures/mixed.docx");
fn load(bytes:&[u8])->Document{DocxFormat.import(&mut Cursor::new(bytes)).expect("fixture must import")}
fn cycle(doc:&Document)->Document{let mut out=Cursor::new(Vec::new());DocxFormat.export(doc,&mut out).unwrap();out.set_position(0);DocxFormat.import(&mut out).unwrap()}
fn texts(doc:&Document)->Vec<String>{document_paragraphs(doc).iter().map(|p|inline_text(&p.content)).collect()}
fn find<'a>(doc:&'a Document,text:&str)->&'a Paragraph { document_paragraphs(doc).into_iter().find(|p|inline_text(&p.content)==text).unwrap_or_else(||panic!("Missing paragraph {text:?}: {:?}",texts(doc))) }
fn note_text(doc:&Document)->String{let note=doc.review.footnotes.values().next().unwrap();let mut ps=Vec::new();visit_paragraphs(&note.blocks,&mut ps);ps.iter().map(|p|inline_text(&p.content)).collect::<Vec<_>>().join("\n")}
fn has_link(items:&[Inline],target:&str)->bool{items.iter().any(|i|matches!(i,Inline::Hyperlink(h) if h.target==target||has_link(&h.content,target)))}
fn has_page_ref(items:&[Inline])->bool{items.iter().any(|i|match i{Inline::Field(f)=>matches!(&f.kind,FieldKind::PageReference{bookmark} if bookmark=="Topic"),Inline::Hyperlink(h)=>has_page_ref(&h.content),_=>false})}

#[test]fn mixed_import_preserves_entities_links_styles_and_review(){
    let d=load(MIXED);let heading=find(&d,"Résumé Ελληνικά & α β");
    assert_eq!(heading.style.as_ref().map(|s|s.0.as_str()),Some("Heading1"));
    let Inline::Text(run)=&heading.content[0]else{panic!("text run")};assert_eq!(run.properties.bold,Some(false));
    let normal=d.styles.resolve_character(Some(&StyleId::from("Normal")));assert_eq!(normal.font_size_pt,Some(12.0));assert_eq!(normal.font_family.as_deref(),Some("Arial"));
    assert!(document_paragraphs(&d).iter().any(|p|has_link(&p.content,"https://example.invalid/path?a=1&b=2")));
    assert_eq!(d.review.comments.len(),1);let c=d.review.comments.values().next().unwrap();assert_eq!(c.text,"Check Greek & accents.");assert!(valid_range(&d,c.range.as_ref().unwrap()));
    let b=d.review.bookmarks.values().find(|b|b.name=="Topic").unwrap();assert!(valid_range(&d,b.range.as_ref().unwrap()));
    assert_eq!(d.review.revisions.len(),1);let r=d.review.revisions.values().next().unwrap();assert_eq!(inline_text(&r.before.content),"Track: old");find(&d,"Track: new");
    assert_eq!(note_text(&d),"One note α.");assert_eq!(d.review.footnotes.len(),1);assert_eq!(footnote_references(&d).len(),1);
    assert_eq!(d.review.contents_paragraphs.len(),2);assert!(document_paragraphs(&d).iter().any(|p|has_page_ref(&p.content)));
    assert_eq!(d.sections.len(),2);assert_eq!(d.resources.len(),1);assert_eq!(d.lists.len(),1);
    assert!(d.sections.iter().any(|s|s.blocks.iter().any(|b|matches!(b,Block::PageBreak{..}))));
}
fn document_only(xml:&str)->Vec<u8>{let mut zip=zip::ZipWriter::new(Cursor::new(Vec::new()));zip.start_file("word/document.xml",zip::write::SimpleFileOptions::default()).unwrap();zip.write_all(xml.as_bytes()).unwrap();zip.finish().unwrap().into_inner()}

#[test]fn picture_crop_and_outer_shadow_survive_docx_cycle(){
    let mut document=Document::default();
    let id=ResourceId::new();
    document.resources.insert(id.clone(),Resource { font_faces: Vec::new(), id:id.clone(),media_type:"image/png".into(),original_name:Some("test.png".into()),data:vec![137,80,78,71,13,10,26,10].into()});
    let effects=ImageEffects{crop:Some(ImageCrop{left:0.1,top:0.2,right:0.0,bottom:0.0}),
        outer_shadow:Some(ImageShadow{color:"#224466".into(),opacity:0.4,distance_mm:2.0,direction_degrees:45.0,blur_mm:1.0}),..Default::default()};
    document.sections[0].blocks.push(Block::Paragraph(Paragraph{id:NodeId::new(),style:None,properties:Default::default(),
        content:vec![Inline::Image(InlineImage{id:NodeId::new(),resource:id,width_mm:30.0,height_mm:20.0,alt_text:None,effects:effects.clone()})].into()}));
    let reopened=cycle(&document);
    let Inline::Image(image)=&document_paragraphs(&reopened)[0].content[0] else {panic!("missing image")};
    assert_eq!(image.effects,effects);
}
#[test]fn xml_doctype_unknown_entity_and_deep_nesting_are_rejected(){
    let ns="http://schemas.openxmlformats.org/wordprocessingml/2006/main";
    for bad in [format!("<!DOCTYPE x [<!ENTITY x 'hidden'>]><w:document xmlns:w=\"{ns}\"><w:body/></w:document>"),format!("<w:document xmlns:w=\"{ns}\"><w:body><w:p><w:r><w:t>&unknown;</w:t></w:r></w:p></w:body></w:document>"),format!("<w:document xmlns:w=\"{ns}\"><w:body>{}text{}</w:body></w:document>","<w:sdt>".repeat(140),"</w:sdt>".repeat(140))] {assert!(DocxFormat.import(&mut Cursor::new(document_only(&bad))).is_err());}
}

fn editor() -> (EditorState,NodeId) {
    let mut d=Document::default();let p=Paragraph::plain("body");let id=p.id.clone();
    d.sections[0].blocks=(vec![Block::Paragraph(p)]).into();let mut e=EditorState::new(d);e.set_caret(id.clone(),4);(e,id)
}
fn bytes(e:&EditorState)->Vec<u8>{let mut out=Cursor::new(Vec::new());DocxFormat.export(&e.document,&mut out).unwrap();out.into_inner()}
fn xml(data:&[u8],name:&str)->String{let mut zip=zip::ZipArchive::new(Cursor::new(data)).unwrap();let mut out=String::new();zip.by_name(name).unwrap().read_to_string(&mut out).unwrap();out}
#[test]fn endnotes_have_separate_ooxml_parts_and_roundtrip_kind(){
    let(mut e,id)=editor();e.insert_footnote("Footnote α").unwrap();e.set_caret(id,0);e.insert_endnote("Endnote β").unwrap();let data=bytes(&e);
    assert!(xml(&data,"word/footnotes.xml").contains("Footnote α"));assert!(xml(&data,"word/endnotes.xml").contains("Endnote β"));
    assert!(xml(&data,"word/document.xml").contains("<w:endnoteReference"));assert!(xml(&data,"word/_rels/document.xml.rels").contains("/endnotes"));
    let reopened=DocxFormat.import(&mut Cursor::new(data)).unwrap();assert_eq!(reopened.review.footnotes.values().filter(|n|n.kind==NoteKind::Footnote).count(),1);assert_eq!(reopened.review.footnotes.values().filter(|n|n.kind==NoteKind::Endnote).count(),1);
}

#[test]
fn all_crop_presets_and_adjustments_preserve_geometry_outline_and_original_image() {
    let choices: Vec<serde_json::Value> = serde_json::from_str(include_str!("../../../design/picture-shapes.json")).unwrap();
    assert_eq!(choices.len(), 176);
    let mut document = Document::default();
    let resource = ResourceId::new();
    let original = vec![137,80,78,71,13,10,26,10];
    document.resources.insert(resource.clone(), Resource { id: resource.clone(), media_type: "image/png".into(), original_name: None, font_faces: Vec::new(), data: original.clone().into() });
    let expected = choices.iter().map(|choice| ImageEffects {
        shape: ImageShape { preset: choice["preset"].as_str().unwrap().into(), adjustments: std::collections::BTreeMap::from([("adj".into(), "val 25000".into())]) },
        outline: Some(Border { width_pt: 2.0, color: "#6B817A".into(), style: BorderStyle::Single }),
        crop: Some(ImageCrop { left: 0.1, top: 0.2, right: 0.1, bottom: 0.0 }),
        ..Default::default()
    }).collect::<Vec<_>>();
    document.sections[0].blocks = expected.iter().map(|effects| Block::Paragraph(Paragraph {
        id: NodeId::new(), style: None, properties: Default::default(), content: vec![Inline::Image(InlineImage {
            id: NodeId::new(), resource: resource.clone(), width_mm: 30.0, height_mm: 20.0, alt_text: None, effects: effects.clone(),
        })].into(),
    })).collect();
    let reopened = cycle(&document);
    let mut native = Cursor::new(Vec::new());
    format_native::NativeImpressFormat.export(&reopened, &mut native).unwrap();
    native.set_position(0);
    let reopened = format_native::NativeImpressFormat.import(&mut native).unwrap();
    for (paragraph, expected) in document_paragraphs(&reopened).into_iter().zip(expected) {
        let Inline::Image(image) = &paragraph.content[0] else { panic!("picture") };
        assert_eq!(image.effects, expected);
        assert_eq!(reopened.resources[&image.resource].data.as_slice(), original);
    }
}

#[test]
fn contents_templates_and_direct_outline_levels_survive_docx() {
    let mut doc = Document::default();
    let front = Paragraph::plain("Cover");
    let front_id = front.id.clone();
    let heading = Paragraph::plain("Custom chapter");
    let heading_id = heading.id.clone();
    doc.sections[0].blocks = vec![Block::Paragraph(front), Block::Paragraph(heading)].into();
    let mut editor = EditorState::new(doc);
    editor.set_caret(heading_id, 0);
    editor.set_contents_level(2).unwrap();
    editor.set_caret(front_id, "Cover".len());
    let mut options = ContentsOptions { title: "Roadmap".into(), max_heading_level: 2, ..Default::default() };
    options.apply_template(ContentsTemplate::Modern);
    options.leader = TabLeader::Dashes;
    editor.update_contents_options(options.clone()).unwrap();
    let reopened = cycle(&editor.document);
    assert_eq!(reopened.review.contents_options, options);
    assert!(reopened.review.automatic_contents);
    let paragraphs = document_paragraphs(&reopened);
    assert_eq!(inline_text(&paragraphs[0].content), "Cover");
    assert_eq!(inline_text(&paragraphs[1].content), "Roadmap");
    assert_eq!(paragraphs[2].properties.contents_leader, Some(TabLeader::Dashes));
    assert_eq!(paragraphs[3].properties.outline_level, Some(2));
    assert_eq!(reopened.review.contents_paragraphs, vec![paragraphs[1].id.clone(), paragraphs[2].id.clone()]);
}
