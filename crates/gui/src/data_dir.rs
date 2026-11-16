//! Where Impress keeps per-user files: recovery copies and the spelling
//! dictionary. A portable copy keeps them next to the executable, as before;
//! a system install (the .deb puts Impress in /usr/lib/impress) cannot write
//! there, so they go to the user's data folder instead.
use std::path::{Path, PathBuf};

/// The folder for per-user files.
pub fn data_dir() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe_dir = exe.parent().ok_or("Executable directory not found")?;
    Ok(choose(exe_dir, user_data_dir))
}

/// `exe_dir` when Impress may write there, else the user's data folder.
fn choose(exe_dir: &Path, user_data: impl FnOnce() -> Option<PathBuf>) -> PathBuf {
    if writable(exe_dir) { return exe_dir.to_owned(); }
    user_data().unwrap_or_else(|| exe_dir.to_owned())
}

/// Whether a file can be created in `dir` (permissions alone do not say so
/// for every file system, so this tries).
fn writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".impress-write-test-{}", std::process::id()));
    let created = std::fs::OpenOptions::new().write(true).create_new(true).open(&probe).is_ok();
    if created { let _ = std::fs::remove_file(&probe); }
    created
}

/// `$XDG_DATA_HOME/impress` (`~/.local/share/impress`), or
/// `%LOCALAPPDATA%\Impress` on Windows.
pub(crate) fn user_data_dir() -> Option<PathBuf> {
    if cfg!(windows) { return std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("Impress")); }
    let base = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    Some(base.join("impress"))
}

#[cfg(test)] #[path = "../tests/unit/data_dir.rs"] mod tests;
