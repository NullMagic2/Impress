param(
    [ValidateSet("release", "debug")]
    [string]$Profile = "release"
)
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw "Rust/Cargo was not found in PATH. Install Rust with rustup first."
}
if ($Profile -eq "debug") {
    cargo build -p gui
} else {
    cargo build -p gui --release
}
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
Write-Host "[Impress] Build completed successfully."
