# victory-update.ps1: keeps this PC on the newest tested build of potatoe0100/VIctory (branch nvidia-4060).
# EXPERIMENTAL, AI-assisted (Claude). Lives OUTSIDE the repo, in C:\codeprojects\victory-tools. Run by the scheduled task
# "VIctory update" (see EXTERNAL-SETUP.md section 9), or by hand:
#   powershell -NoProfile -ExecutionPolicy Bypass -File C:\codeprojects\victory-tools\victory-update.ps1 [-CheckOnly]
#
# What it does, in order:
#  1. Stops if the game, a build or a test run is already going, or another update is running.
#  2. git fetch (code only; it never touches game files).
#  3. Picks the newest tag named tested-* that is part of origin/nvidia-4060 and whose message does not say
#     "not the default". Those are the builds that passed the full test suite on the reference RTX 4060.
#  4. If that is newer than this PC's build: moves to it (or rebases the local branch "local", if this PC has its own
#     commits), builds, and runs the test suite on this PC's card. A failed build or tests: back to the previous build.
#  5. If the build moved or the setup doc changed, and Claude Code is installed, asks Claude (headless, limited to this
#     folder) whether this PC's launcher wrappers need changing, applies small changes, and writes a short summary.
#  6. Writes the outcome to "VIctory update.txt" on the Desktop and to update.log here.
param(
    [string]$Repo = 'C:\codeprojects\victory',
    [string]$Branch = 'nvidia-4060',
    [string]$DocBranch = 'external-setup',
    [string]$LocalBranch = 'local',
    [string]$Adapter = '',          # e.g. 4070: the card the tests run on (see my-setup.md)
    [switch]$SkipTests,
    [switch]$NoClaude,
    [switch]$CheckOnly
)
$ErrorActionPreference = 'Stop'
$Tools = $PSScriptRoot
$logDir = Join-Path $Tools 'logs'
New-Item -ItemType Directory -Force -Path $logDir | Out-Null
$logFile = Join-Path $Tools 'update.log'
$stateFile = Join-Path $Tools 'state.json'
$lockFile = Join-Path $Tools 'update.lock'
$statusFile = Join-Path ([Environment]::GetFolderPath('Desktop')) 'VIctory update.txt'
$settingsFile = Join-Path $Tools 'settings.json'   # optional: {"Adapter":"4070","SkipTests":false,"NoClaude":false}

function Log([string]$m) {
    $line = '{0:yyyy-MM-dd HH:mm:ss} {1}' -f (Get-Date), $m
    Add-Content -Path $logFile -Value $line
    Write-Host $line
}
function Status([string]$m) {
    $text = "VIctory update, {0:ddd d MMM yyyy h:mm tt}`r`n`r`n{1}`r`n`r`n(Experimental, AI-assisted build. Log: {2})" -f (Get-Date), $m, $logFile
    Set-Content -Path $statusFile -Value $text
    Log "STATUS: $m"
}
# Native commands write progress to stderr; in Windows PowerShell 5.1 that must not count as an error.
function Invoke-Git {
    $old = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
    $out = & git.exe -C $Repo @args 2>&1 | ForEach-Object { "$_" }
    $code = $LASTEXITCODE; $ErrorActionPreference = $old
    if ($code -ne 0) { throw "git $($args -join ' ') failed ($code): $($out -join ' | ')" }
    return $out
}
function TryGit {
    $old = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
    $out = & git.exe -C $Repo @args 2>&1 | ForEach-Object { "$_" }
    $ok = ($LASTEXITCODE -eq 0); $ErrorActionPreference = $old
    return @{ Ok = $ok; Out = $out }
}
# Runs cargo in the repo, stdout to $file and stderr to $file.err. Returns the exit code.
function Invoke-Cargo([string]$argline, [string]$file) {
    $p = Start-Process -FilePath 'cargo' -ArgumentList $argline -WorkingDirectory $Repo -NoNewWindow -Wait -PassThru `
        -RedirectStandardOutput $file -RedirectStandardError "$file.err"
    return $p.ExitCode
}
function Invoke-Build([string]$label) {
    $f = Join-Path $logDir "build-$label.log"
    Log "Building ($label), log $f(.err)"
    $code = Invoke-Cargo 'build --locked --profile diag -p victory --features diagnostics' $f
    return ($code -eq 0)
}
# The suite as the reference PC runs it. Returns @{ Ok; Summary }.
function Invoke-Tests([string]$label) {
    $f = Join-Path $logDir "test-$label.log"
    $env:VICTORY_DYNAMIC_SLOTS = '524288'
    if ($Adapter) { $env:VICTORY_ADAPTER = $Adapter }
    Log "Testing ($label), log $f"
    $null = Invoke-Cargo 'test --workspace --locked --no-fail-fast -- --test-threads=1 --skip heap_direct_texture_reads_mark_their_heap_index' $f
    $lines = @(Get-Content $f) + @(Get-Content "$f.err" -ErrorAction SilentlyContinue)
    $passed = 0; $failed = 0; $ignored = 0
    foreach ($l in $lines) {
        if ($l -match 'test result: \w+\. (\d+) passed; (\d+) failed; (\d+) ignored') {
            $passed += [int]$Matches[1]; $failed += [int]$Matches[2]; $ignored += [int]$Matches[3]
        }
    }
    $names = @($lines | Where-Object { $_ -match '^test (\S+) \.\.\. FAILED' } | ForEach-Object { ($_ -split ' ')[1] } | Select-Object -Unique)
    $compileError = ($lines | Where-Object { $_ -match '^error(\[E\d+\])?:' }).Count -gt 0
    if ($passed -eq 0 -or $compileError) { return @{ Ok = $false; Summary = "tests did not run (see $f)" } }
    $summary = "$passed passed, $failed failed, $ignored ignored"
    if ($names.Count -eq 0 -and $failed -eq 0) { return @{ Ok = $true; Summary = $summary } }
    if ($names.Count -gt 3) { return @{ Ok = $false; Summary = "$summary. Failed: $($names -join ', ')" } }
    # A few failures: run each once more on its own (one base-fork test is known to be flaky).
    $still = @()
    foreach ($n in $names) {
        $rf = Join-Path $logDir "retest-$label-$($n -replace '[^\w]', '_').log"
        $code = Invoke-Cargo "test --workspace --locked -- --exact $n --test-threads=1" $rf
        if ($code -ne 0) { $still += $n }
    }
    if ($still.Count -eq 0) { return @{ Ok = $true; Summary = "$summary; passed when re-run alone: $($names -join ', ')" } }
    return @{ Ok = $false; Summary = "$summary. Still failing alone: $($still -join ', ')" }
}

# --- settings and guards -------------------------------------------------------------------------------------------
if (Test-Path $settingsFile) {
    $cfg = Get-Content $settingsFile -Raw | ConvertFrom-Json
    if (-not $PSBoundParameters.ContainsKey('Adapter') -and $cfg.Adapter) { $Adapter = [string]$cfg.Adapter }
    if (-not $PSBoundParameters.ContainsKey('SkipTests') -and $cfg.SkipTests) { $SkipTests = $true }
    if (-not $PSBoundParameters.ContainsKey('NoClaude') -and $cfg.NoClaude) { $NoClaude = $true }
}
$busy = Get-Process -Name 'victory', 'victory_sm', 'cargo', 'rustc' -ErrorAction SilentlyContinue
if ($busy) { Log "Skipped: $((($busy | Select-Object -ExpandProperty Name -Unique) -join ', ')) running."; return }
if (Test-Path $lockFile) {
    if ((Get-Item $lockFile).LastWriteTime -gt (Get-Date).AddHours(-6)) { Log 'Skipped: another update is running.'; return }
    Remove-Item $lockFile -Force
}
if (-not (Test-Path (Join-Path $Repo '.git'))) { Status "No repository at $Repo. Ask Claude Code to redo the setup (EXTERNAL-SETUP.md)."; return }
Set-Content $lockFile $PID

try {
    # --- what this PC has, and what is out there -----------------------------------------------------------------
    $state = $null
    if (Test-Path $stateFile) { $state = Get-Content $stateFile -Raw | ConvertFrom-Json }
    $f = TryGit fetch --prune --tags --force origin
    if (-not $f.Ok) { Log "Skipped: could not reach GitHub ($($f.Out -join ' '))."; return }
    $tags = Invoke-Git for-each-ref "--merged=origin/$Branch" --sort=-creatordate '--format=%(refname:short)|%(contents:subject)' 'refs/tags/tested-*'
    $pick = $tags | Where-Object { $_ -and $_ -notmatch 'not the default' } | Select-Object -First 1
    if (-not $pick) { Status "No tested build found on $Branch. Nothing changed."; return }
    $newTag, $newNote = $pick -split '\|', 2
    $docHead = (TryGit rev-parse "origin/$DocBranch").Out | Select-Object -First 1

    $curTag = if ($state -and $state.tag) { [string]$state.tag } else { $null }
    if (-not $curTag) {
        $d = TryGit describe --tags --exact-match HEAD
        if ($d.Ok) { $curTag = [string]($d.Out | Select-Object -First 1) }
    }
    $docChanged = ($state -and $state.doc -and $docHead -and $state.doc -ne $docHead)
    Log "This PC: $curTag. Newest tested on ${Branch}: $newTag. Setup doc changed: $docChanged."

    if ($CheckOnly) {
        if ($curTag -eq $newTag) { Write-Host 'Up to date.' } else { Write-Host "Would move $curTag -> $newTag ($newNote)" }
        return
    }

    $moved = $false
    $summary = ''
    if ($curTag -ne $newTag) {
        # Only tracked files count: victory.toml and target\ are ignored by git.
        $dirty = Invoke-Git status --porcelain --untracked-files=no
        if ($dirty) {
            Status "A newer build ($newTag) is out, but the repo has uncommitted edits, so it was left alone. Ask Claude Code to look (EXTERNAL-SETUP.md section 9).`r`n$($dirty -join "`r`n")"
            return
        }
        $prevHead = (Invoke-Git rev-parse HEAD | Select-Object -First 1)
        $hasLocal = (TryGit rev-parse --verify --quiet "refs/heads/$LocalBranch").Ok
        if ($hasLocal -and -not $curTag) {
            Status "A newer build ($newTag) is out, but this PC has its own branch ($LocalBranch) and no record of which build it sits on, so nothing changed. Ask Claude Code to rebase it by hand (EXTERNAL-SETUP.md section 9)."
            return
        }
        if ($hasLocal) {
            Invoke-Git checkout -q $LocalBranch | Out-Null
            $base = $curTag
            $r = TryGit rebase --onto $newTag $base $LocalBranch
            if (-not $r.Ok) {
                TryGit rebase --abort | Out-Null
                Invoke-Git reset -q --hard $prevHead | Out-Null
                Status "A newer build ($newTag) is out, but this PC's own changes (branch $LocalBranch) clash with it, so nothing changed. Ask Claude Code to merge them by hand (EXTERNAL-SETUP.md section 9)."
                return
            }
        } else {
            Invoke-Git checkout -q --detach $newTag | Out-Null
        }
        $ok = Invoke-Build $newTag
        $testNote = 'tests skipped'
        if ($ok -and -not $SkipTests) {
            $t = Invoke-Tests $newTag
            $ok = $t.Ok; $testNote = "tests: $($t.Summary)"
        }
        if (-not $ok) {
            Log "Rolling back to $prevHead"
            if ($hasLocal) { Invoke-Git checkout -q $LocalBranch | Out-Null; Invoke-Git reset -q --hard $prevHead | Out-Null } else { Invoke-Git checkout -q --detach $prevHead | Out-Null }
            $rebuilt = Invoke-Build 'rollback'
            $back = if ($rebuilt) { "Back on $curTag and rebuilt; the shortcut still works." } else { "Rolling back to $curTag did not build either: ask Claude Code to look." }
            Status "Tried $newTag but it failed on this PC ($testNote; logs in $logDir). $back"
            return
        }
        $moved = $true
        $summary = "Updated $curTag -> $newTag ($testNote).`r`nWhat's new: $newNote`r`nThe first launch after an update can sit on a black screen for a while as shaders are rebuilt."
        @{ tag = $newTag; head = (Invoke-Git rev-parse HEAD | Select-Object -First 1); doc = $docHead; when = (Get-Date).ToString('s') } |
            ConvertTo-Json | Set-Content $stateFile
    } elseif (-not $state -or -not $state.doc) {
        @{ tag = $curTag; head = (Invoke-Git rev-parse HEAD | Select-Object -First 1); doc = $docHead; when = (Get-Date).ToString('s') } |
            ConvertTo-Json | Set-Content $stateFile
    }

    # --- Claude review of what changed --------------------------------------------------------------------------
    if (($moved -or $docChanged) -and -not $NoClaude -and (Get-Command claude -ErrorAction SilentlyContinue)) {
        $ctx = Join-Path $Tools 'update-context.md'
        $from = if ($curTag) { $curTag } else { 'HEAD' }
        $parts = @("# Update context ($(Get-Date -Format s))", "From $from to $newTag. Tag note: $newNote", '')
        if ($moved) {
            $parts += '## Commits'; $parts += (Invoke-Git log --oneline "$from..$newTag"); $parts += ''
            $parts += '## Changes to docs and launch scripts'
            $parts += (Invoke-Git diff "$from..$newTag" '--' README-4060.md CHANGES-4060.md scripts victory.example.toml rust-toolchain.toml)
        }
        if ($docChanged) {
            $parts += ''; $parts += '## EXTERNAL-SETUP.md changes'
            $parts += (Invoke-Git diff "$($state.doc)..$docHead" '--' EXTERNAL-SETUP.md victory-update.ps1)
        }
        Set-Content -Path $ctx -Value ($parts -join "`r`n")
        # One line, no double quotes: it goes through claude's command-line shim unchanged.
        $prompt = 'You are maintaining an experimental VIctory setup on this PC. Read update-context.md (what changed upstream), my-setup.md (this PC''s hardware and chosen settings) and the launcher wrappers (*.cmd) in this folder. Decide whether the wrappers or settings need changing because of the update, for example a renamed or new launcher option, a changed preset, or a new recommended switch for this PC''s card. If a change is small and clearly needed, make it in the files in this folder only, and note it at the end of my-setup.md with today''s date. Never edit the repository, never touch game files, never post anything online. If EXTERNAL-SETUP.md or victory-update.ps1 changed, do not apply them; say in one line what the user should ask Claude Code to redo. Then reply with at most 6 short plain-English lines for a non-developer: what changed that matters to them, what you changed, and anything they need to do.'
        Log 'Asking Claude to review the update'
        $old = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
        Push-Location $Tools
        try {
            $review = & claude -p $prompt --permission-mode acceptEdits --allowedTools Read Glob Grep Edit Write 2>&1 | ForEach-Object { "$_" }
        } finally { Pop-Location; $ErrorActionPreference = $old }
        if ($LASTEXITCODE -eq 0 -and $review) { $summary = ($summary + "`r`n`r`nClaude's notes:`r`n" + ($review -join "`r`n")).Trim() }
        else { $summary = ($summary + "`r`n`r`n(Claude's review did not run: $($review -join ' '))").Trim() }
        if ($docChanged -and -not $moved) {
            @{ tag = $curTag; head = (Invoke-Git rev-parse HEAD | Select-Object -First 1); doc = $docHead; when = (Get-Date).ToString('s') } |
                ConvertTo-Json | Set-Content $stateFile
        }
    } elseif ($docChanged -and -not $moved) {
        $summary = 'The setup instructions (EXTERNAL-SETUP.md) changed. Ask Claude Code: "re-read EXTERNAL-SETUP.md and apply what changed".'
        @{ tag = $curTag; head = (Invoke-Git rev-parse HEAD | Select-Object -First 1); doc = $docHead; when = (Get-Date).ToString('s') } |
            ConvertTo-Json | Set-Content $stateFile
    }
    if ($summary) { Status $summary } else { Log "Up to date on $curTag." }
} catch {
    Status "The update check hit an error: $($_.Exception.Message)`r`nIf the shortcut no longer starts the game, ask Claude Code to check C:\codeprojects\victory against update.log."
} finally {
    Remove-Item $lockFile -Force -ErrorAction SilentlyContinue
}
