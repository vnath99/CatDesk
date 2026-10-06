[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Workspace,
    [int]$InitialDelaySeconds = 20,
    [int]$MaximumAttempts = 8,
    [int]$InitialBackoffSeconds = 5,
    [int]$MaximumBackoffSeconds = 60,
    [int]$HealthPollSeconds = 60,
    [int]$FailureCooldownSeconds = 300,
    [int]$MaximumMonitorCycles = 0,
    [switch]$LoadOnly
)

$ErrorActionPreference = 'Stop'
$script:SupervisorSeams = @{}

function Invoke-SupervisorSeam {
    param([string]$Name, [object[]]$Arguments, [scriptblock]$Default)
    if ($script:SupervisorSeams.ContainsKey($Name)) { return & $script:SupervisorSeams[$Name] @Arguments }
    return & $Default @Arguments
}

function Get-SupervisorWorkspaceId {
    param([string]$Root)
    $sha = [Security.Cryptography.SHA256]::Create()
    try {
        $digest = (($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($Root)) | ForEach-Object { $_.ToString('x2') }) -join '')
        return $digest.Substring(0, 16)
    } finally {
        $sha.Dispose()
    }
}

function Acquire-SupervisorGuard {
    param([string]$Root)
    Invoke-SupervisorSeam -Name 'AcquireGuard' -Arguments @($Root) -Default {
        param($workspaceRoot)
        $created = $false
        $mutex = [Threading.Mutex]::new($true, "Local\CatDesk.Autostart.$(Get-SupervisorWorkspaceId $workspaceRoot)", [ref]$created)
        [pscustomobject]@{ Acquired = $created; Handle = $mutex }
    }
}

function Release-SupervisorGuard {
    param($Guard)
    if ($null -eq $Guard -or $null -eq $Guard.Handle) { return }
    try { $Guard.Handle.ReleaseMutex() } catch {}
    $Guard.Handle.Dispose()
}

function Resolve-TrustedLifecycleFacadePath {
    param([string]$Root)
    $expected = [System.IO.Path]::GetFullPath((Join-Path $Root 'catdesk.ps1'))
    $item = Get-Item -LiteralPath $expected -Force -ErrorAction Stop
    if ($item -isnot [System.IO.FileInfo]) { throw 'public lifecycle facade is not a regular file' }
    if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'public lifecycle facade cannot be a reparse point' }
    $observed = [System.IO.Path]::GetFullPath($item.FullName)
    if (-not [string]::Equals($observed, $expected, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw 'public lifecycle facade identity mismatch'
    }
    return $observed
}

function Invoke-PublicLifecycle {
    param([string]$Root, [ValidateSet('status', 'recover')][string]$Action)
    Invoke-SupervisorSeam -Name 'Lifecycle' -Arguments @($Root, $Action) -Default {
        param($workspaceRoot, $command)
        $facade = Resolve-TrustedLifecycleFacadePath -Root $workspaceRoot
        $raw = @(& $facade $command -Workspace $workspaceRoot)
        $text = [string]::Join("`n", [string[]]$raw)
        if ($text.Length -gt 4096) { return 'ACTION_REQUIRED' }
        try {
            $parsed = $text | ConvertFrom-Json -ErrorAction Stop
            if ($parsed.state -in @('READY', 'LOCAL_READY_EXTERNAL_RUNTIME_PENDING', 'LOCAL_DAEMON_PENDING', 'CONNECTED_VERIFIED', 'TRANSPORT_VERIFICATION_FAILED', 'ACTION_REQUIRED', 'STATUS_UNAVAILABLE')) {
                return [string]$parsed.state
            }
        } catch {}
        return 'ACTION_REQUIRED'
    }
}

function Wait-SupervisorDelay {
    param([int]$Seconds)
    Invoke-SupervisorSeam -Name 'Sleep' -Arguments @($Seconds) -Default { param($delay) Start-Sleep -Seconds $delay }
}

function Write-SupervisorState {
    param([string]$State)
    # Fixed local state only: it intentionally omits workspace path and raw
    # lifecycle output.
    [pscustomobject]@{ supervisor = 'catdesk'; state = $State } | ConvertTo-Json -Compress
}

function Invoke-RecoveryBurst {
    param([string]$Root, [int]$Attempts, [int]$InitialBackoff, [int]$MaximumBackoff)
    $backoff = $InitialBackoff
    for ($attempt = 1; $attempt -le $Attempts; $attempt++) {
        $recovery = Invoke-PublicLifecycle -Root $Root -Action recover
        if ($recovery -in @('CONNECTED_VERIFIED', 'TRANSPORT_VERIFICATION_FAILED')) { return $recovery }
        if ($attempt -lt $Attempts) {
            Wait-SupervisorDelay -Seconds $backoff
            $backoff = [Math]::Min($MaximumBackoff, $backoff * 2)
        }
    }
    return 'ACTION_REQUIRED'
}

function Invoke-CatDeskAutostartSupervisor {
    param(
        [int]$InitialDelaySeconds = 20,
        [int]$MaximumAttempts = 8,
        [int]$InitialBackoffSeconds = 5,
        [int]$MaximumBackoffSeconds = 60,
        [int]$HealthPollSeconds = 60,
        [int]$FailureCooldownSeconds = 300,
        [int]$MaximumMonitorCycles = 0
    )
    if ($InitialDelaySeconds -lt 0 -or $InitialDelaySeconds -gt 300 -or $MaximumAttempts -lt 1 -or $MaximumAttempts -gt 16 -or $InitialBackoffSeconds -lt 1 -or $MaximumBackoffSeconds -lt $InitialBackoffSeconds -or $MaximumBackoffSeconds -gt 300 -or $HealthPollSeconds -lt 15 -or $HealthPollSeconds -gt 3600 -or $FailureCooldownSeconds -lt 30 -or $FailureCooldownSeconds -gt 900 -or $MaximumMonitorCycles -lt 0 -or $MaximumMonitorCycles -gt 100000) {
        throw 'supervisor retry configuration is invalid'
    }
    $root = (Resolve-Path -LiteralPath $Workspace -ErrorAction Stop).Path
    $guard = Acquire-SupervisorGuard -Root $root
    if (-not $guard.Acquired) { Write-SupervisorState 'SUPERVISOR_ALREADY_RUNNING'; return }
    try {
        if ($InitialDelaySeconds -gt 0) { Wait-SupervisorDelay -Seconds $InitialDelaySeconds }
        $cycles = 0
        while ($MaximumMonitorCycles -eq 0 -or $cycles -lt $MaximumMonitorCycles) {
            $status = Invoke-PublicLifecycle -Root $root -Action status
            if ($status -eq 'READY') {
                Write-SupervisorState 'READY'
                $cycles++
                if ($MaximumMonitorCycles -eq 0 -or $cycles -lt $MaximumMonitorCycles) { Wait-SupervisorDelay -Seconds $HealthPollSeconds }
                continue
            }
            # The public recovery facade owns canonical release validation. This
            # supervisor never compiles, provisions, or assumes ownership of
            # an externally started runtime.
            $recovery = Invoke-RecoveryBurst -Root $root -Attempts $MaximumAttempts -InitialBackoff $InitialBackoffSeconds -MaximumBackoff $MaximumBackoffSeconds
            if ($recovery -eq 'CONNECTED_VERIFIED') {
                Write-SupervisorState 'RECOVERED'
                $cycles++
                if ($MaximumMonitorCycles -eq 0 -or $cycles -lt $MaximumMonitorCycles) { Wait-SupervisorDelay -Seconds $HealthPollSeconds }
                continue
            }
            if ($recovery -eq 'TRANSPORT_VERIFICATION_FAILED') {
                # The local daemon remains healthy and owns no external runtime.
                # Wait for the official runtime's own recovery loop before the next
                # public status probe; do not create a local restart/recovery storm.
                Write-SupervisorState 'EXTERNAL_RUNTIME_PENDING'
                $cycles++
                if ($MaximumMonitorCycles -eq 0 -or $cycles -lt $MaximumMonitorCycles) { Wait-SupervisorDelay -Seconds $HealthPollSeconds }
                continue
            }
            Write-SupervisorState 'DEGRADED'
            $cycles++
            if ($MaximumMonitorCycles -eq 0 -or $cycles -lt $MaximumMonitorCycles) { Wait-SupervisorDelay -Seconds $FailureCooldownSeconds }
        }
        Write-SupervisorState 'MONITORING_COMPLETE'
    } finally {
        Release-SupervisorGuard -Guard $guard
    }
}

if (-not $LoadOnly) {
    Invoke-CatDeskAutostartSupervisor -InitialDelaySeconds $InitialDelaySeconds -MaximumAttempts $MaximumAttempts -InitialBackoffSeconds $InitialBackoffSeconds -MaximumBackoffSeconds $MaximumBackoffSeconds -HealthPollSeconds $HealthPollSeconds -FailureCooldownSeconds $FailureCooldownSeconds -MaximumMonitorCycles $MaximumMonitorCycles
}
