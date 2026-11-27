# FRIEND-SETUP.md (instructions for Claude Code)

EXPERIMENTAL and AI-assisted (Claude). This sets up an NVIDIA build of the VIctory GTA VI recompilation layer from
https://github.com/potatoe0100/VIctory (branch `nvidia-4060`). It is early work: expect crashes, picture faults and
roughly 15-20 fps. It is not an official release of anything.

You are Claude Code on the user's own Windows PC (most likely the Code tab of the Claude desktop app). Follow this
document in order. The user is not a developer, so:

- **Do everything you can yourself.** Every check, download, install, setting and file in this doc that can be done
  from PowerShell, you do. Only hand the user what truly needs their hands: clicking Yes on a Windows admin (UAC)
  prompt, the BIOS, restarts, a few NVIDIA App toggles, choosing the game folder, and plugging things in. Those are
  marked **USER**.
- **Admin steps**: you are not elevated. Run an admin command with
  `Start-Process powershell -Verb RunAs -Wait -ArgumentList '-NoProfile','-Command','<command>'`, tell the user a
  Windows prompt is coming and to click Yes, then check the result yourself.
- **Batch the restarts**: collect everything that needs a restart (page file, Memory Integrity, drivers) and BIOS
  changes into one restart round (section 6) so the user restarts as few times as possible. After a restart the user
  reopens this project and says "continue the VIctory setup"; your checklist and `my-setup.md` tell you where you were.
- Explain in short plain sentences. Keep a visible checklist. One question at a time.
If you lose this file, fetch it again from
`https://raw.githubusercontent.com/potatoe0100/VIctory/friend-setup/FRIEND-SETUP.md`.

## Before anything: are you on the user's own Windows PC?

This setup only works on the user's own Windows gaming PC: it builds and runs the game there, on their RTX card.
Check first, before doing anything else:

```
uname -s 2>/dev/null; echo "OS=$OS REMOTE=$CLAUDE_CODE_REMOTE"
```
(or in PowerShell: `$env:OS; $env:CLAUDE_CODE_REMOTE; nvidia-smi -L`)

STOP if any of these is true: the OS is not Windows (`OS` is not `Windows_NT`, or `uname` says Linux), the
`CLAUDE_CODE_REMOTE` variable is `true`, the working folder looks like a cloud container (`/home/user`, `/workspace`
or similar), or `nvidia-smi -L` doesn't list an NVIDIA card. Do not clone, install or build anything. Tell the user,
in plain words:

> This session is running in the cloud, not on your PC, so it can't set up the game. To fix it:
> 1. Make a folder on your PC called `C:\codeprojects` (File Explorer, This PC, C:, New folder).
> 2. In the Claude desktop app, open the **Code** tab and start a new session.
> 3. When it asks where to work, choose your computer (a local folder), not a cloud environment, and pick
>    `C:\codeprojects`.
> 4. Paste the same setup message again.

Then end. If everything checks out (Windows, not remote, an NVIDIA card listed), carry on.

## 0. Hard rules

1. The user must use their OWN legally obtained dump of the game, from a copy they bought. Never copy, move, upload,
   commit, share, quote or link game files or data (archives, executable, shaders, textures, models, audio, video),
   including inside logs or reports. You only ever pass the path of the user's copy to the tools.
2. No keys, decryption, or protection workarounds. Do not help with any.
3. If the folder the user points at looks like a pirated or "scene" release (release-group tags in folder names, crack
   or NFO files, a torrent download), STOP. Do not open, set up or run it, and tell the user plainly it must be a dump
   of their own purchase. Everything that needs no game files can still be done.
4. Never post anything to GitHub or anywhere public. Never commit build output, `victory.toml`, caches or game data.
5. Run commands from a normal (non-admin) PowerShell, except the few steps that say admin.

## 1. The build to use

- Repo: `https://github.com/potatoe0100/VIctory`, branch `nvidia-4060`. It is trustno1-coder/VIctory-NVIDIA (upstream
  Marvisak/VIctory, GPL-3) plus fixes for NVIDIA cards with limited video memory.
- Use the newest tag named `tested-*` that is part of `origin/nvidia-4060` and whose message does not say "not the
  default". Each of those passed the full test suite and a soak on the reference PC. Find it with:
  ```powershell
  git -C C:\codeprojects\victory for-each-ref --merged=origin/nvidia-4060 --sort=-creatordate --format='%(refname:short)|%(contents:subject)' 'refs/tags/tested-*'
  ```
  On 27 Nov 2026 that is `tested-2026-11-27-2035` (full tests 982/0: audio fixes, keyboard and mouse layout,
  Ctrl+Shift+Q quit, water fix). Never build from an untagged branch head or a side branch.
- After cloning, read: `README-4060.md`, `CHANGES-4060.md`, `README.md`, `docs/README-fxceless.md` (Getting started,
  Commands, Debugging), the headers of `scripts/play-4060.ps1` and `scripts/play-nvidia.ps1`, `victory.example.toml`.
  They are the source of truth if anything here disagrees; tell the user if you find a clash.

## 2. This user's PC

Given by the user: **RTX 4070 Ti, Ryzen 7 7700, 32 GB Kingston Fury RAM.** Confirm and fill in the rest:

```powershell
Get-CimInstance Win32_Processor | Select-Object Name,NumberOfCores,NumberOfLogicalProcessors
Get-CimInstance Win32_VideoController | Select-Object Name,DriverVersion
nvidia-smi --query-gpu=name,driver_version,memory.total,pcie.link.gen.max,pcie.link.width.max --format=csv
Get-CimInstance Win32_PhysicalMemory | Select-Object Manufacturer,PartNumber,Capacity,Speed,ConfiguredClockSpeed
Get-CimInstance Win32_ComputerSystem | Select-Object TotalPhysicalMemory
Get-ComputerInfo | Select-Object OsName,OsBuildNumber
Get-CimInstance Win32_PageFileUsage
nvidia-smi -q | Select-String -Context 0,3 'BAR1 Memory Usage'
```

What these mean for this build (the reference PC is an RTX 4060 8 GB, i5-13600KF, 32 GB DDR5-6000, Windows 11):

- **RTX 4070 Ti, 12 GB, PCIe 4.0 x16.** More video memory than the 8 GB card the presets were sized for, and the same
  12 GB as the RTX 5070 the base fork was measured on (about 18-22 fps there with 1,536 MB of card buffer copies).
  Start with `-Preset fast` (1,536 MB). Then compare `-Preset max` (2,048 MB). The launcher warns that max "may spill":
  that warning is about 8 GB cards. On 12 GB it should fit, but check Dedicated GPU memory in Task Manager.
  Optional comparison: `-Resolution 720` (sharper; on the reference CPU-bound setup it mostly costs GPU time).
  Never use 1080 on Series S: it crashes at start on every card.
- **The card buffer size is the main fps lever, not resolution**: frames are CPU-bound (on the reference PC about
  30 ms of a frame is the CPU preparing work and about 6 ms is the GPU drawing). Going below 540p barely helps.
- **Ryzen 7 7700: 8 Zen 4 cores, 16 threads, one kind of core, 32 MB L3.** Close to the 8-core Ryzen the base fork
  was tuned on, so the base's thread settings fit as they are. Leave the Intel-hybrid switches off
  (`VICTORY_SUBMIT_CORE`, `VICTORY_PIN_SPREAD`, `VICTORY_FAST_CORES`): they were written for 6 P-core + 8 E-core chips.
- **The 7700 has integrated graphics.** Make sure the monitor is plugged into the 4070 Ti, not the motherboard, and
  pass `-Adapter 4070` in the launcher wrapper and `VICTORY_ADAPTER=4070` for tests, so nothing runs on the iGPU.
  The run log names the card it picked ("host device on the card VICTORY_ADAPTER names").
- **32 GB DDR5 (AM5 is DDR5-only), Kingston Fury.** Expect an EXPO or XMP profile; check `ConfiguredClockSpeed`
  against the kit's rating. Over 24 GB, so the launcher leaves the memory-window setting (`-WindowsBarMB`) off.
- **Resizable BAR** should be on: BAR1 total in `nvidia-smi -q` close to 12 GB means on; 256 MB means off (section 6).
- **Driver**: a current NVIDIA Game Ready driver. Note the version for reports.

Write these facts and every setting you choose to `C:\codeprojects\victory-tools\my-setup.md` (create the folder).
The update task (section 9) reads that file.

## 3. Install everything (you do this; the user only clicks Yes on admin prompts)

First check what is there, then install only what is missing. Run the checks yourself and show the user a short
"have / need" list.

```powershell
winget --version                                   # App Installer; missing on very old Windows 10 builds
git --version
rustup --version; cargo --version
& "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe" -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin" -Directory -ErrorAction SilentlyContinue | Select-Object -Last 1 Name
nvidia-smi --query-gpu=driver_version --format=csv,noheader
Get-Command claude -ErrorAction SilentlyContinue
Get-PSDrive C | Select-Object Used,Free
Test-Path 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\Auto Update\RebootRequired'   # True: restart first
```

| Need | Why | How (you run it) |
|---|---|---|
| ~60 GB free on the build drive | Build ~10 GB, shader caches 20+ GB, page file 32 GB on C: | If short, tell the user and stop until there is room. Put `cache_dir`/`data_dir` on another drive if that helps. |
| winget | Installs the rest | If missing: `Add-AppxPackage -RegisterByFamilyName -MainPackage Microsoft.DesktopAppInstaller_8wekyb3d8bbwe`, else open the Microsoft Store page for "App Installer" for the **USER** to click Install. |
| Git for Windows | Clone and updates (the Code tab also uses it) | `winget install --id Git.Git -e --accept-source-agreements --accept-package-agreements` |
| Visual Studio 2022 Build Tools, C++ workload + Windows SDK | Rust's MSVC linker, the assembler for one crate, and `dxil.dll` for shader tests | `winget install --id Microsoft.VisualStudio.2022.BuildTools -e --accept-source-agreements --accept-package-agreements --override "--quiet --wait --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"` (several GB, 10-30 min; one UAC prompt) |
| rustup | Fetches the pinned Rust 1.98.1 by itself on the first `cargo` command in the repo | `winget install --id Rustlang.Rustup -e --accept-source-agreements --accept-package-agreements`. Don't install another toolchain. |
| Current NVIDIA driver | Game Ready driver for the 4070 Ti; Smooth Motion needs a recent one | Compare `nvidia-smi`'s version with the latest on nvidia.com for the RTX 4070 Ti. If older, install the NVIDIA App (`winget search "NVIDIA App"`, install the match published by NVIDIA) and have the **USER** click its driver update, or download the driver yourself and run it (UAC). |
| AMD chipset driver (AM5) | Correct power and core handling on the Ryzen 7 7700 | Check Installed apps for "AMD Chipset Software". If missing, download the latest AM5 chipset driver from amd.com and run it (UAC). |
| Claude Code command line | The update task's review step (section 9) | See the note below the table. Optional: updates still work without it. |
| Controller driver | Xbox-style pads work out of the box on Windows | Nothing to install for Xbox pads. Other pads need XInput mode (e.g. Steam Input or the pad's own switch). |

Claude Code command line: if `Get-Command claude` finds nothing (the desktop app doesn't always add it), run
`irm https://claude.ai/install.ps1 | iex` in PowerShell, then have the **USER** run `claude` once in a new PowerShell
window and sign in with the same account.

For admin steps with several lines, write them to a temporary `.ps1` under `C:\codeprojects\victory-tools` and run it
with `Start-Process powershell -Verb RunAs -Wait -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File','<that file>'`.

After installs, PATH changes only reach new processes. Refresh it in your shell instead of asking for a restart:
`$env:Path = [Environment]::GetEnvironmentVariable('Path','Machine') + ';' + [Environment]::GetEnvironmentVariable('Path','User')`,
then re-run the checks. Also run `git config --global core.longpaths true` (deep paths under `target\`).

If any install fails, read its log, retry once, and only then explain the problem to the user in one or two lines.

## 4. Clone, configure, build, test

```powershell
New-Item -ItemType Directory -Force C:\codeprojects | Out-Null
git clone -b nvidia-4060 https://github.com/potatoe0100/VIctory C:\codeprojects\victory
cd C:\codeprojects\victory
git fetch --tags
git checkout <newest tested tag from section 1>
```

Configure: copy `victory.example.toml` to `victory.toml` (git-ignored) in the repo folder. Set `game_dir` to the
user's own dump folder (**USER** gives you the path; check it against rule 0.3). Optionally set `cache_dir` and
`data_dir` on a fast drive with 20+ GB free. All paths must be OUTSIDE the repo (the tools refuse otherwise). After
the build below, check them with `target\diag\victory.exe config` (run from the repo folder).

Build (the launcher runs `target\diag\victory.exe`; the first build takes a long time):

```powershell
cargo build --locked --profile diag -p victory --features diagnostics
```

Test, the same way the reference PC tests each tag (needs no game data; uses the GPU; takes a while, so keep the PC
otherwise idle):

```powershell
$env:VICTORY_DYNAMIC_SLOTS = '524288'; $env:VICTORY_ADAPTER = '4070'
cargo test --workspace --locked --no-fail-fast -- --test-threads=1 --skip heap_direct_texture_reads_mark_their_heap_index
```

- Reference result for `tested-2026-11-27-2035` on the RTX 4060: 982 passed, 0 failed. A different card can give a
  slightly different count. The budget test `the_8gb_launcher_settings_fit_the_reported_budget` prints this card's
  numbers: keep them in `my-setup.md`.
- `views_off_their_elements_get_a_copy_of_their_bytes` is a known flaky base-fork test: re-run a failure alone with
  `cargo test --workspace --locked -- --exact <full test name> --test-threads=1` before calling it real.
- Shader tests failing with `Unknown shader model 'ps_6_6'` mean an old `dxil.dll` validator: install a current
  Windows SDK, or point `VICTORY_DXIL` at a current `dxil.dll` from Microsoft's DirectXShaderCompiler releases.
- Any other failure: do NOT change code. Note the test names and first error lines for the report (section 11).

## 5. Launcher, wrappers and desktop shortcut

The launcher is `scripts\play-4060.ps1 -Exe <the game's executable inside the user's own dump>`. It sets the speed
settings from `play-nvidia.ps1`, the preset, and `VICTORY_KBM_GTA=1` (keyboard and mouse, only while no controller is
connected). Options, from the script headers:

| Option | Meaning |
|---|---|
| `-Preset safe / balanced / fast / max` | Card buffer copies 512 (480p, levels on demand) / 1,024 / 1,536 / 2,048 MB. For a 12 GB card: start at `fast`. |
| `-Resolution 480 / 540 / 720` | Overrides the preset's render height (output stays 1080p). |
| `-Quality` | Fewer stretched bodies and less rainbow skin, about 7 fps slower. |
| `-NoSmoothMotion` | Runs `victory.exe` itself. By default it runs a copy, `victory_sm.exe`, so an NVIDIA App Smooth Motion profile can stick to it. |
| `-Adapter 4070` | The card to use (needed on this PC because of the iGPU). |
| `-Build diag / release` | Default diag; every measurement is of diag. |
| `-More @{VICTORY_X='1'}` | Extra settings for comparisons, e.g. `@{VICTORY_MOUSE_SENS='1.5'}`. |

Make wrappers OUTSIDE the repo, so updates never clash with them:

1. **USER** picks the game's executable inside their own dump. You only copy its path.
2. `C:\codeprojects\victory-tools\play.cmd`:
   ```bat
   @echo off
   cd /d C:\codeprojects\victory
   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\play-4060.ps1 -Exe "<path>" -Preset fast -Adapter 4070
   pause
   ```
   `pause` keeps the console open: it prints an fps line every 5 seconds and the last lines matter after a crash.
3. `play-nosm.cmd`: the same with `-NoSmoothMotion`, and `play-max.cmd` with `-Preset max`, for comparisons.
4. A Desktop shortcut "VIctory (experimental)" to `play.cmd`, start in `C:\codeprojects\victory-tools`, not run as admin.

Optional, before the first launch: translate every shader ahead of time (CPU only, minutes, can be stopped and
resumed). The shader cache key includes some launcher settings, so set them first or the work is wasted:

```powershell
cd C:\codeprojects\victory
$env:VICTORY_BVH_MISS = '1'; $env:VICTORY_NAN_SCRUB = '1'
target\diag\victory.exe shaders        # uses game_dir from victory.toml
```

First launch: it can sit on a black screen for 10+ minutes while shaders and pipelines are built. Don't kill it.
Later launches are much faster. Each new place still stalls briefly the first time.

## 6. PC settings (do the Windows ones yourself; BIOS and a few toggles are the USER's)

Order: do every item you can (A) first, then one restart round for the BIOS (B), then the NVIDIA App toggles (C).
Tell the user to save their work before any restart. Record each result in `my-setup.md`.

**A. You do these (each needs one UAC Yes from the USER):**

1. **Fixed 32 GB page file** (the game commits about 33 GB in the world; with a Windows-managed page file the
   reference PC came within 0.8 GB of the limit; running out shows as `0x8007000E` or error 1455 in `crashes.log`).
   Use the drive with the most free space if C: is short:
   ```powershell
   $cs = Get-CimInstance Win32_ComputerSystem
   Set-CimInstance -InputObject $cs -Property @{ AutomaticManagedPagefile = $false }
   $pf = Get-CimInstance Win32_PageFileSetting
   if (-not $pf) { $pf = New-CimInstance -ClassName Win32_PageFileSetting -Property @{ Name = 'C:\pagefile.sys' } }
   $pf | Set-CimInstance -Property @{ InitialSize = [uint32]32768; MaximumSize = [uint32]32768 }
   ```
   (Run as admin. Takes effect after the restart in B.)
2. **Power**: `powercfg /setactive SCHEME_MIN` (High performance; no admin needed). If that scheme is hidden, leave the
   plan and have the **USER** set Settings, System, Power, Power mode to "Best performance".
3. **Memory Integrity off** (optional): it saves some CPU time and turns off one security layer. Ask the **USER** first
   in one sentence. If yes (admin):
   `reg add "HKLM\SYSTEM\CurrentControlSet\Control\DeviceGuard\Scenarios\HypervisorEnforcedCodeIntegrity" /v Enabled /t REG_DWORD /d 0 /f`
   (takes effect after the restart in B; check afterwards in Windows Security, Device security, Core isolation).
4. **Hardware-accelerated GPU scheduling**: leave it as it is.

**B. One restart into the BIOS (USER, with you guiding by text; they can read your steps on their phone):**

Find the motherboard model first (`Get-CimInstance Win32_BaseBoard | Select-Object Manufacturer,Product`) so you can
name the exact menu paths for that board, and give the BIOS key (usually Del or F2 at power-on). Then, in one visit:

1. **EXPO (or XMP) profile on** for the Kingston Fury kit. Warn before they go in:
   - On AM5 the first boot after changing memory settings trains the memory and can show a black screen for several
     minutes. Wait at least 5 minutes before touching anything. "Memory Context Restore", if the board has it, makes
     later boots fast.
   - If the PC then won't boot and only the DRAM debug light stays on, it is not damaged. Turn it fully off, switch the
     PSU off for 30 seconds, and start again. If it repeats, clear the CMOS (the board's jumper or button, or the
     battery out for a minute) and run the memory one speed step below its rating.
2. **Resizable BAR**: "Above 4G Decoding" on and "Re-Size BAR Support" on (often under PCI Subsystem or Advanced;
   needs CSM off, which it already is on most Windows 11 installs).
3. Leave the integrated graphics on; just make sure the **monitor cable is in the graphics card**.
4. Save and exit. Back in Windows, the **USER** reopens this project and says "continue the VIctory setup". You then
   check: `ConfiguredClockSpeed` matches the kit, BAR1 total near 12 GB (`nvidia-smi -q`), the page file is 32768
   (`Get-CimInstance Win32_PageFileSetting`), Memory Integrity state. Fix anything that didn't stick.

**C. NVIDIA App toggles (USER clicks; you say exactly where):**

1. **Shader cache size 100 GB**: NVIDIA App, Graphics, Global settings, Shader Cache Size (or NVIDIA Control Panel,
   Manage 3D settings).
2. **Power management mode: Prefer maximum performance** (same list).
3. **Smooth Motion** (optional, never required): Graphics, Program settings, add
   `C:\codeprojects\victory\target\diag\victory_sm.exe` (exists after the first launch), turn Smooth Motion on. It's
   driver frame generation: smoother to look at, about one frame more input lag, smearing on fast turns. Report real
   fps, not generated.

**D. Before playing (USER):** plug in a controller if they have one (Xbox-style is easiest). Keyboard and mouse also
work while no controller is connected: F2 (or backtick) locks mouse-look, Esc or P releases it; left click shoots,
right click aims, WASD moves, Shift sprints, Space jumps, F enters vehicles, Tab weapon wheel. Full table in
`README-4060.md`. Close the browser and other heavy apps.

## 7. What to expect (from the reference PC's first run, 27 Nov 2026, build tested-2026-11-27-1425)

- About 16-19 fps average in the prologue: 18-30 indoors, 5-15 outdoors, the odd hitch of about a second. Frames are
  CPU-bound. On this PC (more video memory, similar CPU class) expect a similar range; that is an estimate, not a
  measurement.
- Picture faults seen: faces sometimes drawn as rainbow noise, some detached heads and floating hair, overexposed
  characters indoors, around 1,000 draws a frame dropped in some scenes. Soft picture (540p upscaled).
- In that run audio stuttered, mouse-look did nothing and there was no way to quit. The current tag
  (`tested-2026-11-27-2035`) adds fixes for all three; they pass tests but had not yet been played on 27 Nov.
- Quit with **Ctrl+Shift+Q**, the window's X, or Alt+F4: all shut down cleanly. The game's own menu has no quit.
- A red box on the minimap is the wanted-level flash, not a fault.
- Entering the world can reset the driver (black flash) or, rarely, crash the PC. Save work first.

## 8. Troubleshooting

1. "No ...victory.exe": build missing, rebuild.
2. Black screen on first launch: wait 10+ minutes; watch the console for fps lines.
3. Crash at start: resolution must be 720 or lower; try `-Preset balanced` or `safe`.
4. Wrong card used (very low fps, iGPU busy in Task Manager): check `-Adapter 4070` and the monitor cable.
5. Device lost / black flash / driver reset: a lower preset, close other apps, current driver, keep the last 30 console
   lines. Crash details go to `crashes.log` in the data folder (default `%LOCALAPPDATA%\victory\data`).
6. fps collapses after a while: video memory full (Task Manager, GPU, Dedicated GPU memory near 12 GB: go one preset
   down) or commit full (`0x8007000E`, error 1455: fix the page file).
7. Bad picture only: try `-Quality`. Laggy: compare `-NoSmoothMotion`.
8. Don't delete shader caches without telling the user it means another long first launch.

The console's 5-second line, like `20.5 fps, 48.8 ms/frame (...)`, splits each frame into preparing (CPU), on the GPU,
copying back, dropped draws and textures waiting. Use it to tell CPU-bound from GPU-bound from memory-starved.

## 9. Automatic updates (scheduled task)

The user wants this PC to follow the newest tested build automatically.

1. Download the update script next to the wrappers (it is plain PowerShell; read it before registering it, and tell
   the user in one line what it does):
   ```powershell
   Invoke-WebRequest https://raw.githubusercontent.com/potatoe0100/VIctory/friend-setup/victory-update.ps1 -OutFile C:\codeprojects\victory-tools\victory-update.ps1
   ```
2. `C:\codeprojects\victory-tools\settings.json`: `{"Adapter":"4070","SkipTests":false,"NoClaude":false}`.
3. Record the starting point and test it: run
   `powershell -NoProfile -ExecutionPolicy Bypass -File C:\codeprojects\victory-tools\victory-update.ps1`
   once by hand. It should say it is up to date and create `state.json`. Then try `-CheckOnly`.
4. Register the task for this user (no admin needed). Ask the user what time the PC is usually on but not in use;
   default 4 AM, with "run as soon as possible after a missed start":
   ```powershell
   $a = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument '-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File C:\codeprojects\victory-tools\victory-update.ps1'
   $t = New-ScheduledTaskTrigger -Daily -At 4am
   $s = New-ScheduledTaskSettingsSet -StartWhenAvailable -ExecutionTimeLimit (New-TimeSpan -Hours 4) -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries
   Register-ScheduledTask -TaskName 'VIctory update' -Action $a -Trigger $t -Settings $s -Description 'Moves VIctory to the newest tested build (experimental)'
   ```
5. For the Claude review step to work, Claude Code must be signed in for this Windows user (`claude -p "say hi"` from
   a normal PowerShell should answer). If not, the update still runs and just skips the review.

What the script does each run: skips if the game, cargo or another update is running; fetches; picks the newest tag by
the rule in section 1; if newer, checks it out, builds, runs the tests with this PC's settings, and goes back to the
previous build if the build or tests fail. After a successful move (or if this doc changed) it asks headless Claude,
limited to `victory-tools`, to check the release notes against `my-setup.md` and the wrappers, apply small needed
changes there, and summarise. The outcome lands in `VIctory update.txt` on the Desktop; details in
`victory-tools\update.log` and `victory-tools\logs\`.

Local changes: if this PC ever needs its own code change, commit it on a local branch named `local` (run
`cargo xtask install-hooks` first; the hook blocks game data in commits). The script rebases `local` onto each new
tag, and leaves everything alone with a Desktop note if they clash. Keep settings in the wrappers instead whenever
possible. Never leave uncommitted edits in the repo: the script won't update over them.

When the user asks you to "check the VIctory update" or the Desktop note says to: read `update.log`, fix what it
describes, and re-run the script by hand.

## 10. Updating by hand

`powershell -NoProfile -ExecutionPolicy Bypass -File C:\codeprojects\victory-tools\victory-update.ps1`. Don't run
`scripts\sync-4060.ps1` (it's for the branch's maintainer). Don't `git pull` over a changed tree.

## 11. Reporting results (to the person who sent this doc, privately; never publicly)

Write a short note for the user to send:
1. Card, VRAM, driver, CPU, RAM speed, PCIe link, BAR1 size, Windows version.
2. Tag, wrapper and options used; which section 6 steps were done.
3. Test line (passed / failed / ignored) and any failing test names; the budget test's printed numbers.
4. Where in the game (menu, prologue indoors, outdoors), the fps lines from there, and whether Smooth Motion was on.
5. Crash or black screen: the last 30 or so console lines and the newest `crashes.log` entry, with anything quoting
   game content removed.
6. Dedicated GPU memory and Committed memory from Task Manager while in the world.

No game files, game screenshots or video, shader or texture dumps, or logs that quote game content.

## 12. Finish

Tell the user briefly: what's installed, which tag, the shortcut name, the update task's time, which section 6 steps
are still open, how to quit (Ctrl+Shift+Q), and what to send back.
