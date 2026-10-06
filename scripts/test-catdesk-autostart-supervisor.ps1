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
        else { 'TRANSPORT_VERIFICATION_FAILED' }
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
[pscustomobject]@{ state = $state } | ConvertTo-Json -Compress
'@
    [IO.File]::WriteAllText((Join-Path $workspace 'catdesk.ps1'), $fakeFacade, [Text.UTF8Encoding]::new($false))
    [IO.File]::WriteAllLines((Join-Path $workspace 'states.txt'), [string[]]@('LOCAL_READY_EXTERNAL_RUNTIME_PENDING', 'TRANSPORT_VERIFICATION_FAILED', 'READY'), [Text.UTF8Encoding]::new($false))
    $publicFacadePending = States (Invoke-CatDeskAutostartSupervisor -InitialDelaySeconds 0 -MaximumAttempts 3 -InitialBackoffSeconds 2 -MaximumBackoffSeconds 3 -HealthPollSeconds 15 -FailureCooldownSeconds 30 -MaximumMonitorCycles 2)
    Require (($publicFacadePending -join ',') -eq 'EXTERNAL_RUNTIME_PENDING,READY,MONITORING_COMPLETE') 'default public facade converges an external-runtime pending state without direct control seams'
    $publicCalls = @([IO.File]::ReadAllLines((Join-Path $workspace 'calls.log')))
    $expectedWorkspace = (Resolve-Path -LiteralPath $workspace).Path
    Require (($publicCalls -join ',') -eq "status|$expectedWorkspace,recover|$expectedWorkspace,status|$expectedWorkspace") 'default public facade receives only status/recover and the resolved workspace identity'
    Require (([string]::Join(',', $delays)) -eq '15') 'default public facade transport pending uses ordinary health polling'

    $calls.Clear(); $delays.Clear()
    $script:SupervisorSeams = @{}
    $script:SupervisorSeams['AcquireGuard'] = { param($root) [pscustomobject]@{ Acquired = $true; Handle = $null } }
    $script:SupervisorSeams['Sleep'] = { param($seconds) $delays.Add([int]$seconds) }
    $script:statusIndex = 0
    $script:SupervisorSeams['Lifecycle'] = {
        param($root, $action)
        [void]$calls.Add($action)
        if ($action -eq 'status') { $script:statusIndex++; if ($script:statusIndex -eq 1) { 'LOCAL_READY_EXTERNAL_RUNTIME_PENDING' } else { 'READY' } }
        else { 'ACTION_REQUIRED' }
    }
    $cooldown = States (Invoke-CatDeskAutostartSupervisor -InitialDelaySeconds 0 -MaximumAttempts 3 -InitialBackoffSeconds 2 -MaximumBackoffSeconds 3 -HealthPollSeconds 15 -FailureCooldownSeconds 30 -MaximumMonitorCycles 2)
    Require (($cooldown -join ',') -eq 'DEGRADED,READY,MONITORING_COMPLETE') 'exhausted burst cools down then later monitoring can recover'
    Require (([string]::Join(',', $calls)) -eq 'status,recover,recover,recover,status') 'exhausted burst is bounded and next cycle resumes public status'
    Require (([string]::Join(',', $delays)) -eq '2,3,30') 'backoff and cooldown are bounded and deterministic'

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
