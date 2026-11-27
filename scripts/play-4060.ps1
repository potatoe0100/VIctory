# EXPERIMENTAL, AI-assisted, untested against the game. Starts the title on an 8 GB NVIDIA card (RTX 4060 class).
#   scripts\play-4060.ps1 -Exe <path of your own copy's executable> [-Resolution 480|540|720] [-Quality] [-NoSmoothMotion]
#
# This is scripts\play-nvidia.ps1 (Series S, 540p, ray tracing mostly off) with settings that fit 8 GB:
# - VICTORY_CARD_BUFFERS_MB 768 instead of 1536. The 1536 MB figure was measured on a 12 GB RTX 5070; on an 8 GB
#   card the system's video memory budget is about 7.5 GB, and past it Windows moves the card's memory into the PC's
#   (much slower, and what ended the fork's runs 20 and 24).
# - On PCs with under 24 GB of RAM, play-nvidia.ps1 puts up to 3072 MB of title memory windows in the card's memory
#   (VICTORY_WINDOWS_BAR_MB). That is sized for 12 GB+ cards: on 8 GB, 3072 + 3072 kept for render targets + 768 of
#   buffer copies is past the textures' line (about 6.5 GB). Here it is 1536 instead (-WindowsBarMB N to change).
#   PCs with 24 GB or more keep it off, as in play-nvidia.ps1.
# - Everything else (texture budget, room kept for render targets) already scales with the budget Windows reports.
# Extra settings can still be passed: -More @{VICTORY_CARD_BUFFERS_MB='512'}
param(
    [Parameter(Mandatory)][string]$Exe,
    [ValidateSet('1080', '720', '540', '480')][string]$Resolution,
    [switch]$Quality,
    [switch]$NoSmoothMotion,
    [ValidateSet('diag', 'release')][string]$Build = 'diag',
    [hashtable]$More = @{},
    [uint32]$WindowsBarMB
)
$fit8gb = @{
    VICTORY_CARD_BUFFERS_MB = '768'
}
foreach ($k in $More.Keys) { $fit8gb[$k] = $More[$k] }
$splat = @{ Exe = $Exe; More = $fit8gb; Build = $Build; Quality = $Quality; NoSmoothMotion = $NoSmoothMotion }
if ($Resolution) { $splat.Resolution = $Resolution }
if ($PSBoundParameters.ContainsKey('WindowsBarMB')) {
    $splat.WindowsBarMB = $WindowsBarMB
} else {
    $ram = try { (Get-CimInstance Win32_ComputerSystem -ErrorAction Stop).TotalPhysicalMemory } catch { 0 }
    if ($ram -gt 0 -and $ram -lt 24GB) { $splat.WindowsBarMB = [uint32]1536 }
}
& (Join-Path $PSScriptRoot 'play-nvidia.ps1') @splat
