@echo off
setlocal EnableExtensions
cd /d "%~dp0"

where cargo >nul 2>nul || (
  echo [Impress] Rust/Cargo was not found in PATH.
  echo Install Rust from https://rustup.rs/ and reopen this terminal.
  exit /b 1
)

set PROFILE=release
if /I "%~1"=="debug" set PROFILE=debug
if /I "%~1"=="release" set PROFILE=release

if /I "%PROFILE%"=="debug" (
  echo [Impress] Building debug application...
  cargo build -p gui
) else (
  echo [Impress] Building release application...
  cargo build -p gui --release
)

if errorlevel 1 (
  echo [Impress] Build failed.
  exit /b 1
)

echo [Impress] Build completed successfully.
if /I "%PROFILE%"=="debug" (
  echo Output: %CD%\target\debug\Impress.exe
) else (
  echo Output: %CD%\target\release\Impress.exe
)
