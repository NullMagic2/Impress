@echo off
setlocal EnableExtensions
cd /d "%~dp0"
where cargo >nul 2>nul || (
  if exist target rmdir /s /q target
  echo [Impress] Cleaned target directory.
  exit /b 0
)
cargo clean
if errorlevel 1 exit /b 1
echo [Impress] Clean complete.
