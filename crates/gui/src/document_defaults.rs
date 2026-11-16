//! New-document policy only. Importers continue to honor stored typography.
use model::{Document, KerningMode, LigatureMode, OpenTypeTypography, StyleId, STYLE_NORMAL};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::PathBuf};

#[derive(Default, Serialize, Deserialize)]
struct NormalTemplate {
    #[serde(default)] language: Option<String>,
    #[serde(default)] shortcuts: BTreeMap<String, String>,
}

fn template_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("IMPRESS_TEMPLATE_PATH") { return Some(PathBuf::from(path)); }
    std::env::var_os("APPDATA").map(|folder|PathBuf::from(folder).join("Impress").join("normal-template.json"))
}

fn load_template() -> NormalTemplate {
    template_path().and_then(|path|fs::read(path).ok())
        .filter(|bytes|bytes.len()<=64*1024)
        .and_then(|bytes|serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

pub fn update_template(language: Option<&str>, shortcuts: Option<&BTreeMap<String, String>>) -> Result<(), String> {
    let path=template_path().ok_or("No user configuration directory is available")?;
    let mut template=load_template();
    if let Some(language)=language {template.language=Some(language.to_owned());}
    if let Some(shortcuts)=shortcuts {template.shortcuts=shortcuts.clone();}
    let bytes=serde_json::to_vec_pretty(&template).map_err(|error|error.to_string())?;
    let parent=path.parent().ok_or("The template path has no parent directory")?;
    fs::create_dir_all(parent).map_err(|error|format!("Cannot create template directory: {error}"))?;
    fs::write(path,bytes).map_err(|error|format!("Cannot save the Normal template: {error}"))
}

pub fn new_document() -> Document {
    let mut document=Document::default();
    if let Some(normal)=document.styles.styles.get_mut(&StyleId::from(STYLE_NORMAL)) {
        normal.character.typography=Some(OpenTypeTypography {
            kerning_mode: KerningMode::Optical,
            ligatures: LigatureMode::Discretionary, contextual_ligatures:true,
            ..OpenTypeTypography::default()
        });
        let template=load_template();
        if let Some(language)=template.language {normal.character.language=Some(language);}
        normal.formatting.extend(template.shortcuts);
    }
    document
}
