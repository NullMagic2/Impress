use super::*;

fn page(lines: Vec<Line>) -> PageText { PageText { width: 595.0, height: 842.0, lines, ..PageText::default() } }

#[test]
fn aligned_cells_become_a_table() {
    let mut lines = vec![Line::plain("Results are summarised below in the table that follows here.", 72.0, 523.0, 100.0, 11.0)];
    for (i, (name, value)) in [("Item", "Price"), ("Apples", "1.20"), ("Pears", "2.50"), ("Plums", "0.90")].iter().enumerate() {
        let y = 130.0 + i as f64 * 15.0;
        lines.push(Line::plain(name, 72.0, 72.0 + name.len() as f64 * 5.5, y, 11.0));
        lines.push(Line::plain(value, 300.0, 300.0 + value.len() as f64 * 5.5, y, 11.0));
    }
    let source = PdfSource { pages: vec![page(lines)], ..PdfSource::default() };
    let analysis = analyze_structure(&source, &ImportOptions::default());
    let table = analysis.items.iter().find_map(|i| if let Item::Table(t) = i { Some(t) } else { None }).expect("a table");
    assert_eq!(table.rows.len(), 4);
    assert_eq!(table.widths.len(), 2);
    assert_eq!(table.rows[2][1][0].text, "2.50");
}

