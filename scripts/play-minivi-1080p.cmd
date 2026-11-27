@echo off
REM Portable MiniVI Series S native 1080p preset. Pass exe as arg 1 or set VICTORY_GAME_DIR.
setlocal
if not "%~1"=="" (
    powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0play-minivi.ps1" -Preset native-1080p -Exe "%~1"
) else (
    powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0play-minivi.ps1" -Preset native-1080p
)
if errorlevel 1 pause
