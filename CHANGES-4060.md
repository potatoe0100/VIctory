# nvidia-4060: what's in this branch and why

EXPERIMENTAL, AI-assisted (Claude), not yet run against the game. Do not treat it as working.

Base: trustno1-coder/VIctory-NVIDIA at 7480e96 (Series S, half render scale, ray tracing mostly off, card-side memory
compares; 18-22 fps measured by its author on a 12 GB RTX 5070). Goal: fit that in an 8 GB card (RTX 4060).

Rule for adding anything: it must fix or prevent something specific on an 8 GB NVIDIA card.

## Merged

| Change | Source | Why on 8 GB NVIDIA |
| --- | --- | --- |
| Device-removal report names the call that removed it | potatoe0100 `nvidia/removal-names-its-trigger` | Crash reports from testers say what failed, so fixes can be targeted |
| `scripts/play-4060.ps1` | this branch | The fork's launcher keeps 1536 MB of card-side buffer copies (sized on 12 GB); this uses 768 MB |
| VIctory-NVIDIA up to 6a31e43 (27 Nov) | automatic update, tests passed | Mouse moves the camera; GPU timings include memory compares; title memory windows in card memory on PCs under 24 GB RAM |
| `play-4060.ps1` windows cap 1536 MB | this branch | play-nvidia.ps1's 3072 MB of windows (sized for 12 GB+) plus render targets would pass the 8 GB card's budget |
| `scripts/sync-4060.ps1` | this branch | Follows VIctory-NVIDIA and lists new upstream GPU commits to port |

## Already in the base (not merged again)

| Upstream PR | What the fork has instead |
| --- | --- |
| #2461 NVIDIA view heap cap | Own `probe_view_limit`: shaders clamp view indices to the capped heap's null slot, and `VICTORY_DYNAMIC_SLOTS` takes more dynamic slots on a capped heap. The heap tests that fail on upstream main all pass here on the 4060. |
| #1215 fast copy-back | `NtWriteVirtualMemory` copy-back in `host.rs` |
| Texture budget | `VICTORY_TEXTURE_BUDGET_MB`, and by default textures stop at 90% of the budget Windows reports |
| #1272 mip-drop | "Held levels" (`VICTORY_HELD_LEVELS`, on by default): a host copy starts at the first level the title's views show, so levels the title doesn't hold take no card memory (1.2-1.6 GB shown of 6.3-6.7 GB in the fork's recorded frames). No quality loss, unlike dropping levels the title does use. Optional "levels on demand" (`VICTORY_LEVELS_ON_DEMAND=1`, off) goes further with reserved textures. |
| #2872 evict past budget | Idle host copies are let go and remade when used, and new copies wait for room under the budget |

## Not ported (yet)

| PR | Why |
| --- | --- |
| #1272 mip-drop (rebased branch `mipdrop-1272-rebased`) | Clashes with the fork's held-levels code in the exact place that shifts a host copy's level 0 (views, read-back, uploads). Both at once risks the same off-by-levels read-back bug found in #1272's review. Port only if testers on 8 GB see texture creation refused (E_OUTOFMEMORY) in their logs. |
| #2872 evict (branch `evict-2872-present-fix`) | Overlaps the fork's let-go-idle sweep and its room checks; 1,200 lines into the same resource code. Port only if testers see the card stuck over budget. |

## Left out

| PR | Why |
| --- | --- |
| #2875 loose windows in GPU_UPLOAD | No budget check: on 8 GB it can push the card past its budget |
| Merging upstream main | About 600 clashing spots with the fork; ported piece by piece instead (`sync-4060.ps1`) |

## Tests on the RTX 4060 (driver 621.14, Windows 11)

`cargo test --workspace --locked --no-fail-fast -- --test-threads=1 --skip heap_direct_texture_reads_mark_their_heap_index`

| Build | Passed | Failed | Ignored |
| --- | --- | --- | --- |
| VIctory-NVIDIA 7480e96 (base) | 927 | 0 | 45 |
| nvidia-4060 (with the crash-report fix) | 931 | 0 | 45 |

Also on the 4060:
- `levels_on_demand_on_the_card` (ignored by default; `VICTORY_TILES_TEST_CARD=1`, release): all steps passed on the
  card, tiled resources tier 4, debug layer on. So `VICTORY_LEVELS_ON_DEMAND=1` is safe to try on this card.
- `the_8gb_launcher_settings_fit_the_reported_budget`: budget 7,188 MiB with the desktop running; card buffers 768 +
  render targets 3,072 + texture floor 1,797 = 5,637 MiB, under the 90% line of 6,469 MiB.

## Also left out

| Branch | Why |
| --- | --- |
| `nvidia-xtx-ports` (zero-copy renderer ports 5-7) | Brings in avatarmanz/VIctory-7900XTX up to c9c932c, which merges code from codec-d/cool_fox (commit a12870a), a CGBJ-derived source this branch does not use. The ports are off by default and save CPU copy time, not video memory. |
