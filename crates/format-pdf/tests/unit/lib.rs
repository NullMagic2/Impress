use super::*;

fn line(text: &str, x0: f64, y: f64, size: f64) -> Line { Line::plain(text, x0, x0 + text.len() as f64 * size * 0.5, y, size) }
fn page(lines: Vec<Line>) -> PageText { PageText { width: 595.0, height: 842.0, lines, ..PageText::default() } }

fn report() -> PdfSource {
    PdfSource { pages: vec![
        page(vec![
            line("Annual Report", 72.0, 90.0, 24.0),
            line("Introduction", 72.0, 140.0, 16.0),
            line("This is the first line of a para-", 72.0, 170.0, 11.0),
            line("graph that continues here and keeps going on", 72.0, 183.0, 11.0),
            line("to the end.", 72.0, 196.0, 11.0),
            line("A second paragraph starts after a gap.", 72.0, 222.0, 11.0),
            line("Results", 72.0, 260.0, 16.0),
            line("• First point", 72.0, 290.0, 11.0),
            line("• Second point", 72.0, 303.0, 11.0),
            line("1", 290.0, 800.0, 9.0),
        ]),
        page(vec![line("Text on page two.", 72.0, 90.0, 11.0), line("2", 290.0, 800.0, 9.0)]),
    ], ..Default::default() }
}

#[test]
fn lines_become_paragraphs_and_headings() {
    let source = report();
    let pieces = pieces(&source, &ImportOptions::default());
    let kinds: Vec<Kind> = pieces.iter().map(|p| p.kind).collect();
    assert_eq!(kinds, [Kind::Title, Kind::Heading(1), Kind::Body, Kind::Body, Kind::Heading(1), Kind::Body, Kind::Body, Kind::Body]);
    assert_eq!(joined(&pieces[2].lines, true), "This is the first line of a paragraph that continues here and keeps going on to the end.");
    assert!(pieces.iter().all(|p| p.lines.iter().all(|l| l != "1" && l != "2")), "page numbers are dropped");
    let with_breaks = super::pieces(&source, &ImportOptions { page_breaks: true, ..ImportOptions::default() });
    assert!(with_breaks.last().unwrap().page_break_before);
    let only_second = super::pieces(&source, &ImportOptions { pages: Some(vec![1]), ..ImportOptions::default() });
    assert_eq!(only_second.len(), 1);
}

