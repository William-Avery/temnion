@echo off
setlocal
title Temnion Database Setup Wizard

echo ================================================================================
echo         TEMNION ENTERPRISE TEMPORAL DATABASE - INSTALLER LAUNCHER
echo ================================================================================
echo.
echo Launching PostgreSQL-style interactive setup wizard...
echo.

set "SCRIPT_DIR=%~dp0"

if exist "%SCRIPT_DIR%installer\install.ps1" (
    powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%SCRIPT_DIR%installer\install.ps1" %*
) else if exist "%SCRIPT_DIR%install.ps1" (
    powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%SCRIPT_DIR%install.ps1" %*
) else (
    echo Error: Could not locate install.ps1 in installer\ or current directory.
    pause
    exit /b 1
)

if %ERRORLEVEL% equ 0 (
    echo.
    echo Installation completed successfully.
) else (
    echo.
    echo Installation exited with code %ERRORLEVEL%.
)

pause
