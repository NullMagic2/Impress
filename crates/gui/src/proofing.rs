//! Language-aware proofing snapshots. Text spans keep their document offsets;
//! formatting and selection clipping happen before any provider sees the text.
use crate::spelling::{self, Dictionary, Report};
use editor::EditorState;
use model::{Inline, NodeId, Paragraph, StyleSheet};
use std::collections::BTreeMap;
use unicode_script::{Script, UnicodeScript};
use unicode_segmentation::UnicodeSegmentation;

const MAX_PARAGRAPH_BYTES: usize = 65_536;
const MAX_DOCUMENT_BYTES: usize = 2 * 1024 * 1024;
const MAX_ISSUES: usize = 256;

pub struct Fragment {
    node: NodeId,
    start: usize,
    text: String,
    language: String,
}
#[derive(Default)]
pub struct Input {
    fragments: Vec<Fragment>,
    limited: bool,
}

/// A stable snapshot of all proofable text, or exactly the selected spans.
pub fn snapshot(
    editor: &EditorState,
    fallback: &str,
    selection_only: bool,
) -> Result<Input, String> {
    let ranges = if selection_only {
        if editor
            .selection
            .as_ref()
            .is_none_or(|selection| selection.is_collapsed())
        {
            return Err("Select the text you want to spellcheck first.".into());
        }
        Some(
            editor
                .selected_paragraph_spans()
                .into_iter()
                .map(|(node, start, end)| (node, (start, end)))
                .collect::<BTreeMap<_, _>>(),
        )
    } else {
        None
    };
    let document = &editor.document;
    let default_language = document.metadata.language.as_deref().unwrap_or(fallback);
    let mut input = Input::default();
    let mut bytes = 0;
    for paragraph in model::document_paragraphs(document) {
        let length = model::inline_text(&paragraph.content).len();
        let range = match &ranges {
            Some(ranges) => match ranges.get(&paragraph.id) {
                Some(range) => *range,
                None => continue,
            },
            None => (0, length),
        };
        if range.1.saturating_sub(range.0) > MAX_PARAGRAPH_BYTES {
            input.limited = true;
            continue;
        }
        if document
            .styles
            .resolve_formatting(paragraph.style.as_ref())
            .get("lng_no_proof")
            .is_some_and(|value| value == "true")
        {
            continue;
        }
        bytes += range.1.saturating_sub(range.0);
        if bytes > MAX_DOCUMENT_BYTES {
            input.limited = true;
            break;
        }
        walk(
            &paragraph.content,
            paragraph,
            &document.styles,
            default_language,
            range,
            &mut 0,
            &mut input.fragments,
        );
    }
    Ok(input)
}

/// The common proofing language of the selection, or None for mixed languages.
pub fn selection_language(editor: &EditorState, fallback: &str) -> Option<String> {
    if editor
        .selection
        .as_ref()
        .is_some_and(|selection| !selection.is_collapsed())
    {
        let input = snapshot(editor, fallback, true).ok()?;
        let first = input.fragments.first()?.language.clone();
        input
            .fragments
            .iter()
            .all(|fragment| fragment.language.eq_ignore_ascii_case(&first))
            .then_some(first)
    } else {
        if let Some(language)=editor.typing_properties.language.as_ref() {return Some(language.clone());}
        let focus=editor.focus_position()?;
        let paragraph=model::document_paragraphs(&editor.document).into_iter().find(|p|p.id==focus.node_id)?;
        let mut fragments=Vec::new();
        let language=editor.document.metadata.language.as_deref().unwrap_or(fallback);
        walk(&paragraph.content,paragraph,&editor.document.styles,language,
            (0,model::inline_text(&paragraph.content).len()),&mut 0,&mut fragments);
        fragments.iter().find(|f|f.start<=focus.byte_offset && focus.byte_offset<f.start+f.text.len())
            .or_else(||fragments.last()).map(|f|f.language.clone())
            .or_else(||Some(editor.formatting_summary().character.language.unwrap_or_else(||language.to_owned())))
    }
}

fn walk(
    items: &[Inline],
    paragraph: &Paragraph,
    styles: &StyleSheet,
    fallback: &str,
    range: (usize, usize),
    offset: &mut usize,
    fragments: &mut Vec<Fragment>,
) {
    for item in items {
        match item {
            Inline::Hyperlink(link) => walk(
                &link.content,
                paragraph,
                styles,
                fallback,
                range,
                offset,
                fragments,
            ),
            Inline::Text(run) => {
                let properties = styles.resolve_run(
                    paragraph.style.as_ref(),
                    run.style.as_ref(),
                    &run.properties,
                );
                let end = *offset + run.text.len();
                if !properties
                    .visual_effects
                    .get("lng_no_proof")
                    .is_some_and(|value| value == "true")
                {
                    let start = (*offset).max(range.0);
                    let clipped_end = end.min(range.1);
                    if start < clipped_end {
                        if let Some(text) = run.text.get(start - *offset..clipped_end - *offset) {
                            let explicit = run.properties.language.as_deref()
                                .or_else(|| style_language(styles, run.style.as_ref()))
                                .or_else(|| style_language(styles, paragraph.style.as_ref()));
                            if let Some(language) = explicit {
                                push(fragments, &paragraph.id, start, text, language);
                            } else {
                                push_inferred(fragments, &paragraph.id, start, text,
                                    properties.language.as_deref().unwrap_or(fallback), fallback);
                            }
                        }
                    }
                }
                *offset = end;
            }
            _ => {
                let text = match item {
                    Inline::Tab { .. } => "\t",
                    Inline::LineBreak { .. } => "\n",
                    _ => "\u{fffc}",
                };
                let end = *offset + text.len();
                // Keep boundaries between words and inline objects. Markers
                // occupy the same UTF-8 bytes as the editor's flat text.
                if *offset >= range.0 && end <= range.1 {
                    push(fragments, &paragraph.id, *offset, text, fallback);
                }
                *offset = end;
            }
        }
    }
}
// The built-in Normal style carries an English default. It is not an
// explicit instruction to proof Greek letters with an English dictionary.
// Direct language formatting and language-specific named styles take priority.
fn style_language<'a>(styles: &'a StyleSheet, id: Option<&model::StyleId>) -> Option<&'a str> {
    let mut current=id.cloned();
    for _ in 0..styles.styles.len() {
        let id=current?;
        if id.0==model::STYLE_NORMAL {return None;}
        let style=styles.get(&id)?;
        if let Some(language)=style.character.language.as_deref(){return Some(language);}
        current=style.based_on.clone();
    }
    None
}
fn greek_language(tag:&str)->bool {tag.split(['-','_']).next().is_some_and(|base|base.eq_ignore_ascii_case("el"))}
fn push_inferred(fragments:&mut Vec<Fragment>,node:&NodeId,start:usize,text:&str,language:&str,fallback:&str) {
    for (offset,span) in text.split_word_bound_indices() {
        let greek=span.chars().any(|ch|ch.is_alphabetic() && ch.script()==Script::Greek);
        let chosen=if greek {
            if greek_language(language){language}else if greek_language(fallback){fallback}else{"el-GR"}
        }else if !span.chars().any(char::is_alphabetic) {
            fragments.last().filter(|last|last.node==*node && last.start+last.text.len()==start+offset)
                .map_or(language,|last|last.language.as_str())
        }else{language};
        // Own the choice before push may extend or reallocate the fragment list.
        let chosen=chosen.to_owned();
        push(fragments,node,start+offset,span,&chosen);
    }
}

fn push(fragments: &mut Vec<Fragment>, node: &NodeId, start: usize, text: &str, language: &str) {
    if let Some(last) = fragments.last_mut() {
        if last.node == *node
            && last.start + last.text.len() == start
            && last.language.eq_ignore_ascii_case(language)
        {
            last.text.push_str(text);
            return;
        }
    }
    fragments.push(Fragment {
        node: node.clone(),
        start,
        text: text.to_string(),
        language: language.to_string(),
    });
}

fn installed_language(language: &str, installed: &[String]) -> String {
    spelling::dictionary_language(language, installed).unwrap_or_else(|| language.to_string())
}

pub fn check(input: Input, dictionary: &Dictionary) -> Result<Report, String> {
    let installed = spelling::languages().unwrap_or_default();
    let mut groups = BTreeMap::<String, Vec<(NodeId, String)>>::new();
    let mut positions = BTreeMap::new();
    let mut normalized = Vec::new();
    for fragment in input.fragments {
        let language = installed_language(&fragment.language, &installed);
        push(
            &mut normalized,
            &fragment.node,
            fragment.start,
            &fragment.text,
            &language,
        );
    }
    for (order, fragment) in normalized.into_iter().enumerate() {
        if !fragment.text.chars().any(char::is_alphabetic) {
            continue;
        }
        let language = fragment.language;
        let temporary = NodeId::new();
        positions.insert(temporary.clone(), (fragment.node, fragment.start, order));
        groups
            .entry(language)
            .or_default()
            .push((temporary, fragment.text));
    }
    let mut report = Report {
        limited: input.limited,
        ..Default::default()
    };
    let mut issues = Vec::new();
    for (language, paragraphs) in groups {
        match spelling::check(&language, paragraphs, dictionary) {
            Ok(result) => {
                report.limited |= result.limited;
                for mut issue in result.issues {
                    if let Some((node, start, order)) = positions.get(&issue.node) {
                        issue.node = node.clone();
                        issue.start += start;
                        issue.end += start;
                        issues.push((*order, issue));
                    }
                }
            }
            Err(error) => report.warnings.push(format!("{language}: {error}")),
        }
    }
    issues.sort_by_key(|(order, issue)| (*order, issue.start));
    if issues.len() > MAX_ISSUES {
        report.limited = true;
        issues.truncate(MAX_ISSUES);
    }
    report.issues = issues.into_iter().map(|(_, issue)| issue).collect();
    Ok(report)
}

#[cfg(test)]
#[path = "../tests/unit/proofing.rs"]
mod tests;
