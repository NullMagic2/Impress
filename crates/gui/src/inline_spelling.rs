//! Debounced provider checks. Worker results are tied to document generation and
//! revision, and marks use source geometry rather than guessed character widths.
use crate::*;
use std::sync::mpsc::{self, Receiver, TryRecvError};
pub struct InlineSpelling {
    pub enabled: bool,
    pub language: String,
    pub error: Option<String>,
    seen: Option<(u64, u64)>,
    due: Option<Instant>,
    job: Option<Job>,
    accepted: Option<(u64, u64)>,
    issues: Vec<spelling::Issue>,
}
struct Job {
    key: (u64, u64),
    language: String,
    receiver: Receiver<Result<spelling::Report, String>>,
}
impl Default for InlineSpelling {
    fn default() -> Self {
        let settings = spelling::Settings::load();
        Self {
            enabled: settings.enabled,
            language: settings.language,
            error: None,
            seen: None,
            due: None,
            job: None,
            accepted: None,
            issues: Vec::new(),
        }
    }
}
pub fn enable(state: &mut CanvasState, language: String) -> Result<(), String> {
    let language = language.trim().to_string();
    if !spelling::valid_language(&language) {
        return Err("Choose a valid spelling language".into());
    }
    state.inline_spelling.language = language;
    state.inline_spelling.enabled = true;
    invalidate(state);
    Ok(())
}
pub fn disable(state: &mut CanvasState) {
    state.inline_spelling.enabled = false;
    state.inline_spelling.job = None;
    invalidate(state);
}
pub fn invalidate(state: &mut CanvasState) {
    state.inline_spelling.job = None;
    state.inline_spelling.seen = None;
    state.inline_spelling.accepted = None;
    state.inline_spelling.issues.clear();
    state.inline_spelling.error = None;
    refresh_overlay(state);
}
pub fn issue_at_caret(state: &CanvasState) -> Option<spelling::Issue> {
    let spelling = &state.inline_spelling;
    if !spelling.enabled
        || spelling.accepted != Some((state.layout_generation, state.editor.revision))
    {
        return None;
    }
    let caret = state.editor.selection.as_ref()?.focus.clone();
    spelling
        .issues
        .iter()
        .find(|issue| {
            issue.node == caret.node_id
                && caret.byte_offset >= issue.start
                && caret.byte_offset <= issue.end
        })
        .cloned()
}
pub fn poll(state: &mut CanvasState) {
    if !state.inline_spelling.enabled {
        return;
    }
    let key = (state.layout_generation, state.editor.revision);
    if state.inline_spelling.seen != Some(key) {
        state.inline_spelling.seen = Some(key);
        state.inline_spelling.due = Some(Instant::now() + Duration::from_millis(700));
        state.inline_spelling.accepted = None;
        state.inline_spelling.issues.clear();
        refresh_overlay(state);
    }
    let completed =
        state
            .inline_spelling
            .job
            .as_ref()
            .and_then(|job| match job.receiver.try_recv() {
                Ok(r) => Some(r),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => {
                    Some(Err("Inline spelling worker disconnected".into()))
                }
            });
    if let Some(result) = completed {
        let job = state.inline_spelling.job.take().unwrap();
        if job.key == key && job.language == state.inline_spelling.language {
            match result {
                Ok(report) => {
                    state.inline_spelling.error =
                        (!report.warnings.is_empty()).then(|| report.warnings.join("\n"));
                    if let Some(error) = &state.inline_spelling.error {
                        state.action_status = Some(error.clone());
                    }
                    state.inline_spelling.issues = report.issues;
                    state.inline_spelling.accepted = Some(key);
                    if report.limited {
                        state.action_status=Some("Inline spelling shows a bounded result set; use Spelling for a correction session.".into());
                    }
                }
                Err(e) => {
                    state.inline_spelling.error = Some(e.clone());
                    state.action_status = Some(format!("Spelling: {e}"));
                }
            }
            refresh_overlay(state);
        }
    }
    if !state.inline_spelling.enabled
        || state.inline_spelling.job.is_some()
        || !state
            .inline_spelling
            .due
            .is_some_and(|due| Instant::now() >= due)
    {
        return;
    }
    state.inline_spelling.due = None;
    let input =
        match crate::proofing::snapshot(&state.editor, &state.inline_spelling.language, false) {
            Ok(input) => input,
            Err(error) => {
                state.inline_spelling.error = Some(error);
                return;
            }
        };
    let language = state.inline_spelling.language.clone();
    let (tx, receiver) = mpsc::channel();
    let spawn = std::thread::Builder::new()
        .name("impress-inline-spelling".into())
        .spawn(move || {
            let result =
                spelling::Dictionary::load().and_then(|d| crate::proofing::check(input, &d));
            let _ = tx.send(result);
        });
    match spawn {
        Ok(_) => {
            state.inline_spelling.job = Some(Job {
                key,
                language,
                receiver,
            })
        }
        Err(e) => {
            state.inline_spelling.error = Some(e.to_string());
            state.action_status = Some(format!("Cannot start inline spelling: {e}"));
        }
    }
}
pub fn adorn(state: &mut CanvasState) {
    if !state.inline_spelling.enabled
        || state.inline_spelling.accepted != Some((state.layout_generation, state.editor.revision))
        || state.display_generation != Some(state.layout_generation)
    {
        return;
    }
    // Resolve each affected text run once. Building an editor selection for each
    // misspelling traverses the full document repeatedly on every mouse click.
    use std::collections::HashMap;
    let mut ranges = HashMap::<&str, Vec<std::ops::Range<usize>>>::new();
    for issue in &state.inline_spelling.issues {
        ranges.entry(issue.node.0.as_str()).or_default().push(issue.start..issue.end);
    }
    let mut marks = Vec::new();
    for (page_index, page) in state.vector_document.pages.iter().enumerate() {
        if !page.has_paint { continue; }
        for region in &page.hit_regions {
            let Some(ranges) = ranges.get(region.node_id.as_str()) else { continue; };
            for range in ranges {
                let Some(selection) = render::text_range_rect(page_index, region, range.clone()) else { continue; };
                let rect = selection.rect;
                let segments = (rect.width / 1.5).ceil().clamp(1.0, 2048.0) as usize;
                for i in 0..segments {
                    if marks.len() == 60_000 {
                        state.overlay.review_marks = marks;
                        return;
                    }
                    let x = rect.x + i as f32 * 1.5;
                    let y = rect.y + rect.height + if i % 2 == 0 { 0.3 } else { 1.1 };
                    marks.push(render::ColoredOverlayRect {
                        area: render::OverlayRect {
                            page_index,
                            rect: render::Rect::new(x, y, (rect.max_x() - x).clamp(0.1, 1.5), 0.6),
                        },
                        color: render::Color::rgb8(190, 42, 42),
                    });
                }
            }
        }
    }
    state.overlay.review_marks = marks;
}
