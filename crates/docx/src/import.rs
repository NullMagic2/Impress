//! Import OOXML package parts into editable document nodes and styles.
use quick_xml::{events::Event, Reader};
use model::{
    Block, Border, BorderSet, BorderStyle, CellVerticalAlignment, CharacterProperties, Document,
    Equation, EquationNode, Field, FieldKind, HeaderFooter, Inline, InlineImage, LimitPosition,
    ListDefinition, ListFormat, ListLevel,
    NodeId, Paragraph, ParagraphAlignment, ParagraphDirection, ParagraphProperties, Resource,
    PreservedPackagePart, ResourceId, Section, SectionStart, Table, TableAlignment, TableCell,
    TableCellMargins, TableProperties, TableRow, TextRun, VerticalPosition,
};
use format::{FormatError, FormatResult, ReadSeek};
use std::{collections::BTreeMap, io::Read};
use zip::ZipArchive;

#[path = "import_review.rs"]
mod review;
#[path = "theme_fonts.rs"]
mod theme_fonts;
#[path = "import_controls.rs"]
mod controls;
#[path = "font_import.rs"]
mod fonts;
#[path = "import_alternate.rs"]
pub(super) mod alternate;
use review::ImportReview;
#[path="import_tracked.rs"]
mod tracked;


/// A DOCX tab entry either replaces a position or clears an inherited one.
#[derive(Clone, Debug)]
struct TabEdit {
    position_mm: f32,
    stop: Option<model::TabStop>,
}

fn merge_tab_edits(mut tabs: Vec<model::TabStop>, edits: &[TabEdit]) -> Vec<model::TabStop> {
    for edit in edits {
        tabs.retain(|tab| (tab.position_mm * TWIPS_PER_MM).round() != (edit.position_mm * TWIPS_PER_MM).round());
        if let Some(stop) = &edit.stop { tabs.push(stop.clone()); }
    }
    tabs.sort_by(|a, b| a.position_mm.total_cmp(&b.position_mm));
    tabs
}

fn imported_style_tabs(id: &model::StyleId, sheet: &model::StyleSheet, edits: &BTreeMap<model::StyleId, Vec<TabEdit>>, seen: &mut std::collections::BTreeSet<model::StyleId>) -> Vec<model::TabStop> {
    if !seen.insert(id.clone()) { return Vec::new(); }
    let Some(style) = sheet.get(id) else { return Vec::new(); };
    let inherited = style.based_on.as_ref().map(|base| imported_style_tabs(base, sheet, edits, seen)).unwrap_or_default();
    match edits.get(id) {
        Some(edits) => merge_tab_edits(inherited, edits),
        None => style.paragraph.tabs.clone().unwrap_or(inherited),
    }
}

fn resolve_paragraph_tabs(document: &mut Document, context: &ImportContext) {
    fn visit(blocks: &mut [Block], styles: &model::StyleSheet, edits: &BTreeMap<NodeId, Vec<TabEdit>>) {
        for block in blocks {
            match block {
                Block::Paragraph(paragraph) => {
                    if let Some(edits) = edits.get(&paragraph.id) {
                        let inherited = styles.resolve_paragraph(paragraph.style.as_ref()).tabs.unwrap_or_default();
                        paragraph.properties.tabs = Some(merge_tab_edits(inherited, edits));
                    }
                }
                Block::Table(table) => for row in &mut table.rows { for cell in &mut row.cells { visit(&mut cell.blocks, styles, edits); } },
                _ => {}
            }
        }
    }
    let edits = context.tab_edits.borrow();
    for section in &mut document.sections {
        visit(&mut section.blocks, &document.styles, &edits);
        for story in [section.headers.default.as_mut(), section.headers.first.as_mut(), section.headers.even.as_mut(), section.footers.default.as_mut(), section.footers.first.as_mut(), section.footers.even.as_mut()].into_iter().flatten() {
            visit(&mut story.blocks, &document.styles, &edits);
        }
    }
    for note in document.review.footnotes.values_mut() { visit(&mut note.blocks, &document.styles, &edits); }
}

const EMU_PER_MM: f32 = 36_000.0;
const TWIPS_PER_MM: f32 = 1440.0 / 25.4;

#[derive(Clone, Debug, Default)]
struct ImportContext {
    source_paragraphs: std::rc::Rc<std::cell::RefCell<BTreeMap<usize,Paragraph>>>,
    source_members: std::rc::Rc<std::cell::RefCell<BTreeMap<usize,NodeId>>>,
    theme_fonts: std::rc::Rc<theme_fonts::ThemeFonts>,
    content_controls: std::rc::Rc<std::cell::RefCell<BTreeMap<NodeId,model::ContentControl>>>,
    tab_edits: std::rc::Rc<std::cell::RefCell<BTreeMap<NodeId, Vec<TabEdit>>>>,
    images: BTreeMap<String, ResourceId>,
    embedded: BTreeMap<String, ResourceId>,
    anchored: std::rc::Rc<std::cell::RefCell<Vec<model::BlockImage>>>,
    in_table:bool,
    table_styles: std::rc::Rc<ImportedTableStyles>,
    links: BTreeMap<String, String>,
    note_ids: BTreeMap<String,NodeId>,
    annotations: std::rc::Rc<std::cell::RefCell<ImportReview>>,
    part: String,
    resources: BTreeMap<ResourceId, Resource>,
    lists: BTreeMap<NodeId, ListDefinition>,
    num_to_list: BTreeMap<i32, NodeId>,
    named_lists: BTreeMap<NodeId, model::NamedListStyle>,
    headers: BTreeMap<String, HeaderFooter>,
    footers: BTreeMap<String, HeaderFooter>,
}

/// Parsing a before-value must not register live annotations, controls or drawings.
fn snapshot_context(context:&ImportContext)->ImportContext{
    let mut isolated=context.clone();isolated.annotations=Default::default();isolated.anchored=Default::default();isolated.tab_edits=Default::default();isolated.content_controls=Default::default();isolated.source_paragraphs=Default::default();isolated.source_members=Default::default();isolated
}

pub fn import_docx(input: &mut dyn ReadSeek) -> FormatResult<Document> {
    let mut zip = ZipArchive::new(input).map_err(zip_err)?;
    format::validate_zip_directory(&mut zip, "DOCX")?;
    let xml = read_required_text(&mut zip, "word/document.xml")?;
    let relationships_xml = read_optional_text(&mut zip, "word/_rels/document.xml.rels")?.unwrap_or_default();
    let relationships = parse_relationships(&relationships_xml)?;

    let mut context = ImportContext { part: "word/document.xml".into(), ..Default::default() };
    let style_tree = read_optional_text(&mut zip, "word/styles.xml")?
        .map(|styles| parse_supported_tree(&styles)).transpose()?;
    if let Some(root) = style_tree.as_ref().and_then(|tree| find_descendant(tree, "w:styles")) {
        context.table_styles = std::rc::Rc::new(ImportedTableStyles::read(root));
    }
    context.links = relationship_links(&relationships);
    context.theme_fonts = theme_fonts::ThemeFonts::load(&mut zip,&relationships)?;
    let comment_part = |kind, fallback: &str| relationships.values()
        .find(|rel| !rel.external && rel.kind.ends_with(kind))
        .map(|rel| relationship_path(&rel.target)).unwrap_or_else(|| fallback.to_owned());
    if let Some(comments) = read_optional_text(&mut zip, &comment_part("/comments", "word/comments.xml"))? {
        context.annotations.borrow_mut().read_comments(&parse_supported_tree(&comments)?);
        if let Some(extra) = read_optional_text(&mut zip, &comment_part("/commentsExtended", "word/commentsExtended.xml"))? {
            if let Ok(tree) = parse_supported_tree(&extra) {
                context.annotations.borrow_mut().read_comment_extensions(&tree);
            }
        }
    }
    load_relationship_resources(&mut zip, &relationships, &mut context)?;
    let font_parts=fonts::load(&mut zip,&relationships,&mut context)?;

    if let Some(numbering_xml) = read_optional_text(&mut zip, "word/numbering.xml")? {
        let parsed = parse_numbering(&numbering_xml)?;
        context.lists = parsed.lists;
        context.num_to_list = parsed.instances;
        context.named_lists = parsed.named;
    }

    for (stem,kind,prefix) in [("footnote",model::NoteKind::Footnote,""),("endnote",model::NoteKind::Endnote,"end:")] {
    let part=format!("word/{stem}s.xml");
    if let Some(notes_xml)=read_optional_text(&mut zip,&part)? {
        let tree=parse_supported_tree(&notes_xml)?;
        if let Some(root)=find_descendant(&tree,&format!("w:{stem}s")) {
            for note in root.children_named(&format!("w:{stem}")) {
                if note.attr("w:type").is_some_and(|t|t!="normal"){continue;}
                if let Some(source)=note.attr("w:id"){context.note_ids.insert(format!("{prefix}{source}"),NodeId::new());}
            }
            let mut note_context=context.clone();note_context.part=part.clone();
            if let Some(rels)=read_optional_text(&mut zip,&format!("word/_rels/{stem}s.xml.rels"))? {
                let rels=parse_relationships(&rels)?;note_context.links=relationship_links(&rels);load_relationship_resources(&mut zip,&rels,&mut note_context)?;context.resources.extend(note_context.resources.clone());
            }
            for note in root.children_named(&format!("w:{stem}")) {
                let Some(id)=note.attr("w:id").and_then(|id|context.note_ids.get(&format!("{prefix}{id}"))).cloned() else{continue;};
                let mut normalized=note.clone();normalize_note_mark(&mut normalized);
                let mut blocks=Vec::new();for child in &normalized.children{match child.name.as_str(){"w:p"=>blocks.push(Block::Paragraph(parse_paragraph(child,&note_context))),"w:tbl"=>blocks.push(Block::Table(parse_table(child,&note_context))),_=>{}}}
                controls::project_blocks(&normalized,&blocks,&note_context);
                context.annotations.borrow_mut().state.footnotes.insert(id.clone(),model::Footnote{kind,id,blocks: blocks.into()});
            }
        }
    }
    }
    for (id, relation) in &relationships {
        if !relation.external && (relation.kind.ends_with("/header") || relation.kind.ends_with("/footer")) {
            let path = relationship_path(&relation.target);
            if let Some(part_xml) = read_optional_text(&mut zip, &path)? {
                let rel_path = relationship_part_path(&path);
                let part_relationships = read_optional_text(&mut zip, &rel_path)?
                    .map(|xml| parse_relationships(&xml)).transpose()?.unwrap_or_default();
                let mut part_context = context.clone();
                part_context.part = path.clone();
                part_context.links = relationship_links(&part_relationships);
                load_relationship_resources(&mut zip, &part_relationships, &mut part_context)?;
                // Share resources discovered through header/footer relationship parts
                // with the document-wide resource store.
                context.resources.extend(part_context.resources.clone());
                let blocks = parse_header_footer_blocks(&part_xml, &part_context)?;
                let value = HeaderFooter { id: NodeId::new(), blocks: blocks.into()};
                if relation.kind.ends_with("/header") { context.headers.insert(id.clone(), value); }
                else { context.footers.insert(id.clone(), value); }
            }
        }
    }

    let mut document = parse_document_xml(&xml, &context)?;
    if context.annotations.borrow().history_limit_exceeded{return Err(invalid("Tracked changes exceed the supported history size limit"));}
    document.review = context.annotations.borrow_mut().finish();
    document.custom_xml = controls::load_stores(&mut zip)?;
    document.content_controls=context.content_controls.borrow().clone();
    let mut default_style=model::StyleId::from(model::STYLE_NORMAL);
    if let Some(tree) = &style_tree {
        if let Some(root)=find_descendant(tree,"w:styles"){
            if let Some(id)=root.children_named("w:style").find(|style|style.attr("w:type")==Some("paragraph")&&style.attr("w:default").is_some_and(|value|matches!(value,"1"|"true"|"on"))).and_then(|style|style.attr("w:styleId")){default_style=model::StyleId(id.into());}
        }
        import_styles(tree, &mut document, &context);
    }
    for (id, mut named) in std::mem::take(&mut context.named_lists) {
        named.headings.retain(|_, heading| document.styles.get(heading).is_some_and(|style| {
            style.kind == model::StyleKind::Paragraph && style.paragraph.list_id.as_ref() == Some(&id)
        }));
        context.lists.remove(&id);
        document.styles.list_styles.insert(id, named);
    }
    resolve_paragraph_tabs(&mut document, &context);
    tracked::finish(&mut document,&context,&default_style);
    tracked::paragraph_boundaries(&mut document,&context);
    if document.review.tracked_changes.len()>model::MAX_TRACKED_CHANGES || model::tracked_changes_bytes(&document.review.tracked_changes).unwrap_or(usize::MAX)>model::MAX_TRACKED_HISTORY_BYTES{return Err(invalid("Tracked changes exceed the supported history size limit"));}
    if let Some(settings) = read_optional_text(&mut zip, "word/settings.xml")? {
        let tree = parse_supported_tree(&settings)?;
        document.review.track_changes = find_descendant(&tree, "w:trackRevisions")
            .is_some_and(|node| node.attr("w:val").is_none_or(|value| matches!(value, "true" | "1" | "on")));
        if let Some(value) = find_descendant(&tree, "w:defaultTabStop").and_then(|node| node.attr("w:val"))
            .and_then(|value| value.parse::<f32>().ok()).map(|twips| twips * 25.4 / 1440.0)
            .filter(|value| value.is_finite() && (model::MIN_TAB_INTERVAL_MM..=model::MAX_TAB_INTERVAL_MM).contains(value)) {
            if let Some(normal) = document.styles.styles.get_mut(&model::StyleId::from(model::STYLE_NORMAL)) { normal.paragraph.default_tab_mm = Some(value); }
        }
    }
    crate::compatibility::audit_xml(&xml,"word/document.xml",&mut document.compatibility);
    context.theme_fonts.audit(&mut document);
    document.resources = std::mem::take(&mut context.resources);
    document.lists = std::mem::take(&mut context.lists);
    controls::finish(&mut document,&context);

    let report_parts=zip.file_names().filter(|n|n.starts_with("word/")&&n.ends_with(".xml")&&(n.contains("header")||n.contains("footer")||(n.ends_with("footnotes.xml")||n.ends_with("endnotes.xml"))||n.ends_with("styles.xml"))).map(str::to_owned).collect::<Vec<_>>();
    for part in report_parts{if let Some(xml)=read_optional_text(&mut zip,&part)?{crate::compatibility::audit_xml(&xml,&part,&mut document.compatibility);}}
    // Keep package parts that Impress does not semantically own. This allows
    // themes, settings, custom XML, document properties and other unrelated
    // OOXML data to survive an open/save cycle. Generated semantic parts are
    // either merged explicitly on export or deliberately regenerated.
    let names: Vec<String> = zip.file_names().map(str::to_owned).collect();
    for name in names {
        if font_parts.contains(&name) || !should_preserve_part(&name) { continue; }
        if let Ok(member) = zip.by_name(&name) {
            let mut data = Vec::new();
            member.take(format::MAX_PACKAGE_MEMBER + 1).read_to_end(&mut data)?;
            format::check_member_len(data.len(), "DOCX")?;
            document.preserved_parts.insert(name.clone(), PreservedPackagePart {
                media_type: None,
                data_base64: encode_base64(&data).into(),
            });
        }
    }
    Ok(document)
}

#[derive(Clone, Debug, Default)]
struct Relationship {
    kind: String,
    target: String,
    external: bool,
}

fn load_relationship_resources<R: Read + std::io::Seek>(
    zip: &mut ZipArchive<R>,
    relationships: &BTreeMap<String, Relationship>,
    context: &mut ImportContext,
) -> FormatResult<()> {
    for (id, relation) in relationships {
        let is_image=relation.kind.ends_with("/image");
        let is_embedded=relation.kind.ends_with("/oleObject")||relation.kind.ends_with("/package");
        if !(is_image||is_embedded) || relation.external { continue; }
        let path = relationship_path(&relation.target);
        if let Ok(member) = zip.by_name(&path) {
            let mut data = Vec::new();
            member.take(format::MAX_PACKAGE_MEMBER + 1).read_to_end(&mut data)?;
            format::check_member_len(data.len(), "DOCX")?;
            // Reuse identical package targets within a part when possible.
            let existing = context.resources.iter().find_map(|(resource_id, resource)| {
                ((resource.media_type.starts_with("image/")==is_image)&&resource.data.as_ref()==&data).then(|| resource_id.clone())
            });
            let resource_id = existing.unwrap_or_else(ResourceId::new);
            if !context.resources.contains_key(&resource_id) {
                let media_type = if is_image {media_type_for_name(&path).to_owned()}
                    else if relation.kind.ends_with("/package") {"application/vnd.openxmlformats-officedocument.package".to_owned()}
                    else {"application/vnd.openxmlformats-officedocument.oleObject".to_owned()};
                let original_name = path.rsplit('/').next().map(str::to_owned);
                context.resources.insert(resource_id.clone(), Resource { font_faces: Vec::new(),  id: resource_id.clone(), media_type, original_name, data: data.into() });
            }
            if is_image {context.images.insert(id.clone(),resource_id);}else{context.embedded.insert(id.clone(),resource_id);}
        }
    }
    Ok(())
}

fn normalize_note_mark(note:&mut XmlNode) {
    let Some(p)=note.children.iter_mut().find(|n|n.name=="w:p")else{return;};
    let mut found=false;let mut separator=false;
    for run in &mut p.children {
        if run.name!="w:r"{continue;}
        let mut children=Vec::new();
        for mut child in std::mem::take(&mut run.children) {
            if child.name=="w:footnoteRef"||child.name=="w:endnoteRef" {found=true;continue;}
            if found&&!separator {
                if child.name=="w:t" {if child.text.starts_with(' '){child.text.remove(0);}separator=true;}
                else if child.name=="w:tab"{separator=true;continue;}
                else if child.name!="w:rPr"{separator=true;}
            }
            children.push(child);
        }run.children=children;
    }
}

fn relationship_part_path(part_path: &str) -> String {
    match part_path.rsplit_once('/') {
        Some((dir, file)) => format!("{dir}/_rels/{file}.rels"),
        None => format!("_rels/{part_path}.rels"),
    }
}

fn should_preserve_part(name: &str) -> bool {
    // These three package-control parts are retained for relationship/content-
    // type merging even though they are regenerated at export.
    if matches!(name, "[Content_Types].xml" | "_rels/.rels" | "word/_rels/document.xml.rels" | "word/styles.xml") {
        return true;
    }
    if name == "word/fontTable.xml" || name == "word/_rels/fontTable.xml.rels" || name.starts_with("word/fonts/") { return false; }
    if name == "word/endnotes.xml" || name == "word/_rels/endnotes.xml.rels" || name == "word/footnotes.xml" || name == "word/_rels/footnotes.xml.rels" || name == "word/comments.xml" || name == "word/commentsExtended.xml" || name == "word/document.xml" || name == "word/numbering.xml" || name.starts_with("word/media/")
        || name.starts_with("word/header") || name.starts_with("word/footer")
        || name.starts_with("word/_rels/header") || name.starts_with("word/_rels/footer") {
        return false;
    }
    true
}

fn parse_relationships(xml: &str) -> FormatResult<BTreeMap<String, Relationship>> {
    if xml.trim().is_empty() { return Ok(BTreeMap::new()); }
    let tree = parse_xml_tree(xml)?;
    let mut result = BTreeMap::new();
    collect_relationships(&tree, &mut result);
    Ok(result)
}

fn collect_relationships(node: &XmlNode, out: &mut BTreeMap<String, Relationship>) {
    if node.name.ends_with("Relationship") {
        if let (Some(id), Some(kind), Some(target)) = (node.attr("Id"), node.attr("Type"), node.attr("Target")) {
            out.insert(id.to_owned(), Relationship { kind: kind.to_owned(), target: target.to_owned(), external: node.attr("TargetMode").is_some_and(|v|v.eq_ignore_ascii_case("External")) });
        }
    }
    for child in &node.children { collect_relationships(child, out); }
}

fn relationship_path(target: &str) -> String {
    let target = target.trim_start_matches('/');
    if target.starts_with("word/") { target.to_owned() }
    else if let Some(rest) = target.strip_prefix("../") { rest.to_owned() }
    else { format!("word/{target}") }
}

fn read_required_text<R: Read + std::io::Seek>(zip: &mut ZipArchive<R>, name: &str) -> FormatResult<String> {
    let mut text = String::new();
    zip.by_name(name).map_err(zip_err)?.take(format::MAX_XML_MEMBER + 1).read_to_string(&mut text)?;
    if text.len() as u64 > format::MAX_XML_MEMBER { return Err(invalid("XML part exceeds the size limit")); }
    Ok(text)
}

fn read_optional_text<R: Read + std::io::Seek>(zip: &mut ZipArchive<R>, name: &str) -> FormatResult<Option<String>> {
    match zip.by_name(name) {
        Ok(member) => {
            let mut text = String::new();
            member.take(format::MAX_XML_MEMBER + 1).read_to_string(&mut text)?;
            if text.len() as u64 > format::MAX_XML_MEMBER { return Err(invalid("XML part exceeds the size limit")); }
            Ok(Some(text))
        }
        Err(zip::result::ZipError::FileNotFound) => Ok(None),
        Err(error) => Err(zip_err(error)),
    }
}

#[derive(Clone, Debug, Default)]
pub(super) struct XmlNode {
    pub(super) name: String,
    pub(super) attrs: BTreeMap<String, String>,
    pub(super) text: String,
    pub(super) children: Vec<XmlNode>,
    pub(super) choice_supported: bool,
}

impl XmlNode {
    pub(super) fn child(&self, name: &str) -> Option<&XmlNode> { self.children.iter().find(|child| child.name == name) }
    fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a XmlNode> + 'a { self.children.iter().filter(move |child| child.name == name) }
    pub(super) fn attr(&self, name: &str) -> Option<&str> { self.attrs.get(name).map(String::as_str) }
    fn descendant_text(&self) -> String {
        let mut out = self.text.clone();
        for child in &self.children { out.push_str(&child.descendant_text()); }
        out
    }
}

fn parse_document_xml(xml: &str, context: &ImportContext) -> FormatResult<Document> {
    let tree = parse_supported_tree(xml)?;
    let document_node = find_descendant(&tree, "w:document").ok_or_else(|| invalid("word/document.xml has no w:document element"))?;
    let body = document_node.child("w:body").ok_or_else(|| invalid("word/document.xml has no w:body element"))?;

    let mut sections = Vec::new();
    let mut section = Section::default();
    let mut body_items=Vec::new();flatten_body(body,None,&mut body_items);
    for (child,managed_contents) in body_items {
        match child.name.as_str() {
            "w:p" => {
                let mut paragraph = parse_paragraph(child, context);
                if let Some(tag) = managed_contents {
                    let mut annotations = context.annotations.borrow_mut();
                    let review = &mut annotations.state;
                    if review.contents_paragraphs.is_empty() {
                        if let Some((options, automatic)) = crate::parse_contents_tag(tag) {
                            review.contents_options = options;
                            review.automatic_contents = automatic;
                        }
                        let title = model::inline_text(&paragraph.content);
                        if !title.trim().is_empty() { review.contents_options.title = title; }
                    } else if let Some(tab) = paragraph.properties.tabs.as_ref().and_then(|tabs| {
                        tabs.iter().find(|tab| tab.alignment == model::TabAlignment::Right)
                    }) {
                        paragraph.properties.contents_leader = Some(tab.leader);
                    }
                    review.contents_paragraphs.push(paragraph.id.clone());
                }
                let section_break = child.child("w:pPr").and_then(|p| p.child("w:sectPr"));
                let has_content = section_break.is_none() || !paragraph.content.is_empty() || paragraph.style.is_some();
                if (paragraph.content.is_empty() || (paragraph.content.len() == 1 && matches!(paragraph.content[0], Inline::PaginationBreak { .. }))) && section_break.is_none() {
                    if let Some(kind)=find_descendant(child,"w:br").and_then(|n|n.attr("w:type")) {
                        match kind {"page"=>{section.blocks.push(Block::PageBreak{id:paragraph.id.clone()});continue;},"column"=>{section.blocks.push(Block::ColumnBreak{id:paragraph.id.clone()});continue;},_=>{}}
                    }
                }
                if has_content {
                    let mut anchored=context.anchored.borrow_mut();let mut i=0;
                    while i<anchored.len(){if anchored[i].anchor.as_ref().is_some_and(|a|a.paragraph==paragraph.id){section.blocks.push(Block::Image(anchored.remove(i)));}else{i+=1;}}
                    section.blocks.push(Block::Paragraph(paragraph));
                }
                if let Some(sect_pr) = section_break {
                    apply_section_properties(&mut section, sect_pr, context);
                    sections.push(section);
                    section = Section::default();
                }
            }
            "w:tbl" => section.blocks.push(Block::Table(parse_table(child, context))),
            "m:oMathPara" => {
                if let Some(math) = child.child("m:oMath") { section.blocks.push(Block::Equation(Equation::block(parse_math_container(math)))); }
            }
            "w:sectPr" => apply_section_properties(&mut section, child, context),
            _ => {}
        }
    }
    sections.push(section); // Preserve an intentionally empty final section.
    let mut document = Document::default();
    document.sections = sections;
    let body_blocks=document.sections.iter().flat_map(|section|section.blocks.iter().cloned()).collect::<Vec<_>>();
    controls::project_blocks(body,&body_blocks,context);
    Ok(document)
}

/// Expand block content controls without treating a nested table's paragraphs
/// as body siblings. Managed contents membership is retained through DOCX.
fn flatten_body<'a>(node:&'a XmlNode,managed:Option<&'a str>,out:&mut Vec<(&'a XmlNode,Option<&'a str>)>) {
    for child in &node.children {
        match child.name.as_str() {
            "w:p"|"w:tbl"|"m:oMathPara"|"w:sectPr"=>out.push((child,managed)),
            "w:sdt"=>{let tag = child.child("w:sdtPr").and_then(|n|n.child("w:tag")).and_then(|n|n.attr("w:val")).filter(|tag| crate::is_contents_tag(tag));if let Some(content)=child.child("w:sdtContent"){flatten_body(content,managed.or(tag),out);}},
            "w:sdtContent"|"w:customXml"|"w:smartTag"|"w:ins"|"w:moveTo"=>flatten_body(child,managed,out),
            _=>{}
        }
    }
}

fn parse_header_footer_blocks(xml: &str, context: &ImportContext) -> FormatResult<Vec<Block>> {
    let tree = parse_supported_tree(xml)?;
    let root = find_descendant(&tree, "w:hdr").or_else(|| find_descendant(&tree, "w:ftr")).ok_or_else(|| invalid("header/footer XML has no root"))?;
    let mut blocks = Vec::new();
    let mut items = Vec::new();
    flatten_body(root, None, &mut items);
    for (child, _) in items {
        match child.name.as_str() {
            "w:p" => {
                let paragraph = parse_paragraph(child, context);
                let mut anchored = context.anchored.borrow_mut();
                let mut index = 0;
                while index < anchored.len() {
                    if anchored[index].anchor.as_ref().is_some_and(|anchor| anchor.paragraph == paragraph.id) {
                        blocks.push(Block::Image(anchored.remove(index)));
                    } else { index += 1; }
                }
                blocks.push(Block::Paragraph(paragraph));
            },
            "w:tbl" => blocks.push(Block::Table(parse_table(child, context))),
            "m:oMathPara" => if let Some(math) = child.child("m:oMath") { blocks.push(Block::Equation(Equation::block(parse_math_container(math)))); },
            _ => {}
        }
    }
    controls::project_blocks(root,&blocks,context);
    Ok(blocks)
}

fn parse_supported_tree(xml: &str) -> FormatResult<XmlNode> {
    let mut tree = parse_xml_tree(xml)?;
    alternate::resolve(&mut tree);
    Ok(tree)
}

pub(super) fn parse_xml_tree(xml: &str) -> FormatResult<XmlNode> {
    if xml.len() as u64 > format::MAX_XML_MEMBER { return Err(invalid("XML part exceeds the size limit")); }
    let mut reader = Reader::from_str(xml); reader.config_mut().trim_text(false);
    let mut stack = vec![XmlNode { name: "#root".into(), ..Default::default() }];
    let mut namespaces = vec![BTreeMap::<String,String>::new()];
    let mut nodes = 0usize;
    loop {
        let event = reader.read_event().map_err(xml_err)?;
        let open = matches!(&event,Event::Start(_));
        match event {
            Event::Start(e) | Event::Empty(e) => {
                nodes += 1;
                if nodes > 1_000_000 || stack.len() > 128 { return Err(invalid("XML node/depth limit exceeded")); }
                let mut scope = namespaces.last().cloned().unwrap_or_default();
                let mut node = node_from_start(&e)?;
                for (key,value) in &node.attrs {
                    if key=="xmlns" { scope.insert(String::new(),value.clone()); }
                    else if let Some(prefix)=key.strip_prefix("xmlns:") { scope.insert(prefix.into(),value.clone()); }
                }
                node.name=canonical_name(&node.name,&scope,false);
                if node.name == "mc:Choice" {
                    node.choice_supported = alternate::requirements_supported(node.attr("Requires"), &scope);
                }
                node.attrs=node.attrs.into_iter().map(|(k,v)|(canonical_name(&k,&scope,true),v)).collect();
                if open { stack.push(node); namespaces.push(scope); }
                else { stack.last_mut().expect("xml parent").children.push(node); }
            }
            Event::Text(t) => stack.last_mut().expect("xml root").text.push_str(&quick_xml::escape::unescape(t.as_ref()).map_err(xml_err)?),
            Event::CData(t) => stack.last_mut().expect("xml root").text.push_str(t.as_ref()),
            Event::GeneralRef(r) => {
                // quick-xml emits references separately from ordinary Text.
                // Resolve only XML's predefined and numeric references; never a DTD.
                let encoded=format!("&{};",r.as_ref());
                let value=quick_xml::escape::unescape(&encoded).map_err(xml_err)?;
                stack.last_mut().expect("xml root").text.push_str(&value);
            }
            Event::End(_) => {
                if stack.len()<=1 { return Err(invalid("Unmatched XML closing element")); }
                let node=stack.pop().expect("node");namespaces.pop();stack.last_mut().expect("parent").children.push(node);
            }
            Event::DocType(_) => return Err(invalid("DTD/entity declarations are not supported")),
            Event::Eof => break,
            _ => {}
        }
    }
    if stack.len()!=1 { return Err(invalid("Unclosed XML element")); }
    let root=stack.pop().expect("root");
    if root.children.len()!=1 || !root.text.trim().is_empty() { return Err(invalid("Expected one XML root element")); }
    Ok(root)
}
fn canonical_name(name:&str,scope:&BTreeMap<String,String>,attribute:bool)->String {
    let (prefix,local)=name.split_once(':').unwrap_or(("",name));
    if attribute && prefix.is_empty(){return name.into();}
    if prefix == "mc" && !scope.get(prefix).is_some_and(|uri| uri == alternate::MARKUP_COMPATIBILITY_NAMESPACE) {
        return format!("unrecognized-mc:{local}");
    }
    let Some(uri)=scope.get(prefix) else {return name.into();};
    let canonical=match uri.as_str() {
        "http://schemas.openxmlformats.org/markup-compatibility/2006" => "mc",
        "urn:impress:revisions:1" | "urn:impress:comments:1" | "urn:impress:tables:1" => "impress",
        "urn:schemas-microsoft-com:vml" => "v",
        "http://schemas.microsoft.com/office/word/2010/wordprocessingShape" => "wps",
        "http://schemas.microsoft.com/office/word/2010/wordml" => "w14",
        "http://schemas.microsoft.com/office/word/2012/wordml" => "w15",
        "http://schemas.openxmlformats.org/wordprocessingml/2006/main"|"http://purl.oclc.org/ooxml/wordprocessingml/main"=>"w",
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships"|"http://purl.oclc.org/ooxml/officeDocument/relationships"=>"r",
        "http://schemas.openxmlformats.org/officeDocument/2006/math"|"http://purl.oclc.org/ooxml/officeDocument/math"=>"m",
        "http://schemas.openxmlformats.org/drawingml/2006/main"|"http://purl.oclc.org/ooxml/drawingml/main"=>"a",
        "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"|"http://purl.oclc.org/ooxml/drawingml/wordprocessingDrawing"=>"wp",
        "http://schemas.openxmlformats.org/drawingml/2006/picture"|"http://purl.oclc.org/ooxml/drawingml/picture"=>"pic",
        "http://schemas.openxmlformats.org/package/2006/relationships"|"http://schemas.openxmlformats.org/package/2006/content-types"=>"",
        _=>return name.into(),
    };
    if canonical.is_empty(){local.into()}else{format!("{canonical}:{local}")}
}

fn node_from_start(e: &quick_xml::events::BytesStart<'_>) -> FormatResult<XmlNode> {
    let name = e.name().as_ref().to_owned();
    let mut attrs = BTreeMap::new();
    for attr in e.attributes().with_checks(true) {
        let attr = attr.map_err(xml_err)?;
        let key = attr.key.as_ref().to_owned();
        let value = quick_xml::escape::unescape(attr.value.as_ref()).map_err(xml_err)?.into_owned();
        attrs.insert(key, value);
    }
    Ok(XmlNode { name, attrs, ..Default::default() })
}

fn find_descendant<'a>(node: &'a XmlNode, name: &str) -> Option<&'a XmlNode> {
    if node.name == name { return Some(node); }
    node.children.iter().find_map(|child| find_descendant(child, name))
}

fn parse_paragraph(node:&XmlNode,context:&ImportContext)->Paragraph {
    let mut paragraph = review::parse_review_paragraph(node,context);
    if paragraph.content.is_empty() {
        if let Some(rpr) = node.child("w:pPr").and_then(|p| p.child("w:rPr")) {
            let mut mark = TextRun::plain("");
            mark.properties = parse_character_properties(Some(rpr), context);
            mark.style = rpr.child("w:rStyle").and_then(|s| s.attr("w:val")).map(|s| model::StyleId::from(s));
            paragraph.content.push(Inline::Text(mark));
        }
    }
    paragraph
}

fn parse_paragraph_properties(paragraph: &mut Paragraph, ppr: &XmlNode, context: &ImportContext) {
    paragraph.properties.outline_level = ppr.child("w:outlineLvl").and_then(|n| n.attr("w:val"))
        .and_then(|raw| raw.parse::<u8>().ok()).filter(|level| *level <= 9)
        .map(|level| if level == 9 { 0 } else { level + 1 });
    if let Some(style) = ppr.child("w:pStyle").and_then(|n| n.attr("w:val")) { paragraph.style = Some(model::StyleId(style.to_owned())); }
    if let Some(jc) = ppr.child("w:jc").and_then(|n| n.attr("w:val")) {
        paragraph.properties.alignment = match jc { "center" => Some(ParagraphAlignment::Center), "right" | "end" => Some(ParagraphAlignment::Right), "both" | "distribute" => Some(ParagraphAlignment::Justify), _ => Some(ParagraphAlignment::Left) };
    }
    if let Some(bidi) = on_off(ppr.child("w:bidi")) {
        paragraph.properties.direction = Some(if bidi { ParagraphDirection::RightToLeft } else { ParagraphDirection::LeftToRight });
    }
    paragraph.properties.keep_with_next = on_off(ppr.child("w:keepNext"));
    paragraph.properties.keep_lines_together = on_off(ppr.child("w:keepLines"));
    paragraph.properties.page_break_before = on_off(ppr.child("w:pageBreakBefore"));
    paragraph.properties.hyphenation = on_off(ppr.child("w:suppressAutoHyphens")).map(|suppress|!suppress);
    if let Some(widow) = ppr.child("w:widowControl") {
        paragraph.properties.widow_control = Some(widow.attr("w:val").is_none_or(|v| !matches!(v, "0" | "false" | "off")));
    }
    if let Some(tabs) = ppr.child("w:tabs") {
        let edits = tabs.children_named("w:tab").filter_map(|tab| {
            let position_mm = tab.attr("w:pos").and_then(parse_twips_mm)?;
            if tab.attr("w:val") == Some("clear") { return Some(TabEdit { position_mm, stop: None }); }
            let alignment = match tab.attr("w:val").unwrap_or("left") {
                "center" => model::TabAlignment::Center,
                "right" => model::TabAlignment::Right,
                "decimal" => model::TabAlignment::Decimal,
                "bar" => model::TabAlignment::Bar,
                _ => model::TabAlignment::Left,
            };
            let leader = match tab.attr("w:leader").unwrap_or("none") {
                "dot" => model::TabLeader::Dots,
                "hyphen" => model::TabLeader::Dashes,
                "underscore" => model::TabLeader::Line,
                "heavy" => model::TabLeader::Heavy,
                "middleDot" => model::TabLeader::MiddleDots,
                _ => model::TabLeader::None,
            };
            Some(TabEdit { position_mm, stop: Some(model::TabStop { position_mm, alignment, leader }) })
        }).collect::<Vec<_>>();
        paragraph.properties.tabs = Some(merge_tab_edits(Vec::new(), &edits));
        context.tab_edits.borrow_mut().insert(paragraph.id.clone(), edits);
    }
    if let Some(shd) = ppr.child("w:shd").and_then(|n| n.attr("w:fill")) { paragraph.properties.shading = Some(format!("#{shd}")); }
    if let Some(borders) = ppr.child("w:pBdr") { paragraph.properties.borders = Some(parse_border_set(borders)); }
    if let Some(ind) = ppr.child("w:ind") {
        paragraph.properties.left_indent_mm = ind.attr("w:left").or_else(|| ind.attr("w:start")).and_then(parse_twips_mm);
        paragraph.properties.right_indent_mm = ind.attr("w:right").or_else(|| ind.attr("w:end")).and_then(parse_twips_mm);
        if let Some(v) = ind.attr("w:firstLine").and_then(parse_twips_mm) { paragraph.properties.first_line_indent_mm = Some(v); }
        if let Some(v) = ind.attr("w:hanging").and_then(parse_twips_mm) { paragraph.properties.first_line_indent_mm = Some(-v); }
    }
    if let Some(spacing) = ppr.child("w:spacing") {
        if let Some(value) = spacing.attr("w:before").and_then(|v| v.parse::<f32>().ok()) {
            paragraph.properties.space_before_pt = Some(value / 20.0);
        }
        if let Some(value) = spacing.attr("w:after").and_then(|v| v.parse::<f32>().ok()) {
            paragraph.properties.space_after_pt = Some(value / 20.0);
        }
        if let Some(value) = spacing.attr("w:line").and_then(|v| v.parse::<f32>().ok()) {
            paragraph.properties.line_spacing = Some(value / 240.0);
        }
    }
    if let Some(num_pr) = ppr.child("w:numPr") {
        let level = num_pr.child("w:ilvl").and_then(|n| n.attr("w:val")).and_then(|v| v.parse::<u8>().ok()).unwrap_or(0);
        let num_id = num_pr.child("w:numId").and_then(|n| n.attr("w:val")).and_then(|v| v.parse::<i32>().ok());
        if let Some(list_id) = num_id.and_then(|id| context.num_to_list.get(&id)) {
            paragraph.properties.list_id = Some(list_id.clone());
            paragraph.properties.list_level = Some(level);
        }
    }
}

fn parse_character_properties(rpr: Option<&XmlNode>, context: &ImportContext) -> CharacterProperties {
    let mut props = CharacterProperties::default();
    if let Some(rpr) = rpr {
        if let Some(value) = rpr.child("w14:ligatures").and_then(|n| n.attr("w14:val")) {
            props.typography = crate::ligatures::from_word(value);
        }
        props.bold = on_off(rpr.child("w:b"));
        props.italic = on_off(rpr.child("w:i"));
        props.right_to_left = on_off(rpr.child("w:rtl"));
        props.underline = rpr.child("w:u").map(|n|!matches!(n.attr("w:val"),Some("none"|"0"|"false"|"off")));
        if let Some(sz) = rpr.child("w:sz").and_then(|n| n.attr("w:val")).and_then(|v| v.parse::<f32>().ok()) { props.font_size_pt = Some(sz / 2.0); }
        props.font_slots = context.theme_fonts.properties(rpr);
        props.font_family = props.font_slots.as_ref().and_then(|slots|slots.ascii.as_ref().or(slots.h_ansi.as_ref())).and_then(|slot|slot.family_for('a',None)).map(str::to_owned);
        if let Some(color) = rpr.child("w:color").and_then(|n| n.attr("w:val")) { props.color = Some(format!("#{color}")); }
        if let Some(fill)=rpr.child("w:shd").and_then(|n|n.attr("w:fill")){props.highlight=if fill=="auto"{Some("none".into())}else if fill.len()==6&&fill.bytes().all(|b|b.is_ascii_hexdigit()){Some(format!("#{fill}"))}else{None};}
        if let Some(name)=rpr.child("w:highlight").and_then(|n|n.attr("w:val")){props.highlight=docx_highlight(name).map(str::to_owned).or(props.highlight);}
        if let Some(lang) = rpr.child("w:lang").and_then(|n| n.attr("w:val")) { props.language = Some(lang.to_owned()); }
    }
    props
}

fn append_run(paragraph: &mut Paragraph, run: &XmlNode, context: &ImportContext) {
    let props = parse_character_properties(run.child("w:rPr"), context);
    if let Some(drawing) = run.child("w:drawing") {
        if let Some(image) = parse_drawing_image(drawing, context) {
            if let Some(anchor)=find_descendant(drawing,"wp:anchor") {
                if !context.in_table {
                    if let Some((a,wrap))=parse_floating_anchor(anchor,paragraph.id.clone()){
                        context.anchored.borrow_mut().push(model::BlockImage{id:image.id,resource:image.resource,width_mm:image.width_mm,height_mm:image.height_mm,alt_text:image.alt_text,effects:image.effects,alignment:model::ImageAlignment::Left,wrap,anchor:Some(a)});
                        // Drawing runs with additional text continue below.
                    }else{paragraph.content.push(Inline::Image(image));}
                }else{paragraph.content.push(Inline::Image(image));}
            }else{paragraph.content.push(Inline::Image(image));}
        } else if let Some(boxed) = parse_drawing_text_box(drawing, context, paragraph.id.clone()) {
            paragraph.content.push(Inline::TextBox(boxed));
        }
    }
    for child in &run.children {
        match child.name.as_str() {
            "w:object" => if let Some(object)=parse_embedded_object(child,context){paragraph.content.push(Inline::EmbeddedObject(object));}
                else if let Some(image)=parse_vml_image(child,context){paragraph.content.push(Inline::Image(image));},
            "w:pict" => if let Some(boxed)=parse_vml_text_box(child,context){paragraph.content.push(Inline::TextBox(boxed));}
                else if let Some(image)=parse_vml_image(child,context){paragraph.content.push(Inline::Image(image));},
            "w:t" => if !child.text.is_empty() { paragraph.content.push(Inline::Text(TextRun { id: NodeId::new(), text: child.text.clone(), style: run.child("w:rPr").and_then(|r|r.child("w:rStyle")).and_then(|n|n.attr("w:val")).map(|s|model::StyleId(s.into())), properties: props.clone() })); },
            "w:br" => {
                match child.attr("w:type") {
                    Some("page") => paragraph.content.push(Inline::PaginationBreak { id: NodeId::new(), break_kind: model::PaginationBreakKind::Page }),
                    Some("column") => paragraph.content.push(Inline::PaginationBreak { id: NodeId::new(), break_kind: model::PaginationBreakKind::Column }),
                    _ => paragraph.content.push(Inline::LineBreak { id: NodeId::new() }),
                }
            }
            "w:footnoteReference"|"w:endnoteReference" => {
                let prefix=if child.name=="w:endnoteReference"{"end:"}else{""};
                if let Some(id)=child.attr("w:id").and_then(|id|context.note_ids.get(&format!("{prefix}{id}"))) {
                    paragraph.content.push(Inline::Field(Field{id:NodeId::new(),kind:FieldKind::FootnoteReference{note:id.clone()},display_text:None,color:field_color(run)}));
                }else{paragraph.content.push(Inline::Text(TextRun::plain("[missing footnote]")));}
            }
            "w:tab" => paragraph.content.push(Inline::Tab { id: NodeId::new() }),
            // <w:sym w:font="Symbol" w:char="F0B7"/>: a symbol-font character,
            // stored as the Unicode character it depicts.
            "w:sym" => if let Some(symbol) = child.attr("w:char").and_then(|code| u32::from_str_radix(code, 16).ok()).and_then(char::from_u32) {
                // Codes may be written without the U+F000 private-use offset.
                let symbol = if u32::from(symbol) < 0x100 { char::from_u32(0xF000 | u32::from(symbol)).unwrap_or(symbol) } else { symbol };
                let text = model::symbol_font_fallback(symbol).to_string();
                paragraph.content.push(Inline::Text(TextRun { id: NodeId::new(), text, style: None, properties: props.clone() }));
            },
            _ => {}
        }
    }
}

fn vml_dimensions(node:&XmlNode)->(f32,f32){
    let shape=find_descendant(node,"v:shape");
    let style=shape.and_then(|s|s.attr("style")).unwrap_or("");
    let length=|key:&str|->Option<f32>{
        let value=style.split(';').find_map(|part|part.trim().split_once(':').filter(|(name,_)|name.trim()==key).map(|(_,value)|value.trim()))?;
        if let Some(pt)=value.strip_suffix("pt"){pt.trim().parse::<f32>().ok().map(|v|v*25.4/72.0)}
        else if let Some(px)=value.strip_suffix("px"){px.trim().parse::<f32>().ok().map(|v|v*25.4/96.0)}
        else {None}
    };
    (length("width").unwrap_or(50.0).clamp(1.0,1000.0),length("height").unwrap_or(25.0).clamp(1.0,1000.0))
}

fn parse_vml_text_box(node:&XmlNode,context:&ImportContext)->Option<model::TextBox>{
    let content=find_descendant(node,"w:txbxContent")?;
    let (width_mm,height_mm)=vml_dimensions(node);
    let blocks=content.children.iter().filter_map(|child|match child.name.as_str(){
        "w:p"=>Some(Block::Paragraph(parse_paragraph(child,context))),
        "w:tbl"=>Some(Block::Table(parse_table(child,context))),_=>None,
    }).collect::<Vec<_>>();
    let shape=find_descendant(node,"v:shape");
    Some(model::TextBox{id:NodeId::new(),width_mm,height_mm,blocks:blocks.into(),
        fill:shape.and_then(|s|s.attr("fillcolor")).map(str::to_owned),
        line:shape.and_then(|s|s.attr("strokecolor")).map(str::to_owned),
        anchor:None,wrap:model::ImageWrap::Inline})
}

fn vml_preview(node:&XmlNode,context:&ImportContext)->Option<ResourceId>{
    let image=find_descendant(node,"v:imagedata")?;
    image.attr("r:id").or_else(||image.attr("o:relid"))
        .and_then(|id|context.images.get(id)).cloned()
}

fn parse_vml_image(node:&XmlNode,context:&ImportContext)->Option<InlineImage>{
    let resource=vml_preview(node,context)?;
    let (width_mm,height_mm)=vml_dimensions(node);
    Some(InlineImage{id:NodeId::new(),resource,width_mm,height_mm,alt_text:None,effects:Default::default()})
}

fn parse_embedded_object(node:&XmlNode,context:&ImportContext)->Option<model::EmbeddedObject>{
    let ole=find_descendant(node,"o:OLEObject")?;
    let payload=context.embedded.get(ole.attr("r:id")?)?.clone();
    let (width_mm,height_mm)=vml_dimensions(node);
    Some(model::EmbeddedObject{id:NodeId::new(),payload,preview:vml_preview(node,context),
        width_mm,height_mm,program:ole.attr("ProgID").map(str::to_owned)})
}

fn parse_drawing_text_box(node: &XmlNode, context: &ImportContext, paragraph: NodeId) -> Option<model::TextBox> {
    let content = find_descendant(node, "w:txbxContent")?;
    let extent = find_descendant(node, "wp:extent");
    let width_mm = extent.and_then(|n| n.attr("cx")).and_then(|v| v.parse::<f32>().ok())
        .map(|v| v / EMU_PER_MM).unwrap_or(50.0).max(1.0);
    let height_mm = extent.and_then(|n| n.attr("cy")).and_then(|v| v.parse::<f32>().ok())
        .map(|v| v / EMU_PER_MM).unwrap_or(25.0).max(1.0);
    let blocks = content.children.iter().filter_map(|child| match child.name.as_str() {
        "w:p" => Some(Block::Paragraph(parse_paragraph(child, context))),
        "w:tbl" => Some(Block::Table(parse_table(child, context))),
        _ => None,
    }).collect::<Vec<_>>();
    let fill = find_descendant(node, "a:solidFill").and_then(|n| n.child("a:srgbClr"))
        .and_then(|n| n.attr("val")).map(|v| format!("#{v}"));
    let line = find_descendant(node, "a:ln").and_then(|n| n.child("a:solidFill"))
        .and_then(|n| n.child("a:srgbClr")).and_then(|n| n.attr("val"))
        .map(|v| format!("#{v}"));
    let floating = if context.part == "word/document.xml" && !context.in_table {
        find_descendant(node, "wp:anchor").and_then(|anchor| parse_floating_anchor(anchor, paragraph))
    } else { None };
    let (anchor, wrap) = floating.map(|(anchor, wrap)| (Some(anchor), wrap)).unwrap_or((None, model::ImageWrap::Inline));
    Some(model::TextBox { id: NodeId::new(), width_mm, height_mm, blocks: blocks.into(), fill, line, anchor, wrap })
}

fn parse_drawing_image(node: &XmlNode, context: &ImportContext) -> Option<InlineImage> {
    let blip = find_descendant(node, "a:blip")?;
    let rel = blip.attr("r:embed")?;
    let resource = context.images.get(rel)?.clone();
    let extent = find_descendant(node, "wp:extent");
    let width_mm = extent.and_then(|n| n.attr("cx")).and_then(|v| v.parse::<f32>().ok()).map(|v| v / EMU_PER_MM).unwrap_or(25.4);
    let height_mm = extent.and_then(|n| n.attr("cy")).and_then(|v| v.parse::<f32>().ok()).map(|v| v / EMU_PER_MM).unwrap_or(25.4);
    let alt_text = find_descendant(node, "wp:docPr").and_then(|n| n.attr("descr")).map(str::to_owned);
    let crop = find_descendant(node, "a:srcRect").and_then(|n| {
        let fraction = |key| n.attr(key).unwrap_or("0").parse::<f32>().ok().map(|v| (v / 100_000.0).clamp(0.0, 1.0));
        let (left, top, right, bottom) = (fraction("l")?, fraction("t")?, fraction("r")?, fraction("b")?);
        (left + right < 1.0 && top + bottom < 1.0).then_some(model::ImageCrop { left, top, right, bottom })
    });
    let outer_shadow = find_descendant(node, "a:outerShdw").map(|n| {
        let color_node = n.child("a:srgbClr");
        model::ImageShadow {
            color: format!("#{}", color_node.and_then(|c| c.attr("val")).unwrap_or("000000")),
            opacity: color_node.and_then(|c| c.child("a:alpha")).and_then(|a| a.attr("val"))
                .and_then(|v| v.parse::<f32>().ok()).map(|v| (v / 100_000.0).clamp(0.0, 1.0)).unwrap_or(0.5),
            distance_mm: n.attr("dist").and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0) / EMU_PER_MM,
            direction_degrees: n.attr("dir").and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0) / 60_000.0,
            blur_mm: n.attr("blurRad").and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0) / EMU_PER_MM,
        }
    });
    let glow=find_descendant(node,"a:glow").map(|n|{
        let color=n.child("a:srgbClr");
        model::ImageGlow{color:format!("#{}",color.and_then(|c|c.attr("val")).unwrap_or("000000")),
            opacity:color.and_then(|c|c.child("a:alpha")).and_then(|a|a.attr("val"))
                .and_then(|v|v.parse::<f32>().ok()).map(|v|(v/100_000.0).clamp(0.0,1.0)).unwrap_or(1.0),
            radius_mm:n.attr("rad").and_then(|v|v.parse::<f32>().ok()).unwrap_or(0.0)/EMU_PER_MM}
    });
    let soft_edge_mm=find_descendant(node,"a:softEdge").and_then(|n|n.attr("rad"))
        .and_then(|v|v.parse::<f32>().ok()).map(|v|v/EMU_PER_MM);
    let reflection=find_descendant(node,"a:reflection").map(|n|model::ImageReflection{
        blur_mm:n.attr("blurRad").and_then(|v|v.parse::<f32>().ok()).unwrap_or(0.0)/EMU_PER_MM,
        distance_mm:n.attr("dist").and_then(|v|v.parse::<f32>().ok()).unwrap_or(0.0)/EMU_PER_MM,
        start_opacity:n.attr("stA").and_then(|v|v.parse::<f32>().ok()).map(|v|(v/100_000.0).clamp(0.0,1.0)).unwrap_or(0.5),
        end_opacity:n.attr("endA").and_then(|v|v.parse::<f32>().ok()).map(|v|(v/100_000.0).clamp(0.0,1.0)).unwrap_or(0.0),
    });
    let properties = find_descendant(node, "pic:spPr");
    let shape = properties.and_then(|p| p.child("a:prstGeom")).map(|geometry| model::ImageShape {
        preset: geometry.attr("prst").unwrap_or("rect").into(),
        adjustments: geometry.child("a:avLst").into_iter().flat_map(|list| &list.children)
            .filter(|guide| guide.name == "a:gd")
            .filter_map(|guide| Some((guide.attr("name")?.into(), guide.attr("fmla")?.into()))).collect(),
    }).unwrap_or_default();
    let outline = properties.and_then(|p| p.child("a:ln")).and_then(|line| {
        let color = line.child("a:solidFill")?.child("a:srgbClr")?.attr("val")?;
        if color.len() != 6 || !color.bytes().all(|b| b.is_ascii_hexdigit()) { return None; }
        let width_pt = line.attr("w").and_then(|v| v.parse::<f32>().ok())
            .map(|v| v / crate::EMU_PER_PT).unwrap_or(1.0);
        if !width_pt.is_finite() || width_pt <= 0.0 { return None; }
        Some(model::Border { width_pt, color: format!("#{color}"), style: model::BorderStyle::Single })
    });
    Some(InlineImage { id: NodeId::new(), resource, width_mm, height_mm, alt_text,
        effects: model::ImageEffects { crop, outer_shadow, glow, soft_edge_mm, reflection, shape, outline } })
}

fn field_color(node: &XmlNode) -> Option<String> {
    find_descendant(node, "w:rPr")?.child("w:color")?.attr("w:val")
        .filter(|value| value.len() == 6 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .map(|value| format!("#{value}"))
}

fn field_from_instruction(instruction: &str, display: String, color: Option<String>) -> Field {
    let upper = instruction.trim().to_ascii_uppercase();
    let kind = if upper.split_whitespace().next()==Some("PAGEREF") {
            FieldKind::PageReference{bookmark:instruction.split_whitespace().nth(1).unwrap_or("").trim_matches('"').to_owned()}
        } else if upper.split_whitespace().next()==Some("PAGE") { FieldKind::PageNumber }
        else if upper.split_whitespace().next()==Some("NUMPAGES") { FieldKind::PageCount }
        else if upper.split_whitespace().next()==Some("SECTION") { FieldKind::SectionNumber }
        else if upper.split_whitespace().next()==Some("DATE") { FieldKind::Date { format: None } }
        else { FieldKind::Raw { instruction: instruction.trim().to_owned() } };
    Field { id: NodeId::new(), kind, display_text: if display.is_empty() { None } else { Some(display) }, color }
}

fn parse_table(node: &XmlNode, context: &ImportContext) -> Table {
    let mut local=context.clone();local.in_table=true;let context=&local;
    let mut table = Table { id: NodeId::new(), properties: TableProperties::default(), rows: Vec::new() };
    if let Some(pr) = node.child("w:tblPr") {
        table.properties.fixed_layout = pr.child("w:tblLayout").and_then(|n| n.attr("w:type")).map(|v| v == "fixed");
        table.properties.style = pr.child("w:tblStyle").and_then(|node| node.attr("w:val")).map(|id| model::StyleId(id.into()));
        if let Some(look) = pr.child("w:tblLook") {
            let flags = look.attr("w:val").and_then(|value| u16::from_str_radix(value, 16).ok());
            let flag = |name, bit| look.attr(name).map(|value| matches!(value, "1" | "true" | "on"))
                .or_else(|| flags.map(|value| value & bit != 0));
            table.properties.style_first_row = flag("w:firstRow", 0x20);
            table.properties.style_row_banding = flag("w:noHBand", 0x200).map(|value| !value);
        }
        table.properties.preferred_width_mm = pr.child("w:tblW")
            .filter(|n| matches!(n.attr("w:type"), None | Some("dxa")))
            .and_then(|n| n.attr("w:w")).and_then(parse_twips_mm)
            .filter(|width| width.is_finite() && *width > 0.0);
        table.properties.left_indent_mm = pr.child("w:tblInd")
            .filter(|n| matches!(n.attr("w:type"), None | Some("dxa")))
            .and_then(|n| n.attr("w:w")).and_then(parse_twips_mm);
        table.properties.alignment = match pr.child("w:jc").and_then(|n| n.attr("w:val")) { Some("center") => Some(TableAlignment::Center), Some("right") | Some("end") => Some(TableAlignment::Right), Some(_) => Some(TableAlignment::Left), None => None };
        table.properties.cell_spacing_mm = pr.child("w:tblCellSpacing").and_then(|n| n.attr("w:w")).and_then(parse_twips_mm);
        table.properties.borders = pr.child("w:tblBorders").map(parse_border_set).filter(border_set_has_any);
        table.properties.position = pr.child("w:tblpPr").map(|p| parse_table_position(p, pr.child("w:tblOverlap")));
    }
    let inherited_margins = context.table_styles.margins(table.properties.style.as_ref());
    table.properties.cell_margins = Some(node.child("w:tblPr").and_then(|pr| pr.child("w:tblCellMar"))
        .map(parse_cell_margins).unwrap_or_default().over(inherited_margins));
    if let Some(grid) = node.child("w:tblGrid") {
        table.properties.grid_column_widths_mm = grid.children_named("w:gridCol")
            .filter_map(|column| column.attr("w:w").and_then(parse_twips_mm))
            .filter(|width| width.is_finite() && *width > 0.0)
            .collect();
    }

    // Logical grid column -> origin semantic cell for an active vMerge.
    // Continuation w:tc elements are consumed into the origin cell's row_span
    // rather than becoming duplicate editable cells in Impress.
    let mut active_vmerge: BTreeMap<usize, (usize, usize, usize)> = BTreeMap::new();
    for row_node in controls::children(node,"w:tr") {
        let row_index = table.rows.len();
        // Word can retain duplicate row properties after tracked edits. The
        // last value is the effective one (this file has 0 then true).
        let repeat_as_header = on_off(row_node.child("w:trPr").and_then(|p| p.children_named("w:tblHeader").last())).unwrap_or(false);
        let allow_break_across_pages = !on_off(row_node.child("w:trPr").and_then(|p| p.children_named("w:cantSplit").last())).unwrap_or(false);
        let height_mm = row_node.child("w:trPr").and_then(|p| p.child("w:trHeight")).and_then(|n| n.attr("w:val")).and_then(parse_twips_mm);
        let row_margins = row_node.child("w:tblPrEx").and_then(|pr| pr.child("w:tblCellMar"))
            .map(parse_cell_margins).unwrap_or_default();
        let mut row = TableRow { id: NodeId::new(), cells: Vec::new(), repeat_as_header, allow_break_across_pages, height_mm };
        context.source_members.borrow_mut().insert(row_node as *const XmlNode as usize,row.id.clone());
        let mut grid_column = 0usize;
        for cell_node in controls::children(row_node,"w:tc") {
            let pr = cell_node.child("w:tcPr");
            let column_span = pr.and_then(|p| p.child("w:gridSpan")).and_then(|n| n.attr("w:val")).and_then(|v| v.parse::<u16>().ok()).unwrap_or(1);
            let grid_span = usize::from(column_span.max(1));
            let vmerge = pr.and_then(|p| p.child("w:vMerge"));
            let revised_merge=pr.and_then(|p|p.child("w:cellMerge")).and_then(|n|n.attr("w:vMerge"));
            let restart = revised_merge.map(|value|value=="rest").unwrap_or_else(||vmerge.and_then(|n| n.attr("w:val")).is_some_and(|value| value == "restart"));
            let continuation = revised_merge.map(|value|value=="cont").unwrap_or(vmerge.is_some() && !restart);

            if continuation {
                if let Some((origin_row, origin_cell, origin_span)) = active_vmerge.get(&grid_column).copied() {
                    if origin_span == grid_span {
                        let origin = &mut table.rows[origin_row].cells[origin_cell];
                        origin.row_span = origin.row_span.saturating_add(1);
                        grid_column += grid_span;
                        continue;
                    }
                }
                // Malformed/standalone continuation: preserve its contents as
                // an ordinary cell rather than dropping user data.
            }

            for column in grid_column..grid_column + grid_span { active_vmerge.remove(&column); }
            let width_mm = pr.and_then(|p| p.child("w:tcW")).and_then(|n| n.attr("w:w")).and_then(parse_twips_mm);
            let shading = pr.and_then(|p| p.child("w:shd")).and_then(|n| {
                if n.attr("w:val") == Some("nil") || n.attr("w:fill") == Some("auto") { Some("none".into()) }
                else { n.attr("w:fill").filter(|v| v.len() == 6 && v.bytes().all(|b| b.is_ascii_hexdigit())).map(|v| format!("#{v}")) }
            });
            let vertical_alignment = match pr.and_then(|p| p.child("w:vAlign")).and_then(|n| n.attr("w:val")) { Some("center") => CellVerticalAlignment::Center, Some("bottom") => CellVerticalAlignment::Bottom, _ => CellVerticalAlignment::Top };
            let borders = pr.and_then(|p| p.child("w:tcBorders")).map(parse_border_set).filter(border_set_has_any);
            let margins = pr.and_then(|p| p.child("w:tcMar")).map(parse_cell_margins).unwrap_or_default().over(row_margins);
            let text_direction = match pr.and_then(|p| p.child("w:textDirection")).and_then(|p| p.attr("w:val")) {
                Some("btLr" | "lr") => Some(model::CellTextDirection::BottomToTop),
                Some("tbRl" | "rl") => Some(model::CellTextDirection::TopToBottom),
                _ => None,
            };
            let mut blocks = Vec::new();
            for child in &cell_node.children {
                match child.name.as_str() {
                    "w:p" => {
                let paragraph = parse_paragraph(child, context);
                let mut anchored = context.anchored.borrow_mut();
                let mut index = 0;
                while index < anchored.len() {
                    if anchored[index].anchor.as_ref().is_some_and(|anchor| anchor.paragraph == paragraph.id) {
                        blocks.push(Block::Image(anchored.remove(index)));
                    } else { index += 1; }
                }
                blocks.push(Block::Paragraph(paragraph));
            },
                    "w:tbl" => blocks.push(Block::Table(parse_table(child, context))),
                    _ => {}
                }
            }
            if blocks.is_empty() { blocks.push(Block::Paragraph(Paragraph::plain(""))); }
            let cell_index = row.cells.len();
            row.cells.push(TableCell { id: NodeId::new(), blocks: blocks.into(), column_span, row_span: 1, width_mm, shading, borders, vertical_alignment,
                margins: (margins != TableCellMargins::default()).then_some(margins), text_direction });
            if let Some(cell)=row.cells.last(){context.source_members.borrow_mut().insert(cell_node as *const XmlNode as usize,cell.id.clone());}
            if restart {
                for column in grid_column..grid_column + grid_span {
                    active_vmerge.insert(column, (row_index, cell_index, grid_span));
                }
            }
            grid_column += grid_span;
        }
        table.rows.push(row);
    }
    context.source_members.borrow_mut().insert(node as *const XmlNode as usize,table.id.clone());
    tracked::table(node,&mut table,context);
    table
}

#[derive(Debug, Default)]
struct ImportedTableStyles {
    default_style: Option<String>,
    margins: BTreeMap<String, (Option<String>, TableCellMargins)>,
}

impl ImportedTableStyles {
    fn read(root: &XmlNode) -> Self {
        let mut result = Self::default();
        for style in root.children_named("w:style").filter(|node| node.attr("w:type") == Some("table")) {
            let Some(id) = style.attr("w:styleId") else { continue; };
            if matches!(style.attr("w:default"), Some("1" | "true" | "on")) {
                result.default_style = Some(id.into());
            }
            let parent = style.child("w:basedOn").and_then(|n| n.attr("w:val")).map(str::to_owned);
            let margins = style.child("w:tblPr").and_then(|pr| pr.child("w:tblCellMar")).map(parse_cell_margins).unwrap_or_default();
            result.margins.insert(id.into(), (parent, margins));
        }
        result
    }

    fn margins(&self, style: Option<&model::StyleId>) -> TableCellMargins {
        let mut current = style.map(|id| id.0.as_str()).or(self.default_style.as_deref());
        let mut margins = TableCellMargins::default();
        let mut visited = std::collections::BTreeSet::new();
        while let Some(id) = current.filter(|id| visited.insert(*id)) {
            let Some((parent, inherited)) = self.margins.get(id) else { break; };
            margins = margins.over(*inherited);
            current = parent.as_deref();
        }
        margins.over(TableCellMargins {
            top_mm: Some(0.0), right_mm: Some(115.0 / TWIPS_PER_MM),
            bottom_mm: Some(0.0), left_mm: Some(115.0 / TWIPS_PER_MM),
        })
    }
}

fn parse_cell_margins(node: &XmlNode) -> TableCellMargins {
    let size = |child: Option<&XmlNode>| match child?.attr("w:type") {
        Some("nil") => Some(0.0),
        None | Some("dxa") => child?.attr("w:w").and_then(parse_twips_mm).filter(|mm| mm.is_finite() && *mm >= 0.0),
        _ => None,
    };
    TableCellMargins {
        top_mm: size(node.child("w:top")),
        right_mm: size(node.child("w:end").or_else(|| node.child("w:right"))),
        bottom_mm: size(node.child("w:bottom")),
        left_mm: size(node.child("w:start").or_else(|| node.child("w:left"))),
    }
}

fn parse_border_set(node: &XmlNode) -> BorderSet {
    BorderSet {
        top: node.child("w:top").and_then(parse_border),
        right: node.child("w:right").or_else(|| node.child("w:end")).and_then(parse_border),
        bottom: node.child("w:bottom").and_then(parse_border),
        left: node.child("w:left").or_else(|| node.child("w:start")).and_then(parse_border),
        inside_horizontal: node.child("w:insideH").and_then(parse_border),
        inside_vertical: node.child("w:insideV").and_then(parse_border),
    }
}

fn parse_border(node: &XmlNode) -> Option<Border> {
    let value = node.attr("w:val").unwrap_or("single");
    let style = match value {
        "nil" | "none" => BorderStyle::None,
        "double" => BorderStyle::Double,
        "dotted" | "dotDash" | "dotDotDash" => BorderStyle::Dotted,
        "dashed" | "dashSmallGap" => BorderStyle::Dashed,
        _ => BorderStyle::Single,
    };
    let width_pt = node.attr("w:sz").and_then(|v| v.parse::<f32>().ok()).map(|v| v / 8.0).unwrap_or(0.5);
    let color = node.attr("w:color").filter(|v| *v != "auto").map(|v| format!("#{v}"))
        .unwrap_or_else(|| "#000000".into());
    Some(Border { width_pt, color, style })
}

fn border_set_has_any(borders: &BorderSet) -> bool {
    borders.top.is_some() || borders.right.is_some() || borders.bottom.is_some() || borders.left.is_some()
        || borders.inside_horizontal.is_some() || borders.inside_vertical.is_some()
}

fn apply_section_properties(section: &mut Section, node: &XmlNode, context: &ImportContext) {
    if let Some(pg) = node.child("w:pgSz") {
        if let Some(v) = pg.attr("w:w").and_then(parse_twips_mm) { section.properties.page_width_mm = v; }
        if let Some(v) = pg.attr("w:h").and_then(parse_twips_mm) { section.properties.page_height_mm = v; }
    }
    if let Some(mar) = node.child("w:pgMar") {
        if let Some(v) = mar.attr("w:top").and_then(parse_twips_mm) { section.properties.margin_top_mm = v; }
        if let Some(v) = mar.attr("w:bottom").and_then(parse_twips_mm) { section.properties.margin_bottom_mm = v; }
        if let Some(v) = mar.attr("w:left").and_then(parse_twips_mm) { section.properties.margin_left_mm = v; }
        if let Some(v) = mar.attr("w:right").and_then(parse_twips_mm) { section.properties.margin_right_mm = v; }
        if let Some(v) = mar.attr("w:header").and_then(parse_twips_mm) { section.properties.header_distance_mm = v; }
        if let Some(v) = mar.attr("w:footer").and_then(parse_twips_mm) { section.properties.footer_distance_mm = v; }
    }
    if let Some(cols) = node.child("w:cols") {
        section.properties.column_count = cols.attr("w:num").and_then(|v| v.parse::<u16>().ok()).unwrap_or(1).max(1);
        if let Some(v) = cols.attr("w:space").and_then(parse_twips_mm) { section.properties.column_gap_mm = v; }
        section.properties.column_separator = matches!(cols.attr("w:sep"), Some("1" | "true" | "on"));
        if cols.attr("w:equalWidth").is_some_and(|v| matches!(v, "0" | "false" | "off")) {
            let columns: Vec<model::ColumnSpec> = cols.children_named("w:col").map(|col| model::ColumnSpec {
                width_mm: col.attr("w:w").and_then(parse_twips_mm).unwrap_or(0.0),
                spacing_mm: col.attr("w:space").and_then(parse_twips_mm).unwrap_or(0.0),
            }).collect();
            if columns.len() >= 2 {
                section.properties.column_count = columns.len() as u16;
                section.properties.columns = columns;
            }
        }
    }
    section.properties.different_first_page = on_off(node.child("w:titlePg")).unwrap_or(false);
    if let Some(numbering) = node.child("w:pgNumType") {
        section.properties.page_number_start = numbering.attr("w:start").and_then(|value| value.parse::<u32>().ok()).filter(|value| *value > 0);
        section.properties.page_number_format = numbering.attr("w:fmt").and_then(|value| match value {
            "decimal" => Some(model::PageNumberFormat::Decimal),
            "upperRoman" => Some(model::PageNumberFormat::UpperRoman),
            "lowerRoman" => Some(model::PageNumberFormat::LowerRoman),
            "upperLetter" => Some(model::PageNumberFormat::UpperLetter),
            "lowerLetter" => Some(model::PageNumberFormat::LowerLetter),
            _ => None,
        });
    }
    section.properties.start_on = match node.child("w:type").and_then(|n| n.attr("w:val")) { Some("continuous") => SectionStart::Continuous, Some("oddPage") => SectionStart::OddPage, Some("evenPage") => SectionStart::EvenPage, Some("nextColumn") => SectionStart::NewColumn, _ => SectionStart::NewPage };
    for reference in node.children.iter().filter(|n| n.name == "w:headerReference") {
        if let Some(rel) = reference.attr("r:id").and_then(|id| context.headers.get(id)).cloned() {
            match reference.attr("w:type").unwrap_or("default") { "first" => section.headers.first = Some(rel), "even" => { section.headers.even = Some(rel); section.properties.different_odd_even = true; }, _ => section.headers.default = Some(rel) }
        }
    }
    for reference in node.children.iter().filter(|n| n.name == "w:footerReference") {
        if let Some(rel) = reference.attr("r:id").and_then(|id| context.footers.get(id)).cloned() {
            match reference.attr("w:type").unwrap_or("default") { "first" => section.footers.first = Some(rel), "even" => { section.footers.even = Some(rel); section.properties.different_odd_even = true; }, _ => section.footers.default = Some(rel) }
        }
    }
    tracked::section(node,section,context);
}

struct ImportedNumbering {
    lists: BTreeMap<NodeId, ListDefinition>,
    instances: BTreeMap<i32, NodeId>,
    named: BTreeMap<NodeId, model::NamedListStyle>,
}

fn parse_numbering(xml: &str) -> FormatResult<ImportedNumbering> {
    let tree = parse_supported_tree(xml)?;
    let root = find_descendant(&tree, "w:numbering").ok_or_else(|| invalid("numbering.xml has no w:numbering"))?;
    let mut abstracts = BTreeMap::new();
    for abstract_node in root.children_named("w:abstractNum") {
        let abstract_id = abstract_node.attr("w:abstractNumId").and_then(|v| v.parse::<i32>().ok()).unwrap_or(0);
        let mut levels = Vec::new();
        let mut headings = BTreeMap::new();
        let name = abstract_node.child("w:name").and_then(|n| n.attr("w:val")).filter(|s| !s.trim().is_empty());
        for level_node in abstract_node.children_named("w:lvl") {
            let level = level_node.attr("w:ilvl").and_then(|v| v.parse::<u8>().ok()).unwrap_or(0);
            let format = match level_node.child("w:numFmt").and_then(|n| n.attr("w:val")) { Some("bullet") => ListFormat::Bullet, Some("lowerLetter") => ListFormat::LowerLetter, Some("upperLetter") => ListFormat::UpperLetter, Some("lowerRoman") => ListFormat::LowerRoman, Some("upperRoman") => ListFormat::UpperRoman, _ => ListFormat::Decimal };
            let text = level_node.child("w:lvlText").and_then(|n| n.attr("w:val")).unwrap_or("%1.").to_owned();
            let start_at = level_node.child("w:start").and_then(|n| n.attr("w:val")).and_then(|v| v.parse::<i32>().ok()).unwrap_or(1);
            let ind = level_node.child("w:pPr").and_then(|p| p.child("w:ind"));
            let left_indent_mm = ind.and_then(|n| n.attr("w:left").or_else(|| n.attr("w:start"))).and_then(parse_twips_mm).unwrap_or(6.35);
            let hanging_indent_mm = ind.and_then(|n| n.attr("w:hanging")).and_then(parse_twips_mm).unwrap_or(3.18);
            if level > 8 { continue; }
            if let Some(heading) = level_node.child("w:pStyle").and_then(|n| n.attr("w:val")) {
                headings.insert(level, model::StyleId(heading.to_owned()));
            }
            levels.push(ListLevel { level, format, text, start_at, left_indent_mm, hanging_indent_mm });
        }
        levels.sort_by_key(|level| level.level);
        levels.dedup_by_key(|level| level.level);
        let stable_id = abstract_node.child("w:styleLink").and_then(|n| n.attr("w:val"))
            .and_then(|id| id.strip_prefix("ImpressList")).filter(|id| !id.is_empty());
        abstracts.insert(abstract_id, (levels, name, headings, stable_id));
    }
    let mut lists = BTreeMap::new();
    let mut num_to_list = BTreeMap::new();
    let mut named = BTreeMap::new();
    let mut named_abstracts = std::collections::BTreeSet::new();
    let mut names = std::collections::BTreeSet::new();
    for num_node in root.children_named("w:num") {
        let num_id = num_node.attr("w:numId").and_then(|v| v.parse::<i32>().ok()).unwrap_or(0);
        let abstract_id = num_node.child("w:abstractNumId").and_then(|n| n.attr("w:val")).and_then(|v| v.parse::<i32>().ok()).unwrap_or(0);
        if let Some((levels, name, headings, stable_id)) = abstracts.get(&abstract_id) {
            let is_named = name.is_some() && !levels.is_empty() && named_abstracts.insert(abstract_id);
            let id = stable_id.filter(|_| is_named).map(|id| NodeId(id.to_owned()))
                .filter(|id| !lists.contains_key(id)).unwrap_or_else(NodeId::new);
            let definition = ListDefinition { id: id.clone(), levels: levels.clone() };
            if let Some(name) = name.filter(|_| is_named && names.insert(name.unwrap_or_default().to_lowercase())) {
                named.insert(id.clone(), model::NamedListStyle { name: name.to_owned(), definition: definition.clone(), headings: headings.clone() });
            }
            lists.insert(id.clone(), definition);
            num_to_list.insert(num_id, id);
        }
    }
    Ok(ImportedNumbering { lists, instances: num_to_list, named })
}

/// A floating table's `w:tblpPr` (and `w:tblOverlap`).
fn parse_table_position(p: &XmlNode, overlap: Option<&XmlNode>) -> model::TablePosition {
    use model::{HorizontalFrom, HorizontalPlacement, VerticalFrom, VerticalPlacement};
    let distance = |name: &str| p.attr(name).and_then(parse_twips_mm).filter(|v| v.is_finite()).map(|v| v.clamp(0.0, 100.0)).unwrap_or(0.0);
    let offset = |name: &str| p.attr(name).and_then(parse_twips_mm).filter(|v| v.is_finite()).map(|v| v.clamp(-1000.0, 1000.0)).unwrap_or(0.0);
    let horizontal_from = match p.attr("w:horzAnchor") { Some("page") => HorizontalFrom::Page, Some("margin") => HorizontalFrom::Margin, _ => HorizontalFrom::Column };
    let vertical_from = match p.attr("w:vertAnchor") { Some("page") => VerticalFrom::Page, Some("text") => VerticalFrom::Paragraph, _ => VerticalFrom::Margin };
    let horizontal = match p.attr("w:tblpXSpec") {
        Some("left") => HorizontalPlacement::Left, Some("center") => HorizontalPlacement::Center, Some("right") => HorizontalPlacement::Right,
        Some("inside") => HorizontalPlacement::Inside, Some("outside") => HorizontalPlacement::Outside,
        _ => HorizontalPlacement::Offset(offset("w:tblpX")),
    };
    let vertical = match p.attr("w:tblpYSpec") {
        Some("top") | Some("inside") => VerticalPlacement::Top, Some("center") => VerticalPlacement::Center,
        Some("bottom") | Some("outside") => VerticalPlacement::Bottom,
        _ => VerticalPlacement::Offset(offset("w:tblpY")),
    };
    model::TablePosition {
        horizontal_from, horizontal, vertical_from, vertical,
        distance_left_mm: distance("w:leftFromText"), distance_right_mm: distance("w:rightFromText"),
        distance_top_mm: distance("w:topFromText"), distance_bottom_mm: distance("w:bottomFromText"),
        wrap: match p.attr("impress:wrap") {
            Some("top_and_bottom") => model::TableWrap::TopAndBottom,
            Some("overlap" | "in_front_of_text") => model::TableWrap::InFrontOfText,
            Some("behind_text") => model::TableWrap::BehindText,
            _ => model::TableWrap::Around,
        },
        allow_overlap: overlap.and_then(|o| o.attr("w:val")) != Some("never"),
        visual_offset: p.attr("impress:offset").filter(|json| json.len() <= 160)
            .and_then(|json| serde_json::from_str::<model::TableOffset>(json).ok()).filter(|offset| offset.valid()),
        preserved_text_flow: p.attr("impress:flow").filter(|json|json.len() <= 4096 && matches!(p.attr("impress:wrap"), Some("overlap" | "in_front_of_text" | "behind_text")))
            .and_then(|json|serde_json::from_str::<Box<model::TableTextFlow>>(json).ok())
            .filter(|flow| match flow.as_ref() {
                model::TableTextFlow::Inline => true,
                model::TableTextFlow::Positioned(p) => p.preserved_text_flow.is_none() && p.validate().is_ok(),
            }),
    }
}

fn parse_twips_mm(value: &str) -> Option<f32> { value.parse::<f32>().ok().map(|v| v / TWIPS_PER_MM) }
fn media_type_for_name(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".jpg") || lower.ends_with(".jpeg") { "image/jpeg" }
    else if lower.ends_with(".gif") { "image/gif" }
    else if lower.ends_with(".bmp") { "image/bmp" }
    else if lower.ends_with(".tif") || lower.ends_with(".tiff") { "image/tiff" }
    else { "image/png" }
}

fn encode_base64(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let a = chunk[0] as u32;
        let b = chunk.get(1).copied().unwrap_or(0) as u32;
        let c = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (a << 16) | (b << 8) | c;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 { TABLE[((n >> 6) & 63) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { TABLE[(n & 63) as usize] as char } else { '=' });
    }
    out
}

fn parse_math_container(node: &XmlNode) -> EquationNode {
    let children: Vec<EquationNode> = node.children.iter()
        .filter(|child| !is_math_property(&child.name))
        .map(parse_math_node)
        .collect();
    collapse_row(children)
}

fn parse_math_node(node: &XmlNode) -> EquationNode {
    match node.name.as_str() {
        "m:oMath" | "m:num" | "m:den" | "m:e" | "m:deg" | "m:sub" | "m:sup" | "m:fName" => parse_math_container(node),
        "m:r" => token_node(node.descendant_text().trim()),
        "m:f" => EquationNode::Fraction {
            numerator: Box::new(node.child("m:num").map(parse_math_container).unwrap_or_else(empty_math)),
            denominator: Box::new(node.child("m:den").map(parse_math_container).unwrap_or_else(empty_math)),
        },
        "m:rad" => {
            let radicand = node.child("m:e").map(parse_math_container).unwrap_or_else(empty_math);
            let degree = node.child("m:deg").map(parse_math_container).filter(|n| !is_empty_math(n));
            EquationNode::Radical { radicand: Box::new(radicand), index: degree.map(Box::new) }
        }
        "m:sSup" => EquationNode::Superscript {
            base: Box::new(node.child("m:e").map(parse_math_container).unwrap_or_else(empty_math)),
            exponent: Box::new(node.child("m:sup").map(parse_math_container).unwrap_or_else(empty_math)),
        },
        "m:sSub" => EquationNode::Subscript {
            base: Box::new(node.child("m:e").map(parse_math_container).unwrap_or_else(empty_math)),
            subscript: Box::new(node.child("m:sub").map(parse_math_container).unwrap_or_else(empty_math)),
        },
        "m:sSubSup" => EquationNode::Scripts {
            base: Box::new(node.child("m:e").map(parse_math_container).unwrap_or_else(empty_math)),
            subscript: node.child("m:sub").map(parse_math_container).map(Box::new),
            superscript: node.child("m:sup").map(parse_math_container).map(Box::new),
        },
        "m:m" => {
            let rows = node.children_named("m:mr").map(|row| {
                row.children_named("m:e").map(parse_math_container).collect::<Vec<_>>()
            }).collect();
            EquationNode::Matrix { rows }
        }
        "m:d" => {
            let properties = node.child("m:dPr");
            let left = properties.and_then(|p| p.child("m:begChr")).and_then(|n| n.attr("m:val")).unwrap_or("(").to_owned();
            let right = properties.and_then(|p| p.child("m:endChr")).and_then(|n| n.attr("m:val")).unwrap_or(")").to_owned();
            let body = node.child("m:e").map(parse_math_container).unwrap_or_else(empty_math);
            EquationNode::Delimited { left, body: Box::new(body), right }
        }
        "m:func" => {
            let name = node.child("m:fName").map(XmlNode::descendant_text).unwrap_or_default();
            let argument = node.child("m:e").map(parse_math_container).unwrap_or_else(empty_math);
            EquationNode::Function { name: name.trim().to_owned(), argument: Box::new(argument) }
        }
        "m:acc" => {
            let accent = node.child("m:accPr").and_then(|p| p.child("m:chr")).and_then(|n| n.attr("m:val")).unwrap_or("hat").to_owned();
            let body = node.child("m:e").map(parse_math_container).unwrap_or_else(empty_math);
            EquationNode::Accent { accent, body: Box::new(body) }
        }
        "m:nary" => {
            let operator = node.child("m:naryPr").and_then(|p| p.child("m:chr")).and_then(|n| n.attr("m:val")).unwrap_or("∑").to_owned();
            EquationNode::Nary {
                operator,
                body: Box::new(node.child("m:e").map(parse_math_container).unwrap_or_else(empty_math)),
                subscript: node.child("m:sub").map(parse_math_container).filter(|n| !is_empty_math(n)).map(Box::new),
                superscript: node.child("m:sup").map(parse_math_container).filter(|n| !is_empty_math(n)).map(Box::new),
            }
        }
        "m:limLow" => EquationNode::Limit {
            base: Box::new(node.child("m:e").map(parse_math_container).unwrap_or_else(empty_math)),
            limit: Box::new(node.child("m:lim").map(parse_math_container).unwrap_or_else(empty_math)),
            position: LimitPosition::Lower,
        },
        "m:limUpp" => EquationNode::Limit {
            base: Box::new(node.child("m:e").map(parse_math_container).unwrap_or_else(empty_math)),
            limit: Box::new(node.child("m:lim").map(parse_math_container).unwrap_or_else(empty_math)),
            position: LimitPosition::Upper,
        },
        "m:sPre" => EquationNode::PreScripts {
            base: Box::new(node.child("m:e").map(parse_math_container).unwrap_or_else(empty_math)),
            subscript: node.child("m:sub").map(parse_math_container).filter(|n| !is_empty_math(n)).map(Box::new),
            superscript: node.child("m:sup").map(parse_math_container).filter(|n| !is_empty_math(n)).map(Box::new),
        },
        "m:bar" => {
            let pos = node.child("m:barPr").and_then(|p| p.child("m:pos")).and_then(|n| n.attr("m:val")).unwrap_or("top");
            EquationNode::Bar {
                body: Box::new(node.child("m:e").map(parse_math_container).unwrap_or_else(empty_math)),
                position: if pos == "bot" { VerticalPosition::Under } else { VerticalPosition::Over },
            }
        }
        "m:groupChr" => {
            let props = node.child("m:groupChrPr");
            let character = props.and_then(|p| p.child("m:chr")).and_then(|n| n.attr("m:val")).unwrap_or("⏞").to_owned();
            let pos = props.and_then(|p| p.child("m:pos")).and_then(|n| n.attr("m:val")).unwrap_or("top");
            EquationNode::GroupCharacter {
                body: Box::new(node.child("m:e").map(parse_math_container).unwrap_or_else(empty_math)),
                character,
                position: if pos == "bot" { VerticalPosition::Under } else { VerticalPosition::Over },
            }
        }
        "m:eqArr" => EquationNode::EquationArray {
            rows: node.children_named("m:e").map(parse_math_container).collect(),
        },
        "m:box" => EquationNode::Boxed {
            body: Box::new(node.child("m:e").map(parse_math_container).unwrap_or_else(empty_math)),
        },
        "m:phant" => EquationNode::Phantom {
            body: Box::new(node.child("m:e").map(parse_math_container).unwrap_or_else(empty_math)),
        },
        _ => {
            let children: Vec<_> = node.children.iter().filter(|child| !is_math_property(&child.name)).map(parse_math_node).collect();
            if children.is_empty() { token_node(node.text.trim()) } else { collapse_row(children) }
        }
    }
}

fn token_node(text: &str) -> EquationNode {
    if text.is_empty() { return empty_math(); }
    if text.parse::<f64>().is_ok() { return EquationNode::Number { value: text.to_owned() }; }
    if matches!(text, "=" | "≠" | "<" | ">" | "≤" | "≥" | "≈" | "∈") {
        return EquationNode::Relation { value: text.to_owned() };
    }
    if matches!(text, "+" | "-" | "−" | "*" | "×" | "/" | "÷" | "±" | "∑" | "∫") {
        return EquationNode::Operator { value: text.to_owned() };
    }
    if text.chars().all(|c| c.is_alphanumeric() || c == '.') {
        return EquationNode::Identifier { value: text.to_owned() };
    }
    EquationNode::Symbol { value: text.to_owned() }
}

fn collapse_row(mut nodes: Vec<EquationNode>) -> EquationNode {
    match nodes.len() {
        0 => empty_math(),
        1 => nodes.remove(0),
        _ => EquationNode::Row { children: nodes },
    }
}

fn empty_math() -> EquationNode { EquationNode::Row { children: Vec::new() } }
fn is_empty_math(node: &EquationNode) -> bool { matches!(node, EquationNode::Row { children } if children.is_empty()) }
fn is_math_property(name: &str) -> bool { name.ends_with("Pr") }
fn zip_err(e: zip::result::ZipError) -> FormatError { invalid(e.to_string()) }
fn xml_err<E: std::fmt::Display>(e: E) -> FormatError { invalid(e.to_string()) }
fn invalid(message: impl Into<String>) -> FormatError { FormatError::Invalid { format: "DOCX", message: message.into() } }

fn on_off(node:Option<&XmlNode>)->Option<bool>{node.map(|n|!matches!(n.attr("w:val"),Some("0"|"false"|"off")))}
fn relationship_links(relations:&BTreeMap<String,Relationship>)->BTreeMap<String,String>{
    relations.iter().filter(|(_,r)|r.kind.ends_with("/hyperlink")).map(|(id,r)|(id.clone(),r.target.clone())).collect()
}
fn import_styles(tree:&XmlNode,document:&mut Document,context:&ImportContext) {
    let Some(root)=find_descendant(tree,"w:styles") else{return;};
    let defaults=root.child("w:docDefaults");let mut base=model::StyleSheet::default();
    fn character(rpr:Option<&XmlNode>,context:&ImportContext)->CharacterProperties {
        let run=XmlNode{name:"w:r".into(),children:rpr.into_iter().cloned().chain(std::iter::once(XmlNode{name:"w:t".into(),text:"x".into(),..Default::default()})).collect(),..Default::default()};
        let mut p=Paragraph::plain("");p.content.clear();append_run(&mut p,&run,context);
        if let Some(Inline::Text(t))=p.content.first(){t.properties.clone()}else{CharacterProperties::default()}
    }
    let defaults_character=character(defaults.and_then(|n|n.child("w:rPrDefault")).and_then(|n|n.child("w:rPr")),context);
    let mut defaults_paragraph=Paragraph::plain("");
    // OOXML defaults start with zero paragraph spacing and single spacing.
    // Impress's native Normal style has extra after-space, which otherwise
    // accumulates across every imported table paragraph and changes page flow.
    defaults_paragraph.properties.space_before_pt=Some(0.0);
    defaults_paragraph.properties.space_after_pt=Some(0.0);
    defaults_paragraph.properties.line_spacing=Some(1.0);
    // Hyphenation is on by default, as in new Impress documents; paragraphs
    // the file marks w:suppressAutoHyphens still keep their words whole.
    defaults_paragraph.properties.hyphenation=Some(true);
    defaults_paragraph.properties.glyph_expansion_per_mille=Some(0);
    if let Some(ppr)=defaults.and_then(|n|n.child("w:pPrDefault")).and_then(|n|n.child("w:pPr")){parse_paragraph_properties(&mut defaults_paragraph,ppr,context);}
    if let Some(normal)=base.styles.get_mut(&model::StyleId::from(model::STYLE_NORMAL)){normal.character.overlay(&defaults_character);normal.paragraph.overlay(&defaults_paragraph.properties);}
    let mut style_tab_edits = BTreeMap::new();
    for style in root.children_named("w:style") {
        let Some(id)=style.attr("w:styleId") else{continue;};
        if style.attr("w:type") == Some("table") {
            let fill = |properties: Option<&XmlNode>| properties.and_then(|p| p.child("w:shd"))
                .and_then(|n| n.attr("w:fill")).filter(|value| value.len() == 6 && value.bytes().all(|c| c.is_ascii_hexdigit()))
                .map(|color| format!("#{color}"));
            let region = |name| style.children_named("w:tblStylePr")
                .find(|n| n.attr("w:type") == Some(name)).and_then(|n| n.child("w:tcPr"));
            let table = model::TableStyle {
                name: style.child("w:name").and_then(|n| n.attr("w:val")).unwrap_or(id).to_owned(),
                body_fill: fill(style.child("w:tcPr")).or_else(|| fill(style.child("w:tblPr"))),
                header_fill: fill(region("firstRow")),
                band_fill: fill(region("band1Horz")),
                borders: style.child("w:tblPr").and_then(|p| p.child("w:tblBorders")).map(parse_border_set),
            };
            base.table_styles.insert(model::StyleId(id.to_owned()), table);
            continue;
        }
        let kind=match style.attr("w:type"){Some("character")=>model::StyleKind::Character,Some("paragraph")|None=>model::StyleKind::Paragraph,_=>continue};
        let based_on=style.child("w:basedOn").and_then(|n|n.attr("w:val")).map(|v|model::StyleId(v.into()));
        let next_style=style.child("w:next").and_then(|n|n.attr("w:val")).map(|v|model::StyleId(v.into()));
        let mut p=Paragraph::plain("");p.content.clear();if let Some(ppr)=style.child("w:pPr"){parse_paragraph_properties(&mut p,ppr,context);}
        if let Some(edits) = context.tab_edits.borrow_mut().remove(&p.id) { style_tab_edits.insert(model::StyleId(id.into()), edits); }
        let mut run_properties=character(style.child("w:rPr"),context);
        if kind==model::StyleKind::Paragraph&&based_on.is_none(){let mut c=defaults_character.clone();c.overlay(&run_properties);run_properties=c;let mut pp=defaults_paragraph.properties.clone();pp.overlay(&p.properties);p.properties=pp;}
        let id=model::StyleId(id.into());base.styles.insert(id.clone(),model::Style{id,name:style.child("w:name").and_then(|n|n.attr("w:val")).unwrap_or("Imported style").into(),kind,based_on,next_style:if kind==model::StyleKind::Paragraph{next_style}else{None},character:run_properties,paragraph:p.properties,formatting:Default::default()});
    }
    // The file's own Heading 2, Subtitle… replace Impress's optional built-ins of the same name.
    for builtin in model::OPTIONAL_BUILTIN_STYLES{
        let id=model::StyleId::from(builtin);
        let Some(name)=base.styles.get(&id).map(|s|s.name.to_lowercase()) else{continue;};
        if base.styles.values().any(|s|s.id!=id&&s.name.to_lowercase()==name){base.styles.remove(&id);}
    }
    let mut linked_characters = std::collections::BTreeSet::new();
    for node in root.children_named("w:style") {
        let Some(id) = node.attr("w:styleId").map(|id| model::StyleId(id.to_owned())) else { continue; };
        let Some(link) = node.child("w:link").and_then(|n| n.attr("w:val")).map(|id| model::StyleId(id.to_owned())) else { continue; };
        let pair = match (base.get(&id).map(|s| s.kind), base.get(&link).map(|s| s.kind)) {
            (Some(model::StyleKind::Paragraph), Some(model::StyleKind::Character)) => (id, link),
            (Some(model::StyleKind::Character), Some(model::StyleKind::Paragraph)) => (link, id),
            _ => continue,
        };
        if !base.linked_styles.contains_key(&pair.0) && linked_characters.insert(pair.1.clone()) {
            let inherited = base.resolve_character(Some(&pair.0));
            if let Some(style) = base.styles.get_mut(&pair.1) {
                let c = &mut style.character;
                macro_rules! remove_equal {
                    ($($field:ident),* $(,)?) => { $(if c.$field == inherited.$field { c.$field = None; })* };
                }
                remove_equal!(font_family, font_slots, font_size_pt, bold, italic, underline, color, highlight, language, right_to_left, typography);
            }
            base.linked_styles.insert(pair.0, pair.1);
        }
    }
    let default=root.children_named("w:style").find(|s|s.attr("w:type")==Some("paragraph")&&s.attr("w:default").is_some_and(|v|matches!(v,"1"|"true"|"on")))
        .and_then(|s|s.attr("w:styleId")).map(|v|model::StyleId(v.into())).unwrap_or_else(||model::StyleId::from(model::STYLE_NORMAL));
    fn apply(blocks:&mut[Block],id:&model::StyleId){for b in blocks{match b{Block::Paragraph(p)=>{if p.style.is_none(){p.style=Some(id.clone());}},Block::Table(t)=>for r in &mut t.rows{for c in &mut r.cells{apply(&mut c.blocks,id);}},_=>{}}}}
    for s in &mut document.sections{apply(&mut s.blocks,&default);for hf in [s.headers.default.as_mut(),s.headers.first.as_mut(),s.headers.even.as_mut(),s.footers.default.as_mut(),s.footers.first.as_mut(),s.footers.even.as_mut()].into_iter().flatten(){apply(&mut hf.blocks,&default);}}
    for note in document.review.footnotes.values_mut(){apply(&mut note.blocks,&default);}
    // Resolve OOXML's additive/clear operations into the model's complete sets.
    let original = base.clone();
    for id in style_tab_edits.keys() {
        let tabs = imported_style_tabs(id, &original, &style_tab_edits, &mut std::collections::BTreeSet::new());
        if let Some(style) = base.styles.get_mut(id) { style.paragraph.tabs = Some(tabs); }
    }
    document.styles=base;
}

fn parse_floating_anchor(node:&XmlNode,paragraph:NodeId)->Option<(model::ImageAnchor,model::ImageWrap)>{
    let horizontal=node.child("wp:positionH")?;let vertical=node.child("wp:positionV")?;
    let reference=match(horizontal.attr("relativeFrom"),vertical.attr("relativeFrom")){
        (Some("page"),Some("page"))=>model::AnchorReference::Page,
        (Some("column"|"margin"),Some("paragraph"))=>model::AnchorReference::Paragraph,_=>return None,
    };
    let x=horizontal.child("wp:posOffset")?.text.parse::<f32>().ok()?/EMU_PER_MM;let y=vertical.child("wp:posOffset")?.text.parse::<f32>().ok()?/EMU_PER_MM;
    let gap=["distT","distB","distL","distR"].iter().filter_map(|key|node.attr(key).and_then(|s|s.parse::<f32>().ok())).fold(0.0f32,f32::max)/EMU_PER_MM;
    let wrap=if node.child("wp:wrapSquare").is_some(){model::ImageWrap::Square}else if node.child("wp:wrapTight").is_some(){model::ImageWrap::Tight}else if node.child("wp:wrapTopAndBottom").is_some(){model::ImageWrap::TopAndBottom}else if node.child("wp:wrapNone").is_some(){if matches!(node.attr("behindDoc"),Some("1"|"true")){model::ImageWrap::BehindText}else{model::ImageWrap::InFrontOfText}}else{return None;};
    let anchor=model::ImageAnchor{paragraph,reference,x_mm:x,y_mm:y,gap_mm:gap,locked:matches!(node.attr("locked"),Some("1"|"true"))};anchor.validate().ok()?;Some((anchor,wrap))
}

fn docx_highlight(name:&str)->Option<&'static str>{Some(match name{
"black"=>"#000000","blue"=>"#0000FF","cyan"=>"#00FFFF","green"=>"#00FF00","magenta"=>"#FF00FF","red"=>"#FF0000","yellow"=>"#FFFF00","white"=>"#FFFFFF","darkBlue"=>"#000080","darkCyan"=>"#008080","darkGreen"=>"#008000","darkMagenta"=>"#800080","darkRed"=>"#800000","darkYellow"=>"#808000","darkGray"=>"#808080","lightGray"=>"#C0C0C0","none"=>"none",_=>return None})}

