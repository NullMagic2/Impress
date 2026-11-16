//! Assemble editable text stories, tables, styles and Word97 stream directories.
use super::{
    fkp::{self, Run},
    properties::*,
};
use format::{FormatError, FormatResult, WriteSeek};
use model::*;
use std::{
    collections::BTreeMap,
    io::{Cursor, Write},
};

#[path = "write_tables.rs"]
mod tables;

const TEXT_START: usize = 1024;
const FIB_PAIR_OFFSET: usize = 154;
const FIB_PAIR_COUNT: u16 = 93;
const DEFAULT_FONT: &str = "Times New Roman";
const WORD_DOCUMENT_CLASS_ID: &str = "00020906-0000-0000-C000-000000000046";
struct Story<'a> {
    document: &'a Document,
    text: Vec<u16>,
    characters: Vec<Run>,
    paragraphs: Vec<Run>,
    fields: Vec<(u32, [u8; 2])>,
    fonts: Vec<String>,
    styles: super::styles::StyleIds,
    lists: super::lists::Lists,
    data: Vec<u8>,
    error: Option<FormatError>,
    anchors: super::annotations::Anchors,
    active_anchor: Option<(NodeId, usize)>,
    note_references: Vec<(u32, NodeId)>,
    comment_references: Vec<(u32, NodeId)>,
    note_prefix: bool,
    table_depth: u32,
    floating_images: Vec<&'a BlockImage>,
    drawings: super::drawings::Drawings,
    emitted_drawings: std::collections::BTreeSet<NodeId>,
}
impl<'a> Story<'a> {
    fn new(document: &'a Document) -> FormatResult<Self> {
        Ok(Self {
            document,
            text: Vec::new(),
            characters: Vec::new(),
            paragraphs: Vec::new(),
            fields: Vec::new(),
            fonts: vec![DEFAULT_FONT.into()],
            lists: super::lists::Lists::new(document),
            data: Vec::new(),
            error: None,
            anchors: BTreeMap::new(),
            active_anchor: None,
            note_references: Vec::new(),
            comment_references: Vec::new(),
            note_prefix: false,
            table_depth: 0,
            floating_images: super::drawings::collect(document),
            drawings: super::drawings::Drawings::default(),
            emitted_drawings: std::collections::BTreeSet::new(),
            styles: super::styles::StyleIds::new(document)?,
        })
    }
    fn cp(&self) -> u32 {
        self.text.len() as u32
    }
    fn font(&mut self, properties: &CharacterProperties) -> u16 {
        let name = properties.font_family.as_deref().unwrap_or(DEFAULT_FONT);
        if let Some(index) = self.fonts.iter().position(|font| font == name) {
            return index as u16;
        }
        self.fonts.push(name.to_string());
        (self.fonts.len() - 1) as u16
    }
    fn text(&mut self, text: &str, properties: &CharacterProperties, special: bool) {
        for span in properties.font_spans(text) {
            let mut resolved = properties.clone();
            resolved.set_font_family(span.family.map(str::to_owned));
            let font = self.font(&resolved);
            self.text_encoded(&text[span.range], character(&resolved, font, special));
        }
    }
    fn text_encoded(&mut self, text: &str, encoded: Vec<u8>) {
        let start = self.cp();
        for unit in text.encode_utf16() {
            self.text.push(match unit {
                0x000A | 0x000D => 0x000B,
                _ => unit,
            });
        }
        if start == self.cp() {
            return;
        }
        if let Some(last) = self.characters.last_mut() {
            if last.end == start && last.properties == encoded {
                last.end = start + text.encode_utf16().count() as u32;
                return;
            }
        }
        self.characters.push(Run {
            start,
            end: self.cp(),
            properties: encoded,
        });
    }
    fn control(&mut self, unit: u16, properties: &CharacterProperties, special: bool) {
        let start = self.cp();
        self.text.push(unit);
        let font = self.font(properties);
        self.characters.push(Run {
            start,
            end: start + 1,
            properties: character(properties, font, special),
        });
    }
    fn style_index(&self, id: Option<&StyleId>) -> u16 {
        self.styles.index(id)
    }
    fn inline(&mut self, items: &[Inline], base: &CharacterProperties) {
        for item in items {
            let anchor = self.active_anchor.clone();
            let before = self.cp();
            if let (Some((id, offset)), Inline::Text(run)) = (&anchor, item) {
                let mut utf16 = 0;
                let points = self.anchors.entry(id.clone()).or_default();
                for (byte, character) in run.text.char_indices() {
                    points.insert(offset + byte, before + utf16);
                    utf16 += character.len_utf16() as u32;
                }
            }
            match item {
                Inline::Text(run) => {
                    let mut effective = base.clone();
                    if run.style.is_some() {
                        effective
                            .overlay(&self.document.styles.resolve_character(run.style.as_ref()));
                    }
                    effective.overlay(&run.properties);
                    for span in effective.font_spans(&run.text) {
                        let mut direct = run.properties.clone();
                        // Binary DOC has no OOXML theme selectors. Materialize
                        // only regional choices; ordinary style references stay sparse.
                        if effective.font_slots.is_some() {
                            direct.set_font_family(span.family.map(str::to_owned));
                        }
                        let font = self.font(&direct);
                        let mut encoded = super::styles::sparse_character(&direct, font);
                        if run.style.is_some() {
                            word_property(
                                &mut encoded,
                                0x4A30,
                                self.style_index(run.style.as_ref()),
                            );
                        }
                        self.text_encoded(&run.text[span.range], encoded);
                    }
                }
                Inline::Hyperlink(link) => {
                    self.fields.push((self.cp(), [19, 88]));
                    self.control(19, base, true);
                    let instruction = if let Some(anchor) = link.target.strip_prefix('#') {
                        format!(" HYPERLINK \\l \"{}\" ", anchor.replace('"', "\\\""))
                    } else {
                        format!(" HYPERLINK \"{}\" ", link.target.replace('"', "\\\""))
                    };
                    self.text(&instruction, base, false);
                    self.fields.push((self.cp(), [20, 0]));
                    self.control(20, base, true);
                    self.inline(&link.content, base);
                    self.fields.push((self.cp(), [21, 0x80]));
                    self.control(21, base, true);
                }
                Inline::PaginationBreak { break_kind, .. } => self.control(
                    match break_kind {
                        model::PaginationBreakKind::Page => 12,
                        model::PaginationBreakKind::Column => 14,
                    },
                    base,
                    false,
                ),
                Inline::LineBreak { .. } => self.control(11, base, false),
                Inline::Tab { .. } => self.control(9, base, false),
                Inline::Field(field) => self.field(field, base),
                Inline::Image(image) => self.picture(
                    &image.resource,
                    image.width_mm,
                    image.height_mm,
                    &image.effects,
                    image.alt_text.as_deref(),
                    base,
                ),
                Inline::EmbeddedObject(_) => self.text("[Embedded object]", base, false),
                Inline::Equation(equation) => self.text(
                    equation
                        .alt_text
                        .as_deref()
                        .or(equation.source_hint.as_deref())
                        .unwrap_or("[Equation]"),
                    base,
                    false,
                ),
                Inline::TextBox(boxed) => {
                    let active = self.active_anchor.take();
                    let mut paragraphs = Vec::new();
                    model::visit_paragraphs(&boxed.blocks, &mut paragraphs);
                    for (index, paragraph) in paragraphs.iter().enumerate() {
                        if index > 0 {
                            self.control(11, base, false);
                        }
                        self.inline(&paragraph.content, base);
                    }
                    self.active_anchor = active;
                }
            }
            if !matches!(item, Inline::Hyperlink(_)) {
                if let Some((id, offset)) = anchor {
                    let length = match item {
                        Inline::Text(run) => run.text.len(),
                        Inline::LineBreak { .. }
                        | Inline::PaginationBreak { .. }
                        | Inline::Tab { .. } => 1,
                        _ => 3,
                    };
                    let end = self.cp();
                    self.anchors
                        .entry(id.clone())
                        .or_default()
                        .entry(offset)
                        .or_insert(before);
                    self.anchors
                        .entry(id.clone())
                        .or_default()
                        .insert(offset + length, end);
                    self.active_anchor = Some((id, offset + length));
                }
            }
        }
    }
    fn picture(
        &mut self,
        resource: &ResourceId,
        width: f32,
        height: f32,
        effects: &ImageEffects,
        alt: Option<&str>,
        base: &CharacterProperties,
    ) {
        let result = self
            .document
            .resources
            .get(resource)
            .ok_or_else(|| unsupported("A picture resource is missing"))
            .and_then(|resource| {
                super::pictures::append_inline(
                    &mut self.data,
                    resource,
                    width,
                    height,
                    effects,
                    alt,
                )
            });
        match result {
            Ok(offset) => {
                self.control(1, base, true);
                if let Some(run) = self.characters.last_mut() {
                    word(&mut run.properties, 0x6A03);
                    long(&mut run.properties, offset);
                }
            }
            Err(error) => {
                if self.error.is_none() {
                    self.error = Some(error);
                }
            }
        }
    }
    fn field(&mut self, field: &Field, base: &CharacterProperties) {
        let (instruction, kind) = match &field.kind {
            FieldKind::PageNumber => ("PAGE".to_string(), 33),
            FieldKind::PageCount => ("NUMPAGES".to_string(), 26),
            FieldKind::SectionNumber => ("SECTION".to_string(), 65),
            FieldKind::Date { format } => (
                format
                    .as_ref()
                    .map(|pattern| format!("DATE \\@ \"{}\"", pattern.replace('"', "\\\"")))
                    .unwrap_or_else(|| "DATE".to_string()),
                31,
            ),
            FieldKind::PageReference { bookmark } => (format!("PAGEREF {bookmark}"), 37),
            FieldKind::Raw { instruction } => (instruction.clone(), 0),

            FieldKind::FootnoteReference { note } => {
                if self.document.review.footnotes.contains_key(note) {
                    self.note_references.push((self.cp(), note.clone()));
                    self.control(2, base, true);
                    if let Some(run) = self.characters.last_mut() {
                        byte_property(&mut run.properties, 0x2A48, 1);
                    }
                } else if self.error.is_none() {
                    self.error = Some(unsupported("A note reference has no note content"));
                }
                return;
            }
        };
        let mut properties = base.clone();
        if let Some(color) = &field.color {
            properties.color = Some(color.clone());
        }
        self.fields.push((self.cp(), [19, kind]));
        self.control(19, &properties, true);
        self.text(&format!(" {instruction} "), &properties, false);
        self.fields.push((self.cp(), [20, 0]));
        self.control(20, &properties, true);
        self.text(
            field.display_text.as_deref().unwrap_or("1"),
            &properties,
            false,
        );
        self.fields.push((self.cp(), [21, 0x80]));
        self.control(21, &properties, true);
    }
    fn paragraph(&mut self, paragraph: &Paragraph, extra: &[u8], terminator: u16) {
        let start = self.cp();
        let mut properties = self
            .document
            .styles
            .resolve_paragraph(paragraph.style.as_ref());
        properties.overlay(&paragraph.properties);
        let character = self
            .document
            .styles
            .resolve_character(paragraph.style.as_ref());
        let previous_anchor = self.active_anchor.take();
        if self.note_prefix {
            self.control(2, &character, true);
            self.note_prefix = false;
        }
        let images = self
            .floating_images
            .iter()
            .filter(|image| {
                image
                    .anchor
                    .as_ref()
                    .map(|anchor| &anchor.paragraph)
                    .unwrap_or(&image.id)
                    == &paragraph.id
            })
            .copied()
            .collect::<Vec<_>>();
        for image in images {
            if self.emitted_drawings.insert(image.id.clone()) {
                if let Err(error) = self.drawings.add(self.cp(), image, self.document) {
                    self.error.get_or_insert(error);
                }
                self.control(8, &character, true);
            }
        }
        let anchor_start = self.cp();
        self.anchors
            .entry(paragraph.id.clone())
            .or_default()
            .insert(0, anchor_start);
        self.active_anchor = Some((paragraph.id.clone(), 0));
        self.inline(&paragraph.content, &character);
        for comment in self.document.review.comments.values() {
            if comment
                .range
                .as_ref()
                .is_some_and(|range| range.end.paragraph == paragraph.id)
            {
                self.comment_references
                    .push((self.cp(), comment.id.clone()));
                self.control(5, &character, true);
            }
        }
        self.active_anchor = previous_anchor;
        self.control(terminator, &character, false);
        let mut encoded = Vec::new();
        word(&mut encoded, self.style_index(paragraph.style.as_ref()));
        encoded.extend(super::styles::sparse_paragraph(&paragraph.properties));
        self.lists.paragraph(&properties, &mut encoded);
        encoded.extend_from_slice(extra);
        self.paragraphs.push(Run {
            start,
            end: self.cp(),
            properties: encoded,
        });
    }
    fn blocks(&mut self, blocks: &[Block], width: f32) -> FormatResult<()> {
        for block in blocks {
            match block {
                Block::Paragraph(paragraph) => {
                    let extra = tables::paragraph_context(self.table_depth);
                    self.paragraph(paragraph, &extra, 13);
                }
                Block::Table(table) => self.table(table, width)?,
                Block::PageBreak { .. } | Block::ColumnBreak { .. } => {
                    let mut paragraph = Paragraph::plain("");
                    paragraph.properties.space_after_pt = Some(0.0);
                    self.paragraph(
                        &paragraph,
                        &[],
                        if matches!(block, Block::PageBreak { .. }) {
                            12
                        } else {
                            14
                        },
                    );
                }
                Block::Image(image) => {
                    if image.wrap == ImageWrap::Inline {
                        let mut paragraph = Paragraph::plain("");
                        paragraph.id = image.id.clone();
                        paragraph.properties.alignment = Some(match image.alignment {
                            ImageAlignment::Left => ParagraphAlignment::Left,
                            ImageAlignment::Center => ParagraphAlignment::Center,
                            ImageAlignment::Right => ParagraphAlignment::Right,
                        });
                        paragraph.content = vec![Inline::Image(InlineImage {
                            id: image.id.clone(),
                            resource: image.resource.clone(),
                            width_mm: image.width_mm,
                            height_mm: image.height_mm,
                            alt_text: image.alt_text.clone(),
                            effects: image.effects.clone(),
                        })]
                        .into();
                        self.paragraph(
                            &paragraph,
                            &tables::paragraph_context(self.table_depth),
                            13,
                        );
                    } else if image.anchor.is_none() {
                        let mut paragraph = Paragraph::plain("");
                        paragraph.id = image.id.clone();
                        self.paragraph(
                            &paragraph,
                            &tables::paragraph_context(self.table_depth),
                            13,
                        );
                    }
                }
                Block::Equation(equation) => self.paragraph(
                    &Paragraph::plain(
                        equation
                            .alt_text
                            .as_deref()
                            .or(equation.source_hint.as_deref())
                            .unwrap_or("[Equation]"),
                    ),
                    &[],
                    13,
                ),
                Block::Foreign(_) => {
                    self.paragraph(&Paragraph::plain("[Unsupported content]"), &[], 13)
                }
            }
        }
        Ok(())
    }
    fn note_story(
        &mut self,
        kind: NoteKind,
        references: &[(u32, NodeId)],
    ) -> FormatResult<(u32, Vec<u32>)> {
        let start = self.cp();
        let mut boundaries = vec![0];
        for (_, id) in references {
            let Some(note) = self
                .document
                .review
                .footnotes
                .get(id)
                .filter(|note| note.kind == kind)
            else {
                continue;
            };
            self.note_prefix = true;
            self.blocks(&note.blocks, 159.2)?;
            if self.note_prefix {
                self.paragraph(&Paragraph::plain(""), &[], 13);
            }
            boundaries.push(self.cp() - start);
        }
        if boundaries.len() > 1 {
            self.paragraph(&Paragraph::plain(""), &[], 13);
            boundaries.push(self.cp() - start);
        }
        Ok((self.cp() - start, boundaries))
    }
    fn comment_story(&mut self, references: &[(u32, NodeId)]) -> (u32, Vec<u32>) {
        let start = self.cp();
        let mut boundaries = vec![0];
        for (_, id) in references {
            let comment = &self.document.review.comments[id];
            for line in comment.text.split('\n') {
                self.paragraph(&Paragraph::plain(line), &[], 13);
            }
            boundaries.push(self.cp() - start);
        }
        if boundaries.len() > 1 {
            self.paragraph(&Paragraph::plain(""), &[], 13);
            boundaries.push(self.cp() - start);
        }
        (self.cp() - start, boundaries)
    }
}

pub(super) fn export(document: &Document, output: &mut dyn WriteSeek) -> FormatResult<()> {
    if document.sections.iter().any(|section| {
        section.properties.columns.len() > 45 || section.properties.column_count > 45
    }) {
        return Err(unsupported(
            "Native DOC sections support at most 45 text columns",
        ));
    }
    if document.lists.len() > i16::MAX as usize
        || document.lists.values().any(|list| {
            list.levels.len() > 9
                || list
                    .levels
                    .iter()
                    .any(|level| level.level > 8 || level.text.encode_utf16().count() > 255)
        })
    {
        return Err(unsupported(
            "List definitions exceed the native DOC numbering limits",
        ));
    }
    let mut story = Story::new(document)?;
    let mut sections = Vec::new();
    for (index, section) in document.sections.iter().enumerate() {
        let start = story.cp();
        story.blocks(
            &section.blocks,
            (section.properties.page_width_mm
                - section.properties.margin_left_mm
                - section.properties.margin_right_mm)
                .max(1.0),
        )?;
        if story.cp() == start || !matches!(story.text.last(), Some(13 | 12 | 14)) {
            story.paragraph(&Paragraph::plain(""), &[], 13);
        }
        if index + 1 < document.sections.len() {
            if let Some(last) = story.text.last_mut() {
                *last = 12;
            }
        }
        sections.push((story.cp(), section.properties.clone()));
    }
    if story.text.is_empty() {
        story.paragraph(&Paragraph::plain(""), &[], 13);
        sections.push((story.cp(), SectionProperties::default()));
    }
    if let Some(error) = story.error.take() {
        return Err(error);
    }
    let unanchored = document
        .review
        .comments
        .values()
        .filter(|comment| comment.range.is_none())
        .collect::<Vec<_>>();
    if !unanchored.is_empty() {
        let start = story.cp();
        for comment in unanchored {
            story
                .comment_references
                .push((story.cp(), comment.id.clone()));
            story.control(5, &CharacterProperties::default(), true);
        }
        story.control(13, &CharacterProperties::default(), false);
        story.paragraphs.push(Run {
            start,
            end: story.cp(),
            properties: vec![0, 0],
        });
        if let Some((end, _)) = sections.last_mut() {
            *end = story.cp();
        }
    }
    let body_cp = story.cp();
    for comment in document
        .review
        .comments
        .values()
        .filter(|comment| comment.range.is_some())
    {
        if !story
            .comment_references
            .iter()
            .any(|(_, id)| *id == comment.id)
        {
            return Err(unsupported(
                "A DOC comment anchor is outside the document body or missing",
            ));
        }
    }
    for bookmark in document.review.bookmarks.values() {
        if let Some(range) = &bookmark.range {
            if super::annotations::cp(&story.anchors, &range.start).is_none_or(|cp| cp > body_cp)
                || super::annotations::cp(&story.anchors, &range.end).is_none_or(|cp| cp > body_cp)
            {
                return Err(unsupported(
                    "A DOC bookmark anchor is outside the document body or missing",
                ));
            }
        }
    }
    let note_references = story.note_references.clone();
    let comment_references = story.comment_references.clone();
    let (footnote_cp, footnote_cps) = story.note_story(NoteKind::Footnote, &note_references)?;
    let header_start = story.cp();
    let mut header_cps = vec![0u32; 7];
    for section in &document.sections {
        for stream in [
            section.headers.even.as_ref(),
            section.headers.default.as_ref(),
            section.footers.even.as_ref(),
            section.footers.default.as_ref(),
            section.headers.first.as_ref(),
            section.footers.first.as_ref(),
        ] {
            if let Some(stream) = stream {
                let start = story.cp();
                story.blocks(
                    &stream.blocks,
                    (section.properties.page_width_mm
                        - section.properties.margin_left_mm
                        - section.properties.margin_right_mm)
                        .max(1.0),
                )?;
                if story.cp() == start || story.text.last() != Some(&13) {
                    story.paragraph(&Paragraph::plain(""), &[], 13);
                }
            }
            header_cps.push(story.cp() - header_start);
        }
    }
    let header_cp = story.cp() - header_start;
    header_cps.push(header_cp);
    let comment_start = story.cp();
    let (comment_cp, comment_cps) = story.comment_story(&comment_references);
    let endnote_start = story.cp();
    let (endnote_cp, endnote_cps) = story.note_story(NoteKind::Endnote, &note_references)?;
    if let Some(error) = story.error.take() {
        return Err(error);
    }
    let counts = StoryCounts {
        body: body_cp,
        footnote: footnote_cp,
        header: header_cp,
        comment: comment_cp,
        endnote: endnote_cp,
    };
    if story.text.len() > crate::MAX_STORY_CHARACTERS as usize {
        return Err(unsupported("Native DOC export exceeds the character limit"));
    }
    let mut word_stream = vec![0; TEXT_START];
    for unit in &story.text {
        word(&mut word_stream, *unit);
    }
    let text_end = word_stream.len() as u32;
    let mut table = vec![0; 4];
    let mut pairs = BTreeMap::new();
    for (index, bytes) in super::annotations::bookmarks(document, &story.anchors, body_cp) {
        part(&mut table, &mut pairs, index, &bytes);
    }
    for (kind, cps) in [
        (NoteKind::Footnote, &footnote_cps),
        (NoteKind::Endnote, &endnote_cps),
    ] {
        for (index, bytes) in
            super::annotations::note_tables(&note_references, cps, kind, document, body_cp + 1)
        {
            part(&mut table, &mut pairs, index, &bytes);
        }
    }
    for (index, bytes) in super::annotations::comments(
        document,
        &story.anchors,
        &comment_references,
        &comment_cps,
        body_cp + 1,
    ) {
        part(&mut table, &mut pairs, index, &bytes);
    }
    let clx = piece_table(story.cp());
    part(&mut table, &mut pairs, 33, &clx);
    if !document.lists.is_empty() {
        let (definitions, fixed_len, overrides) = story.lists.tables(document);
        let offset = table.len() as u32;
        table.extend(definitions);
        pairs.insert(73, (offset, fixed_len as u32));
        part(&mut table, &mut pairs, 74, &overrides);
    }
    if story.emitted_drawings.len() != story.floating_images.len() {
        return Err(unsupported(
            "A floating picture anchor paragraph is missing",
        ));
    }
    for (index, bytes) in story
        .drawings
        .tables(body_cp, header_start, comment_start)?
    {
        part(&mut table, &mut pairs, index, &bytes);
    }
    let style_ids = story.styles.clone();
    let stylesheet = style_ids.encode(document, |properties| story.font(properties))?;
    part(&mut table, &mut pairs, 1, &stylesheet);
    part(&mut table, &mut pairs, 15, &fonts(&story.fonts)?);
    let character_bins = fkp::pages(&mut word_stream, &story.characters, false, &mut story.data)?;
    part(&mut table, &mut pairs, 12, &character_bins);
    let paragraph_bins = fkp::pages(&mut word_stream, &story.paragraphs, true, &mut story.data)?;
    part(&mut table, &mut pairs, 13, &paragraph_bins);
    let mut section_table = Vec::new();
    long(&mut section_table, 0);
    for (cp, _) in &sections {
        long(&mut section_table, *cp);
    }
    for (_, properties) in &sections {
        let bytes = section(properties);
        let offset = word_stream.len() as u32;
        word(&mut word_stream, bytes.len() as u16);
        word_stream.extend(bytes);
        word(&mut section_table, 0);
        long(&mut section_table, offset);
        word(&mut section_table, 0);
        long(&mut section_table, u32::MAX);
    }
    part(&mut table, &mut pairs, 6, &section_table);
    if header_cp > 0 {
        let mut bytes = Vec::new();
        for cp in header_cps {
            long(&mut bytes, cp);
        }
        part(&mut table, &mut pairs, 11, &bytes);
    }
    for (index, start, end) in [
        (16, 0, body_cp),
        (18, body_cp, header_start),
        (17, header_start, comment_start),
        (19, comment_start, endnote_start),
        (48, endnote_start, story.cp()),
    ] {
        let fields = story
            .fields
            .iter()
            .filter(|(cp, _)| *cp >= start && *cp < end)
            .collect::<Vec<_>>();
        if fields.is_empty() {
            continue;
        }
        let mut bytes = Vec::new();
        for (cp, _) in &fields {
            long(&mut bytes, *cp - start);
        }
        long(&mut bytes, end - start);
        for (_, fld) in fields {
            bytes.extend_from_slice(fld);
        }
        part(&mut table, &mut pairs, index, &bytes);
    }
    let mut dop = vec![0u8; 500];
    if document
        .sections
        .iter()
        .any(|s| s.properties.different_odd_even)
    {
        dop[0] |= 1;
    }
    put16(&mut dop, 10, 720);
    part(&mut table, &mut pairs, 31, &dop);
    fib(&mut word_stream, text_end, counts, &pairs);
    let mut compound =
        cfb::CompoundFile::create_with_version(cfb::Version::V3, Cursor::new(Vec::new()))?;
    compound.set_storage_clsid(
        "/",
        WORD_DOCUMENT_CLASS_ID
            .parse()
            .expect("valid constant Word document class ID"),
    )?;
    for (name, data) in [
        ("/WordDocument", word_stream),
        ("/1Table", table),
        ("/Data", story.data),
    ] {
        compound.create_stream(name)?.write_all(&data)?;
    }
    let bytes = compound.into_inner().into_inner();
    output.write_all(&bytes)?;
    Ok(())
}
fn part(table: &mut Vec<u8>, pairs: &mut BTreeMap<usize, (u32, u32)>, index: usize, bytes: &[u8]) {
    pairs.insert(index, (table.len() as u32, bytes.len() as u32));
    table.extend_from_slice(bytes);
}
fn piece_table(cp: u32) -> Vec<u8> {
    let mut out = vec![2];
    long(&mut out, 16);
    long(&mut out, 0);
    long(&mut out, cp);
    word(&mut out, 0);
    long(&mut out, TEXT_START as u32);
    word(&mut out, 0);
    out
}
fn put16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}
fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
struct StoryCounts {
    body: u32,
    footnote: u32,
    header: u32,
    comment: u32,
    endnote: u32,
}
fn fib(bytes: &mut [u8], text_end: u32, counts: StoryCounts, pairs: &BTreeMap<usize, (u32, u32)>) {
    put16(bytes, 0, 0xA5EC);
    put16(bytes, 2, 0x00C1);
    put16(bytes, 6, 0x0409);
    put16(bytes, 10, 0x1200);
    put16(bytes, 12, 0x00BF);
    put32(bytes, 24, TEXT_START as u32);
    put32(bytes, 28, text_end);
    put16(bytes, 32, 14);
    put16(bytes, 62, 22);
    put32(bytes, 64, bytes.len() as u32);
    put32(bytes, 76, counts.body);
    put32(bytes, 80, counts.footnote);
    put32(bytes, 84, counts.header);
    put32(bytes, 92, counts.comment);
    put32(bytes, 96, counts.endnote);
    put16(bytes, 152, FIB_PAIR_COUNT);
    for (index, (offset, length)) in pairs {
        put32(bytes, FIB_PAIR_OFFSET + index * 8, *offset);
        put32(bytes, FIB_PAIR_OFFSET + index * 8 + 4, *length);
    }
    put16(bytes, 898, 0);
}
fn fonts(names: &[String]) -> FormatResult<Vec<u8>> {
    if names.len() > 0x7FF0 {
        return Err(unsupported("Native DOC export contains too many fonts"));
    }
    let mut out = Vec::new();
    word(&mut out, names.len() as u16);
    word(&mut out, 0);
    for name in names {
        let mut data = vec![0u8; 39];
        data[0] = 0x16;
        data[1..3].copy_from_slice(&400u16.to_le_bytes());
        for unit in name.encode_utf16() {
            word(&mut data, unit);
        }
        word(&mut data, 0);
        if data.len() > 255 {
            return Err(unsupported("A font name is too long for native DOC export"));
        }
        out.push(data.len() as u8);
        out.extend(data);
    }
    Ok(out)
}
fn unsupported(message: &str) -> FormatError {
    FormatError::Unsupported {
        format: "DOC",
        feature: message.into(),
    }
}
