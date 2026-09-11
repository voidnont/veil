@echo off
setlocal EnableExtensions
set "SCRIPT_DIR=%~dp0"
for %%I in ("%SCRIPT_DIR%..\..") do set "ROOT=%%~fI"
set "EXE=%ROOT%\tools\build\release\veil-browser.exe"
set "LOGDIR=%ROOT%\tools\logs"
set "LOG=%LOGDIR%\veil-browser-safe-run.log"
if not exist "%LOGDIR%" mkdir "%LOGDIR%" >nul 2>nul
cd /d "%ROOT%"
if not exist "%EXE%" (
  echo [ERROR] Veil Browser has not been built yet.
  echo Run tools\windows\build.bat first.
  pause
  exit /b 1
)
del /q "%LOG%" >nul 2>nul
"%EXE%" --safe-ui 2> "%LOG%"
set "CODE=%ERRORLEVEL%"
if not "%CODE%"=="0" (
  echo.
  echo [ERROR] Veil Browser safe UI exited with code %CODE%.
  if exist "%LOG%" type "%LOG%"
  if exist "%ROOT%\tools\build\release\veil-browser-crash.log" (
    echo.
    echo -------- crash log --------
    type "%ROOT%\tools\build\release\veil-browser-crash.log"
  )
  pause
)
exit /b %CODE%
