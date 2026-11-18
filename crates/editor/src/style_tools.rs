use super::*;
use model::{NamedListStyle, STYLE_NORMAL, Style, StyleKind, StyleSheet, TableStyle};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyleMatch {
    pub node: NodeId,
    pub range: Range<usize>,
    pub preview: String,
}

fn paragraph_style(paragraph: &Paragraph) -> StyleId {
    paragraph
        .style
        .clone()
        .unwrap_or_else(|| StyleId::from(STYLE_NORMAL))
}

fn character_ranges(
    items: &[Inline],
    style: &StyleId,
    offset: &mut usize,
    ranges: &mut Vec<Range<usize>>,
) {
    for item in items {
        let len = match item {
            Inline::Text(run) => {
                if run.style.as_ref() == Some(style) && !run.text.is_empty() {
                    let end = *offset + run.text.len();
                    if let Some(last) = ranges.last_mut().filter(|last| last.end == *offset) {
                        last.end = end;
                    } else {
                        ranges.push(*offset..end);
                    }
                }
                run.text.len()
            }
            Inline::Hyperlink(link) => {
                character_ranges(&link.content, style, offset, ranges);
                continue;
            }
            other => model::inline_flat_len(other),
        };
        *offset += len;
    }
}

impl EditorState {
    pub fn style_matches(&self, id: &StyleId) -> Vec<StyleMatch> {
        let Some(style) = self.document.styles.get(id) else {
            return Vec::new();
        };
        let mut matches = Vec::new();
        for paragraph in model::document_paragraphs(&self.document) {
            let ranges = match style.kind {
                StyleKind::Paragraph if paragraph_style(paragraph) == *id => {
                    std::iter::once(0..paragraph_flat_text(paragraph).len()).collect()
                }
                StyleKind::Character => {
                    let mut ranges = Vec::new();
                    character_ranges(&paragraph.content, id, &mut 0, &mut ranges);
                    ranges
                }
                _ => continue,
            };
            if ranges.is_empty() {
                continue;
            }
            let text = paragraph_flat_text(paragraph);
            for range in ranges {
                let preview = text
                    .get(range.clone())
                    .unwrap_or_default()
                    .chars()
                    .take(90)
                    .collect::<String>()
                    .replace(['\n', '\t'], " ");
                matches.push(StyleMatch {
                    node: paragraph.id.clone(),
                    range,
                    preview,
                });
            }
        }
        matches
    }

    pub fn replace_style_uses(
        &mut self,
        old: &StyleId,
        new: &StyleId,
        one: Option<&StyleMatch>,
    ) -> Result<usize, String> {
        let sheet = &self.document.styles;
        let kind = sheet.get(old).ok_or("Style not found")?.kind;
        if old == new || sheet.get(new).is_none_or(|style| style.kind != kind) {
            return Err("Choose a different style of the same type.".into());
        }
        let matches: Vec<_> = self
            .style_matches(old)
            .into_iter()
            .filter(|m| one.is_none_or(|wanted| wanted == m))
            .collect();
        let mut group: BTreeMap<NodeId, Vec<Range<usize>>> = BTreeMap::new();
        for item in &matches {
            if self.document.review.automatic_contents
                && self
                    .document
                    .review
                    .contents_paragraphs
                    .contains(&item.node)
            {
                return Err("Automatic contents are derived. Edit the heading or disable automatic contents before replacing inside them.".into());
            }
            group
                .entry(item.node.clone())
                .or_default()
                .push(item.range.clone());
        }
        let mut mutations = Vec::new();
        for (id, ranges) in group {
            let before = self.paragraph(&id).cloned().ok_or("Paragraph not found")?;
            let mut after = before.clone();
            match kind {
                StyleKind::Paragraph => after.style = Some(new.clone()),
                StyleKind::Character => {
                    if one.is_none() {
                        retag_runs(&mut after.content, old, new);
                    } else if let Some(range) = ranges.first() {
                        let (mut prefix, mut selected, suffix) =
                            self.split_for_formatting(&after, range.start, range.end)?;
                        retag_runs(&mut selected, old, new);
                        prefix.extend(selected);
                        prefix.extend(suffix);
                        after.content = self.finish_formatting_content(prefix).into();
                    }
                }
            }
            if before != after {
                mutations.push(Mutation::ParagraphChanged { id, before, after });
            }
        }
        self.commit_style_work("Replace style", mutations)?;
        Ok(matches.len())
    }

    pub(super) fn commit_style_work(
        &mut self,
        label: &'static str,
        mutations: Vec<Mutation>,
    ) -> Result<(), String> {
        if mutations.is_empty() {
            return Ok(());
        }
        let mut staged = self.clone();
        for mutation in &mutations {
            apply_mutation(&mut staged.document, mutation, true)?;
        }
        staged.commit(label, mutations, self.selection.clone(), None)?;
        staged.reset_typing_properties_from_caret();
        *self = staged;
        Ok(())
    }

    pub fn install_style_sheet(&mut self, after: StyleSheet) -> Result<(), String> {
        super::styles::validate_styles(&after)?;
        validate_extras(&after)?;
        let before = self.document.styles.clone();
        if before == after {
            return Ok(());
        }
        self.commit_style_work(
            "Change styles",
            vec![Mutation::StylesChanged { before, after }],
        )
    }

    pub fn import_style_sheet(&mut self, source: &StyleSheet) -> Result<(), String> {
        let after = merge_style_sheets(&self.document.styles, source, &self.document.lists)?;
        self.install_style_sheet(after)
    }

    pub fn link_style(&mut self, paragraph: &StyleId, character: &StyleId) -> Result<(), String> {
        let mut sheet = self.document.styles.clone();
        sheet
            .linked_styles
            .retain(|_, existing| existing != character);
        sheet
            .linked_styles
            .insert(paragraph.clone(), character.clone());
        self.install_style_sheet(sheet)
    }

    pub fn put_table_style(&mut self, id: StyleId, style: TableStyle) -> Result<(), String> {
        let mut sheet = self.document.styles.clone();
        sheet.table_styles.insert(id, style);
        self.install_style_sheet(sheet)
    }

    pub fn put_list_style(&mut self, style: NamedListStyle) -> Result<(), String> {
        if self.document.lists.contains_key(&style.definition.id) {
            return Err("This list ID is already in use.".into());
        }
        let mut sheet = self.document.styles.clone();
        sheet.list_styles.insert(style.definition.id.clone(), style);
        self.install_style_sheet(sheet)
    }

    pub fn apply_list_style(&mut self, id: &NodeId, level: u8) -> Result<(), String> {
        let list = self
            .document
            .styles
            .list_styles
            .get(id)
            .ok_or("Style not found")?;
        if !list
            .definition
            .levels
            .iter()
            .any(|item| item.level == level)
        {
            return Err("List level does not exist.".into());
        }
        let nodes = self.paragraph_formatting_spans();
        if nodes.is_empty() {
            return Err("Select text or place the caret first".into());
        }
        let mutations = nodes
            .into_iter()
            .filter_map(|(node, _, _)| {
                let before = self.paragraph(&node)?.clone();
                let mut after = before.clone();
                after.properties.list_id = Some(id.clone());
                after.properties.list_level = Some(level);
                (after != before).then_some(Mutation::ParagraphChanged {
                    id: node,
                    before,
                    after,
                })
            })
            .collect();
        self.commit_style_work("Apply list style", mutations)
    }

    pub fn inspect_styles(&self) -> Option<StyleInspection> {
        let focus = self.focus_position()?;
        let paragraph = self.paragraph(&focus.node_id)?;
        let sheet = &self.document.styles;
        let paragraph_id = paragraph_style(paragraph);
        let mut character_id = self.typing_style.clone();
        let mut direct_character = self.typing_properties.clone();
        if !self.selection.as_ref()?.is_collapsed() {
            let (node, start, end) = self
                .character_formatting_spans()
                .into_iter()
                .find(|(_, start, end)| start < end)?;
            let paragraph = self.paragraph(&node)?;
            let sample = slice_content(&paragraph.content, start, end).ok()?;
            fn first_run(items: &[Inline]) -> Option<&TextRun> {
                items.iter().find_map(|item| match item {
                    Inline::Text(run) if !run.text.is_empty() => Some(run),
                    Inline::Hyperlink(link) => first_run(&link.content),
                    _ => None,
                })
            }
            if let Some(run) = first_run(&sample) {
                character_id = run.style.clone();
                direct_character = run.properties.clone();
            } else {
                character_id = None;
                direct_character = Default::default();
            }
            return Some(inspection(
                sheet,
                paragraph_style(paragraph),
                paragraph,
                character_id,
                direct_character,
            ));
        }
        Some(inspection(
            sheet,
            paragraph_id,
            paragraph,
            character_id,
            direct_character,
        ))
    }
}

pub struct StyleInspection {
    pub paragraph_style: Option<Style>,
    pub character_style: Option<Style>,
    pub inherited_paragraph: model::ParagraphProperties,
    pub inherited_character: CharacterProperties,
    pub direct_paragraph: model::ParagraphProperties,
    pub direct_character: CharacterProperties,
}

fn inspection(
    sheet: &StyleSheet,
    paragraph_id: StyleId,
    paragraph: &Paragraph,
    character_id: Option<StyleId>,
    direct_character: CharacterProperties,
) -> StyleInspection {
    StyleInspection {
        paragraph_style: sheet.get(&paragraph_id).cloned(),
        character_style: character_id.as_ref().and_then(|id| sheet.get(id)).cloned(),
        inherited_paragraph: sheet.resolve_paragraph(Some(&paragraph_id)),
        inherited_character: sheet.resolve_run(
            Some(&paragraph_id),
            character_id.as_ref(),
            &Default::default(),
        ),
        direct_paragraph: paragraph.properties.clone(),
        direct_character,
    }
}

fn retag_runs(items: &mut [Inline], old: &StyleId, new: &StyleId) {
    for item in items {
        match item {
            Inline::Text(run) if run.style.as_ref() == Some(old) => run.style = Some(new.clone()),
            Inline::Hyperlink(link) => retag_runs(&mut link.content, old, new),
            _ => {}
        }
    }
}

pub fn validate_extras(sheet: &StyleSheet) -> Result<(), String> {
    let mut characters = BTreeSet::new();
    for (paragraph, character) in &sheet.linked_styles {
        if sheet
            .get(paragraph)
            .is_none_or(|s| s.kind != StyleKind::Paragraph)
            || sheet
                .get(character)
                .is_none_or(|s| s.kind != StyleKind::Character)
            || !characters.insert(character)
        {
            return Err(
                "Linked styles need one paragraph style and one distinct character style.".into(),
            );
        }
    }
    if sheet.table_styles.len() > 4096 || sheet.list_styles.len() > 4096 {
        return Err("A style set contains too many styles.".into());
    }
    fn check_name(name: &str, names: &mut BTreeSet<String>) -> bool {
        !name.trim().is_empty()
            && name.len() <= 256
            && !name.chars().any(char::is_control)
            && names.insert(name.trim().to_lowercase())
    }
    let mut names = BTreeSet::new();
    for table in sheet.table_styles.values() {
        if !check_name(&table.name, &mut names) {
            return Err("Table style names must be unique and nonempty.".into());
        }
        let color_valid = |value: &str| {
            value.len() == 7
                && value.starts_with('#')
                && value.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit)
        };
        if [&table.body_fill, &table.header_fill, &table.band_fill]
            .into_iter()
            .flatten()
            .any(|fill| !color_valid(fill))
        {
            return Err("Use a color in #RRGGBB format.".into());
        }
        if let Some(borders) = &table.borders {
            for border in [
                &borders.top,
                &borders.right,
                &borders.bottom,
                &borders.left,
                &borders.inside_horizontal,
                &borders.inside_vertical,
            ]
            .into_iter()
            .flatten()
            {
                if !color_valid(&border.color) || !(0.0..=24.0).contains(&border.width_pt) {
                    return Err(
                        "Use a border color in #RRGGBB format and a width of 0–24 pt.".into(),
                    );
                }
            }
        }
    }
    names.clear();
    let mut linked = BTreeSet::new();
    for (id, list) in &sheet.list_styles {
        if !check_name(&list.name, &mut names)
            || id != &list.definition.id
            || list.definition.levels.is_empty()
            || list.definition.levels.len() > 9
        {
            return Err("Invalid named list style.".into());
        }
        let mut levels = BTreeSet::new();
        for level in &list.definition.levels {
            if level.level > 8
                || !levels.insert(level.level)
                || !(1..=999_999).contains(&level.start_at)
                || ![level.left_indent_mm, level.hanging_indent_mm]
                    .iter()
                    .all(|v| (0.0..=200.0).contains(v))
                || level.text.len() > 128
                || level.text.chars().any(char::is_control)
            {
                return Err("Invalid list level, numbering pattern or indentation.".into());
            }
        }
        for (level, id) in &list.headings {
            if !levels.contains(level)
                || sheet.get(id).is_none_or(|s| s.kind != StyleKind::Paragraph)
                || !linked.insert(id)
            {
                return Err("A paragraph style can be linked to only one list level.".into());
            }
        }
    }
    Ok(())
}

pub fn style_subset(sheet: &StyleSheet, id: &StyleId) -> StyleSheet {
    let mut subset = StyleSheet::default();
    subset.styles.clear();
    let mut pending = vec![id.clone()];
    while let Some(id) = pending.pop() {
        if subset.styles.contains_key(&id) {
            continue;
        }
        let Some(style) = sheet.get(&id) else {
            continue;
        };
        subset.styles.insert(id.clone(), style.clone());
        pending.extend(
            style
                .based_on
                .iter()
                .chain(style.next_style.iter())
                .cloned(),
        );
        for (p, c) in &sheet.linked_styles {
            if *p == id || *c == id {
                pending.extend([p.clone(), c.clone()]);
                subset.linked_styles.insert(p.clone(), c.clone());
            }
        }
        let list_id = style
            .paragraph
            .list_id
            .as_ref()
            .or_else(|| sheet.linked_list(&id).map(|(list, _)| list));
        if let Some((key, list)) = list_id.and_then(|id| sheet.list_styles.get_key_value(id)) {
            let mut list = list.clone();
            list.headings.clear();
            subset.list_styles.insert(key.clone(), list);
        }
    }
    for (id, list) in &mut subset.list_styles {
        list.headings.extend(
            sheet.list_styles[id]
                .headings
                .iter()
                .filter(|(_, heading)| subset.styles.contains_key(*heading))
                .map(|(level, heading)| (*level, heading.clone())),
        );
    }
    subset
}

pub fn merge_style_sheets(
    destination: &StyleSheet,
    source: &StyleSheet,
    local_lists: &BTreeMap<NodeId, ListDefinition>,
) -> Result<StyleSheet, String> {
    super::styles::validate_styles(source)?;
    validate_extras(source)?;
    let mut merged = destination.clone();
    let mut ids = BTreeMap::new();
    let mut reserved = destination.styles.keys().cloned().collect::<BTreeSet<_>>();
    for (id, incoming) in &source.styles {
        let existing = destination
            .styles
            .values()
            .filter(|s| s.name.to_lowercase() == incoming.name.to_lowercase())
            .min_by_key(|style| (style.id.0.starts_with("builtin."), style.id != *id));
        let target = if let Some(existing) = existing {
            if existing.kind != incoming.kind {
                return Err("A style with the same name has a different type.".into());
            }
            existing.id.clone()
        } else if reserved.contains(id) {
            StyleId::new()
        } else {
            id.clone()
        };
        reserved.insert(target.clone());
        ids.insert(id.clone(), target);
    }
    let mut list_ids = BTreeMap::new();
    for (id, list) in &source.list_styles {
        let existing = destination
            .list_styles
            .iter()
            .find(|(_, s)| s.name.to_lowercase() == list.name.to_lowercase());
        let target = existing.map(|(id, _)| id.clone()).unwrap_or_else(|| {
            if merged.list_styles.contains_key(id) || local_lists.contains_key(id) {
                NodeId::new()
            } else {
                id.clone()
            }
        });
        let mut list = list.clone();
        list.definition.id = target.clone();
        for heading in list.headings.values_mut() {
            *heading = ids[heading].clone();
        }
        for other in merged.list_styles.values_mut() {
            other
                .headings
                .retain(|_, heading| !list.headings.values().any(|id| id == heading));
        }
        list_ids.insert(id.clone(), target.clone());
        merged.list_styles.insert(target, list);
    }
    for (id, source) in &source.styles {
        let mut style = source.clone();
        style.id = ids[id].clone();
        style.based_on = style
            .based_on
            .as_ref()
            .and_then(|base| ids.get(base).cloned());
        style.next_style = style
            .next_style
            .as_ref()
            .and_then(|next| ids.get(next).cloned());
        if let Some(list) = style.paragraph.list_id.as_ref() {
            style.paragraph.list_id = list_ids.get(list).cloned();
            if style.paragraph.list_id.is_none() {
                style.paragraph.list_level = None;
            }
        }
        merged.styles.insert(style.id.clone(), style);
    }
    for (paragraph, character) in &source.linked_styles {
        merged.linked_styles.retain(|_, old| old != &ids[character]);
        merged
            .linked_styles
            .insert(ids[paragraph].clone(), ids[character].clone());
    }
    for (id, style) in &source.table_styles {
        let existing = destination
            .table_styles
            .iter()
            .find(|(_, s)| s.name.to_lowercase() == style.name.to_lowercase());
        let target = existing.map(|(id, _)| id.clone()).unwrap_or_else(|| {
            if merged.table_styles.contains_key(id) {
                StyleId::new()
            } else {
                id.clone()
            }
        });
        merged.table_styles.insert(target, style.clone());
    }
    super::styles::validate_styles(&merged)?;
    validate_extras(&merged)?;
    Ok(merged)
}

#[cfg(test)]
#[path = "../tests/unit/style_tools.rs"]
mod tests;
