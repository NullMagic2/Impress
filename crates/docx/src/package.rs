use crate::omml::node_to_omml;
use model::{
    Block, Border, BorderSet, BorderStyle, CharacterProperties, Document, FieldKind, HeaderFooter,
    ImageAlignment, Inline, ListDefinition, ListFormat, NodeId, Paragraph, ParagraphAlignment, ParagraphDirection,
    ResourceId, Section, Table, TableAlignment, TableCell, TextRun,
};
use format::{FormatError, FormatResult, WriteSeek};
use std::{collections::{BTreeMap, BTreeSet}, io::Write};
use quick_xml::{events::Event, Reader};
use zip::{write::SimpleFileOptions, ZipWriter};

#[path = "package_review.rs"]
mod review;
#[path="package_tracked.rs"]
mod tracked;
#[path = "font_package.rs"]
mod fonts;
#[path = "package_theme_fonts.rs"]
mod theme_fonts;
#[path = "package_styles.rs"]
mod styles;
#[path = "package_controls.rs"]
mod controls;

const MM_TO_TWIPS: f32 = 1440.0 / 25.4;
const MM_TO_EMU: f32 = 36_000.0;

#[derive(Clone, Debug)]
struct HeaderFooterPart {
    section_index: usize,
    kind: &'static str,
    header: bool,
    rel_id: String,
    target: String,
    part_name: String,
}

#[derive(Clone, Debug, Default)]
struct ExportContext {
    images: BTreeMap<ResourceId, (String, String)>,
    embedded: BTreeMap<ResourceId, (String, String)>,
    revision_serial: std::cell::Cell<u32>,
    links: BTreeMap<String,String>,
    bookmark_ids: BTreeMap<NodeId,usize>,
    comment_ids: BTreeMap<NodeId,usize>,
    footnote_ids: BTreeMap<NodeId,usize>,
    lists: BTreeMap<NodeId, usize>,
    header_footer_parts: Vec<HeaderFooterPart>,
}

impl ExportContext {
    fn new(document: &Document) -> Self {
        let images = document.resources.iter().filter(|(_,resource)|resource.media_type.starts_with("image/"))
            .enumerate().map(|(index, (id, resource))| {
            let extension = media_extension(&resource.media_type);
            (id.clone(), (format!("rIdImage{}", index + 1), format!("media/image{}.{}", index + 1, extension)))
        }).collect();
        let embedded=document.resources.iter().filter(|(_,resource)|resource.media_type=="application/vnd.openxmlformats-officedocument.oleObject"
            ||resource.media_type=="application/vnd.openxmlformats-officedocument.package")
            .enumerate().map(|(index,(id,_))|(id.clone(),(format!("rIdEmbedded{}",index+1),format!("embeddings/impressObject{}.bin",index+1)))).collect();
        let lists = all_lists(document).keys().enumerate().map(|(index, id)| ((*id).clone(), index + 1)).collect();
        let mut header_footer_parts = Vec::new();
        let mut header_index = 0usize;
        let mut footer_index = 0usize;
        for (section_index, section) in document.sections.iter().enumerate() {
            for (kind, item) in [("default", section.headers.default.as_ref()), ("first", section.headers.first.as_ref()), ("even", section.headers.even.as_ref())] {
                if item.is_some() {
                    header_index += 1;
                    header_footer_parts.push(HeaderFooterPart {
                        section_index, kind, header: true,
                        rel_id: format!("rIdHeader{header_index}"),
                        target: format!("header{header_index}.xml"),
                        part_name: format!("word/header{header_index}.xml"),
                    });
                }
            }
            for (kind, item) in [("default", section.footers.default.as_ref()), ("first", section.footers.first.as_ref()), ("even", section.footers.even.as_ref())] {
                if item.is_some() {
                    footer_index += 1;
                    header_footer_parts.push(HeaderFooterPart {
                        section_index, kind, header: false,
                        rel_id: format!("rIdFooter{footer_index}"),
                        target: format!("footer{footer_index}.xml"),
                        part_name: format!("word/footer{footer_index}.xml"),
                    });
                }
            }
        }
        let links=review::collect_links(document).into_iter().enumerate().map(|(index,target)|(target,format!("rIdImpressLink{}",index+1))).collect();
        let bookmark_ids=document.review.bookmarks.keys().enumerate().map(|(i,id)|(id.clone(),i+1)).collect();
        let comment_ids=document.review.comments.keys().enumerate().map(|(i,id)|(id.clone(),i+1)).collect();
        let mut footnote_ids=BTreeMap::new();
        for (_,id) in model::footnote_references(document){if !footnote_ids.contains_key(&id){let n=footnote_ids.len()+1;footnote_ids.insert(id,n);}}
        for id in document.review.footnotes.keys(){if !footnote_ids.contains_key(id){let n=footnote_ids.len()+1;footnote_ids.insert(id.clone(),n);}}
        Self { images, embedded, lists, header_footer_parts, links, bookmark_ids, comment_ids, footnote_ids, revision_serial: std::cell::Cell::new(1) }
    }

    fn image_rel(&self, id: &ResourceId) -> Option<&str> { self.images.get(id).map(|v| v.0.as_str()) }
    fn list_num_id(&self, id: &NodeId) -> Option<usize> { self.lists.get(id).copied() }

    fn header_footer_rel(&self, section_index: usize, kind: &str, header: bool) -> Option<&str> {
        self.header_footer_parts.iter()
            .find(|p| p.section_index == section_index && p.kind == kind && p.header == header)
            .map(|p| p.rel_id.as_str())
    }
}

pub fn export_docx(document: &Document, output: &mut dyn WriteSeek) -> FormatResult<()> {
    let projected=tracked::project(document);
    let document=projected.as_ref().unwrap_or(document);
    let context = ExportContext::new(document);
    let fonts = fonts::FontPackage::new(document)?;
    let mut zip = ZipWriter::new(output);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    write_member(&mut zip, "[Content_Types].xml", &content_types(document, &context), options)?;
    write_member(&mut zip, "_rels/.rels", &root_relationships(document), options)?;
    write_member(&mut zip, "word/_rels/document.xml.rels", &document_relationships(document, &context), options)?;
    let styles = review::styles_xml(document,&context);
    write_member(&mut zip, "word/styles.xml", &styles, options)?;
    fonts.write(&mut zip,options)?;
    write_member(&mut zip,"word/settings.xml",&review::settings_xml(document),options)?;
    if has_numbering(document) {
        write_member(&mut zip, "word/numbering.xml", &numbering_xml(document), options)?;
    }

    if !document.review.comments.is_empty() {
        write_member(&mut zip,"word/comments.xml",&review::comments_xml(document,&context),options)?;
    }
    let mut drawing_id = 1u32;
    for (stem,kind) in [("footnote",model::NoteKind::Footnote),("endnote",model::NoteKind::Endnote)] {
        if document.review.footnotes.values().any(|n|n.kind==kind) {
            write_member(&mut zip,&format!("word/{stem}s.xml"),&review::notes_xml(document,&context,&mut drawing_id,kind),options)?;
            write_member(&mut zip,&format!("word/_rels/{stem}s.xml.rels"),&header_footer_relationships(&context,document),options)?;
        }
    }
    write_member(&mut zip, "word/document.xml", &document_xml(document, &context, &mut drawing_id), options)?;

    for part in &context.header_footer_parts {
        let section = &document.sections[part.section_index];
        let content = if part.header {
            match part.kind {
                "first" => section.headers.first.as_ref(),
                "even" => section.headers.even.as_ref(),
                _ => section.headers.default.as_ref(),
            }
        } else {
            match part.kind {
                "first" => section.footers.first.as_ref(),
                "even" => section.footers.even.as_ref(),
                _ => section.footers.default.as_ref(),
            }
        };
        if let Some(content) = content {
            write_member(&mut zip, &part.part_name, &header_footer_xml(content, part.header, document, &context, &mut drawing_id), options)?;
            if !context.images.is_empty() || !context.embedded.is_empty() || !context.links.is_empty() {
                let rel_name = relationship_part_name(&part.part_name);
                write_member(&mut zip, &rel_name, &header_footer_relationships(&context,document), options)?;
            }
        }
    }

    for (id, resource) in &document.resources {
        if resource.data.is_empty() { continue; }
        let Some((_, target)) = context.images.get(id).or_else(||context.embedded.get(id)) else { continue; };
        zip.start_file(format!("word/{target}"), options).map_err(zip_err)?;
        zip.write_all(&resource.data)?;
    }

    let mut custom_xml_names=BTreeSet::new();
    for store in document.custom_xml.values() {
        if let Some(name)=&store.original_name {write_member(&mut zip,name,&store.xml,options)?;custom_xml_names.insert(name.clone());}
    }

    // Preserve unknown parts that do not collide with generated semantic parts.
    for (name, part) in &document.preserved_parts {
        if generated_part_name(name) || custom_xml_names.contains(name) { continue; }
        if let Ok(data) = decode_base64(&part.data_base64) {
            zip.start_file(name, options).map_err(zip_err)?;
            zip.write_all(&data)?;
        }
    }

    zip.finish().map_err(zip_err)?;
    Ok(())
}

fn document_xml(document: &Document, context: &ExportContext, drawing_id: &mut u32) -> String {
    let mut body = String::new();
    for (section_index, section) in document.sections.iter().enumerate() {
        let mut managed=false;
        for block in &section.blocks {
            let contents=matches!(block,Block::Paragraph(p) if document.review.contents_paragraphs.contains(&p.id));
            if contents && !managed {
                let tag = crate::contents_tag(&document.review);
                body.push_str(&format!("<w:sdt><w:sdtPr><w:tag w:val=\"{tag}\"/></w:sdtPr><w:sdtContent>"));
            }
            if !contents&&managed{body.push_str("</w:sdtContent></w:sdt>");}
            managed=contents;body.push_str(&block_xml(block, document, context, drawing_id));
        }
        if managed{body.push_str("</w:sdtContent></w:sdt>");}
        if section_index + 1 < document.sections.len() {
            body.push_str(&format!("<w:p><w:pPr>{}</w:pPr></w:p>", tracked::section_properties(section, section_index, document, context)));
        }
    }
    if let Some((index, last)) = document.sections.iter().enumerate().last() {
        body.push_str(&tracked::section_properties(last, index, document, context));
    }
    format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" mc:Ignorable="w14" xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture" xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape" xmlns:v="urn:schemas-microsoft-com:vml" xmlns:o="urn:schemas-microsoft-com:office:office"><w:body>{body}</w:body></w:document>"#)
}

fn header_footer_xml(
    content: &HeaderFooter,
    header: bool,
    document: &Document,
    context: &ExportContext,
    drawing_id: &mut u32,
) -> String {
    let mut body = String::new();
    for block in &content.blocks { body.push_str(&block_xml(block, document, context, drawing_id)); }
    if body.is_empty() { body.push_str("<w:p/>"); }
    let tag = if header { "w:hdr" } else { "w:ftr" };
    format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><{tag} xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" mc:Ignorable="w14" xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture" xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape" xmlns:v="urn:schemas-microsoft-com:vml" xmlns:o="urn:schemas-microsoft-com:office:office">{body}</{tag}>"#)
}

fn block_xml(block: &Block, document: &Document, context: &ExportContext, drawing_id: &mut u32) -> String {
    let xml = match block {
        Block::Paragraph(p) => paragraph_xml(p, document, context, drawing_id),
        Block::Equation(eq) => format!("<m:oMathPara><m:oMath>{}</m:oMath></m:oMathPara>", node_to_omml(&eq.root)),
        Block::Table(table) => tracked::table(table, document, context, drawing_id),
        Block::Image(image) if image.wrap!=model::ImageWrap::Inline&&image.anchor.as_ref().is_some_and(|a|document.sections.iter().flat_map(|s|&s.blocks).any(|b|matches!(b,Block::Paragraph(p)if p.id==a.paragraph))) => String::new(),
        Block::Image(image) => {
            let jc = match image.alignment { ImageAlignment::Left => "left", ImageAlignment::Center => "center", ImageAlignment::Right => "right" };
            let drawing = drawing_xml(&image.resource, image.width_mm, image.height_mm, image.alt_text.as_deref(), &image.effects, document, context, drawing_id);
            format!("<w:p><w:pPr><w:jc w:val=\"{jc}\"/></w:pPr><w:r>{drawing}</w:r></w:p>")
        }
        Block::PageBreak { .. } => "<w:p><w:r><w:br w:type=\"page\"/></w:r></w:p>".into(),
        Block::ColumnBreak { .. } => "<w:p><w:r><w:br w:type=\"column\"/></w:r></w:p>".into(),
        Block::Foreign(_) => String::new(),
    };
    let id=match block {Block::Paragraph(p)=>&p.id,Block::Table(t)=>&t.id,Block::Image(i)=>&i.id,Block::Equation(e)=>&e.id,Block::PageBreak{id}|Block::ColumnBreak{id}=>id,Block::Foreign(f)=>&f.id};
    controls::wrap_member(id,model::ContentControlScope::Blocks,xml,document)
}

fn paragraph_xml(p:&Paragraph,document:&Document,context:&ExportContext,drawing_id:&mut u32)->String {
    review::paragraph_with_annotations(p,document,context,drawing_id)
}

fn paragraph_properties_xml(p: &Paragraph, context: &ExportContext, inherited_tabs: &[model::TabStop]) -> String {
    let props = &p.properties;
    let mut out = String::new();
    if let Some(style) = &p.style { out.push_str(&format!("<w:pStyle w:val=\"{}\"/>", xml_escape(&style.0))); }
    if let Some(alignment) = props.alignment {
        let val = match alignment { ParagraphAlignment::Left => "left", ParagraphAlignment::Center => "center", ParagraphAlignment::Right => "right", ParagraphAlignment::Justify => "both" };
        out.push_str(&format!("<w:jc w:val=\"{val}\"/>"));
    }
    if let Some(direction)=props.direction {out.push_str(if direction==ParagraphDirection::RightToLeft{"<w:bidi/>"}else{"<w:bidi w:val=\"0\"/>"});}
    if let Some(list_id) = &props.list_id {
        if let Some(num_id) = context.list_num_id(list_id) {
            let level = props.list_level.unwrap_or(0);
            out.push_str(&format!("<w:numPr><w:ilvl w:val=\"{level}\"/><w:numId w:val=\"{num_id}\"/></w:numPr>"));
        }
    }
    if let Some(value)=props.keep_with_next {out.push_str(if value{"<w:keepNext/>"}else{"<w:keepNext w:val=\"0\"/>"});}
    if let Some(value)=props.keep_lines_together {out.push_str(if value{"<w:keepLines/>"}else{"<w:keepLines w:val=\"0\"/>"});}
    if let Some(value)=props.page_break_before {out.push_str(if value{"<w:pageBreakBefore/>"}else{"<w:pageBreakBefore w:val=\"0\"/>"});}
    if let Some(enabled)=props.hyphenation {out.push_str(if enabled{"<w:suppressAutoHyphens w:val=\"0\"/>"}else{"<w:suppressAutoHyphens/>"});}
    if let Some(value)=props.widow_control {out.push_str(if value{"<w:widowControl/>"}else{"<w:widowControl w:val=\"0\"/>"});}
    if let Some(shading) = &props.shading { out.push_str(&format!("<w:shd w:val=\"clear\" w:fill=\"{}\"/>", xml_escape(shading.trim_start_matches('#')))); }
    if let Some(borders) = &props.borders { out.push_str(&borders_xml("w:pBdr", borders)); }
    if props.left_indent_mm.is_some() || props.right_indent_mm.is_some() || props.first_line_indent_mm.is_some() {
        out.push_str("<w:ind");
        if let Some(v) = props.left_indent_mm { out.push_str(&format!(" w:left=\"{}\"", twips(v))); }
        if let Some(v) = props.right_indent_mm { out.push_str(&format!(" w:right=\"{}\"", twips(v))); }
        if let Some(v) = props.first_line_indent_mm {
            if v >= 0.0 { out.push_str(&format!(" w:firstLine=\"{}\"", twips(v))); }
            else { out.push_str(&format!(" w:hanging=\"{}\"", twips(-v))); }
        }
        out.push_str("/>");
    }
    if props.space_before_pt.is_some() || props.space_after_pt.is_some() || props.line_spacing.is_some() {
        out.push_str("<w:spacing");
        if let Some(v) = props.space_before_pt { out.push_str(&format!(" w:before=\"{}\"", (v * 20.0).round() as i32)); }
        if let Some(v) = props.space_after_pt { out.push_str(&format!(" w:after=\"{}\"", (v * 20.0).round() as i32)); }
        if let Some(v) = props.line_spacing { out.push_str(&format!(" w:line=\"{}\" w:lineRule=\"auto\"", (v * 240.0).round() as i32)); }
        out.push_str("/>");
    }
    if let Some(tabs) = &props.tabs {
        out.push_str("<w:tabs>");
        // OOXML merges tabs with the style. Explicit clears preserve Impress's
        // replacement semantics, including Clear All on an inherited set.
        for inherited in inherited_tabs {
            if !tabs.iter().any(|tab| twips(tab.position_mm) == twips(inherited.position_mm)) {
                out.push_str(&format!("<w:tab w:val=\"clear\" w:pos=\"{}\"/>", twips(inherited.position_mm)));
            }
        }
        for tab in tabs {
            let val = match tab.alignment { model::TabAlignment::Left => "left", model::TabAlignment::Center => "center", model::TabAlignment::Right => "right", model::TabAlignment::Decimal => "decimal", model::TabAlignment::Bar => "bar" };
            let leader = match tab.leader { model::TabLeader::None => "none", model::TabLeader::Dots => "dot", model::TabLeader::Dashes => "hyphen", model::TabLeader::Line => "underscore", model::TabLeader::Heavy => "heavy", model::TabLeader::MiddleDots => "middleDot" };
            out.push_str(&format!("<w:tab w:val=\"{val}\" w:leader=\"{leader}\" w:pos=\"{}\"/>", twips(tab.position_mm)));
        }
        out.push_str("</w:tabs>");
    }
    if let Some(level) = props.outline_level {
        let word_level = if (1..=9).contains(&level) { level - 1 } else { 9 };
        out.push_str(&format!("<w:outlineLvl w:val=\"{word_level}\"/>"));
    }
    // Empty runs represent the paragraph mark's typing format. Store it in
    // pPr/rPr as well, so Word and a subsequent import retain the blank's size.
    if p.content.iter().all(|i| model::inline_flat_len(i) == 0) {
        if let Some(Inline::Text(run)) = p.content.first() { out.push_str(&text_run_props_xml(run)); }
    }
    if out.is_empty() { String::new() } else { format!("<w:pPr>{out}</w:pPr>") }
}

fn table_xml(table: &Table, document: &Document, context: &ExportContext, drawing_id: &mut u32) -> String {
    let placements = table_grid_placements(table);
    let mut continuations: Vec<Vec<(usize, usize, &TableCell)>> = vec![Vec::new(); table.rows.len()];
    for (row_index, row) in table.rows.iter().enumerate() {
        for (cell_index, cell) in row.cells.iter().enumerate() {
            let (start, span, row_span) = placements[row_index][cell_index];
            for continuation_row in row_index + 1..(row_index + row_span).min(table.rows.len()) {
                continuations[continuation_row].push((start, span, cell));
            }
        }
    }

    let mut rows = String::new();
    for (row_index, row) in table.rows.iter().enumerate() {
        let mut entries: Vec<(usize, bool, usize, &TableCell)> = Vec::new();
        for (cell_index, cell) in row.cells.iter().enumerate() {
            let (start, span, _) = placements[row_index][cell_index];
            entries.push((start, false, span, cell));
        }
        for (start, span, cell) in &continuations[row_index] {
            entries.push((*start, true, *span, *cell));
        }
        entries.sort_by_key(|entry| entry.0);

        let mut cells = String::new();
        for (_, continuation, span, cell) in entries {
            let mut tcpr = String::new();
            if let Some(width) = cell.width_mm { tcpr.push_str(&format!("<w:tcW w:w=\"{}\" w:type=\"dxa\"/>", twips(width))); }
            if span > 1 { tcpr.push_str(&format!("<w:gridSpan w:val=\"{}\"/>", span)); }
            if continuation {
                tcpr.push_str("<w:vMerge/>");
                cells.push_str(&format!("<w:tc><w:tcPr>{tcpr}</w:tcPr><w:p/></w:tc>"));
                continue;
            }
            if cell.row_span > 1 { tcpr.push_str("<w:vMerge w:val=\"restart\"/>"); }
            if let Some(fill) = &cell.shading { tcpr.push_str(&format!("<w:shd w:val=\"clear\" w:fill=\"{}\"/>", xml_escape(fill.trim_start_matches('#')))); }
            if let Some(borders) = &cell.borders { tcpr.push_str(&borders_xml("w:tcBorders", borders)); }
            let valign = match cell.vertical_alignment { model::CellVerticalAlignment::Top => "top", model::CellVerticalAlignment::Center => "center", model::CellVerticalAlignment::Bottom => "bottom" };
            tcpr.push_str(&format!("<w:vAlign w:val=\"{valign}\"/>"));
            let mut body = String::new();
            for block in &cell.blocks { body.push_str(&block_xml(block, document, context, drawing_id)); }
            if body.is_empty() { body.push_str("<w:p/>"); }
            cells.push_str(&controls::wrap_member(&cell.id,model::ContentControlScope::Cell,format!("<w:tc><w:tcPr>{tcpr}</w:tcPr>{body}</w:tc>"),document));
        }
        let mut trpr = String::new();
        if row.repeat_as_header { trpr.push_str("<w:tblHeader/>"); }
        if !row.allow_break_across_pages { trpr.push_str("<w:cantSplit/>"); }
        if let Some(height) = row.height_mm { trpr.push_str(&format!("<w:trHeight w:val=\"{}\"/>", twips(height))); }
        rows.push_str(&controls::wrap_member(&row.id,model::ContentControlScope::Row,format!("<w:tr><w:trPr>{trpr}</w:trPr>{cells}</w:tr>"),document));
    }
    let width = table.properties.preferred_width_mm.map(|v| format!("<w:tblW w:w=\"{}\" w:type=\"dxa\"/>", twips(v))).unwrap_or_else(|| "<w:tblW w:w=\"0\" w:type=\"auto\"/>".into());
    let indent = table.properties.left_indent_mm.map(|v| format!("<w:tblInd w:w=\"{}\" w:type=\"dxa\"/>", twips(v))).unwrap_or_default();
    let align = match table.properties.alignment.unwrap_or(TableAlignment::Left) { TableAlignment::Left => "left", TableAlignment::Center => "center", TableAlignment::Right => "right" };
    let borders = if table.properties.style_overrides_table_borders == Some(true)
        && document.styles.table_style(table).is_some_and(|style| style.borders.is_some()) { String::new() }
        else { table.properties.borders.as_ref().map(|b| borders_xml("w:tblBorders", b)).unwrap_or_default() };
    let spacing = table.properties.cell_spacing_mm.map(|v| format!("<w:tblCellSpacing w:w=\"{}\" w:type=\"dxa\"/>", twips(v))).unwrap_or_default();
    let column_count = placements.iter().flat_map(|row| row.iter())
        .map(|(start, span, _)| start + span).max().unwrap_or(1);
    let grid_widths = docx_table_grid_widths(table, &placements, column_count);
    let grid = format!("<w:tblGrid>{}</w:tblGrid>", grid_widths.iter()
        .map(|width_mm| format!("<w:gridCol w:w=\"{}\"/>", twips(*width_mm)))
        .collect::<String>());
    let position = table.properties.position.as_ref().map(table_position_xml).unwrap_or_default();
    let style_tag = table.properties.style.as_ref()
        .map(|id| format!(r#"<w:tblStyle w:val="{}"/>"#, xml_escape(&id.0))).unwrap_or_default();
    let look = if style_tag.is_empty() { String::new() } else {
        let first = table.properties.style_first_row.unwrap_or(true);
        let no_bands = !table.properties.style_row_banding.unwrap_or(true);
        let flags = 0x400 | if first { 0x20 } else { 0 } | if no_bands { 0x200 } else { 0 };
        format!(r#"<w:tblLook w:val="{flags:04X}" w:firstRow="{}" w:noHBand="{}" w:noVBand="1"/>"#, u8::from(first), u8::from(no_bands))
    };
    format!("<w:tbl><w:tblPr>{style_tag}{position}{width}{indent}<w:jc w:val=\"{align}\"/>{spacing}{borders}{look}</w:tblPr>{grid}{rows}</w:tbl>")
}

fn docx_table_grid_widths(
    table: &Table,
    placements: &[Vec<(usize, usize, usize)>],
    column_count: usize,
) -> Vec<f32> {
    if table.properties.grid_column_widths_mm.len() == column_count
        && table.properties.grid_column_widths_mm.iter().all(|value| value.is_finite() && *value > 0.0)
    {
        return table.properties.grid_column_widths_mm.clone();
    }
    let fallback = table.properties.preferred_width_mm
        .filter(|value| value.is_finite() && *value > 0.0)
        .map(|width| width / column_count.max(1) as f32)
        .unwrap_or(25.0);
    let mut widths = vec![None::<f32>; column_count.max(1)];
    for (row_index, row) in table.rows.iter().enumerate() {
        for (cell_index, cell) in row.cells.iter().enumerate() {
            let Some(width_mm) = cell.width_mm.filter(|value| value.is_finite() && *value > 0.0) else { continue; };
            let (start, span, _) = placements[row_index][cell_index];
            let per_column = width_mm / span.max(1) as f32;
            for column in start..(start + span).min(widths.len()) {
                if widths[column].is_none() { widths[column] = Some(per_column); }
            }
        }
    }
    widths.into_iter().map(|width| width.unwrap_or(fallback)).collect()
}

fn table_grid_placements(table: &Table) -> Vec<Vec<(usize, usize, usize)>> {
    let mut result = Vec::with_capacity(table.rows.len());
    let mut span_until: Vec<usize> = Vec::new();
    for (row_index, row) in table.rows.iter().enumerate() {
        let mut placements = Vec::with_capacity(row.cells.len());
        let mut cursor = 0usize;
        for cell in &row.cells {
            let span = usize::from(cell.column_span.max(1));
            loop {
                if span_until.len() < cursor + span { span_until.resize(cursor + span, 0); }
                if (cursor..cursor + span).all(|column| span_until[column] <= row_index) { break; }
                cursor += 1;
            }
            let row_span = usize::from(cell.row_span.max(1)).min(table.rows.len().saturating_sub(row_index).max(1));
            for column in cursor..cursor + span { span_until[column] = span_until[column].max(row_index + row_span); }
            placements.push((cursor, span, row_span));
            cursor += span;
        }
        result.push(placements);
    }
    result
}

fn borders_xml(tag: &str, borders: &BorderSet) -> String {
    let mut body = String::new();
    for (name, border) in [
        ("top", borders.top.as_ref()), ("left", borders.left.as_ref()), ("bottom", borders.bottom.as_ref()),
        ("right", borders.right.as_ref()), ("insideH", borders.inside_horizontal.as_ref()), ("insideV", borders.inside_vertical.as_ref()),
    ] {
        if let Some(border) = border { body.push_str(&border_xml(name, border)); }
    }
    if body.is_empty() { String::new() } else { format!("<{tag}>{body}</{tag}>") }
}

fn border_xml(name: &str, border: &Border) -> String {
    let val = match border.style { BorderStyle::Single => "single", BorderStyle::Double => "double", BorderStyle::Dotted => "dotted", BorderStyle::Dashed => "dashed", BorderStyle::None => "nil" };
    let size = (border.width_pt * 8.0).round().clamp(2.0, 96.0) as i32;
    format!("<w:{name} w:val=\"{val}\" w:sz=\"{size}\" w:color=\"{}\"/>", xml_escape(border.color.trim_start_matches('#')))
}

fn drawing_xml(
    resource: &ResourceId,
    width_mm: f32,
    height_mm: f32,
    alt_text: Option<&str>,
    effects: &model::ImageEffects,
    document: &Document,
    context: &ExportContext,
    drawing_id: &mut u32,
) -> String {
    let Some(rel_id) = context.image_rel(resource) else {
        return format!("<w:t xml:space=\"preserve\">[Missing image {}]</w:t>", xml_escape(&resource.0));
    };
    let name = document.resources.get(resource).and_then(|r| r.original_name.as_deref()).unwrap_or("Picture");
    let description = alt_text.unwrap_or("");
    let cx = (width_mm.max(0.1) * MM_TO_EMU).round() as i64;
    let cy = (height_mm.max(0.1) * MM_TO_EMU).round() as i64;
    let id = *drawing_id;
    *drawing_id = drawing_id.saturating_add(1);
    let crop = effects.crop.as_ref().map(|c| format!("<a:srcRect l=\"{}\" t=\"{}\" r=\"{}\" b=\"{}\"/>",
        (c.left * 100_000.0).round() as i32, (c.top * 100_000.0).round() as i32,
        (c.right * 100_000.0).round() as i32, (c.bottom * 100_000.0).round() as i32)).unwrap_or_default();
    let mut effect_list=String::new();
    if let Some(s)=&effects.outer_shadow {effect_list.push_str(&format!(
        "<a:outerShdw blurRad=\"{}\" dist=\"{}\" dir=\"{}\"><a:srgbClr val=\"{}\"><a:alpha val=\"{}\"/></a:srgbClr></a:outerShdw>",
        (s.blur_mm * MM_TO_EMU).round() as i64, (s.distance_mm * MM_TO_EMU).round() as i64,
        (s.direction_degrees * 60_000.0).round() as i64, xml_escape(s.color.trim_start_matches('#')),
        (s.opacity.clamp(0.0, 1.0) * 100_000.0).round() as i32));}
    if let Some(g)=&effects.glow {effect_list.push_str(&format!("<a:glow rad=\"{}\"><a:srgbClr val=\"{}\"><a:alpha val=\"{}\"/></a:srgbClr></a:glow>",
        (g.radius_mm*MM_TO_EMU).round()as i64,xml_escape(g.color.trim_start_matches('#')),(g.opacity.clamp(0.0,1.0)*100_000.0).round()as i32));}
    if let Some(r)=&effects.reflection {effect_list.push_str(&format!("<a:reflection blurRad=\"{}\" dist=\"{}\" dir=\"5400000\" stA=\"{}\" endA=\"{}\"/>",
        (r.blur_mm*MM_TO_EMU).round()as i64,(r.distance_mm*MM_TO_EMU).round()as i64,
        (r.start_opacity.clamp(0.0,1.0)*100_000.0).round()as i32,(r.end_opacity.clamp(0.0,1.0)*100_000.0).round()as i32));}
    if let Some(radius)=effects.soft_edge_mm {effect_list.push_str(&format!("<a:softEdge rad=\"{}\"/>",(radius.max(0.0)*MM_TO_EMU).round()as i64));}
    let effects_xml=if effect_list.is_empty(){String::new()}else{format!("<a:effectLst>{effect_list}</a:effectLst>")};
    let shape = xml_escape(&effects.shape.preset);
    let adjustments = effects.shape.adjustments.iter().map(|(name, formula)| {
        format!("<a:gd name=\"{}\" fmla=\"{}\"/>", xml_escape(name), xml_escape(formula))
    }).collect::<String>();
    let outline = effects.outline.as_ref().filter(|line| line.style != BorderStyle::None).map(|line| {
        format!("<a:ln w=\"{}\"><a:solidFill><a:srgbClr val=\"{}\"/></a:solidFill></a:ln>",
            (line.width_pt.max(0.0) * crate::EMU_PER_PT).round() as i64, xml_escape(line.color.trim_start_matches('#')))
    }).unwrap_or_default();
    format!(r#"<w:drawing><wp:inline distT="0" distB="0" distL="0" distR="0"><wp:extent cx="{cx}" cy="{cy}"/><wp:docPr id="{id}" name="{}" descr="{}"/><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic><pic:nvPicPr><pic:cNvPr id="0" name="{}" descr="{}"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="{rel_id}"/>{crop}<a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="{shape}"><a:avLst>{adjustments}</a:avLst></a:prstGeom>{outline}{effects_xml}</pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing>"#,
        xml_escape(name), xml_escape(description), xml_escape(name), xml_escape(description))
}

fn text_box_xml(boxed: &model::TextBox, document: &Document, context: &ExportContext, drawing_id: &mut u32) -> String {
    let cx = (boxed.width_mm.max(1.0) * MM_TO_EMU).round() as i64;
    let cy = (boxed.height_mm.max(1.0) * MM_TO_EMU).round() as i64;
    let id = *drawing_id;
    *drawing_id = drawing_id.saturating_add(1);
    let fill = boxed.fill.as_deref().map(|c| format!("<a:solidFill><a:srgbClr val=\"{}\"/></a:solidFill>", xml_escape(c.trim_start_matches('#'))))
        .unwrap_or_else(|| "<a:noFill/>".into());
    let line = boxed.line.as_deref().map(|c| format!("<a:ln><a:solidFill><a:srgbClr val=\"{}\"/></a:solidFill></a:ln>", xml_escape(c.trim_start_matches('#'))))
        .unwrap_or_else(|| "<a:ln><a:noFill/></a:ln>".into());
    let mut content = boxed.blocks.iter().map(|b| block_xml(b, document, context, drawing_id)).collect::<String>();
    if content.is_empty() { content.push_str("<w:p/>"); }
    let inline=format!("<w:drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\"><wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:docPr id=\"{id}\" name=\"Text Box {id}\"/><a:graphic><a:graphicData uri=\"http://schemas.microsoft.com/office/word/2010/wordprocessingShape\"><wps:wsp><wps:cNvSpPr txBox=\"1\"/><wps:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom>{fill}{line}</wps:spPr><wps:txbx><w:txbxContent>{content}</w:txbxContent></wps:txbx><wps:bodyPr wrap=\"square\" lIns=\"45720\" tIns=\"45720\" rIns=\"45720\" bIns=\"45720\"/></wps:wsp></a:graphicData></a:graphic></wp:inline></w:drawing>");
    let Some(anchor)=&boxed.anchor else{return inline;};
    if boxed.wrap==model::ImageWrap::Inline{return inline;}
    let Some(start)=inline.find("<wp:extent")else{return inline;};
    let Some(end)=inline.rfind("</wp:inline>")else{return inline;};
    let mut body=inline[start..end].to_owned();
    let Some(extent_end)=body.find("/>").map(|n|n+2)else{return inline;};
    let wrap=wrap_xml(boxed.wrap);
    body.insert_str(extent_end,wrap);
    let page=anchor.reference==model::AnchorReference::Page;
    let h=if page{"page"}else{"column"};let v=if page{"page"}else{"paragraph"};
    let x=(anchor.x_mm*MM_TO_EMU).round()as i64;let y=(anchor.y_mm*MM_TO_EMU).round()as i64;
    let gap=(anchor.gap_mm*MM_TO_EMU).round()as i64;
    let behind=usize::from(boxed.wrap==model::ImageWrap::BehindText);let locked=usize::from(anchor.locked);
    format!("<w:drawing><wp:anchor distT=\"{gap}\" distB=\"{gap}\" distL=\"{gap}\" distR=\"{gap}\" simplePos=\"0\" relativeHeight=\"1\" behindDoc=\"{behind}\" locked=\"{locked}\" layoutInCell=\"0\" allowOverlap=\"1\"><wp:simplePos x=\"0\" y=\"0\"/><wp:positionH relativeFrom=\"{h}\"><wp:posOffset>{x}</wp:posOffset></wp:positionH><wp:positionV relativeFrom=\"{v}\"><wp:posOffset>{y}</wp:posOffset></wp:positionV>{body}</wp:anchor></w:drawing>")
}

fn embedded_object_xml(object:&model::EmbeddedObject,context:&ExportContext,drawing_id:&mut u32)->String{
    let Some((payload_rel,_))=context.embedded.get(&object.payload)else{return String::new();};
    let id=*drawing_id;*drawing_id=drawing_id.saturating_add(1);
    let shape=format!("_x0000_i{id}");
    let preview=object.preview.as_ref().and_then(|id|context.image_rel(id))
        .map(|rel|format!("<v:imagedata r:id=\"{}\"/>",xml_escape(rel))).unwrap_or_default();
    let program=xml_escape(object.program.as_deref().unwrap_or("Package"));
    let width=object.width_mm.max(1.0)*72.0/25.4;
    let height=object.height_mm.max(1.0)*72.0/25.4;
    format!("<w:object><v:shape id=\"{shape}\" style=\"width:{width:.2}pt;height:{height:.2}pt\">{preview}</v:shape><o:OLEObject Type=\"Embed\" ProgID=\"{program}\" ShapeID=\"{shape}\" DrawAspect=\"Content\" r:id=\"{payload_rel}\"/></w:object>")
}

/// Place the drawing in the actual anchor paragraph, not an unrelated empty
/// block. Tight alpha wrapping is conservatively exported as square with warning.
fn floating_drawing_xml(image:&model::BlockImage,document:&Document,context:&ExportContext,drawing_id:&mut u32)->String{
    let Some(anchor)=&image.anchor else{return String::new();};
    let inline=drawing_xml(&image.resource,image.width_mm,image.height_mm,image.alt_text.as_deref(),&image.effects,document,context,drawing_id);
    let Some(start)=inline.find("<wp:extent")else{return inline;};let Some(end)=inline.rfind("</wp:inline>")else{return inline;};
    let mut content=inline[start..end].to_string();let Some(extent_end)=content.find("/>").map(|n|n+2)else{return inline;};
    let wrap=wrap_xml(image.wrap);
    content.insert_str(extent_end,wrap);
    let page=anchor.reference==model::AnchorReference::Page;let h=if page{"page"}else{"column"};let v=if page{"page"}else{"paragraph"};
    let x=(anchor.x_mm*MM_TO_EMU).round()as i64;let y=(anchor.y_mm*MM_TO_EMU).round()as i64;let gap=(anchor.gap_mm*MM_TO_EMU).round()as i64;let behind=usize::from(image.wrap==model::ImageWrap::BehindText);let locked=usize::from(anchor.locked);
    format!("<w:drawing><wp:anchor distT=\"{gap}\" distB=\"{gap}\" distL=\"{gap}\" distR=\"{gap}\" simplePos=\"0\" relativeHeight=\"1\" behindDoc=\"{behind}\" locked=\"{locked}\" layoutInCell=\"0\" allowOverlap=\"1\"><wp:simplePos x=\"0\" y=\"0\"/><wp:positionH relativeFrom=\"{h}\"><wp:posOffset>{x}</wp:posOffset></wp:positionH><wp:positionV relativeFrom=\"{v}\"><wp:posOffset>{y}</wp:posOffset></wp:positionV>{content}</wp:anchor></w:drawing>")
}

fn field_xml(field: &model::Field) -> String {
    let instruction = match &field.kind {
        FieldKind::PageNumber => "PAGE".to_owned(),
        FieldKind::PageReference{bookmark} => format!("PAGEREF {} \\h",bookmark),
        FieldKind::FootnoteReference{..} => String::new(),
        FieldKind::PageCount => "NUMPAGES".to_owned(),
        FieldKind::SectionNumber => "SECTION".to_owned(),
        FieldKind::Date { format } => format.as_ref().map(|f| format!("DATE \\@ \"{}\"", f)).unwrap_or_else(|| "DATE".into()),
        FieldKind::Raw { instruction } => instruction.clone(),
    };
    let display = field.display_text.as_deref().unwrap_or("");
    let properties = run_props_xml(&CharacterProperties { color: field.color.clone(), ..Default::default() });
    format!("<w:r><w:fldChar w:fldCharType=\"begin\"/></w:r><w:r><w:instrText xml:space=\"preserve\"> {}</w:instrText></w:r><w:r><w:fldChar w:fldCharType=\"separate\"/></w:r><w:r>{properties}<w:t>{}</w:t></w:r><w:r><w:fldChar w:fldCharType=\"end\"/></w:r>", xml_escape(&instruction), xml_escape(display))
}

fn text_run_props_xml(run: &TextRun) -> String {
    let mut props = run_props_xml(&run.properties);
    if let Some(style)=&run.style {
        let style=format!("<w:rStyle w:val=\"{}\"/>",xml_escape(&style.0));
        if props.is_empty(){props=format!("<w:rPr>{style}</w:rPr>");}else{props.insert_str("<w:rPr>".len(),&style);}
    }
    props
}

fn run_xml(run: &TextRun) -> String {
    let props = text_run_props_xml(run);
    format!("<w:r>{props}<w:t xml:space=\"preserve\">{}</w:t></w:r>", xml_escape(&run.text))
}

fn run_props_xml(p: &CharacterProperties) -> String {
    let mut props = String::new();
    if let Some(value)=p.bold { props.push_str(if value{"<w:b/>"}else{"<w:b w:val=\"0\"/>"}); }
    if let Some(value)=p.italic { props.push_str(if value{"<w:i/>"}else{"<w:i w:val=\"0\"/>"}); }
    if let Some(value)=p.right_to_left { props.push_str(if value{"<w:rtl/>"}else{"<w:rtl w:val=\"0\"/>"}); }
    if let Some(value)=p.underline { props.push_str(if value{"<w:u w:val=\"single\"/>"}else{"<w:u w:val=\"none\"/>"}); }
    if let Some(size) = p.font_size_pt { props.push_str(&format!("<w:sz w:val=\"{}\"/>", (size * 2.0).round() as i32)); }
    props.push_str(&theme_fonts::properties(p));
    if let Some(color) = &p.color { props.push_str(&format!("<w:color w:val=\"{}\"/>", xml_escape(color.trim_start_matches('#')))); }
    if let Some(color)=&p.highlight { if color=="none" {props.push_str("<w:highlight w:val=\"none\"/><w:shd w:val=\"clear\" w:fill=\"auto\"/>");} else {props.push_str(&format!("<w:shd w:val=\"clear\" w:fill=\"{}\"/>",xml_escape(color.trim_start_matches('#'))));} }
    if let Some(typography) = &p.typography {
        props.push_str(&format!("<w14:ligatures xmlns:w14=\"http://schemas.microsoft.com/office/word/2010/wordml\" w14:val=\"{}\"/>", crate::ligatures::to_word(typography)));
    }
    if props.is_empty() { String::new() } else { format!("<w:rPr>{props}</w:rPr>") }
}

fn section_properties_xml(section: &Section, section_index: usize, context: &ExportContext) -> String {
    let p = &section.properties;
    let mut inner = String::new();
    for kind in ["default", "first", "even"] {
        if let Some(rel) = context.header_footer_rel(section_index, kind, true) {
            inner.push_str(&format!("<w:headerReference w:type=\"{kind}\" r:id=\"{rel}\"/>"));
        }
        if let Some(rel) = context.header_footer_rel(section_index, kind, false) {
            inner.push_str(&format!("<w:footerReference w:type=\"{kind}\" r:id=\"{rel}\"/>"));
        }
    }
    inner.push_str(&format!(
        "<w:pgSz w:w=\"{}\" w:h=\"{}\"/><w:pgMar w:top=\"{}\" w:right=\"{}\" w:bottom=\"{}\" w:left=\"{}\" w:header=\"{}\" w:footer=\"{}\" w:gutter=\"0\"/>",
        twips(p.page_width_mm), twips(p.page_height_mm), twips(p.margin_top_mm), twips(p.margin_right_mm), twips(p.margin_bottom_mm), twips(p.margin_left_mm), twips(p.header_distance_mm), twips(p.footer_distance_mm)
    ));
    inner.push_str(&columns_xml(p));
    if p.page_number_start.is_some() || p.page_number_format.is_some() {
        inner.push_str("<w:pgNumType");
        if let Some(start) = p.page_number_start { inner.push_str(&format!(" w:start=\"{start}\"")); }
        if let Some(format) = p.page_number_format {
            let value = match format {
                model::PageNumberFormat::Decimal => "decimal",
                model::PageNumberFormat::UpperRoman => "upperRoman",
                model::PageNumberFormat::LowerRoman => "lowerRoman",
                model::PageNumberFormat::UpperLetter => "upperLetter",
                model::PageNumberFormat::LowerLetter => "lowerLetter",
            };
            inner.push_str(&format!(" w:fmt=\"{value}\""));
        }
        inner.push_str("/>");
    }
    if p.different_first_page { inner.push_str("<w:titlePg/>"); }
    let start = match p.start_on { model::SectionStart::Continuous => "continuous", model::SectionStart::NewPage => "nextPage", model::SectionStart::OddPage => "oddPage", model::SectionStart::EvenPage => "evenPage", model::SectionStart::NewColumn => "nextColumn" };
    inner.push_str(&format!("<w:type w:val=\"{start}\"/>"));
    format!("<w:sectPr>{inner}</w:sectPr>")
}

fn has_numbering(document: &Document) -> bool {
    !document.lists.is_empty() || !document.styles.list_styles.is_empty()
}

fn all_lists(document: &Document) -> BTreeMap<&NodeId, &ListDefinition> {
    document.lists.iter().chain(document.styles.list_styles.iter().map(|(id, style)| (id, &style.definition))).collect()
}

fn numbering_xml(document: &Document) -> String {
    let mut abstracts = String::new();
    let mut nums = String::new();
    for (index, list) in all_lists(document).values().enumerate() {
        let abstract_id = index + 1;
        let named = document.styles.list_styles.get(&list.id);
        let name = named.map(|style| format!(r#"<w:multiLevelType w:val="multilevel"/><w:name w:val="{}"/><w:styleLink w:val="{}"/>"#,
            xml_escape(&style.name), list_style_id(&list.id))).unwrap_or_default();
        let mut levels = String::new();
        for level in &list.levels {
            let fmt = match level.format { ListFormat::Bullet => "bullet", ListFormat::Decimal => "decimal", ListFormat::LowerLetter => "lowerLetter", ListFormat::UpperLetter => "upperLetter", ListFormat::LowerRoman => "lowerRoman", ListFormat::UpperRoman => "upperRoman" };
            let run_props = if level.format == ListFormat::Bullet { "<w:rPr><w:rFonts w:ascii=\"Symbol\" w:hAnsi=\"Symbol\"/></w:rPr>" } else { "" };
            let heading = named.and_then(|style| style.headings.get(&level.level))
                .map(|id| format!(r#"<w:pStyle w:val="{}"/>"#, xml_escape(&id.0))).unwrap_or_default();
            levels.push_str(&format!("<w:lvl w:ilvl=\"{}\"><w:start w:val=\"{}\"/><w:numFmt w:val=\"{}\"/>{heading}<w:lvlText w:val=\"{}\"/><w:pPr><w:ind w:left=\"{}\" w:hanging=\"{}\"/></w:pPr>{run_props}</w:lvl>", level.level, level.start_at, fmt, xml_escape(&level.text), twips(level.left_indent_mm), twips(level.hanging_indent_mm)));
        }
        abstracts.push_str(&format!("<w:abstractNum w:abstractNumId=\"{abstract_id}\">{name}{levels}</w:abstractNum>"));
        nums.push_str(&format!("<w:num w:numId=\"{abstract_id}\"><w:abstractNumId w:val=\"{abstract_id}\"/></w:num>"));
    }
    format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" mc:Ignorable="w14">{abstracts}{nums}</w:numbering>"#)
}

fn list_style_id(id: &NodeId) -> String {
    format!("ImpressList{}", xml_escape(&id.0))
}

fn document_relationships(document: &Document, context: &ExportContext) -> String {
    let mut body = String::from("<Relationship Id=\"rIdStyles\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/>");
    body.push_str("<Relationship Id=\"rIdImpressFontTable\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/fontTable\" Target=\"fontTable.xml\"/>");
    let mut reserved = BTreeSet::from(["rIdStyles".to_owned(),"rIdImpressFontTable".to_owned()]);
    if has_numbering(document) {
        body.push_str("<Relationship Id=\"rIdNumbering\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering\" Target=\"numbering.xml\"/>");
        reserved.insert("rIdNumbering".to_owned());
    }
    for (_, (rel_id, target)) in &context.images {
        body.push_str(&format!("<Relationship Id=\"{rel_id}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"{}\"/>", xml_escape(target)));
        reserved.insert(rel_id.clone());
    }
    for (id,(rel_id,target)) in &context.embedded {
        let kind=if document.resources.get(id).is_some_and(|r|r.media_type.ends_with(".package")){"package"}else{"oleObject"};
        body.push_str(&format!("<Relationship Id=\"{rel_id}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/{kind}\" Target=\"{}\"/>",xml_escape(target)));
        reserved.insert(rel_id.clone());
    }
    for part in &context.header_footer_parts {
        let relation = if part.header { "header" } else { "footer" };
        body.push_str(&format!("<Relationship Id=\"{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/{relation}\" Target=\"{}\"/>", part.rel_id, part.target));
        reserved.insert(part.rel_id.clone());
    }
    for (target,id) in &context.links {
        body.push_str(&format!("<Relationship Id=\"{id}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink\" Target=\"{}\" TargetMode=\"External\"/>",xml_escape(target)));reserved.insert(id.clone());
    }
    for (stem,kind,rid) in [("footnote",model::NoteKind::Footnote,"rIdImpressFootnotes"),("endnote",model::NoteKind::Endnote,"rIdImpressEndnotes")] {
        if document.review.footnotes.values().any(|n|n.kind==kind) {body.push_str(&format!("<Relationship Id=\"{rid}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/{stem}s\" Target=\"{stem}s.xml\"/>"));reserved.insert(rid.into());}
    }
    if !document.review.comments.is_empty(){body.push_str("<Relationship Id=\"rIdImpressComments\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments\" Target=\"comments.xml\"/>");reserved.insert("rIdImpressComments".into());}
    body.push_str("<Relationship Id=\"rIdImpressSettings\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings\" Target=\"settings.xml\"/>");reserved.insert("rIdImpressSettings".into());
    body.push_str(&preserved_relationship_elements(
        document,
        "word/_rels/document.xml.rels",
        &reserved,
        &["/fontTable", "/styles", "/numbering", "/image", "/header", "/footer", "/hyperlink", "/comments", "/footnotes", "/endnotes", "/settings"],
    ));
    format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{body}</Relationships>"#)
}

fn relationship_part_name(part_name: &str) -> String {
    match part_name.rsplit_once('/') {
        Some((dir, file)) => format!("{dir}/_rels/{file}.rels"),
        None => format!("_rels/{part_name}.rels"),
    }
}

fn header_footer_relationships(context: &ExportContext,document:&Document) -> String {
    let mut body = String::new();
    for (_, (rel_id, target)) in &context.images {
        body.push_str(&format!("<Relationship Id=\"{rel_id}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"{}\"/>", xml_escape(target)));
    }
    for (id,(rel_id,target)) in &context.embedded {
        let kind=if document.resources.get(id).is_some_and(|r|r.media_type.ends_with(".package")){"package"}else{"oleObject"};
        body.push_str(&format!("<Relationship Id=\"{rel_id}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/{kind}\" Target=\"{}\"/>",xml_escape(target)));
    }
    for (target,id) in &context.links { body.push_str(&format!("<Relationship Id=\"{id}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink\" Target=\"{}\" TargetMode=\"External\"/>",xml_escape(target))); }
    format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{body}</Relationships>"#)
}

fn content_types(document: &Document, context: &ExportContext) -> String {
    let numbering = if !has_numbering(document) { "" } else { "<Override PartName=\"/word/numbering.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml\"/>" };
    let mut headers = String::new();
    for part in &context.header_footer_parts {
        let content_type = if part.header { "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml" } else { "application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml" };
        headers.push_str(&format!("<Override PartName=\"/{}\" ContentType=\"{content_type}\"/>", part.part_name));
    }
    let mut notes=String::new();
    for (stem,kind) in [("footnote",model::NoteKind::Footnote),("endnote",model::NoteKind::Endnote)] {
        if document.review.footnotes.values().any(|n|n.kind==kind) {notes.push_str(&format!("<Override PartName=\"/word/{stem}s.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.{stem}s+xml\"/>"));}
    }
    let comments=if document.review.comments.is_empty(){""}else{"<Override PartName=\"/word/comments.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml\"/>"};
    let preserved = preserved_content_type_elements(document);
    let embedded=context.embedded.iter().map(|(id,(_,target))|{
        let media=&document.resources[id].media_type;
        format!("<Override PartName=\"/word/{}\" ContentType=\"{}\"/>",xml_escape(target),xml_escape(media))
    }).collect::<String>();
    format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Default Extension="jpg" ContentType="image/jpeg"/><Default Extension="jpeg" ContentType="image/jpeg"/><Default Extension="gif" ContentType="image/gif"/><Default Extension="bmp" ContentType="image/bmp"/><Default Extension="tif" ContentType="image/tiff"/><Default Extension="tiff" ContentType="image/tiff"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/><Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/><Override PartName="/word/fontTable.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.fontTable+xml"/><Default Extension="odttf" ContentType="application/vnd.openxmlformats-officedocument.obfuscatedFont"/>{numbering}{headers}{notes}{comments}{embedded}{preserved}</Types>"#)
}

fn generated_part_name(name: &str) -> bool {
    name == "word/fontTable.xml" || name == "word/_rels/fontTable.xml.rels" || name.starts_with("word/fonts/") || name == "word/endnotes.xml" || name == "word/_rels/endnotes.xml.rels" || name == "word/settings.xml" || name == "word/footnotes.xml" || name == "word/_rels/footnotes.xml.rels" || name == "word/comments.xml" || name == "[Content_Types].xml" || name == "_rels/.rels" || name == "word/document.xml"
        || name == "word/_rels/document.xml.rels" || name == "word/styles.xml"
        || name == "word/numbering.xml" || name.starts_with("word/header") || name.starts_with("word/footer")
        || name.starts_with("word/_rels/header") || name.starts_with("word/_rels/footer")
        || name.starts_with("word/media/") || name.starts_with("word/embeddings/impressObject")
}

fn decode_base64(text: &str) -> Result<Vec<u8>, ()> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut reverse = [255u8; 256];
    for (i, byte) in TABLE.iter().enumerate() { reverse[*byte as usize] = i as u8; }
    let input: Vec<u8> = text.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if input.len() % 4 != 0 { return Err(()); }
    let mut out = Vec::with_capacity(input.len() / 4 * 3);
    for chunk in input.chunks(4) {
        let a = reverse[chunk[0] as usize]; let b = reverse[chunk[1] as usize];
        if a == 255 || b == 255 { return Err(()); }
        let c = if chunk[2] == b'=' { 0 } else { reverse[chunk[2] as usize] };
        let d = if chunk[3] == b'=' { 0 } else { reverse[chunk[3] as usize] };
        if c == 255 || d == 255 { return Err(()); }
        let n = ((a as u32) << 18) | ((b as u32) << 12) | ((c as u32) << 6) | d as u32;
        out.push((n >> 16) as u8);
        if chunk[2] != b'=' { out.push((n >> 8) as u8); }
        if chunk[3] != b'=' { out.push(n as u8); }
    }
    Ok(out)
}

fn preserved_text(document: &Document, name: &str) -> Option<String> {
    let part = document.preserved_parts.get(name)?;
    String::from_utf8(decode_base64(&part.data_base64).ok()?).ok()
}

fn preserved_relationship_elements(
    document: &Document,
    name: &str,
    reserved_ids: &BTreeSet<String>,
    excluded_type_suffixes: &[&str],
) -> String {
    let Some(xml) = preserved_text(document, name) else { return String::new(); };
    let mut reader = Reader::from_str(&xml);
    reader.config_mut().trim_text(true);
    let mut out = String::new();
    loop {
        match reader.read_event() {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) if e.name().as_ref().ends_with("Relationship") => {
                let mut attrs = BTreeMap::new();
                for attr in e.attributes().with_checks(false).flatten() {
                    attrs.insert(attr.key.as_ref().to_owned(), quick_xml::escape::unescape(attr.value.as_ref()).map(|v|v.into_owned()).unwrap_or_default());
                }
                let Some(id) = attrs.get("Id") else { continue; };
                let Some(kind) = attrs.get("Type") else { continue; };
                let Some(target) = attrs.get("Target") else { continue; };
                if reserved_ids.contains(id) || excluded_type_suffixes.iter().any(|suffix| kind.ends_with(suffix)) { continue; }
                out.push_str(&format!("<Relationship Id=\"{}\" Type=\"{}\" Target=\"{}\"", xml_escape(id), xml_escape(kind), xml_escape(target)));
                if let Some(mode) = attrs.get("TargetMode") { out.push_str(&format!(" TargetMode=\"{}\"", xml_escape(mode))); }
                out.push_str("/>");
            }
            Ok(Event::Eof) => break,
            Err(_) => return String::new(),
            _ => {}
        }
    }
    out
}

fn preserved_content_type_elements(document: &Document) -> String {
    let Some(xml) = preserved_text(document, "[Content_Types].xml") else { return String::new(); };
    let generated_extensions = ["odttf", "rels", "xml", "png", "jpg", "jpeg", "gif", "bmp", "tif", "tiff"];
    let mut reader = Reader::from_str(&xml);
    reader.config_mut().trim_text(true);
    let mut out = String::new();
    loop {
        match reader.read_event() {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) => {
                let name = e.name().as_ref().to_owned();
                if !(name.ends_with("Default") || name.ends_with("Override")) { continue; }
                let mut attrs = BTreeMap::new();
                for attr in e.attributes().with_checks(false).flatten() {
                    attrs.insert(attr.key.as_ref().to_owned(), quick_xml::escape::unescape(attr.value.as_ref()).map(|v|v.into_owned()).unwrap_or_default());
                }
                if name.ends_with("Default") {
                    let Some(extension) = attrs.get("Extension") else { continue; };
                    if generated_extensions.iter().any(|known| extension.eq_ignore_ascii_case(known)) { continue; }
                    let Some(content_type) = attrs.get("ContentType") else { continue; };
                    out.push_str(&format!("<Default Extension=\"{}\" ContentType=\"{}\"/>", xml_escape(extension), xml_escape(content_type)));
                } else {
                    let Some(part_name) = attrs.get("PartName") else { continue; };
                    let normalized = part_name.trim_start_matches('/');
                    if generated_part_name(normalized) { continue; }
                    let Some(content_type) = attrs.get("ContentType") else { continue; };
                    if content_type=="application/vnd.openxmlformats-officedocument.obfuscatedFont" || content_type=="application/vnd.openxmlformats-officedocument.wordprocessingml.fontTable+xml" {continue;}
                    out.push_str(&format!("<Override PartName=\"{}\" ContentType=\"{}\"/>", xml_escape(part_name), xml_escape(content_type)));
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => return String::new(),
            _ => {}
        }
    }
    out
}

fn write_member<W: Write + std::io::Seek>(zip: &mut ZipWriter<W>, name: &str, text: &str, options: SimpleFileOptions) -> FormatResult<()> {
    zip.start_file(name, options).map_err(zip_err)?;
    zip.write_all(text.as_bytes())?;
    Ok(())
}

fn root_relationships(document: &Document) -> String {
    let reserved = BTreeSet::from(["rId1".to_owned()]);
    let preserved = preserved_relationship_elements(document, "_rels/.rels", &reserved, &["/officeDocument"]);
    format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>{preserved}</Relationships>"#)
}
/// `w:tblpPr` and `w:tblOverlap` for a floating table. Word has no
/// top-and-bottom wrap for tables; such tables export as wrapping around.
fn table_position_xml(p: &model::TablePosition) -> String {
    use model::{HorizontalFrom, HorizontalPlacement, VerticalFrom, VerticalPlacement};
    let horz = match p.horizontal_from { HorizontalFrom::Page => "page", HorizontalFrom::Margin => "margin", HorizontalFrom::Column => "text" };
    let vert = match p.vertical_from { VerticalFrom::Page => "page", VerticalFrom::Margin => "margin", VerticalFrom::Paragraph => "text" };
    let x = match p.horizontal {
        HorizontalPlacement::Offset(mm) => format!("w:tblpX=\"{}\"", twips(mm)),
        HorizontalPlacement::Left => "w:tblpXSpec=\"left\"".into(), HorizontalPlacement::Center => "w:tblpXSpec=\"center\"".into(),
        HorizontalPlacement::Right => "w:tblpXSpec=\"right\"".into(), HorizontalPlacement::Inside => "w:tblpXSpec=\"inside\"".into(),
        HorizontalPlacement::Outside => "w:tblpXSpec=\"outside\"".into(),
    };
    let y = match p.vertical {
        VerticalPlacement::Offset(mm) => format!("w:tblpY=\"{}\"", twips(mm)),
        VerticalPlacement::Top => "w:tblpYSpec=\"top\"".into(), VerticalPlacement::Center => "w:tblpYSpec=\"center\"".into(),
        VerticalPlacement::Bottom => "w:tblpYSpec=\"bottom\"".into(),
    };
    let overlap = if p.allow_overlap { "" } else { "<w:tblOverlap w:val=\"never\"/>" };
    format!("<w:tblpPr w:leftFromText=\"{}\" w:rightFromText=\"{}\" w:topFromText=\"{}\" w:bottomFromText=\"{}\" w:vertAnchor=\"{vert}\" w:horzAnchor=\"{horz}\" {x} {y}/>{overlap}",
        twips(p.distance_left_mm), twips(p.distance_right_mm), twips(p.distance_top_mm), twips(p.distance_bottom_mm))
}

/// `w:cols`: equal columns as a count and a gap, otherwise one `w:col`
/// per column with its width and the space after it; `w:sep` draws the line.
fn columns_xml(p: &model::SectionProperties) -> String {
    let separator = if p.column_separator { " w:sep=\"1\"" } else { "" };
    if p.has_equal_columns() {
        return format!("<w:cols w:num=\"{}\" w:space=\"{}\"{separator}/>", p.column_count.max(1), twips(p.column_gap_mm));
    }
    let columns: String = p.columns.iter().map(|column| {
        format!("<w:col w:w=\"{}\" w:space=\"{}\"/>", twips(column.width_mm), twips(column.spacing_mm))
    }).collect();
    format!("<w:cols w:num=\"{}\" w:equalWidth=\"0\"{separator}>{columns}</w:cols>", p.columns.len())
}

fn twips(mm: f32) -> i32 { (mm * MM_TO_TWIPS).round() as i32 }
fn media_extension(media_type: &str) -> &'static str { match media_type { "image/jpeg" | "image/jpg" => "jpg", "image/gif" => "gif", "image/bmp" => "bmp", "image/tiff" => "tif", _ => "png" } }
fn xml_escape(s: &str) -> String { s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;") }
fn zip_err(e: zip::result::ZipError) -> FormatError { FormatError::Invalid { format: "DOCX", message: e.to_string() } }

#[cfg(test)] #[path = "../tests/unit/package/column_tests.rs"] mod column_tests;

#[cfg(test)] #[path = "../tests/unit/package.rs"] mod tests;

/// The `wp:wrap*` element of a floating picture or text box.
fn wrap_xml(wrap: model::ImageWrap) -> &'static str {
    match wrap {
        model::ImageWrap::BehindText | model::ImageWrap::InFrontOfText => "<wp:wrapNone/>",
        model::ImageWrap::TopAndBottom => "<wp:wrapTopAndBottom/>",
        _ => "<wp:wrapSquare wrapText=\"bothSides\"/>",
    }
}
