$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot
if (Get-Command cargo -ErrorAction SilentlyContinue) {
    cargo clean
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
} elseif (Test-Path "$PSScriptRoot\target") {
    Remove-Item -Recurse -Force "$PSScriptRoot\target"
}
Write-Host "[Impress] Clean complete."
