@echo off
setlocal EnableExtensions
set "SCRIPT_DIR=%~dp0"
for %%I in ("%SCRIPT_DIR%..\..") do set "ROOT=%%~fI"
set "EXE=%ROOT%\tools\build\release\veil-browser.exe"
set "LOGDIR=%ROOT%\tools\logs"
set "LOG=%LOGDIR%\veil-browser-run.log"
if not exist "%LOGDIR%" mkdir "%LOGDIR%" >nul 2>nul
cd /d "%ROOT%"
if not exist "%EXE%" (
  echo [ERROR] Veil Browser has not been built yet.
  echo Run tools\windows\build.bat first.
  pause
  exit /b 1
)
del /q "%LOG%" >nul 2>nul
"%EXE%" 2> "%LOG%"
set "CODE=%ERRORLEVEL%"
if not "%CODE%"=="0" (
  echo.
  echo [ERROR] Veil Browser exited with code %CODE%.
  echo.
  if exist "%LOG%" type "%LOG%"
  if exist "%ROOT%\tools\build\release\veil-browser-crash.log" (
    echo.
    echo -------- crash log --------
    type "%ROOT%\tools\build\release\veil-browser-crash.log"
  )
  echo.
  echo Try tools\windows\start-safe.bat next.
  pause
)
exit /b %CODE%
