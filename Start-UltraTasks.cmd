@echo off
setlocal
powershell.exe -NoLogo -NoProfile -File "%~dp0scripts\start-demo.ps1" %*
set "ultratasks_result=%ERRORLEVEL%"
if not "%ultratasks_result%"=="0" (
    echo UltraTasks could not start or finish. This launcher never builds locally.
    echo If no binary is installed, download a successful Windows artifact from iiinno/UltraTasks Actions.
    pause
)
exit /b %ultratasks_result%
