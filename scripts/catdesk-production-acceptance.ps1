[CmdletBinding()]
param(
    [ValidateSet('preflight', 'compare')]
    [string]$Mode = 'preflight',
    [string]$Workspace = '',
    [string]$ConfigPath = (Join-Path ([Environment]::GetFolderPath('UserProfile')) '.catdesk\config.toml'),
    [string]$ExpectedWakeTargetSha256 = '',
    [string]$PreSnapshotPath = '',
    [string]$PostSnapshotPath = '',
    [hashtable]$TestSeams = @{}
)

$ErrorActionPreference = 'Stop'
if ([string]::IsNullOrWhiteSpace($Workspace)) { $Workspace = Join-Path $PSScriptRoot '..' }
$script:AcceptanceSeams = $TestSeams
$requestedMode = $Mode
$requestedWorkspace = $Workspace
$requestedConfigPath = $ConfigPath
$requestedExpectedWakeTargetSha256 = $ExpectedWakeTargetSha256
$requestedPreSnapshotPath = $PreSnapshotPath
$requestedPostSnapshotPath = $PostSnapshotPath
$bootstrapEngine = Join-Path $PSScriptRoot 'start-catdesk-stack.ps1'
if (-not (Test-Path -LiteralPath $bootstrapEngine -PathType Leaf)) { throw 'canonical bootstrap engine is unavailable' }
. $bootstrapEngine
$Mode = $requestedMode
$Workspace = $requestedWorkspace
$ConfigPath = $requestedConfigPath
$ExpectedWakeTargetSha256 = $requestedExpectedWakeTargetSha256
$PreSnapshotPath = $requestedPreSnapshotPath
$PostSnapshotPath = $requestedPostSnapshotPath

function Invoke-AcceptanceSeam {
    param([string]$Name, [object[]]$Arguments, [scriptblock]$Default)
    if ($script:AcceptanceSeams.ContainsKey($Name)) { return & $script:AcceptanceSeams[$Name] @Arguments }
    return & $Default @Arguments
}

function New-Gate([string]$Id, [ValidateSet('PASS', 'FAIL', 'ATTENTION')][string]$State) {
    [pscustomobject][ordered]@{ id = $Id; state = $State }
}

function Get-Sha256Hex([string]$Value) {
    $sha = [Security.Cryptography.SHA256]::Create()
    try { return (($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($Value)) | ForEach-Object { $_.ToString('x2') }) -join '') }
    finally { $sha.Dispose() }
}

function Test-CanonicalRelease([string]$Root) {
    Invoke-AcceptanceSeam -Name 'CanonicalRelease' -Arguments @($Root) -Default {
        param($workspaceRoot)
        $binary = Join-Path $workspaceRoot 'target\release\catdesk.exe'
        $manifest = Join-Path $workspaceRoot 'target\release\catdesk.exe.sha256'
        if (-not (Test-Path -LiteralPath $binary -PathType Leaf) -or -not (Test-Path -LiteralPath $manifest -PathType Leaf)) { return $null }
        $value = (Get-Content -LiteralPath $manifest -Raw).Trim()
        if ($value -notmatch '^[A-Fa-f0-9]{64}$') { return $null }
        $actual = (Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($actual -ne $value.ToLowerInvariant()) { return $null }
        return $actual
    }
}

function Test-PublicFacade([string]$Root) {
    Invoke-AcceptanceSeam -Name 'PublicFacade' -Arguments @($Root) -Default {
        param($workspaceRoot)
        $path = Join-Path $workspaceRoot 'catdesk.ps1'
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { return $false }
        $tokens = $null; $errors = $null
        [void][System.Management.Automation.Language.Parser]::ParseFile($path, [ref]$tokens, [ref]$errors)
        return ($errors.Count -eq 0)
    }
}

function Get-PublicLifecycleState([string]$Root) {
    Invoke-AcceptanceSeam -Name 'LifecycleStatus' -Arguments @($Root) -Default {
        param($workspaceRoot)
        $facade = Join-Path $workspaceRoot 'catdesk.ps1'
        if (-not (Test-Path -LiteralPath $facade -PathType Leaf)) { return 'STATUS_UNAVAILABLE' }
        $raw = @(& $facade status -Workspace $workspaceRoot)
        $text = [string]::Join("`n", [string[]]$raw)
        if ($text.Length -gt 4096) { return 'STATUS_UNAVAILABLE' }
        try {
            $state = [string](($text | ConvertFrom-Json -ErrorAction Stop).state)
            if ($state -in @('READY', 'LOCAL_READY_EXTERNAL_RUNTIME_PENDING', 'LOCAL_DAEMON_PENDING', 'STATUS_UNAVAILABLE')) { return $state }
        } catch {}
        return 'STATUS_UNAVAILABLE'
    }
}

function Get-AutostartState([string]$Root) {
    Invoke-AcceptanceSeam -Name 'AutostartStatus' -Arguments @($Root) -Default {
        param($workspaceRoot)
        $facade = Join-Path $workspaceRoot 'catdesk.ps1'
        if (-not (Test-Path -LiteralPath $facade -PathType Leaf)) { return 'AUTOSTART_UNAVAILABLE' }
        $raw = @(& $facade autostart status -Workspace $workspaceRoot)
        $text = [string]::Join("`n", [string[]]$raw)
        if ($text.Length -gt 4096) { return 'AUTOSTART_UNAVAILABLE' }
        try {
            $state = [string](($text | ConvertFrom-Json -ErrorAction Stop).state)
            if ($state -in @('AUTOSTART_ENABLED', 'AUTOSTART_DISABLED', 'AUTOSTART_CONFLICT', 'AUTOSTART_UNAVAILABLE')) { return $state }
        } catch {}
        return 'AUTOSTART_UNAVAILABLE'
    }
}

function Test-PersistentAutostartSurface([string]$Root) {
    Invoke-AcceptanceSeam -Name 'PersistentAutostartSurface' -Arguments @($Root) -Default {
        param($workspaceRoot)
        $facade = Get-Content -LiteralPath (Join-Path $workspaceRoot 'catdesk.ps1') -Raw
        $supervisor = Get-Content -LiteralPath (Join-Path $workspaceRoot 'scripts\catdesk-autostart-supervisor.ps1') -Raw
        return ($facade -match 'ExecutionTimeLimit \(\[TimeSpan\]::Zero\)' -and $facade -match 'AllowStartIfOnBatteries' -and $facade -match 'DontStopIfGoingOnBatteries' -and $facade -notmatch 'Register-ScheduledTask.*-Force' -and $supervisor -match 'HealthPollSeconds' -and $supervisor -match 'MaximumMonitorCycles = 0')
    }
}

function Get-WakeEvidence([string]$Root, [string]$ExpectedHash) {
    Invoke-AcceptanceSeam -Name 'WakeEvidence' -Arguments @($Root, $ExpectedHash) -Default {
        param($workspaceRoot, $expected)
        if ($expected -notmatch '^[A-Fa-f0-9]{64}$') { return [pscustomobject]@{ RuntimePresent = $false; TargetBound = $false } }
        $base = Join-Path $workspaceRoot '.catdesk\wake-bridge'
        $configPath = Join-Path $base 'config.json'
        if (-not (Test-Path -LiteralPath $configPath -PathType Leaf)) { return [pscustomobject]@{ RuntimePresent = $false; TargetBound = $false } }
        try {
            $config = Get-Content -LiteralPath $configPath -Raw | ConvertFrom-Json -ErrorAction Stop
            $value = [string]$config.conversation_url
            $profileValue = [string]$config.profile_dir
            if (-not $value -or -not $profileValue) { return [pscustomobject]@{ RuntimePresent = $false; TargetBound = $false } }
            $uri = [Uri]$value
            $urlValid = $uri.Scheme -eq 'https' -and $uri.Host -in @('chatgpt.com', 'chat.openai.com') -and -not $uri.UserInfo -and -not $uri.Query -and -not $uri.Fragment -and $uri.AbsolutePath -match '^/c/[A-Za-z0-9-]+/?$'
            $profile = [IO.Path]::GetFullPath((Join-Path $workspaceRoot $profileValue))
            $rootPrefix = $workspaceRoot.TrimEnd('\') + '\'
            $profileValid = $profile.StartsWith($rootPrefix, [StringComparison]::OrdinalIgnoreCase) -and (Test-Path -LiteralPath $profile -PathType Container)
            $runtimePresent = $profileValid -and (Test-Path -LiteralPath (Join-Path $base 'venv\Scripts\python.exe') -PathType Leaf)
            $targetBound = $urlValid -and ((Get-Sha256Hex $value) -eq $expected.ToLowerInvariant())
            return [pscustomobject]@{ RuntimePresent = $runtimePresent; TargetBound = $targetBound }
        } catch { return [pscustomobject]@{ RuntimePresent = $false; TargetBound = $false } }
    }
}

function Test-OfficialRuntimeOwnership([string]$Root) {
    Invoke-AcceptanceSeam -Name 'OfficialRuntimeOwnership' -Arguments @($Root, $ConfigPath) -Default {
        param($workspaceRoot, $config)
        try {
            if ((Read-OfficialRuntimeProcessMode -Path $config) -ne 'official_runtime') { return $false }
            $alias = Get-ConfiguredRuntimeAlias -Path $config
            $client = Find-TunnelClient -ExplicitPath ''
            if (-not $client) { return $false }
            return [bool](Test-OfficialRuntimeVerified -Root $workspaceRoot -TimeoutSeconds 5 -Alias $alias -ClientPath $client)
        } catch { return $false }
    }
}

function Test-RetentionEvidence([string]$Root) {
    Invoke-AcceptanceSeam -Name 'RetentionEvidence' -Arguments @($Root) -Default {
        param($workspaceRoot)
        $post = Join-Path $workspaceRoot 'docs\orchestrator\T-0057_STORAGE_INVENTORY_RETENTION_POST.json'
        $receipt = Join-Path $workspaceRoot 'docs\orchestrator\T-0057_SAFE_STORAGE_CLEANUP_MANIFEST_EXECUTION.json'
        if (-not (Test-Path -LiteralPath $post -PathType Leaf) -or -not (Test-Path -LiteralPath $receipt -PathType Leaf)) { return $false }
        try { return [double]((Get-Content -LiteralPath $post -Raw | ConvertFrom-Json).workspaceGiB) -le 12.0 } catch { return $false }
    }
}

function Get-CanonicalListenerInstance([string]$Root, [string]$CanonicalHash) {
    Invoke-AcceptanceSeam -Name 'CanonicalListenerInstance' -Arguments @($Root, $ConfigPath, $CanonicalHash) -Default {
        param($workspaceRoot, $config, $buildHash)
        try {
            $canonical = Invoke-AcceptanceSeam -Name 'CanonicalIdentity' -Arguments @($workspaceRoot, $buildHash) -Default {
                param($root, $hash)
                Get-CanonicalCatDeskIdentity -Root $root -ExpectedHash $hash -FingerprintPath (Join-Path $root 'target\release\catdesk.exe.sha256')
            }
            $local = Invoke-AcceptanceSeam -Name 'ConfiguredLocalMcp' -Arguments @($config) -Default {
                param($configPath)
                Get-ConfiguredLocalMcp -Path $configPath
            }
            $listener = Invoke-AcceptanceSeam -Name 'CanonicalListener' -Arguments @($local, $canonical) -Default {
                param($endpoint, $identity)
                Get-LoopbackCatDeskListener -Port $endpoint.Port -Canonical $identity
            }
            if ($null -eq $listener -or -not $listener.MatchesCanonical -or -not $listener.Pid) { return 'UNKNOWN' }
            # The host can legitimately have GUI or foreign CatDesk processes.
            # Bind this acceptance fingerprint to the one exact canonical daemon
            # process instead of treating every same-name process as ambiguity.
            $daemons = @(Invoke-AcceptanceSeam -Name 'CanonicalDaemonCandidates' -Arguments @($canonical) -Default {
                param($identity)
                Get-CatDeskDaemonProcessCandidates -Canonical $identity
            })
            if ($daemons.Count -ne 1 -or -not $daemons[0].MatchesCanonical -or [int]$daemons[0].Pid -ne [int]$listener.Pid) { return 'UNKNOWN' }
            $process = Invoke-AcceptanceSeam -Name 'ProcessById' -Arguments @([int]$listener.Pid) -Default {
                param($processId)
                Get-Process -Id $processId -ErrorAction Stop
            }
            $processPath = Invoke-AcceptanceSeam -Name 'ResolveProcessPath' -Arguments @([string]$process.Path) -Default {
                param($path)
                (Resolve-Path -LiteralPath $path -ErrorAction Stop).Path
            }
            if (-not $process.Path -or -not ([string]$processPath).Equals($canonical.Path, [StringComparison]::OrdinalIgnoreCase)) { return 'UNKNOWN' }
            return (Get-Sha256Hex ("$($listener.Pid)|$($process.StartTime.ToUniversalTime().Ticks)|$buildHash")).Substring(0, 24)
        } catch { return 'UNKNOWN' }
    }
}

function Get-PreflightSnapshot {
    $root = (Resolve-Path -LiteralPath $Workspace -ErrorAction Stop).Path
    $gates = [Collections.Generic.List[object]]::new()
    $release = Test-CanonicalRelease $root
    $gates.Add((New-Gate 'CANONICAL_RELEASE' $(if ($release) { 'PASS' } else { 'FAIL' })))
    $gates.Add((New-Gate 'PUBLIC_LIFECYCLE' $(if (Test-PublicFacade $root) { 'PASS' } else { 'FAIL' })))
    $lifecycle = Get-PublicLifecycleState $root
    $gates.Add((New-Gate 'LIFECYCLE_STATUS' $(if ($lifecycle -eq 'READY') { 'PASS' } elseif ($lifecycle -eq 'LOCAL_READY_EXTERNAL_RUNTIME_PENDING') { 'ATTENTION' } else { 'FAIL' })))
    $autostart = Get-AutostartState $root
    $gates.Add((New-Gate 'AUTOSTART_OWNERSHIP' $(if ($autostart -eq 'AUTOSTART_ENABLED') { 'PASS' } elseif ($autostart -eq 'AUTOSTART_DISABLED') { 'ATTENTION' } else { 'FAIL' })))
    $gates.Add((New-Gate 'PERSISTENT_SUPERVISOR' $(if (Test-PersistentAutostartSurface $root) { 'PASS' } else { 'FAIL' })))
    $wake = Get-WakeEvidence $root $ExpectedWakeTargetSha256
    $gates.Add((New-Gate 'WAKE_RUNTIME_PRESENCE' $(if ($wake.RuntimePresent) { 'PASS' } else { 'FAIL' })))
    $gates.Add((New-Gate 'WAKE_TARGET_BINDING' $(if ($wake.TargetBound) { 'PASS' } else { 'FAIL' })))
    $gates.Add((New-Gate 'RETENTION_EVIDENCE' $(if (Test-RetentionEvidence $root) { 'PASS' } else { 'FAIL' })))
    $gates.Add((New-Gate 'EXTERNAL_RUNTIME_OWNERSHIP' $(if (Test-OfficialRuntimeOwnership $root) { 'PASS' } else { 'FAIL' })))
    $instance = if ($release) { Get-CanonicalListenerInstance $root $release } else { 'UNKNOWN' }
    $gates.Add((New-Gate 'CANONICAL_LISTENER_INSTANCE' $(if ($instance -ne 'UNKNOWN') { 'PASS' } else { 'FAIL' })))
    $states = @($gates | ForEach-Object { $_.state })
    $overall = if ($states -contains 'FAIL') { 'FAIL' } elseif ($states -contains 'ATTENTION') { 'ATTENTION' } else { 'PASS' }
    [pscustomobject][ordered]@{
        schemaVersion = 1
        stage = 'PRE_REBOOT_PREFLIGHT'
        overallState = $overall
        capturedAtUtc = [DateTime]::UtcNow.ToString('o')
        canonicalBuildFingerprint = if ($release) { $release } else { 'UNKNOWN' }
        lifecycleState = $lifecycle
        instanceFingerprint = $instance
        gates = @($gates)
    }
}

function Test-PreflightSnapshot {
    param([object]$Snapshot)

    $requiredProperties = @(
        'schemaVersion', 'stage', 'overallState', 'capturedAtUtc',
        'canonicalBuildFingerprint', 'lifecycleState', 'instanceFingerprint', 'gates'
    )
    $requiredGates = @(
        'CANONICAL_RELEASE', 'PUBLIC_LIFECYCLE', 'LIFECYCLE_STATUS', 'AUTOSTART_OWNERSHIP',
        'PERSISTENT_SUPERVISOR', 'WAKE_RUNTIME_PRESENCE', 'WAKE_TARGET_BINDING',
        'RETENTION_EVIDENCE', 'EXTERNAL_RUNTIME_OWNERSHIP', 'CANONICAL_LISTENER_INSTANCE'
    )
    $supportedOverallStates = @('PASS', 'ATTENTION', 'FAIL')
    $supportedLifecycleStates = @('READY', 'LOCAL_READY_EXTERNAL_RUNTIME_PENDING', 'LOCAL_DAEMON_PENDING', 'STATUS_UNAVAILABLE')
    $supportedGateStates = @('PASS', 'ATTENTION', 'FAIL')

    if ($null -eq $Snapshot) { return $false }
    $propertyNames = @($Snapshot.PSObject.Properties.Name)
    if ($propertyNames.Count -ne $requiredProperties.Count -or @($propertyNames | Where-Object { $_ -notin $requiredProperties }).Count -ne 0) { return $false }
    if (($Snapshot.schemaVersion -isnot [long] -and $Snapshot.schemaVersion -isnot [int]) -or $Snapshot.schemaVersion -ne 1 -or [string]$Snapshot.stage -ne 'PRE_REBOOT_PREFLIGHT') { return $false }
    if ([string]$Snapshot.overallState -notin $supportedOverallStates -or [string]$Snapshot.lifecycleState -notin $supportedLifecycleStates) { return $false }
    if ([string]$Snapshot.canonicalBuildFingerprint -notmatch '^[a-f0-9]{64}$' -or [string]$Snapshot.instanceFingerprint -notmatch '^[a-f0-9]{24}$') { return $false }

    $capturedAt = [DateTimeOffset]::MinValue
    if (-not [DateTimeOffset]::TryParse([string]$Snapshot.capturedAtUtc, [Globalization.CultureInfo]::InvariantCulture, [Globalization.DateTimeStyles]::RoundtripKind, [ref]$capturedAt)) { return $false }
    $gates = @($Snapshot.gates)
    if ($gates.Count -ne $requiredGates.Count) { return $false }
    $gateStates = [Collections.Generic.List[string]]::new()
    for ($index = 0; $index -lt $requiredGates.Count; $index++) {
        $gate = $gates[$index]
        if ($null -eq $gate) { return $false }
        $gateProperties = @($gate.PSObject.Properties.Name)
        if ($gateProperties.Count -ne 2 -or @($gateProperties | Where-Object { $_ -notin @('id', 'state') }).Count -ne 0) { return $false }
        if ([string]$gate.id -ne $requiredGates[$index] -or [string]$gate.state -notin $supportedGateStates) { return $false }
        $gateStates.Add([string]$gate.state)
    }

    $computedOverall = if ($gateStates -contains 'FAIL') { 'FAIL' } elseif ($gateStates -contains 'ATTENTION') { 'ATTENTION' } else { 'PASS' }
    if ([string]$Snapshot.overallState -ne $computedOverall) { return $false }
    $lifecycleGate = $gates[2].state
    if (($Snapshot.lifecycleState -eq 'READY' -and $lifecycleGate -ne 'PASS') -or ($Snapshot.lifecycleState -eq 'LOCAL_READY_EXTERNAL_RUNTIME_PENDING' -and $lifecycleGate -ne 'ATTENTION') -or ($Snapshot.lifecycleState -in @('LOCAL_DAEMON_PENDING', 'STATUS_UNAVAILABLE') -and $lifecycleGate -ne 'FAIL')) { return $false }
    return $true
}

function Test-OnlyExternalRuntimePending([object]$Snapshot) {
    if ($Snapshot.lifecycleState -ne 'LOCAL_READY_EXTERNAL_RUNTIME_PENDING' -or $Snapshot.overallState -ne 'ATTENTION') { return $false }
    foreach ($gate in @($Snapshot.gates)) {
        if ($gate.id -eq 'LIFECYCLE_STATUS') {
            if ($gate.state -ne 'ATTENTION') { return $false }
        } elseif ($gate.state -ne 'PASS') { return $false }
    }
    return $true
}

function Compare-AcceptanceSnapshots {
    $pre = $null; $post = $null
    try {
        if ((Test-Path -LiteralPath $PreSnapshotPath -PathType Leaf) -and (Test-Path -LiteralPath $PostSnapshotPath -PathType Leaf)) {
            $pre = Get-Content -LiteralPath $PreSnapshotPath -Raw -ErrorAction Stop | ConvertFrom-Json -ErrorAction Stop
            $post = Get-Content -LiteralPath $PostSnapshotPath -Raw -ErrorAction Stop | ConvertFrom-Json -ErrorAction Stop
        }
    } catch { $pre = $null; $post = $null }
    $snapshotsValid = (Test-PreflightSnapshot $pre) -and (Test-PreflightSnapshot $post)
    $sameBuild = $snapshotsValid -and ($pre.canonicalBuildFingerprint -eq $post.canonicalBuildFingerprint)
    $newInstance = $snapshotsValid -and ($pre.instanceFingerprint -ne $post.instanceFingerprint)
    $postReady = $snapshotsValid -and ($post.lifecycleState -eq 'READY')
    $externalPending = $snapshotsValid -and (Test-OnlyExternalRuntimePending $post)
    $postPassesAllGates = $snapshotsValid -and (@($post.gates | Where-Object { $_.state -ne 'PASS' }).Count -eq 0)
    $state = if ($sameBuild -and $newInstance -and $postReady -and $post.overallState -eq 'PASS' -and $postPassesAllGates) { 'PASS' } elseif ($sameBuild -and $newInstance -and $externalPending) { 'ATTENTION' } else { 'FAIL' }
    [pscustomobject][ordered]@{
        schemaVersion = 1
        stage = 'POST_REBOOT_COMPARISON'
        overallState = $state
        canonicalBuildUnchanged = $sameBuild
        newCatDeskInstance = $newInstance
        externalRuntimePending = $externalPending
        postLifecycleState = if ($postReady) { 'READY' } elseif ($externalPending) { 'LOCAL_READY_EXTERNAL_RUNTIME_PENDING' } else { 'UNAVAILABLE' }
    }
}

if ($Mode -eq 'compare') { Compare-AcceptanceSnapshots | ConvertTo-Json -Depth 5 }
else { Get-PreflightSnapshot | ConvertTo-Json -Depth 5 }
