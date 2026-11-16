use crate::{LayoutEngine, LayoutError, LayoutOptions};
use model::Document;
use render::VectorDocument;
use rayon::ThreadPoolBuilder;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Condvar, Mutex,
};
use std::thread::{self, JoinHandle};

#[derive(Clone)]
struct LayoutRequest {
    revision: u64,
    document: Arc<Document>,
}

#[derive(Debug)]
#[derive(Clone)]
pub struct PreparedLayout {
    pub work: crate::LayoutWork,
    pub reused_pages: usize,
    pub prepared_cache_hits: usize,
    pub document: Arc<VectorDocument>,
}

#[derive(Debug)]
pub struct LayoutResult {
    pub revision: u64,
    pub layout: Result<PreparedLayout, String>,
}

/// Latest-only asynchronous document layout.
///
/// Submitting a new revision replaces work that has not started yet. Results are
/// generation-tagged so the UI can discard stale layouts without ever waiting on
/// a worker. The worker's Rayon pool intentionally leaves one hardware thread for
/// event handling and wxDragon painting.
pub struct LayoutScheduler {
    mailbox: Arc<(Mutex<Option<LayoutRequest>>, Condvar)>,
    results: Arc<Mutex<Option<LayoutResult>>>,
    generation: Arc<AtomicU64>,
    stopping: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl LayoutScheduler {
    pub fn new(options: LayoutOptions) -> Result<Self, LayoutError> {
        let mailbox = Arc::new((Mutex::new(None::<LayoutRequest>), Condvar::new()));
        let results = Arc::new(Mutex::new(None));
        let worker_results = Arc::clone(&results);
        let generation = Arc::new(AtomicU64::new(0));
        let stopping = Arc::new(AtomicBool::new(false));

        let worker_mailbox = Arc::clone(&mailbox);
        let worker_generation = Arc::clone(&generation);
        let worker_stopping = Arc::clone(&stopping);
        let threads = options.worker_threads.max(1);
        // Initialization failure is returned synchronously, not published with a
        // stale generation that the latest-only UI could silently ignore.
        let pool=ThreadPoolBuilder::new().num_threads(threads)
            .thread_name(|i|format!("layout-{i}"))
            .build().map_err(|e|LayoutError::WorkerPool(e.to_string()))?;
        let worker = thread::Builder::new().name("layout-coordinator".into()).spawn(move || {
            let engine = LayoutEngine { options };

            let mut pagination = crate::incremental::LayoutCache::default();
            while !worker_stopping.load(Ordering::Acquire) {
                let request = {
                    let (lock, cv) = &*worker_mailbox;
                    let mut slot = lock.lock().expect("layout mailbox poisoned");
                    while slot.is_none() && !worker_stopping.load(Ordering::Acquire) {
                        slot = cv.wait(slot).expect("layout mailbox poisoned");
                    }
                    slot.take()
                };
                let Some(request) = request else { continue; };
                let revision = request.revision;
                let hits_before = crate::cache_totals().0;
                let result = pool.install(|| {
                    let document = engine.layout_document_cached(
                        &request.document, revision, &worker_generation, &mut pagination,
                    )?;
                    if worker_generation.load(Ordering::Acquire) != revision {
                        return Err(LayoutError::Cancelled);
                    }
                    // Pagination already counts the intact page prefixes and
                    // suffixes it reused. Comparing every glyph outline in
                    // every page again only to display a status metric slows
                    // down each keystroke in longer documents.
                    let reused_pages = (pagination.work.prefix_pages_reused
                        + pagination.work.suffix_pages_reused).min(document.pages.len());
                    if worker_generation.load(Ordering::Acquire) != revision {
                        return Err(LayoutError::Cancelled);
                    }
                    Ok(PreparedLayout { work: pagination.work, document: Arc::new(document), reused_pages, prepared_cache_hits:crate::cache_totals().0.saturating_sub(hits_before) })
                });
                if worker_generation.load(Ordering::Acquire) != revision {
                    continue;
                }
                let result = result.map_err(|e| e.to_string());
                // Replacing an unconsumed result is bounded and drops its heavy pages
                // on this worker, not while the UI drains a stale-result queue.
                let old = worker_results.lock().expect("layout results poisoned")
                    .replace(LayoutResult { revision, layout: result });
                drop(old);
            }
        }).map_err(|e| LayoutError::WorkerPool(e.to_string()))?;

        Ok(Self { mailbox, results, generation, stopping, worker: Some(worker) })
    }

    pub fn submit(&self, revision: u64, document: Arc<Document>) {
        self.generation.store(revision, Ordering::Release);
        let (lock, cv) = &*self.mailbox;
        *lock.lock().expect("layout mailbox poisoned") = Some(LayoutRequest { revision, document });
        cv.notify_one();
    }

    pub fn try_recv(&self) -> Option<LayoutResult> {
        let mut slot=self.results.try_lock().ok()?;
        // Leave obsolete results for replacement/destruction by the worker.
        if slot.as_ref().is_some_and(|r|r.revision==self.latest_revision()){slot.take()}else{None}
    }

    /// Cooperative cancellation does not wait for font/shaping or driver work.
    /// Taking the mailbox lock pairs the stop flag with the condvar wait predicate.
    pub fn request_stop(&self) {
        let (lock,cv)=&*self.mailbox;
        let mut pending=lock.lock().unwrap_or_else(|e|e.into_inner());
        self.stopping.store(true,Ordering::Release);
        self.generation.fetch_add(1,Ordering::AcqRel);
        pending.take();cv.notify_all();
    }

    pub fn latest_revision(&self) -> u64 { self.generation.load(Ordering::Acquire) }
}

impl Drop for LayoutScheduler {
    fn drop(&mut self) {
        self.request_stop();
        if let Some(worker) = self.worker.take() {
            if worker.is_finished(){let _=worker.join();}
            // A running worker owns only its own engine/cache and Arc mailboxes;
            // dropping JoinHandle detaches it safely. No UI handles or file
            // durability tasks are detached by this scheduler.
        }
    }
}
