//! Windows private dictionary downloads and native elevated language servicing.
use super::{download_files, progress, run, Operation, Outcome, Package, Progress};
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc::Sender,
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, GetLastError},
    System::{
        SystemInformation::GetSystemDirectoryW,
        Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE},
    },
    UI::{
        Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW},
        WindowsAndMessaging::SW_HIDE,
    },
};

const REBOOT_REQUIRED: u32 = 3010;
const AUTHORIZATION_CANCELED: u32 = 1223;
const WINDOWS_PATH_CAPACITY: usize = 32768;

fn system_directory() -> Result<PathBuf, String> {
    let mut buffer = vec![0u16; WINDOWS_PATH_CAPACITY];
    let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    if length == 0 || length >= buffer.len() {
        return Err("The Windows system directory is unavailable.".into());
    }
    use std::os::windows::ffi::OsStringExt;
    Ok(PathBuf::from(std::ffi::OsString::from_wide(
        &buffer[..length],
    )))
}

pub(super) fn local(
    package: &Package,
    commit: &str,
    folder: &Path,
    sender: &Sender<Progress>,
) -> Result<(), String> {
    download_files(package, commit, folder, sender)
}

pub(super) fn download(url: &str, target: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    // Paths travel through the environment, never through PowerShell source.
    run(Command::new(system_directory()?.join("WindowsPowerShell/v1.0/powershell.exe"))
        .creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW)
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command",
            "$ErrorActionPreference='Stop'; [Net.ServicePointManager]::SecurityProtocol=[Net.SecurityProtocolType]::Tls12; Invoke-WebRequest -UseBasicParsing -TimeoutSec 120 -Uri $env:IMPRESS_DICTIONARY_URL -OutFile $env:IMPRESS_DICTIONARY_TARGET"])
        .env("IMPRESS_DICTIONARY_URL", url).env("IMPRESS_DICTIONARY_TARGET", target))
}

pub(super) fn system(package: &Package, sender: &Sender<Progress>) -> Result<Outcome, String> {
    service(package, sender, Operation::Install)
}

pub(super) fn can_remove(_: &Package) -> bool {
    true
}

pub(super) fn identity(package: &Package) -> Result<String, String> {
    let system = system_directory()?;
    let windows = system
        .parent()
        .ok_or("The Windows directory is unavailable")?;
    let prefix = format!(
        "microsoft-windows-languagefeatures-basic-{}-package~",
        package.tag.to_ascii_lowercase()
    );
    let mut identities = Vec::new();
    for entry in std::fs::read_dir(windows.join("servicing/Packages")).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if name.starts_with(&prefix) && name.ends_with(".mum") {
            identities.push(format!(
                "{name}:{}",
                super::receipts::file_identity(&entry.path())?
            ));
        }
    }
    identities.sort();
    if identities.is_empty() {
        return Err("Windows did not expose a verifiable dictionary package identity.".into());
    }
    Ok(identities.join("\n"))
}

pub(super) fn remove_system(
    package: &Package,
    sender: &Sender<Progress>,
) -> Result<Outcome, String> {
    super::receipts::require_owned(package, super::Scope::System)?;
    service(package, sender, Operation::Remove)
}

fn service(
    package: &Package,
    sender: &Sender<Progress>,
    operation: Operation,
) -> Result<Outcome, String> {
    use std::os::windows::ffi::OsStrExt;
    let executable = system_directory()?.join("Dism.exe");
    let file = executable
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let command = match operation {
        Operation::Install => "Add-Capability",
        Operation::Remove => "Remove-Capability",
    };
    let parameters = format!(
        "/Online /{command} /CapabilityName:Language.Basic~~~{}~0.0.1.0 /NoRestart",
        package.tag
    );
    let parameters = parameters.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let verb = "runas".encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    progress(
        sender,
        "Waiting for administrator approval and Windows language servicing…",
        1,
    );
    unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS;
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.lpParameters = parameters.as_ptr();
        info.nShow = SW_HIDE;
        if ShellExecuteExW(&mut info) == 0 {
            let code = GetLastError();
            return Err(if code == AUTHORIZATION_CANCELED {
                format!("{} canceled.", operation.title())
            } else {
                format!("Windows could not start dictionary servicing (error {code}).")
            });
        }
        if info.hProcess.is_null() {
            return Err("Windows did not return an installer process.".into());
        }
        let waited = WaitForSingleObject(info.hProcess, INFINITE);
        let mut code = 0;
        let queried = GetExitCodeProcess(info.hProcess, &mut code);
        CloseHandle(info.hProcess);
        if waited != 0 || queried == 0 {
            return Err("Could not read the Windows installation result.".into());
        }
        if code != 0 && code != REBOOT_REQUIRED {
            return Err(format!(
                "Windows dictionary operation failed (error {code}). Check Windows language settings and try again."
            ));
        }
        progress(
            sender,
            match operation {
                Operation::Install => "Windows dictionary installed.",
                Operation::Remove => "Windows dictionary removed.",
            },
            4,
        );
        Ok(Outcome {
            restart_required: code == REBOOT_REQUIRED,
        })
    }
}
