$ErrorActionPreference = "Stop"

$workspace = (Get-Location).Path
$replacement = Join-Path $workspace ".catdesk\t0048-final-proven\debug\catdesk.exe"
if (-not (Test-Path -LiteralPath $replacement -PathType Leaf)) {
    throw "Verified T-0048 final CatDesk binary is missing."
}

# Restore normal authenticated per-user Codex context for the replacement.
$env:CATDESK_CODEX_HOME = Join-Path $HOME ".codex"
if (-not (Test-Path -LiteralPath $env:CATDESK_CODEX_HOME -PathType Container)) {
    throw "Authenticated per-user Codex home is missing."
}

# Resolve the native OpenAI Codex executable locally without printing its path.
if ([string]::IsNullOrWhiteSpace($env:CATDESK_CODEX_CLI_EXECUTABLE) -or -not (Test-Path -LiteralPath $env:CATDESK_CODEX_CLI_EXECUTABLE -PathType Leaf)) {
    $npmRoot = npm root -g
    if ([string]::IsNullOrWhiteSpace($npmRoot)) { throw "Unable to resolve global npm root." }
    $codexExe = Get-ChildItem -Path $npmRoot -Recurse -Filter codex.exe -File -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -match '@openai' } |
        Select-Object -First 1 -ExpandProperty FullName
    if (-not $codexExe) { throw "Unable to locate the native OpenAI Codex executable." }
    $env:CATDESK_CODEX_CLI_EXECUTABLE = $codexExe
}

# Verify that exact Codex executable sees the existing ChatGPT login.
$priorCodexHome = $env:CODEX_HOME
try {
    $env:CODEX_HOME = $env:CATDESK_CODEX_HOME
    & $env:CATDESK_CODEX_CLI_EXECUTABLE login status | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "Codex authentication preflight failed." }
} finally {
    if ($null -eq $priorCodexHome) { Remove-Item Env:CODEX_HOME -ErrorAction SilentlyContinue }
    else { $env:CODEX_HOME = $priorCodexHome }
}

$listeners = @(Get-NetTCPConnection -State Listen -LocalPort 3200 -ErrorAction SilentlyContinue |
    Where-Object { $_.LocalAddress -in @("127.0.0.1", "::1") })
if ($listeners.Count -ne 1) {
    throw "Expected exactly one loopback CatDesk listener on port 3200; found $($listeners.Count)."
}

$oldPid = [int]$listeners[0].OwningProcess
$old = Get-Process -Id $oldPid -ErrorAction Stop
if ($old.ProcessName -ne "catdesk") {
    throw "Port 3200 is not owned by CatDesk. Refusing bootstrap replacement."
}

# This is the last external bootstrap: terminate only the exact current listener owner.
Stop-Process -Id $oldPid -Force -ErrorAction Stop

$deadline = (Get-Date).AddSeconds(20)
do {
    Start-Sleep -Milliseconds 100
    $remaining = @(Get-NetTCPConnection -State Listen -LocalPort 3200 -ErrorAction SilentlyContinue |
        Where-Object { $_.LocalAddress -in @("127.0.0.1", "::1") })
} while ($remaining.Count -gt 0 -and (Get-Date) -lt $deadline)

if ($remaining.Count -gt 0) {
    throw "Port 3200 did not release after the old CatDesk process stopped."
}

$new = Start-Process -FilePath $replacement -ArgumentList "--catdesk-daemon" -WorkingDirectory $workspace -WindowStyle Hidden -PassThru

$deadline = (Get-Date).AddSeconds(30)
do {
    Start-Sleep -Milliseconds 250
    $ready = @(Get-NetTCPConnection -State Listen -LocalPort 3200 -ErrorAction SilentlyContinue |
        Where-Object { $_.LocalAddress -in @("127.0.0.1", "::1") -and $_.OwningProcess -eq $new.Id })
} while ($ready.Count -eq 0 -and -not $new.HasExited -and (Get-Date) -lt $deadline)

if ($new.HasExited) { throw "Verified replacement CatDesk exited before MCP became ready." }
if ($ready.Count -eq 0) { throw "Verified replacement CatDesk did not bind port 3200 in time." }

Write-Host "T-0048 final proven daemon loaded. Native listener inheritance fix active."
Write-Host "External Secure MCP tunnel was not restarted."
