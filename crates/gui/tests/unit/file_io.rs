use super::*;
#[test]
fn selected_save_type_replaces_stale_native_suffix() {
    assert_eq!(ensure_extension_for_filter(Path::new("Untitled.impress"), 1), Path::new("Untitled.docx"));
    assert_eq!(ensure_extension_for_filter(Path::new("report.docx"), 3), Path::new("report.rtf"));
    assert_eq!(ensure_extension_for_filter(Path::new("report.rtf"), 2), Path::new("report.typ"));
}

#[test]
fn explicit_save_embedding_can_be_enabled_then_removed_in_both_formats() {
    let mut formats=FormatRegistry::default();formats.register(format_native::NativeImpressFormat);formats.register(docx::DocxFormat);
    let dir=std::env::temp_dir().join(format!("impress-embed-save-{}",model::NodeId::new()));std::fs::create_dir_all(&dir).unwrap();
    let mut document=Document::default();let id=model::ResourceId::new();
    document.resources.insert(id.clone(),model::Resource{id,media_type:typography::EMBEDDED_FONT_MEDIA_TYPE.into(),original_name:None,
        font_faces:vec![model::EmbeddedFontFace{family:"Save Test Alias".into(),bold:false,italic:false,face_index:0}],data:typst_assets::fonts().next().unwrap().to_vec().into()});
    let mut p=model::Paragraph::plain("Hello");if let model::Inline::Text(run)=&mut p.content[0]{run.properties.font_family=Some("Save Test Alias".into());}
    document.sections[0].blocks.push(model::Block::Paragraph(p));
    let original=document.clone();
    for extension in ["impress","docx"] {
        let path=dir.join(format!("test.{extension}"));save_with_font_embedding(&path,&document,&formats,true).unwrap();
        let embedded=load_document(&path,&formats).unwrap();assert!(embedded.resources.values().any(|r|r.media_type==typography::EMBEDDED_FONT_MEDIA_TYPE));
        assert!(typography::FontCatalog::for_resources(&embedded.resources).resolve("Save Test Alias",false,false).is_some());
        save_with_font_embedding(&path,&embedded,&formats,false).unwrap();assert!(!load_document(&path,&formats).unwrap().resources.values().any(|r|r.media_type==typography::EMBEDDED_FONT_MEDIA_TYPE));
    }
    assert_eq!(original,document);std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn modified_and_redefined_style_sizes_survive_native_and_docx_saves() {
    let mut formats=FormatRegistry::default();formats.register(format_native::NativeImpressFormat);formats.register(docx::DocxFormat);
    let dir=std::env::temp_dir().join(format!("impress-style-save-{}",model::NodeId::new()));std::fs::create_dir_all(&dir).unwrap();
    let mut document=Document::default();
    let mut title=model::Paragraph::plain("Γενετική μηχανή");title.style=Some(model::StyleId::from(model::STYLE_HEADING_1));
    if let model::Inline::Text(run)=&mut title.content[0] {run.properties.font_size_pt=Some(12.0);}
    let mut body=model::Paragraph::plain("Πάντα ευχήθηκαν");
    if let model::Inline::Text(run)=&mut body.content[0] {run.properties.font_size_pt=Some(12.0);}
    let first=title.id.clone();document.sections[0].blocks=vec![model::Block::Paragraph(title),model::Block::Paragraph(body)].into();
    let mut editor=editor::EditorState::new(document);editor.set_caret(first.clone(),0);editor.extend_selection(first,"Γενετική μηχανή".len());
    let mut style=editor.document.styles.get(&model::StyleId::from(model::STYLE_HEADING_1)).unwrap().clone();style.character.font_size_pt=Some(14.0);
    editor.put_styles_with_selection(vec![style]).unwrap();
    editor.update_style_from_selection(&model::StyleId::from(model::STYLE_TITLE)).unwrap();
    for extension in ["impress","docx"] {
        let path=dir.join(format!("test.{extension}"));save_with_font_embedding(&path,&editor.document,&formats,false).unwrap();
        let loaded=load_document(&path,&formats).unwrap();let paragraphs=model::document_paragraphs(&loaded);
        for (text,size) in [("Γενετική μηχανή",14.0),("Πάντα ευχήθηκαν",12.0)] {
            let paragraph=paragraphs.iter().find(|p|model::inline_text(&p.content)==text).unwrap();
            let run=paragraph.content.iter().find_map(|i|match i {model::Inline::Text(t)=>Some(t),_=>None}).unwrap();
            assert_eq!(loaded.styles.resolve_run(paragraph.style.as_ref(),run.style.as_ref(),&run.properties).font_size_pt,Some(size),"{extension}: {text}");
        }
        let title=loaded.styles.id_by_name("Title").unwrap();assert_eq!(loaded.styles.resolve_character(Some(&title)).font_size_pt,Some(14.0));
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn enter_format_survives_empty_paragraph_native_and_docx_roundtrips() {
    use model::{Block, Paragraph, Inline, StyleId, STYLE_NORMAL, CharacterProperties};
    let mut formats=FormatRegistry::default();formats.register(format_native::NativeImpressFormat);formats.register(docx::DocxFormat);
    let dir=std::env::temp_dir().join(format!("impress-enter-format-{}",model::NodeId::new()));std::fs::create_dir_all(&dir).unwrap();
    for direct in [false,true] {
        let mut document=Document::default();let mut p=Paragraph::plain("Body αβ");let first=p.id.clone();
        document.styles.styles.get_mut(&StyleId::from(STYLE_NORMAL)).unwrap().character.font_size_pt=Some(16.0);
        if direct {if let Inline::Text(run)=&mut p.content[0] {run.properties=CharacterProperties{font_size_pt:Some(18.0),italic:Some(true),language:Some("el-GR".into()),..Default::default()};}}
        document.sections[0].blocks.push(Block::Paragraph(p));let mut e=editor::EditorState::new(document);e.set_caret(first,"Body αβ".len());e.split_paragraph().unwrap();
        let expected=if direct {18.0}else{16.0};
        for extension in ["impress","docx"] {
            let path=dir.join(format!("blank-{direct}.{extension}"));save_with_font_embedding(&path,&e.document,&formats,false).unwrap();
            let document=load_document(&path,&formats).unwrap();let paragraphs=model::document_paragraphs(&document);
            assert_eq!(paragraphs.len(),2);assert_eq!(model::inline_text(&paragraphs[1].content),"");let id=paragraphs[1].id.clone();
            let mut reopened=editor::EditorState::new(document);reopened.set_caret(id.clone(),0);
            assert_eq!(reopened.formatting_summary().character.font_size_pt,Some(expected),"{extension}, direct={direct}");
            assert_eq!(reopened.typing_properties.font_size_pt,if direct{Some(18.0)}else{None},"style sizes must remain linked");
            if direct {assert_eq!(reopened.typing_properties.italic,Some(true));assert_eq!(reopened.typing_properties.language.as_deref(),Some("el-GR"));}
            reopened.insert_text("Next α").unwrap();assert_eq!(reopened.formatting_summary().character.font_size_pt,Some(expected));
        }
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn docx_tables_with_different_cell_line_origins_open_and_paginate_after_roundtrip() {
    use model::{Block, Paragraph, Table, TableRow, TableCell};
    let mut document=Document::default();
    let first=Paragraph::plain("Different cell lines must break independently. ".repeat(90));
    let mut second=Paragraph::plain("All text remains editable after the page break. ".repeat(90));
    second.properties.space_before_pt=Some(1.0);
    let expected=[model::inline_text(&first.content),model::inline_text(&second.content)];
    let cell=|p|TableCell{id:model::NodeId::new(),blocks:vec![Block::Paragraph(p)].into(),column_span:1,row_span:1,width_mm:None,shading:None,borders:None,vertical_alignment:Default::default()};
    document.sections[0].blocks.push(Block::Table(Table{id:model::NodeId::new(),properties:Default::default(),rows:vec![TableRow{
        id:model::NodeId::new(),cells:vec![cell(first),cell(second)],repeat_as_header:false,allow_break_across_pages:true,height_mm:None}]}));
    let mut formats=FormatRegistry::default();formats.register(docx::DocxFormat);
    let dir=std::env::temp_dir().join(format!("impress-docx-tall-row-{}",model::NodeId::new()));std::fs::create_dir_all(&dir).unwrap();
    for name in ["first.docx","roundtrip.docx"] {
        let path=dir.join(name);save_document(&path,&document,&formats).unwrap();document=load_document(&path,&formats).unwrap();
        let e=editor::EditorState::new(document);let paragraphs=model::document_paragraphs(&e.document);
        assert_eq!(paragraphs.iter().map(|p|model::inline_text(&p.content)).collect::<Vec<_>>(),expected);
        let pages=layout::LayoutEngine::default().layout_document(&e.document).unwrap();assert!(pages.pages.len()>2);
        for p in paragraphs {
            let text=model::inline_text(&p.content);let mut ranges=pages.pages.iter().flat_map(|p|&p.hit_regions)
                .filter(|h|h.node_id==p.id.0).map(|h|h.byte_range.clone()).collect::<Vec<_>>();
            ranges.sort_by_key(|r|(r.start,r.end));let mut end=0;
            for range in ranges {assert!(range.start<=end,"DOCX text must not disappear at page breaks");end=end.max(range.end);}
            assert_eq!(end,text.len());
        }
        document=e.document;
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn launcher_decodes_file_uris_once_and_preserves_native_paths() {
    use std::os::unix::ffi::OsStringExt;
    let path = PathBuf::from("/tmp/100%20 Ελληνικά.docx");
    let uri = url::Url::from_file_path(&path).unwrap();
    assert_eq!(launch_path(uri.as_str().into()).unwrap(), path);
    assert_eq!(launch_path(path.clone().into_os_string()).unwrap(), path);
    let bytes = std::ffi::OsString::from_vec(b"/tmp/test\xff.docx".to_vec());
    assert_eq!(launch_path(bytes.clone()).unwrap(), PathBuf::from(bytes));
    assert!(launch_path("file://remote.example/document.docx".into()).is_err());
}
