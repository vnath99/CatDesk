[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$workspace = Join-Path ([IO.Path]::GetTempPath()) ('catdesk-acceptance-fixture-' + [Guid]::NewGuid().ToString('N'))
$checker = Join-Path $PSScriptRoot 'catdesk-production-acceptance.ps1'
$wakeTarget = 'https://chatgpt.com/c/fixture-conversation-1234'
$wakeHash = ([Security.Cryptography.SHA256]::Create().ComputeHash([Text.Encoding]::UTF8.GetBytes($wakeTarget)) | ForEach-Object { $_.ToString('x2') }) -join ''
$source = Get-Content -LiteralPath $checker -Raw
$tokens = $null; $errors = $null
[void][System.Management.Automation.Language.Parser]::ParseInput($source, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'production acceptance script did not parse' }

function Require([bool]$Condition, [string]$Message) { if (-not $Condition) { throw "assertion failed: $Message" } }
Require ($source -match '\[string\]\$Workspace = ''''' -and $source -match 'IsNullOrWhiteSpace\(\$Workspace\).*Join-Path \$PSScriptRoot') 'default workspace is resolved only after script-root initialization'
Require ($source -match 'Get-CatDeskDaemonProcessCandidates' -and $source -notmatch 'Get-Process -Name catdesk') 'canonical listener instance uses exact daemon identity rather than every same-name process'
function Gate($Snapshot, [string]$Id) { return @($Snapshot.gates | Where-Object { $_.id -eq $Id })[0].state }
function New-CompleteSnapshot([string]$Build, [string]$Instance, [string]$Lifecycle = 'READY', [hashtable]$GateStates = @{}) {
    $gateIds = @(
        'CANONICAL_RELEASE', 'PUBLIC_LIFECYCLE', 'LIFECYCLE_STATUS', 'AUTOSTART_OWNERSHIP',
        'PERSISTENT_SUPERVISOR', 'WAKE_RUNTIME_PRESENCE', 'WAKE_TARGET_BINDING',
        'RETENTION_EVIDENCE', 'EXTERNAL_RUNTIME_OWNERSHIP', 'CANONICAL_LISTENER_INSTANCE'
    )
    $gates = foreach ($id in $gateIds) {
        $default = if ($id -eq 'LIFECYCLE_STATUS' -and $Lifecycle -eq 'LOCAL_READY_EXTERNAL_RUNTIME_PENDING') { 'ATTENTION' } elseif ($id -eq 'LIFECYCLE_STATUS' -and $Lifecycle -ne 'READY') { 'FAIL' } else { 'PASS' }
        [pscustomobject][ordered]@{ id = $id; state = if ($GateStates.ContainsKey($id)) { $GateStates[$id] } else { $default } }
    }
    $states = @($gates | ForEach-Object { $_.state })
    [pscustomobject][ordered]@{
        schemaVersion = 1
        stage = 'PRE_REBOOT_PREFLIGHT'
        overallState = if ($states -contains 'FAIL') { 'FAIL' } elseif ($states -contains 'ATTENTION') { 'ATTENTION' } else { 'PASS' }
        capturedAtUtc = '2026-08-12T00:00:00.0000000Z'
        canonicalBuildFingerprint = $Build
        lifecycleState = $Lifecycle
        instanceFingerprint = $Instance
        gates = @($gates)
    }
}

New-Item -ItemType Directory -Path $workspace -Force | Out-Null
try {
    $base = @{
        CanonicalRelease = { param($root) 'a' * 64 }
        PublicFacade = { param($root) $true }
        LifecycleStatus = { param($root) 'READY' }
        AutostartStatus = { param($root) 'AUTOSTART_ENABLED' }
        PersistentAutostartSurface = { param($root) $true }
        WakeEvidence = { param($root, $expected) [pscustomobject]@{ RuntimePresent = $true; TargetBound = ($expected -eq $wakeHash) } }
        RetentionEvidence = { param($root) $true }
        OfficialRuntimeOwnership = { param($root, $config) $true }
        CanonicalListenerInstance = { param($root, $config, $hash) 'instance-pre' }
    }
    $healthy = & $checker -Workspace $workspace -ExpectedWakeTargetSha256 $wakeHash -TestSeams $base | ConvertFrom-Json
    Require ($healthy.schemaVersion -eq 1 -and $healthy.stage -eq 'PRE_REBOOT_PREFLIGHT' -and $healthy.overallState -eq 'PASS') 'healthy preflight produces a machine-readable PASS snapshot'
    Require ((Gate $healthy 'CANONICAL_RELEASE') -eq 'PASS' -and (Gate $healthy 'WAKE_RUNTIME_PRESENCE') -eq 'PASS' -and (Gate $healthy 'WAKE_TARGET_BINDING') -eq 'PASS') 'healthy release and measured wake gates pass'
    Require (($healthy | ConvertTo-Json -Depth 5) -notmatch '(?i)fixture-|secret|token|https?://|route_|tunnel') 'snapshot redacts fixture path, secret, URL, route, and tunnel material'

    $missingManifest = $base.Clone(); $missingManifest.CanonicalRelease = { param($root) $null }
    $missing = & $checker -Workspace $workspace -ExpectedWakeTargetSha256 $wakeHash -TestSeams $missingManifest | ConvertFrom-Json
    Require ($missing.overallState -eq 'FAIL' -and (Gate $missing 'CANONICAL_RELEASE') -eq 'FAIL') 'missing or invalid manifest fails canonical-release gate'
    $malformedTask = $base.Clone(); $malformedTask.AutostartStatus = { param($root) 'AUTOSTART_CONFLICT' }
    $taskFailure = & $checker -Workspace $workspace -ExpectedWakeTargetSha256 $wakeHash -TestSeams $malformedTask | ConvertFrom-Json
    Require ((Gate $taskFailure 'AUTOSTART_OWNERSHIP') -eq 'FAIL') 'autostart mismatch fails closed'
    $nonPersistent = $base.Clone(); $nonPersistent.PersistentAutostartSurface = { param($root) $false }
    $supervisorFailure = & $checker -Workspace $workspace -ExpectedWakeTargetSha256 $wakeHash -TestSeams $nonPersistent | ConvertFrom-Json
    Require ((Gate $supervisorFailure 'PERSISTENT_SUPERVISOR') -eq 'FAIL') 'non-persistent supervisor fails preflight'
    $absentWake = $base.Clone(); $absentWake.WakeEvidence = { param($root, $expected) [pscustomobject]@{ RuntimePresent = $false; TargetBound = $false } }
    $wakeFailure = & $checker -Workspace $workspace -ExpectedWakeTargetSha256 $wakeHash -TestSeams $absentWake | ConvertFrom-Json
    Require ((Gate $wakeFailure 'WAKE_RUNTIME_PRESENCE') -eq 'FAIL' -and (Gate $wakeFailure 'WAKE_TARGET_BINDING') -eq 'FAIL') 'absent wake prerequisites fail measured wake gates'
    $wrongWake = & $checker -Workspace $workspace -ExpectedWakeTargetSha256 ('0' * 64) -TestSeams $base | ConvertFrom-Json
    Require ((Gate $wrongWake 'WAKE_TARGET_BINDING') -eq 'FAIL') 'wrong wake fingerprint fails closed'
    $missingWakeHash = & $checker -Workspace $workspace -TestSeams $base | ConvertFrom-Json
    Require ((Gate $missingWakeHash 'WAKE_TARGET_BINDING') -eq 'FAIL') 'missing wake fingerprint fails closed'
    $wakeBase = Join-Path $workspace '.catdesk\wake-bridge'
    New-Item -ItemType Directory -Path (Join-Path $wakeBase 'browser-profile') -Force | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $wakeBase 'venv\Scripts') -Force | Out-Null
    Set-Content -LiteralPath (Join-Path $wakeBase 'venv\Scripts\python.exe') -Value 'fixture' -Encoding ascii
    $directWake = $base.Clone(); [void]$directWake.Remove('WakeEvidence')
    @{ conversation_url = $wakeTarget; profile_dir = '.catdesk\wake-bridge\browser-profile' } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $wakeBase 'config.json') -Encoding utf8
    $validWake = & $checker -Workspace $workspace -ExpectedWakeTargetSha256 $wakeHash -TestSeams $directWake | ConvertFrom-Json
    Require ((Gate $validWake 'WAKE_RUNTIME_PRESENCE') -eq 'PASS' -and (Gate $validWake 'WAKE_TARGET_BINDING') -eq 'PASS') 'minimal valid wake config binds the expected opaque fingerprint'
    @{ conversation_url = 'https://chatgpt.com/c/fixture-conversation-1234?forbidden=query'; profile_dir = '.catdesk\wake-bridge\browser-profile' } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $wakeBase 'config.json') -Encoding utf8
    $malformedWake = & $checker -Workspace $workspace -ExpectedWakeTargetSha256 $wakeHash -TestSeams $directWake | ConvertFrom-Json
    Require ((Gate $malformedWake 'WAKE_TARGET_BINDING') -eq 'FAIL') 'unsupported/malformed wake URL fails closed'
    Require (($malformedWake | ConvertTo-Json -Depth 5) -notmatch 'fixture-conversation|forbidden=query|chatgpt\.com|'+$wakeHash) 'wake target URL and expected fingerprint never leak into snapshots'
    $storageFailure = $base.Clone(); $storageFailure.RetentionEvidence = { param($root) $false }
    $retention = & $checker -Workspace $workspace -ExpectedWakeTargetSha256 $wakeHash -TestSeams $storageFailure | ConvertFrom-Json
    Require ((Gate $retention 'RETENTION_EVIDENCE') -eq 'FAIL') 'missing retention evidence fails preflight'
    $pendingRuntime = $base.Clone(); $pendingRuntime.LifecycleStatus = { param($root) 'LOCAL_READY_EXTERNAL_RUNTIME_PENDING' }
    $pending = & $checker -Workspace $workspace -ExpectedWakeTargetSha256 $wakeHash -TestSeams $pendingRuntime | ConvertFrom-Json
    Require ($pending.overallState -eq 'ATTENTION' -and (Gate $pending 'LIFECYCLE_STATUS') -eq 'ATTENTION') 'external runtime pending remains distinct attention state'
    $unverifiedOfficial = $base.Clone(); $unverifiedOfficial.OfficialRuntimeOwnership = { param($root, $config) $false }
    $runtimeFailure = & $checker -Workspace $workspace -ExpectedWakeTargetSha256 $wakeHash -TestSeams $unverifiedOfficial | ConvertFrom-Json
    Require ((Gate $runtimeFailure 'EXTERNAL_RUNTIME_OWNERSHIP') -eq 'FAIL') 'unverified or non-official runtime fails closed'
    $noncanonicalListener = $base.Clone(); $noncanonicalListener.CanonicalListenerInstance = { param($root, $config, $hash) 'UNKNOWN' }
    $listenerFailure = & $checker -Workspace $workspace -ExpectedWakeTargetSha256 $wakeHash -TestSeams $noncanonicalListener | ConvertFrom-Json
    Require ((Gate $listenerFailure 'CANONICAL_LISTENER_INSTANCE') -eq 'FAIL' -and $listenerFailure.instanceFingerprint -eq 'UNKNOWN') 'noncanonical or ambiguous listener fails closed'

    # Exercise the production exact-daemon rule itself rather than overriding
    # the full gate. Same-name non-daemon/GUI processes are intentionally not
    # inputs: only the exact canonical daemon candidate matching the listener
    # PID may produce the fingerprint.
    $exactListener = $base.Clone(); [void]$exactListener.Remove('CanonicalListenerInstance')
    $canonicalPath = Join-Path $workspace 'target\release\catdesk.exe'
    $exactListener.CanonicalIdentity = ({ param($root, $hash) [pscustomobject]@{ Path = $canonicalPath; Sha256 = $hash } }.GetNewClosure())
    $exactListener.ConfiguredLocalMcp = { param($config) [pscustomobject]@{ Port = 3200 } }
    $exactListener.CanonicalListener = { param($local, $canonical) [pscustomobject]@{ Pid = 77; MatchesCanonical = $true } }
    $exactListener.CanonicalDaemonCandidates = { param($canonical) @([pscustomobject]@{ Pid = 77; MatchesCanonical = $true }) }
    $exactListener.ProcessById = ({ param($pid) [pscustomobject]@{ Path = $canonicalPath; StartTime = [DateTime]::UtcNow } }.GetNewClosure())
    $exactListener.ResolveProcessPath = { param($path) $path }
    $exactInstance = & $checker -Workspace $workspace -ExpectedWakeTargetSha256 $wakeHash -TestSeams $exactListener | ConvertFrom-Json
    Require ((Gate $exactInstance 'CANONICAL_LISTENER_INSTANCE') -eq 'PASS' -and $exactInstance.instanceFingerprint -match '^[a-f0-9]{24}$') 'one exact canonical daemon candidate produces a listener fingerprint despite same-name process noise'
    $wrongOwner = $exactListener.Clone(); $wrongOwner.CanonicalDaemonCandidates = { param($canonical) @([pscustomobject]@{ Pid = 78; MatchesCanonical = $true }) }
    $wrongOwnerSnapshot = & $checker -Workspace $workspace -ExpectedWakeTargetSha256 $wakeHash -TestSeams $wrongOwner | ConvertFrom-Json
    Require ((Gate $wrongOwnerSnapshot 'CANONICAL_LISTENER_INSTANCE') -eq 'FAIL') 'canonical daemon candidate with a mismatched listener PID fails closed'
    $multipleOwners = $exactListener.Clone(); $multipleOwners.CanonicalDaemonCandidates = { param($canonical) @([pscustomobject]@{ Pid = 77; MatchesCanonical = $true }, [pscustomobject]@{ Pid = 78; MatchesCanonical = $true }) }
    $multipleOwnerSnapshot = & $checker -Workspace $workspace -ExpectedWakeTargetSha256 $wakeHash -TestSeams $multipleOwners | ConvertFrom-Json
    Require ((Gate $multipleOwnerSnapshot 'CANONICAL_LISTENER_INSTANCE') -eq 'FAIL') 'multiple canonical daemon candidates fail closed'

    $prePath = Join-Path $workspace 'pre.json'; $postPath = Join-Path $workspace 'post.json'
    $pre = New-CompleteSnapshot -Build ('b' * 64) -Instance ('1' * 24)
    $post = New-CompleteSnapshot -Build ('b' * 64) -Instance ('2' * 24)
    $pre | ConvertTo-Json | Set-Content -LiteralPath $prePath -Encoding utf8
    $post | ConvertTo-Json | Set-Content -LiteralPath $postPath -Encoding utf8
    $comparison = & $checker -Mode compare -PreSnapshotPath $prePath -PostSnapshotPath $postPath | ConvertFrom-Json
    Require ($comparison.overallState -eq 'PASS' -and $comparison.canonicalBuildUnchanged -and $comparison.newCatDeskInstance) 'complete successful post-reboot preflight proves a new instance with unchanged build'
    $post = New-CompleteSnapshot -Build ('b' * 64) -Instance ('2' * 24) -Lifecycle 'LOCAL_READY_EXTERNAL_RUNTIME_PENDING'; $post | ConvertTo-Json | Set-Content -LiteralPath $postPath -Encoding utf8
    $pendingComparison = & $checker -Mode compare -PreSnapshotPath $prePath -PostSnapshotPath $postPath | ConvertFrom-Json
    Require ($pendingComparison.overallState -eq 'ATTENTION' -and $pendingComparison.externalRuntimePending) 'comparison distinguishes the sole permitted external runtime pending attention from final ready'
    $wakePost = New-CompleteSnapshot -Build ('b' * 64) -Instance ('2' * 24) -GateStates @{ WAKE_TARGET_BINDING = 'FAIL' }; $wakePost | ConvertTo-Json | Set-Content -LiteralPath $postPath -Encoding utf8
    Require ((& $checker -Mode compare -PreSnapshotPath $prePath -PostSnapshotPath $postPath | ConvertFrom-Json).overallState -eq 'FAIL') 'post wake-target binding failure cannot pass comparison'
    $autostartAttention = New-CompleteSnapshot -Build ('b' * 64) -Instance ('2' * 24) -GateStates @{ AUTOSTART_OWNERSHIP = 'ATTENTION' }; $autostartAttention | ConvertTo-Json | Set-Content -LiteralPath $postPath -Encoding utf8
    Require ((& $checker -Mode compare -PreSnapshotPath $prePath -PostSnapshotPath $postPath | ConvertFrom-Json).overallState -eq 'FAIL') 'post autostart attention cannot use the narrow pending-runtime attention path'
    $autostartFailure = New-CompleteSnapshot -Build ('b' * 64) -Instance ('2' * 24) -GateStates @{ AUTOSTART_OWNERSHIP = 'FAIL' }; $autostartFailure | ConvertTo-Json | Set-Content -LiteralPath $postPath -Encoding utf8
    Require ((& $checker -Mode compare -PreSnapshotPath $prePath -PostSnapshotPath $postPath | ConvertFrom-Json).overallState -eq 'FAIL') 'post autostart failure cannot pass comparison'
    $runtimePost = New-CompleteSnapshot -Build ('b' * 64) -Instance ('2' * 24) -GateStates @{ EXTERNAL_RUNTIME_OWNERSHIP = 'FAIL' }; $runtimePost | ConvertTo-Json | Set-Content -LiteralPath $postPath -Encoding utf8
    Require ((& $checker -Mode compare -PreSnapshotPath $prePath -PostSnapshotPath $postPath | ConvertFrom-Json).overallState -eq 'FAIL') 'post external runtime ownership failure cannot pass comparison'
    $missingGate = New-CompleteSnapshot -Build ('b' * 64) -Instance ('2' * 24); $missingGate.gates = @($missingGate.gates | Select-Object -Skip 1); $missingGate | ConvertTo-Json | Set-Content -LiteralPath $postPath -Encoding utf8
    Require ((& $checker -Mode compare -PreSnapshotPath $prePath -PostSnapshotPath $postPath | ConvertFrom-Json).overallState -eq 'FAIL') 'missing required gate fails comparison closed'
    $duplicateGate = New-CompleteSnapshot -Build ('b' * 64) -Instance ('2' * 24); $duplicateGate.gates[1].id = 'CANONICAL_RELEASE'; $duplicateGate | ConvertTo-Json | Set-Content -LiteralPath $postPath -Encoding utf8
    Require ((& $checker -Mode compare -PreSnapshotPath $prePath -PostSnapshotPath $postPath | ConvertFrom-Json).overallState -eq 'FAIL') 'duplicate gate fails comparison closed'
    $wrongStage = New-CompleteSnapshot -Build ('b' * 64) -Instance ('2' * 24); $wrongStage.stage = 'POST_REBOOT_COMPARISON'; $wrongStage | ConvertTo-Json | Set-Content -LiteralPath $postPath -Encoding utf8
    Require ((& $checker -Mode compare -PreSnapshotPath $prePath -PostSnapshotPath $postPath | ConvertFrom-Json).overallState -eq 'FAIL') 'wrong snapshot stage fails comparison closed'
    $wrongSchema = New-CompleteSnapshot -Build ('b' * 64) -Instance ('2' * 24); $wrongSchema.schemaVersion = 2; $wrongSchema | ConvertTo-Json | Set-Content -LiteralPath $postPath -Encoding utf8
    Require ((& $checker -Mode compare -PreSnapshotPath $prePath -PostSnapshotPath $postPath | ConvertFrom-Json).overallState -eq 'FAIL') 'unsupported snapshot schema fails comparison closed'
    $unknownInstance = New-CompleteSnapshot -Build ('b' * 64) -Instance 'UNKNOWN'; $unknownInstance | ConvertTo-Json | Set-Content -LiteralPath $postPath -Encoding utf8
    Require ((& $checker -Mode compare -PreSnapshotPath $prePath -PostSnapshotPath $postPath | ConvertFrom-Json).overallState -eq 'FAIL') 'unknown instance fingerprint fails comparison closed'
    $invalidBuild = New-CompleteSnapshot -Build 'UNKNOWN' -Instance ('2' * 24); $invalidBuild | ConvertTo-Json | Set-Content -LiteralPath $postPath -Encoding utf8
    Require ((& $checker -Mode compare -PreSnapshotPath $prePath -PostSnapshotPath $postPath | ConvertFrom-Json).overallState -eq 'FAIL') 'unknown build fingerprint fails comparison closed'
    $unknownField = New-CompleteSnapshot -Build ('b' * 64) -Instance ('2' * 24); $unknownField | Add-Member -NotePropertyName unexpected -NotePropertyValue 'ignored'; $unknownField | ConvertTo-Json | Set-Content -LiteralPath $postPath -Encoding utf8
    Require ((& $checker -Mode compare -PreSnapshotPath $prePath -PostSnapshotPath $postPath | ConvertFrom-Json).overallState -eq 'FAIL') 'unknown snapshot field fails comparison closed'
    $post = New-CompleteSnapshot -Build ('b' * 64) -Instance ('1' * 24); $post | ConvertTo-Json | Set-Content -LiteralPath $postPath -Encoding utf8
    $staleComparison = & $checker -Mode compare -PreSnapshotPath $prePath -PostSnapshotPath $postPath | ConvertFrom-Json
    Require ($staleComparison.overallState -eq 'FAIL') 'comparison rejects unchanged CatDesk instance'
    @{ schemaVersion = 1; canonicalBuildFingerprint = ('b' * 64); instanceFingerprint = ('2' * 24); lifecycleState = 'READY' } | ConvertTo-Json | Set-Content -LiteralPath $postPath -Encoding utf8
    Require ((& $checker -Mode compare -PreSnapshotPath $prePath -PostSnapshotPath $postPath | ConvertFrom-Json).overallState -eq 'FAIL') 'handcrafted minimal snapshot cannot pass comparison'
    Set-Content -LiteralPath $postPath -Value '{not-json' -Encoding utf8
    Require ((& $checker -Mode compare -PreSnapshotPath $prePath -PostSnapshotPath $postPath | ConvertFrom-Json).overallState -eq 'FAIL') 'malformed snapshot fails comparison without surfacing parse content'
    Require ($source -notmatch '(?im)^\s*(Register-ScheduledTask|Unregister-ScheduledTask|Stop-Process|Start-Process)\b|tunnel-client.*(connect|stop|remove)|runtimes\s+(connect|stop|rm)|chrome|CONTROL_PLANE_API_KEY|credential') 'checker has no scheduler/runtime/tunnel/browser/credential mutation surface'
} finally {
    if (Test-Path -LiteralPath $workspace) { Remove-Item -LiteralPath $workspace -Recurse -Force }
}

Write-Output 'production acceptance fixture tests passed'
