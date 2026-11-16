# Building Impress

Impress is a Rust workspace with a wxDragon/wxWidgets desktop interface. The
workspace declares Rust 1.92 or newer. The Ubuntu release has been built with
Rust 1.93.1. Build output belongs in `target/` and is not checked into Git.

## Linux

Install a C/C++ toolchain, CMake, Clang/libclang, pkg-config and the GTK 3,
PNG, JPEG, TIFF, Expat, OpenGL and GLU development packages. On Ubuntu,
these include `build-essential`, `cmake`, `clang`, `libclang-dev`, `pkg-config`,
`libgtk-3-dev`, `libpng-dev`, `libjpeg-dev`, `libtiff-dev`, `libexpat1-dev`,
`libgl1-mesa-dev` and `libglu1-mesa-dev`.

From the repository root:

```sh
cargo build --release --locked --bin Impress --bin impress-convert
./target/release/Impress
```

Pass a document path as an argument to open it. To install the launcher and icon
for the current user, run `packaging/linux/install.sh`; pass `--uninstall` to
remove that integration. A packaged Ubuntu install supplies this integration.

## Windows

Install Rust and the C/C++ build tools required by wxDragon/wxWidgets. Then run:

```powershell
.\build.ps1
```

`build.bat` is also available. Release output is `target/release/Impress.exe`.
Use `build.ps1 debug` or `build.bat debug` for a development build.

## Checks

```sh
cargo check --workspace --locked
cargo test --workspace --locked
```

`check.ps1` and `check.bat` run the workspace checks and tests on Windows. The
standard-library UI resource checks can be run with Python, for example:

```sh
python3 -m unittest discover -s tools/tests -p 'test_ribbon_runtime_contracts.py'
```

## Source map

| Area | Location |
| --- | --- |
| Document model | `crates/core` |
| Editing commands, selection and undo | `crates/editor` |
| Fonts and shaping | `crates/typography` |
| Pagination | `crates/layout` |
| Vector scene and viewport | `crates/render` |
| Desktop interface | `crates/gui` |
| Import and export | `crates/docx`, `crates/format-*` |
| PDF and raster output | `crates/output` |
| Interface references and generators | `design`, `tools` |

See [ARCHITECTURE.md](../ARCHITECTURE.md) for the boundaries between these layers.
