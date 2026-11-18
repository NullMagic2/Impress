//! Build and maintain the document's managed table of contents.
use super::*;
use crate::formatting::first_paragraph_id;
use model::{AnchorRange, Bookmark, ContentsOptions, ContentsTemplate, TabAlignment, TabStop, TextAnchor};
use std::collections::{BTreeMap, BTreeSet};

const UPDATE: &str = "Update table of contents";
const REMOVE: &str = "Remove table of contents";

struct Heading {
    node: NodeId,
    text: String,
    level: u8,
}

fn headings(document: &Document, depth: u8) -> Vec<Heading> {
    crate::search::outline(document).into_iter().filter_map(|(node, level, text)| {
        let text = text.replace(|c: char| c.is_control() || c == '\u{fffc}', " ");
        let title = text.split_whitespace().collect::<Vec<_>>().join(" ");
        (!title.is_empty() && level <= depth).then_some(Heading { node, text: title, level })
    }).collect()
}

fn contents_location(document: &Document) -> Option<(usize, usize)> {
    let managed: BTreeSet<_> = document.review.contents_paragraphs.iter().collect();
    document.sections.iter().enumerate().find_map(|(section, value)| {
        value.blocks.iter().position(|block| matches!(block, Block::Paragraph(p) if managed.contains(&p.id)))
            .map(|block| (section, block))
    })
}

fn entry_parts(paragraph: &Paragraph) -> Option<(&model::Hyperlink, &str)> {
    let Inline::Hyperlink(link) = paragraph.content.first()? else { return None; };
    Some((link, link.target.strip_prefix('#')?))
}

fn generated_bookmark(name: &str) -> bool {
    name.strip_prefix("_Impress")
        .is_some_and(|id| id.len() == 32 && id.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn indentation(level: u8, template: ContentsTemplate) -> f32 {
    f32::from(level - 1) * if template == ContentsTemplate::Compact { 3.0 } else { 4.0 }
}

fn current(editor: &EditorState, wanted: &[Heading]) -> bool {
    let review = &editor.document.review;
    let options = &review.contents_options;
    if review.contents_paragraphs.len() != wanted.len() + 1 { return false; }
    let Some(title) = review.contents_paragraphs.first().and_then(|id| editor.paragraph(id)) else { return false; };
    if model::inline_text(&title.content) != options.title { return false; }
    let bookmarks: BTreeMap<_, _> = review.bookmarks.values()
        .filter_map(|b| b.range.as_ref().map(|range| (b.name.as_str(), &range.start))).collect();
    for (heading, id) in wanted.iter().zip(review.contents_paragraphs.iter().skip(1)) {
        let Some(p) = editor.paragraph(id) else { return false; };
        let Some((link, name)) = entry_parts(p) else { return false; };
        let Some(Inline::Text(run)) = link.content.first() else { return false; };
        let leader = options.show_page_numbers.then_some(options.leader);
        if !bookmarks.get(name).is_some_and(|start| start.paragraph == heading.node && start.offset == 0)
            || run.text != heading.text
            || p.properties.left_indent_mm != Some(indentation(heading.level, options.template))
            || p.properties.contents_leader != leader
            || (if options.show_page_numbers {
                !matches!(&link.content[..], [Inline::Text(_), Inline::Tab { .. }, Inline::Field(model::Field {
                    kind: model::FieldKind::PageReference { bookmark }, ..
                })] if bookmark == name)
            } else { link.content.len() != 1 })
        {
            return false;
        }
    }
    true
}

impl EditorState {
    pub fn update_contents(&mut self, _pages: &BTreeMap<NodeId, usize>) -> Result<(), String> {
        self.update_contents_options(self.document.review.contents_options.clone())
    }

    pub fn update_contents_options(&mut self, options: ContentsOptions) -> Result<(), String> {
        if options.title.trim().is_empty() || options.title.len() > 256 || options.title.contains(['\r', '\n', '\0'])
            || !(1..=9).contains(&options.max_heading_level)
        {
            return Err("Choose a one-line contents title and heading depth from 1 to 9".into());
        }
        if self.document.review.track_text {
            return Err("Turn off paragraph-text tracking before rebuilding contents".into());
        }
        self.check_contents_revisions()?;
        let wanted = headings(&self.document, options.max_heading_level);
        let existing = contents_location(&self.document);
        if existing.is_some() && options == self.document.review.contents_options && current(self, &wanted) {
            return Ok(());
        }
        let location = match existing {
            Some(location) => location,
            None => {
                let caret = self.focus_position().ok_or("Place the caret in the main text to insert contents.")?;
                let (section, block) = self.locate_paragraph(&caret.node_id)
                    .ok_or("Place the caret in the main text to insert contents.")?;
                (section, block + usize::from(caret.byte_offset != 0))
            }
        };
        self.change_contents(UPDATE, |editor| editor.rebuild_contents(options, &wanted, location, existing.is_none()))
    }

    pub fn remove_contents(&mut self) -> Result<(), String> {
        self.check_contents_revisions()?;
        let location = contents_location(&self.document);
        self.change_contents(REMOVE, |editor| {
            editor.remove_contents_blocks();
            editor.document.review.automatic_contents = false;
            ensure_editable_document(&mut editor.document);
            editor.finish_contents_edit(location);
        })
    }

    pub fn set_contents_level(&mut self, level: u8) -> Result<(), String> {
        if level > 9 { return Err("Choose a contents level from 1 to 9, or exclude the text.".into()); }
        self.apply_paragraph_properties(&model::ParagraphProperties { outline_level: Some(level), ..Default::default() })
    }

    pub fn set_automatic_contents(&mut self, enabled: bool) -> Result<(), String> {
        let mut after = self.document.review.clone();
        after.automatic_contents = enabled;
        self.commit_review("Automatic contents settings", after)
    }

    fn check_contents_revisions(&self) -> Result<(), String> {
        let review = &self.document.review;
        if review.revisions.values().any(|r| review.contents_paragraphs.contains(&r.paragraph)) {
            return Err("Resolve pending revisions in the generated contents before rebuilding it".into());
        }
        Ok(())
    }

    fn change_contents(&mut self, label: &'static str, edit: impl FnOnce(&mut Self)) -> Result<(), String> {
        let mut staged = self.clone();
        let mut changes = Vec::new();
        staged.record_contents_changes(&mut changes, edit);
        staged.commit(label, changes, self.selection.clone(), None)?;
        *self = staged;
        Ok(())
    }

    fn record_contents_changes(&mut self, changes: &mut Vec<Mutation>, edit: impl FnOnce(&mut Self)) {
        let before = self.document.sections.clone();
        let review_before = self.document.review.clone();
        edit(self);
        if before != self.document.sections {
            changes.push(Mutation::SectionsReplaced { index: 0, before, after: self.document.sections.clone() });
        }
        if review_before != self.document.review {
            changes.push(Mutation::ReviewChanged { before: review_before, after: self.document.review.clone() });
        }
    }

    fn remove_contents_blocks(&mut self) {
        let managed: BTreeSet<_> = std::mem::take(&mut self.document.review.contents_paragraphs).into_iter().collect();
        for section in &mut self.document.sections {
            section.blocks.retain(|b| !matches!(b, Block::Paragraph(p) if managed.contains(&p.id)));
        }
    }

    fn rebuild_contents(&mut self, options: ContentsOptions, wanted: &[Heading], location: (usize, usize), insert: bool) {
        let review = &self.document.review;
        let old: Vec<_> = review.contents_paragraphs.iter().filter_map(|id| self.paragraph(id)).cloned().collect();
        let old_title = review.contents_paragraphs.first().cloned();
        let anchors: BTreeMap<_, _> = review.bookmarks.values()
            .filter_map(|bookmark| bookmark.range.as_ref().map(|range| (bookmark.name.as_str(), &range.start.paragraph)))
            .collect();
        let mut existing: BTreeMap<_, _> = old.iter().filter_map(|p| {
            let (_, name) = entry_parts(p)?;
            Some(((*anchors.get(name)?).clone(), p.id.clone()))
        }).collect();
        let mut targets: BTreeMap<_, _> = review.bookmarks.values().filter_map(|bookmark| {
            let range = bookmark.range.as_ref()?;
            generated_bookmark(&bookmark.name).then(|| (range.start.paragraph.clone(), bookmark.id.clone()))
        }).collect();

        self.remove_contents_blocks();
        let (section, block) = location;
        let width = self.document.sections[section].properties.column_bands().first().map_or(10.0, |column| column.width_mm);
        let review = &mut self.document.review;
        review.contents_options = options.clone();
        if insert { review.automatic_contents = true; }

        let (title_size, title_color, spacing) = match options.template {
            ContentsTemplate::Classic => (16.0, None, 3.0),
            ContentsTemplate::Compact => (12.0, None, 0.0),
            ContentsTemplate::Modern => (20.0, Some("#27536B".to_owned()), 5.0),
        };
        let mut title = Paragraph::plain(&options.title);
        if let Some(id) = old_title { title.id = id; }
        title.properties.keep_with_next = Some(!wanted.is_empty());
        title.properties.space_after_pt = Some(if options.template == ContentsTemplate::Compact { 3.0 } else { 8.0 });
        if let Some(Inline::Text(run)) = title.content.first_mut() {
            run.properties.bold = Some(true);
            run.properties.font_size_pt = Some(title_size);
            run.properties.color = title_color;
        }
        let mut contents = Vec::with_capacity(wanted.len() + 1);
        review.contents_paragraphs.push(title.id.clone());
        contents.push(Block::Paragraph(title));
        for heading in wanted {
            let id = targets.entry(heading.node.clone()).or_insert_with(|| {
                let id = NodeId::new();
                let name = format!("_Impress{}", id.0.replace('-', ""));
                review.bookmarks.insert(id.clone(), Bookmark { id: id.clone(), name, range: None });
                id
            });
            let bookmark = review.bookmarks.get_mut(id).expect("contents bookmark");
            let start = TextAnchor { paragraph: heading.node.clone(), offset: 0 };
            bookmark.range = Some(AnchorRange { start: start.clone(), end: start });
            let name = &bookmark.name;
            let mut paragraph = Paragraph::plain("");
            if let Some(id) = existing.remove(&heading.node) { paragraph.id = id; }
            paragraph.properties.left_indent_mm = Some(indentation(heading.level, options.template));
            paragraph.properties.space_after_pt = Some(spacing);
            paragraph.properties.outline_level = Some(0);
            let mut run = TextRun::plain(&heading.text);
            if options.template == ContentsTemplate::Modern && heading.level == 1 {
                run.properties.bold = Some(true);
            }
            let mut link_content = vec![Inline::Text(run)];
            if options.show_page_numbers {
                paragraph.properties.contents_leader = Some(options.leader);
                paragraph.properties.tabs = Some(vec![TabStop { position_mm: width, alignment: TabAlignment::Right, leader: options.leader }]);
                link_content.push(Inline::Tab { id: NodeId::new() });
                link_content.push(Inline::Field(model::Field { id: NodeId::new(),
                    kind: model::FieldKind::PageReference { bookmark: name.clone() }, display_text: None, color: None }));
            }
            paragraph.content = vec![Inline::Hyperlink(model::Hyperlink {
                id: NodeId::new(), target: format!("#{name}"), content: link_content.into(),
            })].into();
            review.contents_paragraphs.push(paragraph.id.clone());
            contents.push(Block::Paragraph(paragraph));
        }
        self.document.sections[section].blocks.splice(block..block, contents);
        let pairs = old.into_iter().filter_map(|before| self.paragraph(&before.id).cloned().map(|after| (before, after))).collect::<Vec<_>>();
        super::review::rebase_ranges(&mut self.document.review, &pairs);
        self.finish_contents_edit(Some(location));
        if insert {
            let id = self.document.review.contents_paragraphs[0].clone();
            self.set_caret(id, 0);
        }
    }

    fn finish_contents_edit(&mut self, location: Option<(usize, usize)>) {
        let live: BTreeSet<_> = model::document_paragraphs(&self.document).into_iter()
            .map(|paragraph| paragraph.id.clone()).collect();
        let review = &mut self.document.review;
        for range in review.bookmarks.values_mut().map(|b| &mut b.range)
            .chain(review.comments.values_mut().map(|c| &mut c.range))
        {
            if range.as_ref().is_some_and(|range| !live.contains(&range.start.paragraph)
                || !live.contains(&range.end.paragraph))
            {
                *range = None;
            }
        }
        if let Some(selection) = self.selection.clone() {
            if self.paragraph(&selection.anchor.node_id).is_some() && self.paragraph(&selection.focus.node_id).is_some() {
                self.selection = Some(Selection {
                    anchor: DocumentPosition::new(selection.anchor.node_id.clone(), self.clamp_offset(&selection.anchor.node_id, selection.anchor.byte_offset)),
                    focus: DocumentPosition::new(selection.focus.node_id.clone(), self.clamp_offset(&selection.focus.node_id, selection.focus.byte_offset)),
                });
                return;
            }
        }
        let target = location.and_then(|(section, block)| self.document.sections[section].blocks.get(block..).and_then(first_paragraph_id))
            .or_else(|| first_paragraph_position(&self.document).map(|position| position.node_id));
        if let Some(target) = target { self.set_caret(target, 0); }
    }

    pub(super) fn synchronize_contents_transaction(&mut self, label: &str, changes: &mut Vec<Mutation>) -> Result<(), String> {
        let review = &self.document.review;
        if !review.automatic_contents || review.contents_paragraphs.is_empty()
            || matches!(label, UPDATE | REMOVE) || label.starts_with("Native history")
            || self.check_contents_revisions().is_err()
        {
            return Ok(());
        }
        let wanted = headings(&self.document, review.contents_options.max_heading_level);
        if current(self, &wanted) { return Ok(()); }
        if let Some(location) = contents_location(&self.document) {
            let options = review.contents_options.clone();
            self.record_contents_changes(changes, |editor| editor.rebuild_contents(options, &wanted, location, false));
        } else {
            self.record_contents_changes(changes, |editor| {
                editor.document.review.contents_paragraphs.clear();
                editor.document.review.automatic_contents = false;
            });
        }
        Ok(())
    }
}
