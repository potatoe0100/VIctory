# EXPERIMENTAL, AI-assisted. VIctory-NVIDIA's play-foxhound.ps1 with settings for an 8 GB card (RTX 4060 class) on a
# 6-core-or-more Intel PC: the Series S profile, the title's own upscaling, and switches 8 GB users measured as faster.
#
#   .\scripts\play-4060.ps1 -Exe <path of your own copy's executable>
#   .\scripts\play-4060.ps1 -Exe ... -Preset safe                     512 MB of buffer copies on the card (least video memory)
#   .\scripts\play-4060.ps1 -Exe ... -More @{VICTORY_CARD_AWAKE='1'}   add or override switches
#
# What each switch does is in a comment where the code reads it. The F1 menu's saved choices
# (victory-settings.txt) go under these settings and -More.
param(
    [Parameter(Mandatory)][string]$Exe,
    [ValidateSet('diag', 'release')][string]$Build = 'release',
    # Buffer copies in the card's memory: safe 512 MB, balanced 1024, fast 1536, max 2048 (may spill on 8 GB).
    # Kept in step with shadow.rs PRESETS_4060, whose test checks they fit the card's budget.
    [ValidateSet('safe', 'balanced', 'fast', 'max')][string]$Preset = 'balanced',
    [switch]$Preload,
    [hashtable]$More = @{}
)
$cards = @{ safe = '512'; balanced = '1024'; fast = '1536'; max = '2048' }
$settings = @{
    VICTORY_CARD_BUFFERS_MB = $cards[$Preset]
    # Measured by 8 GB users of VIctory-NVIDIA: the constants heap in place (about +8 %), 768 MB of title windows
    # on the card (about +15 %).
    VICTORY_PORT_CONSTANTS = '1'
    VICTORY_CARD_WINDOWS_MB = '768'
    # Helped on a 6-core Intel PC (VIctory-NVIDIA #526): four list threads. (The card is not kept awake: since
    # VIctory-NVIDIA #927 play-foxhound.ps1 leaves VICTORY_CARD_AWAKE unset.)
    VICTORY_THIN_THREADS = '4'
}
foreach ($k in $More.Keys) { $settings[$k] = $More[$k] }
$foxhound = @{ Exe = $Exe; Build = $Build; SeriesS = $true; Dlss = 'off'; More = $settings }
if ($Preload) { $foxhound.Preload = $true }
& (Join-Path $PSScriptRoot 'play-foxhound.ps1') @foxhound
