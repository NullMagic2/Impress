use super::*;
#[test] fn live_session_cannot_be_claimed_by_another_window() {
    let dir = std::env::temp_dir().join(format!("impress-recovery-test-{}", NodeId::new().0));
    let first = Recovery::in_directory(dir.clone());
    fs::write(&first.path, b"test snapshot").unwrap();
    let second = Recovery::in_directory(dir.clone());
    assert!(second.candidates(&FormatRegistry::default()).is_empty());
    drop(first);
    assert_eq!(second.candidates(&FormatRegistry::default()).len(), 1);
    drop(second); let _ = fs::remove_dir_all(dir);
}
