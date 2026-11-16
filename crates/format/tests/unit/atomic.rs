use super::*;
fn dir() -> PathBuf {
    let p = std::env::temp_dir().join(format!("impress-atomic-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    fs::create_dir(&p).unwrap(); p
}
#[test] fn export_failure_preserves_original_and_removes_temporary() {
    let d=dir(); let p=d.join("drawing.impress"); fs::write(&p,b"original").unwrap();
    let result=atomic_write(&p, |f| { f.write_all(b"partial").unwrap(); Err("injected export failure".into()) });
    assert!(result.is_err()); assert_eq!(fs::read(&p).unwrap(), b"original");
    assert_eq!(fs::read_dir(&d).unwrap().count(),1); fs::remove_dir_all(d).unwrap();
}
