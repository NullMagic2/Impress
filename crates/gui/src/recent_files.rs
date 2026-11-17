//! A bounded, per-user MRU list. Only completed opens/saves are recorded.
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    path::{Path, PathBuf},
};

pub const DEFAULT_LIMIT: usize = 5;
pub const MAX_LIMIT: usize = 20;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct History {
    pub limit: usize,
    pub files: Vec<PathBuf>,
    pub preferred_limit: usize,
}
impl Default for History {
    fn default() -> Self {
        Self {
            limit: DEFAULT_LIMIT,
            files: Vec::new(),
            preferred_limit: DEFAULT_LIMIT,
        }
    }
}
impl History {
    pub fn load(path: &Path) -> Self {
        let mut history: Self = std::fs::read(path)
            .ok()
            .filter(|data| data.len() <= 65536)
            .and_then(|data| serde_json::from_slice(&data).ok())
            .unwrap_or_default();
        history.normalize();
        history
    }
    fn normalize(&mut self) {
        self.limit = self.limit.min(MAX_LIMIT);
        if self.limit > 0 {
            self.preferred_limit = self.limit;
        } else if !(1..=MAX_LIMIT).contains(&self.preferred_limit) {
            self.preferred_limit = DEFAULT_LIMIT;
        }
        let mut seen = std::collections::HashSet::new();
        self.files
            .retain(|path| path.is_absolute() && seen.insert(path.clone()));
        self.files.truncate(self.limit);
    }
    pub fn remember(&mut self, path: &Path) -> Result<(), String> {
        // Canonicalization gives relative paths and file:// launchers one stable
        // identity, including symlinks. Failed/missing opens never reach here.
        let path = std::fs::canonicalize(path).map_err(|error| error.to_string())?;
        self.files.retain(|other| other != &path);
        self.files.insert(0, path);
        self.normalize();
        Ok(())
    }
    pub fn set_limit(&mut self, limit: usize) {
        self.limit = limit.min(MAX_LIMIT);
        self.normalize();
    }
    pub fn store(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        format::atomic_write(path, |file| {
            file.write_all(&bytes).map_err(|e| e.to_string())
        })
    }
}
pub fn settings_path() -> Result<PathBuf, String> {
    Ok(crate::data_dir::data_dir()?.join("recent-files.json"))
}
pub fn remember(path: &Path) -> Result<(), String> {
    let settings = settings_path()?;
    let mut history = History::load(&settings);
    history.remember(path)?;
    history.store(&settings)
}

#[cfg(test)]
#[path = "../tests/unit/recent_files.rs"]
mod tests;
