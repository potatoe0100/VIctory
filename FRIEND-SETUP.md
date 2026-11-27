# FRIEND-SETUP.md (instructions for Claude Code)

EXPERIMENTAL and AI-assisted (Claude). An 8 GB NVIDIA build of the VIctory GTA VI recompilation layer. Nobody has seen it
run well yet. Expect crashes and about 15-20 fps. It is not an official release of anything.

You are Claude Code running on the user's own Windows PC. Follow this document step by step. The user is not a developer:
explain in plain language, do the work yourself where you can, and only hand the user the steps marked **USER**.
Fetch this file fresh with its raw URL if you lose it. Do not post anything to GitHub or anywhere public.

## 0. Hard rules (read first)

1. The user must use their OWN legally obtained dump of the game, from a copy they bought. Never copy, upload, commit,
   share, quote or link game files or data (archives, executable, shaders, textures, models, audio, video), including in
   logs or reports.
2. No keys, decryption, or protection workarounds. Do not help with any.
3. If the folder the user points at looks like a pirated or "scene" release (names with release-group tags, cracks, NFO
   files, or a downloaded torrent), STOP. Do not open, set up or run it. Tell the user plainly that it must be a dump from
   their own purchase. Everything else in this document can still be done without it.
4. Never commit build output, `victory.toml`, caches or game data. Run `git status` before any commit (you should not need one).
5. Do not run anything as administrator except where a step below says so.

## 1. Source and version

- Repo: https://github.com/potatoe0100/VIctory (public fork; upstream Marvisak/VIctory, GPL-3; base trustno1-coder/VIctory-NVIDIA).
- Branch: `nvidia-4060`. Start from tag `tested-2026-11-27-1425` (full test suite and the 8 GB budget test passed on it).
- Branch heads and newer tags may be ahead of that tag. Stay on the tag unless the user tells you to move.
- Read in the repo after cloning: `README-4060.md` (what this branch is), `CHANGES-4060.md` (what was added and why),
  `README.md` (base fork, how it works), `scripts/play-4060.ps1` and `scripts/play-nvidia.ps1` (the launchers, options are
  documented at the top of each), `victory.example.toml`.
- Known to be missing from this tag: mouse-look, the audio fixes, a quit hotkey. They are being worked on elsewhere.

## 2. Collect the user's specs

Ask the user for anything missing, or detect it yourself:

```powershell
Get-CimInstance Win32_Processor | Select Name,NumberOfCores,NumberOfLogicalProcessors
Get-CimInstance Win32_VideoController | Select Name,DriverVersion,AdapterRAM
nvidia-smi --query-gpu=name,driver_version,memory.total,pcie.link.gen.current,pcie.link.width.current --format=csv
Get-CimInstance Win32_ComputerSystem | Select TotalPhysicalMemory
Get-ComputerInfo | Select OsName,OsBuildNumber
Get-CimInstance Win32_PageFileUsage
```

Requirements (from the README): Windows 10/11 x64, CPU with AVX2, a D3D12 GPU with shader model 6.6 and mesh shaders
(RTX 20-series or newer), an NVIDIA driver that is current. The branch was tested on an RTX 4060 (8 GB), driver 621.14,
i5-13600KF, 32 GB DDR5, Windows 11, Resizable BAR on, PCIe 4.0 x8.

Things that change the result, report them to the user:
- GPU VRAM: 8 GB is what the presets are sized for. See section 7 for other cards.
- PCIe link: this layer moves tens of MB per frame to the card, so link width matters. A card on PCIe 4.0 x4 ran about
  25 percent slower than x16 in the base fork's test. Check the `nvidia-smi` link output while the card is busy (it drops at idle).
- RAM: 32 GB or more is expected. Under 24 GB the launcher turns on a memory-window setting by itself.
- Resizable BAR: should be on (NVIDIA App or `nvidia-smi -q` shows it). If off, tell the user (section 6).

## 3. Install prerequisites

Check each, install only what is missing, prefer `winget`:

```powershell
winget install --id Git.Git -e
winget install --id Rustlang.Rustup -e
winget install --id Microsoft.VisualStudio.2022.BuildTools -e --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```

- The Rust toolchain is pinned in `rust-toolchain.toml` (channel 1.98.1, target x86_64-pc-windows-msvc, with rustfmt and
  clippy). Do not pick another one: after cloning, `rustup` fetches the pinned one on the first `cargo` call.
- Open a NEW terminal after installs so PATH updates. Check `git --version`, `rustc --version`, `cargo --version`, and that
  `link.exe` is found from a Developer PowerShell (the MSVC tools). A Windows SDK is needed (the VCTools workload includes it).
- Windows PowerShell 5.1 runs the scripts; PowerShell 7 is not required.

## 4. Clone, build, test

Use `C:\codeprojects\victory` (or the user's preferred short path, with no spaces). Game data, cache and saves go OUTSIDE
this folder (the tools refuse otherwise).

```powershell
git clone -b nvidia-4060 https://github.com/potatoe0100/VIctory C:\codeprojects\victory
cd C:\codeprojects\victory
git checkout tested-2026-11-27-1425
```

Config: copy `victory.example.toml` to `victory.toml` (git-ignored). Set `game_dir` to the user's own dump folder (USER
provides the path, step 5). Optional `cache_dir` and `data_dir` on a drive with space (the shader cache grows large, plan
for 20+ GB free). Env vars override the file; `VICTORY_CONFIG` picks another file.

Build (the launcher expects `target\diag\victory.exe`; the first build takes a long time, it is normal):

```powershell
cargo build --locked --profile diag -p victory --features diagnostics
```

Test (the same command the branch was tested with; needs a D3D12 GPU, takes a while, run it with the PC otherwise idle):

```powershell
cargo test --workspace --locked --no-fail-fast -- --test-threads=1 --skip heap_direct_texture_reads_mark_their_heap_index
```

Expected on the tagged build on the reference PC: 962+ passed, 0 failed, 45 ignored (the tag later reports 967 with its extra
tests). One known flaky test from the base fork, `views_off_their_elements_get_a_copy_of_their_bytes`, failed once in a
long soak: re-run just that test before calling it a failure. If anything else fails, do NOT fix code. Collect the failing
test names and the first error lines and tell the user to send them back (section 11).

## 5. Launcher and desktop shortcut

Launch script: `scripts\play-4060.ps1 -Exe <path to the game's executable inside the user's own dump>`. Run it from a
normal (non-admin) window: the driver keeps its shader cache elsewhere when elevated.

Options (documented in the script header):

| Option | Meaning |
|---|---|
| `-Preset safe` | 480p, 512 MB card buffer copies, levels on demand. Least video memory. |
| `-Preset balanced` | 540p, 1024 MB. Default. |
| `-Preset fast` | 540p, 1536 MB. |
| `-Preset max` | 540p, 2048 MB. Tightest fit. |
| `-Resolution 480\|540\|720` | Overrides the preset's height. 1080 on Series S crashes at start on every card. |
| `-Quality` | Fewer stretched bodies and rainbow skin, about 7 fps slower. |
| `-NoSmoothMotion` | Run `victory.exe` itself. By default `victory_sm.exe` (a copy) is run so an NVIDIA App Smooth Motion profile sticks. |
| `-Build diag\|release` | Default diag (all tests and measurements are of diag). |
| `-WindowsBarMB N` | Title memory windows in CPU-mapped VRAM. Launcher sets 1536 on PCs under 24 GB RAM, off on 24 GB or more. |
| `-Adapter 4060` or `-Adapter '#1'` | Choose the card on a multi-GPU PC. |
| `-More @{VICTORY_X='1'}` | Extra env settings, for A/B tests. |

Make a desktop shortcut for the user:
1. Ask the USER for the game's executable path (they pick it, you never touch or copy the game files). Store it in a small
   wrapper `C:\codeprojects\victory-play.cmd` OUTSIDE the repo, for example:
   `powershell -NoProfile -ExecutionPolicy Bypass -File C:\codeprojects\victory\scripts\play-4060.ps1 -Exe "<path>"`
   followed by `pause`, so the console (it prints an fps line every 5 seconds) stays readable.
2. Shortcut on the Desktop named "VIctory 4060 (experimental)" pointing at that .cmd, "Start in" `C:\codeprojects\victory`,
   not set to run as administrator.
3. Make a second wrapper with `-NoSmoothMotion` if the user wants to compare.

Before the first launch (optional, saves stalls; both can be stopped and continued later). Follow README.md "How to run
it": `target\diag\victory.exe shaders <the shader archives>` translates every shader on the CPU in minutes, and
`target\diag\victory.exe warm --used` builds pipelines earlier runs drew with (useful after a driver or build change).
The shader archive paths are inside the user's own dump: the user gives you the folder, you only pass the path on.

First launch: it can sit on a black screen for 10+ minutes while shaders and pipelines build. Do not kill it. Later
launches are much faster. The first time a new place is seen it stalls briefly (shaders being made).

## 6. By-hand steps (walk the user through each one, you cannot do these)

Do them in this order, one at a time, and confirm each. Mark which are optional.

1. **XMP/EXPO memory profile on** in the BIOS (recommended, the reference PC runs DDR5-6000 with XMP).
   CAUTION: after a restart with XMP or any overclock, some PCs hang with no picture and only the DRAM debug light on the
   board. It is not damage. Fix: turn the PC fully off, switch the PSU off for 30 seconds (or hold the power button), start
   again; if it repeats, clear the CMOS (jumper or battery out for a minute), then run the memory one speed step slower
   than its XMP rating. Tell the user this BEFORE they enter the BIOS, and to save everything first.
2. **Resizable BAR on** (BIOS: "Above 4G Decoding" and "Re-Size BAR"; also needs a recent NVIDIA driver). Check it in the NVIDIA App
   or with `nvidia-smi -q` (BAR1 total should be about the full VRAM size).
3. **Fixed page file of 32768 MB** (initial = maximum) if the PC has 32 GB RAM. The world commits about 33 GB, a Windows-managed
   file at about 12 GB ran within 0.8 GB of the limit in the first run. Path: Start, type `advanced system settings`,
   Performance Settings, Advanced tab, Virtual memory Change, untick "Automatically manage", C: (or the fastest SSD),
   Custom size 32768 and 32768, Set, OK, restart. Out of commit shows as error `0x8007000E`.
4. **NVIDIA shader cache size 100 GB**: NVIDIA App, Graphics, Global settings (or NVIDIA Control Panel, Manage 3D settings,
   Shader Cache Size).
5. **Power management mode: Prefer maximum performance** (same place), and Windows power plan High/Best performance.
6. **Memory Integrity off** (optional): Windows Security, Device security, Core isolation, Memory integrity. Needs a restart. It costs
   some CPU time; turning it off lowers one layer of protection, so it is the user's call. Say so in one sentence.
7. **Smooth Motion** (optional, off by default, never required): NVIDIA App, Graphics, Programs, add `victory_sm.exe`
   from `C:\codeprojects\victory\target\diag\` and turn Smooth Motion on for it. It is a driver frame generator: it hides low
   fps visually but adds about a frame of lag and smears on fast camera moves. Real fps is what to report, not the generated one.
8. **Controller**: use a gamepad. WASD works but mouse-look and mouse clicks do nothing on this tag. Keyboard stand-in pad:
   Z aim (LT), C shoot (RT), I/J/K/L camera, Enter = A, Backspace = B.
9. Close browsers and other GPU-hungry apps before playing (a browser can take ~1 GB of VRAM). Close other heavy
   programs (including any build or test run) so the numbers are not skewed.

## 7. Other cards: adapting presets and the VRAM budget

The presets are sized for an 8 GB card with a budget of about 7.2 GB reported by Windows. The test
`the_8gb_launcher_settings_fit_the_reported_budget` in `crates/victory-gpu/src/shadow.rs` (`PRESETS_4060`) checks the
arithmetic: card buffer copies + 3,072 MiB reserved for render targets + a texture floor, under 90 percent of the budget.

- 8 GB NVIDIA: use the presets as they are. Start with `balanced`.
- 6 GB or less: try `safe`, then `-More @{VICTORY_LEVELS_ON_DEMAND='1'}`. It may not fit; say so honestly.
- 10 GB to 16 GB: `fast` or `max` should fit, and `-Resolution 720` may be affordable. The base fork README has 5070 12 GB figures
  (about 18-22 fps at 540p) for reference. `play-nvidia.ps1` defaults apply with 1536 MB.
- Do not edit the presets' code unless the user asks. Prefer `-More @{VICTORY_CARD_BUFFERS_MB='...'}`. Past the budget
  Windows moves card memory into system RAM and fps collapses: that is the main failure to avoid. Watch Task Manager,
  Performance, GPU, Dedicated GPU memory while in the world.
- The card buffer size is the main fps lever (more is faster until about 1536 MB); the render height is not, because the
  frame is CPU-bound. Lowering resolution below 540p barely helps.
- If video memory runs out, also try `-More @{VICTORY_LEVELS_ON_DEMAND='1'}` (its card test passed on an RTX 4060; not tried
  in the game on this branch).
- AMD or Intel cards are out of scope for this branch.

## 8. What to expect (known issues from the first run on the reference PC)

- 16-19 fps average in the prologue (roughly 18-30 fps indoors, 5-15 outdoors); frame time is CPU-bound (about 30 ms preparing,
  6 ms on the GPU). Occasional hitches of about a second. About 1,000 draws a frame dropped in some scenes (missing shaders).
- Audio is stuttery and breaks up (a fix exists on a side branch, not in this tag).
- No mouse-look, no mouse clicks. Use a controller.
- The in-game menu has no "quit to desktop". Quit with Alt+F4 or the window's X: it shuts down cleanly and saves the pipeline library.
- The red box on the minimap is the wanted-level flash, not a fault.
- Faces can render as rainbow noise, heads can be detached, indoor characters can look overexposed: known picture faults.
- Possible crash or driver reset on entering the world; one RTX 4060 owner elsewhere got a blue screen. Save work and close apps first.
- Wherever it goes quiet or black on first launch: wait, shaders are building.

## 9. Updates

```powershell
cd C:\codeprojects\victory
git fetch --tags origin
git tag -l "tested-*"
```

Move only to a tag the user names (ask the person who sent them this doc which tag is current). Then:
`git checkout <tag>`, rebuild with the same cargo command, re-run the tests. After a new build or driver, run
`target\diag\victory.exe warm --used` or expect another shader stall. Do not run `scripts\sync-4060.ps1` (it is for the
maintainer and merges upstream). Never `git pull` onto a changed tree without telling the user.

## 10. Troubleshooting ladder

1. Does the launcher print "No ...victory.exe"? Build is missing: rebuild.
2. Black screen: wait 10+ minutes on a first launch. Watch the console for fps lines.
3. Crashes at start: check the resolution is 720 or lower on Series S, run `-Preset safe`.
4. Device lost, black flash, driver reset: lower the preset (`safe`), close other apps, update the NVIDIA driver, and note the
   last 30 console lines.
5. fps collapsed after a while: video memory over budget (Task Manager Dedicated GPU memory near the card size), or commit
   limit reached (`0x8007000E`). Go down one preset, fix the page file.
6. Weird picture only: try `-Quality`.
7. Stutter and lag: compare `-NoSmoothMotion`. Close background apps.
8. Never delete the shader cache to "fix" something without telling the user it costs another long first-launch wait.

The console prints a line every 5 seconds like `20.5 fps, 48.8 ms/frame (...)` with a split into preparing, on the GPU,
copying back, dropped draws and textures waiting. It tells whether the run is memory-starved, CPU-bound or GPU-bound.

## 11. Reporting results (send to the person who gave you this doc, not publicly)

Write a short text note for the user to send back:
1. Card, VRAM, driver version, CPU, RAM, PCIe link, Windows version.
2. Tag, preset and options used, which by-hand steps were done (XMP, ReBAR, page file, Memory Integrity, Smooth Motion).
3. Test result line (passed/failed/ignored) and any failing test names.
4. Where in the game (menu, prologue indoors, outdoors) and the fps lines from there, plus real vs generated fps.
5. Crash or black screen? The last 30 or so console lines, with anything quoting game content removed.
6. Dedicated GPU memory and commit from Task Manager while in the world.

Do not include game files, screenshots or video of the game, shader or texture dumps, or logs that quote game content.
Do not post to GitHub. The log files under `%LOCALAPPDATA%\victory-nvidia-play\logs\` stay on the PC unless the user chooses to share an excerpt with you checked first.

## 12. Finish

When done, tell the user in short plain sentences: what is installed, which tag, the shortcut's name, which by-hand steps
are still open, how to quit (Alt+F4), and what to send back. Keep it brief.
