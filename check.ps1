$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw "Rust/Cargo was not found in PATH. Install Rust with rustup first."
}
cargo check --workspace
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
cargo test --workspace
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
Write-Host "[Impress] Check and tests completed successfully."
