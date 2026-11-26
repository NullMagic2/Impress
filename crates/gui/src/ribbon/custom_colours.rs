//! The user's shared swatches, independent of any document.
use std::path::Path;
pub(super) const LIMIT: usize = 16;
fn normalized(values: &[String]) -> Vec<String> {
    let mut result = Vec::new();
    for value in values {
        if value.len() != 7 || !value.starts_with('#') || !value[1..].bytes().all(|b| b.is_ascii_hexdigit()) { continue; }
        let value = value.to_ascii_uppercase();
        if !result.contains(&value) { result.push(value); }
        if result.len() == LIMIT { break; }
    }
    result
}
fn read(path: &Path) -> Vec<String> {
    std::fs::read(path).ok().filter(|bytes| bytes.len() <= 65536)
        .and_then(|bytes| serde_json::from_slice::<Vec<String>>(&bytes).ok())
        .map(|values| normalized(&values)).unwrap_or_default()
}
fn write(path: &Path, values: &[String]) -> Result<(), String> {
    use std::io::Write;
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
    let bytes = serde_json::to_vec(&normalized(values)).map_err(|e| e.to_string())?;
    format::atomic_write(path, |file| file.write_all(&bytes).map_err(|e| e.to_string()))
}
fn path() -> Result<std::path::PathBuf, String> { Ok(crate::data_dir::data_dir()?.join("custom-colors.json")) }
pub(super) fn load() -> Vec<String> { path().map(|path| read(&path)).unwrap_or_default() }
pub(super) fn add(value: &str) -> Result<(), String> {
    let path = path()?; let mut values = read(&path);
    let Some(value) = normalized(&[value.to_owned()]).pop() else { return Err("Invalid color".into()); };
    if values.contains(&value) { return Ok(()); }
    if values.len() == LIMIT { return Err("Remove a custom color before adding another.".into()); }
    values.push(value); write(&path, &values)
}
pub(super) fn remove(value: &str) -> Result<(), String> {
    let path = path()?; let mut values = read(&path);
    values.retain(|color| !color.eq_ignore_ascii_case(value)); write(&path, &values)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn custom_palette_persists_valid_unique_swatches_and_handles_bad_files() {
        let root = std::env::temp_dir().join(format!("impress-custom-colors-{}", model::NodeId::new()));
        let path = root.join("colors.json");
        write(&path, &["#abcdef".into(), "bad".into(), "#ABCDEF".into(), "#123456".into()]).unwrap();
        assert_eq!(read(&path), ["#ABCDEF", "#123456"]);
        write(&path, &["#123456".into()]).unwrap(); assert_eq!(read(&path), ["#123456"]);
        std::fs::write(&path, "broken").unwrap(); assert!(read(&path).is_empty());
        let many: Vec<_> = (0..25).map(|n| format!("#{n:06X}")).collect();
        assert_eq!(normalized(&many).len(), LIMIT);
        std::fs::remove_dir_all(root).unwrap();
    }
}
