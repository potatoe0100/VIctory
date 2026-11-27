@echo off
REM Portable MiniVI Series S 540p preset. Pass exe as arg 1 or set VICTORY_GAME_DIR.
setlocal
if not "%~1"=="" (
    powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0play-minivi.ps1" -Preset perf-540p -Exe "%~1"
) else (
    powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0play-minivi.ps1" -Preset perf-540p
)
if errorlevel 1 pause
