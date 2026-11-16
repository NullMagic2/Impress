@echo off
setlocal EnableExtensions
cd /d "%~dp0"
where cargo >nul 2>nul || (
  echo [Impress] Rust/Cargo was not found in PATH.
  exit /b 1
)
echo [Impress] Checking all crates...
cargo check --workspace
if errorlevel 1 exit /b 1
echo [Impress] Running tests...
cargo test --workspace
if errorlevel 1 exit /b 1
echo [Impress] Check and tests completed successfully.
