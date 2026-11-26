use super::*;
use crate::{LayoutEngine, LayoutOptions, LayoutScheduler};
use model::{AnchorRange, TextAnchor, TrackedChange};

fn deletion(document: &mut Document, id: &NodeId, offset: usize, text: &str, order: u64) {
    let change = NodeId::new();
    let anchor = TextAnchor {
        paragraph: id.clone(),
        offset,
    };
    let mut run = model::TextRun::plain(text);
    run.properties.font_size_pt = Some(16.0);
    document.review.tracked_changes.insert(
        change.clone(),
        TrackedChange {
            id: change,
            author: "Reviewer".into(),
            date: None,
            source_id: order.to_string(),
            order,
            status: RevisionStatus::Pending,
            effects: vec![RevisionEffect::Inline {
                range: AnchorRange {
                    start: anchor.clone(),
                    end: anchor,
                },
                before: vec![Inline::Text(run)],
                after: vec![],
                change: InlineRevisionKind::Deletion,
                move_name: None,
            }],
        },
    );
}

fn fixture() -> (Document, NodeId) {
    let mut document = Document::default();
    let paragraph = Paragraph::plain("α γ tail");
    let id = paragraph.id.clone();
    document.sections[0].blocks = vec![Block::Paragraph(paragraph)].into();
    deletion(&mut document, &id, "α ".len(), "β\" e\u{301}  ", 1);
    (document, id)
}

#[test]
fn deleted_runs_keep_full_text_format_and_live_editing_offsets() {
    let (document, id) = fixture();
    let original = document.clone();
    let projection = ReviewProjection::new(&document);
    let paragraphs = model::document_paragraphs(&projection.document);
    assert_eq!(
        model::inline_text(&paragraphs[0].content),
        "α β\" e\u{301}  γ tail"
    );
    let removed = paragraphs[0]
        .content
        .iter()
        .find_map(|inline| match inline {
            Inline::Text(run) if run.text == "β\" e\u{301}  " => Some(run),
            _ => None,
        })
        .unwrap();
    assert_eq!(removed.properties.font_size_pt, Some(16.0));
    assert_eq!(
        projection
            .document
            .styles
            .resolve_formatting(removed.style.as_ref())
            .get("fnt_strike")
            .map(String::as_str),
        Some("true")
    );
    let mut scene = LayoutEngine::default()
        .layout_document(&projection.document)
        .unwrap();
    assert!(
        scene
            .pages
            .iter()
            .flat_map(|page| &page.commands)
            .any(|command| {
                matches!(command.unclipped().0, DrawCommand::FillRect { rect, paint }
            if rect.height < 2.0 && rect.width > 5.0
                && *paint == render::Paint::Solid(render::Color::rgb8(202, 56, 59)))
            }),
        "the deleted text must have a drawn strikethrough line"
    );
    projection.remap_scene(&mut scene);
    let text = "α γ tail";
    let hits: Vec<_> = scene
        .pages
        .iter()
        .flat_map(|page| &page.hit_regions)
        .filter(|hit| hit.node_id == id.0)
        .collect();
    assert!(
        hits.iter()
            .any(|hit| hit.byte_range == ("α ".len().."α ".len()))
    );
    for hit in hits {
        assert!(
            text.is_char_boundary(hit.byte_range.start)
                && text.is_char_boundary(hit.byte_range.end)
        );
        for stop in &hit.caret_stops {
            assert!(text.is_char_boundary(stop.byte_offset));
        }
    }
    assert_eq!(
        document, original,
        "displaying revisions must not alter the saved/editable document"
    );
}

#[test]
fn exact_word_deletions_supersede_net_and_history_snapshots() {
    let (mut document, id) = fixture();
    let before = Paragraph::plain("α β\" e\u{301}  γ tail");
    let revision = NodeId::new();
    document.review.revisions.insert(
        revision.clone(),
        model::TextRevision {
            id: revision,
            paragraph: id.clone(),
            author: "Reviewer".into(),
            date: None,
            before: before.clone(),
        },
    );
    let current = model::document_paragraphs(&document)[0].clone();
    document.review.history.push(model::RevisionEvent {
        id: NodeId::new(),
        author: "Reviewer".into(),
        date: "2026-11-26".into(),
        label: "Delete".into(),
        status: RevisionStatus::Pending,
        payload:
            serde_json::json!([{"ParagraphChanged":{"id":id,"before":before,"after":current}}])
                .into(),
    });
    let projection = ReviewProjection::new(&document);
    assert_eq!(
        model::inline_text(&model::document_paragraphs(&projection.document)[0].content),
        "α β\" e\u{301}  γ tail"
    );
}

#[test]
fn nearby_deletions_are_ordered_and_are_never_truncated_or_collapsed() {
    let mut document = Document::default();
    let paragraph = Paragraph::plain("leftright");
    let id = paragraph.id.clone();
    document.sections[0].blocks = vec![Block::Paragraph(paragraph)].into();
    deletion(&mut document, &id, 4, "two ", 2);
    let long = "one  \"β\" ".repeat(25);
    deletion(&mut document, &id, 4, &long, 1);
    let projection = ReviewProjection::new(&document);
    assert_eq!(
        model::inline_text(&model::document_paragraphs(&projection.document)[0].content),
        format!("left{long}two right")
    );
}

#[test]
fn native_net_and_journal_changes_show_full_deleted_graphemes() {
    for journal in [false, true] {
        let mut document = Document::default();
        let current = Paragraph::plain("α γ");
        let id = current.id.clone();
        document.sections[0].blocks = vec![Block::Paragraph(current.clone())].into();
        let before = Paragraph::plain("α e\u{301} β γ終");
        if journal {
            document.review.history.push(model::RevisionEvent {
                id: NodeId::new(), author: "Reviewer".into(), date: "2026-11-26".into(),
                label: "Delete".into(), status: RevisionStatus::Pending,
                payload: serde_json::json!([{"ParagraphChanged":{"id":id,"before":before,"after":current}}]).into(),
            });
        } else {
            let revision = NodeId::new();
            document.review.revisions.insert(
                revision.clone(),
                model::TextRevision {
                    id: revision,
                    paragraph: id,
                    author: "Reviewer".into(),
                    date: None,
                    before,
                },
            );
        }
        let projection = ReviewProjection::new(&document);
        assert_eq!(
            model::inline_text(&model::document_paragraphs(&projection.document)[0].content),
            "α e\u{301} β γ終"
        );
    }
}

#[test]
fn resolved_deletions_are_absent_from_all_markup() {
    for status in [RevisionStatus::Accepted, RevisionStatus::Rejected] {
        let (mut document, _) = fixture();
        document
            .review
            .tracked_changes
            .values_mut()
            .next()
            .unwrap()
            .status = status;
        let projection = ReviewProjection::new(&document);
        assert_eq!(
            model::inline_text(&model::document_paragraphs(&projection.document)[0].content),
            "α γ tail"
        );
    }
}

#[test]
fn all_markup_reflows_inside_table_cells_and_simple_markup_restores_final_geometry() {
    use model::{Table, TableCell, TableRow};
    let (mut document, id) = fixture();
    document.sections[0].properties.page_width_mm = 100.0;
    document.sections[0].properties.page_height_mm = 140.0;
    document.sections[0].properties.margin_left_mm = 10.0;
    document.sections[0].properties.margin_right_mm = 10.0;
    document.sections[0].properties.margin_top_mm = 10.0;
    document.sections[0].properties.margin_bottom_mm = 10.0;
    let blocks = document.sections[0].blocks.clone();
    document.sections[0].blocks = vec![Block::Table(Table {
        id: NodeId::new(),
        properties: Default::default(),
        rows: vec![TableRow {
            id: NodeId::new(),
            repeat_as_header: false,
            allow_break_across_pages: true,
            height_mm: None,
            cells: vec![TableCell {
                id: NodeId::new(),
                blocks,
                column_span: 1,
                row_span: 1,
                width_mm: None,
                shading: None,
                borders: None,
                vertical_alignment: Default::default(),
                margins: None,
                text_direction: None,
            }],
        }],
    })]
    .into();
    deletion(
        &mut document,
        &id,
        "α γ".len(),
        &" deleted words ".repeat(40),
        2,
    );
    let source = Arc::new(document);
    let scheduler = LayoutScheduler::new(LayoutOptions::default()).unwrap();
    let receive = || {
        let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            if let Some(result) = scheduler.try_recv() {
                return result.layout.unwrap().document;
            }
            assert!(std::time::Instant::now() < end, "layout timed out");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    };
    scheduler.submit_for_display(1, source.clone(), 0..20, true);
    let all = receive();
    scheduler.submit_for_display(2, source.clone(), 0..20, false);
    let simple = receive();
    let final_scene = LayoutEngine::default().layout_document(&source).unwrap();
    assert_eq!(
        &*simple, &final_scene,
        "Simple Markup uses final text, without hidden deletion space"
    );
    assert!(
        all.pages.len() > simple.pages.len(),
        "deleted text must consume real cell space and paginate"
    );
    for page in &all.pages {
        for hit in page.hit_regions.iter().filter(|hit| hit.node_id == id.0) {
            assert!(
                hit.rect.x >= 25.0 && hit.rect.max_x() <= page.size.width - 25.0,
                "deleted text escaped the table"
            );
            assert!(hit.byte_range.end <= "α γ tail".len());
        }
    }
    scheduler.submit_for_display(3, source.clone(), 0..20, true);
    assert_eq!(
        receive(),
        all,
        "switching modes must not accumulate deletions or remap offsets twice"
    );
    assert_eq!(
        model::inline_text(&model::document_paragraphs(&source)[0].content),
        "α γ tail"
    );
}

#[test]
fn review_projection_is_stable_when_an_unrelated_paragraph_changes() {
    let mut document = Document::default();
    let paragraph = Paragraph::plain("α tail");
    let id = paragraph.id.clone();
    document.sections[0].blocks = vec![Block::Paragraph(paragraph), Block::Paragraph(Paragraph::plain("other"))].into();
    deletion(&mut document, &id, "α ".len(), "old ", 1);
    let first = crate::review_projection::ReviewProjection::new(&document);
    let Block::Paragraph(other) = &mut document.sections[0].blocks[1] else { unreachable!() };
    other.content = Paragraph::plain("other edited").content;
    let second = crate::review_projection::ReviewProjection::new(&document);
    assert_eq!(first.document.styles, second.document.styles);
    assert_eq!(first.document.sections[0].blocks[0], second.document.sections[0].blocks[0]);
}
