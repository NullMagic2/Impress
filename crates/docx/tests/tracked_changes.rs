//! Imported Word revisions remain individual, editable and reversible.
use docx::DocxFormat;
use editor::EditorState;
use format::DocumentFormat;
use model::*;
use std::io::{Cursor, Read, Write};
fn load(body: &str) -> Document {
    load_styles(body, None)
}
fn load_styles(body: &str, styles: Option<&str>) -> Document {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file(
        "word/document.xml",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    write!(zip,r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}</w:body></w:document>"#).unwrap();
    if let Some(styles) = styles {
        zip.start_file("word/styles.xml", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(styles.as_bytes()).unwrap();
    }
    DocxFormat
        .import(&mut Cursor::new(zip.finish().unwrap().into_inner()))
        .unwrap()
}
fn save(document: &Document) -> (Document, String) {
    let mut bytes = Cursor::new(Vec::new());
    DocxFormat.export(document, &mut bytes).unwrap();
    let raw = bytes.into_inner();
    let mut zip = zip::ZipArchive::new(Cursor::new(&raw)).unwrap();
    let mut xml = String::new();
    zip.by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    (DocxFormat.import(&mut Cursor::new(raw)).unwrap(), xml)
}
fn texts(document: &Document) -> Vec<String> {
    document_paragraphs(document)
        .iter()
        .map(|p| inline_text(&p.content))
        .collect()
}
#[test]
fn individual_authors_timestamps_and_format_rejection_survive_word_roundtrip() {
    let document = load(
        r#"<w:p><w:pPr><w:spacing w:after="240"/><w:pPrChange w:id="1" w:author="Alice" w:date="2025-01-01T01:00:00Z"><w:pPr><w:spacing w:after="0"/></w:pPr></w:pPrChange></w:pPr><w:r><w:rPr><w:b/><w:rPrChange w:id="2" w:author="Bob" w:date="2025-02-01T02:00:00Z"><w:rPr><w:i/></w:rPr></w:rPrChange></w:rPr><w:t>α😀</w:t></w:r><w:ins w:id="3" w:author="Carol"><w:r><w:t>New</w:t></w:r></w:ins><w:del w:id="4" w:author="Dave"><w:r><w:delText>Old</w:delText></w:r></w:del></w:p><w:sectPr/>"#,
    );
    assert_eq!(document.review.tracked_changes.len(), 4);
    let (reopened, xml) = save(&document);
    assert!(xml.contains("w:rPrChange"));
    assert!(xml.contains("w:pPrChange"));
    assert!(xml.contains("2025-02-01T02:00:00Z"));
    assert_eq!(reopened.review.tracked_changes.len(), 4);
    assert_eq!(texts(&reopened), vec!["α😀New"]);
    let mut editor = EditorState::new(reopened);
    for author in ["Bob", "Alice", "Carol", "Dave"] {
        let id = editor
            .document
            .review
            .tracked_changes
            .values()
            .find(|change| change.author == author)
            .unwrap()
            .id
            .clone();
        editor
            .resolve_tracked_change(&id, false)
            .unwrap_or_else(|error| panic!("{author}: {error}"));
    }
    assert_eq!(texts(&editor.document), vec!["α😀Old"]);
    let paragraph = document_paragraphs(&editor.document)[0];
    assert_eq!(paragraph.properties.space_after_pt, Some(0.0));
    let Inline::Text(run) = &paragraph.content[0] else {
        panic!()
    };
    assert_eq!(run.properties.italic, Some(true));
    assert_ne!(run.properties.bold, Some(true));
    assert!(editor.undo());
    assert_eq!(texts(&editor.document), vec!["α😀"]);
}
#[test]
fn linked_moves_cross_paragraphs_and_reject_as_one_atomic_change() {
    let document = load(
        r#"<w:p><w:r><w:t>Source:</w:t></w:r><w:moveFromRangeStart w:id="10" w:name="moveA"/><w:moveFrom w:id="11" w:author="Mover"><w:r><w:delText>α😀</w:delText></w:r></w:moveFrom><w:moveFromRangeEnd w:id="10"/></w:p><w:p><w:moveToRangeStart w:id="12" w:name="moveA"/><w:moveTo w:id="13" w:author="Mover"><w:r><w:t>α😀</w:t></w:r></w:moveTo><w:moveToRangeEnd w:id="12"/></w:p><w:sectPr/>"#,
    );
    assert_eq!(document.review.tracked_changes.len(), 1);
    let change = document.review.tracked_changes.values().next().unwrap();
    assert_eq!(change.effects.len(), 2);
    let (reopened, xml) = save(&document);
    assert!(xml.contains("w:moveFromRangeStart"));
    assert!(xml.contains("w:moveToRangeStart"));
    assert_eq!(reopened.review.tracked_changes.len(), 1, "{xml}");
    let mut editor = EditorState::new(reopened);
    let id = editor
        .document
        .review
        .tracked_changes
        .keys()
        .next()
        .unwrap()
        .clone();
    editor.resolve_tracked_change(&id, false).unwrap();
    assert_eq!(texts(&editor.document), vec!["Source:α😀", ""]);
}
#[test]
fn table_row_cell_and_section_properties_have_semantic_before_values() {
    let document = load(
        r#"<w:tbl><w:tblPr><w:tblW w:w="4000" w:type="dxa"/><w:tblPrChange w:id="21" w:author="Table"><w:tblPr><w:tblW w:w="2000" w:type="dxa"/></w:tblPr></w:tblPrChange></w:tblPr><w:tblGrid><w:gridCol w:w="4000"/></w:tblGrid><w:tr><w:trPr><w:tblHeader/><w:trPrChange w:id="22" w:author="Row"><w:trPr/></w:trPrChange></w:trPr><w:tc><w:tcPr><w:shd w:fill="FF0000"/><w:tcPrChange w:id="23" w:author="Cell"><w:tcPr><w:shd w:fill="0000FF"/></w:tcPr></w:tcPrChange></w:tcPr><w:p><w:r><w:t>Cell</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:trPr><w:del w:id="24" w:author="Delete row"/></w:trPr><w:tc><w:p><w:r><w:t>Deleted</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:sectPr><w:pgSz w:w="12000" w:h="16000"/><w:sectPrChange w:id="25" w:author="Section"><w:sectPr><w:pgSz w:w="10000" w:h="14000"/></w:sectPr></w:sectPrChange></w:sectPr>"#,
    );
    assert_eq!(document.review.tracked_changes.len(), 5);
    assert_eq!(texts(&document), vec!["Cell"]);
    let (reopened, xml) = save(&document);
    for tag in [
        "w:tblPrChange",
        "w:trPrChange",
        "w:tcPrChange",
        "w:sectPrChange",
    ] {
        assert!(xml.contains(tag), "{tag}");
    }
    assert_eq!(reopened.review.tracked_changes.len(), 5);
    let mut editor = EditorState::new(reopened);
    for author in ["Cell", "Row", "Table", "Section", "Delete row"] {
        let id = editor
            .document
            .review
            .tracked_changes
            .values()
            .find(|change| change.author == author)
            .unwrap()
            .id
            .clone();
        editor.resolve_tracked_change(&id, false).unwrap();
    }
    assert_eq!(texts(&editor.document), vec!["Cell", "Deleted"]);
    let Block::Table(table) = &editor.document.sections[0].blocks[0] else {
        panic!()
    };
    assert!(!table.rows[0].repeat_as_header);
    assert_eq!(table.rows[0].cells[0].shading.as_deref(), Some("#0000FF"));
    assert!((table.properties.preferred_width_mm.unwrap() - 2000.0 * 25.4 / 1440.0).abs() < 0.01);
    assert!(
        (editor.document.sections[0].properties.page_width_mm - 10000.0 * 25.4 / 1440.0).abs()
            < 0.01
    );
}
#[test]
fn paragraph_mark_revisions_split_and_join_without_losing_inline_positions() {
    for inserted in [true, false] {
        let tag = if inserted { "ins" } else { "del" };
        let document = load(&format!(
            r#"<w:p><w:pPr><w:rPr><w:{tag} w:id="81" w:author="Paragraph"/></w:rPr></w:pPr><w:r><w:t>α😀</w:t></w:r></w:p><w:p><w:r><w:t>Second</w:t></w:r></w:p><w:sectPr/>"#
        ));
        assert_eq!(
            texts(&document),
            if inserted {
                vec!["α😀", "Second"]
            } else {
                vec!["α😀Second"]
            }
        );
        let (reopened, xml) = save(&document);
        assert_eq!(texts(&reopened), texts(&document), "{xml}");
        let mut editor = EditorState::new(reopened);
        let id = editor
            .document
            .review
            .tracked_changes
            .keys()
            .next()
            .unwrap()
            .clone();
        editor.resolve_tracked_change(&id, false).unwrap();
        assert_eq!(
            texts(&editor.document),
            if inserted {
                vec!["α😀Second"]
            } else {
                vec!["α😀", "Second"]
            }
        );
        assert!(editor.undo());
        assert_eq!(texts(&editor.document), texts(&document));
    }
}
#[test]
fn consecutive_deleted_paragraph_marks_preserve_both_boundaries() {
    let document = load(
        r#"<w:p><w:pPr><w:rPr><w:del w:id="91" w:author="First"/></w:rPr></w:pPr><w:r><w:t>One</w:t></w:r></w:p><w:p><w:pPr><w:rPr><w:del w:id="92" w:author="Second"/></w:rPr></w:pPr><w:r><w:t>Two</w:t></w:r></w:p><w:p><w:r><w:t>Three</w:t></w:r></w:p><w:sectPr/>"#,
    );
    assert_eq!(texts(&document), vec!["OneTwoThree"]);
    let (reopened, xml) = save(&document);
    assert_eq!(texts(&reopened), vec!["OneTwoThree"], "{xml}");
    assert_eq!(reopened.review.tracked_changes.len(), 2);
    let mut editor = EditorState::new(reopened);
    for author in ["Second", "First"] {
        let id = editor
            .document
            .review
            .tracked_changes
            .values()
            .find(|change| change.author == author)
            .unwrap()
            .id
            .clone();
        editor.resolve_tracked_change(&id, false).unwrap();
    }
    assert_eq!(texts(&editor.document), vec!["One", "Two", "Three"]);
}
#[test]
fn inserted_and_deleted_cells_remain_individually_reversible() {
    let document = load(
        r#"<w:tbl><w:tr><w:tc><w:p><w:r><w:t>Keep</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:cellIns w:id="110" w:author="Insert cell"/></w:tcPr><w:p><w:r><w:t>New</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:cellDel w:id="111" w:author="Delete cell"/></w:tcPr><w:p><w:r><w:t>Old</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:sectPr/>"#,
    );
    assert_eq!(texts(&document), vec!["Keep", "New"]);
    let (reopened, xml) = save(&document);
    assert_eq!(texts(&reopened), vec!["Keep", "New"], "{xml}");
    assert_eq!(reopened.review.tracked_changes.len(), 2);
    let mut editor = EditorState::new(reopened);
    for author in ["Insert cell", "Delete cell"] {
        let id = editor
            .document
            .review
            .tracked_changes
            .values()
            .find(|change| change.author == author)
            .unwrap()
            .id
            .clone();
        editor.resolve_tracked_change(&id, false).unwrap();
    }
    assert_eq!(texts(&editor.document), vec!["Keep", "Old"]);
}
#[test]
fn tracked_vertical_merge_restores_original_cell_content_after_word_cycle() {
    let document = load(
        r#"<w:tbl><w:tblGrid><w:gridCol w:w="2000"/></w:tblGrid><w:tr><w:tc><w:tcPr><w:vMerge w:val="restart"/></w:tcPr><w:p><w:r><w:t>Top</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:tcPr><w:cellMerge w:id="120" w:author="Merge" w:vMerge="cont"/></w:tcPr><w:p><w:r><w:t>Bottom</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:sectPr/>"#,
    );
    assert_eq!(texts(&document), vec!["Top"]);
    let (reopened, xml) = save(&document);
    assert!(xml.contains("Bottom"), "{xml}");
    assert_eq!(texts(&reopened), vec!["Top"]);
    assert_eq!(reopened.review.tracked_changes.len(), 1);
    let mut editor = EditorState::new(reopened);
    let id = editor
        .document
        .review
        .tracked_changes
        .keys()
        .next()
        .unwrap()
        .clone();
    editor.resolve_tracked_change(&id, false).unwrap();
    assert_eq!(texts(&editor.document), vec!["Top", "Bottom"]);
    let Block::Table(table) = &editor.document.sections[0].blocks[0] else {
        panic!()
    };
    assert_eq!(table.rows[0].cells[0].row_span, 1);
}
#[test]
fn same_paragraph_move_retains_both_authors_and_resolves_both_ends() {
    let document = load(
        r#"<w:p><w:moveToRangeStart w:id="201" w:name="mixedAuthors"/><w:moveTo w:id="202" w:author="Destination" w:date="2025-02-01T00:00:00Z"><w:r><w:t>α😀</w:t></w:r></w:moveTo><w:moveToRangeEnd w:id="201"/><w:r><w:t xml:space="preserve"> middle </w:t></w:r><w:moveFromRangeStart w:id="203" w:name="mixedAuthors"/><w:moveFrom w:id="204" w:author="Source" w:date="2025-01-01T00:00:00Z"><w:r><w:delText>α😀</w:delText></w:r></w:moveFrom><w:moveFromRangeEnd w:id="203"/></w:p><w:sectPr/>"#,
    );
    let (reopened, xml) = save(&document);
    assert!(xml.contains("Destination"));
    assert!(xml.contains("Source"));
    assert_eq!(reopened.review.tracked_changes.len(), 2);
    let mut editor = EditorState::new(reopened);
    let id = editor
        .document
        .review
        .tracked_changes
        .keys()
        .next()
        .unwrap()
        .clone();
    editor.resolve_tracked_change(&id, false).unwrap();
    assert_eq!(texts(&editor.document), vec![" middle α😀"]);
    assert!(editor
        .document
        .review
        .tracked_changes
        .values()
        .all(|change| change.status == RevisionStatus::Rejected));
}
#[test]
fn inline_control_stays_in_its_paragraph_when_rejecting_join_and_undoing() {
    let document = load(
        r#"<w:p><w:pPr><w:rPr><w:del w:id="301" w:author="Join"/></w:rPr></w:pPr><w:r><w:t>First</w:t></w:r></w:p><w:p><w:sdt><w:sdtPr><w:id w:val="900"/><w:alias w:val="Inline"/><w:text/></w:sdtPr><w:sdtContent><w:r><w:t>Second</w:t></w:r></w:sdtContent></w:sdt></w:p><w:sectPr/>"#,
    );
    assert_eq!(texts(&document), vec!["FirstSecond"]);
    let (reopened, xml) = save(&document);
    assert_eq!(reopened.content_controls.len(), 1, "{xml}");
    assert_eq!(texts(&reopened), vec!["FirstSecond"]);
    let mut editor = EditorState::new(reopened);
    let id = editor
        .document
        .review
        .tracked_changes
        .keys()
        .next()
        .unwrap()
        .clone();
    editor.resolve_tracked_change(&id, false).unwrap();
    assert_eq!(texts(&editor.document), vec!["First", "Second"]);
    let control = editor.document.content_controls.values().next().unwrap();
    assert_eq!(
        model::content_control_text(&editor.document, &control.range).as_deref(),
        Some("Second")
    );
    assert_eq!(control.range.start.paragraph, control.range.end.paragraph);
    assert!(editor.undo());
    assert_eq!(texts(&editor.document), vec!["FirstSecond"]);
    let control = editor.document.content_controls.values().next().unwrap();
    assert_eq!(
        model::content_control_text(&editor.document, &control.range).as_deref(),
        Some("Second")
    );
    assert!(editor.redo());
    assert_eq!(texts(&editor.document), vec!["First", "Second"]);
}
#[test]
fn adjacent_collapsed_deletions_keep_original_order_and_authors() {
    let document = load(
        r#"<w:p><w:r><w:t>A</w:t></w:r><w:del w:id="401" w:author="One"><w:r><w:delText>α</w:delText></w:r></w:del><w:del w:id="402" w:author="Two"><w:r><w:delText>😀</w:delText></w:r></w:del><w:r><w:t>Z</w:t></w:r></w:p><w:sectPr/>"#,
    );
    let (reopened, xml) = save(&document);
    assert!(xml.find("α").unwrap() < xml.find("😀").unwrap(), "{xml}");
    let original = inline_text(
        &reopened
            .review
            .revisions
            .values()
            .next()
            .unwrap()
            .before
            .content,
    );
    assert_eq!(original, "Aα😀Z");
}
#[test]
fn paragraph_revision_keeps_custom_default_style_and_inherited_tab_clears() {
    let styles = r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:default="1" w:styleId="CustomDefault"><w:name w:val="Custom"/><w:pPr><w:tabs><w:tab w:val="left" w:pos="500"/><w:tab w:val="right" w:pos="1000"/></w:tabs></w:pPr></w:style></w:styles>"#;
    let document = load_styles(
        r#"<w:p><w:pPr><w:tabs><w:tab w:val="left" w:pos="3000"/></w:tabs><w:pPrChange w:id="501" w:author="Tabs"><w:pPr><w:tabs><w:tab w:val="clear" w:pos="500"/><w:tab w:val="center" w:pos="2000"/></w:tabs></w:pPr></w:pPrChange></w:pPr><w:r><w:t>Text</w:t></w:r></w:p><w:sectPr/>"#,
        Some(styles),
    );
    let (reopened, _) = save(&document);
    let mut editor = EditorState::new(reopened);
    let id = editor
        .document
        .review
        .tracked_changes
        .keys()
        .next()
        .unwrap()
        .clone();
    editor.resolve_tracked_change(&id, false).unwrap();
    let paragraph = document_paragraphs(&editor.document)[0];
    assert_eq!(paragraph.style.as_ref().unwrap().0, "CustomDefault");
    let tabs = paragraph.properties.tabs.as_ref().unwrap();
    let positions = tabs
        .iter()
        .map(|tab| (tab.position_mm * 1440.0 / 25.4).round() as i32)
        .collect::<Vec<_>>();
    assert_eq!(positions, vec![1000, 2000]);
}
#[test]
fn character_revision_preserves_old_style_across_tabs_and_inline_breaks() {
    let document = load(
        r#"<w:p><w:r><w:rPr><w:rStyle w:val="NewStyle"/><w:b/><w:rPrChange w:id="601" w:author="Rich run"><w:rPr><w:rStyle w:val="OldStyle"/><w:i/></w:rPr></w:rPrChange></w:rPr><w:t>A</w:t><w:tab/><w:t>α😀</w:t><w:br w:type="page"/><w:t>B</w:t></w:r></w:p><w:sectPr/>"#,
    );
    let (reopened, xml) = save(&document);
    assert!(xml.contains("OldStyle"), "{xml}");
    let mut reader = quick_xml::Reader::from_str(&xml);
    let mut ids = std::collections::BTreeSet::new();
    loop {
        match reader.read_event() {
            Ok(quick_xml::events::Event::Start(node))
            | Ok(quick_xml::events::Event::Empty(node))
                if node.name().as_ref() == "w:rPrChange" =>
            {
                let id = node
                    .attributes()
                    .flatten()
                    .find(|attribute| attribute.key.as_ref() == "w:id")
                    .unwrap()
                    .value
                    .into_owned();
                assert!(ids.insert(id), "Revision annotation IDs must be unique");
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(error) => panic!("{error}"),
            _ => {}
        }
    }
    assert_eq!(
        reopened.review.tracked_changes.len(),
        1,
        "One semantic formatting edit remains one review action"
    );
    let mut editor = EditorState::new(reopened);
    let id = editor
        .document
        .review
        .tracked_changes
        .keys()
        .next()
        .unwrap()
        .clone();
    editor.resolve_tracked_change(&id, false).unwrap();
    let paragraph = document_paragraphs(&editor.document)[0];
    assert_eq!(inline_text(&paragraph.content), "A\tα😀\u{000C}B");
    for run in paragraph.content.iter().filter_map(|item| {
        if let Inline::Text(run) = item {
            Some(run)
        } else {
            None
        }
    }) {
        assert_eq!(run.style.as_ref().unwrap().0, "OldStyle");
        assert_eq!(run.properties.italic, Some(true));
    }
}
