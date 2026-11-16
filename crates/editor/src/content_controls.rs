//! Stage constraints, XML bindings and peer updates in the same undo transaction.
use super::*;
use model::{AnchorRange, ContentBinding, ContentControl, ContentControlKind, CustomXmlData};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(super) struct ControlState {
    controls: BTreeMap<NodeId, ContentControl>,
    xml: BTreeMap<String, CustomXmlData>,
}
impl ControlState {
    pub(super) fn read(document: &Document) -> Self {
        Self {
            controls: document.content_controls.clone(),
            xml: document.custom_xml.clone(),
        }
    }
    pub(super) fn apply(&self, document: &mut Document) {
        document.content_controls = self.controls.clone();
        document.custom_xml = self.xml.clone();
    }
}
fn snapshot(document: &Document, range: &AnchorRange) -> Option<Vec<Paragraph>> {
    let paragraphs = model::document_paragraphs(document);
    let first = paragraphs
        .iter()
        .position(|p| p.id == range.start.paragraph)?;
    let last = paragraphs
        .iter()
        .position(|p| p.id == range.end.paragraph)?;
    if first > last {
        return None;
    }
    paragraphs[first..=last]
        .iter()
        .enumerate()
        .map(|(index, p)| {
            let mut paragraph = (*p).clone();
            paragraph.content = slice_content(
                &p.content,
                if index == 0 { range.start.offset } else { 0 },
                if first + index == last {
                    range.end.offset
                } else {
                    paragraph_flat_text(p).len()
                },
            )
            .ok()?
            .into();
            paragraph.id = NodeId::from("control-snapshot");
            normalize_ids(&mut paragraph.content);
            Some(paragraph)
        })
        .collect()
}
fn normalize_ids(items: &mut [Inline]) {
    for item in items {
        match item {
            Inline::Text(run) => run.id = NodeId::from("control-run"),
            Inline::Hyperlink(link) => {
                link.id = NodeId::from("control-link");
                normalize_ids(&mut link.content);
            }
            _ => {}
        }
    }
}

fn rebase(control: &mut ContentControl, mutations: &[Mutation]) {
    let mut suppress = 0;
    for mutation in mutations {
        if let Mutation::AnchorsTransformed {
            mapping,
            suppress: count,
        } = mutation
        {
            mapping.control_range(&mut control.range);
            suppress = *count;
            continue;
        }
        if suppress > 0 {
            suppress -= 1;
            continue;
        }
        if let Mutation::ParagraphChanged { before, after, .. } = mutation {
            let old = paragraph_flat_text(before);
            let new = paragraph_flat_text(after);
            if control.range.start.paragraph == before.id {
                control.range.start.offset =
                    model::rebase_offset(&old, &new, control.range.start.offset, true);
            }
            if control.range.end.paragraph == before.id {
                control.range.end.offset =
                    model::rebase_offset(&old, &new, control.range.end.offset, false);
            }
        }
    }
}
fn replace_value(
    document: &mut Document,
    control: &ContentControl,
    value: &str,
    mutations: &mut Vec<Mutation>,
) -> Result<(), String> {
    if control.range.start.paragraph != control.range.end.paragraph {
        let mut staged = EditorState::new(document.clone());
        staged.selection = Some(Selection {
            anchor: DocumentPosition::new(
                control.range.start.paragraph.clone(),
                control.range.start.offset,
            ),
            focus: DocumentPosition::new(
                control.range.end.paragraph.clone(),
                control.range.end.offset,
            ),
        });
        let mut removed = Vec::new();
        staged.delete_selection_into(&mut removed)?;
        for mutation in &removed {
            if let Mutation::AnchorsTransformed { mapping, .. } = mutation {
                for current in staged.document.content_controls.values_mut() {
                    mapping.control_range(&mut current.range);
                }
            }
        }
        let mut collapsed = control.clone();
        collapsed.range.end = collapsed.range.start.clone();
        replace_value(&mut staged.document, &collapsed, value, &mut removed)?;
        let live = model::document_paragraphs(&staged.document)
            .iter()
            .map(|p| p.id.clone())
            .collect::<Vec<_>>();
        for current in staged.document.content_controls.values_mut() {
            if current.scope == model::ContentControlScope::Blocks {
                current.members.retain(|id| {
                    live.contains(id)
                        || !model::document_paragraphs(document)
                            .iter()
                            .any(|p| &p.id == id)
                });
            }
        }
        *document = staged.document;
        mutations.extend(removed);
        return Ok(());
    }
    let range = &control.range;
    let paragraphs = model::document_paragraphs(document);
    let first = paragraphs
        .iter()
        .position(|p| p.id == range.start.paragraph)
        .ok_or("The content control's start paragraph is missing")?;
    let last = paragraphs
        .iter()
        .position(|p| p.id == range.end.paragraph)
        .ok_or("The content control's end paragraph is missing")?;
    if first > last {
        return Err("The content control's range is invalid".into());
    }
    let originals = paragraphs[first..=last]
        .iter()
        .map(|p| (*p).clone())
        .collect::<Vec<_>>();
    let lines = value.split('\n').collect::<Vec<_>>();
    // Multi-paragraph rich controls retain their existing paragraph structure;
    // additional lines become explicit breaks in the final paragraph.
    for (index, before) in originals.into_iter().enumerate() {
        let start = if index == 0 { range.start.offset } else { 0 };
        let end = if first + index == last {
            range.end.offset
        } else {
            paragraph_flat_text(&before).len()
        };
        let replacement = if first + index == last {
            lines.get(index..).unwrap_or(&[]).join("\n")
        } else {
            lines.get(index).copied().unwrap_or("").into()
        };
        let (mut prefix, _) = split_content(&before.content, start)?;
        let (_, suffix) = split_content(&before.content, end)?;
        let properties = before
            .content
            .iter()
            .find_map(|i| {
                if let Inline::Text(t) = i {
                    Some(t.properties.clone())
                } else {
                    None
                }
            })
            .unwrap_or_default();
        if !replacement.is_empty() {
            prefix.push(Inline::Text(TextRun {
                id: NodeId::new(),
                text: replacement.clone(),
                style: None,
                properties,
            }));
        }
        prefix.extend(suffix);
        let mut after = before.clone();
        after.content = prefix.into();
        if after.content == before.content {
            continue;
        }
        let mapping = anchor_maps::AnchorMapping::Replace {
            paragraph: before.id.clone(),
            start,
            end,
            inserted: replacement.len(),
        };
        for current in document.content_controls.values_mut() {
            mapping.control_range(&mut current.range);
        }
        let mutation = Mutation::ParagraphChanged {
            id: before.id.clone(),
            before,
            after,
        };
        apply_mutation(document, &mutation, true)?;
        mutations.push(Mutation::AnchorsTransformed {
            mapping,
            suppress: 1,
        });
        mutations.push(mutation);
    }
    Ok(())
}
fn replace_picture(
    document: &mut Document,
    control: &ContentControl,
    value: &str,
    mutations: &mut Vec<Mutation>,
) -> Result<(), String> {
    let resource = model::decode_content_picture(value)?;
    if document
        .content_control_picture(control)
        .is_ok_and(|current| current.data == resource.data)
    {
        return Ok(());
    }
    let sections = document.sections.clone();
    let review = document.review.clone();
    let id = resource.id.clone();
    document.resources.insert(id.clone(), resource.clone());
    document.replace_content_control_picture(control, &id)?;
    mutations.push(Mutation::ResourceInserted { id, resource });
    if sections != document.sections {
        mutations.push(Mutation::SectionsReplaced {
            index: 0,
            before: sections,
            after: document.sections.clone(),
        });
    }
    if review != document.review {
        mutations.push(Mutation::ReviewChanged {
            before: review,
            after: document.review.clone(),
        });
    }
    Ok(())
}

fn validate_kind(control: &ContentControl, document: &Document) -> Result<(), String> {
    if matches!(control.kind, ContentControlKind::Picture) {
        document.content_control_picture(control)?;
        return Ok(());
    }
    let text = model::content_control_text(document, &control.range)
        .ok_or("The content control has no valid live range")?;
    control.validate_text(&text)?;
    if matches!(
        control.kind,
        ContentControlKind::PlainText { .. }
            | ContentControlKind::DropDown { .. }
            | ContentControlKind::ComboBox { .. }
            | ContentControlKind::Date { .. }
            | ContentControlKind::CheckBox { .. }
    ) && snapshot(document, &control.range).is_some_and(|ps| {
        ps.iter().any(|p| {
            p.content.iter().any(|i| {
                !matches!(
                    i,
                    Inline::Text(_) | Inline::LineBreak { .. } | Inline::Tab { .. }
                )
            })
        })
    }) {
        return Err("This content control accepts text rather than embedded objects".into());
    }
    Ok(())
}
impl EditorState {
    pub(super) fn synchronize_content_controls(
        &mut self,
        mutations: &mut Vec<Mutation>,
    ) -> Result<(), String> {
        if self.document.content_controls.is_empty()
            && !mutations
                .iter()
                .any(|m| matches!(m, Mutation::ContentControlsChanged { .. }))
        {
            return Ok(());
        }
        let mut original = self.document.clone();
        for mutation in mutations.iter().rev() {
            apply_mutation(&mut original, mutation, false)?;
        }
        let mut candidate = self.document.clone();
        let controls_before = ControlState::read(&original);
        let mut writes = BTreeMap::<(String, String), String>::new();
        let mut temporary = Vec::new();
        for old in original.content_controls.values() {
            let dormant = original.content_control_is_dormant(old);
            if !model::valid_range(&original, &old.range)
                && !model::valid_range(&candidate, &old.range)
            {
                if dormant && !candidate.content_control_is_dormant(old) {
                    candidate.content_controls.remove(&old.id);
                }
                continue;
            }
            let Some(current) = candidate.content_controls.get_mut(&old.id) else {
                if old.lock.deletion {
                    return Err("This content control cannot be deleted".into());
                }
                continue;
            };
            if !mutations
                .iter()
                .any(|mutation| matches!(mutation, Mutation::ContentControlsChanged { .. }))
            {
                let mut mapped = old.clone();
                rebase(&mut mapped, mutations);
                current.range = mapped.range;
            }
            if current.scope == model::ContentControlScope::Inline
                && current.range.start.paragraph != current.range.end.paragraph
            {
                let whole_before = old.range.start.offset == 0
                    && model::document_paragraphs(&original)
                        .iter()
                        .find(|p| p.id == old.range.end.paragraph)
                        .is_some_and(|p| old.range.end.offset == paragraph_flat_text(p).len());
                if !whole_before {
                    return Err("Use a line break inside this inline content control, or move the control to its own paragraph".into());
                }
                current.scope = model::ContentControlScope::Blocks;
            }
            let current = current.clone();
            if current.scope == model::ContentControlScope::Blocks {
                let paragraphs = model::document_paragraphs(&candidate);
                let first = paragraphs
                    .iter()
                    .position(|p| p.id == current.range.start.paragraph);
                let last = paragraphs
                    .iter()
                    .position(|p| p.id == current.range.end.paragraph);
                if let (Some(first), Some(last)) = (first, last) {
                    if first <= last {
                        let members = paragraphs[first..=last]
                            .iter()
                            .map(|p| p.id.clone())
                            .collect::<Vec<_>>();
                        if let Some(control) = candidate.content_controls.get_mut(&old.id) {
                            if old.scope == model::ContentControlScope::Inline
                                || control.members.iter().all(|id| {
                                    model::document_paragraphs(&original)
                                        .iter()
                                        .any(|p| &p.id == id)
                                })
                            {
                                control.members = members;
                            }
                        }
                    }
                }
            }
            if !model::valid_range(&candidate, &current.range) {
                if old.lock.deletion || old.lock.content {
                    return Err("This edit would remove a locked content control".into());
                }
                candidate.content_controls.remove(&old.id);
                continue;
            }
            // Restoring an old table snapshot is not a new edit to its value.
            // Its binding must refresh from current XML in the common peer pass,
            // never overwrite newer XML with its dormant before-snapshot cache.
            if dormant {
                continue;
            }
            if old.lock.content
                && !dormant
                && snapshot(&original, &old.range) != snapshot(&candidate, &current.range)
            {
                return Err("The contents of this control are locked".into());
            }
            if current.temporary
                && snapshot(&original, &old.range) != snapshot(&candidate, &current.range)
            {
                temporary.push(current.id.clone());
            }
            if matches!(current.kind, ContentControlKind::Picture) {
                let old_picture = original.content_control_picture(old)?;
                let new_picture = candidate.content_control_picture(&current)?;
                if old_picture.data != new_picture.data {
                    if old.lock.content {
                        return Err("The contents of this control are locked".into());
                    }
                    if let Some(binding) = &current.binding {
                        let value = model::encode_content_picture(new_picture);
                        candidate.set_bound_content_value(binding, &value)?;
                    }
                    if current.temporary {
                        temporary.push(current.id.clone());
                    }
                }
                continue;
            }
            let before = model::content_control_text(&original, &old.range);
            let after = model::content_control_text(&candidate, &current.range);
            if before != after {
                if current.temporary {
                    temporary.push(current.id.clone());
                }
                validate_kind(&current, &candidate)?;
                if let (Some(binding), Some(text)) = (&current.binding, after) {
                    let key = (
                        model::normalized_store_id(&binding.store_id),
                        binding.xpath.clone(),
                    );
                    let value = current.stored_value(&text);
                    if writes
                        .insert(key, value.clone())
                        .is_some_and(|previous| previous != value)
                    {
                        return Err("This edit gives conflicting values to controls bound to the same XML node".into());
                    }
                    candidate.set_bound_content_value(binding, &value)?;
                }
            }
        }
        let peers = candidate
            .content_controls
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let mut propagated = Vec::new();
        for control in peers {
            if candidate.content_control_is_dormant(&control)
                || !model::valid_range(&candidate, &control.range)
            {
                continue;
            }
            let Some(mut control) = candidate.content_controls.get(&control.id).cloned() else {
                continue;
            };
            let Some(binding) = &control.binding else {
                continue;
            };
            let value = match candidate.bound_content_value(binding) {
                Ok(value) => value,
                Err(error) => {
                    if original.custom_xml != candidate.custom_xml {
                        return Err(error);
                    }
                    continue;
                }
            };
            if matches!(control.kind, ContentControlKind::Picture) {
                replace_picture(&mut candidate, &control, &value, &mut propagated)?;
                continue;
            }
            let display = control.displayed_value(&value);
            if let ContentControlKind::Date { full_date, .. } = &mut control.kind {
                if model::control_date_parts(&value).is_some() {
                    *full_date = Some(value.clone());
                }
            }
            if let ContentControlKind::CheckBox { checked, .. } = &mut control.kind {
                *checked = matches!(value.as_str(), "true" | "1");
            }
            candidate
                .content_controls
                .insert(control.id.clone(), control.clone());
            if model::content_control_text(&candidate, &control.range).as_deref() != Some(&display)
            {
                validate_text_for_peer(&control, &display)?;
                replace_value(&mut candidate, &control, &display, &mut propagated)?;
            }
        }
        // Validate staged peer values and user edits before changing the live XML store.
        for control in candidate.content_controls.values() {
            if candidate.content_control_is_dormant(control)
                || !model::valid_range(&candidate, &control.range)
            {
                continue;
            }
            if original
                .content_controls
                .get(&control.id)
                .is_none_or(|old| {
                    model::content_control_text(&original, &old.range)
                        != model::content_control_text(&candidate, &control.range)
                })
            {
                validate_kind(control, &candidate)?;
            }
        }
        for mutation in propagated {
            apply_mutation(&mut self.document, &mutation, true)?;
            mutations.push(mutation);
        }
        for id in temporary {
            candidate.content_controls.remove(&id);
        }
        let after = ControlState::read(&candidate);
        after.apply(&mut self.document);
        if controls_before != after {
            mutations.push(Mutation::ContentControlsChanged {
                before: controls_before,
                after,
            });
        }
        Ok(())
    }
    /// Change a picker value through its control, with binding propagation and undo.
    pub fn set_content_control_value(&mut self, id: &NodeId, value: &str) -> Result<(), String> {
        let mut control = self
            .document
            .content_controls
            .get(id)
            .cloned()
            .ok_or("Content control not found")?;
        if control.lock.content {
            return Err("The contents of this control are locked".into());
        }
        if matches!(control.kind, ContentControlKind::Date { .. })
            && model::control_date_parts(value).is_none()
        {
            return Err("Choose a valid calendar date".into());
        }
        if let ContentControlKind::Date { full_date, .. } = &mut control.kind {
            *full_date = Some(value.into());
        }
        let display = control.displayed_value(value);
        control.validate_text(&display)?;
        let selection = self.selection.clone();
        let before = ControlState::read(&self.document);
        let mut changes = Vec::new();
        replace_value(&mut self.document, &control, &display, &mut changes)?;
        if let Some(current) = self.document.content_controls.get_mut(id) {
            if let ContentControlKind::CheckBox { checked, .. } = &mut current.kind {
                *checked = matches!(value, "true" | "1");
            }
            if let ContentControlKind::Date { full_date, .. } = &mut current.kind {
                *full_date = Some(value.into());
            }
        }
        let after = ControlState::read(&self.document);
        if before != after {
            changes.push(Mutation::ContentControlsChanged { before, after });
        }
        self.commit("Content control value", changes, selection, None)
    }
    /// Replace a controlled raster while keeping its geometry and data bindings.
    pub fn set_content_control_picture(&mut self, id: &NodeId, bytes: &[u8]) -> Result<(), String> {
        let control = self
            .document
            .content_controls
            .get(id)
            .cloned()
            .ok_or("Content control not found")?;
        if control.lock.content {
            return Err("The contents of this control are locked".into());
        }
        self.document.content_control_picture(&control)?;
        if bytes.len() > model::MAX_CONTENT_CONTROL_IMAGE_BYTES {
            return Err("The bound picture exceeds its size limit".into());
        }
        let resource = Resource {
            id: ResourceId::new(),
            media_type: String::new(),
            original_name: None,
            font_faces: Vec::new(),
            data: bytes.to_vec().into(),
        };
        let selection = self.selection.clone();
        let mut changes = Vec::new();
        replace_picture(
            &mut self.document,
            &control,
            &model::encode_content_picture(&resource),
            &mut changes,
        )?;
        self.commit("Content control picture", changes, selection, None)
    }
    /// Remove a wrapper while keeping its ordinary document content.
    pub fn remove_content_control(&mut self, id: &NodeId) -> Result<(), String> {
        let control = self
            .document
            .content_controls
            .get(id)
            .ok_or("Content control not found")?;
        if control.lock.deletion {
            return Err("This content control cannot be deleted".into());
        }
        let before = ControlState::read(&self.document);
        self.document.content_controls.remove(id);
        let after = ControlState::read(&self.document);
        self.commit(
            "Remove content control",
            vec![Mutation::ContentControlsChanged { before, after }],
            self.selection.clone(),
            None,
        )
    }
    /// Update the real custom XML node and all its bound visible controls together.
    pub fn set_content_binding_value(
        &mut self,
        binding: &ContentBinding,
        value: &str,
    ) -> Result<(), String> {
        let before = ControlState::read(&self.document);
        self.document.set_bound_content_value(binding, value)?;
        let after = ControlState::read(&self.document);
        self.commit(
            "Content binding value",
            vec![Mutation::ContentControlsChanged { before, after }],
            self.selection.clone(),
            None,
        )
    }
}
fn validate_text_for_peer(control: &ContentControl, value: &str) -> Result<(), String> {
    control.validate_text(value)
}
