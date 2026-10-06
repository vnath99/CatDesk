[CmdletBinding()]
param(
    [int]$OldPid,
    [string]$ExpectedOldProcessStartedAtUtc = '',
    [string]$ExpectedOldProcessPath = '',
    [string]$ExpectedOldProcessSha256 = '',
    [string]$BuildPath = (Join-Path $PSScriptRoot "..\target\release\catdesk.exe"),
    [string]$Workspace = (Join-Path $PSScriptRoot ".."),
    [int]$McpPort = 3200,
    [int]$StartupDelaySeconds = 3,
    [int]$ExitTimeoutSeconds = 15,
    [int]$ReadyTimeoutSeconds = 90,
    [switch]$Execute
)

$ErrorActionPreference = "Stop"

function Write-HandoffState {
    param([hashtable]$State)
    $stateDir = Join-Path $resolvedWorkspace ".catdesk\restart-handoff"
    New-Item -ItemType Directory -Force -Path $stateDir | Out-Null
    $target = Join-Path $stateDir "latest.json"
    $temporary = Join-Path $stateDir ("latest-{0}.tmp" -f [guid]::NewGuid().ToString("N"))
    $State | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $temporary -Encoding UTF8
    Move-Item -LiteralPath $temporary -Destination $target -Force
}

function Test-ExpectedOldProcessIdentity {
    param(
        [Parameter(Mandatory = $true)]$Process,
        [string]$ExpectedStartedAtUtc = '',
        [string]$ExpectedPath = '',
        [string]$ExpectedSha256 = ''
    )
    try {
        $null = $Process.Handle
        if ($Process.ProcessName -ne 'catdesk' -or -not $Process.Path) { return $false }
        if ($ExpectedStartedAtUtc) {
            $expectedStarted = [DateTimeOffset]::Parse($ExpectedStartedAtUtc, [Globalization.CultureInfo]::InvariantCulture, [Globalization.DateTimeStyles]::RoundtripKind).ToUniversalTime()
            $actualStarted = ([DateTimeOffset]$Process.StartTime).ToUniversalTime()
            if ($actualStarted.UtcDateTime.Ticks -ne $expectedStarted.UtcDateTime.Ticks) { return $false }
        }
        $actualPath = (Resolve-Path -LiteralPath $Process.Path -ErrorAction Stop).Path
        if ($ExpectedPath) {
            $expectedResolvedPath = (Resolve-Path -LiteralPath $ExpectedPath -ErrorAction Stop).Path
            if (-not $actualPath.Equals($expectedResolvedPath, [StringComparison]::OrdinalIgnoreCase)) { return $false }
        }
        if ($ExpectedSha256) {
            if ($ExpectedSha256 -notmatch '^[A-Fa-f0-9]{64}$') { return $false }
            $actualSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $actualPath -ErrorAction Stop).Hash.ToLowerInvariant()
            if (-not $actualSha256.Equals($ExpectedSha256.ToLowerInvariant(), [StringComparison]::Ordinal)) { return $false }
        }
        return $true
    } catch { return $false }
}

function Resolve-TrustedWindowsPowerShellPath {
    $systemDirectory = [Environment]::SystemDirectory
    if ([string]::IsNullOrWhiteSpace($systemDirectory)) {
        throw "Windows system directory is unavailable; refusing restart handoff."
    }

    $candidate = Join-Path $systemDirectory "WindowsPowerShell\v1.0\powershell.exe"
    $item = Get-Item -LiteralPath $candidate -Force -ErrorAction Stop
    if (-not ($item -is [System.IO.FileInfo])) {
        throw "Trusted Windows PowerShell path is not a file; refusing restart handoff."
    }
    if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw "Trusted Windows PowerShell path is a reparse point; refusing restart handoff."
    }

    $expectedPath = [IO.Path]::GetFullPath($candidate)
    $resolvedPath = [IO.Path]::GetFullPath($item.FullName)
    if (-not $resolvedPath.Equals($expectedPath, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Trusted Windows PowerShell path identity drifted; refusing restart handoff."
    }
    return $resolvedPath
}

function Resolve-TrustedRestartWorkerScriptPath {
    $candidate = Join-Path $PSScriptRoot "restart_catdesk_daemon_worker.ps1"
    $expectedPath = [IO.Path]::GetFullPath($candidate)
    $item = Get-Item -LiteralPath $expectedPath -Force -ErrorAction Stop
    if (-not ($item -is [System.IO.FileInfo])) {
        throw "Restart worker is not a regular file; refusing restart handoff."
    }
    if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw "Restart worker is a reparse point; refusing restart handoff."
    }

    $resolvedPath = [IO.Path]::GetFullPath($item.FullName)
    if (-not $resolvedPath.Equals($expectedPath, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Restart worker path identity drifted; refusing restart handoff."
    }
    return $resolvedPath
}

if ($OldPid -le 0) {
    $listeners = @(Get-NetTCPConnection -State Listen -LocalPort $McpPort -ErrorAction SilentlyContinue |
        Where-Object { $_.LocalAddress -in @("127.0.0.1", "::1") } |
        Select-Object -First 2)
    if ($listeners.Count -ne 1) {
        throw "OldPid was not supplied and exactly one loopback CatDesk MCP listener could not be resolved on the configured port."
    }
    $OldPid = [int]$listeners[0].OwningProcess
}
if ($StartupDelaySeconds -lt 0 -or $StartupDelaySeconds -gt 30) { throw "StartupDelaySeconds must be 0..30." }
if ($ExitTimeoutSeconds -lt 1 -or $ExitTimeoutSeconds -gt 60) { throw "ExitTimeoutSeconds must be 1..60." }
if ($ReadyTimeoutSeconds -lt 5 -or $ReadyTimeoutSeconds -gt 180) { throw "ReadyTimeoutSeconds must be 5..180." }
if ($McpPort -lt 1 -or $McpPort -gt 65535) { throw "McpPort must be 1..65535." }

$resolvedBuild = (Resolve-Path -LiteralPath $BuildPath -ErrorAction Stop).Path
$resolvedWorkspace = (Resolve-Path -LiteralPath $Workspace -ErrorAction Stop).Path
if (-not (Test-Path -LiteralPath $resolvedBuild -PathType Leaf)) { throw "Updated CatDesk executable was not found." }
if (-not (Test-Path -LiteralPath $resolvedWorkspace -PathType Container)) { throw "Workspace does not exist." }
if ((Split-Path -Parent $resolvedBuild) -notlike "$resolvedWorkspace*") { throw "BuildPath must be inside the approved workspace." }

$oldProcess = Get-Process -Id $OldPid -ErrorAction Stop
$null = $oldProcess.Handle
if ($oldProcess.ProcessName -ne "catdesk") { throw "OldPid does not identify a CatDesk process." }
if (-not $oldProcess.Path) { throw "OldPid does not expose a stable CatDesk executable path." }
$oldProcessPath = (Resolve-Path -LiteralPath $oldProcess.Path -ErrorAction Stop).Path
$oldProcessStartedAtUtc = $oldProcess.StartTime.ToUniversalTime().ToString("o")
if ($ExpectedOldProcessStartedAtUtc -or $ExpectedOldProcessPath -or $ExpectedOldProcessSha256) {
    if (-not (Test-ExpectedOldProcessIdentity -Process $oldProcess -ExpectedStartedAtUtc $ExpectedOldProcessStartedAtUtc -ExpectedPath $ExpectedOldProcessPath -ExpectedSha256 $ExpectedOldProcessSha256)) {
        throw "OldPid no longer identifies the exact reviewed CatDesk process instance."
    }
}
if ($env:CATDESK_CODEX_CLI_EXECUTABLE -and -not (Test-Path -LiteralPath $env:CATDESK_CODEX_CLI_EXECUTABLE -PathType Leaf)) {
    throw "CATDESK_CODEX_CLI_EXECUTABLE is set but does not identify an existing operator-local executable."
}

# Do not inspect the command line or process environment: either may contain
# operator-local routing or authentication material. Listener ports and binary
# hash are sufficient non-secret handoff evidence.
$oldPorts = @(Get-NetTCPConnection -State Listen -OwningProcess $OldPid -ErrorAction SilentlyContinue |
    Where-Object { $_.LocalAddress -in @("127.0.0.1", "::1") } |
    Select-Object -ExpandProperty LocalPort -Unique)
$buildHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $resolvedBuild).Hash.ToLowerInvariant()
$oldOwnsConfiguredPort = @(Get-NetTCPConnection -State Listen -LocalPort $McpPort -ErrorAction SilentlyContinue |
    Where-Object { $_.OwningProcess -eq $OldPid -and $_.LocalAddress -in @("127.0.0.1", "::1") }).Count -gt 0
$intent = [ordered]@{
    schemaVersion = 3
    status = "INTENT_RECORDED"
    createdAtUtc = [DateTime]::UtcNow.ToString("o")
    oldPid = $OldPid
    oldListenerPorts = @($oldPorts)
    mcpPort = $McpPort
    oldOwnsConfiguredPort = $oldOwnsConfiguredPort
    workspacePath = $resolvedWorkspace
    buildPath = $resolvedBuild
    buildSha256 = $buildHash
    startupDelaySeconds = $StartupDelaySeconds
    recovery = "If this record remains non-successful, start the verified CatDesk build manually from the approved workspace; do not restart the external Secure MCP tunnel."
}
Write-HandoffState -State $intent

Write-Output "PREFLIGHT_OK old_catdesk_pid=$OldPid"
Write-Output "PREFLIGHT_OK updated_build_sha256=$buildHash"
Write-Output "PREFLIGHT_OK configured_mcp_port=$McpPort"
Write-Output "PREFLIGHT_OK old_process_owns_configured_port=$oldOwnsConfiguredPort"
if ($env:CATDESK_CODEX_CLI_EXECUTABLE) {
    Write-Output "PREFLIGHT_OK CATDESK_CODEX_CLI_EXECUTABLE=present (value redacted)"
} else {
    Write-Output "PREFLIGHT_INFO CATDESK_CODEX_CLI_EXECUTABLE=absent (replacement uses normal current-user Codex discovery)"
}
if ($env:CATDESK_CODEX_HOME) {
    if (-not (Test-Path -LiteralPath $env:CATDESK_CODEX_HOME -PathType Container)) {
        throw "CATDESK_CODEX_HOME does not identify an existing operator-local config directory."
    }
    Write-Output "PREFLIGHT_OK CATDESK_CODEX_HOME=present (path and contents redacted)"
} else {
    Write-Output "PREFLIGHT_INFO CATDESK_CODEX_HOME=absent (child Codex processes use their inherited/default context)"
}

if (-not $Execute) {
    Write-Output "No process was stopped. Re-run with -Execute to launch the detached, PID-scoped handoff."
    exit 0
}

$worker = Resolve-TrustedRestartWorkerScriptPath
$trustedPowerShell = Resolve-TrustedWindowsPowerShellPath
$workerArgs = @(
    "-NoProfile", "-File", $worker,
    "-OldPid", $OldPid,
    "-ExpectedOldProcessStartedAtUtc", $oldProcessStartedAtUtc,
    "-ExpectedOldProcessPath", $oldProcessPath,
    "-BuildPath", $resolvedBuild,
    "-Workspace", $resolvedWorkspace,
    "-McpPort", $McpPort,
    "-StartupDelaySeconds", $StartupDelaySeconds,
    "-ExitTimeoutSeconds", $ExitTimeoutSeconds,
    "-ReadyTimeoutSeconds", $ReadyTimeoutSeconds
)
$detached = Start-Process -FilePath $trustedPowerShell -ArgumentList $workerArgs -WorkingDirectory $resolvedWorkspace -WindowStyle Hidden -PassThru
Write-Output "HANDOFF_DETACHED worker_pid=$($detached.Id)"
