//! New-document policy only. Importers continue to honor stored typography.
use model::{Document, StyleSheet, KerningMode, LigatureMode, OpenTypeTypography, StyleId, STYLE_NORMAL};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, io::Write, path::{Path, PathBuf}};

#[derive(Default, Serialize, Deserialize)]
struct NormalTemplate {
    #[serde(default)] language: Option<String>,
    #[serde(default)] shortcuts: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] styles: Option<StyleSheet>,
}

fn template_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("IMPRESS_TEMPLATE_PATH") { return Some(PathBuf::from(path)); }
    let dir = if cfg!(windows) {
        std::env::var_os("APPDATA").map(|folder| PathBuf::from(folder).join("Impress"))
            .or_else(crate::data_dir::user_data_dir)
    } else { crate::data_dir::user_data_dir() };
    dir.map(|dir| dir.join("normal-template.json"))
}

fn load_from(path: &Path) -> Result<NormalTemplate, String> {
    match fs::read(path) {
        Ok(bytes) if bytes.len() <= 16 * 1024 * 1024 => serde_json::from_slice(&bytes).map_err(|e| e.to_string()),
        Ok(_) => Err("The Normal template is too large.".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(NormalTemplate::default()),
        Err(e) => Err(e.to_string()),
    }
}

fn update(edit: impl FnOnce(&mut NormalTemplate)) -> Result<(), String> {
    let path = template_path().ok_or("No user configuration directory is available")?;
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let lock = fs::OpenOptions::new().create(true).truncate(false).read(true).write(true)
        .open(path.with_extension("lock")).map_err(|e| e.to_string())?;
    lock.lock().map_err(|e| e.to_string())?;
    let mut template = load_from(&path)?;
    edit(&mut template);
    let bytes = serde_json::to_vec_pretty(&template).map_err(|e| e.to_string())?;
    if bytes.len() > 16 * 1024 * 1024 { return Err("The Normal template is too large.".into()); }
    format::atomic_write(&path, |file| file.write_all(&bytes).map_err(|e| e.to_string()))
}

pub fn update_template(language: Option<&str>, shortcuts: Option<&BTreeMap<String, String>>) -> Result<(), String> {
    update(|template| {
        if let Some(language) = language { template.language = Some(language.to_owned()); }
        if let Some(shortcuts) = shortcuts { template.shortcuts = shortcuts.clone(); }
    })
}

pub fn set_default_styles(styles: Option<StyleSheet>) -> Result<(), String> {
    if let Some(styles) = styles.as_ref() {
        editor::merge_style_sheets(&StyleSheet::default(), styles, &BTreeMap::new())?;
    }
    update(|template| template.styles = styles)
}

pub fn new_document() -> Document {
    let mut document=Document::default();
    if let Some(normal)=document.styles.styles.get_mut(&StyleId::from(STYLE_NORMAL)) {
        normal.character.typography=Some(OpenTypeTypography {
            kerning_mode: KerningMode::Optical,
            ligatures: LigatureMode::Discretionary, contextual_ligatures:true,
            ..OpenTypeTypography::default()
        });
    }
    let template = template_path().and_then(|path| load_from(&path).ok()).unwrap_or_default();
    if let Some(styles) = template.styles {
        if let Ok(merged) = editor::merge_style_sheets(&document.styles, &styles, &document.lists) {
            document.styles = merged;
        }
    }
    if let Some(normal) = document.styles.styles.get_mut(&StyleId::from(STYLE_NORMAL)) {
        if let Some(language) = template.language { normal.character.language = Some(language); }
        normal.formatting.extend(template.shortcuts);
    }
    document
}
