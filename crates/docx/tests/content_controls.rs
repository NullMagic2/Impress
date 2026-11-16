//! Controls keep real bindings, constraints, wrappers and undoable synchronized edits.
use docx::DocxFormat;
use editor::EditorState;
use format::DocumentFormat;
use model::*;
use std::io::{Cursor, Write};
const STORE: &str = "{F387D620-44BC-4A5F-86A9-3CDF1A80F6A5}";
fn package(body: &str) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let parts=[("word/document.xml",format!(r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml"><w:body>{body}<w:sectPr/></w:body></w:document>"#)),
    ("word/_rels/document.xml.rels",r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="data" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXml" Target="../customXml/item1.xml"/></Relationships>"#.into()),
    ("customXml/item1.xml",r#"<p:people xmlns:p="urn:people"><p:person id="1"><p:name>Untouched</p:name></p:person><p:person id="2"><p:name>Old</p:name></p:person></p:people>"#.into()),
    ("customXml/itemProps1.xml",format!(r#"<ds:datastoreItem xmlns:ds="http://schemas.openxmlformats.org/officeDocument/2006/customXml" ds:itemID="{STORE}"/>"#)),
    ("customXml/_rels/item1.xml.rels",r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="props" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXmlProps" Target="itemProps1.xml"/></Relationships>"#.into())];
    for (name, xml) in parts {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(xml.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
fn control(id: &str, extra: &str, text: &str, bound: bool) -> String {
    let binding = if bound {
        format!(
            r#"<w:dataBinding w:storeItemID="{STORE}" w:xpath="//p:person[@id='2']/p:name[1]" w:prefixMappings="xmlns:p='urn:people'"/>"#
        )
    } else {
        String::new()
    };
    format!(
        r#"<w:sdt><w:sdtPr><w:id w:val="{id}"/>{binding}{extra}</w:sdtPr><w:sdtContent><w:r><w:t>{text}</w:t></w:r></w:sdtContent></w:sdt>"#
    )
}
fn load(bytes: Vec<u8>) -> Document {
    DocxFormat.import(&mut Cursor::new(bytes)).unwrap()
}
fn cycle(document: &Document) -> Document {
    let mut out = Cursor::new(Vec::new());
    DocxFormat.export(document, &mut out).unwrap();
    load(out.into_inner())
}
fn find<'a>(document: &'a Document, source: &str) -> &'a ContentControl {
    document
        .content_controls
        .values()
        .find(|c| c.source_id.as_deref() == Some(source))
        .unwrap()
}
fn text(document: &Document, source: &str) -> String {
    content_control_text(document, &find(document, source).range).unwrap()
}
#[test]
fn namespace_xpath_bindings_update_peers_real_xml_and_undo_atomically() {
    let body = format!(
        "<w:p>{}</w:p><w:p>{}</w:p>",
        control("1", "<w:text/>", "Old", true),
        control("2", "<w:text/>", "Old", true)
    );
    let document = load(package(&body));
    assert_eq!(document.content_controls.len(), 2);
    assert_eq!(document.custom_xml.len(), 1);
    let id = find(&document, "1").id.clone();
    let binding = find(&document, "1").binding.clone().unwrap();
    assert_eq!(document.bound_content_value(&binding).unwrap(), "Old");
    let mut editor = EditorState::new(document);
    editor.set_content_control_value(&id, "New α😀").unwrap();
    assert_eq!(text(&editor.document, "1"), "New α😀");
    assert_eq!(text(&editor.document, "2"), "New α😀");
    assert_eq!(
        editor.document.bound_content_value(&binding).unwrap(),
        "New α😀"
    );
    assert!(editor
        .document
        .custom_xml
        .values()
        .next()
        .unwrap()
        .xml
        .contains("Untouched"));
    assert!(editor.undo());
    assert_eq!(text(&editor.document, "1"), "Old");
    assert_eq!(text(&editor.document, "2"), "Old");
    assert_eq!(
        editor.document.bound_content_value(&binding).unwrap(),
        "Old"
    );
    assert!(editor.redo());
    let reopened = cycle(&editor.document);
    assert_eq!(text(&reopened, "1"), "New α😀");
    assert_eq!(
        reopened
            .bound_content_value(&find(&reopened, "2").binding.clone().unwrap())
            .unwrap(),
        "New α😀"
    );
}
#[test]
fn content_lock_single_line_and_dropdown_constraints_reject_edits_without_partial_changes() {
    let body=format!("<w:p>{}</w:p><w:p>{}</w:p><w:p>{}</w:p>",control("1","<w:lock w:val=\"sdtContentLocked\"/><w:text/>","Locked",false),control("2","<w:text/>","Single",false),control("3","<w:dropDownList><w:listItem w:displayText=\"One\" w:value=\"1\"/><w:listItem w:displayText=\"Two\" w:value=\"2\"/></w:dropDownList>","One",false));
    let mut editor = EditorState::new(load(package(&body)));
    let id = find(&editor.document, "1").id.clone();
    let range = find(&editor.document, "1").range.clone();
    editor.set_caret(range.start.paragraph.clone(), range.start.offset + 1);
    let before = editor.document.clone();
    assert!(editor.insert_text("x").is_err());
    assert_eq!(editor.document, before);
    assert!(editor.remove_content_control(&id).is_err());
    let id = find(&editor.document, "2").id.clone();
    assert!(editor.set_content_control_value(&id, "Line\nLine").is_err());
    assert_eq!(text(&editor.document, "2"), "Single");
    let id = find(&editor.document, "3").id.clone();
    assert!(editor.set_content_control_value(&id, "Other").is_err());
    editor.set_content_control_value(&id, "2").unwrap();
    assert_eq!(text(&editor.document, "3"), "Two");
}
#[test]
fn block_cell_and_row_control_wrappers_preserve_their_structural_scope() {
    let body = r#"<w:sdt><w:sdtPr><w:id w:val="10"/><w:lock w:val="sdtLocked"/></w:sdtPr><w:sdtContent><w:p><w:r><w:t>Block</w:t></w:r></w:p></w:sdtContent></w:sdt><w:tbl><w:sdt><w:sdtPr><w:id w:val="11"/></w:sdtPr><w:sdtContent><w:tr><w:sdt><w:sdtPr><w:id w:val="12"/><w:text/></w:sdtPr><w:sdtContent><w:tc><w:p><w:r><w:t>Cell</w:t></w:r></w:p></w:tc></w:sdtContent></w:sdt></w:tr></w:sdtContent></w:sdt></w:tbl>"#;
    let document = load(package(body));
    assert_eq!(document.content_controls.len(), 3);
    assert_eq!(text(&document, "10"), "Block");
    assert_eq!(text(&document, "12"), "Cell");
    for (id, scope) in [
        ("10", ContentControlScope::Blocks),
        ("11", ContentControlScope::Row),
        ("12", ContentControlScope::Cell),
    ] {
        assert_eq!(find(&document, id).scope, scope);
        assert_eq!(find(&document, id).members.len(), 1);
    }
    let reopened = cycle(&document);
    assert_eq!(reopened.content_controls.len(), 3);
    assert_eq!(find(&reopened, "11").scope, ContentControlScope::Row);
    assert_eq!(find(&reopened, "12").scope, ContentControlScope::Cell);
}
#[test]
fn full_xpath_attribute_selection_updates_only_the_selected_node_and_rejects_ambiguous_binding() {
    let mut document = load(package("<w:p/>"));
    let binding = ContentBinding {
        store_id: STORE.into(),
        xpath: "/p:people/p:person[2]/@id".into(),
        namespaces: [("p".into(), "urn:people".into())].into_iter().collect(),
    };
    assert_eq!(document.bound_content_value(&binding).unwrap(), "2");
    document
        .set_bound_content_value(&binding, "changed")
        .unwrap();
    assert_eq!(document.bound_content_value(&binding).unwrap(), "changed");
    let ambiguous = ContentBinding {
        xpath: "//p:name".into(),
        ..binding
    };
    assert!(document.set_bound_content_value(&ambiguous, "bad").is_err());
}

#[test]
fn stale_bound_cache_refreshes_and_control_typing_boundaries_remain_live() {
    let body = format!(
        "<w:p><w:r><w:t>Before </w:t></w:r>{}<w:r><w:t> after</w:t></w:r></w:p>",
        control("1", "<w:text/>", "Stale text", true)
    );
    let mut editor = EditorState::new(load(package(&body)));
    assert_eq!(text(&editor.document, "1"), "Old");
    let start = find(&editor.document, "1").range.start.clone();
    editor.set_caret(start.paragraph, start.offset);
    editor.insert_text("N").unwrap();
    assert_eq!(text(&editor.document, "1"), "NOld");
    let end = find(&editor.document, "1").range.end.clone();
    editor.set_caret(end.paragraph, end.offset);
    editor.insert_text("!").unwrap();
    assert_eq!(text(&editor.document, "1"), "NOld!");
    assert_eq!(
        editor
            .document
            .bound_content_value(find(&editor.document, "1").binding.as_ref().unwrap())
            .unwrap(),
        "NOld!"
    );
    assert_eq!(
        inline_text(&document_paragraphs(&editor.document)[0].content),
        "Before NOld! after"
    );
}
#[test]
fn nested_controls_hyperlinks_and_revision_boundaries_export_balanced_wrappers() {
    let body = r#"<w:p><w:sdt><w:sdtPr><w:id w:val="1"/></w:sdtPr><w:sdtContent><w:r><w:t>Prefix</w:t></w:r><w:ins w:id="77" w:author="Author"><w:r><w:t>Added</w:t></w:r></w:ins></w:sdtContent></w:sdt><w:r><w:t>Outside</w:t></w:r></w:p><w:p><w:sdt><w:sdtPr><w:id w:val="2"/></w:sdtPr><w:sdtContent><w:sdt><w:sdtPr><w:id w:val="3"/></w:sdtPr><w:sdtContent><w:hyperlink w:anchor="Target"><w:r><w:t>Linked</w:t></w:r></w:hyperlink></w:sdtContent></w:sdt></w:sdtContent></w:sdt></w:p>"#;
    let document = load(package(body));
    assert_eq!(document.content_controls.len(), 3);
    let reopened = cycle(&document);
    assert_eq!(reopened.content_controls.len(), 3);
    assert_eq!(text(&reopened, "1"), "PrefixAdded");
    assert_eq!(text(&reopened, "2"), "Linked");
    assert_eq!(text(&reopened, "3"), "Linked");
    assert!(!reopened.review.tracked_changes.is_empty());
}
#[test]
fn malformed_binding_namespace_and_excessive_xpath_return_errors() {
    let document = load(package("<w:p/>"));
    for xpath in [
        "//missing:node".into(),
        "(".repeat(65) + "1" + &")".repeat(65),
        "/x".repeat(3000),
    ] {
        let binding = ContentBinding {
            store_id: STORE.into(),
            xpath,
            namespaces: Default::default(),
        };
        assert!(document.bound_content_value(&binding).is_err());
    }
}
#[test]
fn multiline_bound_controls_replace_paragraph_structure_without_extra_values_and_undo() {
    let first = format!(
        r#"<w:sdt><w:sdtPr><w:id w:val="1"/><w:dataBinding w:storeItemID="{STORE}" w:xpath="//p:person[@id='2']/p:name[1]" w:prefixMappings="xmlns:p='urn:people'"/></w:sdtPr><w:sdtContent><w:p><w:r><w:t>Old</w:t></w:r></w:p><w:p><w:r><w:t>Cache</w:t></w:r></w:p></w:sdtContent></w:sdt>"#
    );
    let second = format!(
        "<w:p>{}</w:p>",
        control("2", "<w:text w:multiLine=\"1\"/>", "Old", true)
    );
    let mut editor = EditorState::new(load(package(&(first + &second))));
    let id = find(&editor.document, "2").id.clone();
    editor.set_content_control_value(&id, "New value").unwrap();
    assert_eq!(text(&editor.document, "1"), "New value");
    assert_eq!(text(&editor.document, "2"), "New value");
    let reopened = cycle(&editor.document);
    assert_eq!(text(&reopened, "1"), "New value");
    assert!(editor.undo());
    assert_eq!(text(&editor.document, "2"), "Old");
}
#[test]
fn checkbox_and_date_picker_values_keep_typed_metadata_and_native_roundtrip() {
    let body=format!("<w:p>{}</w:p><w:p>{}</w:p>",control("1","<w14:checkbox><w14:checked w14:val=\"0\"/><w14:checkedState w14:val=\"2612\"/><w14:uncheckedState w14:val=\"2610\"/></w14:checkbox>","☐",false),control("2","<w:date w:fullDate=\"2026-10-18\"><w:dateFormat w:val=\"dd/MM/yyyy\"/><w:calendar w:val=\"gregorian\"/></w:date>","18/10/2026",false));
    let mut editor = EditorState::new(load(package(&body)));
    let checkbox = find(&editor.document, "1").id.clone();
    editor.set_content_control_value(&checkbox, "true").unwrap();
    assert_eq!(text(&editor.document, "1"), "☒");
    let date = find(&editor.document, "2").id.clone();
    assert!(editor
        .set_content_control_value(&date, "2026-02-30")
        .is_err());
    editor
        .set_content_control_value(&date, "2028-02-29")
        .unwrap();
    assert_eq!(text(&editor.document, "2"), "29/02/2028");
    let reopened = cycle(&editor.document);
    assert!(matches!(
        &find(&reopened, "1").kind,
        ContentControlKind::CheckBox { checked: true, .. }
    ));
    assert!(
        matches!(&find(&reopened,"2").kind,ContentControlKind::Date{full_date:Some(date),..} if date=="2028-02-29")
    );
}
#[test]
fn invalid_bound_dropdown_propagation_restores_xml_controls_document_and_history() {
    let choice="<w:dropDownList><w:listItem w:displayText=\"One\" w:value=\"Old\"/><w:listItem w:displayText=\"Two\" w:value=\"New\"/></w:dropDownList>";
    let body = format!("<w:p>{}</w:p>", control("1", choice, "One", true));
    let mut editor = EditorState::new(load(package(&body)));
    let binding = find(&editor.document, "1").binding.clone().unwrap();
    let before = editor.document.clone();
    assert!(editor
        .set_content_binding_value(&binding, "Unlisted")
        .is_err());
    assert_eq!(editor.document, before);
    assert!(!editor.undo());
    editor.set_content_binding_value(&binding, "New").unwrap();
    assert_eq!(text(&editor.document, "1"), "Two");
    assert!(editor.undo());
    assert_eq!(editor.document, before);
}
#[test]
fn rich_inline_control_enter_promotes_whole_paragraph_without_wrapping_outside_text() {
    let body = format!("<w:p>{}</w:p>", control("1", "", "Split here", false));
    let mut editor = EditorState::new(load(package(&body)));
    let range = find(&editor.document, "1").range.clone();
    editor.set_caret(range.start.paragraph, 5);
    editor.split_paragraph().unwrap();
    assert_eq!(
        find(&editor.document, "1").scope,
        ContentControlScope::Blocks
    );
    assert_eq!(text(&editor.document, "1"), "Split\n here");
    assert_eq!(text(&cycle(&editor.document), "1"), "Split\n here");
    let body = format!(
        "<w:p><w:r><w:t>Outside </w:t></w:r>{}</w:p>",
        control("1", "", "Inside", false)
    );
    let mut editor = EditorState::new(load(package(&body)));
    let range = find(&editor.document, "1").range.clone();
    editor.set_caret(range.start.paragraph, range.start.offset + 2);
    let before = editor.document.clone();
    assert!(editor.split_paragraph().is_err());
    assert_eq!(editor.document, before);
}
const RED_PICTURE:&str="iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==";
const BLUE_PICTURE:&str="iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGNgYPj/HwADAgH/5ncLrgAAAABJRU5ErkJggg==";
fn picture_document() -> Document {
    let body = format!(
        "<w:p>{}</w:p><w:p>{}</w:p>",
        control("1", "<w:picture/>", "Cached", false),
        control("2", "<w:picture/>", "Cached", false)
    );
    let mut document = load(package(&body));
    let resource = decode_content_picture(RED_PICTURE).unwrap();
    let resource_id = resource.id.clone();
    document.resources.insert(resource_id.clone(), resource);
    for index in 0..2 {
        let Block::Paragraph(p) = &mut document.sections[0].blocks[index] else {
            panic!("p")
        };
        p.content = vec![Inline::Image(InlineImage {
            id: NodeId::new(),
            resource: resource_id.clone(),
            width_mm: 32.5,
            height_mm: 18.25,
            alt_text: Some("Picture".into()),
            effects: Default::default(),
        })]
        .into();
    }
    let binding = ContentBinding {
        store_id: STORE.into(),
        xpath: "//p:person[@id='2']/p:name[1]".into(),
        namespaces: [("p".into(), "urn:people".into())].into_iter().collect(),
    };
    document
        .set_bound_content_value(&binding, RED_PICTURE)
        .unwrap();
    for control in document.content_controls.values_mut() {
        control.range.start.offset = 0;
        control.range.end.offset = 3;
        control.binding = Some(binding.clone());
    }
    document
}
#[test]
fn picture_base64_bindings_refresh_propagate_preserve_geometry_and_undo() {
    let document = cycle(&picture_document());
    let first = find(&document, "1").id.clone();
    let second = find(&document, "2").id.clone();
    let binding = find(&document, "1").binding.clone().unwrap();
    let mut editor = EditorState::new(document);
    let before = editor.document.clone();
    let blue = decode_content_picture(BLUE_PICTURE).unwrap();
    editor
        .set_content_control_picture(&first, &blue.data)
        .unwrap();
    for id in [&first, &second] {
        assert_eq!(
            editor
                .document
                .content_control_picture(&editor.document.content_controls[id])
                .unwrap()
                .data,
            blue.data
        );
    }
    assert_eq!(
        editor.document.bound_content_value(&binding).unwrap(),
        BLUE_PICTURE
    );
    for paragraph in document_paragraphs(&editor.document) {
        let Inline::Image(image) = &paragraph.content[0] else {
            panic!("image")
        };
        assert_eq!((image.width_mm, image.height_mm), (32.5, 18.25));
    }
    let reopened = cycle(&editor.document);
    assert_eq!(
        reopened
            .content_control_picture(find(&reopened, "2"))
            .unwrap()
            .data,
        blue.data
    );
    assert!(editor.undo());
    assert_eq!(editor.document, before);
    assert!(editor.redo());
    let before = editor.document.clone();
    assert!(editor
        .set_content_binding_value(&binding, "not base64")
        .is_err());
    assert_eq!(editor.document, before);
    editor
        .set_content_binding_value(&binding, RED_PICTURE)
        .unwrap();
    assert_eq!(
        editor
            .document
            .content_control_picture(find(&editor.document, "2"))
            .unwrap()
            .data,
        decode_content_picture(RED_PICTURE).unwrap().data
    );
}
#[test]
fn temporary_control_discards_only_its_wrapper_on_successful_edit_and_undo_restores_it() {
    let body = format!(
        "<w:p>{}</w:p>",
        control("1", "<w:text/><w:temporary/>", "Before", false)
    );
    let mut editor = EditorState::new(load(package(&body)));
    let control = find(&editor.document, "1").clone();
    assert!(control.temporary);
    editor.set_caret(
        control.range.start.paragraph.clone(),
        control.range.start.offset,
    );
    editor.insert_text("New ").unwrap();
    assert!(editor.document.content_controls.is_empty());
    assert_eq!(
        inline_text(&document_paragraphs(&editor.document)[0].content),
        "New Before"
    );
    assert!(editor.undo());
    assert_eq!(find(&editor.document, "1").id, control.id);
}
#[test]
fn controls_after_deleted_table_rows_use_source_identity_and_dormant_controls_survive_edits() {
    let body=format!("<w:tbl><w:tr><w:trPr><w:del w:id=\"55\" w:author=\"Teacher\"/></w:trPr><w:tc><w:p>{}</w:p></w:tc></w:tr><w:tr><w:tc><w:p><w:r><w:t>Live row</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:sdt><w:sdtPr><w:id w:val=\"2\"/></w:sdtPr><w:sdtContent><w:p><w:r><w:t>After table</w:t></w:r></w:p></w:sdtContent></w:sdt>",control("1","<w:text/>","Deleted control",false));
    let mut editor = EditorState::new(load(package(&body)));
    assert_eq!(text(&editor.document, "2"), "After table");
    let dormant = find(&editor.document, "1").clone();
    assert!(editor.document.content_control_is_dormant(&dormant));
    let second = find(&editor.document, "2").range.start.clone();
    editor.set_caret(second.paragraph, second.offset);
    editor.insert_text("New ").unwrap();
    assert!(editor.document.content_controls.contains_key(&dormant.id));
}
#[test]
fn floating_picture_control_keeps_its_wrapper_anchor_and_binding() {
    let mut document = picture_document();
    let first = find(&document, "1").id.clone();
    let paragraph_id = find(&document, "1").range.start.paragraph.clone();
    let resource = document
        .content_control_picture(&document.content_controls[&first])
        .unwrap()
        .id
        .clone();
    let image_id = NodeId::new();
    let image = BlockImage {
        id: image_id.clone(),
        resource,
        width_mm: 32.5,
        height_mm: 18.25,
        alt_text: None,
        effects: Default::default(),
        wrap: ImageWrap::Square,
        alignment: ImageAlignment::Left,
        anchor: Some(ImageAnchor {
            paragraph: paragraph_id.clone(),
            reference: AnchorReference::Page,
            x_mm: 25.0,
            y_mm: 35.0,
            gap_mm: 2.0,
            locked: true,
        }),
    };
    if let Block::Paragraph(paragraph) = &mut document.sections[0].blocks[0] {
        paragraph.content.clear();
    }
    document.sections[0].blocks.insert(0, Block::Image(image));
    let control = document.content_controls.get_mut(&first).unwrap();
    control.range.end.offset = 0;
    control.members = vec![image_id];
    let reopened = cycle(&document);
    assert_eq!(reopened.content_controls.len(), 2);
    let control = find(&reopened, "1");
    assert_eq!(control.range.start, control.range.end);
    assert_eq!(control.members.len(), 1);
    let mut editor = EditorState::new(reopened);
    let binding = find(&editor.document, "1").binding.clone().unwrap();
    editor
        .set_content_binding_value(&binding, BLUE_PICTURE)
        .unwrap();
    let reopened = cycle(&editor.document);
    let image = reopened
        .sections
        .iter()
        .flat_map(|section| &section.blocks)
        .find_map(|block| {
            if let Block::Image(image) = block {
                Some(image)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!((image.width_mm, image.height_mm), (32.5, 18.25));
    let anchor = image.anchor.as_ref().unwrap();
    assert_eq!(
        (anchor.x_mm, anchor.y_mm, anchor.gap_mm, anchor.locked),
        (25.0, 35.0, 2.0, true)
    );
    assert_eq!(
        reopened
            .content_control_picture(find(&reopened, "1"))
            .unwrap()
            .data,
        decode_content_picture(BLUE_PICTURE).unwrap().data
    );
}
#[test]
fn rejecting_deleted_bound_control_refreshes_current_xml_instead_of_overwriting_newer_values() {
    let body=format!("<w:tbl><w:tr><w:trPr><w:del w:id=\"55\" w:author=\"Teacher\"/></w:trPr><w:tc><w:p>{}</w:p></w:tc></w:tr><w:tr><w:tc><w:p><w:r><w:t>Live row</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:p>{}</w:p>",control("1","<w:text/>","Old snapshot",true),control("2","<w:text/>","Old",true));
    let mut editor = EditorState::new(load(package(&body)));
    let dormant = find(&editor.document, "1").id.clone();
    let peer = find(&editor.document, "2").id.clone();
    let binding = find(&editor.document, "2").binding.clone().unwrap();
    assert!(editor
        .document
        .content_control_is_dormant(&editor.document.content_controls[&dormant]));
    editor
        .set_content_control_value(&peer, "New live value")
        .unwrap();
    let deletion = editor
        .document
        .review
        .tracked_changes
        .values()
        .find(|change| {
            change.effects.iter().any(|effect| {
                matches!(
                    effect,
                    RevisionEffect::Table {
                        target: TableRevisionTarget::DeleteRow { .. },
                        ..
                    }
                )
            })
        })
        .unwrap()
        .id
        .clone();
    let before = editor.document.clone();
    editor.resolve_tracked_change(&deletion, false).unwrap();
    assert_eq!(text(&editor.document, "1"), "New live value");
    assert_eq!(text(&editor.document, "2"), "New live value");
    assert_eq!(
        editor.document.bound_content_value(&binding).unwrap(),
        "New live value"
    );
    assert!(editor.undo());
    assert_eq!(editor.document, before);
    assert!(editor.redo());
    assert_eq!(text(&editor.document, "1"), "New live value");
    let mut accepting = EditorState::new(before.clone());
    accepting.resolve_tracked_change(&deletion, true).unwrap();
    assert!(!accepting.document.content_controls.contains_key(&dormant));
    assert!(accepting.undo());
    assert_eq!(accepting.document, before);
}
