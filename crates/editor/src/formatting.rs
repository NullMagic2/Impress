//! Ribbon formatting stays in the semantic editor, not in wxWidgets callbacks.
use super::*;
use model::{
    Border, BorderSet, BorderStyle, CellVerticalAlignment,
    InlineImage, KerningMode, LigatureMode, ListDefinition, ListFormat, ListLevel, ParagraphAlignment,
    ParagraphDirection, ParagraphProperties, Resource, ResourceId, Section, SectionProperties, SectionStart, Table,
    TableCell, TableProperties, TableRow, TabStop,
};

/// None means that a property is mixed across the selected text/paragraphs.
/// At a collapsed caret, the character properties include pending typing format.
#[derive(Clone, Debug, PartialEq)]
pub struct FormattingSummary {
    pub character: CharacterProperties,
    pub ligatures: Option<LigatureMode>,
    pub kerning_mode: Option<KerningMode>,
    pub alignment: Option<ParagraphAlignment>,
    pub direction: Option<ParagraphDirection>,
    pub style: Option<StyleId>,
    pub line_spacing: Option<f32>,
    pub space_before_pt: Option<f32>,
    pub space_after_pt: Option<f32>,
    /// Explicit paragraph tab stops. None means mixed across the current selection.
    pub tabs: Option<Vec<TabStop>>,
}

impl EditorState {
    fn cell_formatting_spans(&self, table: &NodeId, cells: &[NodeId]) -> Vec<(NodeId, usize, usize)> {
        let ids: std::collections::HashSet<_> = cells.iter().collect();
        let mut paragraphs = Vec::new();
        if let Some(table) = self.table_by_id(table) {
            for cell in table.rows.iter().flat_map(|row| &row.cells).filter(|cell| ids.contains(&cell.id)) {
                collect_paragraph_refs(&cell.blocks, &mut paragraphs);
            }
        }
        paragraphs.into_iter().map(|p| (p.id.clone(), 0, flat_content_len(&p.content))).collect()
    }

    /// Reuse the usual formatting commands with whole, possibly disjoint
    /// cells. The scope ends before any subsequent edit or typing.
    pub fn with_table_formatting(&mut self, table: &NodeId, cells: &[NodeId],
        operation: impl FnOnce(&mut Self) -> Result<(), String>,
    ) -> Result<(), String> {
        let spans = self.cell_formatting_spans(table, cells);
        if spans.is_empty() { return Ok(()); }
        let revision = self.revision;
        let previous = self.table_formatting_spans.replace(spans);
        let result = operation(self);
        self.table_formatting_spans = previous;
        if self.revision != revision { self.reset_typing_properties_from_caret(); }
        result
    }

    pub fn table_formatting_summary(&self, table: &NodeId, cells: &[NodeId]) -> FormattingSummary {
        let spans = self.cell_formatting_spans(table, cells);
        self.summarize_formatting(Some(&spans))
    }

    pub(super) fn split_for_formatting(&self, paragraph: &Paragraph, start: usize, end: usize)
        -> Result<(Vec<Inline>, Vec<Inline>, Vec<Inline>), String> {
        if self.table_formatting_spans.is_some() {
            let mut content = paragraph.content.to_vec();
            if content.is_empty() { content.push(Inline::Text(TextRun::plain(""))); }
            return Ok((Vec::new(), content, Vec::new()));
        }
        let (left, suffix) = split_content(&paragraph.content, end)?;
        let (prefix, selected) = split_content(&left, start)?;
        Ok((prefix, selected, suffix))
    }

    pub(super) fn finish_formatting_content(&self, content: Vec<Inline>) -> Vec<Inline> {
        if self.table_formatting_spans.is_some() && flat_content_len(&content) == 0 {
            content
        } else {
            normalized_content(content)
        }
    }

    pub fn apply_character_properties(&mut self, patch: &CharacterProperties) -> Result<(), String> {
        if patch == &CharacterProperties::default() { return Ok(()); }
        if patch.font_size_pt.is_some_and(|v| !v.is_finite() || !(1.0..=400.0).contains(&v)) {
            return Err("Font size must be between 1 and 400 points.".into());
        }
        if patch.font_family.as_ref().is_some_and(|v| v.trim().is_empty()) {
            return Err("Enter a font family name.".into());
        }
        if let Some(color)=&patch.highlight { if color!="none" && !(color.len()==7 && color.starts_with('#') && color[1..].bytes().all(|b|b.is_ascii_hexdigit())) {return Err("Highlight must be #RRGGBB or none".into());} }
        let selection = self.selection.clone().ok_or("no active selection")?;
        if selection.is_collapsed() && self.table_formatting_spans.is_none() {
            self.typing_properties.overlay(patch);
            self.break_coalescing();
            return Ok(());
        }
        let mut mutations = Vec::new();
        for (id, start, end) in self.character_formatting_spans() {
            if start >= end && self.table_formatting_spans.is_none() { continue; }
            let before = self.paragraph(&id).cloned().ok_or("paragraph not found")?;
            let (mut prefix, mut selected, suffix) = self.split_for_formatting(&before, start, end)?;
            if !patch_runs(&mut selected, patch) { continue; }
            prefix.extend(selected);
            prefix.extend(suffix);
            let mut after = before.clone();
            after.content = self.finish_formatting_content(prefix).into();
            self.replace_paragraph(&after)?;
            mutations.push(Mutation::ParagraphChanged { id, before, after });
        }
        self.commit("Character formatting", mutations, Some(selection), None)?;
        Ok(())
    }

    /// Change ONLY ligature intent, retaining each run's other OpenType options.
    /// Selected mixed-format runs are patched separately in one undo transaction.
    pub fn set_ligature_mode(&mut self, mode: LigatureMode) -> Result<(), String> {
        let selection = self.selection.clone().ok_or("no active selection")?;
        if selection.is_collapsed() && self.table_formatting_spans.is_none() {
            let mut props = self.effective_caret_properties();
            props.overlay(&self.typing_properties);
            let mut typography = props.typography.unwrap_or_default();
            typography.ligatures = mode;
            self.typing_properties.typography = Some(typography);
            self.break_coalescing();
            return Ok(());
        }
        let mut mutations = Vec::new();
        for (id,start,end) in self.character_formatting_spans() {
            if start >= end && self.table_formatting_spans.is_none() { continue; }
            let before = self.paragraph(&id).cloned().ok_or("paragraph not found")?;
            let (mut prefix, mut selected, suffix) = self.split_for_formatting(&before, start, end)?;
            if !patch_ligatures(&mut selected,&self.document.styles,before.style.as_ref(),mode) { continue; }
            prefix.extend(selected); prefix.extend(suffix);
            let mut after=before.clone();after.content=self.finish_formatting_content(prefix).into();
            self.replace_paragraph(&after)?;
            mutations.push(Mutation::ParagraphChanged {id,before,after});
        }
        self.commit("Ligatures",mutations,Some(selection),None)?;
        Ok(())
    }

    /// Change optical/metrics kerning without resetting the other font features.
    pub fn set_kerning_mode(&mut self, mode: KerningMode) -> Result<(), String> {
        let selection = self.selection.clone().ok_or("no active selection")?;
        if selection.is_collapsed() && self.table_formatting_spans.is_none() {
            let mut props = self.effective_caret_properties();
            props.overlay(&self.typing_properties);
            let mut typography = props.typography.unwrap_or_default();
            typography.kerning_mode = mode;
            typography.kerning = mode != KerningMode::Off;
            self.typing_properties.typography = Some(typography);
            self.break_coalescing();
            return Ok(());
        }
        let mut mutations = Vec::new();
        for (id,start,end) in self.character_formatting_spans() {
            if start >= end && self.table_formatting_spans.is_none() { continue; }
            let before = self.paragraph(&id).cloned().ok_or("paragraph not found")?;
            let (mut prefix, mut selected, suffix) = self.split_for_formatting(&before, start, end)?;
            if !patch_kerning(&mut selected,&self.document.styles,before.style.as_ref(),mode) { continue; }
            prefix.extend(selected); prefix.extend(suffix);
            let mut after=before.clone();after.content=self.finish_formatting_content(prefix).into();
            self.replace_paragraph(&after)?;
            mutations.push(Mutation::ParagraphChanged {id,before,after});
        }
        self.commit("Kerning",mutations,Some(selection),None)?;
        Ok(())
    }

    pub fn apply_paragraph_properties(&mut self, patch: &ParagraphProperties) -> Result<(), String> {
        for v in [patch.space_before_pt, patch.space_after_pt, patch.line_spacing,
            patch.left_indent_mm, patch.right_indent_mm, patch.first_line_indent_mm].into_iter().flatten() {
            if !v.is_finite() { return Err("Paragraph measurements must be finite numbers.".into()); }
        }
        if patch.line_spacing.is_some_and(|v| !(0.5..=10.0).contains(&v)) {
            return Err("Line spacing must be between 0.5 and 10.".into());
        }
        let before_selection = self.selection.clone();
        let mut mutations = Vec::new();
        for id in self.selected_paragraph_ids() {
            let before = self.paragraph(&id).cloned().ok_or("paragraph not found")?;
            let mut after = before.clone();
            after.properties.overlay(patch);
            if after == before { continue; }
            self.replace_paragraph(&after)?;
            mutations.push(Mutation::ParagraphChanged { id, before, after });
        }
        self.commit("Paragraph formatting", mutations, before_selection, None)?;
        Ok(())
    }

    /// Adjust each selected paragraph's effective indent, rather than setting all
    /// paragraphs to the first one's indent. This is one granular undo record.
    pub fn adjust_left_indent(&mut self, delta_mm: f32) -> Result<(), String> {
        if !delta_mm.is_finite() { return Err("Invalid indent.".into()); }
        let before_selection = self.selection.clone();
        let mut mutations = Vec::new();
        for id in self.selected_paragraph_ids() {
            let before = self.paragraph(&id).cloned().ok_or("paragraph not found")?;
            let si = self.section_index_for_paragraph(&id).ok_or("section not found")?;
            let section = &self.document.sections[si].properties;
            let properties = self.document.styles.resolve_paragraph_with_direct(before.style.as_ref(), &before.properties);
            let maximum = (section.page_width_mm - section.margin_left_mm - section.margin_right_mm
                - properties.right_indent_mm.unwrap_or(0.0) - 10.0).max(0.0);
            let target = (properties.left_indent_mm.unwrap_or(0.0) + delta_mm).clamp(0.0, maximum);
            let mut after = before.clone();
            after.properties.left_indent_mm = Some(target);
            if after == before { continue; }
            self.replace_paragraph(&after)?;
            mutations.push(Mutation::ParagraphChanged { id, before, after });
        }
        self.commit("Paragraph indent", mutations, before_selection, None)?;
        Ok(())
    }

    /// Indent a mixed selection in one transaction. Ordinary paragraphs move
    /// together; semantic lists change level without altering list definitions.
    pub fn adjust_indent(&mut self, delta_mm: f32) -> Result<(), String> {
        if !delta_mm.is_finite() { return Err("Invalid indent.".into()); }
        let before_selection = self.selection.clone();
        let mut entries = Vec::new();
        let mut distance = delta_mm;
        let mut level_delta: i16 = if delta_mm > 0.0 { 1 } else if delta_mm < 0.0 { -1 } else { 0 };
        for id in self.selected_paragraph_ids() {
            let before = self.paragraph(&id).cloned().ok_or("paragraph not found")?;
            let properties = self.document.styles.resolve_paragraph_with_direct(before.style.as_ref(), &before.properties);
            if properties.list_id.is_some() {
                let level = i16::from(properties.list_level.unwrap_or(0).min(8));
                level_delta = level_delta.clamp(-level, 8 - level);
            } else {
                let si = self.section_index_for_paragraph(&id).ok_or("section not found")?;
                let section = &self.document.sections[si].properties;
                let current = properties.left_indent_mm.unwrap_or(0.0);
                let maximum = (section.text_width_mm() - properties.right_indent_mm.unwrap_or(0.0) - 10.0).max(current).max(0.0);
                distance = distance.clamp(-current.max(0.0), maximum - current);
            }
            entries.push((id, before, properties));
        }
        let mut mutations = Vec::new();
        for (id, before, properties) in entries {
            let mut after = before.clone();
            if let Some(list) = properties.list_id {
                if level_delta == 0 { continue; }
                let current_level = properties.list_level.unwrap_or(0).min(8);
                let definition = self.document.styles.list_definition(&self.document.lists, &list);
                let level = definition.and_then(|d| d.levels.iter().find(|l| l.level == current_level));
                let left = properties.left_indent_mm.unwrap_or_else(|| level.map_or(6.35, |l| l.left_indent_mm));
                after.properties.left_indent_mm = Some((left + delta_mm.abs() * f32::from(level_delta)).max(0.0));
                if properties.first_line_indent_mm.is_none() {
                    after.properties.first_line_indent_mm = Some(-level.map_or(3.18, |l| l.hanging_indent_mm));
                }
                after.properties.list_id = Some(list);
                after.properties.list_level = Some((i16::from(current_level) + level_delta) as u8);
            } else {
                if distance.abs() < f32::EPSILON { continue; }
                after.properties.left_indent_mm = Some(properties.left_indent_mm.unwrap_or(0.0) + distance);
            }
            self.replace_paragraph(&after)?;
            mutations.push(Mutation::ParagraphChanged { id, before, after });
        }
        self.commit("Paragraph indent", mutations, before_selection, None)?;
        Ok(())
    }

    /// Effective formatting of the focused paragraph, including its style.
    pub fn paragraph_properties_at_caret(&self) -> Option<ParagraphProperties> {
        let position = self.focus_position()?;
        let paragraph = self.paragraph(&position.node_id)?;
        let mut properties = self.document.styles.resolve_paragraph(paragraph.style.as_ref());
        properties.overlay(&paragraph.properties);
        Some(properties)
    }

    pub fn section_properties_at_caret(&self) -> Option<SectionProperties> {
        let position = self.focus_position()?;
        let si = self.section_index_for_paragraph(&position.node_id)?;
        Some(self.document.sections[si].properties.clone())
    }

    /// Changes only the active section and participates in the same undo history.
    pub fn set_section_properties(&mut self, after: SectionProperties) -> Result<(), String> {
        let dimensions = [after.page_width_mm, after.page_height_mm, after.margin_left_mm,
            after.margin_right_mm, after.margin_top_mm, after.margin_bottom_mm];
        if dimensions.iter().any(|v| !v.is_finite() || *v < 0.0)
            || after.page_width_mm < 50.0 || after.page_height_mm < 50.0
            || after.page_width_mm - after.margin_left_mm - after.margin_right_mm < 10.0
            || after.page_height_mm - after.margin_top_mm - after.margin_bottom_mm < 10.0 {
            return Err("Page size and margins must leave at least 10 mm of editable space.".into());
        }
        let pos = self.focus_position().ok_or("no active caret")?;
        let si = self.section_index_for_paragraph(&pos.node_id).ok_or("section not found")?;
        let section = &mut self.document.sections[si];
        let before = section.properties.clone();
        if before == after { return Ok(()); }
        section.properties = after.clone();
        let mutation = Mutation::SectionPropertiesChanged { section_id: section.id.clone(), before, after };
        self.commit("Page setup", vec![mutation], self.selection.clone(), None)?;
        Ok(())
    }

    /// Puts a display equation on its own line at the caret: before the
    /// paragraph when the caret is at its start, else between the text before
    /// and after the caret. The caret goes after the equation.
    pub fn insert_display_equation(&mut self, mut equation: model::Equation) -> Result<(), String> {
        equation.display = model::EquationDisplay::Block;
        let before_selection = self.selection.clone();
        let mut mutations = Vec::new();
        self.delete_selection_into(&mut mutations)?;
        let pos = self.focus_position().ok_or("no active caret")?;
        let (si, bi) = self.locate_paragraph(&pos.node_id).ok_or("Equations on their own line can only be inserted in the main document flow.")?;
        let section_id = self.document.sections[si].id.clone();
        let paragraph = self.paragraph(&pos.node_id).cloned().ok_or("paragraph not found")?;
        let (left, right) = split_content(&paragraph.content, pos.byte_offset)?;
        let (index, caret) = if normalized_content(left.clone()).is_empty() {
            (bi, DocumentPosition::new(paragraph.id.clone(), 0))
        } else {
            let mut kept = paragraph.clone();
            kept.content = (normalized_content(left)).into();
            let next = Paragraph { id: NodeId::new(), style: paragraph.style.clone(),
                properties: paragraph.properties.clone(), content: (normalized_content(right)).into() };
            self.replace_paragraph(&kept)?;
            mutations.push(Mutation::ParagraphChanged { id: kept.id.clone(), before: paragraph, after: kept });
            let block = Block::Paragraph(next.clone());
            self.document.sections[si].blocks.insert(bi + 1, block.clone());
            mutations.push(Mutation::BlockInserted { section_id: section_id.clone(), index: bi + 1, block });
            (bi + 1, DocumentPosition::new(next.id, 0))
        };
        let block = Block::Equation(equation);
        self.document.sections[si].blocks.insert(index, block.clone());
        mutations.push(Mutation::BlockInserted { section_id, index, block });
        self.selection = Some(Selection::collapsed(caret));
        self.commit("Insert equation", mutations, before_selection, None)?;
        Ok(())
    }

    pub fn insert_page_break(&mut self) -> Result<(), String> {
        let before_selection = self.selection.clone();
        let mut mutations = Vec::new();
        self.delete_selection_into(&mut mutations)?;
        let pos = self.focus_position().ok_or("no active caret")?;
        let (si, bi) = self.locate_paragraph(&pos.node_id).ok_or("Page breaks can only be inserted in the main document flow.")?;
        let before = self.paragraph(&pos.node_id).cloned().ok_or("paragraph not found")?;
        let (left, right) = split_content(&before.content, pos.byte_offset)?;
        let mut after = before.clone();
        after.content = (normalized_content(left)).into();
        let next = Paragraph { id: NodeId::new(), style: before.style.clone(),
            properties: before.properties.clone(), content: (normalized_content(right)).into() };
        self.replace_paragraph(&after)?;
        mutations.push(Mutation::ParagraphChanged { id: after.id.clone(), before, after });
        let section_id = self.document.sections[si].id.clone();
        let blocks = [Block::PageBreak { id: NodeId::new() }, Block::Paragraph(next.clone())];
        for (offset, block) in blocks.into_iter().enumerate() {
            let index = bi + 1 + offset;
            self.document.sections[si].blocks.insert(index, block.clone());
            mutations.push(Mutation::BlockInserted { section_id: section_id.clone(), index, block });
        }
        self.selection = Some(Selection::collapsed(DocumentPosition::new(next.id, 0)));
        self.commit("Insert page break", mutations, before_selection, None)?;
        Ok(())
    }

    /// Toggle a bullet or numbered list over every selected body paragraph.
    /// A fresh semantic list definition is created when enabling; disabling
    /// clears only paragraph references so unrelated list instances are safe.
    pub fn toggle_list(&mut self, format: ListFormat) -> Result<(), String> {
        let ids = self.selected_paragraph_ids();
        if ids.is_empty() { return Err("no paragraph selected".into()); }
        let all_same = ids.iter().all(|id| {
            let Some(p) = self.paragraph(id) else { return false; };
            let Some(list_id) = p.properties.list_id.as_ref() else { return false; };
            let level = p.properties.list_level.unwrap_or(0);
            self.document.styles.list_definition(&self.document.lists, list_id)
                .and_then(|d| d.levels.iter().find(|l| l.level == level))
                .is_some_and(|l| l.format == format)
        });
        let before_selection = self.selection.clone();
        let mut mutations = Vec::new();
        let list_id = if all_same {
            None
        } else {
            let id = NodeId::new();
            let text = match format {
                ListFormat::Bullet => "•".to_string(),
                ListFormat::Decimal => "%1.".to_string(),
                ListFormat::LowerLetter => "%1)".to_string(),
                ListFormat::UpperLetter => "%1)".to_string(),
                ListFormat::LowerRoman => "%1.".to_string(),
                ListFormat::UpperRoman => "%1.".to_string(),
            };
            let definition = ListDefinition { id: id.clone(), levels: (0..9).map(|level| ListLevel {
                level, format, text: text.replace("%1", &format!("%{}", level + 1)), start_at: 1,
                left_indent_mm: 6.35 + f32::from(level) * 5.0, hanging_indent_mm: 3.18,
            }).collect() };
            self.document.lists.insert(id.clone(), definition.clone());
            mutations.push(Mutation::ListInserted { id: id.clone(), definition });
            Some(id)
        };
        for id in ids {
            let before = self.paragraph(&id).cloned().ok_or("paragraph not found")?;
            let mut after = before.clone();
            after.properties.list_id = list_id.clone();
            after.properties.list_level = list_id.as_ref().map(|_| 0);
            if before == after { continue; }
            self.replace_paragraph(&after)?;
            mutations.push(Mutation::ParagraphChanged { id, before, after });
        }
        self.commit(if format == ListFormat::Bullet { "Bullets" } else { "Numbering" }, mutations, before_selection, None)?;
        Ok(())
    }

    pub fn set_column_count(&mut self, count: u16) -> Result<(), String> {
        if !(1..=8).contains(&count) { return Err("Column count must be between 1 and 8.".into()); }
        let mut props = self.section_properties_at_caret().ok_or("No active section")?;
        props.column_count = count;
        self.set_section_properties(props)
    }

    pub fn insert_column_break(&mut self) -> Result<(), String> {
        let before_selection = self.selection.clone();
        let mut mutations = Vec::new();
        self.delete_selection_into(&mut mutations)?;
        let pos = self.focus_position().ok_or("no active caret")?;
        let (si, bi) = self.locate_paragraph(&pos.node_id).ok_or("Column breaks can only be inserted in the main document flow.")?;
        let before = self.paragraph(&pos.node_id).cloned().ok_or("paragraph not found")?;
        let (left, right) = split_content(&before.content, pos.byte_offset)?;
        let mut after = before.clone();
        after.content = (normalized_content(left)).into();
        let next = Paragraph { id: NodeId::new(), style: before.style.clone(), properties: before.properties.clone(), content: (normalized_content(right)).into() };
        self.replace_paragraph(&after)?;
        mutations.push(Mutation::ParagraphChanged { id: after.id.clone(), before, after });
        let section_id = self.document.sections[si].id.clone();
        for (offset, block) in [Block::ColumnBreak { id: NodeId::new() }, Block::Paragraph(next.clone())].into_iter().enumerate() {
            let index = bi + 1 + offset;
            self.document.sections[si].blocks.insert(index, block.clone());
            mutations.push(Mutation::BlockInserted { section_id: section_id.clone(), index, block });
        }
        self.selection = Some(Selection::collapsed(DocumentPosition::new(next.id, 0)));
        self.commit("Insert column break", mutations, before_selection, None)?;
        Ok(())
    }

    /// Split the active body section at the caret and create a new semantic
    /// section with the requested start rule. This is a single undoable edit;
    /// body blocks after the caret move into the new section without being
    /// serialized through any file-format adapter.
    pub fn insert_section_break(&mut self, start_on: SectionStart) -> Result<(), String> {
        let before_selection = self.selection.clone();
        let mut mutations = Vec::new();
        self.delete_selection_into(&mut mutations)?;
        let pos = self.focus_position().ok_or("no active caret")?;
        let (section_index, block_index) = self.locate_paragraph(&pos.node_id)
            .ok_or("Section breaks can only be inserted in the main document flow.")?;
        let section_before = self.document.sections[section_index].clone();
        let paragraph_before = match &section_before.blocks[block_index] {
            Block::Paragraph(paragraph) => paragraph.clone(),
            _ => return Err("caret paragraph not found".into()),
        };
        let (left, right) = split_content(&paragraph_before.content, pos.byte_offset)?;
        let mut current = paragraph_before.clone();
        current.content = (normalized_content(left)).into();
        let next_paragraph = Paragraph {
            id: NodeId::new(),
            style: paragraph_before.style.clone(),
            properties: paragraph_before.properties.clone(),
            content: (normalized_content(right)).into(),
        };

        let mut trailing = self.document.sections[section_index].blocks.split_off(block_index + 1);
        self.document.sections[section_index].blocks[block_index] = Block::Paragraph(current);
        let mut new_properties = self.document.sections[section_index].properties.clone();
        new_properties.start_on = start_on;
        let mut new_blocks = Vec::with_capacity(trailing.len() + 1);
        new_blocks.push(Block::Paragraph(next_paragraph.clone()));
        new_blocks.append(&mut trailing);
        let new_section = Section {
            id: NodeId::new(),
            properties: new_properties,
            blocks: (new_blocks).into(),
            headers: Default::default(),
            footers: Default::default(),
        };
        let section_after = self.document.sections[section_index].clone();
        self.document.sections.insert(section_index + 1, new_section.clone());
        mutations.push(Mutation::SectionChanged {
            section_id: section_after.id.clone(), before: section_before, after: section_after,
        });
        mutations.push(Mutation::SectionInserted { index: section_index + 1, section: new_section });
        self.selection = Some(Selection::collapsed(DocumentPosition::new(next_paragraph.id, 0)));
        self.commit("Insert section break", mutations, before_selection, None)?;
        Ok(())
    }

    pub fn set_section_start(&mut self, start_on: SectionStart) -> Result<(), String> {
        let mut properties = self.section_properties_at_caret().ok_or("No active section")?;
        properties.start_on = start_on;
        self.set_section_properties(properties)
    }

    /// Explicit pagination controls used by the Paragraph dialog/contextual UI.
    /// They stay as paragraph properties so DOCX and native formats share the
    /// same semantics as the layout engine.
    pub fn set_keep_with_next(&mut self, enabled: bool) -> Result<(), String> {
        self.apply_paragraph_properties(&ParagraphProperties { keep_with_next: Some(enabled), ..Default::default() })
    }

    pub fn set_keep_lines_together(&mut self, enabled: bool) -> Result<(), String> {
        self.apply_paragraph_properties(&ParagraphProperties { keep_lines_together: Some(enabled), ..Default::default() })
    }

    pub fn set_page_break_before(&mut self, enabled: bool) -> Result<(), String> {
        self.apply_paragraph_properties(&ParagraphProperties { page_break_before: Some(enabled), ..Default::default() })
    }

    pub fn set_widow_control(&mut self, enabled: bool) -> Result<(), String> {
        self.apply_paragraph_properties(&ParagraphProperties { widow_control: Some(enabled), ..Default::default() })
    }

    /// Insert a 2D semantic table after splitting the caret paragraph. Cell
    /// paragraphs are real document blocks, so DOCX/native formats preserve
    /// them even before full in-cell keyboard navigation is implemented.
    pub fn insert_table(&mut self, rows: usize, columns: usize) -> Result<(), String> {
        if !(1..=100).contains(&rows) || !(1..=32).contains(&columns) { return Err("Table size is out of range.".into()); }
        let before_selection = self.selection.clone();
        let mut mutations = Vec::new();
        self.delete_selection_into(&mut mutations)?;
        let pos = self.focus_position().ok_or("no active caret")?;
        let (si, bi) = self.locate_paragraph(&pos.node_id).ok_or("Nested tables are not supported in this alpha.")?;
        let before = self.paragraph(&pos.node_id).cloned().ok_or("paragraph not found")?;
        let (left, right) = split_content(&before.content, pos.byte_offset)?;
        let mut after = before.clone(); after.content = (normalized_content(left)).into();
        self.replace_paragraph(&after)?;
        mutations.push(Mutation::ParagraphChanged { id: after.id.clone(), before, after });
        let border = Border { width_pt: 0.5, color: "#808080".into(), style: BorderStyle::Single };
        let borders = BorderSet { top: Some(border.clone()), right: Some(border.clone()), bottom: Some(border.clone()), left: Some(border.clone()), inside_horizontal: Some(border.clone()), inside_vertical: Some(border) };
        let table = Table {
            id: NodeId::new(),
            properties: TableProperties {
                style: None, preferred_width_mm: None, left_indent_mm: None, alignment: None, borders: Some(borders), cell_spacing_mm: None,
                ..TableProperties::default()
            },
            rows: (0..rows).map(|row| TableRow {
                id: NodeId::new(), repeat_as_header: row == 0, allow_break_across_pages: true, height_mm: None,
                cells: (0..columns).map(|_| TableCell {
                    id: NodeId::new(), blocks: (vec![Block::Paragraph(Paragraph::plain(""))]).into(), column_span: 1, row_span: 1,
                    width_mm: None, shading: None, borders: None, vertical_alignment: CellVerticalAlignment::Top, margins: None, text_direction: None,
                }).collect(),
            }).collect(),
        };
        let first_cell_paragraph = table.rows.first()
            .and_then(|row| row.cells.first())
            .and_then(|cell| first_paragraph_id(&cell.blocks))
            .ok_or("new table has no editable cell")?;
        let template = self.paragraph(&pos.node_id).cloned().ok_or("paragraph not found")?;
        let next = Paragraph { id: NodeId::new(), style: template.style, properties: template.properties, content: (normalized_content(right)).into() };
        let section_id = self.document.sections[si].id.clone();
        for (offset, block) in [Block::Table(table), Block::Paragraph(next.clone())].into_iter().enumerate() {
            let index = bi + 1 + offset;
            self.document.sections[si].blocks.insert(index, block.clone());
            mutations.push(Mutation::BlockInserted { section_id: section_id.clone(), index, block });
        }
        // Word places the insertion point in the first table cell.  The
        // trailing paragraph remains in the main flow so Down/Tab can leave
        // the table naturally later.
        self.selection = Some(Selection::collapsed(DocumentPosition::new(first_cell_paragraph, 0)));
        self.commit("Insert table", mutations, before_selection, None)?;
        Ok(())
    }

    /// Return the semantic cell containing the caret, if any.  This is the
    /// common context used by keyboard Tab navigation and contextual table
    /// commands; it never exposes DOCX grid identifiers to the editor.
    /// The table and cell ids holding paragraph `paragraph_id` (in the body
    /// or a header or footer).
    pub fn table_cell_of(&self, paragraph_id: &NodeId) -> Option<(NodeId, NodeId)> {
        self.document.sections.iter().find_map(|section| find_table_cell_for_paragraph(&section.blocks, paragraph_id)
            .or_else(|| section.header_footers().find_map(|hf| find_table_cell_for_paragraph(&hf.stream.blocks, paragraph_id))))
            .map(|(table_id, .., cell_id)| (table_id, cell_id))
    }

    pub fn table_cell_context(&self) -> Option<TableCellContext> {
        let paragraph_id = self.focus_position()?.node_id;
        for (section_index, section) in self.document.sections.iter().enumerate() {
            if let Some((table_id, row_index, cell_index, cell_id)) =
                find_table_cell_for_paragraph(&section.blocks, &paragraph_id)
            {
                return Some(TableCellContext { section_index, table_id, row_index, cell_index, cell_id });
            }
            for hf in section.header_footers() {
                if let Some((table_id, row_index, cell_index, cell_id)) = find_table_cell_for_paragraph(&hf.stream.blocks, &paragraph_id) {
                    return Some(TableCellContext { section_index, table_id, row_index, cell_index, cell_id });
                }
            }
        }
        None
    }

    fn table_cell_paragraph_id(&self, table_id: &NodeId, row_index: usize, cell_index: usize) -> Option<NodeId> {
        self.document.sections.iter().find_map(|section| {
            let table = find_table_in_section(section, table_id)?;
            let cell = table.rows.get(row_index)?.cells.get(cell_index)?;
            first_paragraph_id(&cell.blocks)
        })
    }

    /// Tab / Shift+Tab navigation through semantic table cells.  Pressing Tab
    /// in the final cell appends a matching row, just like mainstream word
    /// processors, and the row insertion is one undoable transaction.
    pub fn move_table_cell(&mut self, forward: bool) -> Result<bool, String> {
        let Some(context) = self.table_cell_context() else { return Ok(false); };
        let before_selection = self.selection.clone();

        let target = {
            let section = &self.document.sections[context.section_index];
            let table = find_table_in_section(section, &context.table_id).ok_or("table not found")?;
            if forward {
                if context.cell_index + 1 < table.rows[context.row_index].cells.len() {
                    Some((context.row_index, context.cell_index + 1))
                } else if context.row_index + 1 < table.rows.len() {
                    Some((context.row_index + 1, 0))
                } else { None }
            } else if context.cell_index > 0 {
                Some((context.row_index, context.cell_index - 1))
            } else if context.row_index > 0 {
                let row = context.row_index - 1;
                Some((row, table.rows[row].cells.len().saturating_sub(1)))
            } else { Some((0, 0)) }
        };

        if let Some((row, cell)) = target {
            if let Some(id) = self.table_cell_paragraph_id(&context.table_id, row, cell) {
                self.set_caret(id, 0);
                return Ok(true);
            }
            return Err("target table cell has no editable paragraph".into());
        }

        // Forward Tab from the final cell creates a new physical row.
        let before = self.document.sections[context.section_index].clone();
        let target_id = {
            let table = find_table_mut_in_section(
                &mut self.document.sections[context.section_index],
                &context.table_id,
            ).ok_or("table not found")?;
            let template = table.rows.last().cloned().ok_or("table has no rows")?;
            let cells = template.cells.iter().map(|cell| blank_cell_like(cell)).collect::<Vec<_>>();
            let first = cells.first().and_then(|cell| first_paragraph_id(&cell.blocks)).ok_or("new row has no cell")?;
            table.rows.push(TableRow {
                id: NodeId::new(), cells, repeat_as_header: false,
                allow_break_across_pages: true, height_mm: template.height_mm,
            });
            first
        };
        let after = self.document.sections[context.section_index].clone();
        self.selection = Some(Selection::collapsed(DocumentPosition::new(target_id, 0)));
        self.commit("Add table row", vec![Mutation::SectionChanged {
            section_id: after.id.clone(), before, after,
        }], before_selection, None)?;
        Ok(true)
    }

    pub fn insert_table_row(&mut self, after_current: bool) -> Result<(), String> {
        let context = self.table_cell_context().ok_or("Place the caret in a table cell first.")?;
        let before_selection = self.selection.clone();
        let before = self.document.sections[context.section_index].clone();
        let fallback_target = self.focus_position().map(|position| position.node_id).ok_or("no active caret")?;
        let target_id = {
            let table = find_table_mut_in_section(&mut self.document.sections[context.section_index], &context.table_id)
                .ok_or("table not found")?;
            let placements = table_grid_placements(table);
            let column_count = table_logical_column_count(&placements).max(1);
            ensure_table_grid_widths(table, column_count);
            let template = table.rows.get(context.row_index).cloned().ok_or("table row not found")?;
            let row_span = placements[context.row_index][context.cell_index].2;
            let insert_at = (context.row_index + if after_current { row_span } else { 0 }).min(table.rows.len());

            // Inserting a physical row through an existing vertical merge must
            // extend that merge. The new row only owns semantic cells for the
            // logical columns that are not covered by a span from above.
            let mut covered = vec![false; column_count];
            let mut crossing = Vec::<(usize, usize, usize, usize)>::new();
            for origin_row in 0..insert_at {
                for (cell_index, cell) in table.rows[origin_row].cells.iter().enumerate() {
                    let (start, span, row_span) = placements[origin_row][cell_index];
                    if origin_row + row_span > insert_at {
                        crossing.push((origin_row, cell_index, start, span));
                        for column in start..(start + span).min(column_count) { covered[column] = true; }
                    }
                    let _ = cell;
                }
            }
            for (origin_row, cell_index, _, _) in &crossing {
                let cell = &mut table.rows[*origin_row].cells[*cell_index];
                cell.row_span = cell.row_span.saturating_add(1);
            }

            let mut cells = Vec::new();
            for column in 0..column_count {
                if covered[column] { continue; }
                let mut cell = blank_table_cell();
                cell.width_mm = table.properties.grid_column_widths_mm.get(column).copied();
                cells.push(cell);
            }
            let first = cells.first().and_then(|cell| first_paragraph_id(&cell.blocks));
            table.rows.insert(insert_at, TableRow {
                id: NodeId::new(), cells, repeat_as_header: false,
                allow_break_across_pages: template.allow_break_across_pages, height_mm: template.height_mm,
            });
            first.unwrap_or(fallback_target)
        };
        let after = self.document.sections[context.section_index].clone();
        self.selection = Some(Selection::collapsed(DocumentPosition::new(target_id, 0)));
        self.commit("Insert table row", vec![Mutation::SectionChanged { section_id: after.id.clone(), before, after }], before_selection, None)?;
        Ok(())
    }

    pub fn delete_table_row(&mut self) -> Result<(), String> {
        let context = self.table_cell_context().ok_or("Place the caret in a table cell first.")?;
        let before_selection = self.selection.clone();
        let before = self.document.sections[context.section_index].clone();
        let target = {
            let table = find_table_mut_in_section(&mut self.document.sections[context.section_index], &context.table_id)
                .ok_or("table not found")?;
            if table.rows.len() <= 1 { return Err("A table must contain at least one row.".into()); }
            let placements = table_grid_placements(table);
            let logical_column = placements.get(context.row_index).and_then(|row| row.get(context.cell_index)).map(|entry| entry.0).unwrap_or(0);
            let delete_index = context.row_index;

            // Spans that started above the deleted row become one row shorter.
            for origin_row in 0..delete_index {
                for cell_index in 0..table.rows[origin_row].cells.len() {
                    let (_, _, row_span) = placements[origin_row][cell_index];
                    if origin_row + row_span > delete_index {
                        let cell = &mut table.rows[origin_row].cells[cell_index];
                        cell.row_span = cell.row_span.saturating_sub(1).max(1);
                    }
                }
            }

            // A span whose origin is the deleted row must survive by moving its
            // semantic origin to the following row. Preserve its content and
            // logical start instead of silently deleting the whole merged cell.
            let mut moved = Vec::<(usize, TableCell)>::new();
            for (cell_index, cell) in table.rows[delete_index].cells.iter().enumerate() {
                let (start, _, row_span) = placements[delete_index][cell_index];
                if row_span > 1 {
                    let mut continued = cell.clone();
                    continued.row_span = continued.row_span.saturating_sub(1).max(1);
                    moved.push((start, continued));
                }
            }
            let next_entries = if delete_index + 1 < table.rows.len() {
                placements[delete_index + 1].iter().zip(table.rows[delete_index + 1].cells.iter())
                    .map(|((start, _, _), cell)| (*start, cell.clone())).collect::<Vec<_>>()
            } else { Vec::new() };

            table.rows.remove(delete_index);
            if !moved.is_empty() && delete_index < table.rows.len() {
                let mut combined = next_entries;
                combined.extend(moved);
                combined.sort_by_key(|(start, _)| *start);
                table.rows[delete_index].cells = combined.into_iter().map(|(_, cell)| cell).collect();
            }

            let row = delete_index.min(table.rows.len() - 1);
            table_paragraph_covering_grid(table, row, logical_column)
                .or_else(|| table.rows[row].cells.iter().find_map(|cell| first_paragraph_id(&cell.blocks)))
                .or_else(|| table.rows.iter().flat_map(|r| &r.cells).find_map(|cell| first_paragraph_id(&cell.blocks)))
                .ok_or("remaining table has no editable cell")?
        };
        let after = self.document.sections[context.section_index].clone();
        self.selection = Some(Selection::collapsed(DocumentPosition::new(target, 0)));
        self.commit("Delete table row", vec![Mutation::SectionChanged { section_id: after.id.clone(), before, after }], before_selection, None)?;
        Ok(())
    }

    pub fn insert_table_column(&mut self, after_current: bool) -> Result<(), String> {
        let context = self.table_cell_context().ok_or("Place the caret in a table cell first.")?;
        let before_selection = self.selection.clone();
        let before = self.document.sections[context.section_index].clone();
        let target_id = {
            let table = find_table_mut_in_section(&mut self.document.sections[context.section_index], &context.table_id)
                .ok_or("table not found")?;
            let placements = table_grid_placements(table);
            let old_count = table_logical_column_count(&placements).max(1);
            ensure_table_grid_widths(table, old_count);
            let (current_start, current_span, _) = *placements.get(context.row_index)
                .and_then(|row| row.get(context.cell_index)).ok_or("table cell grid position not found")?;
            let insert_column = if after_current { current_start + current_span } else { current_start };

            // If the insertion point lies inside a merged cell, widening that
            // logical cell is the only rectangular result. Otherwise each row
            // receives a real new semantic cell at the grid boundary.
            let mut covered_rows = vec![false; table.rows.len()];
            let mut widen = Vec::<(usize, usize)>::new();
            for row_index in 0..table.rows.len() {
                for cell_index in 0..table.rows[row_index].cells.len() {
                    let (start, span, row_span) = placements[row_index][cell_index];
                    if start < insert_column && insert_column < start + span {
                        widen.push((row_index, cell_index));
                        let end = (row_index + row_span).min(table.rows.len());
                        for covered in &mut covered_rows[row_index..end] { *covered = true; }
                    }
                }
            }
            for (row_index, cell_index) in widen {
                table.rows[row_index].cells[cell_index].column_span = table.rows[row_index].cells[cell_index].column_span.saturating_add(1);
            }

            let width_hint = inserted_grid_width(&table.properties.grid_column_widths_mm, insert_column);
            table.properties.grid_column_widths_mm.insert(insert_column.min(old_count), width_hint);
            let mut target = None;
            for row_index in 0..table.rows.len() {
                if covered_rows[row_index] { continue; }
                let insert_at = placements[row_index].iter().take_while(|(start, _, _)| *start < insert_column).count();
                let mut cell = blank_table_cell();
                cell.width_mm = Some(width_hint);
                if row_index == context.row_index { target = first_paragraph_id(&cell.blocks); }
                table.rows[row_index].cells.insert(insert_at, cell);
            }
            target.or_else(|| table_paragraph_covering_grid(table, context.row_index, insert_column))
                .ok_or("new table column has no editable cell")?
        };
        let after = self.document.sections[context.section_index].clone();
        self.selection = Some(Selection::collapsed(DocumentPosition::new(target_id, 0)));
        self.commit("Insert table column", vec![Mutation::SectionChanged { section_id: after.id.clone(), before, after }], before_selection, None)?;
        Ok(())
    }

    /// Deletes rows (or logical columns) `first..=last` of a table as one
    /// undo step, as when several are selected with the mouse.
    pub fn delete_table_band(&mut self, table_id: &NodeId, columns: bool, first: usize, last: usize) -> Result<(), String> {
        let depth = self.undo.len();
        let mut result = Ok(());
        for _ in first..=last.max(first) {
            let placements = table_grid_placements(self.table_by_id(table_id).ok_or("Table not found")?);
            let cell = if columns {
                placements.iter().enumerate().find_map(|(row, cells)| cells.iter().position(|p| p.0 == first).map(|c| (row, c)))
            } else {
                placements.get(first).filter(|cells| !cells.is_empty()).map(|_| (first, 0))
            };
            let Some((row, cell)) = cell else { result = Err("That part of the table cannot be deleted.".to_string()); break; };
            let step = self.select_cell(table_id, row, cell)
                .and_then(|_| if columns { self.delete_table_column() } else { self.delete_table_row() });
            if let Err(error) = step { result = Err(error); break; }
        }
        self.group_undo_since(depth, if columns { "Delete table columns" } else { "Delete table rows" });
        result
    }

    pub fn delete_table_column(&mut self) -> Result<(), String> {
        let context = self.table_cell_context().ok_or("Place the caret in a table cell first.")?;
        let before_selection = self.selection.clone();
        let before = self.document.sections[context.section_index].clone();
        let target = {
            let table = find_table_mut_in_section(&mut self.document.sections[context.section_index], &context.table_id)
                .ok_or("table not found")?;
            let placements = table_grid_placements(table);
            let column_count = table_logical_column_count(&placements).max(1);
            if column_count <= 1 { return Err("A table must contain at least one column.".into()); }
            ensure_table_grid_widths(table, column_count);
            let delete_column = placements.get(context.row_index).and_then(|row| row.get(context.cell_index))
                .map(|entry| entry.0).ok_or("table cell grid position not found")?;

            let mut remove = Vec::<(usize, usize)>::new();
            for row_index in 0..table.rows.len() {
                for cell_index in 0..table.rows[row_index].cells.len() {
                    let (start, span, _) = placements[row_index][cell_index];
                    if start <= delete_column && delete_column < start + span {
                        if span > 1 {
                            table.rows[row_index].cells[cell_index].column_span = table.rows[row_index].cells[cell_index].column_span.saturating_sub(1).max(1);
                        } else {
                            remove.push((row_index, cell_index));
                        }
                    }
                }
            }
            // At most one physical origin per row covers a logical column.
            for (row_index, cell_index) in remove.into_iter().rev() {
                table.rows[row_index].cells.remove(cell_index);
            }
            if delete_column < table.properties.grid_column_widths_mm.len() {
                table.properties.grid_column_widths_mm.remove(delete_column);
            }
            let new_column = delete_column.min(column_count - 2);
            let row = context.row_index.min(table.rows.len().saturating_sub(1));
            table_paragraph_covering_grid(table, row, new_column)
                .or_else(|| table.rows.iter().flat_map(|r| &r.cells).find_map(|cell| first_paragraph_id(&cell.blocks)))
                .ok_or("remaining table has no editable cell")?
        };
        let after = self.document.sections[context.section_index].clone();
        self.selection = Some(Selection::collapsed(DocumentPosition::new(target, 0)));
        self.commit("Delete table column", vec![Mutation::SectionChanged { section_id: after.id.clone(), before, after }], before_selection, None)?;
        Ok(())
    }

    pub fn merge_table_cell_right(&mut self) -> Result<(), String> {
        let context = self.table_cell_context().ok_or("Place the caret in a table cell first.")?;
        let before_selection = self.selection.clone();
        let before = self.document.sections[context.section_index].clone();
        let target = {
            let table = find_table_mut_in_section(&mut self.document.sections[context.section_index], &context.table_id)
                .ok_or("table not found")?;
            let row = table.rows.get_mut(context.row_index).ok_or("table row not found")?;
            if context.cell_index + 1 >= row.cells.len() { return Err("There is no cell to the right to merge.".into()); }
            if row.cells[context.cell_index].row_span != 1 || row.cells[context.cell_index + 1].row_span != 1 {
                return Err("Merge across an existing row span is not supported yet.".into());
            }
            let right = row.cells.remove(context.cell_index + 1);
            let left = &mut row.cells[context.cell_index];
            left.column_span = left.column_span.saturating_add(right.column_span).max(1);
            left.width_mm = match (left.width_mm, right.width_mm) {
                (Some(a), Some(b)) => Some(a + b), (Some(a), None) => Some(a), (None, Some(b)) => Some(b), _ => None,
            };
            if !right.blocks.is_empty() {
                if !left.blocks.is_empty() { left.blocks.push(Block::Paragraph(Paragraph::plain(""))); }
                left.blocks.extend(right.blocks);
            }
            first_paragraph_id(&left.blocks).ok_or("merged cell has no editable paragraph")?
        };
        let after = self.document.sections[context.section_index].clone();
        self.selection = Some(Selection::collapsed(DocumentPosition::new(target, 0)));
        self.commit("Merge table cells", vec![Mutation::SectionChanged { section_id: after.id.clone(), before, after }], before_selection, None)?;
        Ok(())
    }

    /// Merge the current cell with the logical cell immediately below it.
    /// The lower semantic cell is removed and the upper cell owns the combined
    /// vertical span. Horizontal spans must match so the logical grid remains
    /// rectangular and DOCX can map the result to vMerge/gridSpan later.
    pub fn merge_table_cell_down(&mut self) -> Result<(), String> {
        let context = self.table_cell_context().ok_or("Place the caret in a table cell first.")?;
        let before_selection = self.selection.clone();
        let before = self.document.sections[context.section_index].clone();
        let target = {
            let table = find_table_mut_in_section(
                &mut self.document.sections[context.section_index],
                &context.table_id,
            ).ok_or("table not found")?;
            let placements = table_grid_placements(table);
            let (grid_start, grid_span, current_row_span) = *placements
                .get(context.row_index).and_then(|row| row.get(context.cell_index))
                .ok_or("table cell grid position not found")?;
            let target_row = context.row_index + current_row_span;
            if target_row >= table.rows.len() { return Err("There is no cell below to merge.".into()); }
            let target_index = placements[target_row].iter().position(|(start, span, _)| {
                *start == grid_start && *span == grid_span
            }).ok_or("The cell below does not align with the current cell span.")?;
            let lower = table.rows[target_row].cells.remove(target_index);
            if usize::from(lower.column_span.max(1)) != grid_span {
                return Err("The cell below has a different horizontal span.".into());
            }
            let upper = table.rows.get_mut(context.row_index)
                .and_then(|row| row.cells.get_mut(context.cell_index))
                .ok_or("table cell not found")?;
            upper.row_span = upper.row_span.saturating_add(lower.row_span.max(1));
            if !lower.blocks.is_empty() {
                if !upper.blocks.is_empty() { upper.blocks.push(Block::Paragraph(Paragraph::plain(""))); }
                upper.blocks.extend(lower.blocks);
            }
            first_paragraph_id(&upper.blocks).ok_or("merged cell has no editable paragraph")?
        };
        let after = self.document.sections[context.section_index].clone();
        self.selection = Some(Selection::collapsed(DocumentPosition::new(target, 0)));
        self.commit("Merge table cells vertically", vec![Mutation::SectionChanged {
            section_id: after.id.clone(), before, after,
        }], before_selection, None)?;
        Ok(())
    }

    pub fn split_table_cell(&mut self) -> Result<(), String> {
        let context = self.table_cell_context().ok_or("Place the caret in a merged table cell first.")?;
        let before_selection = self.selection.clone();
        let before = self.document.sections[context.section_index].clone();
        {
            let table = find_table_mut_in_section(&mut self.document.sections[context.section_index], &context.table_id)
                .ok_or("table not found")?;
            let original_placements = table_grid_placements(table);
            let cell = table.rows.get(context.row_index)
                .and_then(|row| row.cells.get(context.cell_index)).cloned().ok_or("table cell not found")?;
            let columns = usize::from(cell.column_span.max(1));
            let rows = usize::from(cell.row_span.max(1)).min(table.rows.len().saturating_sub(context.row_index).max(1));
            if columns == 1 && rows == 1 { return Err("The current cell is not merged.".into()); }
            let (grid_start, _, _) = original_placements[context.row_index][context.cell_index];
            let split_width = cell.width_mm.map(|width| width / columns as f32);

            // The existing semantic content stays in the top-left cell. Every
            // newly exposed logical grid slot receives an independent blank
            // editable paragraph and inherits the visible cell properties.
            {
                let top_row = &mut table.rows[context.row_index];
                let top = &mut top_row.cells[context.cell_index];
                top.column_span = 1;
                top.row_span = 1;
                top.width_mm = split_width;
                for offset in 1..columns {
                    let mut new_cell = blank_cell_like(&cell);
                    new_cell.column_span = 1; new_cell.row_span = 1; new_cell.width_mm = split_width;
                    top_row.cells.insert(context.cell_index + offset, new_cell);
                }
            }

            for row_index in context.row_index + 1..context.row_index + rows {
                let insert_at = original_placements[row_index].iter()
                    .take_while(|(start, _, _)| *start < grid_start)
                    .count();
                for offset in 0..columns {
                    let mut new_cell = blank_cell_like(&cell);
                    new_cell.column_span = 1; new_cell.row_span = 1; new_cell.width_mm = split_width;
                    table.rows[row_index].cells.insert(insert_at + offset, new_cell);
                }
            }
        }
        let after = self.document.sections[context.section_index].clone();
        self.commit("Split table cell", vec![Mutation::SectionChanged {
            section_id: after.id.clone(), before, after,
        }], before_selection, None)?;
        Ok(())
    }

    pub fn apply_table_style(&mut self, id: Option<&StyleId>) -> Result<(), String> {
        if id.is_some_and(|id| !self.document.styles.table_styles.contains_key(id)) {
            return Err("Style not found".into());
        }
        let context = self.table_cell_context().ok_or("Place the caret in a table cell first.")?;
        let before = self.document.sections[context.section_index].clone();
        let mut after = before.clone();
        let table = find_table_mut_in_section(&mut after, &context.table_id).ok_or("Table not found")?;
        table.properties.style = id.cloned();
        table.properties.style_overrides_table_borders = id.map(|_| true);
        table.properties.style_first_row = None;
        table.properties.style_row_banding = None;
        if before == after { return Ok(()); }
        let mutation = Mutation::SectionChanged { section_id: after.id.clone(), before, after };
        self.commit_style_work("Apply table style", vec![mutation])
    }

    pub fn set_table_row_height(&mut self, height_mm: Option<f32>) -> Result<(), String> {
        if height_mm.is_some_and(|v| !v.is_finite() || !(2.0..=500.0).contains(&v)) {
            return Err("Row height must be between 2 and 500 mm.".into());
        }
        let context = self.table_cell_context().ok_or("Place the caret in a table cell first.")?;
        let before = self.document.sections[context.section_index].clone();
        let table = find_table_mut_in_section(&mut self.document.sections[context.section_index], &context.table_id).ok_or("table not found")?;
        table.rows[context.row_index].height_mm = height_mm;
        let after = self.document.sections[context.section_index].clone();
        self.commit("Table row height", vec![Mutation::SectionChanged { section_id: after.id.clone(), before, after }], self.selection.clone(), None)?;
        Ok(())
    }

    pub fn set_table_column_width(&mut self, width_mm: f32) -> Result<(), String> {
        if !width_mm.is_finite() || !(2.0..=500.0).contains(&width_mm) { return Err("Column width must be between 2 and 500 mm.".into()); }
        let context = self.table_cell_context().ok_or("Place the caret in a table cell first.")?;
        let before = self.document.sections[context.section_index].clone();
        let table = find_table_mut_in_section(&mut self.document.sections[context.section_index], &context.table_id).ok_or("table not found")?;
        let placements = table_grid_placements(table);
        let column_count = table_logical_column_count(&placements).max(1);
        ensure_table_grid_widths(table, column_count);
        let logical_column = placements.get(context.row_index).and_then(|row| row.get(context.cell_index))
            .map(|entry| entry.0).ok_or("table cell grid position not found")?;
        table.properties.grid_column_widths_mm[logical_column] = width_mm;
        // Keep physical single-column width hints in sync for adapters that
        // inspect tcW, while the logical grid remains authoritative.
        for row_index in 0..table.rows.len() {
            for cell_index in 0..table.rows[row_index].cells.len() {
                let (start, span, _) = placements[row_index][cell_index];
                if span == 1 && start == logical_column { table.rows[row_index].cells[cell_index].width_mm = Some(width_mm); }
            }
        }
        let after = self.document.sections[context.section_index].clone();
        self.commit("Table column width", vec![Mutation::SectionChanged { section_id: after.id.clone(), before, after }], self.selection.clone(), None)?;
        Ok(())
    }

    pub fn set_table_row_can_split(&mut self, allow: bool) -> Result<(), String> {
        let context = self.table_cell_context().ok_or("Place the caret in a table cell first.")?;
        let before = self.document.sections[context.section_index].clone();
        let table = find_table_mut_in_section(&mut self.document.sections[context.section_index], &context.table_id).ok_or("table not found")?;
        table.rows[context.row_index].allow_break_across_pages = allow;
        let after = self.document.sections[context.section_index].clone();
        self.commit("Table row split", vec![Mutation::SectionChanged { section_id: after.id.clone(), before, after }], self.selection.clone(), None)?;
        Ok(())
    }

    pub fn set_table_row_repeat_header(&mut self, repeat: bool) -> Result<(), String> {
        let context = self.table_cell_context().ok_or("Place the caret in a table cell first.")?;
        let before = self.document.sections[context.section_index].clone();
        let table = find_table_mut_in_section(&mut self.document.sections[context.section_index], &context.table_id).ok_or("table not found")?;
        table.rows[context.row_index].repeat_as_header = repeat;
        let after = self.document.sections[context.section_index].clone();
        self.commit(if repeat { "Repeat table header row" } else { "Stop repeating table header row" }, vec![Mutation::SectionChanged {
            section_id: after.id.clone(), before, after,
        }], self.selection.clone(), None)?;
        Ok(())
    }

    /// Fill only the addressed cells, including empty and merged cells. The
    /// explicit "none" value overrides an inherited table-style fill.
    pub fn set_table_cell_fill(&mut self, id: &NodeId, cells: &[NodeId], fill: &str) -> Result<(), String> {
        if fill != "none" && !(fill.len() == 7 && fill.starts_with('#') && fill[1..].bytes().all(|b| b.is_ascii_hexdigit())) {
            return Err("Cell fill must be #RRGGBB or none".into());
        }
        let selected: std::collections::HashSet<_> = cells.iter().collect();
        if selected.is_empty() { return Ok(()); }
        let index = self.document.sections.iter().position(|s| find_table_in_section(s, id).is_some()).ok_or("Table not found")?;
        let before = self.document.sections[index].clone();
        let mut after = before.clone();
        let table = find_table_mut_in_section(&mut after, id).ok_or("Table not found")?;
        let count = table.rows.iter_mut().flat_map(|r| &mut r.cells).filter(|c| selected.contains(&c.id))
            .map(|c| { c.shading = Some(if fill == "none" { "none".into() } else { fill.to_ascii_uppercase() }); }).count();
        if count != selected.len() { return Err("Selected table cells no longer exist".into()); }
        if after == before { return Ok(()); }
        self.document.sections[index] = after.clone();
        self.commit("Table cell fill", vec![Mutation::SectionChanged { section_id: after.id.clone(), before, after }], self.selection.clone(), None)?;
        Ok(())
    }

    pub fn set_table_cell_vertical_alignment(&mut self, alignment: CellVerticalAlignment) -> Result<(), String> {
        let context = self.table_cell_context().ok_or("Place the caret in a table cell first.")?;
        let before = self.document.sections[context.section_index].clone();
        let table = find_table_mut_in_section(&mut self.document.sections[context.section_index], &context.table_id).ok_or("table not found")?;
        table.rows.get_mut(context.row_index).and_then(|row| row.cells.get_mut(context.cell_index))
            .ok_or("table cell not found")?.vertical_alignment = alignment;
        let after = self.document.sections[context.section_index].clone();
        self.commit("Table cell vertical alignment", vec![Mutation::SectionChanged {
            section_id: after.id.clone(), before, after,
        }], self.selection.clone(), None)?;
        Ok(())
    }

    pub fn distribute_table_columns_evenly(&mut self) -> Result<(), String> {
        let context = self.table_cell_context().ok_or("Place the caret in a table cell first.")?;
        let before = self.document.sections[context.section_index].clone();
        let table = find_table_mut_in_section(&mut self.document.sections[context.section_index], &context.table_id).ok_or("table not found")?;
        let placements = table_grid_placements(table);
        let column_count = table_logical_column_count(&placements).max(1);
        ensure_table_grid_widths(table, column_count);
        let total = table.properties.grid_column_widths_mm.iter().sum::<f32>();
        let width = if total.is_finite() && total > 0.0 { total / column_count as f32 } else { 25.0 };
        table.properties.grid_column_widths_mm = vec![width; column_count];
        // Per-cell width hints are secondary to the table grid. Clear them so
        // adapters/layout do not retain contradictory old geometry.
        for row in &mut table.rows { for cell in &mut row.cells { cell.width_mm = None; } }
        let after = self.document.sections[context.section_index].clone();
        self.commit("Distribute table columns evenly", vec![Mutation::SectionChanged {
            section_id: after.id.clone(), before, after,
        }], self.selection.clone(), None)?;
        Ok(())
    }

    /// Use the tallest laid-out row as the common minimum. Content can
    /// still grow after further edits or narrower columns.
    pub fn distribute_table_rows_evenly(&mut self, height_mm: f32) -> Result<(), String> {
        if !height_mm.is_finite() || height_mm <= 0.0 {
            return Err("Invalid table row height.".into());
        }
        let context = self.table_cell_context().ok_or("Place the caret in a table cell first.")?;
        let section = &mut self.document.sections[context.section_index];
        let before = section.clone();
        let table = find_table_mut_in_section(section, &context.table_id).ok_or("Table not found.")?;
        for row in &mut table.rows {
            row.height_mm = Some(height_mm);
        }
        let after = section.clone();
        self.commit("Distribute table rows evenly", vec![Mutation::SectionChanged {
            section_id: after.id.clone(), before, after,
        }], self.selection.clone(), None)?;
        Ok(())
    }

    /// Fit top-level tables in single-column sections to the left and right
    /// margins. A floating table keeps its vertical anchor and wrapping.
    pub fn table_margin_width(&self, id: &NodeId) -> Option<f32> {
        self.document.sections.iter().find_map(|section| {
            let contains = |blocks: &[Block]| blocks.iter().any(|block| matches!(block, Block::Table(table) if &table.id == id));
            let top_level = contains(&section.blocks) || section.header_footers().any(|hf| contains(&hf.stream.blocks));
            (top_level && section.properties.column_count <= 1).then(|| section.properties.text_width_mm())
        })
    }

    pub fn fit_table_to_margins(&mut self, widths: &[f32]) -> Result<(), String> {
        let context = self.table_cell_context().ok_or("Place the caret in a table cell first.")?;
        let width_mm = self.table_margin_width(&context.table_id).ok_or("This table cannot span the page margins.")?;
        let current_width: f32 = widths.iter().sum();
        if !current_width.is_finite() || widths.iter().any(|width| *width <= 0.0) || widths.is_empty() {
            return Err("Invalid table column widths.".into());
        }

        let section = &mut self.document.sections[context.section_index];
        let before = section.clone();
        let table = find_table_mut_in_section(section, &context.table_id).ok_or("Table not found.")?;
        if widths.len() != table_logical_column_count(&table_grid_placements(table)).max(1) {
            return Err("Table layout changed. Open the table menu again.".into());
        }
        table.properties.preferred_width_mm = Some(width_mm);
        table.properties.grid_column_widths_mm = widths.iter().map(|width| (width / current_width) * width_mm).collect();
        table.properties.left_indent_mm = None;
        table.properties.alignment = Some(model::TableAlignment::Left);
        if let Some(position) = &mut table.properties.position {
            position.horizontal_from = model::HorizontalFrom::Margin;
            position.horizontal = model::HorizontalPlacement::Left;
        }
        for cell in table.rows.iter_mut().flat_map(|row| &mut row.cells) {
            cell.width_mm = None;
        }
        let after = section.clone();
        self.commit("Fit table to page margins", vec![Mutation::SectionChanged {
            section_id: after.id.clone(), before, after,
        }], self.selection.clone(), None)?;
        Ok(())
    }

    /// Remove the whole table containing the caret. The caret moves to the
    /// paragraph that followed the table (inserting one if the table ended
    /// the section).
    pub fn delete_table(&mut self) -> Result<(), String> {
        let context = self.table_cell_context().ok_or("Place the caret in a table cell first.")?;
        let before_selection = self.selection.clone();
        let section = &self.document.sections[context.section_index];
        let index = section.blocks.iter().position(|block| matches!(block, Block::Table(t) if t.id == context.table_id))
            .ok_or("Only tables in the main text can be deleted as a whole; select its cells instead.")?;
        let section_id = section.id.clone();
        let mut mutations = Vec::new();
        let removed = self.document.sections[context.section_index].blocks.remove(index);
        mutations.push(Mutation::BlockRemoved { section_id: section_id.clone(), index, block: removed });
        let blocks = &self.document.sections[context.section_index].blocks;
        let target = blocks.get(index).and_then(|b| first_paragraph_id(std::slice::from_ref(b)))
            .or_else(|| index.checked_sub(1).and_then(|i| blocks.get(i)).and_then(|b| first_paragraph_id(std::slice::from_ref(b))));
        let target = match target {
            Some(id) => id,
            None => {
                let paragraph = Paragraph::plain("");
                let id = paragraph.id.clone();
                let block = Block::Paragraph(paragraph);
                self.document.sections[context.section_index].blocks.insert(index, block.clone());
                mutations.push(Mutation::BlockInserted { section_id, index, block });
                id
            }
        };
        self.selection = Some(Selection::collapsed(DocumentPosition::new(target, 0)));
        self.commit("Delete table", mutations, before_selection, None)?;
        Ok(())
    }

    /// Word's Insert Cells / Delete Cells. Contents move one cell along the
    /// row (`vertical == false`) or down the column; the grid stays
    /// rectangular. Inserting grows the table by a column or row only when
    /// the cell that would be pushed out is not empty; deleting leaves an
    /// empty cell at the end of the row or column.
    pub fn shift_table_cells(&mut self, insert: bool, vertical: bool) -> Result<(), String> {
        let context = self.table_cell_context().ok_or("Place the caret in a table cell first.")?;
        let before_selection = self.selection.clone();
        let before = self.document.sections[context.section_index].clone();
        let target = {
            let table = find_table_mut_in_section(&mut self.document.sections[context.section_index], &context.table_id)
                .ok_or("table not found")?;
            let run = |table: &Table| -> Vec<(usize, usize)> {
                if vertical {
                    let placements = table_grid_placements(table);
                    let column = placements[context.row_index][context.cell_index].0;
                    (context.row_index..table.rows.len()).filter_map(|row| placements[row].iter()
                        .position(|(start, _, _)| *start == column).map(|cell| (row, cell))).collect()
                } else {
                    (context.cell_index..table.rows[context.row_index].cells.len()).map(|cell| (context.row_index, cell)).collect()
                }
            };
            let mut cells = run(table);
            if insert {
                let &(row, cell) = cells.last().ok_or("table cell not found")?;
                if !cell_is_blank(&table.rows[row].cells[cell]) {
                    if vertical { append_table_row(table); } else { append_table_column(table); }
                    cells = run(table);
                }
                for pair in (1..cells.len()).rev() {
                    let (row, cell) = cells[pair - 1];
                    let blocks = table.rows[row].cells[cell].blocks.clone();
                    let (row, cell) = cells[pair];
                    table.rows[row].cells[cell].blocks = blocks;
                }
                table.rows[context.row_index].cells[context.cell_index].blocks = (vec![Block::Paragraph(Paragraph::plain(""))]).into();
            } else {
                for pair in 1..cells.len() {
                    let (row, cell) = cells[pair];
                    let blocks = table.rows[row].cells[cell].blocks.clone();
                    let (row, cell) = cells[pair - 1];
                    table.rows[row].cells[cell].blocks = blocks;
                }
                let &(row, cell) = cells.last().ok_or("table cell not found")?;
                table.rows[row].cells[cell].blocks = (vec![Block::Paragraph(Paragraph::plain(""))]).into();
            }
            first_paragraph_id(&table.rows[context.row_index].cells[context.cell_index].blocks).ok_or("table cell has no editable paragraph")?
        };
        let after = self.document.sections[context.section_index].clone();
        self.selection = Some(Selection::collapsed(DocumentPosition::new(target, 0)));
        let label = match (insert, vertical) {
            (true, false) => "Insert cells (shift right)", (true, true) => "Insert cells (shift down)",
            (false, false) => "Delete cells (shift left)", (false, true) => "Delete cells (shift up)",
        };
        self.commit(label, vec![Mutation::SectionChanged { section_id: after.id.clone(), before, after }], before_selection, None)?;
        Ok(())
    }

    /// Increase/decrease the nesting level while keeping the same semantic list
    /// instance. Missing level definitions inherit the nearest existing level
    /// in layout/export, matching the current list compositor behaviour.
    pub fn adjust_list_level(&mut self, delta: i8) -> Result<(), String> {
        let ids = self.selected_paragraph_ids();
        if ids.is_empty() { return Err("No paragraph selected.".into()); }
        let before_selection = self.selection.clone();
        let mut mutations = Vec::new();
        for id in ids {
            let before = self.paragraph(&id).cloned().ok_or("paragraph not found")?;
            if before.properties.list_id.is_none() { continue; }
            let current = i16::from(before.properties.list_level.unwrap_or(0));
            let level = (current + i16::from(delta)).clamp(0, 8) as u8;
            if level == before.properties.list_level.unwrap_or(0) { continue; }
            let mut after = before.clone(); after.properties.list_level = Some(level);
            self.replace_paragraph(&after)?;
            mutations.push(Mutation::ParagraphChanged { id, before, after });
        }
        self.commit(if delta > 0 { "Demote list level" } else { "Promote list level" }, mutations, before_selection, None)?;
        Ok(())
    }

    pub fn current_list_level(&self) -> Option<u8> {
        let position = self.focus_position()?;
        let paragraph = self.paragraph(&position.node_id)?;
        paragraph.properties.list_id.as_ref()?;
        Some(paragraph.properties.list_level.unwrap_or(0))
    }

    /// Dimensions of the inline picture adjacent to/containing the caret marker.
    /// This lets front ends expose picture commands without depending on the
    /// paragraph's internal flattened-object representation.
    pub fn inline_image_size_at_caret(&self) -> Option<(f32, f32)> {
        let position = self.focus_position()?;
        let paragraph = self.paragraph(&position.node_id)?;
        let (_, _, image) = image_near_offset(&paragraph.content, position.byte_offset, 0)?;
        Some((image.width_mm, image.height_mm))
    }

    pub fn delete_inline_image_at_caret(&mut self) -> Result<(), String> {
        let position = self.focus_position().ok_or("no active caret")?;
        let before_selection = self.selection.clone();
        let before = self.paragraph(&position.node_id).cloned().ok_or("paragraph not found")?;
        let (start, end, _) = image_near_offset(&before.content, position.byte_offset, 0)
            .ok_or("Place the caret next to an inline picture first.")?;
        let mut after = before.clone();
        delete_range_from_paragraph(&mut after, start, end)?;
        self.replace_paragraph(&after)?;
        self.selection = Some(Selection::collapsed(DocumentPosition::new(after.id.clone(), start)));
        self.commit("Delete picture", vec![Mutation::ParagraphChanged {
            id: after.id.clone(), before, after,
        }], before_selection, None)?;
        Ok(())
    }

    /// Resize the inline image adjacent to/containing the caret marker.  Image
    /// bytes remain shared in Document::resources; only semantic geometry is
    /// changed, so undo is lightweight.
    pub fn resize_inline_image_at_caret(&mut self, width_mm: f32, height_mm: f32) -> Result<(), String> {
        if !width_mm.is_finite() || !height_mm.is_finite() || width_mm <= 0.0 || height_mm <= 0.0 {
            return Err("Image dimensions must be positive finite values.".into());
        }
        let position = self.focus_position().ok_or("no active caret")?;
        let before_selection = self.selection.clone();
        let before = self.paragraph(&position.node_id).cloned().ok_or("paragraph not found")?;
        let mut after = before.clone();
        if !resize_image_near_offset(&mut after.content, position.byte_offset, width_mm, height_mm) {
            return Err("Place the caret next to an inline picture first.".into());
        }
        self.replace_paragraph(&after)?;
        self.commit("Resize picture", vec![Mutation::ParagraphChanged { id: after.id.clone(), before, after }], before_selection, None)?;
        Ok(())
    }

    /// Insert a real resource-backed inline image. Raster decoding is a render
    /// concern; the editor only owns bytes, dimensions and semantic placement.
    pub fn insert_inline_image(&mut self, media_type: String, original_name: Option<String>, data: Vec<u8>, width_mm: f32, height_mm: f32, alt_text: Option<String>) -> Result<(), String> {
        if data.is_empty() { return Err("Image file is empty.".into()); }
        if !width_mm.is_finite() || !height_mm.is_finite() || width_mm <= 0.0 || height_mm <= 0.0 { return Err("Invalid image dimensions.".into()); }
        let before_selection = self.selection.clone();
        let mut mutations = Vec::new();
        self.delete_selection_into(&mut mutations)?;
        let pos = self.focus_position().ok_or("no active caret")?;
        let before = self.paragraph(&pos.node_id).cloned().ok_or("paragraph not found")?;
        let resource_id = ResourceId::new();
        let resource = Resource { font_faces: Vec::new(),  id: resource_id.clone(), media_type, original_name, data: data.into() };
        self.document.resources.insert(resource_id.clone(), resource.clone());
        mutations.push(Mutation::ResourceInserted { id: resource_id.clone(), resource });
        let (mut left, right) = split_content(&before.content, pos.byte_offset)?;
        left.push(Inline::Image(InlineImage { id: NodeId::new(), resource: resource_id, width_mm, height_mm, alt_text, effects: Default::default() }));
        left.extend(right);
        let mut after = before.clone(); after.content = (normalized_content(left)).into();
        self.replace_paragraph(&after)?;
        mutations.push(Mutation::ParagraphChanged { id: after.id.clone(), before, after });
        self.selection = Some(Selection::collapsed(DocumentPosition::new(pos.node_id, pos.byte_offset + INLINE_OBJECT_MARKER.len())));
        self.commit("Insert picture", mutations, before_selection, None)?;
        Ok(())
    }

    pub fn formatting_summary(&self) -> FormattingSummary {
        self.summarize_formatting(self.table_formatting_spans.as_deref())
    }

    fn summarize_formatting(&self, cells: Option<&[(NodeId, usize, usize)]>) -> FormattingSummary {
        let (character, ligatures, kerning_mode) = if cells.is_none()
            && self.selection.as_ref().is_none_or(Selection::is_collapsed) {
            let mut properties = self.effective_caret_properties();
            properties.overlay(&self.document.styles.resolve_character(self.typing_style.as_ref()));
            properties.overlay(&self.typing_properties);
            let character = normalized_effective(properties);
            let typography = character.typography.clone().unwrap_or_default();
            (character, Some(typography.ligatures), Some(effective_kerning_mode(&typography)))
        } else {
            let mut result = None;
            let mut ligatures = Vec::new();
            let mut kerning = Vec::new();
            let selection;
            let spans = match cells {
                Some(cells) => cells,
                None => { selection = self.selected_paragraph_spans(); &selection }
            };
            for (id, start, end) in spans {
                let Some(p) = self.paragraph(id) else { continue; };
                let selected;
                let content = if cells.is_some() {
                    &p.content[..]
                } else {
                    let Ok(slice) = slice_content(&p.content, *start, *end) else { continue; };
                    selected = slice;
                    &selected
                };
                let styles = &self.document.styles;
                collect_common_runs(styles, p.style.as_ref(), content, &mut result);
                collect_ligature_modes(content, styles, p.style.as_ref(), &mut ligatures);
                collect_kerning_modes(content, styles, p.style.as_ref(), &mut kerning);
            }
            (
                result.unwrap_or_else(|| normalized_effective(self.effective_caret_properties())),
                ligatures.first().copied().filter(|first| ligatures.iter().all(|value| value == first)),
                kerning.first().copied().filter(|first| kerning.iter().all(|value| value == first)),
            )
        };
        let ids = cells.map_or_else(|| self.selected_paragraph_ids(),
            |spans| spans.iter().map(|(id, _, _)| id.clone()).collect());
        let mut result: Option<FormattingSummary> = None;
        for id in ids {
            let Some(p) = self.paragraph(&id) else { continue; };
            let props = self.document.styles.resolve_paragraph_with_direct(p.style.as_ref(), &p.properties);
            let current = FormattingSummary {
                character: character.clone(),
                ligatures,
                kerning_mode,
                alignment: Some(props.alignment.unwrap_or(ParagraphAlignment::Left)),
                direction: Some(props.direction.unwrap_or(ParagraphDirection::Auto)),
                style: Some(p.style.clone().unwrap_or_else(|| StyleId::from(STYLE_NORMAL))),
                line_spacing: Some(props.line_spacing.unwrap_or(1.2)),
                space_before_pt: Some(props.space_before_pt.unwrap_or(0.0)),
                space_after_pt: Some(props.space_after_pt.unwrap_or(0.0)),
                tabs: Some(props.tabs.clone().unwrap_or_default()),
            };
            if let Some(common) = &mut result {
                if common.alignment != current.alignment { common.alignment = None; }
                if common.direction != current.direction { common.direction = None; }
                if common.style != current.style { common.style = None; }
                if common.line_spacing != current.line_spacing { common.line_spacing = None; }
                if common.space_before_pt != current.space_before_pt { common.space_before_pt = None; }
                if common.space_after_pt != current.space_after_pt { common.space_after_pt = None; }
                if common.tabs != current.tabs { common.tabs = None; }
            } else { result = Some(current); }
        }
        result.unwrap_or(FormattingSummary { character, ligatures, kerning_mode, alignment: None, direction: None, style: None,
            line_spacing: None, space_before_pt: None, space_after_pt: None, tabs: None })
    }
}

fn blank_table_cell() -> TableCell {
    TableCell {
        id: NodeId::new(),
        blocks: (vec![Block::Paragraph(Paragraph::plain(""))]).into(),
        column_span: 1,
        row_span: 1,
        width_mm: None,
        shading: None,
        borders: None,
        vertical_alignment: CellVerticalAlignment::Top,
        margins: None,
        text_direction: None,
    }
}

fn blank_cell_like(template: &TableCell) -> TableCell {
    TableCell {
        id: NodeId::new(),
        blocks: (vec![Block::Paragraph(Paragraph::plain(""))]).into(),
        column_span: 1,
        row_span: 1,
        width_mm: template.width_mm,
        shading: template.shading.clone(),
        borders: template.borders.clone(),
        vertical_alignment: template.vertical_alignment,
        margins: template.margins,
        text_direction: template.text_direction,
    }
}

pub(crate) fn first_paragraph_id(blocks: &[Block]) -> Option<NodeId> {
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => return Some(paragraph.id.clone()),
            Block::Table(table) => {
                for row in &table.rows {
                    for cell in &row.cells {
                        if let Some(id) = first_paragraph_id(&cell.blocks) { return Some(id); }
                    }
                }
            }
            _ => {}
        }
    }
    None
}

/// Resolve every physical semantic cell to a logical table grid column.
/// Active row spans reserve their columns on continuation rows, matching the
/// placement algorithm used by layout.
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
            for column in cursor..cursor + span {
                span_until[column] = span_until[column].max(row_index + row_span);
            }
            placements.push((cursor, span, row_span));
            cursor += span;
        }
        result.push(placements);
    }
    result
}


fn table_logical_column_count(placements: &[Vec<(usize, usize, usize)>]) -> usize {
    placements.iter().flat_map(|row| row.iter())
        .map(|(start, span, _)| start + span)
        .max().unwrap_or(0)
}

fn ensure_table_grid_widths(table: &mut Table, column_count: usize) {
    if column_count == 0 { table.properties.grid_column_widths_mm.clear(); return; }
    let existing_is_valid = table.properties.grid_column_widths_mm.len() == column_count
        && table.properties.grid_column_widths_mm.iter().all(|value| value.is_finite() && *value > 0.0);
    if existing_is_valid { return; }

    let placements = table_grid_placements(table);
    let fallback = table.properties.preferred_width_mm
        .filter(|value| value.is_finite() && *value > 0.0)
        .map(|width| width / column_count as f32)
        .unwrap_or(25.0);
    let mut hints = vec![None::<f32>; column_count];
    for (row_index, row) in table.rows.iter().enumerate() {
        for (cell_index, cell) in row.cells.iter().enumerate() {
            let Some(width) = cell.width_mm.filter(|value| value.is_finite() && *value > 0.0) else { continue; };
            let (start, span, _) = placements[row_index][cell_index];
            let per_column = width / span.max(1) as f32;
            for column in start..(start + span).min(column_count) {
                if hints[column].is_none() { hints[column] = Some(per_column); }
            }
        }
    }
    table.properties.grid_column_widths_mm = hints.into_iter().map(|hint| hint.unwrap_or(fallback)).collect();
}

fn cell_is_blank(cell: &TableCell) -> bool {
    cell.blocks.iter().all(|block| matches!(block, Block::Paragraph(p)
        if p.content.iter().all(|inline| matches!(inline, Inline::Text(run) if run.text.is_empty()))))
}

/// A new empty logical column at the right edge of the table.
fn append_table_column(table: &mut Table) {
    let count = table_logical_column_count(&table_grid_placements(table)).max(1);
    ensure_table_grid_widths(table, count);
    let width = inserted_grid_width(&table.properties.grid_column_widths_mm, count);
    table.properties.grid_column_widths_mm.push(width);
    for row in &mut table.rows {
        let mut cell = blank_table_cell();
        cell.width_mm = Some(width);
        row.cells.push(cell);
    }
}

/// A new empty row at the bottom of the table, one cell per logical column.
fn append_table_row(table: &mut Table) {
    let count = table_logical_column_count(&table_grid_placements(table)).max(1);
    ensure_table_grid_widths(table, count);
    let template = table.rows.last().cloned();
    let cells = table.properties.grid_column_widths_mm.iter().map(|width| {
        let mut cell = blank_table_cell();
        cell.width_mm = Some(*width);
        cell
    }).collect();
    table.rows.push(TableRow {
        id: NodeId::new(), cells, repeat_as_header: false,
        allow_break_across_pages: template.as_ref().is_none_or(|r| r.allow_break_across_pages),
        height_mm: template.and_then(|r| r.height_mm),
    });
}

fn inserted_grid_width(widths: &[f32], insert_at: usize) -> f32 {
    match (insert_at.checked_sub(1).and_then(|index| widths.get(index)).copied(), widths.get(insert_at).copied()) {
        (Some(left), Some(right)) if left.is_finite() && right.is_finite() => ((left + right) * 0.5).max(2.0),
        (Some(left), _) if left.is_finite() => left.max(2.0),
        (_, Some(right)) if right.is_finite() => right.max(2.0),
        _ => 25.0,
    }
}

fn table_paragraph_covering_grid(table: &Table, row_index: usize, logical_column: usize) -> Option<NodeId> {
    if row_index >= table.rows.len() { return None; }
    let placements = table_grid_placements(table);
    for origin_row in 0..=row_index {
        for (cell_index, cell) in table.rows[origin_row].cells.iter().enumerate() {
            let (start, span, row_span) = placements[origin_row][cell_index];
            if origin_row + row_span > row_index && start <= logical_column && logical_column < start + span {
                return first_paragraph_id(&cell.blocks);
            }
        }
    }
    None
}

fn find_table_cell_for_paragraph(
    blocks: &[Block],
    paragraph_id: &NodeId,
) -> Option<(NodeId, usize, usize, NodeId)> {
    for block in blocks {
        if let Block::Table(table) = block {
            for (row_index, row) in table.rows.iter().enumerate() {
                for (cell_index, cell) in row.cells.iter().enumerate() {
                    if let Some(found) = find_table_cell_for_paragraph(&cell.blocks, paragraph_id) { return Some(found); }
                    if find_paragraph_in_blocks(&cell.blocks, paragraph_id).is_some() {
                        return Some((table.id.clone(), row_index, cell_index, cell.id.clone()));
                    }
                }
            }
        }
    }
    None
}

fn find_table_in_section<'a>(section: &'a Section, id: &NodeId) -> Option<&'a Table> {
    if let Some(table) = find_table_in_blocks(&section.blocks, id) { return Some(table); }
    for hf in section.header_footers() {
        if let Some(table) = find_table_in_blocks(&hf.stream.blocks, id) { return Some(table); }
    }
    None
}
fn find_table_mut_in_section<'a>(section: &'a mut Section, id: &NodeId) -> Option<&'a mut Table> {
    // Destructured so the body and header/footer borrows stay disjoint.
    let Section { blocks, headers, footers, .. } = section;
    if let Some(table) = find_table_mut_in_blocks(blocks, id) { return Some(table); }
    for (_, hf) in headers.iter_mut().chain(footers.iter_mut()) {
        if let Some(table) = find_table_mut_in_blocks(&mut hf.blocks, id) { return Some(table); }
    }
    None
}

fn find_table_in_blocks<'a>(blocks: &'a [Block], table_id: &NodeId) -> Option<&'a Table> {
    for block in blocks {
        if let Block::Table(table) = block {
            if &table.id == table_id { return Some(table); }
            for row in &table.rows {
                for cell in &row.cells {
                    if let Some(found) = find_table_in_blocks(&cell.blocks, table_id) { return Some(found); }
                }
            }
        }
    }
    None
}

fn find_table_mut_in_blocks<'a>(blocks: &'a mut [Block], table_id: &NodeId) -> Option<&'a mut Table> {
    for block in blocks {
        if let Block::Table(table) = block {
            if &table.id == table_id { return Some(table); }
            for row in &mut table.rows {
                for cell in &mut row.cells {
                    if let Some(found) = find_table_mut_in_blocks(&mut cell.blocks, table_id) { return Some(found); }
                }
            }
        }
    }
    None
}


fn image_near_offset<'a>(content: &'a [Inline], offset: usize, base: usize) -> Option<(usize, usize, &'a InlineImage)> {
    let mut cursor = base;
    for inline in content {
        let len = inline_flat_len(inline);
        let end = cursor + len;
        match inline {
            Inline::Image(image) if offset >= cursor && offset <= end => return Some((cursor, end, image)),
            Inline::Hyperlink(link) if offset >= cursor && offset <= end => {
                if let Some(found) = image_near_offset(&link.content, offset, cursor) { return Some(found); }
            }
            _ => {}
        }
        cursor = end;
    }
    None
}

fn resize_image_near_offset(content: &mut [Inline], offset: usize, width_mm: f32, height_mm: f32) -> bool {
    let mut cursor = 0usize;
    for inline in content {
        let len = inline_flat_len(inline);
        let end = cursor + len;
        match inline {
            Inline::Image(image) if offset >= cursor && offset <= end => {
                image.width_mm = width_mm;
                image.height_mm = height_mm;
                return true;
            }
            Inline::Hyperlink(link) if offset >= cursor && offset <= end => {
                if resize_image_near_offset(&mut link.content, offset.saturating_sub(cursor), width_mm, height_mm) { return true; }
            }
            _ => {}
        }
        cursor = end;
    }
    false
}

fn patch_ligatures(content:&mut [Inline],styles:&model::StyleSheet,paragraph_style:Option<&StyleId>,mode:LigatureMode)->bool {
    let mut changed=false;
    for inline in content {match inline {
        Inline::Text(run) => {
            let mut typography=styles.resolve_run(paragraph_style,run.style.as_ref(),&run.properties).typography.unwrap_or_default();
            if typography.ligatures!=mode {
                typography.ligatures=mode;run.properties.typography=Some(typography);changed=true;
            }
        }
        Inline::Hyperlink(link)=>changed|=patch_ligatures(&mut link.content,styles,paragraph_style,mode),
        _=>{}
    }} changed
}
fn patch_kerning(content:&mut [Inline],styles:&model::StyleSheet,paragraph_style:Option<&StyleId>,mode:KerningMode)->bool {
    let mut changed=false;
    for inline in content {match inline {
        Inline::Text(run) => {
            let mut typography=styles.resolve_run(paragraph_style,run.style.as_ref(),&run.properties).typography.unwrap_or_default();
            if typography.kerning_mode!=mode || typography.kerning!=(mode!=KerningMode::Off) {
                typography.kerning_mode=mode;typography.kerning=mode!=KerningMode::Off;
                run.properties.typography=Some(typography);changed=true;
            }
        }
        Inline::Hyperlink(link)=>changed|=patch_kerning(&mut link.content,styles,paragraph_style,mode),
        _=>{}
    }} changed
}
fn effective_kerning_mode(typography:&model::OpenTypeTypography)->KerningMode {
    if typography.kerning {typography.kerning_mode} else {KerningMode::Off}
}
fn collect_kerning_modes(content:&[Inline],styles:&model::StyleSheet,paragraph_style:Option<&StyleId>,out:&mut Vec<KerningMode>) {
    for inline in content {match inline {
        Inline::Text(run)=>out.push(effective_kerning_mode(&styles.resolve_run(paragraph_style,run.style.as_ref(),&run.properties).typography.unwrap_or_default())),
        Inline::Hyperlink(link)=>collect_kerning_modes(&link.content,styles,paragraph_style,out),
        _=>{}
    }}
}
fn collect_ligature_modes(content:&[Inline],styles:&model::StyleSheet,paragraph_style:Option<&StyleId>,out:&mut Vec<LigatureMode>) {
    for inline in content {match inline {
        Inline::Text(run)=>out.push(styles.resolve_run(paragraph_style,run.style.as_ref(),&run.properties).typography.unwrap_or_default().ligatures),
        Inline::Hyperlink(link)=>collect_ligature_modes(&link.content,styles,paragraph_style,out),
        _=>{}
    }}
}

fn patch_runs(content: &mut [Inline], patch: &CharacterProperties) -> bool {
    let mut changed = false;
    for item in content {
        match item {
            Inline::Text(run) => {
                let before = run.properties.clone();
                run.properties.overlay(patch);
                changed |= before != run.properties;
            }
            Inline::Hyperlink(link) => changed |= patch_runs(&mut link.content, patch),
            _ => {}
        }
    }
    changed
}

fn normalized_effective(mut props: CharacterProperties) -> CharacterProperties {
    props.bold = Some(props.bold.unwrap_or(false));
    props.italic = Some(props.italic.unwrap_or(false));
    props.underline = Some(props.underline.unwrap_or(false));
    props
}

fn collect_common_runs(styles: &model::StyleSheet, paragraph_style: Option<&StyleId>,
    content: &[Inline], result: &mut Option<CharacterProperties>) {
    for inline in content {
        match inline {
            Inline::Text(run) => {
                let current = normalized_effective(styles.resolve_run(paragraph_style, run.style.as_ref(), &run.properties));
                if let Some(common) = result {
                    macro_rules! intersect { ($($field:ident),*) => { $(if common.$field != current.$field { common.$field = None; })* }; }
                    intersect!(font_family, font_size_pt, bold, italic, underline, color, highlight, language, typography);
                } else { *result = Some(current); }
            }
            Inline::Hyperlink(link) => collect_common_runs(styles, paragraph_style, &link.content, result),
            _ => {}
        }
    }
}

#[cfg(test)] #[path = "../tests/unit/formatting.rs"] mod tests;

