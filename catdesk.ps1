[CmdletBinding()]
param(
    [Parameter(Mandatory = $true, Position = 0)]
    [ValidateSet('install', 'start', 'status', 'diagnose', 'recover', 'stop', 'autostart', 'wake')]
    [string]$Command,
    [Parameter(Position = 1)]
    [string]$AutostartAction = '',
    [Parameter(Position = 2)]
    [string]$WakeTerminalTaskId = '',
    [string]$Workspace = '',
    [string]$ConfigPath = (Join-Path ([Environment]::GetFolderPath('UserProfile')) '.catdesk\config.toml'),
    [string]$ExpectedBuildSha256 = '',
    [string]$BuildFingerprintPath = '',
    [int]$ReadyTimeoutSeconds = 180,
    [hashtable]$TestSeams = @{}
)

# The only public consumer lifecycle entry point. Lower-level scripts remain
# internal/advanced implementation details. Routine commands use an existing
# fingerprinted release and never initiate build or release-creation work.
$ErrorActionPreference = 'Stop'
# `$PSScriptRoot` is not reliably populated while PowerShell is evaluating
# parameter default expressions for a script launched through `-File`. Resolve
# facade-relative defaults only after parameter binding has completed.
if ([string]::IsNullOrWhiteSpace($Workspace)) { $Workspace = $PSScriptRoot }
if ([string]::IsNullOrWhiteSpace($BuildFingerprintPath)) {
    $BuildFingerprintPath = Join-Path $PSScriptRoot 'target\release\catdesk.exe.sha256'
}
$script:LifecycleSeams = $TestSeams
$engineExpectedPath = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot 'scripts\start-catdesk-stack.ps1'))
try {
    $engineInfo = Get-Item -LiteralPath $engineExpectedPath -Force -ErrorAction Stop
    if (-not ($engineInfo -is [System.IO.FileInfo])) { throw 'canonical lifecycle engine is unavailable' }
    if (($engineInfo.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'canonical lifecycle engine is unavailable' }
    $engineObservedPath = [System.IO.Path]::GetFullPath($engineInfo.FullName)
    if (-not [string]::Equals($engineExpectedPath, $engineObservedPath, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw 'canonical lifecycle engine is unavailable'
    }
    $enginePath = $engineObservedPath
    # The dot-sourced lifecycle engine has its own parameter block with several of
    # the same names as this public facade.  Bind every duplicate explicitly:
    # otherwise the helper evaluates its defaults in this scope and can transiently
    # replace caller-bound identity/timeout values (or fail while resolving its
    # own `$PSScriptRoot`).  Retain the snapshots after import as a second barrier
    # against a future helper body assigning one of these public variables.
    $facadeWorkspace = $Workspace
    $facadeConfigPath = $ConfigPath
    $facadeExpectedBuildSha256 = $ExpectedBuildSha256
    $facadeBuildFingerprintPath = $BuildFingerprintPath
    $facadeReadyTimeoutSeconds = $ReadyTimeoutSeconds
    . $enginePath -Workspace $facadeWorkspace -ConfigPath $facadeConfigPath -ExpectedBuildSha256 $facadeExpectedBuildSha256 -BuildFingerprintPath $facadeBuildFingerprintPath -ReadyTimeoutSeconds $facadeReadyTimeoutSeconds
    $Workspace = $facadeWorkspace
    $ConfigPath = $facadeConfigPath
    $ExpectedBuildSha256 = $facadeExpectedBuildSha256
    $BuildFingerprintPath = $facadeBuildFingerprintPath
    $ReadyTimeoutSeconds = $facadeReadyTimeoutSeconds
} catch {
    [pscustomobject][ordered]@{ command = $Command; state = 'STATUS_UNAVAILABLE'; detail = 'stage=LIFECYCLE_INITIALIZATION' } |
        ConvertTo-Json -Compress
    exit 0
}

function Invoke-LifecycleSeam {
    param([string]$Name, [object[]]$Arguments, [scriptblock]$Default)
    if ($script:LifecycleSeams.ContainsKey($Name)) { return & $script:LifecycleSeams[$Name] @Arguments }
    return & $Default @Arguments
}

function Write-LifecycleStatus {
    param([string]$State, [string]$Detail = 'redacted')
    # States are intentionally fixed vocabulary; never project an exception,
    # configured route, tunnel alias, endpoint, credential, or filesystem path.
    [pscustomobject][ordered]@{ command = $Command; state = $State; detail = $Detail } |
        ConvertTo-Json -Compress
}

function New-LifecycleDiagnosticLayer {
    param([string]$Layer, [string]$State, [string]$Gate)
    $allowedLayers = @(
        'LIFECYCLE_ENGINE', 'CANONICAL_RELEASE', 'RECOVERY_AUTHORITY',
        'LOCAL_MCP_CONFIG', 'LOCAL_DAEMON', 'LOCAL_MCP_PROTOCOL',
        'WAKE_RUNTIME', 'OFFICIAL_RUNTIME'
    )
    $allowedStates = @('READY', 'DEGRADED', 'FAILED', 'NOT_REQUIRED')
    if ($Layer -notin $allowedLayers -or $State -notin $allowedStates -or
        [string]::IsNullOrWhiteSpace($Gate) -or $Gate -notmatch '^[A-Z0-9_]{1,96}$') {
        throw 'lifecycle diagnostic layer is invalid'
    }
    [pscustomobject][ordered]@{ layer = $Layer; state = $State; gate = $Gate }
}

function Write-LifecycleDiagnosis {
    param(
        [string]$State,
        [string]$PrimaryLayer,
        [string]$NextAction,
        [Collections.Generic.List[object]]$Layers
    )
    if ($State -notin @('HEALTHY', 'DEGRADED', 'RECOVERY_AVAILABLE', 'ACTION_REQUIRED')) {
        throw 'lifecycle diagnosis state is invalid'
    }
    if ($PrimaryLayer -notin @(
        'NONE', 'LIFECYCLE_ENGINE', 'CANONICAL_RELEASE', 'RECOVERY_AUTHORITY',
        'LOCAL_MCP_CONFIG', 'LOCAL_DAEMON', 'LOCAL_MCP_PROTOCOL',
        'WAKE_RUNTIME', 'OFFICIAL_RUNTIME'
    )) { throw 'lifecycle diagnosis primary layer is invalid' }
    if ($NextAction -notin @('NONE', 'RUN_RECOVER', 'RUN_INSTALL', 'OPERATOR_ATTENTION')) {
        throw 'lifecycle diagnosis next action is invalid'
    }
    [pscustomobject][ordered]@{
        command = $Command
        state = $State
        primaryLayer = $PrimaryLayer
        nextAction = $NextAction
        layers = @($Layers)
    } | ConvertTo-Json -Compress -Depth 5
}

function Get-LifecycleCanonicalFailureGate {
    param([string]$Message)
    switch ($Message) {
        'CATDESK_CANONICAL_WORKSPACE_RESOLVE' { 'CANONICAL_WORKSPACE_RESOLVE' }
        'CATDESK_CANONICAL_BINARY_MISSING' { 'CANONICAL_BINARY_MISSING' }
        'CATDESK_CANONICAL_FINGERPRINT_MISSING' { 'CANONICAL_FINGERPRINT_MISSING' }
        'CATDESK_CANONICAL_FINGERPRINT_INVALID' { 'CANONICAL_FINGERPRINT_INVALID' }
        'CATDESK_CANONICAL_HASH_MISMATCH' { 'CANONICAL_HASH_MISMATCH' }
        'CATDESK_CANONICAL_IDENTITY_INTERNAL' { 'CANONICAL_IDENTITY_INTERNAL' }
        default { 'CANONICAL_IDENTITY' }
    }
}

function Get-LifecycleLocalMcpReadinessResult {
    param($LocalMcp, $Canonical)
    $observed = Invoke-LifecycleSeam -Name 'LocalMcpReadiness' -Arguments @($LocalMcp, $Canonical) -Default {
        param($endpoint, $identity)
        Get-LocalMcpReadiness -LocalMcp $endpoint -Canonical $identity
    }
    if ($observed -is [bool]) {
        return [pscustomobject]@{
            Ready = [bool]$observed
            Gate = if ($observed) { 'READY' } else { 'LOCAL_MCP_RESPONSE_UNAVAILABLE' }
        }
    }
    if ($null -eq $observed -or $null -eq $observed.PSObject.Properties['Ready'] -or
        $null -eq $observed.PSObject.Properties['Gate']) {
        throw 'local MCP readiness result is invalid'
    }
    $gate = [string]$observed.Gate
    if ($gate -notin @(
        'READY', 'LOCAL_MCP_LISTENER_MISSING', 'LOCAL_MCP_LISTENER_IDENTITY_MISMATCH',
        'LOCAL_MCP_LAUNCHED_PROCESS_MISMATCH', 'LOCAL_MCP_RESPONSE_TIMEOUT',
        'LOCAL_MCP_RESPONSE_UNAVAILABLE', 'LOCAL_MCP_RESPONSE_INVALID',
        'LOCAL_MCP_PROTOCOL_UNREADY'
    )) { throw 'local MCP readiness gate is invalid' }
    return [pscustomobject]@{ Ready = [bool]$observed.Ready; Gate = $gate }
}

function Get-LifecycleOfficialRuntimeVerificationResult {
    if ($script:LifecycleSeams.ContainsKey('RuntimeStatus')) {
        $observed = & $script:LifecycleSeams['RuntimeStatus'] $Workspace $ConfigPath
        if ($observed -is [bool]) {
            return [pscustomobject]@{
                Verified = [bool]$observed
                Gate = if ($observed) { 'READY' } else { 'RUNTIME_STATUS_NOT_READY' }
            }
        }
        if ($null -eq $observed -or $null -eq $observed.PSObject.Properties['Verified'] -or
            $null -eq $observed.PSObject.Properties['Gate']) {
            throw 'runtime verification result is invalid'
        }
        $gate = [string]$observed.Gate
        if ($gate -notin @(
            'READY', 'RUNTIME_CLIENT_UNAVAILABLE', 'RUNTIME_STATUS_TIMEOUT',
            'RUNTIME_STATUS_OVERSIZED', 'RUNTIME_STATUS_COMMAND_FAILED',
            'RUNTIME_STATUS_INVALID', 'RUNTIME_STATUS_UNAVAILABLE',
            'RUNTIME_STATUS_NOT_READY', 'RUNTIME_STATUS_TRANSIENT',
            'RUNTIME_HEALTH_REFERENCE_INVALID', 'RUNTIME_HEALTHZ_UNAVAILABLE',
            'RUNTIME_HEALTHZ_FAILED', 'RUNTIME_READYZ_UNAVAILABLE',
            'RUNTIME_READYZ_FAILED', 'LOCAL_MCP_PENDING'
        )) { throw 'runtime verification gate is invalid' }
        return [pscustomobject]@{ Verified = [bool]$observed.Verified; Gate = $gate }
    }
    $alias = Get-ConfiguredRuntimeAlias -Path $ConfigPath
    $client = Find-TunnelClient -ExplicitPath ''
    if (-not $client) {
        return [pscustomobject]@{ Verified = $false; Gate = 'RUNTIME_CLIENT_UNAVAILABLE' }
    }
    return Get-OfficialRuntimeVerification -Root $Workspace -TimeoutSeconds 5 -Alias $alias -ClientPath $client
}

function Get-LifecycleCanonicalIdentity {
    Invoke-LifecycleSeam -Name 'CanonicalIdentity' -Arguments @($Workspace, $ExpectedBuildSha256, $BuildFingerprintPath) -Default {
        param($root, $expected, $manifest)
        try {
            $resolvedRoot = (Resolve-Path -LiteralPath $root -ErrorAction Stop).Path
        } catch {
            throw 'CATDESK_CANONICAL_WORKSPACE_RESOLVE'
        }
        try {
            Get-CanonicalCatDeskIdentity -Root $resolvedRoot -ExpectedHash $expected -FingerprintPath $manifest
        } catch {
            # Map only fixed internal lifecycle failures to fixed non-secret
            # categories. Never project the original exception text.
            switch ([string]$_.Exception.Message) {
                'canonical CatDesk executable is unavailable' { throw 'CATDESK_CANONICAL_BINARY_MISSING' }
                'verified release fingerprint is unavailable' { throw 'CATDESK_CANONICAL_FINGERPRINT_MISSING' }
                'verified release fingerprint is malformed' { throw 'CATDESK_CANONICAL_FINGERPRINT_INVALID' }
                'canonical CatDesk build fingerprint did not match' { throw 'CATDESK_CANONICAL_HASH_MISMATCH' }
                default { throw 'CATDESK_CANONICAL_IDENTITY_INTERNAL' }
            }
        }
    }
}

function Get-LifecycleLocalMcp {
    Invoke-LifecycleSeam -Name 'LocalMcp' -Arguments @($ConfigPath) -Default {
        param($path)
        Get-ConfiguredLocalMcp -Path $path
    }
}

function Get-LifecycleListener {
    param($LocalMcp, $Canonical)
    Invoke-LifecycleSeam -Name 'Listener' -Arguments @($LocalMcp, $Canonical) -Default {
        param($endpoint, $identity)
        Get-LoopbackCatDeskListener -Port $endpoint.Port -Canonical $identity
    }
}

function Test-LifecycleLocalMcpReadiness {
    param($LocalMcp, $Canonical)
    return [bool](Get-LifecycleLocalMcpReadinessResult -LocalMcp $LocalMcp -Canonical $Canonical).Ready
}

function Get-LifecycleSha256Hex {
    param([string]$Value)
    $sha = [Security.Cryptography.SHA256]::Create()
    try {
        return (($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($Value)) | ForEach-Object { $_.ToString('x2') }) -join '')
    } finally {
        $sha.Dispose()
    }
}

function Get-AutostartDefinition {
    $root = (Resolve-Path -LiteralPath $Workspace -ErrorAction Stop).Path
    $workspaceId = (Get-LifecycleSha256Hex $root).Substring(0, 16)
    $supervisor = Join-Path $PSScriptRoot 'scripts\catdesk-autostart-supervisor.ps1'
    if (-not (Test-Path -LiteralPath $supervisor -PathType Leaf)) { throw 'autostart supervisor is unavailable' }
    $powershellHost = Get-AutostartPowerShellHost
    $arguments = '-NoProfile -NonInteractive -WindowStyle Hidden -ExecutionPolicy Bypass -File "{0}" -Workspace "{1}"' -f $supervisor, $root
    $monitorArguments = '-NoProfile -NonInteractive -WindowStyle Hidden -ExecutionPolicy Bypass -File "{0}" -Workspace "{1}" -InitialDelaySeconds 0' -f $supervisor, $root
    $runKeyName = "CatDesk.Autostart.$workspaceId"
    [pscustomobject]@{
        TaskPath = '\'
        TaskName = $runKeyName
        WorkspaceId = $workspaceId
        Execute = $powershellHost
        Arguments = $arguments
        MonitorArguments = $monitorArguments
        UserId = (Get-AutostartCurrentUser)
        RunKeyPath = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
        RunKeyName = $runKeyName
        RunKeyValue = ('"{0}" {1}' -f $powershellHost, $arguments)
    }
}

function Get-AutostartPowerShellHost {
    Invoke-LifecycleSeam -Name 'AutostartPowerShellHost' -Arguments @() -Default {
        # A persisted task/Run entry needs a stable, absolute executable
        # identity. Falling back to the unresolved command name causes a valid
        # entry created by Windows PowerShell to look foreign when the facade
        # is later invoked from a different shell host.
        $command = Get-Command -Name 'powershell.exe' -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($null -ne $command -and $command.Path -and (Test-Path -LiteralPath $command.Path -PathType Leaf)) {
            return (Resolve-Path -LiteralPath $command.Path -ErrorAction Stop).Path
        }
        $currentHost = Join-Path $PSHOME 'powershell.exe'
        if (Test-Path -LiteralPath $currentHost -PathType Leaf) {
            return (Resolve-Path -LiteralPath $currentHost -ErrorAction Stop).Path
        }
        throw 'autostart PowerShell host is unavailable'
    }
}

function Get-AutostartCurrentUser {
    Invoke-LifecycleSeam -Name 'CurrentUser' -Arguments @() -Default {
        [Security.Principal.WindowsIdentity]::GetCurrent().Name
    }
}

function Invoke-WakePolicyMcp {
    param([string]$Tool, [hashtable]$Arguments)
    $canonical = Get-LifecycleCanonicalIdentity
    $local = Get-LifecycleLocalMcp
    $listener = Get-LifecycleListener -LocalMcp $local -Canonical $canonical
    if ($null -eq $listener -or -not $listener.MatchesCanonical) {
        throw 'verified local CatDesk MCP is unavailable'
    }
    $request = @{ jsonrpc = '2.0'; id = 'catdesk-wake-policy'; method = 'tools/call'; params = @{ name = $Tool; arguments = $Arguments } } |
        ConvertTo-Json -Compress -Depth 5
    $response = Invoke-LifecycleSeam -Name 'WakeMcpCall' -Arguments @($local.Uri, $request) -Default {
        param($uri, $body)
        Invoke-SilentLoopbackHttp -Uri $uri -Method Post -ContentType 'application/json' -Body $body -TimeoutSeconds 5
    }
    if ($null -eq $response -or $response.StatusCode -lt 200 -or $response.StatusCode -ge 300) {
        throw 'local CatDesk MCP did not accept the wake policy request'
    }
    $content = [string]$response.Content
    if ($content.Length -eq 0 -or $content.Length -gt 32768) { throw 'local CatDesk MCP returned an invalid wake policy response' }
    $parsed = $content | ConvertFrom-Json -ErrorAction Stop
    if ($parsed.error -or $parsed.result.isError -or $null -eq $parsed.result.structuredContent) {
        throw 'local CatDesk MCP rejected the wake policy request'
    }
    return $parsed.result.structuredContent
}

function Write-WakePolicyStatus {
    param($Policy, [string]$State = 'WAKE_POLICY', [string]$Readiness = 'UNKNOWN')
    $mode = [string]$Policy.mode
    if ($mode -notin @('MANUAL_OFF', 'INDEFINITE', 'THROUGH_TASK', 'UNTIL_PROVIDER_EXHAUSTED')) {
        throw 'local CatDesk MCP returned an invalid wake policy'
    }
    $generation = [uint64]$Policy.generation
    $terminalTaskId = [string]$Policy.terminal_task_id
    if ($Readiness -notin @('READY', 'NOT_READY', 'UNKNOWN')) { throw 'local CatDesk MCP returned an invalid wake readiness state' }
    $stoppingCondition = switch ($mode) {
        'THROUGH_TASK' { "EXACT_TASK:$terminalTaskId" }
        'UNTIL_PROVIDER_EXHAUSTED' { 'PROVIDER_CHAIN_EXHAUSTED' }
        'MANUAL_OFF' { 'MANUAL' }
        default { 'NONE' }
    }
    [pscustomobject][ordered]@{
        command = $Command
        action = $AutostartAction
        state = $State
        mode = $mode
        generation = $generation
        readiness = $Readiness
        stoppingCondition = $stoppingCondition
        stoppedReason = [string]$Policy.stopped_reason
    } | ConvertTo-Json -Compress
}

function Invoke-WakePolicyLifecycle {
    if ($AutostartAction -notin @('status', 'off', 'on', 'through', 'until-exhausted')) {
        throw 'wake requires status, off, on, through <exact-task-id>, or until-exhausted'
    }
    if ($AutostartAction -eq 'through') {
        if ($WakeTerminalTaskId -notmatch '^[A-Za-z0-9_-]{1,128}$') { throw 'wake through requires one exact stable task ID' }
    } elseif ($WakeTerminalTaskId) {
        throw 'wake action does not accept a terminal task ID'
    }
    try {
        $current = Invoke-WakePolicyMcp -Tool 'autonomy_wake_policy_get' -Arguments @{}
        if ($AutostartAction -eq 'status') {
            Write-WakePolicyStatus -Policy $current.policy -Readiness ([string]$current.readiness)
            return
        }
        $mode = switch ($AutostartAction) {
            'off' { 'MANUAL_OFF' }
            'on' { 'INDEFINITE' }
            'through' { 'THROUGH_TASK' }
            'until-exhausted' { 'UNTIL_PROVIDER_EXHAUSTED' }
        }
        $arguments = @{ expectedGeneration = [uint64]$current.policy.generation; mode = $mode }
        if ($AutostartAction -eq 'through') { $arguments.terminalTaskId = $WakeTerminalTaskId }
        $updated = Invoke-WakePolicyMcp -Tool 'autonomy_wake_policy_set' -Arguments $arguments
        Write-WakePolicyStatus -Policy $updated.policy -State 'WAKE_POLICY_UPDATED'
    } catch {
        # Do not reveal local endpoints, configuration, or internal errors.
        Write-LifecycleStatus 'WAKE_POLICY_UNAVAILABLE'
    }
}

function Get-AutostartTask {
    param($Definition)
    Invoke-LifecycleSeam -Name 'ScheduledTaskGet' -Arguments @($Definition) -Default {
        param($expected)
        try { Get-ScheduledTask -TaskPath $expected.TaskPath -TaskName $expected.TaskName -ErrorAction Stop } catch { $null }
    }
}

function Test-OwnedAutostartTask {
    param($Task, $Definition)
    if ($null -eq $Task) { return $false }
    $actions = @($Task.Actions)
    if ($actions.Count -ne 1 -or $actions[0].Execute -ne $Definition.Execute -or $actions[0].Arguments -ne $Definition.Arguments) { return $false }
    $principal = $Task.Principal
    if ($null -eq $principal -or [string]$principal.UserId -ne $Definition.UserId -or [string]$principal.LogonType -notmatch 'Interactive' -or [string]$principal.RunLevel -notmatch 'Limited') { return $false }
    $triggers = @($Task.Triggers)
    if ($triggers.Count -ne 1 -or [string]$triggers[0].CimClass.CimClassName -notmatch 'Logon') { return $false }
    $settings = $Task.Settings
    if ($null -eq $settings -or -not [bool]$settings.StartWhenAvailable -or -not [bool]$settings.AllowStartIfOnBatteries -or -not [bool]$settings.DontStopIfGoingOnBatteries -or [string]$settings.MultipleInstances -notmatch 'IgnoreNew' -or -not (Test-UnlimitedAutostartExecutionTime -Limit $settings.ExecutionTimeLimit)) { return $false }
    return $true
}

function Test-UnlimitedAutostartExecutionTime {
    param($Limit)
    if ($Limit -is [TimeSpan]) { return $Limit -eq [TimeSpan]::Zero }
    try { return ([TimeSpan]$Limit) -eq [TimeSpan]::Zero } catch { return $false }
}

function Get-OwnedAutostartRunKeyValue {
    param($Definition)
    Invoke-LifecycleSeam -Name 'AutostartRunKeyGet' -Arguments @($Definition) -Default {
        param($expected)
        try {
            $item = Get-ItemProperty -LiteralPath $expected.RunKeyPath -Name $expected.RunKeyName -ErrorAction Stop
            return [string]$item.($expected.RunKeyName)
        } catch {
            return $null
        }
    }
}

function Test-OwnedAutostartRunKey {
    param($Value, $Definition)
    return (-not [string]::IsNullOrWhiteSpace([string]$Value) -and [string]$Value -eq [string]$Definition.RunKeyValue)
}

function Register-OwnedAutostartRunKey {
    param($Definition)
    Invoke-LifecycleSeam -Name 'AutostartRunKeyRegister' -Arguments @($Definition) -Default {
        param($expected)
        if (-not (Test-Path -LiteralPath $expected.RunKeyPath)) {
            [void](New-Item -Path $expected.RunKeyPath -Force)
        }
        Set-ItemProperty -LiteralPath $expected.RunKeyPath -Name $expected.RunKeyName -Value $expected.RunKeyValue -Type String -ErrorAction Stop
    }
}

function Unregister-OwnedAutostartRunKey {
    param($Definition)
    Invoke-LifecycleSeam -Name 'AutostartRunKeyUnregister' -Arguments @($Definition) -Default {
        param($expected)
        Remove-ItemProperty -LiteralPath $expected.RunKeyPath -Name $expected.RunKeyName -ErrorAction Stop
    }
}

function Test-AutostartSupervisorRunning {
    param($Definition)
    Invoke-LifecycleSeam -Name 'AutostartSupervisorRunning' -Arguments @($Definition) -Default {
        param($expected)
        $handle = $null
        try {
            if ([Threading.Mutex]::TryOpenExisting("Local\CatDesk.Autostart.$($expected.WorkspaceId)", [ref]$handle)) {
                $handle.Dispose()
                return $true
            }
        } catch {}
        if ($null -ne $handle) { try { $handle.Dispose() } catch {} }
        return $false
    }
}

function Start-AutostartSupervisor {
    param($Definition)
    Invoke-LifecycleSeam -Name 'AutostartSupervisorStart' -Arguments @($Definition) -Default {
        param($expected)
        Start-Process -FilePath $expected.Execute -ArgumentList $expected.MonitorArguments -WindowStyle Hidden -ErrorAction Stop | Out-Null
    }
}

function Ensure-AutostartSupervisorRunning {
    param($Definition)
    if (Test-AutostartSupervisorRunning -Definition $Definition) { return }
    Start-AutostartSupervisor -Definition $Definition
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    while ([DateTime]::UtcNow -lt $deadline) {
        if (Test-AutostartSupervisorRunning -Definition $Definition) { return }
        Start-Sleep -Milliseconds 250
    }
    throw 'autostart supervisor did not become ready'
}

function Register-OwnedAutostartTask {
    param($Definition)
    Invoke-LifecycleSeam -Name 'ScheduledTaskRegister' -Arguments @($Definition) -Default {
        param($expected)
        $action = New-ScheduledTaskAction -Execute $expected.Execute -Argument $expected.Arguments
        $trigger = New-ScheduledTaskTrigger -AtLogOn
        $principal = New-ScheduledTaskPrincipal -UserId $expected.UserId -LogonType Interactive -RunLevel Limited
        $settings = New-ScheduledTaskSettingsSet -StartWhenAvailable -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit ([TimeSpan]::Zero) -MultipleInstances IgnoreNew -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 5) -Hidden
        Register-ScheduledTask -TaskPath $expected.TaskPath -TaskName $expected.TaskName -Action $action -Trigger $trigger -Principal $principal -Settings $settings | Out-Null
    }
}

function Unregister-OwnedAutostartTask {
    param($Definition)
    Invoke-LifecycleSeam -Name 'ScheduledTaskUnregister' -Arguments @($Definition) -Default {
        param($expected)
        Unregister-ScheduledTask -TaskPath $expected.TaskPath -TaskName $expected.TaskName -Confirm:$false -ErrorAction Stop
    }
}

function Invoke-AutostartLifecycle {
    if ($AutostartAction -notin @('enable', 'status', 'disable')) { throw 'autostart requires enable, status, or disable' }
    try {
        $definition = Get-AutostartDefinition
        $task = Get-AutostartTask -Definition $definition
        $taskOwned = Test-OwnedAutostartTask -Task $task -Definition $definition
        $runValue = Get-OwnedAutostartRunKeyValue -Definition $definition
        $runOwned = Test-OwnedAutostartRunKey -Value $runValue -Definition $definition
        $taskConflict = ($null -ne $task -and -not $taskOwned)
        $runConflict = (-not [string]::IsNullOrWhiteSpace([string]$runValue) -and -not $runOwned)
        if ($taskConflict -or $runConflict) { Write-LifecycleStatus 'AUTOSTART_CONFLICT'; return }
        switch ($AutostartAction) {
            'status' {
                if ($taskOwned -or $runOwned) { Write-LifecycleStatus 'AUTOSTART_ENABLED'; return }
                Write-LifecycleStatus 'AUTOSTART_DISABLED'; return
            }
            'enable' {
                if (-not $taskOwned -and -not $runOwned) {
                    $taskRegistered = $false
                    try {
                        Register-OwnedAutostartTask -Definition $definition
                        $registered = Get-AutostartTask -Definition $definition
                        $taskRegistered = Test-OwnedAutostartTask -Task $registered -Definition $definition
                    } catch {
                        $taskRegistered = $false
                    }
                    if (-not $taskRegistered) {
                        # Standard-user Task Scheduler policy differs across Windows
                        # estates.  A fixed current-user Run value is the bounded,
                        # non-elevated persistence fallback; exact ownership is
                        # still required and a same-name mismatch is never replaced.
                        Register-OwnedAutostartRunKey -Definition $definition
                        $runValue = Get-OwnedAutostartRunKeyValue -Definition $definition
                        if (-not (Test-OwnedAutostartRunKey -Value $runValue -Definition $definition)) {
                            throw 'autostart persistence did not match the owned definition'
                        }
                    }
                }
                # Registration-at-logon alone does not protect the current session.
                # Arm the singleton supervisor now and prove its named guard exists
                # before reporting ENABLED, so a subsequent daemon-loss drill cannot
                # strand control between registration and the next logon.
                Ensure-AutostartSupervisorRunning -Definition $definition
                Write-LifecycleStatus 'AUTOSTART_ENABLED'; return
            }
            'disable' {
                if ($taskOwned) { Unregister-OwnedAutostartTask -Definition $definition }
                if ($runOwned) { Unregister-OwnedAutostartRunKey -Definition $definition }
                Write-LifecycleStatus 'AUTOSTART_DISABLED'; return
            }
        }
    } catch {
        Invoke-LifecycleSeam -Name 'AutostartFailure' -Arguments @($_.Exception.Message) -Default { param($ignored) }
        Write-LifecycleStatus 'AUTOSTART_UNAVAILABLE'
    }
}

function Invoke-CanonicalRecovery {
    param([string]$RequestedCommand)
    # Recovery is the one public lifecycle operation allowed to repair a broken
    # canonical release pair. Do not require canonical identity here: the
    # recovery engine first reconciles an interrupted reviewed promotion or a
    # persistent last-known-good pair, then revalidates identity before launch.
    $raw = @(Invoke-LifecycleSeam -Name 'Recovery' -Arguments @($enginePath, $Workspace, $ConfigPath, $ExpectedBuildSha256, $BuildFingerprintPath, $ReadyTimeoutSeconds, $RequestedCommand) -Default {
        param($engine, $root, $config, $expected, $manifest, $timeout, $requested)
        # Both public start and recover use the same proven, release-only engine.
        & $engine -Mode recover -Workspace $root -ConfigPath $config -ExpectedBuildSha256 $expected -BuildFingerprintPath $manifest -ReadyTimeoutSeconds $timeout -Execute
    })
    $result = Get-LifecycleRecoveryResult -Raw $raw
    if ($null -ne $result -and $result.State -eq 'LKG_AUTHORITY_MISSING') {
        Write-LifecycleStatus 'LKG_AUTHORITY_MISSING'
        return
    }
    if ($null -ne $result -and $result.State -eq 'LKG_AUTHORITY_AMBIGUOUS_OR_DAMAGED') {
        Write-LifecycleStatus 'LKG_AUTHORITY_AMBIGUOUS_OR_DAMAGED'
        return
    }
    if ($null -ne $result -and $result.State -eq 'RECOVERY_RELEASE_AUTHORITY_REQUIRED') {
        Write-LifecycleStatus 'RECOVERY_AUTHORITY_REQUIRED'
        return
    }
    if ($null -ne $result -and $result.State -eq 'CONNECTED_VERIFIED') {
        $state = if ($result.RecoverySource -eq 'INTERRUPTED_PROMOTION') {
            'INTERRUPTED_TRANSACTION_COMPLETED'
        } elseif ($result.RecoverySource -eq 'LAST_KNOWN_GOOD') {
            'RESTORED_KNOWN_GOOD'
        } else {
            'CONNECTED_VERIFIED'
        }
        Write-LifecycleStatus $state 'canonical local daemon and external runtime verified'
        return
    }
    if ($null -ne $result -and $result.State -eq 'TRANSPORT_VERIFICATION_FAILED') {
        Write-LifecycleStatus 'TRANSPORT_VERIFICATION_FAILED' "gate=$($result.Gate)"
        return
    }
    Write-LifecycleStatus 'TRANSPORT_VERIFICATION_FAILED'
}

function Get-LifecycleRecoveryResult {
    param([object[]]$Raw)
    $text = [string]::Join("`n", [string[]]$Raw)
    if ($text.Length -gt 4096) { return $null }
    try {
        $parsed = $text | ConvertFrom-Json -ErrorAction Stop
        if ($parsed -is [array] -or $null -eq $parsed) { return $null }
        $state = [string]$parsed.State
        $source = if ($parsed.PSObject.Properties['RecoverySource']) { [string]$parsed.RecoverySource } else { '' }
        if ($state -notin @('CONNECTED_VERIFIED', 'RECOVERY_READY', 'LKG_AUTHORITY_MISSING', 'LKG_AUTHORITY_AMBIGUOUS_OR_DAMAGED', 'RECOVERY_RELEASE_AUTHORITY_REQUIRED', 'TRANSPORT_VERIFICATION_FAILED')) {
            return $null
        }
        $gate = if ($parsed.PSObject.Properties['Gate']) { [string]$parsed.Gate } else { '' }
        $allowedGates = @(
            'RUNTIME_CLIENT_UNAVAILABLE', 'RUNTIME_STATUS_TIMEOUT',
            'RUNTIME_STATUS_OVERSIZED', 'RUNTIME_STATUS_COMMAND_FAILED',
            'RUNTIME_STATUS_INVALID', 'RUNTIME_STATUS_UNAVAILABLE',
            'RUNTIME_STATUS_NOT_READY', 'RUNTIME_STATUS_TRANSIENT',
            'RUNTIME_HEALTH_REFERENCE_INVALID', 'RUNTIME_HEALTHZ_UNAVAILABLE',
            'RUNTIME_HEALTHZ_FAILED', 'RUNTIME_READYZ_UNAVAILABLE',
            'RUNTIME_READYZ_FAILED', 'LOCAL_MCP_PENDING',
            'LOCAL_MCP_LISTENER_MISSING', 'LOCAL_MCP_LISTENER_IDENTITY_MISMATCH',
            'LOCAL_MCP_LAUNCHED_PROCESS_MISMATCH', 'LOCAL_MCP_RESPONSE_TIMEOUT',
            'LOCAL_MCP_RESPONSE_UNAVAILABLE', 'LOCAL_MCP_RESPONSE_INVALID',
            'LOCAL_MCP_PROTOCOL_UNREADY'
        )
        if ($state -eq 'TRANSPORT_VERIFICATION_FAILED') {
            if ($gate -notin $allowedGates) { return $null }
        } elseif ($gate) {
            return $null
        }
        if ($source -and $source -notin @('LAST_KNOWN_GOOD', 'INTERRUPTED_PROMOTION')) { return $null }
        return [pscustomobject]@{ State = $state; RecoverySource = $source; Gate = $gate }
    } catch {
        return $null
    }
}

function Get-LifecycleRecoveryAssessment {
    $raw = @(Invoke-LifecycleSeam -Name 'RecoveryReadiness' -Arguments @($enginePath, $Workspace, $ConfigPath, $ExpectedBuildSha256, $BuildFingerprintPath, $ReadyTimeoutSeconds) -Default {
        param($engine, $root, $config, $expected, $manifest, $timeout)
        # The engine's recover plan is limited to durable release authority
        # assessment before it can inspect runtime/tunnel state or launch work.
        & $engine -Mode recover -Workspace $root -ConfigPath $config -ExpectedBuildSha256 $expected -BuildFingerprintPath $manifest -ReadyTimeoutSeconds $timeout
    })
    return Get-LifecycleRecoveryResult -Raw $raw
}

function Invoke-InstallPreparation {
    $canonical = Get-LifecycleCanonicalIdentity
    $client = Invoke-LifecycleSeam -Name 'TunnelClient' -Arguments @() -Default { Find-TunnelClient -ExplicitPath '' }
    if (-not $client) {
        $setup = Join-Path $PSScriptRoot 'scripts\setup-secure-mcp.ps1'
        Invoke-LifecycleSeam -Name 'InstallTunnelClient' -Arguments @($setup) -Default {
            param($scriptPath)
            if (-not (Test-Path -LiteralPath $scriptPath -PathType Leaf)) { throw 'secure MCP setup helper is unavailable' }
            & powershell -NoProfile -ExecutionPolicy Bypass -File $scriptPath -Mode install | Out-Null
            if ($LASTEXITCODE -ne 0) { throw 'tunnel client preparation did not complete' }
        }
    }
    Invoke-LifecycleSeam -Name 'CodexPrerequisites' -Arguments @() -Default { Test-CodexOnDemandPrerequisites }
    $wakeReady = Invoke-LifecycleSeam -Name 'WakeRuntime' -Arguments @($Workspace) -Default { param($root) Test-WakeBridgeRuntime -Root $root }
    if (-not $wakeReady) {
        Invoke-LifecycleSeam -Name 'WakeRepair' -Arguments @($Workspace) -Default { param($root) Invoke-WakeBridgeRuntimeRepair -Root $root }
        $wakeReady = Invoke-LifecycleSeam -Name 'WakeRuntime' -Arguments @($Workspace) -Default { param($root) Test-WakeBridgeRuntime -Root $root }
        if (-not $wakeReady) { throw 'wake runtime preparation did not complete' }
    }
    if (-not $canonical.Path -or -not $canonical.Sha256) { throw 'canonical release identity is unavailable' }
    Write-LifecycleStatus 'INSTALL_READY' 'operator authentication and connector binding may still require attention'
}

function Get-NonMutatingStatus {
    # Preserve a fixed, non-secret stage code so a consumer can distinguish
    # which bounded status gate threw without projecting exception text,
    # configured endpoints, tunnel identity, credentials, or filesystem paths.
    $statusStage = 'CANONICAL_IDENTITY'
    try {
        $canonical = Get-LifecycleCanonicalIdentity
        $statusStage = 'LOCAL_MCP_CONFIG'
        $local = Get-LifecycleLocalMcp
        $statusStage = 'LOCAL_MCP_READINESS'
        $localReady = [bool](Test-LifecycleLocalMcpReadiness -LocalMcp $local -Canonical $canonical)
        $statusStage = 'OFFICIAL_RUNTIME'
        $runtimeReady = Invoke-LifecycleSeam -Name 'RuntimeStatus' -Arguments @($Workspace, $ConfigPath) -Default {
            param($root, $config)
            $alias = Get-ConfiguredRuntimeAlias -Path $config
            $client = Find-TunnelClient -ExplicitPath ''
            if (-not $client) { return $false }
            Test-OfficialRuntimeVerified -Root $root -TimeoutSeconds 5 -Alias $alias -ClientPath $client
        }
        if ($localReady -and $runtimeReady) { Write-LifecycleStatus 'READY' 'local daemon and external runtime verified'; return }
        if ($localReady) { Write-LifecycleStatus 'LOCAL_READY_EXTERNAL_RUNTIME_PENDING'; return }
        Write-LifecycleStatus 'LOCAL_DAEMON_PENDING'
    } catch {
        $detailStage = $statusStage
        if ($statusStage -eq 'CANONICAL_IDENTITY') {
            $assessment = Get-LifecycleRecoveryAssessment
            if ($null -ne $assessment) {
                switch ($assessment.State) {
                    'RECOVERY_READY' {
                        $detail = if ($assessment.RecoverySource) { "source=$($assessment.RecoverySource)" } else { 'redacted' }
                        Write-LifecycleStatus 'RECOVERY_AVAILABLE' $detail
                        return
                    }
                    'LKG_AUTHORITY_MISSING' { Write-LifecycleStatus 'LKG_AUTHORITY_MISSING'; return }
                    'LKG_AUTHORITY_AMBIGUOUS_OR_DAMAGED' { Write-LifecycleStatus 'LKG_AUTHORITY_AMBIGUOUS_OR_DAMAGED'; return }
                    'RECOVERY_RELEASE_AUTHORITY_REQUIRED' { Write-LifecycleStatus 'RECOVERY_AUTHORITY_REQUIRED'; return }
                }
            }
            $detailStage = switch ([string]$_.Exception.Message) {
                'CATDESK_CANONICAL_WORKSPACE_RESOLVE' { 'CANONICAL_WORKSPACE_RESOLVE' }
                'CATDESK_CANONICAL_BINARY_MISSING' { 'CANONICAL_BINARY_MISSING' }
                'CATDESK_CANONICAL_FINGERPRINT_MISSING' { 'CANONICAL_FINGERPRINT_MISSING' }
                'CATDESK_CANONICAL_FINGERPRINT_INVALID' { 'CANONICAL_FINGERPRINT_INVALID' }
                'CATDESK_CANONICAL_HASH_MISMATCH' { 'CANONICAL_HASH_MISMATCH' }
                'CATDESK_CANONICAL_IDENTITY_INTERNAL' { 'CANONICAL_IDENTITY_INTERNAL' }
                default { 'CANONICAL_IDENTITY' }
            }
        }
        Write-LifecycleStatus 'STATUS_UNAVAILABLE' "stage=$detailStage"
    }
}

function Invoke-LayeredDiagnosis {
    $layers = [Collections.Generic.List[object]]::new()
    [void]$layers.Add((New-LifecycleDiagnosticLayer -Layer 'LIFECYCLE_ENGINE' -State 'READY' -Gate 'READY'))

    $canonical = $null
    try {
        $canonical = Get-LifecycleCanonicalIdentity
        [void]$layers.Add((New-LifecycleDiagnosticLayer -Layer 'CANONICAL_RELEASE' -State 'READY' -Gate 'READY'))
        [void]$layers.Add((New-LifecycleDiagnosticLayer -Layer 'RECOVERY_AUTHORITY' -State 'NOT_REQUIRED' -Gate 'NOT_REQUIRED'))
    } catch {
        $canonicalGate = Get-LifecycleCanonicalFailureGate -Message ([string]$_.Exception.Message)
        [void]$layers.Add((New-LifecycleDiagnosticLayer -Layer 'CANONICAL_RELEASE' -State 'FAILED' -Gate $canonicalGate))
        $assessment = Get-LifecycleRecoveryAssessment
        if ($null -ne $assessment) {
            switch ($assessment.State) {
                'RECOVERY_READY' {
                    $authorityGate = if ($assessment.RecoverySource) { [string]$assessment.RecoverySource } else { 'RECOVERY_READY' }
                    [void]$layers.Add((New-LifecycleDiagnosticLayer -Layer 'RECOVERY_AUTHORITY' -State 'READY' -Gate $authorityGate))
                    Write-LifecycleDiagnosis -State 'RECOVERY_AVAILABLE' -PrimaryLayer 'CANONICAL_RELEASE' -NextAction 'RUN_RECOVER' -Layers $layers
                    return
                }
                'LKG_AUTHORITY_MISSING' {
                    [void]$layers.Add((New-LifecycleDiagnosticLayer -Layer 'RECOVERY_AUTHORITY' -State 'FAILED' -Gate 'LKG_AUTHORITY_MISSING'))
                    Write-LifecycleDiagnosis -State 'ACTION_REQUIRED' -PrimaryLayer 'RECOVERY_AUTHORITY' -NextAction 'OPERATOR_ATTENTION' -Layers $layers
                    return
                }
                'LKG_AUTHORITY_AMBIGUOUS_OR_DAMAGED' {
                    [void]$layers.Add((New-LifecycleDiagnosticLayer -Layer 'RECOVERY_AUTHORITY' -State 'FAILED' -Gate 'LKG_AUTHORITY_AMBIGUOUS_OR_DAMAGED'))
                    Write-LifecycleDiagnosis -State 'ACTION_REQUIRED' -PrimaryLayer 'RECOVERY_AUTHORITY' -NextAction 'OPERATOR_ATTENTION' -Layers $layers
                    return
                }
                'RECOVERY_RELEASE_AUTHORITY_REQUIRED' {
                    [void]$layers.Add((New-LifecycleDiagnosticLayer -Layer 'RECOVERY_AUTHORITY' -State 'FAILED' -Gate 'RECOVERY_AUTHORITY_REQUIRED'))
                    Write-LifecycleDiagnosis -State 'ACTION_REQUIRED' -PrimaryLayer 'RECOVERY_AUTHORITY' -NextAction 'OPERATOR_ATTENTION' -Layers $layers
                    return
                }
            }
        }
        [void]$layers.Add((New-LifecycleDiagnosticLayer -Layer 'RECOVERY_AUTHORITY' -State 'FAILED' -Gate 'RECOVERY_AUTHORITY_UNAVAILABLE'))
        Write-LifecycleDiagnosis -State 'ACTION_REQUIRED' -PrimaryLayer 'CANONICAL_RELEASE' -NextAction 'OPERATOR_ATTENTION' -Layers $layers
        return
    }

    $local = $null
    try {
        $local = Get-LifecycleLocalMcp
        [void]$layers.Add((New-LifecycleDiagnosticLayer -Layer 'LOCAL_MCP_CONFIG' -State 'READY' -Gate 'READY'))
    } catch {
        [void]$layers.Add((New-LifecycleDiagnosticLayer -Layer 'LOCAL_MCP_CONFIG' -State 'FAILED' -Gate 'LOCAL_MCP_CONFIG_UNAVAILABLE'))
        Write-LifecycleDiagnosis -State 'ACTION_REQUIRED' -PrimaryLayer 'LOCAL_MCP_CONFIG' -NextAction 'OPERATOR_ATTENTION' -Layers $layers
        return
    }

    $localDaemonReady = $false
    try {
        $listener = Get-LifecycleListener -LocalMcp $local -Canonical $canonical
        if ($null -eq $listener) {
            [void]$layers.Add((New-LifecycleDiagnosticLayer -Layer 'LOCAL_DAEMON' -State 'FAILED' -Gate 'LOCAL_MCP_LISTENER_MISSING'))
        } elseif (-not $listener.MatchesCanonical) {
            [void]$layers.Add((New-LifecycleDiagnosticLayer -Layer 'LOCAL_DAEMON' -State 'FAILED' -Gate 'LOCAL_MCP_LISTENER_IDENTITY_MISMATCH'))
        } else {
            $localDaemonReady = $true
            [void]$layers.Add((New-LifecycleDiagnosticLayer -Layer 'LOCAL_DAEMON' -State 'READY' -Gate 'READY'))
        }
    } catch {
        [void]$layers.Add((New-LifecycleDiagnosticLayer -Layer 'LOCAL_DAEMON' -State 'FAILED' -Gate 'LOCAL_MCP_LISTENER_UNAVAILABLE'))
    }

    $localProtocolReady = $false
    if ($localDaemonReady) {
        try {
            $mcp = Get-LifecycleLocalMcpReadinessResult -LocalMcp $local -Canonical $canonical
            $localProtocolReady = [bool]$mcp.Ready
            [void]$layers.Add((New-LifecycleDiagnosticLayer -Layer 'LOCAL_MCP_PROTOCOL' -State $(if ($mcp.Ready) { 'READY' } else { 'FAILED' }) -Gate ([string]$mcp.Gate)))
        } catch {
            [void]$layers.Add((New-LifecycleDiagnosticLayer -Layer 'LOCAL_MCP_PROTOCOL' -State 'FAILED' -Gate 'LOCAL_MCP_READINESS_INVALID'))
        }
    } else {
        [void]$layers.Add((New-LifecycleDiagnosticLayer -Layer 'LOCAL_MCP_PROTOCOL' -State 'NOT_REQUIRED' -Gate 'UPSTREAM_FAILED'))
    }

    $wakeReady = $false
    try {
        $wakeReady = [bool](Invoke-LifecycleSeam -Name 'WakeRuntime' -Arguments @($Workspace) -Default {
            param($root)
            Test-WakeBridgeRuntime -Root $root
        })
        [void]$layers.Add((New-LifecycleDiagnosticLayer -Layer 'WAKE_RUNTIME' -State $(if ($wakeReady) { 'READY' } else { 'FAILED' }) -Gate $(if ($wakeReady) { 'READY' } else { 'WAKE_RUNTIME_NOT_READY' })))
    } catch {
        [void]$layers.Add((New-LifecycleDiagnosticLayer -Layer 'WAKE_RUNTIME' -State 'FAILED' -Gate 'WAKE_RUNTIME_UNAVAILABLE'))
    }

    $runtimeReady = $false
    $runtimeGate = 'RUNTIME_STATUS_TRANSIENT'
    try {
        $runtime = Get-LifecycleOfficialRuntimeVerificationResult
        $runtimeReady = [bool]$runtime.Verified
        $runtimeGate = [string]$runtime.Gate
        [void]$layers.Add((New-LifecycleDiagnosticLayer -Layer 'OFFICIAL_RUNTIME' -State $(if ($runtimeReady) { 'READY' } else { 'FAILED' }) -Gate $runtimeGate))
    } catch {
        $runtimeGate = 'RUNTIME_VERIFICATION_UNAVAILABLE'
        [void]$layers.Add((New-LifecycleDiagnosticLayer -Layer 'OFFICIAL_RUNTIME' -State 'FAILED' -Gate $runtimeGate))
    }

    if (-not $localDaemonReady) {
        Write-LifecycleDiagnosis -State 'DEGRADED' -PrimaryLayer 'LOCAL_DAEMON' -NextAction 'RUN_RECOVER' -Layers $layers
        return
    }
    if (-not $localProtocolReady) {
        Write-LifecycleDiagnosis -State 'DEGRADED' -PrimaryLayer 'LOCAL_MCP_PROTOCOL' -NextAction 'RUN_RECOVER' -Layers $layers
        return
    }
    if (-not $wakeReady) {
        Write-LifecycleDiagnosis -State 'DEGRADED' -PrimaryLayer 'WAKE_RUNTIME' -NextAction 'RUN_RECOVER' -Layers $layers
        return
    }
    if (-not $runtimeReady) {
        $next = if ($runtimeGate -eq 'RUNTIME_CLIENT_UNAVAILABLE') { 'RUN_INSTALL' } elseif ($runtimeGate -eq 'RUNTIME_VERIFICATION_UNAVAILABLE') { 'OPERATOR_ATTENTION' } else { 'RUN_RECOVER' }
        Write-LifecycleDiagnosis -State 'DEGRADED' -PrimaryLayer 'OFFICIAL_RUNTIME' -NextAction $next -Layers $layers
        return
    }

    Write-LifecycleDiagnosis -State 'HEALTHY' -PrimaryLayer 'NONE' -NextAction 'NONE' -Layers $layers
}

function Invoke-CatDeskOnlyStop {
    try {
        $canonical = Get-LifecycleCanonicalIdentity
        $local = Get-LifecycleLocalMcp
        $listener = Get-LifecycleListener -LocalMcp $local -Canonical $canonical
        if ($null -eq $listener) { Write-LifecycleStatus 'STOPPED' 'no local CatDesk listener'; return }
        if (-not $listener.MatchesCanonical -or -not $listener.PSObject.Properties['Pid'] -or [int]$listener.Pid -lt 1) {
            Write-LifecycleStatus 'STOP_REFUSED'; return
        }
        $stopped = Invoke-LifecycleSeam -Name 'StopProcess' -Arguments @($local, $canonical, $listener) -Default {
            param($endpoint, $identity, $candidate)
            Stop-CatDeskListenerProcessForRecovery -LocalMcp $endpoint -Canonical $identity -Candidate $candidate -FailureMessage 'CatDesk listener did not stop within the bounded lifecycle timeout'
        }
        if (-not $stopped) { Write-LifecycleStatus 'STOPPED' 'verified local CatDesk daemon already exited'; return }
        Write-LifecycleStatus 'STOPPED' 'verified local CatDesk daemon only'
    } catch {
        Write-LifecycleStatus 'STOP_REFUSED'
    }
}

try {
    switch ($Command) {
        'install' { Invoke-InstallPreparation }
        'start' { Invoke-CanonicalRecovery -RequestedCommand 'start' }
        'recover' { Invoke-CanonicalRecovery -RequestedCommand 'recover' }
        'status' { Get-NonMutatingStatus }
        'diagnose' { Invoke-LayeredDiagnosis }
        'stop' { Invoke-CatDeskOnlyStop }
        'autostart' { Invoke-AutostartLifecycle }
        'wake' { Invoke-WakePolicyLifecycle }
    }
} catch {
    Write-LifecycleStatus 'ACTION_REQUIRED'
}

# The public lifecycle contract carries operational failure through bounded JSON
# states rather than the host process exit status. Normalize successful contract
# emission to zero so direct `powershell.exe -File catdesk.ps1 ...` callers do
# not mistake a caught lifecycle condition (or stale native LASTEXITCODE) for a
# transport/process failure.
exit 0
