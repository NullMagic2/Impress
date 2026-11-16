//! OOXML annotations stay semantic: no deleted XML is silently accepted as final.
use super::*;
use model::{inline_text, AnchorRange, Bookmark, ReviewComment, ReviewState, TextAnchor};

#[derive(Clone, Debug, Default)]
pub(super) struct ImportReview {
    pub(super) state: ReviewState,
    bookmarks: BTreeMap<String, (String, TextAnchor)>,
    starts: BTreeMap<String, TextAnchor>,
    comments: BTreeMap<String, (String, String, Option<String>)>,
    ends: BTreeMap<String, TextAnchor>,
    active_moves: BTreeMap<String, String>,
    pub(super) previous_tabs: BTreeMap<NodeId, Vec<TabEdit>>,
    pub(super) boundaries: Vec<(NodeId, XmlNode, bool, String)>,
    pub(super) history_limit_exceeded: bool,
    history_bytes: usize,
}
impl ImportReview {
    pub(super) fn record(&mut self, node: &XmlNode, part: &str, effect: model::RevisionEffect) {
        let bytes = effect.encoded_len().unwrap_or(usize::MAX);
        if bytes > model::MAX_TRACKED_EFFECT_BYTES
            || self.history_bytes.saturating_add(bytes) > model::MAX_TRACKED_HISTORY_BYTES
            || self.state.tracked_changes.len() >= model::MAX_TRACKED_CHANGES
        {
            self.history_limit_exceeded = true;
            return;
        }
        self.history_bytes += bytes;
        let move_name = match &effect {
            model::RevisionEffect::Inline { move_name, .. } => move_name.clone(),
            _ => None,
        };
        let source_id = node
            .attr("impress:group")
            .map(|group| format!("{part}:group:{group}"))
            .or_else(|| move_name.map(|name| format!("{part}:move:{name}")))
            .unwrap_or_else(|| format!("{part}:{}:{}", node.name, node.attr("w:id").unwrap_or("")));
        if let Some(existing) = self.state.tracked_changes.values_mut().find(|change| {
            change.source_id == source_id
                && change.author == node.attr("w:author").unwrap_or("Imported author")
                && change.date.as_deref() == node.attr("w:date")
        }) {
            existing.effects.push(effect);
            return;
        }
        let id = NodeId::new();
        let order = self.state.tracked_changes.len() as u64;
        self.state.tracked_changes.insert(
            id.clone(),
            model::TrackedChange {
                id,
                source_id,
                order,
                author: node.attr("w:author").unwrap_or("Imported author").into(),
                date: node.attr("w:date").map(str::to_owned),
                status: model::RevisionStatus::Pending,
                effects: vec![effect],
            },
        );
    }
    pub(super) fn read_comments(&mut self, tree: &XmlNode) {
        if let Some(root) = find_descendant(tree, "w:comments") {
            for c in root.children_named("w:comment") {
                let Some(id) = c.attr("w:id") else {
                    continue;
                };
                let text = c
                    .children_named("w:p")
                    .map(|p| p.descendant_text())
                    .collect::<Vec<_>>()
                    .join("\n");
                self.comments.insert(
                    id.into(),
                    (
                        c.attr("w:author").unwrap_or("").into(),
                        text,
                        c.attr("w:date").map(str::to_owned),
                    ),
                );
            }
        }
    }
    pub(super) fn finish(&mut self) -> ReviewState {
        for (source, (author, text, date)) in &self.comments {
            let id = NodeId::new();
            let range = self
                .starts
                .get(source)
                .zip(self.ends.get(source))
                .map(|(start, end)| AnchorRange {
                    start: start.clone(),
                    end: end.clone(),
                });
            self.state.comments.insert(
                id.clone(),
                ReviewComment {
                    date: date.clone(),
                    replies: Vec::new(),
                    id,
                    author: author.clone(),
                    text: text.clone(),
                    range,
                    resolved: false,
                },
            );
        }
        // Keep malformed or unmatched bookmarks visible rather than pretending
        // their old character offsets survived regeneration of document.xml.
        for (_, (name, _)) in std::mem::take(&mut self.bookmarks) {
            let id = NodeId::new();
            self.state.bookmarks.insert(
                id.clone(),
                Bookmark {
                    id,
                    name,
                    range: None,
                },
            );
        }
        std::mem::take(&mut self.state)
    }
    fn marker(&mut self, node: &XmlNode, p: &Paragraph, part: &str) {
        if matches!(
            node.name.as_str(),
            "w:moveFromRangeStart" | "w:moveToRangeStart"
        ) {
            let key = if node.name == "w:moveFromRangeStart" {
                "from"
            } else {
                "to"
            };
            self.active_moves.insert(
                format!("{part}:{key}"),
                node.attr("w:name").unwrap_or("").into(),
            );
            return;
        }
        if matches!(
            node.name.as_str(),
            "w:moveFromRangeEnd" | "w:moveToRangeEnd"
        ) {
            let key = if node.name == "w:moveFromRangeEnd" {
                "from"
            } else {
                "to"
            };
            self.active_moves.remove(&format!("{part}:{key}"));
            return;
        }
        let Some(source) = node.attr("w:id") else {
            return;
        };
        let anchor = TextAnchor {
            paragraph: p.id.clone(),
            offset: inline_text(&p.content).len(),
        };
        let key = format!("{part}:{source}");
        match node.name.as_str() {
            "w:bookmarkStart" => {
                self.bookmarks.insert(
                    key,
                    (node.attr("w:name").unwrap_or("Bookmark").into(), anchor),
                );
            }
            "w:bookmarkEnd" => {
                if let Some((name, start)) = self.bookmarks.remove(&key) {
                    let id = NodeId::new();
                    self.state.bookmarks.insert(
                        id.clone(),
                        Bookmark {
                            id,
                            name,
                            range: Some(AnchorRange { start, end: anchor }),
                        },
                    );
                }
            }
            "w:commentRangeStart" => {
                self.starts.insert(source.into(), anchor);
            }
            "w:commentRangeEnd" => {
                self.ends.insert(source.into(), anchor);
            }
            "w:commentReference" => {
                self.starts
                    .entry(source.into())
                    .or_insert_with(|| anchor.clone());
                self.ends.entry(source.into()).or_insert(anchor);
            }
            _ => {}
        }
    }
}

#[derive(Default)]
struct Fields {
    instruction: Option<String>,
    display: String,
    result: bool,
    color: Option<String>,
}
fn visit(
    node: &XmlNode,
    p: &mut Paragraph,
    ctx: &ImportContext,
    original: bool,
    markers: bool,
    fields: &mut Fields,
) {
    match node.name.as_str() {
        "w:pPr" => {}
        "w:ins" | "w:moveTo" | "w:del" | "w:moveFrom" => {
            let inserted = matches!(node.name.as_str(), "w:ins" | "w:moveTo");
            let start = inline_text(&p.content).len();
            let start_index = p.content.len();
            if inserted != original {
                for n in &node.children {
                    visit(n, p, ctx, original, markers, fields);
                }
            }
            if markers && !original {
                let after = if inserted {
                    p.content[start_index..].to_vec()
                } else {
                    Vec::new()
                };
                let before = if inserted {
                    Vec::new()
                } else {
                    let mut previous = p.clone();
                    previous.content.clear();
                    let mut deleted_fields = Fields::default();
                    let isolated = snapshot_context(ctx);
                    for n in &node.children {
                        visit(
                            n,
                            &mut previous,
                            &isolated,
                            true,
                            false,
                            &mut deleted_fields,
                        );
                    }
                    previous.content.to_vec()
                };
                let end = inline_text(&p.content).len();
                let change = match node.name.as_str() {
                    "w:ins" => model::InlineRevisionKind::Insertion,
                    "w:del" => model::InlineRevisionKind::Deletion,
                    "w:moveFrom" => model::InlineRevisionKind::MoveFrom,
                    _ => model::InlineRevisionKind::MoveTo,
                };
                let move_name = if matches!(
                    change,
                    model::InlineRevisionKind::MoveFrom | model::InlineRevisionKind::MoveTo
                ) {
                    let key = if inserted { "to" } else { "from" };
                    ctx.annotations
                        .borrow()
                        .active_moves
                        .get(&format!("{}:{key}", ctx.part))
                        .cloned()
                } else {
                    None
                };
                ctx.annotations.borrow_mut().record(
                    node,
                    &ctx.part,
                    model::RevisionEffect::Inline {
                        range: AnchorRange {
                            start: TextAnchor {
                                paragraph: p.id.clone(),
                                offset: start,
                            },
                            end: TextAnchor {
                                paragraph: p.id.clone(),
                                offset: end,
                            },
                        },
                        before,
                        after,
                        change,
                        move_name,
                    },
                );
            }
        }
        "w:r" => {
            if let Some(kind) = node
                .child("w:fldChar")
                .and_then(|n| n.attr("w:fldCharType"))
            {
                match kind {
                    "begin" => {
                        fields.instruction = Some(String::new());
                        fields.display.clear();
                        fields.result = false;
                        fields.color = field_color(node);
                    }
                    "separate" => fields.result = true,
                    "end" => {
                        if let Some(instruction) = fields.instruction.take() {
                            p.content.push(Inline::Field(field_from_instruction(
                                instruction.trim(),
                                std::mem::take(&mut fields.display),
                                fields.color.take(),
                            )));
                        }
                        fields.result = false;
                    }
                    _ => {}
                }
                return;
            }
            if let Some(instruction) = node
                .child("w:instrText")
                .or_else(|| node.child("w:delInstrText"))
            {
                if let Some(buffer) = fields.instruction.as_mut() {
                    buffer.push_str(&instruction.descendant_text());
                }
                return;
            }
            if fields.instruction.is_some() {
                if fields.result {
                    fields.display.push_str(&node.descendant_text());
                    if let Some(color) = field_color(node) {
                        fields.color = Some(color);
                    }
                }
                return;
            }
            let run_start = inline_text(&p.content).len();
            let run_index = p.content.len();
            if original {
                let mut run = node.clone();
                for n in &mut run.children {
                    if n.name == "w:delText" {
                        n.name = "w:t".into();
                    }
                }
                append_run(p, &run, ctx);
            } else {
                append_run(p, node, ctx);
            }
            if markers && !original {
                if let Some(change) = node.child("w:rPr").and_then(|pr| pr.child("w:rPrChange")) {
                    let mut old_node = node.clone();
                    old_node.children.retain(|n| n.name != "w:rPr");
                    if let Some(properties) = change.child("w:rPr") {
                        old_node.children.insert(0, properties.clone());
                    }
                    let mut old = p.clone();
                    old.content.clear();
                    append_run(&mut old, &old_node, &snapshot_context(ctx));
                    ctx.annotations.borrow_mut().record(
                        change,
                        &ctx.part,
                        model::RevisionEffect::Inline {
                            range: AnchorRange {
                                start: TextAnchor {
                                    paragraph: p.id.clone(),
                                    offset: run_start,
                                },
                                end: TextAnchor {
                                    paragraph: p.id.clone(),
                                    offset: inline_text(&p.content).len(),
                                },
                            },
                            before: old.content.to_vec(),
                            after: p.content[run_index..].to_vec(),
                            change: model::InlineRevisionKind::Formatting,
                            move_name: None,
                        },
                    );
                }
            }
            if markers {
                for n in &node.children {
                    if n.name == "w:commentReference" {
                        ctx.annotations.borrow_mut().marker(n, p, &ctx.part);
                    }
                }
            }
        }
        "w:fldSimple" => p.content.push(Inline::Field(field_from_instruction(
            node.attr("w:instr").unwrap_or(""),
            node.descendant_text(),
            field_color(node),
        ))),
        "m:oMath" => p
            .content
            .push(Inline::Equation(Equation::inline(parse_math_container(
                node,
            )))),
        "w:hyperlink" => {
            // Parse into the same paragraph so marker offsets are measured in
            // the enclosing stream, then wrap exactly the newly appended slice.
            let start = p.content.len();
            for n in &node.children {
                visit(n, p, ctx, original, markers, fields);
            }
            let target = node
                .attr("w:anchor")
                .map(|v| format!("#{v}"))
                .or_else(|| node.attr("r:id").and_then(|id| ctx.links.get(id)).cloned());
            if let Some(target) = target {
                let content = p.content.split_off(start);
                p.content.push(Inline::Hyperlink(model::Hyperlink {
                    id: NodeId::new(),
                    target,
                    content: content.into(),
                }));
            }
        }
        "w:bookmarkStart"
        | "w:bookmarkEnd"
        | "w:commentRangeStart"
        | "w:commentRangeEnd"
        | "w:moveFromRangeStart"
        | "w:moveToRangeStart"
        | "w:moveFromRangeEnd"
        | "w:moveToRangeEnd" => {
            if markers {
                ctx.annotations.borrow_mut().marker(node, p, &ctx.part);
            }
        }
        // Content controls and smart tags have a visible content subtree. Import
        // that subtree and report the unsupported control semantics separately.
        "w:sdt" => {
            let control = controls::begin_inline(node, p, ctx);
            if let Some(n) = node.child("w:sdtContent") {
                for n in &n.children {
                    visit(n, p, ctx, original, markers, fields);
                }
            }
            if markers && !original {
                controls::end_inline(control, p, ctx);
            }
        }
        "w:smartTag" | "w:customXml" | "w:sdtContent" => {
            for n in &node.children {
                visit(n, p, ctx, original, markers, fields);
            }
        }
        _ => {}
    }
}
pub(super) fn parse_review_paragraph(node: &XmlNode, context: &ImportContext) -> Paragraph {
    let mut final_p = Paragraph {
        id: NodeId::new(),
        style: None,
        properties: ParagraphProperties::default(),
        content: (Vec::new()).into(),
    };
    if let Some(ppr) = node.child("w:pPr") {
        parse_paragraph_properties(&mut final_p, ppr, context);
    }
    let mut fields = Fields::default();
    for child in &node.children {
        visit(child, &mut final_p, context, false, true, &mut fields);
    }
    if let Some(instruction) = fields.instruction {
        final_p.content.push(Inline::Field(field_from_instruction(
            &instruction,
            fields.display,
            fields.color,
        )));
    }
    // A net paragraph view remains available to older review clients. Exact
    // revision records above are authoritative for Word interchange.
    let revision = find_descendant(node, "w:ins")
        .or_else(|| find_descendant(node, "w:del"))
        .or_else(|| find_descendant(node, "w:moveFrom"))
        .or_else(|| find_descendant(node, "w:moveTo"));
    if let Some(revision) = revision {
        let mut original = final_p.clone();
        original.content.clear();
        let mut fields = Fields::default();
        let isolated = snapshot_context(context);
        for child in &node.children {
            visit(child, &mut original, &isolated, true, false, &mut fields);
        }
        if let Some(instruction) = fields.instruction {
            original.content.push(Inline::Field(field_from_instruction(
                &instruction,
                fields.display,
                fields.color,
            )));
        }
        let id = NodeId::new();
        context.annotations.borrow_mut().state.revisions.insert(
            id.clone(),
            model::TextRevision {
                id,
                paragraph: final_p.id.clone(),
                author: revision
                    .attr("w:author")
                    .unwrap_or("Imported author")
                    .into(),
                date: revision.attr("w:date").map(str::to_owned),
                before: original,
            },
        );
    }
    if let Some(change) = node.child("w:pPr").and_then(|pr| pr.child("w:pPrChange")) {
        let mut before = final_p.clone();
        before.properties = ParagraphProperties::default();
        before.style = None;
        if let Some(properties) = change.child("w:pPr") {
            let mut isolated = context.clone();
            isolated.tab_edits = Default::default();
            parse_paragraph_properties(&mut before, properties, &isolated);
            if let Some(edits) = isolated.tab_edits.borrow().get(&before.id) {
                context
                    .annotations
                    .borrow_mut()
                    .previous_tabs
                    .insert(before.id.clone(), edits.clone());
            }
        }
        before.content.clear();
        let mut after = final_p.clone();
        after.content.clear();
        context.annotations.borrow_mut().record(
            change,
            &context.part,
            model::RevisionEffect::ParagraphProperties {
                paragraph: final_p.id.clone(),
                before,
                after,
            },
        );
    }
    if let Some(properties) = node.child("w:pPr").and_then(|pr| pr.child("w:rPr")) {
        for (name, inserted) in [("w:ins", true), ("w:del", false)] {
            if let Some(change) = properties.child(name) {
                context.annotations.borrow_mut().boundaries.push((
                    final_p.id.clone(),
                    change.clone(),
                    inserted,
                    context.part.clone(),
                ));
            }
        }
    }
    context
        .source_paragraphs
        .borrow_mut()
        .insert(node as *const XmlNode as usize, final_p.clone());
    context
        .source_members
        .borrow_mut()
        .insert(node as *const XmlNode as usize, final_p.id.clone());
    final_p
}
