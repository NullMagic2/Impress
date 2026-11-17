# Linux distribution packages

Impress packages are built separately for these amd64 targets:

| Package target | Build environment | Intended systems |
| --- | --- | --- |
| `mint22` | Mint 22.3 development image, Ubuntu 24.04 base | Ubuntu-based Mint 22.x |
| `ubuntu24.04` | Ubuntu 24.04 | Ubuntu 24.04 LTS |
| `ubuntu26.04` | Ubuntu 26.04 | Ubuntu 26.04 LTS |
| `debian13` | Debian 13 | Debian 13 (Trixie) |

[Mint’s release list](https://www.linuxmint.com/download_all.php) distinguishes
Ubuntu-based releases and LMDE. They have different package bases. The Mint
22.x package is for the Ubuntu-based edition; the Debian 13 package is the
appropriate ABI base for LMDE 7, although the workflow does not test an LMDE
image. Older Mint or Debian versions are not implied by these package names.

The [Mint development image](https://hub.docker.com/r/linuxmintd/mint22.3-amd64)
uses Ubuntu 24.04 as its base and retains Ubuntu's `/etc/os-release` identity.
The packager checks the pinned Mint image and its Zena package repository,
and records the actual base identity and image digest in build metadata.

Use the package matching your distribution. Install a downloaded package with
`sudo apt install ./impress_*.deb` so the package manager resolves dependencies.
The package supplies `impress`, `impress-convert`, the launcher, icon and MIME
registration. It preserves user documents and preferences on upgrades.

## Building and validating a package

Build the release binaries inside the target distribution, following
[BUILDING.md](BUILDING.md), then run:

```sh
python3 packaging/linux/build-deb.py --target debian13 --version 0.8.6-65
sudo apt install ./dist/*.deb
packaging/linux/verify-installed.sh dist/validation
```

The packager refuses an incompatible build host, generates dependencies with
`dpkg-shlibdeps`, and verifies that the packaged binaries match the build.
Changing a package's label or loosening dependency versions cannot make a
newer-system binary work on an older distribution.

## GitHub releases

The manually dispatched **Linux distribution releases** workflow builds all
four targets in pinned amd64 container images. It runs Rust and UI resource
tests, installs each package, checks shared-library resolution, exercises a
DOCX conversion round trip, and launches the GUI under Xvfb. It publishes
separate Mint, Ubuntu and Debian release entries only after all targets pass.
The Ubuntu entry contains both LTS builds. Checksums, build metadata, test logs
and a startup screenshot accompany the packages.

For a packaging-only correction, the workflow can reuse binaries and passing
test evidence from a completed run using `validated_run`. It verifies the
original job's build and test results, unchanged application inputs and identical
pinned build images, then repeats package installation and startup checks.
Build metadata and verification archives record both source commits and the
original run. Changes to application inputs require a fresh compilation and test run.

These checks establish package installation and basic operation in the named
container environments; they do not cover every display server, desktop or
hardware configuration.

Packages depend on Noto core fonts to provide a predictable fallback when a
document requests a font that is not installed.
