//! Compile the vendored Hunspell engine for private dictionaries on both OSes.
fn main() {
    let source = std::path::Path::new("../hunspell-1.7.2/src/hunspell");
    let mut build = cc::Build::new();
    build
        .cpp(true)
        .warnings(false)
        .std("c++17")
        .define("HUNSPELL_STATIC", None)
        .include(source);
    for name in [
        "affentry",
        "affixmgr",
        "csutil",
        "filemgr",
        "hashmgr",
        "hunspell",
        "hunzip",
        "phonet",
        "replist",
        "suggestmgr",
    ] {
        build.file(source.join(format!("{name}.cxx")));
    }
    build.compile("impress_hunspell");
    println!("cargo:rerun-if-changed=../hunspell-1.7.2/src/hunspell");
}
