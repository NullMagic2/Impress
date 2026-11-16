use super::*;
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "impress-recent-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn file(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, b"test").unwrap();
        path
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
fn default_history_keeps_five_most_recent_successes_and_reorders_duplicates() {
    let temp = Temp::new();
    let mut history = History::default();
    let files: Vec<_> = (0..7)
        .map(|i| temp.file(&format!("Document {i}.impress")))
        .collect();
    for file in &files {
        history.remember(file).unwrap();
    }
    assert_eq!(history.limit, 5);
    assert_eq!(
        history.files,
        files[2..].iter().rev().cloned().collect::<Vec<_>>()
    );
    history.remember(&files[4]).unwrap();
    assert_eq!(history.files[0], files[4]);
    assert_eq!(history.files.len(), 5);
    assert_eq!(
        history
            .files
            .iter()
            .filter(|file| **file == files[4])
            .count(),
        1
    );
    let before = history.files.clone();
    assert!(history.remember(&temp.0.join("missing.docx")).is_err());
    assert_eq!(history.files, before);
}
#[test]
fn remembered_limit_and_paths_survive_restart_and_can_be_disabled_or_cleared() {
    let temp = Temp::new();
    let settings = temp.0.join("preferences/recent-files.json");
    let mut history = History::load(&settings);
    history.set_limit(2);
    for name in ["α document.docx", "other.impress", "third.impress"] {
        history.remember(&temp.file(name)).unwrap();
    }
    history.store(&settings).unwrap();
    let mut loaded = History::load(&settings);
    assert_eq!(loaded.limit, 2);
    assert_eq!(loaded.files, history.files);
    loaded.files.clear();
    loaded.store(&settings).unwrap();
    assert!(History::load(&settings).files.is_empty());
    loaded.set_limit(0);
    loaded.remember(&temp.file("disabled.impress")).unwrap();
    loaded.store(&settings).unwrap();
    let loaded = History::load(&settings);
    assert_eq!(loaded.limit, 0);
    assert!(loaded.files.is_empty());
}
#[test]
fn corrupt_or_old_settings_use_defaults_and_bound_loaded_paths() {
    let temp = Temp::new();
    let settings = temp.0.join("recent.json");
    std::fs::write(&settings, b"broken").unwrap();
    assert_eq!(History::load(&settings).limit, 5);
    let path = temp.file("unique.impress");
    std::fs::write(
        &settings,
        serde_json::to_vec(
            &serde_json::json!({"limit":999,"files":[path,path,"relative.impress"]}),
        )
        .unwrap(),
    )
    .unwrap();
    let loaded = History::load(&settings);
    assert_eq!(loaded.limit, 20);
    assert_eq!(loaded.files.len(), 1);
    std::fs::write(&settings, b"{}").unwrap();
    assert_eq!(History::load(&settings).limit, 5);
}
#[cfg(unix)]
#[test]
fn symlink_launches_do_not_duplicate_the_same_document() {
    let temp = Temp::new();
    let path = temp.file("original.docx");
    let link = temp.0.join("link.docx");
    std::os::unix::fs::symlink(&path, &link).unwrap();
    let mut history = History::default();
    history.remember(&path).unwrap();
    history.remember(&link).unwrap();
    assert_eq!(history.files, vec![path]);
}
