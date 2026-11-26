# Keeps the nvidia-4060 branch up to date.
#   scripts\sync-4060.ps1            merge new VIctory-NVIDIA commits, then list new upstream GPU commits to port
#   scripts\sync-4060.ps1 -Test      also build and run the test suite afterwards
#
# Why not rebase on upstream Marvisak/VIctory directly: this branch is based on trustno1-coder/VIctory-NVIDIA, which
# is hundreds of commits apart from upstream (merging upstream main into it clashes in about 600 places). So this
# script follows the fork (a plain merge) and only *lists* upstream commits since the last sync that touch the GPU,
# shader or memory code, so they can be ported one at a time when they fix something on an 8 GB NVIDIA card.
param([switch]$Test)
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')

function Add-RemoteIfMissing($name, $url) {
    if (-not (git remote | Select-String -SimpleMatch -Pattern $name -Quiet)) { git remote add $name $url }
}
Add-RemoteIfMissing nvidia https://github.com/trustno1-coder/VIctory-NVIDIA.git
Add-RemoteIfMissing upstream https://github.com/Marvisak/VIctory.git

if (git status --porcelain --untracked-files=no) { throw 'Uncommitted changes: commit or stash them first.' }
if ((git branch --show-current) -ne 'nvidia-4060') { throw 'Run this on the nvidia-4060 branch.' }

git fetch nvidia main
git fetch upstream main

$new = git rev-list --count HEAD..nvidia/main
Write-Host "VIctory-NVIDIA: $new new commit(s)"
if ([int]$new -gt 0) {
    git merge --no-ff nvidia/main -m 'Merge VIctory-NVIDIA main'
    if ($LASTEXITCODE -ne 0) { throw 'Merge clashed: fix the conflicts, commit, then run this again.' }
}

# Upstream commits since the last time this script ran (the marker file is not committed).
$mark = Join-Path (git rev-parse --git-dir) 'sync-4060-upstream'
$since = if (Test-Path $mark) { (Get-Content $mark -Raw).Trim() } else { git merge-base HEAD upstream/main }
$paths = 'crates/victory-gpu', 'crates/victory-shader', 'crates/victory-hle/src/modules/xmem.rs'
Write-Host "`nUpstream commits since $($since.Substring(0, 8)) touching the GPU, shader or memory code:"
git log --oneline --no-merges "$since..upstream/main" -- @paths
(git rev-parse upstream/main) | Set-Content $mark
Write-Host "`nPort one with: git cherry-pick -x <commit>   (and add a line to CHANGES-4060.md saying what it fixes on 8 GB)"

if ($Test) {
    $vc = 'C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat'
    cmd /c "`"$vc`" >nul 2>&1 && cargo test --workspace --locked --no-fail-fast -- --test-threads=1 --skip heap_direct_texture_reads_mark_their_heap_index"
}
