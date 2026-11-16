//! Dictionary installation plans, private staging, and verified publication.
use serde::{Deserialize, Serialize};
#[cfg(windows)]
use sha2::{Digest, Sha256};
use std::{path::PathBuf, process::Command, sync::mpsc::Sender};

#[cfg(target_os = "linux")]
#[path = "dictionary_install_linux.rs"]
mod platform;
#[cfg(windows)]
#[path = "dictionary_install_windows.rs"]
mod platform;

#[path = "dictionary_receipts.rs"]
mod receipts;

#[cfg(windows)]
const DOWNLOAD_ROOT: &str = "https://raw.githubusercontent.com/LibreOffice/dictionaries";
#[cfg(windows)]
const MAX_DOWNLOAD_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Scope {
    Local,
    System,
}
impl Scope {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Local => "Installed by User",
            Self::System => "System-wide",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Operation {
    Install,
    Remove,
}
impl Operation {
    pub(crate) fn title(self) -> &'static str {
        match self {
            Self::Install => "Installation",
            Self::Remove => "Removal",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct InstalledDictionary {
    pub tag: String,
    pub scope: Scope,
    pub removable: bool,
}

pub(crate) struct Inventory {
    pub dictionaries: Vec<InstalledDictionary>,
    pub warning: Option<String>,
}

fn catalog() -> Result<Catalog, String> {
    serde_json::from_str(include_str!("../assets/dictionary-catalog.json"))
        .map_err(|_| "The dictionary catalog is unavailable.".into())
}

/// Discovery runs on a worker: native provider enumeration can be slow.
pub(crate) fn inventory() -> Result<Inventory, String> {
    let root = crate::spelling::dictionary_root()?;
    let native = crate::spelling::system_languages();
    let warning = native.as_ref().err().cloned();
    let native = native.unwrap_or_default();
    let mut dictionaries = Vec::new();
    for package in catalog()?.languages {
        let folder = root.join(&package.tag);
        if folder.is_dir() {
            let removable = receipts::owned(&package, Scope::Local);
            dictionaries.push(InstalledDictionary {
                tag: package.tag.clone(),
                scope: Scope::Local,
                removable,
            });
        }

        if crate::spelling::dictionary_language(&package.tag, &native).is_some() {
            #[cfg(any(target_os = "linux", windows))]
            let removable =
                platform::can_remove(&package) && receipts::owned(&package, Scope::System);
            #[cfg(not(any(target_os = "linux", windows)))]
            let removable = false;
            dictionaries.push(InstalledDictionary {
                tag: package.tag,
                scope: Scope::System,
                removable,
            });
        }
    }
    Ok(Inventory {
        dictionaries,
        warning,
    })
}

pub(crate) fn apply(
    tag: &str,
    scope: Scope,
    operation: Operation,
    sender: &Sender<Progress>,
) -> Result<Outcome, String> {
    let _lock = receipts::lock()?;
    match operation {
        Operation::Install => install(tag, scope, sender),
        Operation::Remove => remove(tag, scope, sender),
    }
}

fn remove(tag: &str, scope: Scope, sender: &Sender<Progress>) -> Result<Outcome, String> {
    let catalog = catalog()?;
    let package = catalog
        .languages
        .iter()
        .find(|p| p.tag == tag)
        .ok_or("Choose a supported dictionary language.")?;
    receipts::require_owned(package, scope)?;
    if scope == Scope::System {
        #[cfg(any(target_os = "linux", windows))]
        {
            let outcome = platform::remove_system(package, sender)?;
            receipts::forget(package, scope)?;
            return Ok(outcome);
        }
        #[cfg(not(any(target_os = "linux", windows)))]
        return Err("System dictionary removal is supported on Linux and Windows.".into());
    }
    progress(sender, "Removing the local dictionary…", 1);
    let root = crate::spelling::dictionary_root()?;
    let folder = root.join(&package.tag);
    let metadata = std::fs::symlink_metadata(&folder)
        .map_err(|e| format!("Could not find the dictionary: {e}"))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("The dictionary folder is not a regular local directory.".into());
    }
    let staging = Staging::new(&format!("remove-{}", package.tag))?;
    let removed = staging.path.join("dictionary");
    std::fs::rename(&folder, &removed)
        .map_err(|e| format!("Could not remove the dictionary: {e}"))?;
    crate::spelling::forget_local_dictionary(tag);
    std::fs::remove_dir_all(&removed).map_err(|e| {
        format!("The dictionary was removed, but its old files could not be deleted: {e}")
    })?;
    receipts::forget(package, scope)?;
    progress(sender, "Dictionary deleted.", 4);
    Ok(Outcome {
        restart_required: false,
    })
}
#[derive(Clone, Debug)]
pub(crate) struct Progress {
    pub stage: String,
    pub completed_steps: i32,
}
#[derive(Debug)]
pub(crate) struct Outcome {
    pub restart_required: bool,
}
#[derive(Deserialize)]
struct Catalog {
    commit: String,
    languages: Vec<Package>,
}
#[derive(Deserialize)]
pub(crate) struct Package {
    pub tag: String,
    pub package: String,
    pub files: Vec<Download>,
}
#[derive(Deserialize)]
pub(crate) struct Download {
    pub path: String,
    #[cfg(windows)]
    pub sha256: String,
    #[cfg(windows)]
    pub bytes: u64,
}

pub(crate) fn progress(sender: &Sender<Progress>, stage: &str, completed_steps: i32) {
    let _ = sender.send(Progress {
        stage: stage.to_string(),
        completed_steps,
    });
}

pub(crate) fn install(
    tag: &str,
    scope: Scope,
    sender: &Sender<Progress>,
) -> Result<Outcome, String> {
    let catalog = catalog()?;
    let package = catalog
        .languages
        .iter()
        .find(|package| package.tag == tag)
        .ok_or("Choose a supported dictionary language.")?;
    progress(sender, "Preparing installation…", 0);
    #[cfg(any(target_os = "linux", windows))]
    {
        if scope == Scope::System {
            #[cfg(windows)]
            let native = crate::spelling::system_languages()?;
            #[cfg(not(windows))]
            let native = crate::spelling::system_languages().unwrap_or_default();
            if crate::spelling::dictionary_language(tag, &native).is_some() {
                return Err("This system dictionary is already installed. Impress will not claim ownership of an existing installation.".into());
            }
            #[cfg(target_os = "linux")]
            if platform::can_remove(package) {
                return Err("This dictionary package is already installed outside Impress.".into());
            }
            let outcome = platform::system(package, sender)?;
            receipts::record(package, scope).map_err(|error| format!("The dictionary was installed, but its ownership record could not be saved. Deletion remains protected: {error}"))?;
            return Ok(outcome);
        }
        let destination = crate::spelling::dictionary_root()?.join(&package.tag);
        if destination.exists() {
            return Err(
                "This local dictionary already exists. Impress will not claim ownership of it."
                    .into(),
            );
        }
        let work = Staging::new(&package.tag)?;
        platform::local(package, &catalog.commit, &work.path, sender)?;
        progress(sender, "Checking the dictionary…", 3);
        crate::spelling::validate_pair(
            &work.path.join("dictionary.aff"),
            &work.path.join("dictionary.dic"),
        )?;
        let destination = crate::spelling::dictionary_root()?.join(&package.tag);
        if destination.exists() {
            return Err("This dictionary is already installed. Reopen the language menu.".into());
        }
        std::fs::rename(&work.path, destination)
            .map_err(|error| format!("Could not save the dictionary: {error}"))?;
        crate::spelling::forget_local_dictionary(tag);
        receipts::record(package, scope).map_err(|error| format!("The dictionary was installed, but its ownership record could not be saved. Deletion remains protected: {error}"))?;
        progress(sender, "Dictionary installed.", 4);
        Ok(Outcome {
            restart_required: false,
        })
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = (package, scope);
        Err("Dictionary installation is supported on Linux and Windows.".into())
    }
}

struct Staging {
    path: PathBuf,
}
impl Staging {
    fn new(tag: &str) -> Result<Self, String> {
        let root = crate::spelling::dictionary_root()?;
        std::fs::create_dir_all(&root).map_err(|error| error.to_string())?;
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos();
        let path = root.join(format!(".install-{tag}-{}-{unique}", std::process::id()));
        let builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        let builder = {
            use std::os::unix::fs::DirBuilderExt;
            let mut builder = builder;
            builder.mode(0o700);
            builder
        };
        builder.create(&path).map_err(|error| error.to_string())?;
        Ok(Self { path })
    }
}
impl Drop for Staging {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Native processes receive only application catalog values as arguments.
pub(crate) fn run(command: &mut Command) -> Result<(), String> {
    let output = command
        .output()
        .map_err(|error| format!("Could not start dictionary servicing: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let details = String::from_utf8_lossy(&output.stderr)
        .chars()
        .take(1600)
        .collect::<String>();
    Err(format!(
        "The dictionary operation did not finish ({}). {}",
        output.status,
        details.trim()
    ))
}

/// Downloads are pinned to an upstream revision and checked before use.
#[cfg(windows)]
pub(crate) fn download_files(
    package: &Package,
    commit: &str,
    folder: &std::path::Path,
    sender: &Sender<Progress>,
) -> Result<(), String> {
    for (index, file) in package.files.iter().enumerate() {
        if file.bytes > MAX_DOWNLOAD_BYTES {
            return Err("The dictionary download exceeds the size limit.".into());
        }
        progress(
            sender,
            &format!(
                "Downloading dictionary files… ({}/{})",
                index + 1,
                package.files.len()
            ),
            1,
        );
        let target = if index == 0 {
            folder.join("dictionary.aff")
        } else if index == 1 {
            folder.join("dictionary.dic")
        } else {
            folder.join(format!("NOTICE-{index}.txt"))
        };
        let url = format!("{DOWNLOAD_ROOT}/{commit}/{}", file.path);
        platform::download(&url, &target)?;
        verify_download(&target, file)?;
    }
    progress(sender, "Download verified.", 2);
    Ok(())
}

#[cfg(windows)]
fn verify_download(path: &std::path::Path, expected: &Download) -> Result<(), String> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let mut data = Vec::new();
    file.take(MAX_DOWNLOAD_BYTES + 1)
        .read_to_end(&mut data)
        .map_err(|error| error.to_string())?;
    if data.len() as u64 != expected.bytes
        || format!("{:x}", Sha256::digest(&data)) != expected.sha256
    {
        return Err(
            "The dictionary download failed its integrity check. Try installing again.".into(),
        );
    }
    Ok(())
}
