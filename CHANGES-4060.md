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
| `scripts/sync-4060.ps1` | this branch | Follows VIctory-NVIDIA and lists new upstream GPU commits to port |

## Already in the base (not merged again)

| Upstream PR | What the fork has instead |
| --- | --- |
| #2461 NVIDIA view heap cap | Own `probe_view_limit`: shaders clamp view indices to the capped heap's null slot, and `VICTORY_DYNAMIC_SLOTS` takes more dynamic slots on a capped heap. The heap tests that fail on upstream main all pass here on the 4060. |
| #1215 fast copy-back | `NtWriteVirtualMemory` copy-back in `host.rs` |
| Texture budget | `VICTORY_TEXTURE_BUDGET_MB`, and by default textures stop at 90% of the budget Windows reports |

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
