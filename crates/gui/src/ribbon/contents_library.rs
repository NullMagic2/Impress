//! Named contents settings shared by this user's documents.
use model::ContentsOptions;
use std::{collections::BTreeMap, fs, io::{ErrorKind, Write}, path::{Path, PathBuf}};

pub(super) type Templates = BTreeMap<String, ContentsOptions>;

pub(super) fn path() -> Result<PathBuf, String> {
    crate::data_dir::user_data_dir().ok_or_else(|| "User data folder unavailable.".to_string())
        .map(|dir| dir.join("contents-templates.json"))
}

pub(super) fn load(path: &Path) -> Result<Templates, String> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| e.to_string()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(Templates::new()),
        Err(e) => Err(e.to_string()),
    }
}

pub(super) fn matching_name<'a>(templates: &'a Templates, name: &str) -> Option<&'a str> {
    let folded = name.to_lowercase();
    templates.keys().find(|key| key.to_lowercase() == folded).map(String::as_str)
}

/// Check the edited entry under the lock so another window's saves survive.
pub(super) fn change(path: &Path, name: &str, expected: Option<&ContentsOptions>, value: Option<&ContentsOptions>) -> Result<Templates, String> {
    if let Some(parent) = path.parent() { fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
    let lock = fs::OpenOptions::new().create(true).truncate(false).read(true).write(true)
        .open(path.with_extension("lock")).map_err(|e| e.to_string())?;
    lock.lock().map_err(|e| e.to_string())?;
    let mut entries = load(path)?;
    let key = matching_name(&entries, name).unwrap_or(name).to_owned();
    if entries.get(&key) != expected {
        return Err("This template changed in another window. Reopen the template list.".into());
    }
    if let Some(options) = value { entries.insert(key, options.clone()); } else { entries.remove(&key); }
    let data = serde_json::to_vec_pretty(&entries).map_err(|e| e.to_string())?;
    format::atomic_write(path, |file| file.write_all(&data).map_err(|e| e.to_string()))?;
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn named_templates_survive_restart_and_independent_window_edits() {
        let dir = std::env::temp_dir().join(format!("contents-library-{}", model::NodeId::new()));
        let path = dir.join("library.json");
        let book = ContentsOptions { title: "Chapters".into(), ..Default::default() };
        let short = ContentsOptions { max_heading_level: 1, ..book.clone() };
        assert!(load(&path).unwrap().is_empty());
        change(&path, "Books", None, Some(&book)).unwrap();
        change(&path, "Brief", None, Some(&short)).unwrap();
        assert!(change(&path, "BOOKS", None, Some(&short)).is_err());
        change(&path, "books", Some(&book), Some(&short)).unwrap();
        assert!(change(&path, "Books", Some(&book), None).is_err());
        assert_eq!(load(&path).unwrap()["Books"], short);
        change(&path, "Books", Some(&short), None).unwrap();
        assert_eq!(load(&path).unwrap(), Templates::from([("Brief".into(), short)]));
        fs::remove_dir_all(dir).unwrap();
    }
}
