[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$workspace = Join-Path ([IO.Path]::GetTempPath()) ('catdesk-autostart-fixture-' + [Guid]::NewGuid().ToString('N'))
$supervisorPath = Join-Path $PSScriptRoot 'catdesk-autostart-supervisor.ps1'
$source = Get-Content -LiteralPath $supervisorPath -Raw
$tokens = $null; $errors = $null
[void][System.Management.Automation.Language.Parser]::ParseInput($source, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'autostart supervisor did not parse' }

function Require([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw "assertion failed: $Message" }
}

function States($Output) { return @($Output | ForEach-Object { ($_ | ConvertFrom-Json).state }) }

New-Item -ItemType Directory -Path $workspace -Force | Out-Null
try {
    . $supervisorPath -Workspace $workspace -LoadOnly
    $script:SupervisorSeams = @{}
    $calls = [Collections.Generic.List[string]]::new()
    $delays = [Collections.Generic.List[int]]::new()
    $script:SupervisorSeams['AcquireGuard'] = { param($root) [pscustomobject]@{ Acquired = $true; Handle = $null } }
    $script:SupervisorSeams['Sleep'] = { param($seconds) $delays.Add([int]$seconds) }

    $script:SupervisorSeams['Lifecycle'] = { param($root, $action) [void]$calls.Add($action); 'READY' }
    $healthy = States (Invoke-CatDeskAutostartSupervisor -InitialDelaySeconds 0 -MaximumAttempts 2 -InitialBackoffSeconds 1 -MaximumBackoffSeconds 4 -HealthPollSeconds 15 -FailureCooldownSeconds 30 -MaximumMonitorCycles 3)
    Require (($healthy -join ',') -eq 'READY,READY,READY,MONITORING_COMPLETE') 'healthy supervisor continues polling rather than exiting after first READY'
    Require (([string]::Join(',', $calls)) -eq 'status,status,status') 'continuously healthy monitor never calls recover'
    Require (([string]::Join(',', $delays)) -eq '15,15') 'healthy monitor uses low-frequency bounded polling'

    $calls.Clear(); $delays.Clear()
    $script:SupervisorSeams['AcquireGuard'] = { param($root) [pscustomobject]@{ Acquired = $false; Handle = $null } }
    $duplicate = States (Invoke-CatDeskAutostartSupervisor -InitialDelaySeconds 0 -MaximumAttempts 2 -InitialBackoffSeconds 1 -MaximumBackoffSeconds 4 -HealthPollSeconds 15 -FailureCooldownSeconds 30 -MaximumMonitorCycles 1)
    Require (($duplicate -join ',') -eq 'SUPERVISOR_ALREADY_RUNNING') 'singleton remains effective for the entire monitor lifecycle'
    Require ($calls.Count -eq 0 -and $delays.Count -eq 0) 'duplicate supervisor makes no lifecycle call or sleep'

    $calls.Clear(); $delays.Clear(); $script:SupervisorSeams['AcquireGuard'] = { param($root) [pscustomobject]@{ Acquired = $true; Handle = $null } }
    $script:statusIndex = 0
    $script:SupervisorSeams['Lifecycle'] = {
        param($root, $action)
        [void]$calls.Add($action)
        if ($action -eq 'status') { $script:statusIndex++; if ($script:statusIndex -eq 1) { 'READY' } elseif ($script:statusIndex -eq 2) { 'LOCAL_DAEMON_PENDING' } else { 'READY' } }
        else { 'CONNECTED_VERIFIED' }
    }
    $transition = States (Invoke-CatDeskAutostartSupervisor -InitialDelaySeconds 0 -MaximumAttempts 2 -InitialBackoffSeconds 1 -MaximumBackoffSeconds 4 -HealthPollSeconds 15 -FailureCooldownSeconds 30 -MaximumMonitorCycles 3)
    Require (($transition -join ',') -eq 'READY,RECOVERED,READY,MONITORING_COMPLETE') 'later healthy-to-unhealthy transition recovers and returns to monitoring'
    Require (([string]::Join(',', $calls)) -eq 'status,status,recover,status') 'unhealthy transition invokes public recover only'
    Require (([string]::Join(',', $delays)) -eq '15,15') 'successful recovery resumes bounded health polling'

    $calls.Clear(); $delays.Clear()
    $script:statusIndex = 0
    $script:SupervisorSeams['Lifecycle'] = {
        param($root, $action)
        [void]$calls.Add($action)
        if ($action -eq 'status') { $script:statusIndex++; if ($script:statusIndex -eq 1) { 'LOCAL_READY_EXTERNAL_RUNTIME_PENDING' } else { 'READY' } }
        else { 'EXTERNAL_RUNTIME_VERIFICATION_FAILED' }
    }
    $externalPending = States (Invoke-CatDeskAutostartSupervisor -InitialDelaySeconds 0 -MaximumAttempts 3 -InitialBackoffSeconds 2 -MaximumBackoffSeconds 3 -HealthPollSeconds 15 -FailureCooldownSeconds 30 -MaximumMonitorCycles 2)
    Require (($externalPending -join ',') -eq 'EXTERNAL_RUNTIME_PENDING,READY,MONITORING_COMPLETE') 'bounded transport verification failure waits for the external runtime instead of cycling local recovery'
    Require (([string]::Join(',', $calls)) -eq 'status,recover,status') 'transport verification failure performs one public recovery invocation before resuming status polling'
    Require (([string]::Join(',', $delays)) -eq '15') 'external runtime pending uses normal health polling rather than recovery backoff or cooldown'

    # Exercise the default public facade invocation rather than the Lifecycle
    # seam. This proves the supervisor accepts the public redacted JSON state
    # and does not need a direct daemon or tunnel control path to converge.
    $calls.Clear(); $delays.Clear()
    $script:SupervisorSeams = @{}
    $script:SupervisorSeams['AcquireGuard'] = { param($root) [pscustomobject]@{ Acquired = $true; Handle = $null } }
    $script:SupervisorSeams['Sleep'] = { param($seconds) $delays.Add([int]$seconds) }
    $fakeFacade = @'
[CmdletBinding()]
param(
    [Parameter(Position = 0, Mandatory = $true)]
    [ValidateSet('status', 'recover')]
    [string]$Action,
    [string]$Workspace
)
$root = $PSScriptRoot
[IO.File]::AppendAllText((Join-Path $root 'calls.log'), "$Action|$Workspace`n", [Text.UTF8Encoding]::new($false))
$queuePath = Join-Path $root 'states.txt'
$queue = @([IO.File]::ReadAllLines($queuePath))
if ($queue.Count -lt 1 -or -not $queue[0]) { throw 'fake lifecycle queue is empty' }
$state = $queue[0]
if ($queue.Count -gt 1) {
    [IO.File]::WriteAllLines($queuePath, [string[]]$queue[1..($queue.Count - 1)], [Text.UTF8Encoding]::new($false))
} else {
    [IO.File]::WriteAllText($queuePath, '', [Text.UTF8Encoding]::new($false))
}
if ($state.StartsWith('{', [StringComparison]::Ordinal)) { $state; return }
$parts = $state.Split('|', 2)
$detail = if ($parts.Count -eq 2) { $parts[1] } else { 'redacted' }
[pscustomobject]@{ command = $Action; state = $parts[0]; detail = $detail } | ConvertTo-Json -Compress
'@
    [IO.File]::WriteAllText((Join-Path $workspace 'catdesk.ps1'), $fakeFacade, [Text.UTF8Encoding]::new($false))
    [IO.File]::WriteAllLines((Join-Path $workspace 'states.txt'), [string[]]@('LOCAL_READY_EXTERNAL_RUNTIME_PENDING', 'TRANSPORT_VERIFICATION_FAILED|gate=RUNTIME_STATUS_TIMEOUT', 'READY'), [Text.UTF8Encoding]::new($false))
    $publicFacadePending = States (Invoke-CatDeskAutostartSupervisor -InitialDelaySeconds 0 -MaximumAttempts 3 -InitialBackoffSeconds 2 -MaximumBackoffSeconds 3 -HealthPollSeconds 15 -FailureCooldownSeconds 30 -MaximumMonitorCycles 2)
    Require (($publicFacadePending -join ',') -eq 'EXTERNAL_RUNTIME_PENDING,READY,MONITORING_COMPLETE') 'default public facade converges an external-runtime pending state without direct control seams'
    $publicCalls = @([IO.File]::ReadAllLines((Join-Path $workspace 'calls.log')))
    $expectedWorkspace = (Resolve-Path -LiteralPath $workspace).Path
    Require (($publicCalls -join ',') -eq "status|$expectedWorkspace,recover|$expectedWorkspace,status|$expectedWorkspace") 'default public facade receives only status/recover and the resolved workspace identity'
    Require (([string]::Join(',', $delays)) -eq '15') 'default public facade transport pending uses ordinary health polling'

    $calls.Clear(); $delays.Clear()
    [IO.File]::WriteAllText((Join-Path $workspace 'calls.log'), '')
    [IO.File]::WriteAllLines((Join-Path $workspace 'states.txt'), [string[]]@('CORE_CONNECTED_WAKE_PENDING|gate=WAKE_RUNTIME_NOT_READY', 'READY'), [Text.UTF8Encoding]::new($false))
    $wakePending = States (Invoke-CatDeskAutostartSupervisor -InitialDelaySeconds 0 -MaximumAttempts 3 -InitialBackoffSeconds 2 -MaximumBackoffSeconds 3 -HealthPollSeconds 15 -FailureCooldownSeconds 30 -MaximumMonitorCycles 2)
    Require (($wakePending -join ',') -eq 'WAKE_PENDING,READY,MONITORING_COMPLETE') 'verified core with unavailable Wake remains a separate degraded supervisor state'
    $wakeCalls = @([IO.File]::ReadAllLines((Join-Path $workspace 'calls.log')))
    Require (($wakeCalls -join ',') -eq "status|$expectedWorkspace,status|$expectedWorkspace") 'Wake-only status does not cause repeated core recovery'
    Require (([string]::Join(',', $delays)) -eq '15') 'Wake-only status uses ordinary health polling'

    $calls.Clear(); $delays.Clear()
    [IO.File]::WriteAllText((Join-Path $workspace 'calls.log'), '')
    [IO.File]::WriteAllLines((Join-Path $workspace 'states.txt'), [string[]]@('LOCAL_DAEMON_PENDING', 'CORE_CONNECTED_WAKE_PENDING|gate=WAKE_RUNTIME_NOT_READY', 'READY'), [Text.UTF8Encoding]::new($false))
    $wakeRecovered = States (Invoke-CatDeskAutostartSupervisor -InitialDelaySeconds 0 -MaximumAttempts 3 -InitialBackoffSeconds 2 -MaximumBackoffSeconds 3 -HealthPollSeconds 15 -FailureCooldownSeconds 30 -MaximumMonitorCycles 2)
    Require (($wakeRecovered -join ',') -eq 'WAKE_PENDING,READY,MONITORING_COMPLETE') 'partial core recovery never becomes RECOVERED or READY while Wake is unavailable'
    $wakeRecoveryCalls = @([IO.File]::ReadAllLines((Join-Path $workspace 'calls.log')))
    Require (($wakeRecoveryCalls -join ',') -eq "status|$expectedWorkspace,recover|$expectedWorkspace,status|$expectedWorkspace") 'partial Wake recovery ends the burst after one public recover call'
    Require (([string]::Join(',', $delays)) -eq '15') 'partial Wake recovery does not back off or enter degraded cooldown'

    $calls.Clear(); $delays.Clear()
    [IO.File]::WriteAllText((Join-Path $workspace 'calls.log'), '')
    [IO.File]::WriteAllLines((Join-Path $workspace 'states.txt'), [string[]]@('LOCAL_DAEMON_PENDING', 'TRANSPORT_VERIFICATION_FAILED|gate=LOCAL_MCP_RESPONSE_TIMEOUT', 'READY'), [Text.UTF8Encoding]::new($false))
    $localFailure = States (Invoke-CatDeskAutostartSupervisor -InitialDelaySeconds 0 -MaximumAttempts 3 -InitialBackoffSeconds 2 -MaximumBackoffSeconds 3 -HealthPollSeconds 15 -FailureCooldownSeconds 30 -MaximumMonitorCycles 2)
    Require (($localFailure -join ',') -eq 'DEGRADED,READY,MONITORING_COMPLETE') 'local MCP recovery failure does not become external runtime pending'
    $localCalls = @([IO.File]::ReadAllLines((Join-Path $workspace 'calls.log')))
    Require (($localCalls -join ',') -eq "status|$expectedWorkspace,recover|$expectedWorkspace,status|$expectedWorkspace") 'local MCP failure performs one bounded public recovery then returns to status polling'
    Require (([string]::Join(',', $delays)) -eq '30') 'local MCP recovery failure uses degraded cooldown rather than external runtime health poll'

    foreach ($invalidDetail in @('redacted', 'gate=RUNTIME_STATUS_TIMEOUT extra', 'gate=runtime_status_timeout', 'gate=https://example.invalid/secret')) {
        [IO.File]::WriteAllText((Join-Path $workspace 'states.txt'), "TRANSPORT_VERIFICATION_FAILED|$invalidDetail", [Text.UTF8Encoding]::new($false))
        Require ((Invoke-PublicLifecycle -Root $expectedWorkspace -Action recover) -eq 'ACTION_REQUIRED') 'unrecognized recovery gate fails closed'
    }
    foreach ($malformedJson in @(
        '{"command":"recover","state":"TRANSPORT_VERIFICATION_FAILED","detail":["gate=RUNTIME_STATUS_TIMEOUT"]}',
        '{"command":"recover","state":"TRANSPORT_VERIFICATION_FAILED","detail":{"value":"gate=RUNTIME_STATUS_TIMEOUT"}}',
        '{"command":"recover","state":"TRANSPORT_VERIFICATION_FAILED","detail":1}',
        '{"command":"recover","state":"TRANSPORT_VERIFICATION_FAILED","detail":null}',
        '{"command":"recover","state":["TRANSPORT_VERIFICATION_FAILED"],"detail":"gate=RUNTIME_STATUS_TIMEOUT"}'
    )) {
        [IO.File]::WriteAllText((Join-Path $workspace 'states.txt'), $malformedJson, [Text.UTF8Encoding]::new($false))
        Require ((Invoke-PublicLifecycle -Root $expectedWorkspace -Action recover) -eq 'ACTION_REQUIRED') 'non-scalar lifecycle state or recovery detail fails closed'
    }
    foreach ($misboundJson in @(
        '{"state":"TRANSPORT_VERIFICATION_FAILED","detail":"gate=RUNTIME_STATUS_TIMEOUT"}',
        '{"command":"status","state":"TRANSPORT_VERIFICATION_FAILED","detail":"gate=RUNTIME_STATUS_TIMEOUT"}',
        '{"command":["recover"],"state":"TRANSPORT_VERIFICATION_FAILED","detail":"gate=RUNTIME_STATUS_TIMEOUT"}',
        '{"command":null,"state":"TRANSPORT_VERIFICATION_FAILED","detail":"gate=RUNTIME_STATUS_TIMEOUT"}'
    )) {
        [IO.File]::WriteAllText((Join-Path $workspace 'states.txt'), $misboundJson, [Text.UTF8Encoding]::new($false))
        Require ((Invoke-PublicLifecycle -Root $expectedWorkspace -Action recover) -eq 'ACTION_REQUIRED') 'missing, mismatched or non-scalar recovery command fails closed'
    }
    foreach ($invalidWakeJson in @(
        '{"command":"status","state":"CORE_CONNECTED_WAKE_PENDING","detail":"redacted"}',
        '{"command":"status","state":"CORE_CONNECTED_WAKE_PENDING","detail":["gate=WAKE_RUNTIME_NOT_READY"]}',
        '{"command":"recover","state":"CORE_CONNECTED_WAKE_PENDING","detail":{"gate":"WAKE_RUNTIME_NOT_READY"}}',
        '{"command":"recover","state":"CORE_CONNECTED_WAKE_PENDING","detail":"gate=RUNTIME_STATUS_TIMEOUT"}'
    )) {
        [IO.File]::WriteAllText((Join-Path $workspace 'states.txt'), $invalidWakeJson, [Text.UTF8Encoding]::new($false))
        $wakeAction = if ($invalidWakeJson -match '"command":"status"') { 'status' } else { 'recover' }
        Require ((Invoke-PublicLifecycle -Root $expectedWorkspace -Action $wakeAction) -eq 'ACTION_REQUIRED') 'malformed or misclassified Wake gate fails closed'
    }
    [IO.File]::WriteAllText((Join-Path $workspace 'states.txt'), '{"command":"recover","state":"READY","detail":"redacted"}', [Text.UTF8Encoding]::new($false))
    Require ((Invoke-PublicLifecycle -Root $expectedWorkspace -Action status) -eq 'ACTION_REQUIRED') 'stale recover output cannot satisfy a status request'
    Require ((Get-SupervisorRecoveryGateClass -Detail 'gate=LOCAL_MCP_PENDING') -eq 'LOCAL_MCP_VERIFICATION_FAILED') 'local pending gate cannot be classified as external runtime failure'

    $delays.Clear()
    [IO.File]::WriteAllText((Join-Path $workspace 'calls.log'), '')
    [IO.File]::WriteAllLines((Join-Path $workspace 'states.txt'), [string[]]@('LOCAL_DAEMON_PENDING', 'TRANSPORT_VERIFICATION_FAILED|redacted', 'READY'), [Text.UTF8Encoding]::new($false))
    $unknownGate = States (Invoke-CatDeskAutostartSupervisor -InitialDelaySeconds 0 -MaximumAttempts 3 -InitialBackoffSeconds 2 -MaximumBackoffSeconds 3 -HealthPollSeconds 15 -FailureCooldownSeconds 30 -MaximumMonitorCycles 2)
    Require (($unknownGate -join ',') -eq 'DEGRADED,READY,MONITORING_COMPLETE') 'redacted transport gate stays action-required rather than external pending'
    $unknownCalls = @([IO.File]::ReadAllLines((Join-Path $workspace 'calls.log')))
    Require (($unknownCalls -join ',') -eq "status|$expectedWorkspace,recover|$expectedWorkspace,status|$expectedWorkspace") 'redacted transport gate does not trigger repeated recovery'
    Require (([string]::Join(',', $delays)) -eq '30') 'redacted transport gate uses degraded cooldown'

    $calls.Clear(); $delays.Clear()
    $script:SupervisorSeams = @{}
    $script:SupervisorSeams['AcquireGuard'] = { param($root) [pscustomobject]@{ Acquired = $true; Handle = $null } }
    $script:SupervisorSeams['Sleep'] = { param($seconds) $delays.Add([int]$seconds) }
    $script:statusIndex = 0
    $script:SupervisorSeams['Lifecycle'] = {
        param($root, $action)
        [void]$calls.Add($action)
        if ($action -eq 'status') { $script:statusIndex++; if ($script:statusIndex -eq 1) { 'LOCAL_READY_EXTERNAL_RUNTIME_PENDING' } else { 'READY' } }
        else { 'STATUS_UNAVAILABLE' }
    }
    $cooldown = States (Invoke-CatDeskAutostartSupervisor -InitialDelaySeconds 0 -MaximumAttempts 3 -InitialBackoffSeconds 2 -MaximumBackoffSeconds 3 -HealthPollSeconds 15 -FailureCooldownSeconds 30 -MaximumMonitorCycles 2)
    Require (($cooldown -join ',') -eq 'DEGRADED,READY,MONITORING_COMPLETE') 'exhausted burst cools down then later monitoring can recover'
    Require (([string]::Join(',', $calls)) -eq 'status,recover,recover,recover,status') 'transient unavailable recovery retries are bounded and later status polling resumes'
    Require (([string]::Join(',', $delays)) -eq '2,3,30') 'retry backoff and degraded cooldown are bounded and deterministic'

    $calls.Clear(); $delays.Clear(); $script:statusIndex = 0
    $script:SupervisorSeams['Lifecycle'] = {
        param($root, $action)
        [void]$calls.Add($action)
        if ($action -eq 'status') { $script:statusIndex++; if ($script:statusIndex -eq 1) { 'LOCAL_DAEMON_PENDING' } else { 'READY' } }
        else { 'ACTION_REQUIRED' }
    }
    $actionRequired = States (Invoke-CatDeskAutostartSupervisor -InitialDelaySeconds 0 -MaximumAttempts 3 -InitialBackoffSeconds 2 -MaximumBackoffSeconds 3 -HealthPollSeconds 15 -FailureCooldownSeconds 30 -MaximumMonitorCycles 2)
    Require (($actionRequired -join ',') -eq 'DEGRADED,READY,MONITORING_COMPLETE') 'action-required recovery remains degraded'
    Require (([string]::Join(',', $calls)) -eq 'status,recover,status') 'action-required recovery stops without repeated mutation'
    Require (([string]::Join(',', $delays)) -eq '30') 'action-required recovery uses degraded cooldown'

    try { Invoke-CatDeskAutostartSupervisor -InitialDelaySeconds 0 -MaximumAttempts 1 -InitialBackoffSeconds 1 -MaximumBackoffSeconds 1 -HealthPollSeconds 1 -FailureCooldownSeconds 30 -MaximumMonitorCycles 1 | Out-Null; throw 'invalid low poll accepted' } catch { if ($_.Exception.Message -match 'invalid low poll accepted') { throw } }
    try { Invoke-CatDeskAutostartSupervisor -InitialDelaySeconds 0 -MaximumAttempts 1 -InitialBackoffSeconds 1 -MaximumBackoffSeconds 1 -HealthPollSeconds 15 -FailureCooldownSeconds 1 -MaximumMonitorCycles 1 | Out-Null; throw 'invalid low cooldown accepted' } catch { if ($_.Exception.Message -match 'invalid low cooldown accepted') { throw } }
    $resolverIndex = $source.IndexOf('function Resolve-TrustedLifecycleFacadePath', [StringComparison]::Ordinal)
    $invokeIndex = $source.IndexOf('$raw = @(& $facade', [StringComparison]::Ordinal)
    Require ($resolverIndex -ge 0 -and $invokeIndex -gt $resolverIndex) 'trusted lifecycle facade resolver is defined before any public facade invocation'
    Require ($source -match [regex]::Escape('Get-Item -LiteralPath $expected -Force -ErrorAction Stop')) 'public lifecycle facade identity requires Get-Item of the exact expected path'
    Require ($source -match [regex]::Escape('[System.IO.FileInfo]')) 'public lifecycle facade identity requires a real FileInfo'
    Require ($source -match [regex]::Escape('[System.IO.FileAttributes]::ReparsePoint')) 'public lifecycle facade identity rejects reparse points'
    Require ($source -match [regex]::Escape('[System.IO.Path]::GetFullPath($item.FullName)')) 'public lifecycle facade identity normalizes the observed path'
    Require ($source -match [regex]::Escape('[System.StringComparison]::OrdinalIgnoreCase')) 'public lifecycle facade identity uses exact Windows path comparison'
    Require ($source -notmatch [regex]::Escape("Test-Path -LiteralPath `$facade -PathType Leaf")) 'public lifecycle facade cannot regress to leaf-existence-only trust'
    Require ($source -notmatch '(?i)cargo\s+(build|run|test)|provision-catdesk-release|tunnel-client.*(connect|stop|remove)|runtimes\s+(connect|stop|rm)|chrome|credential|CONTROL_PLANE_API_KEY') 'supervisor has no compiler, tunnel ownership, browser, or credential surface'
} finally {
    if (Test-Path -LiteralPath $workspace) { Remove-Item -LiteralPath $workspace -Recurse -Force }
}

Write-Output 'autostart supervisor fixture tests passed'
