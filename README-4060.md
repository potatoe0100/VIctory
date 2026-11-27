# VIctory for 8 GB NVIDIA cards (branch `nvidia-4060`)

**Experimental, AI-assisted (Claude), and not yet run against the game.** Nobody has seen this branch reach the
world yet. Expect crashes. It is not an official or finished release of anything.

It is [trustno1-coder/VIctory-NVIDIA](https://github.com/trustno1-coder/VIctory-NVIDIA) (Series S, half render
scale, ray tracing mostly off; about 20 fps on a 12 GB RTX 5070 per its README) with settings for 8 GB cards such as
the RTX 4060. What was added and why is in [CHANGES-4060.md](CHANGES-4060.md).

## What to expect

- On a 12 GB card the base fork gets about 20 fps in the prologue. On an 8 GB RTX 4060 expect about 10 to 17 real
  fps (an unmeasured estimate), and a soft picture (it renders at 540p and upscales).
- NVIDIA Smooth Motion (driver frame generation) is used to start with, because the real frame rate is low: the
  launcher runs `victory_sm.exe`, and ticking Smooth Motion for that program in the NVIDIA App turns it on. Below
  about 15 real fps expect smearing on fast camera moves and about one frame of extra input lag. The aim is to turn it
  off (`-NoSmoothMotion`) once the real frame rate allows.
- The first time a place is seen it stalls while shaders are made.
- If the card runs out of video memory, Windows moves part of it into the PC's memory and the frame rate falls hard.
  That is the main thing this branch tries to avoid.

## Build and run

Needs Windows 10/11 x64, Rust (the toolchain is pinned, rustup fetches it), the MSVC build tools, and your own
legally obtained dump of the title. Nothing from the game is in this repository.

```powershell
git clone -b nvidia-4060 https://github.com/potatoe0100/VIctory
cd VIctory
copy victory.example.toml victory.toml      # then set your paths, outside this folder
cargo build --locked --profile diag -p victory --features diagnostics
scripts\play-4060.ps1 -Exe <path of your copy's executable>
```

Presets: `-Preset safe` (480p, least video memory), `-Preset balanced` (540p, the default) or `-Preset fast`
(480p, more buffer copies on the card). Each is checked to fit an 8 GB card's budget, also with 1 GB of it taken by
other programs. Other options: `-Resolution 480` (faster, softer) or `-Resolution 720` (sharper, slower); `-Quality` (fewer stretched
bodies, slower); `-NoSmoothMotion`. If video memory runs out, try
`-More @{VICTORY_LEVELS_ON_DEMAND='1'}`: textures take memory only for the levels the game holds. Its test passes on
an RTX 4060, but it hasn't been tried in the game on this branch.

## Keyboard and mouse

The game reads a controller. With none connected, `play-4060.ps1` sets `VICTORY_KBM_GTA=1` (upstream pull request
3345): a GTA V PC-style layout on the stand-in pad. Press `F2` or `` ` `` to lock mouse look (the pointer hides and
stays in the window); `Esc`, `P` or switching windows lets it go. `VICTORY_MOUSE_SENS` scales the camera speed
(default 1.0), `VICTORY_MOUSE_INVERT_Y=1` inverts it. `Ctrl+Shift+Q` ends the game (it has no quit option of its own).

| Keys / mouse | Pad |
| --- | --- |
| W A S D | left stick (move, steer) |
| mouse (locked) or I J K L | right stick (camera, aim) |
| left / right click | RT (shoot, gas) / LT (aim, brake) |
| Shift or Enter, R or Backspace, Space, F | A (sprint, confirm), B (reload, back), X (jump), Y (vehicle) |
| Tab / Q | LB (weapon wheel) / RB (cover, handbrake) |
| Ctrl, C or middle click or Caps Lock | L3 (crouch, horn), R3 (look behind) |
| E or Right, Up or M, Down or Z, Left or G | D-pad |
| Esc or P, V | Menu (pause), View (camera) |

## Reporting how it runs

Every 5 seconds the console window prints a line like `20.5 fps, 48.8 ms/frame (...)`. Please note:

1. Your card, its video memory, driver version, CPU and RAM.
2. Which script and options you used.
3. Where you were (menu, prologue indoors, outdoors) and the fps lines from there.
4. Whether it crashed or the screen went black, and the last 30 or so lines of the console if it did.
5. Video memory use from Task Manager (Performance, GPU, "Dedicated GPU memory") while in the world.

Don't post anything taken from the game itself (files, shaders, textures, logs that quote them).

## Keeping up to date

`scripts\sync-4060.ps1` merges new VIctory-NVIDIA commits and lists new upstream (Marvisak/VIctory) GPU commits
that could be ported.
