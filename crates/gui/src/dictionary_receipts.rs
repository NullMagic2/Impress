//! Durable ownership records, separate from replaceable application files.
use super::{Package, Scope};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::{Path, PathBuf},
};

const SCHEMA: u32 = 1;
const MAX_RECEIPT_BYTES: u64 = 8192;
const MAX_OWNED_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_OWNED_FILES: usize = 16;
const RECEIPT_DIRECTORY: &str = "dictionary-receipts";

#[derive(Serialize, Deserialize)]
struct Receipt {
    schema: u32,
    application: String,
    tag: String,
    scope: Scope,
    package: String,
    identity: String,
}

fn root() -> Result<PathBuf, String> {
    crate::data_dir::user_data_dir()
        .map(|path| path.join(RECEIPT_DIRECTORY))
        .ok_or_else(|| "The user data folder is unavailable.".into())
}

fn path(package: &Package, scope: Scope) -> Result<PathBuf, String> {
    let name = match scope {
        Scope::Local => "local",
        Scope::System => "system",
    };
    Ok(root()?.join(format!("{name}-{}.json", package.tag)))
}

/// Advisory OS locks are released automatically even after an interrupted run.
pub(super) fn lock() -> Result<std::fs::File, String> {
    let root = root()?;
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join("operations.lock"))
        .map_err(|e| e.to_string())?;
    lock.try_lock().map_err(|_| {
        "Another Impress instance is changing dictionaries. Try again when it finishes.".to_string()
    })?;
    Ok(lock)
}

fn identity(package: &Package, scope: Scope) -> Result<String, String> {
    match scope {
        Scope::Local => local_identity(&crate::spelling::dictionary_root()?.join(&package.tag)),
        Scope::System => {
            #[cfg(any(target_os = "linux", windows))]
            return super::platform::identity(package);
            #[cfg(not(any(target_os = "linux", windows)))]
            return Err("System dictionary identity is unavailable.".into());
        }
    }
}

pub(super) fn owned(package: &Package, scope: Scope) -> bool {
    let read = || -> Result<bool, String> {
        let file = std::fs::File::open(path(package, scope)?).map_err(|e| e.to_string())?;
        let receipt: Receipt =
            serde_json::from_reader(file.take(MAX_RECEIPT_BYTES)).map_err(|e| e.to_string())?;
        Ok(receipt.schema == SCHEMA
            && receipt.application == "Impress"
            && receipt.tag == package.tag
            && receipt.scope == scope
            && receipt.package == package.package
            && receipt.identity == identity(package, scope)?)
    };
    read().unwrap_or(false)
}

pub(super) fn require_owned(package: &Package, scope: Scope) -> Result<(), String> {
    if owned(package, scope) {
        Ok(())
    } else {
        Err("Impress can only delete dictionaries it installed and can still identify. This dictionary has no matching installation record or has changed outside Impress.".into())
    }
}

pub(super) fn record(package: &Package, scope: Scope) -> Result<(), String> {
    let receipt = Receipt {
        schema: SCHEMA,
        application: "Impress".into(),
        tag: package.tag.clone(),
        scope,
        package: package.package.clone(),
        identity: identity(package, scope)?,
    };
    let bytes = serde_json::to_vec_pretty(&receipt).map_err(|e| e.to_string())?;
    let root = root()?;
    std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
    format::atomic_write(&path(package, scope)?, |file| {
        use std::io::Write;
        file.write_all(&bytes).map_err(|e| e.to_string())
    })
}

pub(super) fn forget(package: &Package, scope: Scope) -> Result<(), String> {
    match std::fs::remove_file(path(package, scope)?) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!(
            "Dictionary deleted, but its installation record could not be removed: {e}"
        )),
    }
}

/// Include filesystem identity as well as content so replacement files do not
/// inherit an old receipt. Application upgrades leave these files untouched.
pub(super) fn file_identity(path: &Path) -> Result<String, String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("Unexpected dictionary file type.".into());
    }
    let modified = metadata
        .modified()
        .ok()
        .and_then(|v| v.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|v| v.as_nanos());
    let created = metadata
        .created()
        .ok()
        .and_then(|v| v.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|v| v.as_nanos());
    let result = format!("{}:{modified:?}:{created:?}", metadata.len());
    #[cfg(unix)]
    let result = {
        use std::os::unix::fs::MetadataExt;
        format!(
            "{result}:{}:{}:{}:{}",
            metadata.dev(),
            metadata.ino(),
            metadata.ctime(),
            metadata.ctime_nsec()
        )
    };
    Ok(result)
}

fn local_identity(folder: &Path) -> Result<String, String> {
    let metadata = std::fs::symlink_metadata(folder).map_err(|e| e.to_string())?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("Unexpected dictionary directory.".into());
    }
    let mut files = std::fs::read_dir(folder)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    if files.len() > MAX_OWNED_FILES {
        return Err("The dictionary folder contains unrecognized files.".into());
    }
    files.sort_by_key(|entry| entry.file_name());
    let mut hash = Sha256::new();
    let mut pair_files = 0;
    for file in files {
        let name = file
            .file_name()
            .into_string()
            .map_err(|_| "Unrecognized dictionary filename.")?;
        let notice = name
            .strip_prefix("NOTICE-")
            .and_then(|v| v.strip_suffix(".txt"))
            .is_some_and(|v| !v.is_empty() && v.bytes().all(|c| c.is_ascii_digit()));
        if !matches!(
            name.as_str(),
            "dictionary.aff" | "dictionary.dic" | "COPYRIGHT"
        ) && !notice
        {
            return Err("The dictionary folder contains files not installed by Impress.".into());
        }
        if matches!(name.as_str(), "dictionary.aff" | "dictionary.dic") {
            pair_files += 1;
        }
        let path = file.path();
        hash.update(name.as_bytes());
        hash.update(file_identity(&path)?.as_bytes());
        if std::fs::metadata(&path).map_err(|e| e.to_string())?.len() > MAX_OWNED_FILE_BYTES {
            return Err("The dictionary exceeds the size limit.".into());
        }
        let mut stream = std::fs::File::open(path)
            .map_err(|e| e.to_string())?
            .take(MAX_OWNED_FILE_BYTES + 1);
        let mut buffer = [0u8; 8192];
        loop {
            let read = stream.read(&mut buffer).map_err(|e| e.to_string())?;
            if read == 0 {
                break;
            }
            hash.update(&buffer[..read]);
        }
    }
    if pair_files != 2 {
        return Err("The dictionary file pair is incomplete.".into());
    }
    Ok(format!("{:x}", hash.finalize()))
}
