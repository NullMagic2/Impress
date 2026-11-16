//! Carry native styles and source-position anchors into Impress's OOXML bridge.
use crate::{
    assemble::LegacyBuildOutput,
    error::Result,
    fib::Fib,
    model::{AuthoredNote, Block, Run, SourceRegionKind},
    opc::Package,
    util::{u16le, u32le},
    write::{esc_attr, esc_text},
    DocState,
};
use quick_xml::{events::Event, Reader, Writer};
use std::collections::{BTreeMap, BTreeSet};

const WORD_NAMESPACE: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const COMMENTS_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments";
const COMMENTS_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml";
const STYLES_PART: &str = "word/styles.xml";
const DOCUMENT_PART: &str = "word/document.xml";
const COMMENTS_PART: &str = "word/comments.xml";
const MARKER_PREFIX: &str = "ImpressCp";
const NOTE_REFERENCE_BYTES: usize = 2;
const COMMENT_REFERENCE_BYTES: usize = 30;
const SHAPE_REFERENCE_BYTES: usize = 26;
const BOOKMARK_BEGIN_BYTES: usize = 4;

#[derive(Default)]
pub(crate) struct Prepared {
    events: BTreeMap<usize, Vec<String>>,
    comments: String,
    unprojected_shapes: usize,
}

/// Keep conversion-only metadata out of the upstream reader/render model.
pub(crate) fn clear_metadata(model: &mut crate::DocModel) {
    fn clear(blocks: &mut [Block]) {
        for block in blocks {
            match block {
                Block::Paragraph(paragraph) => {
                    paragraph.props.native_ppr = None;
                    paragraph.props.native_num_id = None;
                    paragraph.props.style_id = None;
                    if paragraph.props.heading_level.is_some() {
                        paragraph.props.list = None;
                    }
                    paragraph.runs.retain(|run| {
                        !run.bookmark
                            .as_deref()
                            .is_some_and(|name| name.starts_with(MARKER_PREFIX))
                    });
                    for run in &mut paragraph.runs {
                        run.props.native_rpr = None;
                    }
                    fn plain(run: &Run) -> bool {
                        matches!(run.field, crate::model::FieldRole::None)
                            && run.image.is_none()
                            && run.comment.is_none()
                            && run.revision.is_none()
                            && run.content_control.is_none()
                            && run.bookmark.is_none()
                            && run.note.is_none()
                            && run.note_blocks.is_empty()
                            && !run.field_dirty
                            && run.field_unsupported_reason.is_none()
                    }
                    let mut runs: Vec<Run> = Vec::new();
                    for run in std::mem::take(&mut paragraph.runs) {
                        if let Some(previous) = runs.last_mut().filter(|previous| {
                            plain(previous) && plain(&run) && previous.props == run.props
                        }) {
                            previous.text.push_str(&run.text);
                        } else {
                            runs.push(run);
                        }
                    }
                    paragraph.runs = runs;
                }
                Block::Table(table) => {
                    table.native_properties = None;
                    table.native_grid_twips.clear();
                    table.native_row_properties.clear();
                    for row in &mut table.rows {
                        for cell in &mut row.cells {
                            cell.native_properties = None;
                            clear(&mut cell.blocks);
                        }
                    }
                }
                Block::SectionBreak(section) => {
                    for blocks in [
                        &mut section.header,
                        &mut section.footer,
                        &mut section.first_header,
                        &mut section.first_footer,
                        &mut section.even_header,
                        &mut section.even_footer,
                    ] {
                        clear(blocks);
                    }
                }
                _ => {}
            }
        }
    }
    clear(&mut model.blocks);
    model.meta.stats = crate::assemble::compute_stats(&model.blocks);
    for blocks in [
        &mut model.setup.header,
        &mut model.setup.footer,
        &mut model.setup.first_header,
        &mut model.setup.first_footer,
        &mut model.setup.even_header,
        &mut model.setup.even_footer,
    ] {
        clear(blocks);
    }
}

pub(crate) fn conversion_warnings(state: &DocState) -> Vec<crate::DocumentWarning> {
    let mut assembled = crate::legacy_build_output_from_doc_state(state);
    crate::legacy_project_promoted_running_surfaces(&mut assembled);
    let prepared = prepare(state, &mut assembled);
    let remaining = |kind| {
        assembled
            .model
            .regions
            .iter()
            .filter(|region| region.kind == kind)
            .map(|region| region.source_len_cp)
            .sum()
    };
    let mut warnings = Vec::new();
    if let Some(warning) = crate::report::legacy_doc_flattened_subdocuments_warning(
        remaining(SourceRegionKind::Footnote),
        remaining(SourceRegionKind::HeaderFooter),
        remaining(SourceRegionKind::Annotation),
        remaining(SourceRegionKind::Endnote),
        remaining(SourceRegionKind::TextBox),
    ) {
        warnings.push(warning);
    }
    if prepared.unprojected_shapes > 0 {
        warnings.push(crate::DocumentWarning::FloatingShapePlaceholderOnly {
            count: prepared.unprojected_shapes,
        });
    }
    warnings
}

pub(crate) fn marker_cps(word: &[u8], table: &[u8], fib: &Fib) -> BTreeSet<usize> {
    let mut markers = BTreeSet::new();
    for (index, size, base) in [
        (2, NOTE_REFERENCE_BYTES, 0),
        (46, NOTE_REFERENCE_BYTES, 0),
        (4, COMMENT_REFERENCE_BYTES, 0),
        (40, SHAPE_REFERENCE_BYTES, 0),
        (
            41,
            SHAPE_REFERENCE_BYTES,
            (fib.ccp_text + fib.ccp_ftn) as usize,
        ),
    ] {
        markers.extend(
            reference_cps(word, table, index, size)
                .into_iter()
                .map(|cp| cp + base),
        );
    }
    for (begin, end) in [(22, 23), (42, 43)] {
        for (start, end) in bookmark_ranges(word, table, begin, end) {
            markers.insert(start);
            markers.insert(end);
        }
    }
    markers
}

pub(crate) fn pair<'a>(word: &[u8], table: &'a [u8], index: usize) -> Option<&'a [u8]> {
    let (offset, length) = crate::fib::fc_lcb_pair(word, index)?;
    table.get(offset..offset.checked_add(length)?)
}

fn reference_cps(word: &[u8], table: &[u8], pair_index: usize, size: usize) -> Vec<usize> {
    let Some(bytes) = pair(word, table, pair_index) else {
        return Vec::new();
    };
    if bytes.len() < 4 || (bytes.len() - 4) % (4 + size) != 0 {
        return Vec::new();
    }
    let count = (bytes.len() - 4) / (4 + size);
    (0..count)
        .filter_map(|index| u32le(bytes, index * 4).map(|cp| cp as usize))
        .collect()
}

fn bookmark_ranges(word: &[u8], table: &[u8], starts: usize, ends: usize) -> Vec<(usize, usize)> {
    let (Some(starts), Some(ends)) = (pair(word, table, starts), pair(word, table, ends)) else {
        return Vec::new();
    };
    if starts.len() < 4
        || (starts.len() - 4) % (4 + BOOKMARK_BEGIN_BYTES) != 0
        || ends.len() % 4 != 0
    {
        return Vec::new();
    }
    let count = (starts.len() - 4) / (4 + BOOKMARK_BEGIN_BYTES);
    let data_start = (count + 1) * 4;
    (0..count)
        .filter_map(|index| {
            let start = u32le(starts, index * 4)? as usize;
            let end_index = u16le(starts, data_start + index * BOOKMARK_BEGIN_BYTES)? as usize;
            let end = u32le(ends, end_index.checked_mul(4)?)? as usize;
            (end >= start).then_some((start, end))
        })
        .collect()
}

fn strings(bytes: &[u8]) -> Option<Vec<(String, Vec<u8>)>> {
    if u16le(bytes, 0)? != 0xFFFF {
        return None;
    }
    let count = usize::from(u16le(bytes, 2)?);
    let extra_size = usize::from(u16le(bytes, 4)?);
    let mut offset = 6usize;
    let mut output = Vec::with_capacity(count);
    for _ in 0..count {
        let length = usize::from(u16le(bytes, offset)?);
        offset += 2;
        let end = offset.checked_add(length.checked_mul(2)?)?;
        let units = bytes
            .get(offset..end)?
            .chunks_exact(2)
            .map(|unit| u16::from_le_bytes([unit[0], unit[1]]))
            .collect::<Vec<_>>();
        let text = String::from_utf16(&units).ok()?;
        offset = end;
        let extra = bytes.get(offset..offset.checked_add(extra_size)?)?.to_vec();
        offset += extra_size;
        output.push((text, extra));
    }
    Some(output)
}

pub(crate) fn prepare(state: &DocState, assembled: &mut LegacyBuildOutput) -> Prepared {
    let unprojected_shapes = crate::legacy_drawings::attach(state, &mut assembled.model);
    let mut prepared = Prepared {
        unprojected_shapes,
        ..Prepared::default()
    };
    let mut removed = vec![false; assembled.model.blocks.len()];
    let regions = assembled.model.regions.clone();
    let mut actual_markers = BTreeSet::new();
    visit_runs(&mut assembled.model.blocks, &mut |run| {
        if let Some(cp) = marker_cp(run) {
            actual_markers.insert(cp);
        }
    });
    for (kind, note_kind, references) in [
        (
            SourceRegionKind::Footnote,
            crate::NoteKind::Footnote,
            reference_cps(&state.word, &state.table, 2, NOTE_REFERENCE_BYTES),
        ),
        (
            SourceRegionKind::Endnote,
            crate::NoteKind::Endnote,
            reference_cps(&state.word, &state.table, 46, NOTE_REFERENCE_BYTES),
        ),
    ] {
        for region in regions.iter().filter(|region| region.kind == kind) {
            let Some(cp) = region
                .source_story_index
                .and_then(|index| references.get(index).copied())
            else {
                continue;
            };
            let start = region.block_start.min(assembled.model.blocks.len());
            let end = region.block_end.min(assembled.model.blocks.len());
            if start >= end {
                continue;
            }
            let content = assembled.model.blocks[start..end].to_vec();
            let text = crate::docx::blocks_text(&content);
            let mut attached = false;
            visit_runs(&mut assembled.model.blocks, &mut |run| {
                if marker_cp(run) == Some(cp) {
                    run.note = Some(AuthoredNote {
                        kind: note_kind,
                        text: text.clone(),
                    });
                    run.note_blocks = content.clone();
                    attached = true;
                }
            });
            if attached {
                removed[start..end].fill(true);
            }
        }
    }
    let names = pair(&state.word, &state.table, 21)
        .and_then(strings)
        .unwrap_or_default();
    let ranges = bookmark_ranges(&state.word, &state.table, 22, 23);
    for (index, ((name, _), (start, end))) in names.into_iter().zip(ranges).enumerate() {
        if name.is_empty() {
            continue;
        }
        prepared.events.entry(start).or_default().push(format!(
            r#"<w:bookmarkStart w:id="{index}" w:name="{}"/>"#,
            esc_attr(&name)
        ));
        prepared
            .events
            .entry(end)
            .or_default()
            .push(format!(r#"<w:bookmarkEnd w:id="{index}"/>"#));
    }
    let comment_cps = reference_cps(&state.word, &state.table, 4, COMMENT_REFERENCE_BYTES);
    let range_names = pair(&state.word, &state.table, 37)
        .and_then(strings)
        .unwrap_or_default();
    let range_positions = bookmark_ranges(&state.word, &state.table, 42, 43);
    let range_tags = range_names
        .into_iter()
        .zip(range_positions)
        .filter_map(|((_, extra), range)| u32le(&extra, 2).map(|tag| (tag, range)))
        .collect::<BTreeMap<_, _>>();
    let records = pair(&state.word, &state.table, 4).unwrap_or_default();
    let records_start = (comment_cps.len() + 1) * 4;
    for region in regions
        .iter()
        .filter(|region| region.kind == SourceRegionKind::Annotation)
    {
        let Some(index) = region.source_story_index else {
            continue;
        };
        let Some(&reference_cp) = comment_cps.get(index) else {
            continue;
        };
        let tag = u32le(
            records,
            records_start + index * COMMENT_REFERENCE_BYTES + 26,
        )
        .unwrap_or(u32::MAX);
        let (start, end) = range_tags
            .get(&tag)
            .copied()
            .unwrap_or((reference_cp, reference_cp));
        if ![reference_cp, start, end]
            .iter()
            .all(|cp| actual_markers.contains(cp))
        {
            continue;
        }
        let metadata = state.annotation_metadata.get(index);
        let author = metadata
            .and_then(|metadata| metadata.owner_index)
            .and_then(|owner| state.annotation_owners.get(owner))
            .map(String::as_str)
            .unwrap_or("");
        let initials = metadata
            .and_then(|metadata| metadata.initials.as_deref())
            .unwrap_or("");
        let first = region.block_start.min(assembled.model.blocks.len());
        let last = region.block_end.min(assembled.model.blocks.len());
        if first >= last {
            continue;
        }
        let text = crate::docx::blocks_text(&assembled.model.blocks[first..last]);
        prepared.comments.push_str(&format!(r#"<w:comment w:id="{index}" w:author="{}" w:initials="{}"><w:p><w:r><w:t xml:space="preserve">{}</w:t></w:r></w:p></w:comment>"#, esc_attr(author),esc_attr(initials),esc_text(&text)));
        prepared
            .events
            .entry(start)
            .or_default()
            .push(format!(r#"<w:commentRangeStart w:id="{index}"/>"#));
        prepared
            .events
            .entry(end)
            .or_default()
            .push(format!(r#"<w:commentRangeEnd w:id="{index}"/>"#));
        prepared
            .events
            .entry(reference_cp)
            .or_default()
            .push(format!(
                r#"<w:r><w:commentReference w:id="{index}"/></w:r>"#
            ));
        removed[first..last].fill(true);
    }
    remove_promoted(assembled, &removed);
    prepared
}

fn marker_cp(run: &Run) -> Option<usize> {
    run.bookmark
        .as_deref()?
        .strip_prefix(MARKER_PREFIX)?
        .parse()
        .ok()
}

fn visit_runs(blocks: &mut [Block], visit: &mut impl FnMut(&mut Run)) {
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => {
                for run in &mut paragraph.runs {
                    visit(run);
                }
            }
            Block::Table(table) => {
                for row in &mut table.rows {
                    for cell in &mut row.cells {
                        visit_runs(&mut cell.blocks, visit);
                    }
                }
            }
            _ => {}
        }
    }
}

fn remove_promoted(assembled: &mut LegacyBuildOutput, removed: &[bool]) {
    let remap = |index: usize| {
        removed[..index.min(removed.len())]
            .iter()
            .filter(|removed| !**removed)
            .count()
    };
    assembled
        .model
        .regions
        .retain(|region| !removed.get(region.block_start).copied().unwrap_or(false));
    for region in &mut assembled.model.regions {
        region.block_start = remap(region.block_start);
        region.block_end = remap(region.block_end);
    }
    macro_rules! retain { ($($field:ident),*) => { $(crate::retain_legacy_conversion_slots(&mut assembled.$field, removed);)* }; }
    crate::retain_legacy_conversion_slots(&mut assembled.model.blocks, removed);
    retain!(
        pagination_hints,
        line_spacing_hints,
        tab_stops,
        note_reference_anchors,
        column_break_offsets,
        table_cell_column_break_offsets,
        section_column_gap_pt,
        section_column_layouts,
        section_column_separators,
        section_column_rtl,
        table_row_pagination,
        table_cell_pagination,
        table_cell_line_spacing,
        table_cell_tab_stops
    );
    #[cfg(feature = "render")]
    {
        retain!(render_tab_stops, render_table_cell_tab_stops);
    }
    assembled.model.meta.stats = crate::assemble::compute_stats(&assembled.model.blocks);
}

pub(crate) fn finish(state: &DocState, prepared: Prepared, bytes: Vec<u8>) -> Result<Vec<u8>> {
    let mut package = Package::from_zip(&bytes)?;
    package.set_part(
        STYLES_PART,
        state.stylesheet.source_xml(&state.fonts).into_bytes(),
        None,
    );
    if !state.lists.is_empty() {
        package.set_part(
            "word/numbering.xml",
            state.lists.source_xml(&state.fonts).into_bytes(),
            Some("application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"),
        );
        package.ensure_relationship(
            DOCUMENT_PART,
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering",
            "word/numbering.xml",
        );
    }
    if !prepared.comments.is_empty() {
        package.set_part(COMMENTS_PART, format!(r#"<?xml version="1.0" encoding="UTF-8"?><w:comments xmlns:w="{WORD_NAMESPACE}">{}</w:comments>"#, prepared.comments).into_bytes(), Some(COMMENTS_TYPE));
        package.ensure_relationship(DOCUMENT_PART, COMMENTS_REL, COMMENTS_PART);
    }
    let parts = package
        .part_names()
        .filter(|name| {
            name.starts_with("word/") && name.ends_with(".xml") && !name.contains("styles")
        })
        .map(str::to_owned)
        .collect::<Vec<_>>();
    for name in parts {
        let Some(bytes) = package.part(&name) else {
            continue;
        };
        let bytes = project_markers(&bytes, &prepared.events)?;
        package.set_part(&name, bytes, None);
    }
    package.to_zip()
}

fn project_markers(bytes: &[u8], events: &BTreeMap<usize, Vec<String>>) -> Result<Vec<u8>> {
    let mut reader = Reader::from_reader(bytes);
    let mut writer = Writer::new(Vec::new());
    let mut hidden_ids = BTreeSet::new();
    loop {
        let event = reader
            .read_event()
            .map_err(|error| crate::Error::Docx(error.to_string()))?;
        match event {
            Event::Eof => break,
            Event::Empty(ref element) | Event::Start(ref element)
                if element.name().as_ref() == b"w:bookmarkStart" =>
            {
                let attrs = element
                    .attributes()
                    .filter_map(std::result::Result::ok)
                    .collect::<Vec<_>>();
                let name = attrs
                    .iter()
                    .find(|attribute| attribute.key.as_ref() == b"w:name")
                    .map(|attribute| attribute.value.as_ref());
                let cp = name
                    .and_then(|name| std::str::from_utf8(name).ok())
                    .and_then(|name| name.strip_prefix(MARKER_PREFIX))
                    .and_then(|cp| cp.parse::<usize>().ok());
                if let Some(cp) = cp {
                    if let Some(id) = attrs
                        .iter()
                        .find(|attribute| attribute.key.as_ref() == b"w:id")
                    {
                        hidden_ids.insert(id.value.to_vec());
                    }
                    if let Some(events) = events.get(&cp) {
                        for event in events {
                            writer.get_mut().extend_from_slice(event.as_bytes());
                        }
                    }
                } else {
                    writer
                        .write_event(event)
                        .map_err(|error| crate::Error::Docx(error.to_string()))?;
                }
            }
            Event::Empty(ref element) if element.name().as_ref() == b"w:bookmarkEnd" => {
                let hidden = element
                    .attributes()
                    .filter_map(std::result::Result::ok)
                    .find(|attribute| attribute.key.as_ref() == b"w:id")
                    .is_some_and(|id| hidden_ids.contains(id.value.as_ref()));
                if !hidden {
                    writer
                        .write_event(event)
                        .map_err(|error| crate::Error::Docx(error.to_string()))?;
                }
            }
            _ => writer
                .write_event(event)
                .map_err(|error| crate::Error::Docx(error.to_string()))?,
        }
    }
    Ok(writer.into_inner())
}
