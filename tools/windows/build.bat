@echo off
setlocal EnableExtensions
set "SCRIPT_DIR=%~dp0"
for %%I in ("%SCRIPT_DIR%..\..") do set "ROOT=%%~fI"
set "TARGET=%ROOT%\tools\build"
set "LOGDIR=%ROOT%\tools\logs"
set "LOG=%LOGDIR%\veil-build.log"
if not exist "%LOGDIR%" mkdir "%LOGDIR%" >nul 2>nul
cd /d "%ROOT%"
cls

echo ============================================================
echo             VEIL BROWSER 0.8.0 WINDOWS BUILDER
echo ============================================================
echo Project: %ROOT%
echo Build output: %TARGET%
echo Build log: %LOG%
echo.

del /q "%LOG%" >nul 2>nul
where cargo >nul 2>nul
if errorlevel 1 (
  echo [ERROR] Rust/Cargo was not found.
  echo Install stable Rust from https://rustup.rs/
  echo Rust/Cargo not found. > "%LOG%"
  goto :failed
)

set "CARGO_TARGET_DIR=%TARGET%"

echo [1/3] Rust toolchain...
rustc --version
cargo --version
(rustc --version & cargo --version) > "%LOG%" 2>&1

echo.
echo [2/3] Building Veil Browser + Veil Engine...
cargo build --release --bins >> "%LOG%" 2>&1
if errorlevel 1 (
  echo [ERROR] cargo build failed.
  goto :failed
)

if not exist "%TARGET%\release\veil-browser.exe" (
  echo [ERROR] Cargo returned success, but veil-browser.exe was not created.
  echo Expected %TARGET%\release\veil-browser.exe >> "%LOG%"
  goto :failed
)
if not exist "%TARGET%\release\veil-engine.exe" (
  echo [ERROR] Cargo returned success, but veil-engine.exe was not created.
  echo Expected %TARGET%\release\veil-engine.exe >> "%LOG%"
  goto :failed
)

echo [OK] Release binaries built.

echo.
echo [3/3] Running tests...
cargo test >> "%LOG%" 2>&1
if errorlevel 1 (
  echo [WARNING] Tests failed, but the release binaries were built successfully.
  echo See tools\logs\veil-build.log for the test failure.
  goto :built_with_test_warning
)
echo [OK] Tests passed.

echo.
echo [OK] Build complete.
echo Browser: tools\build\release\veil-browser.exe
echo Engine:  tools\build\release\veil-engine.exe
echo.
echo Launch with tools\windows\start.bat.
echo.
pause
exit /b 0

:built_with_test_warning
echo.
echo [OK] Browser binary exists and can be launched for testing.
echo Browser: tools\build\release\veil-browser.exe
echo Engine:  tools\build\release\veil-engine.exe
echo.
echo Start with tools\windows\start-safe.bat first if your graphics driver dislikes transparency.
echo.
pause
exit /b 0

:failed
echo.
echo ---------------- LAST BUILD OUTPUT ----------------
powershell -NoProfile -Command "if (Test-Path '%LOG%') { Get-Content '%LOG%' -Tail 120 }"
echo ---------------------------------------------------
echo.
echo The full build error is saved in:
echo %LOG%
echo.
pause
exit /b 1
