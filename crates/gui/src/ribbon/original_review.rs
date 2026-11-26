//! Reconstruct and paginate a read-only copy for Original review mode.
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) struct OriginalPreview {
    key: (u64, u64),
    cancel: Arc<AtomicBool>,
    stage: Stage,
}

enum Stage {
    Building(Receiver<Result<OriginalLayout, String>>),
    Paginating(OriginalLayout),
    Ready,
    Failed,
}

struct OriginalLayout {
    document: Arc<Document>,
    scheduler: layout::LayoutScheduler,
    incomplete: bool,
    page: usize,
}

impl Drop for OriginalPreview {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}

/// Work backwards on a disposable editor, reusing the same conflict checks
/// as rejecting a revision. Word's exact edits supersede its net text mirror.
pub(super) fn original_document(document: Document, cancelled: &AtomicBool) -> (Document, bool) {
    use model::{RevisionEffect, RevisionStatus::Pending};
    let mut editor = EditorState::new(document);
    let mut incomplete = false;
    let history: Vec<_> = editor
        .document
        .review
        .history
        .iter()
        .rev()
        .filter(|event| event.status == Pending)
        .map(|event| event.id.clone())
        .collect();
    for id in history {
        if cancelled.load(Ordering::Acquire) {
            return (editor.document, incomplete);
        }
        incomplete |= editor.resolve_history_event(&id, false).is_err();
    }

    let review = &mut editor.document.review;
    review.track_history = false;
    review.track_text = false;
    review.track_changes = false;
    review.automatic_contents = false;
    let mut word: Vec<_> = review
        .tracked_changes
        .values()
        .filter(|change| change.status == Pending)
        .map(|change| (change.order, change.id.clone()))
        .collect();
    word.sort_by(|a, b| b.cmp(a));
    let represented: std::collections::HashSet<_> = review
        .tracked_changes
        .values()
        .filter(|change| change.status == Pending)
        .flat_map(|change| change.effects.iter())
        .filter_map(|effect| match effect {
            RevisionEffect::Inline { range, .. } => Some(range.start.paragraph.clone()),
            _ => None,
        })
        .collect();
    let paragraphs: Vec<_> = review
        .revisions
        .values()
        .filter(|change| !represented.contains(&change.paragraph))
        .map(|change| change.id.clone())
        .collect();
    for (_, id) in word {
        if cancelled.load(Ordering::Acquire) {
            return (editor.document, incomplete);
        }
        // Rejecting one half of a linked move restores both halves.
        if editor.document.review.tracked_changes[&id].status == Pending {
            incomplete |= editor.resolve_tracked_change(&id, false).is_err();
        }
    }
    for id in paragraphs {
        if cancelled.load(Ordering::Acquire) {
            return (editor.document, incomplete);
        }
        if editor.document.review.revisions.contains_key(&id) {
            incomplete |= editor.resolve_revision(&id, false).is_err();
        }
    }
    editor.document.review.comments.clear();
    (editor.document, incomplete)
}

impl OriginalPreview {
    fn start(key: (u64, u64), document: Document, page: usize) -> Result<Self, String> {
        let (sender, receiver) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancelled = cancel.clone();
        std::thread::Builder::new()
            .name("impress-original-review".into())
            .spawn(move || {
                let (original, incomplete) = original_document(document, &cancelled);
                if cancelled.load(Ordering::Acquire) {
                    return;
                }
                let result = layout::LayoutScheduler::new(layout::LayoutOptions {
                    worker_threads: 2,
                    ..Default::default()
                })
                .map(|scheduler| {
                    let document = Arc::new(original);
                    scheduler.submit_preview(
                        1,
                        document.clone(),
                        page..page + render::PAGE_WINDOW_SIZE,
                        false,
                    );
                    OriginalLayout {
                        document,
                        scheduler,
                        incomplete,
                        page,
                    }
                })
                .map_err(|error| error.to_string());
                let _ = sender.send(result);
            })
            .map_err(|error| error.to_string())?;
        Ok(Self {
            key,
            cancel,
            stage: Stage::Building(receiver),
        })
    }

    fn poll(&mut self, ribbon: &Ribbon) {
        let result = match &self.stage {
            Stage::Building(receiver) => match receiver.try_recv() {
                Ok(Ok(layout)) => {
                    self.stage = Stage::Paginating(layout);
                    return;
                }
                Ok(Err(error)) => Err(error),
                Err(TryRecvError::Disconnected) => {
                    Err("The original view could not be prepared.".into())
                }
                Err(TryRecvError::Empty) => return,
            },
            Stage::Paginating(layout) => {
                let Some(result) = layout.scheduler.try_recv() else {
                    return;
                };
                result.layout.and_then(|result| {
                    ribbon.original_reader.set_output(
                        result.document,
                        layout.document.clone(),
                        layout.page,
                    )?;
                    Ok(layout.incomplete)
                })
            }
            Stage::Ready | Stage::Failed => return,
        };
        match result {
            Ok(incomplete) => {
                self.stage = Stage::Ready;
                ribbon.original_label(if incomplete {
                    "Original · Some edits could not be restored"
                } else {
                    "Original · read-only"
                });
            }
            Err(error) => {
                self.stage = Stage::Failed;
                ribbon.original_error(error);
            }
        }
    }
}

impl Ribbon {
    pub(super) fn original_label(&self, label: &str) {
        self.ui
            .original_caption
            .set_label(&crate::i18n::text(label));
        self.ui.original_panel.layout();
    }

    fn original_error(&self, error: String) {
        self.original_label("The original view could not be prepared.");
        self.state.borrow_mut().action_status = Some(error);
    }

    pub(super) fn sync_original_preview(&self) {
        let (show, key) = {
            let state = self.state.borrow();
            (
                state.revisions.is_original(),
                (state.documents.epoch, state.editor.revision),
            )
        };
        if show && self.mode.get() != ViewMode::Graphical {
            self.set_view(ViewMode::Graphical);
        }
        if self.ui.original_panel.is_shown() != show {
            self.ui.original_panel.show(show);
            self.ui
                .canvas_container
                .show(!show && self.mode.get() != ViewMode::Typst);
            if show {
                self.install_face(&self.ui.original_close_button);
            }
            self.ui.workspace_panel.layout();
            self.focus_document();
        }
        let mut preview = self.original_preview.borrow_mut();
        if !show {
            if preview.take().is_some() {
                self.original_reader.clear_output();
            }
            return;
        }
        if preview.as_ref().is_none_or(|current| current.key != key) {
            preview.take();
            self.original_label("Preparing original view…");
            self.original_reader.clear_output();
            let (document, page) = {
                let state = self.state.borrow();
                let page = state
                    .viewport
                    .visible_pages(&state.vector_document)
                    .first()
                    .copied()
                    .unwrap_or(0);
                (state.editor.document.clone(), page)
            };
            match OriginalPreview::start(key, document, page) {
                Ok(started) => *preview = Some(started),
                Err(error) => {
                    self.original_error(error);
                    *preview = Some(OriginalPreview {
                        key,
                        cancel: Arc::new(AtomicBool::new(true)),
                        stage: Stage::Failed,
                    });
                }
            }
        }
        if let Some(preview) = preview.as_mut() {
            preview.poll(self);
        }
    }
}
