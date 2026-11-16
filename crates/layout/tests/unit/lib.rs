use super::*;
use model::Paragraph;

#[test]
fn floating_table_is_placed_by_position_and_text_wraps_around_it(){
    use model::{HorizontalFrom,HorizontalPlacement,TablePosition,TableWrap,VerticalFrom,VerticalPlacement};
    let long="Wrapping text flows beside the floating table and continues below it. ".repeat(12);
    let cell=||TableCell{id:NodeId::new(),blocks:vec![Block::Paragraph(Paragraph::plain("Cell"))].into(),column_span:1,row_span:1,
        width_mm:None,shading:None,borders:None,vertical_alignment:CellVerticalAlignment::Top};
    let make=|position:Option<TablePosition>|{
        let table=Table{id:NodeId::new(),properties:model::TableProperties{grid_column_widths_mm:vec![25.0,25.0],position,..Default::default()},
            rows:(0..3).map(|_|model::TableRow{id:NodeId::new(),cells:vec![cell(),cell()],repeat_as_header:false,allow_break_across_pages:true,height_mm:None}).collect()};
        let mut document=Document::default();
        let table_id=table.id.0.clone();
        let text=Paragraph::plain(&long);let text_id=text.id.0.clone();
        document.sections[0].blocks=vec![Block::Table(table),Block::Paragraph(text)].into();
        (LayoutEngine::default().layout_document(&document).unwrap(),table_id,text_id)
    };
    let margin=document_left_margin();
    // Right-aligned in the margins, 10 mm below the anchor paragraph's top.
    let position=TablePosition{horizontal_from:HorizontalFrom::Margin,horizontal:HorizontalPlacement::Right,
        vertical_from:VerticalFrom::Paragraph,vertical:VerticalPlacement::Offset(10.0),wrap:TableWrap::Around,..TablePosition::default()};
    let (floating,table_id,text_id)=make(Some(position.clone()));
    let page=&floating.pages[0];
    let cells:Vec<_>=page.object_regions.iter().filter(|r|matches!(&r.kind,ObjectKind::TableCell(c) if c.table_id==table_id)).collect();
    assert_eq!(cells.len(),6);
    let left=cells.iter().map(|r|r.rect.x).fold(f32::MAX,f32::min);
    let right=cells.iter().map(|r|r.rect.x+r.rect.width).fold(f32::MIN,f32::max);
    let top=cells.iter().map(|r|r.rect.y).fold(f32::MAX,f32::min);
    let bottom=cells.iter().map(|r|r.rect.y+r.rect.height).fold(f32::MIN,f32::max);
    let content_right=page.size.width-margin;
    assert!((right-content_right).abs()<0.5,"right aligned: {right} vs {content_right}");
    // Lines beside the table stay left of it (minus the distance from text);
    // lines below it use the full width.
    let text=|page:&render::VectorPage|page.hit_regions.iter().filter(|r|r.node_id==text_id).cloned().collect::<Vec<_>>();
    let beside:Vec<_>=text(page).into_iter().filter(|r|r.rect.y+r.rect.height>top&&r.rect.y<bottom).collect();
    assert!(!beside.is_empty());
    assert!(beside.iter().all(|r|r.rect.x+r.rect.width<=left-position.distance_left_mm*MM_TO_PT+0.5));
    assert!(text(page).iter().any(|r|r.rect.y>bottom&&r.rect.x+r.rect.width>left));
    // The same table inline starts at the margin, above the text.
    let (inline,inline_id,_)=make(None);
    let first=inline.pages[0].object_regions.iter().find(|r|matches!(&r.kind,ObjectKind::TableCell(c) if c.table_id==inline_id)).unwrap();
    assert!((first.rect.x-margin).abs()<0.5);
    // Top and bottom: no text beside the table.
    let (stacked,_,stacked_text)=make(Some(TablePosition{wrap:TableWrap::TopAndBottom,..position}));
    let page=&stacked.pages[0];
    assert!(!page.hit_regions.iter().any(|r|r.node_id==stacked_text&&r.rect.y+r.rect.height>top+1.0&&r.rect.y<bottom-1.0));
}

fn document_left_margin()->f32{model::SectionProperties::default().margin_left_mm*MM_TO_PT}

#[test]
fn inline_object_marker_is_atomic_utf8_slot() {
    assert_eq!(INLINE_OBJECT_MARKER_LEN, 3);
}

#[test]
fn two_column_geometry_accounts_for_gap_and_margins() {
    let mut section=Section::default();
    section.properties.column_count=2; section.properties.column_gap_mm=10.0;
    let geometry=SectionGeometry::new(&section);
    let content_width=(section.properties.page_width_mm-section.properties.margin_left_mm-section.properties.margin_right_mm)*MM_TO_PT;
    let gap=section.properties.column_gap_mm*MM_TO_PT;
    assert!((geometry.column_width*2.0+gap-content_width).abs()<0.01);
    assert!((geometry.column_left(1)-geometry.column_left(0)-geometry.column_width-gap).abs()<0.01);
}

#[test]
fn list_marker_counters_are_stable_and_restart_per_definition() {
    let list_id=NodeId::new();
    let definition=ListDefinition { id:list_id.clone(), levels:vec![model::ListLevel {
        level:0, format:ListFormat::Decimal, text:"%1.".into(), start_at:3, left_indent_mm:6.0, hanging_indent_mm:3.0,
    }]};
    let mut section=Section::default();
    for text in ["a","b"] {
        let mut p=Paragraph::plain(text); p.properties.list_id=Some(list_id.clone()); p.properties.list_level=Some(0);
        section.blocks.push(Block::Paragraph(p));
    }
    let mut lists=BTreeMap::new(); lists.insert(list_id,definition);
    let markers=compute_list_markers(&section,&lists,&StyleSheet::default());
    let values=section.blocks.iter().filter_map(|b| if let Block::Paragraph(p)=b{markers.get(&p.id).cloned()}else{None}).collect::<Vec<_>>();
    assert_eq!(values,vec!["3.".to_string(),"4.".to_string()]);
}

#[test]
fn page_fields_use_final_context_values() {
    let context=FieldContext{page_number:7,page_count:23,section_number:2, page_number_format: model::PageNumberFormat::Decimal};
    assert_eq!(resolve_field_text(&FieldKind::PageNumber,&context),"7");
    assert_eq!(resolve_field_text(&FieldKind::PageCount,&context),"23");
    assert_eq!(resolve_field_text(&FieldKind::SectionNumber,&context),"2");
}

#[test]
fn a_row_split_across_pages_is_closed_with_padding_on_both_pages() {
    let text = "A long cell that runs on for many lines of text across the page break. ".repeat(40);
    let cell = |text: &str| model::TableCell {
        id: NodeId::new(), blocks: (vec![Block::Paragraph(Paragraph::plain(text))]).into(),
        column_span: 1, row_span: 1, width_mm: None, shading: None, borders: None,
        vertical_alignment: CellVerticalAlignment::Top,
    };
    let long = cell(&text);
    let long_id = long.id.0.to_string();
    let table = Table { id: NodeId::new(), properties: Default::default(), rows: vec![model::TableRow {
        id: NodeId::new(), cells: vec![cell("A"), long], repeat_as_header: false, allow_break_across_pages: true, height_mm: None }] };
    let mut document = Document::default();
    for _ in 0..12 { document.sections[0].blocks.push(Block::Paragraph(Paragraph::plain("Filler text before the table. ".repeat(8)))); }
    document.sections[0].blocks.push(Block::Table(table));
    let laid = LayoutEngine::default().layout_document(&document).unwrap();
    let parts: Vec<(usize, Rect)> = laid.pages.iter().enumerate()
        .flat_map(|(i, p)| p.object_regions.iter().filter(|r| r.node_id.to_string() == long_id).map(move |r| (i, r.rect))).collect();
    assert!(parts.len() >= 2, "the row should split: {parts:?}");
    let paragraph = |page: usize| laid.pages[page].hit_regions.iter().map(|h| h.rect).collect::<Vec<_>>();
    let (first_page, first) = parts[0];
    let (next_page, next) = parts[1];
    // The first part ends a padding below its last line, the rest starts a
    // padding below the top of the next page's part.
    let last_line = paragraph(first_page).into_iter().filter(|r| r.x >= first.x && r.x < first.x + first.width && r.y < first.y + first.height)
        .map(|r| r.y + r.height).fold(f32::MIN, f32::max);
    assert!((first.y + first.height - last_line - TABLE_CELL_PADDING).abs() < 0.5, "{last_line} vs {first:?}");
    let first_line = paragraph(next_page).into_iter().filter(|r| r.x >= next.x && r.x < next.x + next.width && r.y >= next.y)
        .map(|r| r.y).fold(f32::MAX, f32::min);
    assert!((first_line - next.y - TABLE_CELL_PADDING).abs() < 0.5, "{first_line} vs {next:?}");
    // Both parts draw a horizontal border at their split edge.
    let border_at = |page: usize, y: f32| laid.pages[page].commands.iter().any(|c| matches!(c,
        DrawCommand::FillRect { rect, .. } if rect.height < 2.0 && rect.width > 10.0 && (rect.y + rect.height * 0.5 - y).abs() < 0.5));
    assert!(border_at(first_page, first.y + first.height), "no closing border on the first page");
    assert!(border_at(next_page, next.y), "no opening border on the next page");
}

#[test]
fn keep_with_next_chain_uses_all_linked_paragraphs() {
    let paragraph = |keep: bool, height: f32| PreparedBlock::Paragraph {
        lines: vec![empty_line("p",0,0,height,height*0.8)],
        space_before: 0.0, space_after: 0.0, left_indent: 0.0, marker: None, decoration: None,
        keep_with_next: keep, keep_lines_together: false,
        page_break_before: false, widow_control: true,
    };
    let blocks = vec![paragraph(true,10.0), paragraph(true,12.0), paragraph(false,14.0)];
    let height = minimum_keep_chain_height(&blocks,0).unwrap();
    assert!((height-36.0).abs()<0.01);
}

#[test]
fn widow_control_moves_three_line_paragraph_when_fresh_column_fits_all_lines() {
    let geometry=SectionGeometry::single_column(100.0,100.0,0.0,0.0,40.0,100.0);
    let lines=vec![empty_line("p",0,0,10.0,8.0),empty_line("p",0,1,10.0,8.0),empty_line("p",0,2,10.0,8.0)];
    let mut pages=vec![new_page(100.0,100.0,false)];
    let mut cursor=FlowCursor{page_index:0,column_index:0,y:15.0};
    paginate_paragraph(&lines,0.0,0.0,0.0,None,None,false,true,&mut pages,&mut cursor,&geometry,false).unwrap();
    assert_eq!(pages.len(),2);
    assert_eq!(cursor.page_index,1);
    assert!((cursor.y-30.0).abs()<0.01);
}


#[test]
fn embedded_fonts_shape_body_headers_notes_and_parallel_table_cells() {
    let mut document=Document::default();let id=model::ResourceId::new();
    document.resources.insert(id.clone(),model::Resource{id,media_type:typography::EMBEDDED_FONT_MEDIA_TYPE.into(),original_name:None,
        font_faces:vec![model::EmbeddedFontFace{family:"Impress Layout Test Alias".into(),bold:false,italic:false,face_index:0}],
        data:typst_assets::fonts().next().unwrap().to_vec().into()});
    // A nonexistent family proves that shaping uses the document binding. The
    // resulting width must agree with the isolated embedded face, not fallback.
    let mut paragraph=Paragraph::plain("Hello");
    if let Inline::Text(run)=&mut paragraph.content[0]{run.properties.font_family=Some("Impress Layout Test Alias".into());run.properties.font_size_pt=Some(12.0);}
    let note=NodeId::new();
    document.review.footnotes.insert(note.clone(),model::Footnote{id:note.clone(),kind:model::NoteKind::Footnote,blocks:vec![Block::Paragraph(paragraph.clone())].into()});
    let mut body=paragraph.clone();body.content.push(Inline::Field(model::Field{id:NodeId::new(),kind:model::FieldKind::FootnoteReference{note},display_text:None,color:None}));
    document.sections[0].blocks.push(Block::Paragraph(body));
    document.sections[0].headers.default=Some(model::HeaderFooter{id:NodeId::new(),blocks:vec![Block::Paragraph(paragraph.clone())].into()});
    let cell=||model::TableCell{id:NodeId::new(),blocks:vec![Block::Paragraph(paragraph.clone())].into(),column_span:1,row_span:1,width_mm:None,shading:None,borders:None,vertical_alignment:CellVerticalAlignment::Top};
    document.sections[0].blocks.push(Block::Table(model::Table{id:NodeId::new(),properties:Default::default(),rows:(0..4).map(|_|model::TableRow{id:NodeId::new(),cells:vec![cell()],repeat_as_header:false,allow_break_across_pages:true,height_mm:None}).collect()}));
    let layout=LayoutEngine::default().layout_document(&document).unwrap();
    let fonts=FontCatalog::for_resources(&document.resources);let mut props=CharacterProperties::default();props.font_family=Some("Impress Layout Test Alias".into());props.font_size_pt=Some(12.0);
    let expected=shape_token("Hello",&props,&fonts).unwrap();
    assert!(expected.advance_pt>0.0);
    let mut widths=Vec::new();
    fn collect(commands:&[DrawCommand],out:&mut Vec<f32>){for command in commands{match command{DrawCommand::TextRun(run) if run.actual_text.as_deref()==Some("Hello")=>out.push(run.advance_pt),DrawCommand::Clipped{command,..}=>collect(std::slice::from_ref(command.as_ref()),out),_=>{}}}}
    for page in &layout.pages{collect(&page.commands,&mut widths);}
    assert!(widths.len()>=7,"body, header, note, and four table cells: {widths:?}");
    for width in widths{assert!((width-expected.advance_pt).abs()<0.1,"{width} vs {}",expected.advance_pt);}
}

#[test]
fn neo_aegean_printable_glyphs_have_visible_outlines() {
    let samples=[
        include_bytes!("../../../typography/tests/fixtures/neo-aegean/NeoAegean-Regular.otf").as_slice(),
        include_bytes!("../../../typography/tests/fixtures/neo-aegean/NeoAegean-Italic.otf").as_slice(),
        include_bytes!("../../../typography/tests/fixtures/neo-aegean/NeoAegean-CompactLight.otf").as_slice(),
        include_bytes!("../../../typography/tests/fixtures/neo-aegean/NeoAegean-CompactLightItalic.otf").as_slice(),
        include_bytes!("../../../typography/tests/fixtures/neo-aegean/NeoAegean-CompactBold.otf").as_slice(),
        include_bytes!("../../../typography/tests/fixtures/neo-aegean/NeoAegean-CompactBoldItalic.otf").as_slice(),
        include_bytes!("../../../typography/tests/fixtures/neo-aegean/NeoAegean-Regular.ttf").as_slice(),
        include_bytes!("../../../typography/tests/fixtures/neo-aegean/NeoAegean-Italic.ttf").as_slice(),
        include_bytes!("../../../typography/tests/fixtures/neo-aegean/NeoAegean-CompactLight.ttf").as_slice(),
        include_bytes!("../../../typography/tests/fixtures/neo-aegean/NeoAegean-CompactLightItalic.ttf").as_slice(),
        include_bytes!("../../../typography/tests/fixtures/neo-aegean/NeoAegean-CompactBold.ttf").as_slice(),
        include_bytes!("../../../typography/tests/fixtures/neo-aegean/NeoAegean-CompactBoldItalic.ttf").as_slice()];
    for (index,bytes) in samples.iter().enumerate(){
        let fonts=FontCatalog::from_font_data(bytes.to_vec());let name=fonts.family_names()[0].clone();let resolved=fonts.resolve(&name,false,false).unwrap();
        fonts.with_face(resolved,|face|{for ch in "ABCabcαβγἄνθρωπος".chars(){let glyph=face.glyph_index(ch).unwrap();
            let outline=render::glyph_outline_from_face(face,glyph.0 as u32).unwrap_or_else(|e|panic!("face {index}, char {ch}: {e}"));
            assert!(!outline.path.verbs.is_empty(),"face {index}, char {ch}");
        }}).unwrap();
    }
}

#[test]
fn neo_aegean_text_reaches_page_pixels_for_each_face() {
    use std::sync::atomic::AtomicBool;
    let samples=[
        include_bytes!("../../../typography/tests/fixtures/neo-aegean/NeoAegean-Regular.otf").as_slice(),
        include_bytes!("../../../typography/tests/fixtures/neo-aegean/NeoAegean-Italic.otf").as_slice(),
        include_bytes!("../../../typography/tests/fixtures/neo-aegean/NeoAegean-CompactLight.otf").as_slice(),
        include_bytes!("../../../typography/tests/fixtures/neo-aegean/NeoAegean-CompactLightItalic.otf").as_slice(),
        include_bytes!("../../../typography/tests/fixtures/neo-aegean/NeoAegean-CompactBold.otf").as_slice(),
        include_bytes!("../../../typography/tests/fixtures/neo-aegean/NeoAegean-CompactBoldItalic.otf").as_slice(),
        include_bytes!("../../../typography/tests/fixtures/neo-aegean/NeoAegean-Regular.ttf").as_slice(),
        include_bytes!("../../../typography/tests/fixtures/neo-aegean/NeoAegean-Italic.ttf").as_slice(),
        include_bytes!("../../../typography/tests/fixtures/neo-aegean/NeoAegean-CompactLight.ttf").as_slice(),
        include_bytes!("../../../typography/tests/fixtures/neo-aegean/NeoAegean-CompactLightItalic.ttf").as_slice(),
        include_bytes!("../../../typography/tests/fixtures/neo-aegean/NeoAegean-CompactBold.ttf").as_slice(),
        include_bytes!("../../../typography/tests/fixtures/neo-aegean/NeoAegean-CompactBoldItalic.ttf").as_slice()];
    let mut previous=Vec::new();
    for (index,bytes) in samples.iter().enumerate(){
        if index==6 {previous.clear();}
        let mut document=Document::default();let id=model::ResourceId::new();let variant=index%6;let family=if variant>=4{"Neo Aegean Compact"}else if variant>=2{"Neo Aegean Compact Light"}else{"Neo Aegean"}.to_owned();
        document.resources.insert(id.clone(),model::Resource{id,media_type:typography::EMBEDDED_FONT_MEDIA_TYPE.into(),original_name:None,
            font_faces:vec![model::EmbeddedFontFace{family:family.clone(),bold:variant>=4,italic:variant%2==1,face_index:0}],data:bytes.to_vec().into()});
        let mut paragraph=Paragraph::plain("Neo Aegean ABC abc office ἄνθρωπος");if let Inline::Text(run)=&mut paragraph.content[0]{run.properties.font_family=Some(family);run.properties.font_size_pt=Some(11.0);run.properties.bold=Some(variant>=4);run.properties.italic=Some(variant%2==1);run.properties.typography=Some(model::OpenTypeTypography{kerning_mode:model::KerningMode::Optical,ligatures:model::LigatureMode::Discretionary,contextual_ligatures:true,..Default::default()});}
        document.sections[0].blocks.push(Block::Paragraph(paragraph));let page=LayoutEngine::default().layout_document(&document).unwrap().pages.remove(0);
        let glyphs:Vec<_>=page.commands.iter().filter_map(|c|match c.unclipped().0{DrawCommand::TextRun(r)=>Some(r.glyphs.len()),_=>None}).collect();assert!(glyphs.iter().sum::<usize>()>=13,"face {index}, {glyphs:?}");
        let pixels=output::raster::BandRenderer::default().render(&page,700,900,Affine2D::IDENTITY,&AtomicBool::new(false)).unwrap();
        assert!(pixels.chunks_exact(4).filter(|pixel|pixel[0]<160&&pixel[1]<160&&pixel[2]<160).count()>100,"face {index} has no visible text");
        assert!(!previous.contains(&pixels),"face {index} reused another face's pixels");previous.push(pixels);
    }
}

#[test]
fn outline_cache_keeps_different_font_catalogs_separate() {
    let a=FontCatalog::from_font_data(include_bytes!("../../../typography/tests/fixtures/neo-aegean/NeoAegean-Regular.otf").to_vec());
    let b=FontCatalog::from_font_data(include_bytes!("../../../typography/tests/fixtures/neo-aegean/NeoAegean-Italic.otf").to_vec());
    let fa=a.resolve("Neo Aegean",false,false).unwrap();let fb=b.resolve("Neo Aegean",false,true).unwrap();assert_eq!(fa.id,fb.id);
    let outline=|fonts:&FontCatalog,font|fonts.with_face(font,|face|{let glyph=face.glyph_index('A').unwrap().0 as u32;scaled_outline(face,font,glyph,24.0,1.0).unwrap()}).unwrap();
    let regular=outline(&a,fa);let italic=outline(&b,fb);
    assert_ne!(*regular,*italic,"The same local glyph ID must not reuse another face's outline");
    assert!(Arc::ptr_eq(&regular,&outline(&a,fa)));assert!(Arc::ptr_eq(&italic,&outline(&b,fb)));
}

#[test]
fn justified_greek_lines_reach_the_right_margin_without_stretching_the_last_line() {
    let text="Από πότε που δημιουργήθηκαν, οι Ηνωμένες Πολιτείες αποφάσιζαν";
    let fonts=FontCatalog::from_font_data(include_bytes!("../../../typography/tests/fixtures/neo-aegean/NeoAegean-Regular.otf").to_vec());
    let mut ctx=WorkerContext{fonts,typst:TypstTypesetter::new()};
    let mut styles=StyleSheet::default();
    for style in styles.styles.values_mut() {
        style.character.font_family=Some("Neo Aegean".into());
        style.character.font_size_pt=Some(20.0);
        style.paragraph.hyphenation=Some(false);
        style.paragraph.alignment=Some(ParagraphAlignment::Justify);
        style.paragraph.first_line_indent_mm=Some(0.0);
        style.paragraph.glyph_expansion_per_mille=Some(0);
        style.paragraph.hanging_punctuation=Some(false);
    }
    let paragraph=Paragraph::plain(text);
    let fields=FieldContext{page_number:1,page_count:1,section_number:1,page_number_format:model::PageNumberFormat::Decimal};
    for width in [300.0,350.0,400.0,450.0,500.0] {
        let PreparedBlock::Paragraph{lines,..}=prepare_paragraph(&paragraph,width,&styles,&mut ctx,&BTreeMap::new(),None,&fields).unwrap() else {panic!()};
        assert!(lines.len()>1);
        for (i,line) in lines.iter().enumerate() {
            let right=line.hit_regions.iter().filter(|h| text.get(h.byte_range.clone()).is_some_and(|s|!s.trim().is_empty()))
                .map(|h|h.rect.max_x()).fold(0.0,f32::max);
            if i+1<lines.len() {assert!((right-width).abs()<0.05,"width {width}, line {i}: right {right}");}
            else {assert!(right<width-1.0,"last line must retain natural spacing");}
        }
        let bytes=lines.iter().flat_map(|l|l.hit_regions.iter()).map(|h|h.byte_range.len()).sum::<usize>();
        assert_eq!(bytes,text.len());
    }
}

#[test]
fn round_picture_mask_and_outline_reach_the_shared_render_scene() {
    let source = image::RgbaImage::from_pixel(64, 64, image::Rgba([220, 140, 60, 255]));
    let mut encoded = std::io::Cursor::new(Vec::new());
    source.write_to(&mut encoded, image::ImageFormat::Png).unwrap();
    let resource = Resource { id: ResourceId::new(), media_type: "image/png".into(), data: encoded.into_inner().into(), original_name: None, font_faces: Vec::new() };
    let original = resource.data.clone();
    let effects = model::ImageEffects {
        shape: model::ImageShape { preset: "ellipse".into(), ..Default::default() },
        outline: Some(Border { width_pt: 2.0, color: "#6B817A".into(), style: BorderStyle::Single }),
        ..Default::default()
    };
    let commands = raster_preview_commands(&resource, 100.0, 100.0, &effects).unwrap();
    let DrawCommand::Image { image, .. } = &commands[0] else { panic!("masked bitmap") };
    assert_eq!(image.pixels[3], 0);
    assert_eq!(image.pixels[(32 * 64 + 32) * 4 + 3], 255);
    assert!(commands.iter().any(|command| matches!(command, DrawCommand::Path(path) if path.stroke.as_ref().is_some_and(|(_, style)| style.width_pt == 2.0))));
    assert_eq!(resource.data, original);
}

#[test]
fn ruler_margin_metadata_follows_sections_including_intentional_blank_pages() {
    let mut document = Document::default();
    document.sections[0].properties.margin_left_mm = 30.0;
    document.sections[0].properties.margin_top_mm = 22.0;
    document.sections[0].blocks = vec![Block::Paragraph(Paragraph::plain("First"))].into();
    let mut second = model::Section::default();
    second.properties.page_width_mm = 297.0;
    second.properties.page_height_mm = 210.0;
    second.properties.margin_left_mm = 15.0;
    second.properties.margin_right_mm = 35.0;
    second.properties.margin_top_mm = 18.0;
    second.properties.margin_bottom_mm = 28.0;
    second.properties.start_on = model::SectionStart::OddPage;
    second.blocks = vec![Block::Paragraph(Paragraph::plain("Second"))].into();
    document.sections.push(second);
    let laid = LayoutEngine::default().layout_document(&document).unwrap();
    assert_eq!(laid.pages.len(), 3);
    let first = laid.pages[0].margin_rect.unwrap();
    assert!((first.x - 30.0 * MM_TO_PT).abs() < 0.001);
    assert!((first.y - 22.0 * MM_TO_PT).abs() < 0.001);
    for page in &laid.pages[1..] {
        let rect = page.margin_rect.unwrap();
        assert!((rect.x - 15.0 * MM_TO_PT).abs() < 0.001);
        assert!((rect.y - 18.0 * MM_TO_PT).abs() < 0.001);
        assert!((page.size.width - rect.max_x() - 35.0 * MM_TO_PT).abs() < 0.001);
        assert!((page.size.height - rect.max_y() - 28.0 * MM_TO_PT).abs() < 0.001);
    }
}
