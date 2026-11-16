//! Transactional file replacement. The destination is never opened for writing.
//! The temporary lives beside it, so rename never crosses filesystem boundaries.
use std::{fs::{self, File, OpenOptions}, io::{self, Write}, path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering}};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temporary(PathBuf);
impl Drop for Temporary { fn drop(&mut self) { let _ = fs::remove_file(&self.0); } }

/// Export, validate and sync a complete file before replacing the destination.
/// Errors (including a failed rename on a locked Windows file) leave the old
/// destination intact. The callback must finish any archive writer before return.
/// Atomic replacement is not a universal power-loss guarantee on every filesystem.
pub fn atomic_write(path: &Path, export: impl FnOnce(&mut File) -> Result<(), String>) -> Result<(), String> {
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let name = path.file_name().ok_or("The save path has no file name")?;
    let metadata = match fs::symlink_metadata(path) {
        Ok(m) => {
            if !m.is_file() || m.file_type().is_symlink() { return Err("Save to a regular file, not a directory or symbolic link".into()); }
            if m.permissions().readonly() { return Err("The destination is read-only; use Save As".into()); }
            Some(m)
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("Cannot inspect destination: {e}")),
    };
    let mut chosen = None;
    for _ in 0..64 {
        let mut tmp_name = std::ffi::OsString::from("."); tmp_name.push(name);
        tmp_name.push(format!(".impress-save-{}-{}.tmp", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let tmp = parent.join(tmp_name);
        match OpenOptions::new().read(true).write(true).create_new(true).open(&tmp) {
            Ok(f) => { chosen = Some((Temporary(tmp), f)); break; }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("Cannot create save file beside {}: {e}", path.display())),
        }
    }
    let (temporary, mut file) = chosen.ok_or("Could not allocate a unique save file")?;
    export(&mut file)?;
    file.flush().map_err(|e| format!("Cannot flush export: {e}"))?;
    if file.metadata().map_err(|e| e.to_string())?.len() == 0 { return Err("Exporter produced an empty file".into()); }
    if let Some(old) = metadata { file.set_permissions(old.permissions()).map_err(|e| format!("Cannot preserve file permissions: {e}"))?; }
    file.sync_all().map_err(|e| format!("Cannot sync export: {e}"))?;
    drop(file); // Windows requires the temporary exporter handle to be closed.
    fs::rename(&temporary.0, path).map_err(|e| format!("Cannot replace {} (original preserved): {e}", path.display()))?;
    // Best effort directory durability after successful replacement. A directory
    // sync failure must not be reported as an export failure after committing.
    #[cfg(unix)] if let Ok(directory) = File::open(parent) { let _ = directory.sync_all(); }
    Ok(())
}

#[cfg(test)] #[path = "../tests/unit/atomic.rs"] mod tests;
