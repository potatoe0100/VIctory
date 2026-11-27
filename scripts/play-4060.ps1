# EXPERIMENTAL, AI-assisted, untested against the game. Starts the title on an 8 GB NVIDIA card (RTX 4060 class).
#   scripts\play-4060.ps1 -Exe <path of your own copy's executable> [-Resolution 480|540|720] [-Quality] [-NoSmoothMotion]
#
# This is scripts\play-nvidia.ps1 (Series S, 540p, ray tracing mostly off) with settings that fit 8 GB:
# - Card buffer copies (VICTORY_CARD_BUFFERS_MB) are set by the preset below, not the 1536 MB play-nvidia.ps1 uses
#   everywhere: on an 8 GB card the budget is about 7.2 GB, and past it Windows moves the card's memory into the PC's
#   (much slower, and what ended the fork's runs 20 and 24).
# - On PCs with under 24 GB of RAM, play-nvidia.ps1 puts up to 3072 MB of title memory windows in the card's memory
#   (VICTORY_WINDOWS_BAR_MB). That is sized for 12 GB+ cards: on 8 GB, 3072 + 3072 kept for render targets + 768 of
#   buffer copies is past the textures' line (about 6.5 GB). Here it is 1536 instead (-WindowsBarMB N to change).
#   PCs with 24 GB or more keep it off, as in play-nvidia.ps1.
# - Everything else (texture budget, room kept for render targets) already scales with the budget Windows reports.
# Presets (-Preset; the numbers are also in shadow.rs `PRESETS_4060`, whose test checks they fit the card's budget,
# and with 1 GB less free, e.g. a browser open):
#   safe      480p, 512 MB of card buffer copies, levels on demand (least video memory)
#   balanced  540p, 1024 MB (the default)
#   fast      540p, 1536 MB (the fork's own figure: more memory compares on the card, more frames a second)
#   max       540p, 2048 MB (A/B: an RTX 3070 Ti 8 GB went from 3.1 to 16.3 fps going 512 -> 2048; tightest fit)
# -Resolution overrides the preset's resolution.
# -Tweaks: two settings to compare against a run without them (A/B; not known to help yet):
#   VICTORY_BAKE_TEXTURES_MS=0 (no wait for textures before the title's decoration bake draws) and
#   VICTORY_SHADER_LOOP_LIMIT=256 (translated shader loops stop sooner; diagnostics build only).
# Extra settings can still be passed: -More @{VICTORY_CARD_BUFFERS_MB='512'}
param(
    [Parameter(Mandatory)][string]$Exe,
    [ValidateSet('safe', 'balanced', 'fast', 'max')][string]$Preset = 'balanced',
    [ValidateSet('1080', '720', '540', '480')][string]$Resolution,
    [switch]$Quality,
    [switch]$NoSmoothMotion,
    [ValidateSet('diag', 'release')][string]$Build = 'diag',
    [hashtable]$More = @{},
    [uint32]$WindowsBarMB,
    [switch]$Tweaks
)
$presets = @{
    safe     = @{ Resolution = '480'; Settings = @{ VICTORY_CARD_BUFFERS_MB = '512'; VICTORY_LEVELS_ON_DEMAND = '1' } }
    balanced = @{ Resolution = '540'; Settings = @{ VICTORY_CARD_BUFFERS_MB = '1024' } }
    fast     = @{ Resolution = '540'; Settings = @{ VICTORY_CARD_BUFFERS_MB = '1536' } }
    max      = @{ Resolution = '540'; Settings = @{ VICTORY_CARD_BUFFERS_MB = '2048' } }
}
$fit8gb = @{}
foreach ($k in $presets[$Preset].Settings.Keys) { $fit8gb[$k] = $presets[$Preset].Settings[$k] }
if ($Tweaks) { $fit8gb.VICTORY_BAKE_TEXTURES_MS = '0'; $fit8gb.VICTORY_SHADER_LOOP_LIMIT = '256'; Write-Host 'Tweaks: on (A/B)' }
foreach ($k in $More.Keys) { $fit8gb[$k] = $More[$k] }
if (-not $Resolution) { $Resolution = $presets[$Preset].Resolution }
Write-Host "Preset: $Preset"
$splat = @{ Exe = $Exe; More = $fit8gb; Build = $Build; Quality = $Quality; NoSmoothMotion = $NoSmoothMotion; Resolution = $Resolution }
if ($PSBoundParameters.ContainsKey('WindowsBarMB')) {
    $splat.WindowsBarMB = $WindowsBarMB
} else {
    $ram = try { (Get-CimInstance Win32_ComputerSystem -ErrorAction Stop).TotalPhysicalMemory } catch { 0 }
    if ($ram -gt 0 -and $ram -lt 24GB) { $splat.WindowsBarMB = [uint32]1536 }
}
& (Join-Path $PSScriptRoot 'play-nvidia.ps1') @splat
