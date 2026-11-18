//! Behavioral tests of the REAL layout/scene pipeline. The original synthetic
//! OpenType face is generated in memory; no font file is bundled or installed.
use super::*;
use model::{TableProperties, TableRow};
fn context() -> WorkerContext {
    WorkerContext {
        fonts: FontCatalog::from_font_data(ligature_fixture::font()),
        typst: TypstTypesetter::new(),
    }
}
fn styles() -> StyleSheet {
    let mut s = StyleSheet::default();
    for style in s.styles.values_mut() {
        style.character.font_family = Some(ligature_fixture::FAMILY.into());
        style.paragraph.hyphenation = Some(false);
        style.paragraph.space_after_pt = Some(0.0);
    }
    s
}
fn fields() -> FieldContext {
    FieldContext {
        page_number: 1,
        page_count: 1,
        section_number: 1,
        page_number_format: model::PageNumberFormat::Decimal,
    }
}
fn cell(blocks: Vec<Block>) -> TableCell {
    TableCell {
        id: NodeId::new(),
        blocks: blocks.into(),
        column_span: 1,
        row_span: 1,
        width_mm: None,
        shading: None,
        borders: None,
        vertical_alignment: CellVerticalAlignment::Top,
    }
}
fn row(cells: Vec<TableCell>) -> TableRow {
    TableRow {
        id: NodeId::new(),
        cells,
        repeat_as_header: false,
        allow_break_across_pages: true,
        height_mm: None,
    }
}
fn table(rows: Vec<TableRow>) -> Table {
    Table {
        id: NodeId::new(),
        properties: TableProperties::default(),
        rows,
    }
}
fn prepare(p: &Paragraph, width: f32) -> Vec<PreparedLine> {
    let PreparedBlock::Paragraph { lines, .. } = prepare_paragraph(
        p,
        width,
        &styles(),
        &mut context(),
        &BTreeMap::new(),
        None,
        &fields(),
    )
    .unwrap() else {
        panic!("paragraph expected")
    };
    lines
}
fn command_text(commands: &[DrawCommand]) -> String {
    commands
        .iter()
        .filter_map(|c| match c.unclipped().0 {
            DrawCommand::TextRun(r) => r.actual_text.as_deref(),
            _ => None,
        })
        .collect()
}
#[test]
fn unbroken_text_wraps_and_keeps_every_source_byte() {
    let text = "AVfist".repeat(100);
    let p = Paragraph::plain(&text);
    let lines = prepare(&p, 70.0);
    assert!(lines.len() > 10);
    assert_eq!(
        lines
            .iter()
            .map(|l| command_text(&l.commands))
            .collect::<String>(),
        text
    );
    for line in &lines {
        for h in &line.hit_regions {
            assert!(h.rect.x >= -0.01);
            assert!(h.rect.max_x() <= 70.02, "{:?}", h.rect);
        }
    }
}
fn geometry() -> SectionGeometry<'static> {
    SectionGeometry::single_column(160.0, 180.0, 10.0, 10.0, 170.0, 140.0)
}
#[test]
fn unsplittable_oversized_rows_error_instead_of_painting_off_page() {
    let text = "AVfist ".repeat(100);
    let mut t = table(vec![row(vec![cell(vec![Block::Paragraph(
        Paragraph::plain(&text),
    )])])]);
    t.rows[0].allow_break_across_pages = false;
    let prepared = prepare_table(
        &t,
        140.0,
        &styles(),
        &mut context(),
        &BTreeMap::new(),
        &fields(),
    )
    .unwrap();
    let g = geometry();
    let mut pages = vec![new_page(g.page_width, g.page_height, false)];
    let mut cursor = FlowCursor {
        page_index: 0,
        column_index: 0,
        y: g.content_top,
    };
    assert!(paginate_table(&prepared, &mut pages, &mut cursor, &g, false).is_err());
}

#[test]
fn inline_pagination_controls_move_following_text_without_splitting_the_model_paragraph() {
    let mut paragraph = Paragraph::plain("AV");
    paragraph.content.push(Inline::PaginationBreak {
        id: NodeId::new(),
        break_kind: model::PaginationBreakKind::Page,
    });
    paragraph
        .content
        .push(Inline::Text(model::TextRun::plain("fi")));
    paragraph.content.push(Inline::PaginationBreak {
        id: NodeId::new(),
        break_kind: model::PaginationBreakKind::Column,
    });
    paragraph
        .content
        .push(Inline::Text(model::TextRun::plain("st")));
    let lines = prepare(&paragraph, 140.0);
    let g = geometry();
    let mut pages = vec![new_page(g.page_width, g.page_height, false)];
    let mut cursor = FlowCursor {
        page_index: 0,
        column_index: 0,
        y: g.content_top,
    };
    paginate_paragraph(
        &lines,
        0.0,
        0.0,
        0.0,
        None,
        None,
        false,
        false,
        &mut pages,
        &mut cursor,
        &g,
        false,
    )
    .unwrap();
    assert_eq!(pages.len(), 3);
    assert_eq!(
        pages
            .iter()
            .map(|page| command_text(&page.commands))
            .collect::<Vec<_>>(),
        vec!["AV", "fi", "st"]
    );
}
#[test]
fn inline_column_break_uses_next_column_without_an_extra_page() {
    let mut paragraph = Paragraph::plain("AV");
    paragraph.content.push(Inline::PaginationBreak {
        id: NodeId::new(),
        break_kind: model::PaginationBreakKind::Column,
    });
    paragraph
        .content
        .push(Inline::Text(model::TextRun::plain("fi")));
    let mut section = Section::default();
    section.properties.column_count = 2;
    let geometry = SectionGeometry::new(&section);
    let lines = prepare(&paragraph, geometry.column_width);
    let mut pages = vec![new_page(geometry.page_width, geometry.page_height, false)];
    let mut cursor = FlowCursor {
        page_index: 0,
        column_index: 0,
        y: geometry.content_top,
    };
    paginate_paragraph(
        &lines,
        0.0,
        0.0,
        0.0,
        None,
        None,
        false,
        false,
        &mut pages,
        &mut cursor,
        &geometry,
        false,
    )
    .unwrap();
    assert_eq!(pages.len(), 1);
    assert_eq!(command_text(&pages[0].commands), "AVfi");
    let hits = &pages[0].hit_regions;
    assert!(hits[1].rect.x > hits[0].rect.x + geometry.column_width);
    assert!((hits[0].rect.y - hits[1].rect.y).abs() < 0.01);
}

#[test]
fn blank_paragraph_line_and_caret_keep_the_paragraph_marks_size() {
    for size in [8.0, 18.0, 32.0] {
        let mut p=Paragraph::plain("");
        if let Inline::Text(run)=&mut p.content[0] {run.properties.font_size_pt=Some(size);}
        let lines=prepare(&p,300.0);assert_eq!(lines.len(),1);
        assert!((lines[0].height-size*1.2).abs()<0.01);
        assert!((lines[0].ascent-size).abs()<0.01);
        assert!((lines[0].hit_regions[0].rect.height-size*1.2).abs()<0.01);
    }
}

#[test]
fn a_tall_row_with_offset_cell_lines_paginates_without_losing_text() {
    let long = "AVfist ".repeat(350);
    let first = Paragraph::plain(&long);let first_id=first.id.0.clone();
    let mut second = Paragraph::plain(&long);let second_id=second.id.0.clone();
    // The same line spacing with a different paragraph origin has no shared
    // horizontal split between cells, despite both having many legal breaks.
    second.properties.space_before_pt=Some(1.0);
    let t=table(vec![row(vec![cell(vec![Block::Paragraph(first)]),cell(vec![Block::Paragraph(second)])])]);
    let prepared=prepare_table(&t,140.0,&styles(),&mut context(),&BTreeMap::new(),&fields()).unwrap();
    let g=geometry();let mut pages=vec![new_page(g.page_width,g.page_height,false)];
    let mut cursor=FlowCursor{page_index:0,column_index:0,y:g.content_top};
    paginate_table(&prepared,&mut pages,&mut cursor,&g,false).unwrap();
    assert!(pages.len()>2);
    for id in [first_id,second_id] {
        let mut expected=prepared.rows[0].cells.iter().flat_map(|c|&c.hit_regions).filter(|h|h.node_id==id)
            .map(|h|(h.byte_range.start,h.byte_range.end,h.visual_line)).collect::<Vec<_>>();
        let mut actual=pages.iter().flat_map(|p|&p.hit_regions).filter(|h|h.node_id==id)
            .map(|h|(h.byte_range.start,h.byte_range.end,h.visual_line)).collect::<Vec<_>>();
        expected.sort_unstable();actual.sort_unstable();assert_eq!(actual,expected,"text/caret ranges must appear exactly once");
    }
    for page in &pages {for region in &page.object_regions {
        assert!(region.rect.max_y()<=g.content_bottom+0.02);
    }}
}

#[test]
fn independent_cell_breaks_keep_short_cell_once_and_repeat_headers_within_the_page() {
    let mut header=row((0..3).map(|_|cell(vec![Block::Paragraph(Paragraph::plain("Header"))])).collect());header.repeat_as_header=true;
    let first=Paragraph::plain("AVfist ".repeat(160));
    let short=Paragraph::plain("AV");let short_id=short.id.0.clone();
    let mut last=Paragraph::plain("AVfist ".repeat(130));last.properties.space_before_pt=Some(1.0);
    let t=table(vec![header,row(vec![cell(vec![Block::Paragraph(first)]),cell(vec![Block::Paragraph(short)]),cell(vec![Block::Paragraph(last)])])]);
    let prepared=prepare_table(&t,140.0,&styles(),&mut context(),&BTreeMap::new(),&fields()).unwrap();
    let g=geometry();let mut pages=vec![new_page(g.page_width,g.page_height,false)];let mut cursor=FlowCursor{page_index:0,column_index:0,y:g.content_top};
    paginate_table(&prepared,&mut pages,&mut cursor,&g,false).unwrap();
    assert!(pages.len()>2 && pages.len()<100,"continuations must make progress");
    let short_hits=pages.iter().flat_map(|p|&p.hit_regions).filter(|h|h.node_id==short_id).count();
    assert_eq!(short_hits,prepared.rows[1].cells[1].hit_regions.len(),"finished cells cannot duplicate text on later pages");
    let header_id=prepared.rows[0].cells[0].own_region.node_id.clone();
    for page in &pages {
        assert_eq!(page.object_regions.iter().filter(|r|r.node_id==header_id).count(),1);
        for r in &page.object_regions { assert!(r.rect.max_y()<=g.content_bottom+0.02); }
    }
}

fn document_with_tall_picture(height: f32) -> (Document, NodeId, NodeId, NodeId) {
    let mut doc = Document::default();
    let section = &mut doc.sections[0];
    section.properties.page_height_mm = 279.4;
    section.properties.margin_top_mm = 25.4;
    section.properties.margin_bottom_mm = 25.4;
    let before = Paragraph::plain("Before picture");
    let after = Paragraph::plain("After picture");
    let mut picture = Paragraph::plain("");
    picture.content = vec![Inline::Image(model::InlineImage {
        id: NodeId::new(), resource: ResourceId::new(), width_mm: 128.87,
        height_mm: height, alt_text: None, effects: Default::default(),
    })].into();
    let ids = (before.id.clone(), picture.id.clone(), after.id.clone());
    section.blocks = vec![Block::Paragraph(before), Block::Paragraph(picture), Block::Paragraph(after)].into();
    (doc, ids.0, ids.1, ids.2)
}

#[test]
fn picture_exactly_as_tall_as_printable_body_retains_its_size() {
    let (doc, before, picture, after) = document_with_tall_picture(228.6);
    let pages = LayoutEngine::default().layout_document(&doc).unwrap().pages;
    assert_eq!(pages.len(), 3);
    assert!(pages[0].hit_regions.iter().any(|h| h.node_id == before.0));
    let image = pages[1].hit_regions.iter().find(|h| h.node_id == picture.0).unwrap();
    assert!((image.rect.height - 228.6 * MM_TO_PT).abs() < 0.01);
    assert!(pages[2].hit_regions.iter().any(|h| h.node_id == after.0));
}

#[test]
fn unplaceable_picture_keeps_surrounding_paragraphs_in_the_editing_view() {
    let (doc, before, picture, after) = document_with_tall_picture(350.0);
    let original = doc.clone();
    let engine = LayoutEngine::default();
    assert!(engine.layout_document(&doc).is_err());
    let pages = engine.layout_document_pass(&doc, None, &mut Default::default(), true).unwrap().pages;
    let hits: Vec<_> = pages.iter().flat_map(|p| &p.hit_regions).collect();
    for id in [before, picture, after] {
        assert!(hits.iter().any(|h| h.node_id == id.0), "missing source {:?}", id);
    }
    assert_eq!(pages.len(), 1, "discard the partial placement of the failed picture");
    assert_eq!(doc, original);
}
