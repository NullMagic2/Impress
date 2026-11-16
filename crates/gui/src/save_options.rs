//! Per-user save preferences. Update one key without discarding other settings.
use std::{
    io::Write,
    path::{Path, PathBuf},
};
use wxdragon::{dialogs::file_dialog::FileDialog, window::WxWidget};

pub fn settings_path() -> Result<PathBuf, String> {
    Ok(crate::data_dir::data_dir()?.join("save-options.json"))
}
pub fn load(path: &Path) -> bool {
    std::fs::read(path)
        .ok()
        .filter(|bytes| bytes.len() <= 65536)
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|value| value.get("embed_fonts").and_then(|v| v.as_bool()))
        .unwrap_or(false)
}
pub fn store(path: &Path, embed: bool) -> Result<(), String> {
    let mut settings = std::fs::read(path)
        .ok()
        .filter(|bytes| bytes.len() <= 65536)
        .and_then(|bytes| {
            serde_json::from_slice::<serde_json::Map<String, serde_json::Value>>(&bytes).ok()
        })
        .unwrap_or_default();
    settings.insert("embed_fonts".into(), embed.into());
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let bytes = serde_json::to_vec_pretty(&settings).map_err(|e| e.to_string())?;
    format::atomic_write(path, |file| {
        file.write_all(&bytes).map_err(|e| e.to_string())
    })
}
// wxDragon's vendored C++ bridge owns the customize hook with the file dialog.
unsafe extern "C" {
    fn wxd_FileDialog_EnableFontEmbedding(dialog: *mut std::ffi::c_void, previous: i32) -> i32;
    fn wxd_FileDialog_GetFontEmbedding(dialog: *const std::ffi::c_void) -> i32;
}
pub fn attach(dialog: &FileDialog, previous: bool) -> Result<(), String> {
    if unsafe { wxd_FileDialog_EnableFontEmbedding(dialog.handle_ptr().cast(), i32::from(previous)) } == 0 {
        return Err("This platform cannot add font options to the save dialog".into());
    }
    Ok(())
}
pub fn selected(dialog: &FileDialog) -> bool {
    unsafe { wxd_FileDialog_GetFontEmbedding(dialog.handle_ptr().cast()) != 0 }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preference_survives_reload_and_preserves_other_keys() {
        let dir =
            std::env::temp_dir().join(format!("impress-save-options-{}", model::NodeId::new()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("options.json");
        assert!(!load(&path));
        std::fs::write(&path, b"{\"future_option\":7}").unwrap();
        store(&path, true).unwrap();
        assert!(load(&path));
        store(&path, false).unwrap();
        assert!(!load(&path));
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(value["future_option"], 7);
        std::fs::write(&path, b"broken").unwrap();
        assert!(!load(&path));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
