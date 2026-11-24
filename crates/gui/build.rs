fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        use embed_manifest::manifest::{ActiveCodePage, DpiAwareness, HeapType, Setting, SupportedOS::*};
        use embed_manifest::{embed_manifest, new_manifest};
        let manifest = new_manifest("Impress.WordProcessor")
            .supported_os(Windows7..=Windows10)
            .active_code_page(ActiveCodePage::Utf8)
            .heap_type(HeapType::SegmentHeap)
            .dpi_awareness(DpiAwareness::PerMonitorV2)
            .long_path_aware(Setting::Enabled);
        if let Err(error) = embed_manifest(manifest) {
            println!("cargo:warning=Could not embed Windows manifest: {error}");
        }
        embed_app_icon();
    }
    println!("cargo:rerun-if-changed=build.rs");
}

fn embed_app_icon() {
    use std::{env, path::PathBuf, process::Command};
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest directory"));
    let resource = manifest_dir.join("assets/app_icon.rc");
    let icon = manifest_dir.join("assets/impress.ico");
    println!("cargo:rerun-if-changed={}", resource.display());
    println!("cargo:rerun-if-changed={}", icon.display());
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("build output"));
    let target = env::var("TARGET").expect("target triple");
    if target.ends_with("-msvc") {
        let compiler = env::var_os("RC").map(PathBuf::from).or_else(|| {
            let kits = PathBuf::from(env::var_os("ProgramFiles(x86)")?).join("Windows Kits/10/bin");
            let mut versions = std::fs::read_dir(kits).ok()?.flatten().map(|entry| entry.path()).collect::<Vec<_>>();
            versions.sort();
            versions.into_iter().rev().map(|v| v.join("x64/rc.exe")).find(|p| p.is_file())
        }).expect("Windows SDK rc.exe is required to embed the Impress icon");
        let output = out.join("impress-icon.res");
        let status = Command::new(compiler).current_dir(&manifest_dir).arg("/nologo")
            .arg(format!("/fo{}", output.display())).arg(&resource).status().expect("run rc.exe");
        assert!(status.success(), "could not compile Impress icon resource");
        println!("cargo:rustc-link-arg-bins={}", output.display());
    } else {
        let output = out.join("impress-icon.o");
        let status = Command::new(env::var_os("WINDRES").unwrap_or_else(|| "windres".into())).current_dir(&manifest_dir).arg("-O").arg("coff")
            .arg("-i").arg(&resource).arg("-o").arg(&output).status().expect("run windres");
        assert!(status.success(), "could not compile Impress icon resource");
        println!("cargo:rustc-link-arg-bins={}", output.display());
    }
}
