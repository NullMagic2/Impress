//! Periodic native-format recovery snapshots, never writes to the user's file.
//! A session lock survives as long as its worker does. Stale snapshots are only
//! offered after obtaining that lock, including when several Impress windows run.
use crate::file_io::{load_document, save_document};
use model::{Document, NodeId};
use editor::EditorState;
use format::{atomic_write, FormatRegistry};
use std::{fs::{self, File, OpenOptions}, io::Write, path::{Path, PathBuf},
    sync::{Arc, atomic::{AtomicU64, Ordering}}, thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH}};
const PERIOD: Duration = Duration::from_secs(30);
#[derive(serde::Serialize, serde::Deserialize)]
struct Metadata { original_path: Option<PathBuf>, revision: u64, saved_at: u64,
    /// Absent in snapshots written before 0.8.3.
    #[serde(default)] has_content: Option<bool> }
pub struct Candidate {
    pub path: PathBuf,
    /// Document name ("Untitled document" when it was never saved).
    pub title: String,
    /// Where the original lives, if it had been saved.
    pub original: Option<PathBuf>,
    /// Seconds since the snapshot was written.
    pub age_seconds: u64,
    pub revision: Option<u64>,
    _lock: File,
}
pub struct Recovery {
    directory: PathBuf,
    path: PathBuf,
    lease: Option<Arc<File>>,
    job: Option<JoinHandle<Result<(), String>>>,
    epoch: Arc<AtomicU64>,
    last_attempt: Instant,
    last_revision: Option<u64>,
    clear_pending: bool,
    recovered: Option<Candidate>,
    pub error: Option<String>,
}
fn lock(path: &Path) -> Result<File, String> {
    let file = OpenOptions::new().read(true).write(true).create(true).truncate(false)
        .open(path.with_extension("lock")).map_err(|e| e.to_string())?;
    file.try_lock().map_err(|e| e.to_string())?;
    Ok(file)
}
fn remove_snapshot(path: &Path) {
    let _ = fs::remove_file(path);
    let _ = fs::remove_file(path.with_extension("json"));
    // Keep the tiny sidecar lock inode. Removing a lock file while another
    // process waits on it could allow two owners of different inodes.
}
impl Recovery {
    pub fn new() -> Self {
        let directory = std::env::var_os("IMPRESS_RECOVERY_DIR").map(PathBuf::from)
            .unwrap_or_else(|| crate::data_dir::data_dir().unwrap_or_else(|_| PathBuf::from(".")).join("recovery"));
        Self::in_directory(directory)
    }
    fn in_directory(directory: PathBuf) -> Self {
        let path = directory.join(format!("{}.impress", NodeId::new().0));
        let lease = fs::create_dir_all(&directory).map_err(|e| e.to_string()).and_then(|_| lock(&path));
        let (lease, error) = match lease { Ok(file) => (Some(Arc::new(file)), None),
            Err(error) => (None, Some(format!("Recovery unavailable in {}: {error}", directory.display()))) };
        Self { directory, path, lease, error, job: None, epoch: Arc::new(AtomicU64::new(0)),
            last_attempt: Instant::now(), last_revision: None, clear_pending: false, recovered: None }
    }
    /// Snapshots to offer. Untitled documents without content are deleted
    /// instead: there is nothing in them to restore.
    pub fn candidates(&self, formats: &FormatRegistry) -> Vec<Candidate> {
        let Ok(entries) = fs::read_dir(&self.directory) else { return Vec::new(); };
        let mut paths: Vec<_> = entries.filter_map(Result::ok).map(|entry| entry.path())
            .filter(|p| p.extension().is_some_and(|e| e == "impress") && p != &self.path
                && fs::symlink_metadata(p).is_ok_and(|m| m.is_file() && !m.file_type().is_symlink())).collect();
        paths.sort_by_key(|p| std::cmp::Reverse(fs::metadata(p).and_then(|m| m.modified()).unwrap_or(UNIX_EPOCH)));
        paths.into_iter().take(64).filter_map(|path| {
            let lease = lock(&path).ok()?;
            let metadata = fs::read(path.with_extension("json")).ok()
                .filter(|data| data.len() <= 65536).and_then(|data| serde_json::from_slice::<Metadata>(&data).ok());
            let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
            let modified = fs::metadata(&path).and_then(|m| m.modified()).ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(now, |d| d.as_secs());
            let (original, revision, saved_at, has_content) = match metadata {
                Some(m) => (m.original_path, Some(m.revision), m.saved_at, m.has_content),
                None => (None, None, modified, None),
            };
            // An untitled copy that turns out blank is not offered. A stored
            // "has content" is checked again: what counts as content has
            // changed (an equation of empty boxes no longer does).
            let blank = original.is_none() && (has_content == Some(false)
                || load_document(&path, formats).is_ok_and(|document| !document.has_content()));
            if blank { remove_snapshot(&path); return None; }
            let title = original.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "Untitled document".into());
            Some(Candidate { path, title, original, age_seconds: now.saturating_sub(saved_at), revision, _lock: lease })
        }).collect()
    }
    pub fn restore(&mut self, candidate: Candidate, formats: &FormatRegistry) -> Result<Document, String> {
        let document = load_document(&candidate.path, formats)?;
        self.recovered = Some(candidate); // retain the claim until save/discard
        self.last_attempt = Instant::now() - PERIOD;
        Ok(document)
    }
    /// Deletes a snapshot the user chose to discard.
    pub fn discard(&self, candidate: Candidate) { remove_snapshot(&candidate.path); }
    /// Must be called only after a successful explicit save or an explicit
    /// discard. A cancelled/failed save must leave every recovery copy intact.
    pub fn mark_clean(&mut self) {
        self.epoch.fetch_add(1, Ordering::AcqRel);
        self.clear_pending = true;
        self.last_revision = None;
        self.last_attempt = Instant::now();
        if let Some(candidate) = self.recovered.take() { remove_snapshot(&candidate.path); }
        if self.job.is_none() { remove_snapshot(&self.path); self.clear_pending = false; }
    }
    pub fn poll_snapshot(&mut self, editor: &EditorState, source: Option<&Path>, formats: &Arc<FormatRegistry>, snapshot:Option<Arc<Document>>) {
        if self.job.as_ref().is_some_and(|job| job.is_finished()) {
            let result = self.job.take().expect("finished job").join().unwrap_or_else(|_| Err("Recovery worker panicked".into()));
            if let Err(error) = result { self.error = Some(error); self.last_revision = None; }
            else { self.error = None; }
        }
        if self.job.is_some() { return; }
        if self.clear_pending { remove_snapshot(&self.path); self.clear_pending = false; }
        if !editor.dirty || self.lease.is_none() || self.last_revision == Some(editor.revision)
            || self.last_attempt.elapsed() < PERIOD { return; }
        self.last_attempt = Instant::now();
        self.last_revision = Some(editor.revision);
        let document = snapshot.unwrap_or_else(||Arc::new(editor.document.clone()));
        let has_content = document.has_content();
        if source.is_none() && !has_content { remove_snapshot(&self.path); return; }
        let path = self.path.clone();
        let formats = Arc::clone(formats);
        let epoch = Arc::clone(&self.epoch);
        let expected = epoch.load(Ordering::Acquire);
        let lease = Arc::clone(self.lease.as_ref().expect("available lease"));
        let metadata = Metadata { original_path: source.map(Path::to_owned), revision: editor.revision,
            saved_at: SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(), has_content: Some(has_content) };
        match thread::Builder::new().name("impress-recovery".into()).spawn(move || {
            let _lease = lease;
            if epoch.load(Ordering::Acquire) != expected { return Ok(()); }
            let result = save_snapshot(&path, &document, &formats, &metadata);
            if epoch.load(Ordering::Acquire) != expected { remove_snapshot(&path); }
            result
        }) {
            Ok(job) => self.job = Some(job),
            Err(error) => { self.error = Some(format!("Could not start recovery worker: {error}")); self.last_revision = None; }
        }
    }
    /// Called during a non-vetoable OS shutdown. A distinct destination avoids
    /// racing an older periodic snapshot; incomplete files remain temp files.
    pub fn emergency_snapshot(&self, editor: &EditorState, source: Option<&Path>, formats: &FormatRegistry) -> Result<(), String> {
        let has_content = editor.document.has_content();
        if !editor.dirty || (source.is_none() && !has_content) { return Ok(()); }
        fs::create_dir_all(&self.directory).map_err(|e| e.to_string())?;
        let path = self.directory.join(format!("{}.impress", NodeId::new().0));
        let _lease = lock(&path)?;
        save_snapshot(&path, &editor.document, formats, &Metadata { original_path: source.map(Path::to_owned), revision: editor.revision,
            saved_at: SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(), has_content: Some(has_content) })
    }
}
fn save_snapshot(path: &Path, doc: &Document, formats: &FormatRegistry, metadata: &Metadata) -> Result<(), String> {
    save_document(path, doc, formats).map_err(|e| format!("Recovery write failed: {e}"))?;
    let bytes = serde_json::to_vec(metadata).map_err(|e| e.to_string())?;
    // The native snapshot is already recoverable even if its descriptive sidecar
    // cannot be written. Startup tolerates a missing/interrupted metadata file.
    atomic_write(&path.with_extension("json"), |file| file.write_all(&bytes).map_err(|e| e.to_string()))
}
#[cfg(test)] #[path = "../tests/unit/recovery.rs"] mod tests;
