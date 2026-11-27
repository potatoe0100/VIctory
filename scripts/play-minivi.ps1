# Portable MiniVI launcher for 8 GB VRAM / 16-32 GB RAM systems (Series S profile).
# Supply -Exe or set $env:VICTORY_GAME_DIR before launching.
param(
    [string]$Exe,
    [ValidateSet('perf-540p', 'balanced-720p', 'quality-720p', 'native-1080p')][string]$Preset = 'balanced-720p',
    [ValidateSet('diag', 'release')][string]$Build = 'diag',
    [switch]$NoSmoothMotion,
    [switch]$RayTracing,
    [string]$Adapter,
    [hashtable]$More = @{}
)

if (-not $Exe -and $env:VICTORY_GAME_DIR) {
    $candidate = Join-Path $env:VICTORY_GAME_DIR 'game_scarlett_master.exe'
    if (Test-Path $candidate) { $Exe = $candidate }
}
if (-not $Exe -or -not (Test-Path $Exe)) {
    Write-Host "Pass -Exe <path\to\game_scarlett_master.exe> or set `$env:VICTORY_GAME_DIR."
    exit 1
}

$scale = switch ($Preset) {
    'perf-540p'     { '0.5' }
    'balanced-720p' { '0.6666667' }
    'quality-720p'  { '0.6666667' }
    'native-1080p'  { '1.0' }
}
$maxDim = switch ($Preset) {
    'perf-540p'     { '1024' }
    'balanced-720p' { '1024' }
    'quality-720p'  { '2048' }
    'native-1080p'  { '2048' }
}
$bakeMb = if ($Preset -eq 'perf-540p') { '128' } else { '256' }

$settings = @{
    VICTORY_TEXTURE_NO_WAIT   = '1'
    VICTORY_RELAY             = '1'
    VICTORY_RELAY_FLUSH_TABLES = '1'
    VICTORY_SKIP_WC_BACK      = '1'
    VICTORY_CARD_STEPS        = '2'
    VICTORY_WIDE_BACK         = '1'
    VICTORY_CARD_REHASH_RUNS  = '1'
    VICTORY_JOIN_COPIES       = '1'
    VICTORY_INDIRECT_WINDOW   = '1'
    VICTORY_RELAY_COPY        = '1'
    VICTORY_INDIRECT_BATCH    = '2'
    VICTORY_MAP_THREADS       = '1'
    VICTORY_TITLE_PRIORITIES  = '0'
    VICTORY_CARD_AWAKE        = '1'
    VICTORY_SHOW_AFTER        = '1'
    VICTORY_THIN              = '1'
    VICTORY_JOIN_PARTS        = '8'
    VICTORY_UPLOAD_AHEAD      = '1'
    VICTORY_AMBIENT           = '1.4'
    VICTORY_AMBIENT_AIR       = '1.5'
    VICTORY_AMBIENT_MOST      = '2'
    VICTORY_NAN_SCRUB         = '1'
    VICTORY_NO_WATCHDOG       = '1'
    VICTORY_COMPOSE           = '1'
    VICTORY_RENDER_SCALE      = $scale
    VICTORY_OWN_UPSCALER      = '1'
    VICTORY_BVH_MISS          = '1'
    VICTORY_DYNAMIC_SLOTS     = '524288'
    VICTORY_CARD_BUFFERS_MIN_MB = '1'
    VICTORY_CARD_BUFFERS_MB   = '1024'
    VICTORY_CARD_WINDOWS_MB   = '256'
    VICTORY_CARD_ROOM_FOR_TARGETS_MB = '1024'
    VICTORY_TEXTURE_ROOM_PERCENT = '92'
    VICTORY_DSTORAGE_STAGING_MB = '64'
    VICTORY_EARLY_TEXTURES_MS = '35'
    VICTORY_LEVELS_ON_DEMAND  = '1'
    VICTORY_LEVELS_ALL        = '1'
    VICTORY_LEVELS_MAX_DIM    = $maxDim
    VICTORY_LEVELS_RECLAIM    = '1'
    VICTORY_RESIDENCY         = '1'
    VICTORY_BAKE_SOURCES_MB   = $bakeMb
    VICTORY_PIPELINE_WAIT_MS  = '150'
    VICTORY_PIPELINE_WORKERS  = '6'
    VICTORY_PIPELINE_LOWEST   = '1'
    VICTORY_D3D12_DRED        = 'light'
    VICTORY_SKIP_PIPELINES    = 'none'
}

if ($Preset -eq 'quality-720p' -or $Preset -eq 'native-1080p') {
    $settings.VICTORY_AMBIENT_SHADE   = '1.2,0.85'
    $settings.VICTORY_AMBIENT_REFLECT = '0.65'
}

$rtList = Join-Path $PSScriptRoot 'rt_off_hashes.txt'
if ($RayTracing) {
    $settings.VICTORY_BVH_BUILDER = '1'
    $settings.VICTORY_FIRSTLANE_OWN = '1'
    $settings.VICTORY_RT_MEMORY_MB = '1024'
    $settings.VICTORY_CARD_BUFFERS_MB = '768'
    $settings.VICTORY_SKIP_PIPELINES = 'none'
    foreach ($off in 'VICTORY_BVH_MISS', 'VICTORY_AMBIENT', 'VICTORY_AMBIENT_AIR', 'VICTORY_AMBIENT_MOST', 'VICTORY_AMBIENT_SHADE', 'VICTORY_AMBIENT_REFLECT', 'VICTORY_TEXTURE_MIP_DROP') {
        $settings.Remove($off)
    }
} elseif (Test-Path $rtList) {
    $settings.VICTORY_SKIP_PIPELINES = (Get-Content $rtList -Raw).Trim()
}
if ($Adapter) { $settings.VICTORY_ADAPTER = $Adapter }
foreach ($k in $More.Keys) { $settings[$k] = $More[$k] }

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$targetRoot = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $repoRoot 'target' }
$victory = Join-Path $targetRoot "$Build\victory.exe"
if (-not (Test-Path $victory)) {
    Write-Host "Missing $victory. Build first with: cargo build --locked --profile $Build -p victory"
    exit 1
}

if (-not $NoSmoothMotion) {
    $sm = Join-Path (Split-Path $victory) 'victory_sm.exe'
    if (-not (Test-Path $sm) -or (Get-Item $sm).LastWriteTime -lt (Get-Item $victory).LastWriteTime) {
        Copy-Item $victory $sm -Force
    }
    $victory = $sm
}

$keep = 'VICTORY_CONFIG', 'VICTORY_GAME_DIR', 'VICTORY_CACHE_DIR', 'VICTORY_DATA_DIR'
Get-ChildItem Env:VICTORY_* | Where-Object { $_.Name -notin $keep } | ForEach-Object { Remove-Item "Env:$($_.Name)" }
foreach ($k in $settings.Keys) { Set-Item "Env:$k" $settings[$k] }

$Exe = (Resolve-Path $Exe).Path
Write-Host "MiniVI ($Preset, max mip ${maxDim}px, render scale $scale)"
Push-Location $repoRoot
try { & $victory run $Exe --execute --console series-s } finally { Pop-Location }
