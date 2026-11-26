# VIctory for 8 GB NVIDIA cards (branch `nvidia-4060`)

**Experimental, AI-assisted (Claude), and not yet run against the game.** Nobody has seen this branch reach the
world yet. Expect crashes. It is not an official or finished release of anything.

It is [trustno1-coder/VIctory-NVIDIA](https://github.com/trustno1-coder/VIctory-NVIDIA) (Series S, half render
scale, ray tracing mostly off; about 20 fps on a 12 GB RTX 5070 per its README) with settings for 8 GB cards such as
the RTX 4060. What was added and why is in [CHANGES-4060.md](CHANGES-4060.md).

## What to expect

- On a 12 GB card the base fork gets about 20 fps in the prologue. An 8 GB card is slower and has less room, so
  expect lower, and expect the picture to be soft (it renders at 540p and upscales).
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

Options: `-Resolution 480` (faster, softer) or `-Resolution 720` (sharper, slower); `-Quality` (fewer stretched
bodies, slower); `-NoSmoothMotion`.

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
