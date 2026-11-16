//! Prints how a PDF would be imported: `cargo run -p format-pdf --example pdf_preview -- file.pdf`
//! Add `--stats` for what was read from each page and how long each step took,
//! and `--document` for the blocks, styles and runs of the converted document.
fn main() {
    let path = std::env::args().nth(1).expect("usage: pdf_preview FILE.pdf [--stats]");
    let stats = std::env::args().any(|a| a == "--stats");
    let bytes = std::fs::read(&path).expect("read PDF");
    let started = std::time::Instant::now();
    let source = format_pdf::analyze(&bytes).unwrap_or_else(|e| panic!("{e}"));
    let read = started.elapsed();
    println!("{} page(s), title {:?}\n", source.pages.len(), source.title);
    if stats {
        for (index, page) in source.pages.iter().enumerate() {
            println!("page {}: {} lines, {} pictures, {} links", index + 1, page.lines.len(), page.pictures.len(), page.links.len());
        }
    }
    let started = std::time::Instant::now();
    let text = format_pdf::preview(&source, &format_pdf::ImportOptions::default(), 200);
    let shaped = started.elapsed();
    print!("{text}");
    if stats { println!("\nread {read:?}, structure {shaped:?}"); }
    if std::env::args().any(|a| a == "--document") { print_document(&format_pdf::to_document(&source, &format_pdf::ImportOptions::default(), model::Document::default())); }
}

fn print_document(document: &model::Document) {
    use model::{Block, Inline};
    let p = &document.sections[0].properties;
    println!("\n== page {:.0}×{:.0} mm, margins L{:.1} R{:.1} T{:.1} B{:.1}", p.page_width_mm, p.page_height_mm, p.margin_left_mm, p.margin_right_mm, p.margin_top_mm, p.margin_bottom_mm);
    for (name, style) in &document.styles.styles {
        let c = &style.character;
        if c.font_family.is_some() || c.font_size_pt.is_some() { println!("style {name}: {:?} {:?} bold {:?} italic {:?} colour {:?}", c.font_family, c.font_size_pt, c.bold, c.italic, c.color); }
    }
    let runs = |content: &[Inline]| content.iter().map(|inline| match inline {
        Inline::Text(run) => {
            let c = &run.properties;
            let mut marks = String::new();
            if let Some(f) = &c.font_family { marks.push_str(&format!(" font={f}")); }
            if let Some(s) = c.font_size_pt { marks.push_str(&format!(" size={s}")); }
            if c.bold == Some(true) { marks.push_str(" bold"); }
            if c.italic == Some(true) { marks.push_str(" italic"); }
            if let Some(col) = &c.color { marks.push_str(&format!(" {col}")); }
            if marks.is_empty() { run.text.clone() } else { format!("[{}|{}]", run.text, marks.trim()) }
        }
        Inline::Hyperlink(link) => format!("<link {}: {} runs>", link.target, link.content.len()),
        Inline::Field(field) => format!("{{{:?}}}", field.kind),
        Inline::LineBreak { .. } => "⏎".into(),
        _ => "·".into(),
    }).collect::<String>();
    for issue in &document.compatibility { println!("NOTE {}", issue.message); }
    for (index, section) in document.sections.iter().enumerate() {
    println!("-- section {index}: {} column(s), gap {:.1} mm, {:?}", section.properties.column_count, section.properties.column_gap_mm, section.properties.start_on);
    for (label, stream) in [("header", &section.headers.default), ("footer", &section.footers.default)] {
        if let Some(stream) = stream { for block in stream.blocks.iter() { if let Block::Paragraph(par) = block { println!("{label} {:?}: {}", par.properties.alignment, runs(&par.content)); } } }
    }
    for block in section.blocks.iter() {
        match block {
            Block::Paragraph(par) => {
                let style = par.style.as_ref().map_or("-".into(), |s| s.0.clone());
                let list = par.properties.list_id.as_ref().map(|id| { let l = &document.lists[id]; let level = par.properties.list_level.unwrap_or(0); format!(" list{:?}/{level}", l.levels[usize::from(level)].format) }).unwrap_or_default();
                let indent = par.properties.left_indent_mm.map(|v| format!(" indent{v:.1}")).unwrap_or_default();
                println!("P {style} {:?}{list}{indent}: {}", par.properties.alignment, runs(&par.content));
            }
            Block::Table(table) => {
                println!("TABLE {} cols {:?}", table.rows.len(), table.properties.grid_column_widths_mm.iter().map(|w| format!("{w:.0}")).collect::<Vec<_>>());
                for row in &table.rows { println!("  | {}", row.cells.iter().map(|c| c.blocks.iter().map(|b| if let Block::Paragraph(p) = b { runs(&p.content) } else { String::new() }).collect::<String>()).collect::<Vec<_>>().join(" | ")); }
            }
            Block::Image(image) => println!("IMAGE {:.0}×{:.0} mm {:?} ({})", image.width_mm, image.height_mm, image.alignment, document.resources[&image.resource].media_type),
            Block::PageBreak { .. } => println!("PAGE BREAK"),
            Block::ColumnBreak { .. } => println!("COLUMN BREAK"),
            _ => println!("other block"),
        }
    }
    }
}
