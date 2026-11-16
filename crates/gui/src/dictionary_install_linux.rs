//! Ubuntu/Debian dictionary packages: private extraction or elevated APT install.
use super::{progress, run, Outcome, Package, Progress};
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc::Sender,
};

const APT: &str = "/usr/bin/apt-get";
const DPKG: &str = "/usr/bin/dpkg-deb";
const PKEXEC: &str = "/usr/bin/pkexec";
const DPKG_MANAGER: &str = "/usr/bin/dpkg";
const DPKG_QUERY: &str = "/usr/bin/dpkg-query";

pub(super) fn can_remove(package: &Package) -> bool {
    supported().is_ok()
        && Command::new(DPKG_QUERY)
            .args([
                "--show",
                "--showformat=${db:Status-Status}",
                &package.package,
            ])
            .env("LC_ALL", "C")
            .output()
            .is_ok_and(|output| output.status.success() && output.stdout == b"installed")
}

pub(super) fn identity(package: &Package) -> Result<String, String> {
    let output = Command::new(DPKG_QUERY)
        .args([
            "--show",
            "--showformat=${Version}:${db:Status-Status}",
            &package.package,
        ])
        .env("LC_ALL", "C")
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() || !output.stdout.ends_with(b":installed") {
        return Err("The dictionary package is not installed.".into());
    }
    let list = Path::new("/var/lib/dpkg/info").join(format!("{}.list", package.package));
    Ok(format!(
        "{}:{}",
        String::from_utf8_lossy(&output.stdout),
        super::receipts::file_identity(&list)?
    ))
}

pub(super) fn remove_system(
    package: &Package,
    sender: &Sender<Progress>,
) -> Result<Outcome, String> {
    supported()?;
    super::receipts::require_owned(package, super::Scope::System)?;
    if !can_remove(package) {
        return Err("This dictionary is managed by another system provider. Remove it through your system's package manager.".into());
    }
    if !Path::new(PKEXEC).is_file() {
        return Err("System-wide removal needs PolicyKit (pkexec).".into());
    }
    progress(
        sender,
        "Waiting for system authorization and dictionary removal…",
        1,
    );
    // Remove only the selected dictionary package. Refuse dependency-breaking
    // removal instead of letting an APT solution remove other applications.
    run(Command::new(PKEXEC).args([
        "--disable-internal-agent",
        DPKG_MANAGER,
        "--refuse-depends",
        "--refuse-remove-essential",
        "--refuse-remove-protected",
        "--remove",
        &package.package,
    ]))?;
    progress(sender, "Dictionary package removed.", 4);
    Ok(Outcome {
        restart_required: false,
    })
}

fn supported() -> Result<(), String> {
    let release = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
    let debian = release
        .lines()
        .filter_map(|line| line.split_once('='))
        .filter(|(key, _)| matches!(*key, "ID" | "ID_LIKE"))
        .any(|(_, value)| {
            value
                .trim_matches('"')
                .split_whitespace()
                .any(|id| matches!(id, "ubuntu" | "debian"))
        });
    if !debian || !Path::new(APT).is_file() || !Path::new(DPKG).is_file() {
        return Err("Automatic dictionary installation currently supports Ubuntu and Debian-based Linux systems.".into());
    }
    Ok(())
}

pub(super) fn local(
    package: &Package,
    _: &str,
    folder: &Path,
    sender: &Sender<Progress>,
) -> Result<(), String> {
    supported()?;
    progress(sender, "Downloading the dictionary package…", 1);
    run(Command::new(APT).current_dir(folder).args([
        "-o",
        "APT::Get::AllowUnauthenticated=false",
        "-o",
        "Acquire::AllowInsecureRepositories=false",
        "-o",
        "Acquire::Retries=1",
        "-o",
        "Acquire::http::Timeout=60",
        "-o",
        "Acquire::https::Timeout=60",
        "download",
        &package.package,
    ]))?;
    let archives = std::fs::read_dir(folder)
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "deb"))
        .collect::<Vec<_>>();
    if archives.len() != 1 {
        return Err(
            "The dictionary package was not downloaded. Check your package sources and try again."
                .into(),
        );
    }
    progress(sender, "Extracting the dictionary for your account…", 2);
    let unpacked = folder.join("unpacked");
    run(Command::new(DPKG)
        .arg("--extract")
        .arg(&archives[0])
        .arg(&unpacked))?;
    let dictionaries = unpacked.join("usr/share/hunspell");
    let choices = [
        package.tag.replace('-', "_"),
        package.tag.split('-').next().unwrap_or("").to_string(),
        package.files[0]
            .path
            .rsplit('/')
            .next()
            .unwrap_or("")
            .trim_end_matches(".aff")
            .to_string(),
    ];
    let base = choices
        .into_iter()
        .map(|name| dictionaries.join(name))
        .find(|path| path.with_extension("aff").is_file() && path.with_extension("dic").is_file())
        .ok_or("The package does not contain the selected dictionary.")?;
    for extension in ["aff", "dic"] {
        let source = base
            .with_extension(extension)
            .canonicalize()
            .map_err(|error| error.to_string())?;
        if !source.starts_with(unpacked.canonicalize().map_err(|error| error.to_string())?) {
            return Err("The dictionary package contains an unexpected file link.".into());
        }
        std::fs::copy(source, folder.join(format!("dictionary.{extension}")))
            .map_err(|error| error.to_string())?;
    }
    let notice = unpacked
        .join("usr/share/doc")
        .join(&package.package)
        .join("copyright");
    std::fs::copy(notice, folder.join("COPYRIGHT"))
        .map_err(|error| format!("Could not preserve the dictionary license: {error}"))?;
    std::fs::remove_dir_all(unpacked).map_err(|error| error.to_string())?;
    std::fs::remove_file(&archives[0]).map_err(|error| error.to_string())?;
    Ok(())
}

pub(super) fn system(package: &Package, sender: &Sender<Progress>) -> Result<Outcome, String> {
    supported()?;
    if !PathBuf::from(PKEXEC).is_file() {
        return Err(
            "System installation needs PolicyKit (pkexec). Choose a local installation instead."
                .into(),
        );
    }
    progress(
        sender,
        "Waiting for system authorization and package installation…",
        1,
    );
    run(Command::new(PKEXEC).args([
        "--disable-internal-agent",
        APT,
        "--assume-yes",
        "--no-remove",
        "-o",
        "APT::Get::AllowUnauthenticated=false",
        "-o",
        "Acquire::AllowInsecureRepositories=false",
        "install",
        &package.package,
        "libenchant-2-2",
    ]))?;
    progress(sender, "Dictionary package installed.", 4);
    Ok(Outcome {
        restart_required: false,
    })
}
