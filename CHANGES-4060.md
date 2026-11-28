# nvidia-4060-on-nv-main

Experimental, AI-assisted (Claude). VIctory-NVIDIA main (63b902fe) plus a small set of changes for an 8 GB card
(RTX 4060) on an Intel PC. Not yet measured in game; each change below is kept only if it measurably helps.

## 8 GB card (each has a switch)

- Small-card lines, off until measured (`VICTORY_SMALL_CARD_LINES=1`): on budgets of 9 GiB or less the texture line
  94 % and room for card buffer copies at most a sixteenth of the budget.
- Bindless feedback (skornuta, `VICTORY_BINDLESS_FEEDBACK=1` set by play.rs): NV's slot eviction may drop textures
  picked by index that no shader has read for 60 s.
- Render-target room at most a quarter of the budget on cards of 9 GiB or less (3,072 MiB was half of a 4060's
  6.2 GB budget; the 8 GB presets did not fit it with 1 GiB of the card used by other programs).
- Texture room only while the card is under its budget (edupode a0ead598): free parts of mostly empty blocks no
  longer count as room once the card is past it.
- List upload buffers pooled (edupode 62bba05, `VICTORY_UPLOAD_POOL=0` backs out).
- Texture pool: 3 spare 64 MiB blocks kept on small cards (64 before, 4 GiB), 2 made ahead (`VICTORY_RIPE_BLOCKS`).
- Ray tracing pages by address: at most 1 GiB on small cards (`VICTORY_RT_MEMORY_MB`).
- Tile floor 256 KiB under 9 GiB; opt-in top-mip cap (`VICTORY_LEVELS_MAX_DIM`) and tile reclaim
  (`VICTORY_LEVELS_RECLAIM=1`), both off.

## Intel (off by default)

- `VICTORY_SUBMIT_CORE=1`: the submitting thread gets a fast physical core to itself (2: the process's other
  threads kept off it too).
- (Our branch's pin shift and spread are diagnostics-build only and are left out here.)

## Launcher

`scripts/play-4060.ps1`: play-foxhound.ps1 with the Series S profile, its own upscaling, `-Preset
safe|balanced|fast|max` (512 to 2048 MB of buffer copies; a test checks they fit the card's budget), and the
settings 8 GB users measured: `VICTORY_PORT_CONSTANTS=1`, `VICTORY_CARD_WINDOWS_MB=768`, `VICTORY_THIN_THREADS=4`,
`VICTORY_CARD_AWAKE=0`.

## Other

- A removal report names the call that removed the device.
- `release-lto` / `diag-lto` profiles (thin LTO, one codegen unit) for A/B.
