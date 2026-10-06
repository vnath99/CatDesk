[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Import-Module Microsoft.PowerShell.Utility -ErrorAction Stop
$promoter = Join-Path $PSScriptRoot 'promote-reviewed-catdesk-build.ps1'
$source = Get-Content -LiteralPath $promoter -Raw
$tokens = $null; $errors = $null
[void][System.Management.Automation.Language.Parser]::ParseInput($source, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'promotion script did not parse' }
if ($source -match '(?s)param\s*\([^)]*\$PSScriptRoot') { throw 'promotion parameter defaults must not reference PSScriptRoot before script initialization' }

function Require([bool]$Condition, [string]$Message) { if (-not $Condition) { throw "assertion failed: $Message" } }
function Hash([string]$Path) { (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant() }
function New-Fixture([string]$Root) {
    $release = Join-Path $Root 'target\release'
    New-Item -ItemType Directory -Path $release -Force | Out-Null
    $canonical = Join-Path $release 'catdesk.exe'
    $candidate = Join-Path $Root 'reviewed\catdesk-reviewed.exe'
    New-Item -ItemType Directory -Path (Split-Path -Parent $candidate) -Force | Out-Null
    [IO.File]::WriteAllText($canonical, 'canonical-old', [Text.UTF8Encoding]::new($false))
    [IO.File]::WriteAllText($candidate, 'candidate-new', [Text.UTF8Encoding]::new($false))
    [IO.File]::WriteAllText("$canonical.sha256", "$(Hash $canonical)`n", [Text.UTF8Encoding]::new($false))
    [pscustomobject]@{ Root = $Root; Canonical = $canonical; Candidate = $candidate }
}
function Reset-Fixture([string]$Root) {
    foreach ($path in @((Join-Path $Root 'target'), (Join-Path $Root 'reviewed'), (Join-Path $Root '.catdesk'))) {
        if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path -Recurse -Force }
    }
    New-Fixture $Root
}
function Write-InterruptedTransaction($Fixture, [string]$PriorHash, [string]$CandidateHash) {
    $directory = Join-Path $Fixture.Root '.catdesk\promotion-recovery'
    New-Item -ItemType Directory -Path $directory -Force | Out-Null
    Copy-Item -LiteralPath $Fixture.Canonical -Destination (Join-Path $directory 'previous-catdesk.exe') -Force
    [IO.File]::WriteAllText((Join-Path $directory 'previous-catdesk.exe.sha256'), "$PriorHash`n", [Text.UTF8Encoding]::new($false))
    @{ schemaVersion = 2; transactionId = '11111111111111111111111111111111'; authorizationId = '0123456789abcdef0123456789abcdef'; phase = 'CANONICAL_MUTATION_STARTED'; priorHash = $PriorHash; candidateHash = $CandidateHash; candidateRelativePath = 'reviewed\catdesk-reviewed.exe' } | ConvertTo-Json -Compress | Set-Content -LiteralPath (Join-Path $directory 'promotion-transaction.json') -Encoding utf8
}
function TransactionPath([string]$Root) { Join-Path $Root '.catdesk\promotion-recovery\promotion-transaction.json' }
function New-Seams([Collections.Generic.List[string]]$Calls, [bool[]]$Results = @($true, $true)) {
    $handoffState = [pscustomobject]@{ Index = 0; Results = @($Results) }
    $restart = {
        param($root, $from, $target, $hash, $port, $timeout)
        [void]$Calls.Add("handoff:$([IO.Path]::GetFileName($target))")
        $result = $handoffState.Results[[Math]::Min($handoffState.Index, $handoffState.Results.Count - 1)]
        $handoffState.Index++
        return $result
    }.GetNewClosure()
    @{
        ListenerForBuild = { param($build, $hash, $port) [pscustomobject]@{ Pid = 42; ProcessStartedAtUtc = '2026-09-07T12:00:00.1234567Z'; BuildPath = $build; BuildHash = $hash } }
        RestartHandoff = $restart
        ProtectedAuthorization = { param($root, $token, $transactionId, $candidate, $candidateHash, $priorHash) [pscustomobject]@{ authorizationId = '0123456789abcdef0123456789abcdef'; transactionId = '11111111111111111111111111111111' } }
        Failure = { param($message) [void]$Calls.Add("failure:$message") }
    }
}
function Write-Phase1Evidence($Fixture, [hashtable]$Overrides = @{}) {
    $directory = Join-Path $Fixture.Root '.catdesk\restart-handoff'
    New-Item -ItemType Directory -Path $directory -Force | Out-Null
    $completed = [DateTimeOffset]::UtcNow
    $record = [ordered]@{
        schemaVersion = 3
        status = 'RECOVERED_PENDING_TRANSPORT_CHECK'
        completedAtUtc = $completed.ToString('o')
        oldPid = 41
        newPid = 42
        newProcessStartedAtUtc = $completed.AddSeconds(-10).ToString('o')
        mcpPort = 3200
        oldListenerPorts = @(3200)
        workspacePath = $Fixture.Root
        buildPath = $Fixture.Candidate
        buildSha256 = Hash $Fixture.Candidate
        stage = 'complete'
        recovery = 'bounded fixture evidence'
    }
    foreach ($entry in $Overrides.GetEnumerator()) { $record[$entry.Key] = $entry.Value }
    $record | ConvertTo-Json -Compress | Set-Content -LiteralPath (Join-Path $directory 'latest.json') -Encoding utf8
}
function Write-NativePhase1Evidence($Fixture, [hashtable]$Overrides = @{}) {
    $directory = Join-Path $Fixture.Root '.catdesk\restart-handoff'
    New-Item -ItemType Directory -Path $directory -Force | Out-Null
    $completed = [DateTimeOffset]::UtcNow
    $record = [ordered]@{
        schemaVersion = 1
        status = 'REPLACEMENT_READY_PENDING_TRANSPORT_RECONNECT'
        completedAtUnix = $completed.ToUnixTimeSeconds()
        oldPid = 41
        newPid = 42
        port = 3200
        replacementSha256 = Hash $Fixture.Candidate
        stage = 'complete'
        rollbackAttempted = $false
        rollbackReady = $false
    }
    foreach ($entry in $Overrides.GetEnumerator()) { $record[$entry.Key] = $entry.Value }
    $record | ConvertTo-Json -Compress | Set-Content -LiteralPath (Join-Path $directory 'latest.json') -Encoding utf8
}
function New-CandidateListener($Fixture, [DateTimeOffset]$StartedAtUtc) {
    [pscustomobject]@{
        Pid = 42
        ProcessName = 'catdesk'
        ProcessStartedAtUtc = $StartedAtUtc.ToUniversalTime().ToString('o')
        BuildPath = $Fixture.Candidate
        BuildHash = Hash $Fixture.Candidate
    }
}
function New-ResumeSeams([Collections.Generic.List[string]]$Calls, [object[]]$Listeners, [bool[]]$Results = @($true)) {
    $seams = New-Seams $Calls $Results
    $capturedListeners = @($Listeners)
    $seams.Phase1ListenerRecords = { param($port) $capturedListeners }.GetNewClosure()
    return $seams
}
function Require-ValidationReadOnly($Fixture, [string]$BeforeHash, [Collections.Generic.List[string]]$Calls, $Result, [string]$ExpectedState, [string]$Message) {
    Require ($Result.state -eq $ExpectedState -and $Result.candidateValidated -and $Result.canonicalReleaseValidated -and $Result.tunnelAction -eq 'NONE') "$Message returns only the bounded validation result"
    Require ((Hash $Fixture.Canonical) -eq $BeforeHash -and $Calls.Count -eq 0) "$Message performs no daemon handoff or canonical mutation"
    Require (-not (Test-Path -LiteralPath (Join-Path $Fixture.Root '.catdesk\promotion-recovery'))) "$Message creates no backup or transaction state"
}

$workspace = Join-Path ([IO.Path]::GetTempPath()) ('catdesk-promotion-fixture-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $workspace -Force | Out-Null
try {
    $fixture = Reset-Fixture $workspace
    $beforeDirect = Hash $fixture.Canonical
    $direct = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -Execute | ConvertFrom-Json
    Require ($direct.state -eq 'REVIEWED_PROMOTION_AUTHORIZATION_REQUIRED' -and (Hash $fixture.Canonical) -eq $beforeDirect -and -not (Test-Path -LiteralPath (TransactionPath $workspace))) 'direct Execute without protected authorization performs zero promotion mutation'

    $fixture = Reset-Fixture $workspace
    $before = Hash $fixture.Canonical
    $plan = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace | ConvertFrom-Json
    Require ($plan.state -eq 'PLAN_READY' -and $plan.candidateValidated -and $plan.canonicalReleaseValidated -and $plan.promotionRequired -and $plan.tunnelAction -eq 'NONE') 'plan validates a pending promotion without a tunnel action'
    Require ((Hash $fixture.Canonical) -eq $before -and -not (Test-Path -LiteralPath (Join-Path $workspace '.catdesk\promotion-recovery'))) 'plan mode makes no binary or backup mutation'
    Require (($plan | ConvertTo-Json -Compress) -notmatch '(?i)secret|token|https?://|route|tunnel-client|catdesk-reviewed\.exe') 'plan output remains compact and redacted'

    # T-0132: validation is a read-only view of the existing T-0127 Phase-1
    # proof. All validation outcomes must leave both the listener and release
    # mutation paths untouched.
    $fixture = Reset-Fixture $workspace
    Copy-Item -LiteralPath $fixture.Canonical -Destination $fixture.Candidate -Force
    $before = Hash $fixture.Canonical
    $calls = [Collections.Generic.List[string]]::new()
    $validationAlreadyCurrent = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -ValidatePhase1 -TestSeams (New-Seams $calls) | ConvertFrom-Json
    Require-ValidationReadOnly $fixture $before $calls $validationAlreadyCurrent 'ALREADY_CURRENT' 'already-current validation'
    Require (-not $validationAlreadyCurrent.promotionRequired) 'already-current validation reports no promotion requirement'

    $fixture = Reset-Fixture $workspace
    $before = Hash $fixture.Canonical
    $completed = [DateTimeOffset]::UtcNow
    Write-NativePhase1Evidence $fixture @{ completedAtUnix = $completed.ToUnixTimeSeconds() }
    $calls = [Collections.Generic.List[string]]::new()
    $nativeValidation = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -ValidatePhase1 -TestSeams (New-ResumeSeams $calls @((New-CandidateListener $fixture $completed.AddSeconds(-10)))) | ConvertFrom-Json
    Require-ValidationReadOnly $fixture $before $calls $nativeValidation 'PHASE1_PROVEN' 'native Phase-1 validation'
    Require ($nativeValidation.promotionRequired) 'native Phase-1 validation retains the pending-promotion indication'

    $fixture = Reset-Fixture $workspace
    $before = Hash $fixture.Canonical
    $completed = [DateTimeOffset]::UtcNow.AddMinutes(-2)
    Write-Phase1Evidence $fixture @{ completedAtUtc = $completed.ToString('o'); newProcessStartedAtUtc = $completed.AddSeconds(-10).ToString('o') }
    $calls = [Collections.Generic.List[string]]::new()
    $legacyValidation = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -ValidatePhase1 -TestSeams (New-ResumeSeams $calls @((New-CandidateListener $fixture $completed.AddSeconds(-10)))) | ConvertFrom-Json
    Require-ValidationReadOnly $fixture $before $calls $legacyValidation 'PHASE1_PROVEN' 'legacy Phase-1 validation'

    $fixture = Reset-Fixture $workspace
    $before = Hash $fixture.Canonical
    $completed = [DateTimeOffset]::UtcNow
    Write-NativePhase1Evidence $fixture @{ completedAtUnix = $completed.ToUnixTimeSeconds() }
    $calls = [Collections.Generic.List[string]]::new()
    $samePid = New-CandidateListener $fixture $completed.AddSeconds(-10)
    $dualStackValidation = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -ValidatePhase1 -TestSeams (New-ResumeSeams $calls @($samePid, $samePid)) | ConvertFrom-Json
    Require-ValidationReadOnly $fixture $before $calls $dualStackValidation 'PHASE1_PROVEN' 'same-PID IPv4/IPv6 Phase-1 validation'

    $fixture = Reset-Fixture $workspace
    $before = Hash $fixture.Canonical
    $calls = [Collections.Generic.List[string]]::new()
    $notStartedValidation = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -ValidatePhase1 -TestSeams (New-ResumeSeams $calls @()) | ConvertFrom-Json
    Require-ValidationReadOnly $fixture $before $calls $notStartedValidation 'PHASE1_NOT_STARTED' 'absent Phase-1 with exact canonical listener validation'

    foreach ($case in @(
        @{ Name = 'malformed receipt'; Record = { param($f) $d = Join-Path $f.Root '.catdesk\restart-handoff'; New-Item -ItemType Directory -Path $d -Force | Out-Null; Set-Content -LiteralPath (Join-Path $d 'latest.json') -Value '{malformed' -Encoding utf8 }; Listeners = @() },
        @{ Name = 'future receipt timestamp'; Record = { param($f) Write-NativePhase1Evidence $f @{ completedAtUnix = [DateTimeOffset]::UtcNow.AddMinutes(2).ToUnixTimeSeconds() } }; Listeners = @((New-CandidateListener $fixture ([DateTimeOffset]::UtcNow.AddSeconds(-10)))) },
        @{ Name = 'candidate hash drift'; Record = { param($f) Write-NativePhase1Evidence $f @{ replacementSha256 = ('0' * 64) } }; Listeners = @((New-CandidateListener $fixture ([DateTimeOffset]::UtcNow.AddSeconds(-10)))) },
        @{ Name = 'receipt PID drift'; Record = { param($f) Write-NativePhase1Evidence $f @{ newPid = 43 } }; Listeners = @((New-CandidateListener $fixture ([DateTimeOffset]::UtcNow.AddSeconds(-10)))) }
    )) {
        $fixture = Reset-Fixture $workspace
        $before = Hash $fixture.Canonical
        & $case.Record $fixture
        $calls = [Collections.Generic.List[string]]::new()
        $listeners = @($case.Listeners | ForEach-Object { $_ })
        foreach ($listener in $listeners) { if ($null -ne $listener) { $listener.BuildPath = $fixture.Candidate; $listener.BuildHash = Hash $fixture.Candidate } }
        $unsafeValidation = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -ValidatePhase1 -TestSeams (New-ResumeSeams $calls $listeners) | ConvertFrom-Json
        Require-ValidationReadOnly $fixture $before $calls $unsafeValidation 'OPERATOR_ATTENTION_PHASE1' "$($case.Name) validation"
    }

    foreach ($listenerCase in @(
        @{ Name = 'distinct listener PIDs'; Rows = { param($f, $when) $first = New-CandidateListener $f $when; $second = New-CandidateListener $f $when; $second.Pid = 43; @($first, $second) } },
        @{ Name = 'three listener rows'; Rows = { param($f, $when) $one = New-CandidateListener $f $when; @($one, $one, $one) } },
        @{ Name = 'candidate path drift'; Rows = { param($f, $when) $one = New-CandidateListener $f $when; $one.BuildPath = $f.Canonical; @($one) } },
        @{ Name = 'process-start drift'; Rows = { param($f, $when) @((New-CandidateListener $f ([DateTimeOffset]::UtcNow.AddMinutes(1)))) } }
    )) {
        $fixture = Reset-Fixture $workspace
        $before = Hash $fixture.Canonical
        $completed = [DateTimeOffset]::UtcNow.AddMinutes(-2)
        Write-NativePhase1Evidence $fixture @{ completedAtUnix = $completed.ToUnixTimeSeconds() }
        $calls = [Collections.Generic.List[string]]::new()
        $unsafeValidation = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -ValidatePhase1 -TestSeams (New-ResumeSeams $calls @(& $listenerCase.Rows $fixture $completed.AddSeconds(-10))) | ConvertFrom-Json
        Require-ValidationReadOnly $fixture $before $calls $unsafeValidation 'OPERATOR_ATTENTION_PHASE1' "$($listenerCase.Name) validation"
    }

    foreach ($canonicalFailure in @('missing canonical listener', 'wrong canonical listener')) {
        $fixture = Reset-Fixture $workspace
        $before = Hash $fixture.Canonical
        $calls = [Collections.Generic.List[string]]::new()
        $seams = New-ResumeSeams $calls @()
        $seams.ListenerForBuild = { param($build, $hash, $port) return $null }
        $attentionValidation = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -ValidatePhase1 -TestSeams $seams | ConvertFrom-Json
        Require-ValidationReadOnly $fixture $before $calls $attentionValidation 'OPERATOR_ATTENTION_PHASE1' "$canonicalFailure validation"
    }

    $fixture = Reset-Fixture $workspace
    $bothModesRejected = $false
    try { [void](& $promoter -BuildPath $fixture.Candidate -Workspace $workspace -ValidatePhase1 -Execute) } catch { $bothModesRejected = $true }
    Require $bothModesRejected 'ValidatePhase1 and Execute are mutually exclusive'

    $fixture = Reset-Fixture $workspace
    $completed = [DateTimeOffset]::UtcNow
    Write-NativePhase1Evidence $fixture @{ completedAtUnix = $completed.ToUnixTimeSeconds() }
    $calls = [Collections.Generic.List[string]]::new()
    $nativeListener = New-CandidateListener $fixture $completed.AddSeconds(-10)
    $nativeResume = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -Execute -TestSeams (New-ResumeSeams $calls @($nativeListener)) | ConvertFrom-Json
    Require ($nativeResume.state -eq 'PROMOTED_CANONICAL_READY' -and (Hash $fixture.Canonical) -eq (Hash $fixture.Candidate) -and ($calls -join ',') -eq 'handoff:catdesk.exe') 'native reload receipt resumes directly into canonical promotion after exact live process proof'

    $fixture = Reset-Fixture $workspace
    $completed = [DateTimeOffset]::UtcNow.AddHours(-2)
    Write-NativePhase1Evidence $fixture @{ completedAtUnix = $completed.ToUnixTimeSeconds() }
    $calls = [Collections.Generic.List[string]]::new()
    $nativeLongRunningListener = New-CandidateListener $fixture $completed.AddSeconds(-10)
    $nativeLongRunning = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -Execute -TestSeams (New-ResumeSeams $calls @($nativeLongRunningListener)) | ConvertFrom-Json
    Require ($nativeLongRunning.state -eq 'PROMOTED_CANONICAL_READY' -and ($calls -join ',') -eq 'handoff:catdesk.exe') 'older native receipt remains resumable only while exact long-running process-instance continuity is still proven'

    $fixture = Reset-Fixture $workspace
    $completed = [DateTimeOffset]::UtcNow
    Write-NativePhase1Evidence $fixture @{ completedAtUnix = $completed.ToUnixTimeSeconds() }
    $calls = [Collections.Generic.List[string]]::new()
    $nativeListener = New-CandidateListener $fixture $completed.AddSeconds(-10)
    $nativeDualStack = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -Execute -TestSeams (New-ResumeSeams $calls @($nativeListener, $nativeListener)) | ConvertFrom-Json
    Require ($nativeDualStack.state -eq 'PROMOTED_CANONICAL_READY' -and ($calls -join ',') -eq 'handoff:catdesk.exe') 'native reload receipt tolerates at most an IPv4/IPv6-equivalent pair only when both rows prove the same candidate PID/path/hash'

    foreach ($nativeOverride in @(
        @{ status = 'ROLLED_BACK_READY_PENDING_TRANSPORT_RECONNECT' },
        @{ stage = 'rollback-readiness' },
        @{ port = 3201 },
        @{ replacementSha256 = ('0' * 64) },
        @{ newPid = 43 },
        @{ rollbackAttempted = $true },
        @{ rollbackReady = $true },
        @{ completedAtUnix = 'malformed' }
    )) {
        $fixture = Reset-Fixture $workspace
        $before = Hash $fixture.Canonical
        $completed = [DateTimeOffset]::UtcNow
        Write-NativePhase1Evidence $fixture $nativeOverride
        $calls = [Collections.Generic.List[string]]::new()
        $blocked = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -Execute -TestSeams (New-ResumeSeams $calls @((New-CandidateListener $fixture $completed.AddSeconds(-10)))) | ConvertFrom-Json
        Require ($blocked.state -eq 'OPERATOR_ATTENTION_PHASE1' -and (Hash $fixture.Canonical) -eq $before -and $calls.Count -eq 0) 'native reload rollback/failure/mismatched/malformed evidence fails closed before canonical mutation'
    }

    $fixture = Reset-Fixture $workspace
    $before = Hash $fixture.Canonical
    $futureCompleted = [DateTimeOffset]::UtcNow.AddSeconds(60)
    Write-NativePhase1Evidence $fixture @{ completedAtUnix = $futureCompleted.ToUnixTimeSeconds() }
    $calls = [Collections.Generic.List[string]]::new()
    $futureNative = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -Execute -TestSeams (New-ResumeSeams $calls @((New-CandidateListener $fixture $futureCompleted.AddSeconds(-10)))) | ConvertFrom-Json
    Require ($futureNative.state -eq 'OPERATOR_ATTENTION_PHASE1' -and (Hash $fixture.Canonical) -eq $before -and $calls.Count -eq 0) 'future native reload receipt fails closed before canonical mutation'

    $fixture = Reset-Fixture $workspace
    $before = Hash $fixture.Canonical
    $oldCompleted = [DateTimeOffset]::UtcNow.AddMinutes(-10)
    Write-NativePhase1Evidence $fixture @{ completedAtUnix = $oldCompleted.ToUnixTimeSeconds() }
    $calls = [Collections.Generic.List[string]]::new()
    $reusedNativePid = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -Execute -TestSeams (New-ResumeSeams $calls @((New-CandidateListener $fixture ([DateTimeOffset]::UtcNow.AddSeconds(-10))))) | ConvertFrom-Json
    Require ($reusedNativePid.state -eq 'OPERATOR_ATTENTION_PHASE1' -and (Hash $fixture.Canonical) -eq $before -and $calls.Count -eq 0) 'native receipt cannot authorize a later process instance that merely reuses the recorded PID'

    $fixture = Reset-Fixture $workspace
    $before = Hash $fixture.Canonical
    $completed = [DateTimeOffset]::UtcNow
    Write-NativePhase1Evidence $fixture @{ completedAtUnix = $completed.ToUnixTimeSeconds(); unexpectedField = 'blocked' }
    $calls = [Collections.Generic.List[string]]::new()
    $extraNative = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -Execute -TestSeams (New-ResumeSeams $calls @((New-CandidateListener $fixture $completed.AddSeconds(-10)))) | ConvertFrom-Json
    Require ($extraNative.state -eq 'OPERATOR_ATTENTION_PHASE1' -and (Hash $fixture.Canonical) -eq $before -and $calls.Count -eq 0) 'native receipt with any extra field fails closed'

    $fixture = Reset-Fixture $workspace
    $before = Hash $fixture.Canonical
    $completed = [DateTimeOffset]::UtcNow
    Write-NativePhase1Evidence $fixture @{ completedAtUnix = $completed.ToUnixTimeSeconds() }
    $nativePath = Join-Path $workspace '.catdesk\restart-handoff\latest.json'
    $missingNative = Get-Content -LiteralPath $nativePath -Raw | ConvertFrom-Json
    $missingNative.PSObject.Properties.Remove('replacementSha256')
    $missingNative | ConvertTo-Json -Compress | Set-Content -LiteralPath $nativePath -Encoding utf8
    $calls = [Collections.Generic.List[string]]::new()
    $missingNativeResult = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -Execute -TestSeams (New-ResumeSeams $calls @((New-CandidateListener $fixture $completed.AddSeconds(-10)))) | ConvertFrom-Json
    Require ($missingNativeResult.state -eq 'OPERATOR_ATTENTION_PHASE1' -and (Hash $fixture.Canonical) -eq $before -and $calls.Count -eq 0) 'native receipt with any required field missing fails closed'

    $fixture = Reset-Fixture $workspace
    $before = Hash $fixture.Canonical
    $completed = [DateTimeOffset]::UtcNow
    Write-NativePhase1Evidence $fixture @{ completedAtUnix = $completed.ToUnixTimeSeconds() }
    $calls = [Collections.Generic.List[string]]::new()
    $nativeListener = New-CandidateListener $fixture $completed.AddSeconds(-10)
    $distinctNativeListener = New-CandidateListener $fixture $completed.AddSeconds(-10)
    $distinctNativeListener.Pid = 43
    $distinctNative = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -Execute -TestSeams (New-ResumeSeams $calls @($nativeListener, $distinctNativeListener)) | ConvertFrom-Json
    Require ($distinctNative.state -eq 'OPERATOR_ATTENTION_PHASE1' -and (Hash $fixture.Canonical) -eq $before -and $calls.Count -eq 0) 'native dual-stack-looking rows with distinct owner PIDs fail closed'

    $fixture = Reset-Fixture $workspace
    $before = Hash $fixture.Canonical
    $completed = [DateTimeOffset]::UtcNow
    Write-NativePhase1Evidence $fixture @{ completedAtUnix = $completed.ToUnixTimeSeconds() }
    $calls = [Collections.Generic.List[string]]::new()
    $nativeListener = New-CandidateListener $fixture $completed.AddSeconds(-10)
    $threeNative = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -Execute -TestSeams (New-ResumeSeams $calls @($nativeListener, $nativeListener, $nativeListener)) | ConvertFrom-Json
    Require ($threeNative.state -eq 'OPERATOR_ATTENTION_PHASE1' -and (Hash $fixture.Canonical) -eq $before -and $calls.Count -eq 0) 'three native listener rows remain ambiguous even when all report the same PID'

    $fixture = Reset-Fixture $workspace
    $before = Hash $fixture.Canonical
    $completed = [DateTimeOffset]::UtcNow
    Write-NativePhase1Evidence $fixture @{ completedAtUnix = $completed.ToUnixTimeSeconds() }
    $calls = [Collections.Generic.List[string]]::new()
    $nativeListener = New-CandidateListener $fixture $completed.AddSeconds(-10)
    $mismatchedNativeListener = New-CandidateListener $fixture $completed.AddSeconds(-10)
    $mismatchedNativeListener.BuildHash = ('0' * 64)
    $mismatchedNativeRows = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -Execute -TestSeams (New-ResumeSeams $calls @($nativeListener, $mismatchedNativeListener)) | ConvertFrom-Json
    Require ($mismatchedNativeRows.state -eq 'OPERATOR_ATTENTION_PHASE1' -and (Hash $fixture.Canonical) -eq $before -and $calls.Count -eq 0) 'every row in an accepted same-PID native pair must prove the exact candidate path and hash'

    $fixture = Reset-Fixture $workspace
    $completed = [DateTimeOffset]::UtcNow.AddMinutes(-16)
    Write-Phase1Evidence $fixture @{ completedAtUtc = $completed.ToString('o'); newProcessStartedAtUtc = $completed.AddSeconds(-10).ToString('o') }
    $calls = [Collections.Generic.List[string]]::new()
    $candidateListener = New-CandidateListener $fixture $completed.AddSeconds(-10)
    $olderResume = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -Execute -TestSeams (New-ResumeSeams $calls @($candidateListener)) | ConvertFrom-Json
    Require ($olderResume.state -eq 'PROMOTED_CANONICAL_READY' -and ($calls -join ',') -eq 'handoff:catdesk.exe') 'a Phase 1 handoff older than fifteen minutes resumes when the same process instance is proven'

    $fixture = Reset-Fixture $workspace
    $completed = [DateTimeOffset]::UtcNow.AddDays(-7)
    Write-Phase1Evidence $fixture @{ completedAtUtc = $completed.ToString('o'); newProcessStartedAtUtc = $completed.AddSeconds(-20).ToString('o') }
    $calls = [Collections.Generic.List[string]]::new()
    $candidateListener = New-CandidateListener $fixture $completed.AddSeconds(-20)
    $longRunningResume = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -Execute -TestSeams (New-ResumeSeams $calls @($candidateListener)) | ConvertFrom-Json
    Require ($longRunningResume.state -eq 'PROMOTED_CANONICAL_READY' -and ($calls -join ',') -eq 'handoff:catdesk.exe') 'much older Phase 1 evidence remains valid only while exact process-instance continuity is proven'

    $fixture = Reset-Fixture $workspace
    $completed = [DateTimeOffset]::UtcNow.AddHours(-2)
    Write-Phase1Evidence $fixture @{ schemaVersion = 2; completedAtUtc = $completed.ToString('o'); newProcessStartedAtUtc = $null }
    $legacyV2Path = Join-Path $workspace '.catdesk\restart-handoff\latest.json'
    $legacyV2 = Get-Content -LiteralPath $legacyV2Path -Raw | ConvertFrom-Json
    $legacyV2.PSObject.Properties.Remove('newProcessStartedAtUtc')
    $legacyV2 | ConvertTo-Json -Compress | Set-Content -LiteralPath $legacyV2Path -Encoding utf8
    $calls = [Collections.Generic.List[string]]::new()
    $schemaTwoResume = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -Execute -TestSeams (New-ResumeSeams $calls @((New-CandidateListener $fixture $completed.AddSeconds(-10)))) | ConvertFrom-Json
    Require ($schemaTwoResume.state -eq 'PROMOTED_CANONICAL_READY' -and ($calls -join ',') -eq 'handoff:catdesk.exe') 'the observed older schema-2 handoff resumes only when current process-instance continuity is proven'

    $fixture = Reset-Fixture $workspace
    $before = Hash $fixture.Canonical
    Write-Phase1Evidence $fixture
    $calls = [Collections.Generic.List[string]]::new()
    $candidateListener = New-CandidateListener $fixture ([DateTimeOffset]::UtcNow.AddSeconds(-10))
    $resumed = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -Execute -TestSeams (New-ResumeSeams $calls @($candidateListener)) | ConvertFrom-Json
    Require ($resumed.state -eq 'PROMOTED_CANONICAL_READY' -and (Hash $fixture.Canonical) -eq (Hash $fixture.Candidate)) 'fresh, exact Phase 1 evidence resumes directly into canonical promotion'
    Require (($calls -join ',') -eq 'handoff:catdesk.exe') 'resume performs only the canonical final handoff and never launches a duplicate candidate daemon'
    Require (-not (Test-Path -LiteralPath (TransactionPath $workspace))) 'successful resume clears its newly-created promotion transaction'
    $repeat = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -Execute | ConvertFrom-Json
    Require ($repeat.state -eq 'ALREADY_CURRENT' -and -not $repeat.promotionRequired) 'a completed Phase 1 resume is idempotently already current'

    $fixture = Reset-Fixture $workspace
    Write-Phase1Evidence $fixture
    $legacyPath = Join-Path $workspace '.catdesk\restart-handoff\latest.json'
    $legacy = Get-Content -LiteralPath $legacyPath -Raw | ConvertFrom-Json
    $legacy.schemaVersion = 1
    $legacy.PSObject.Properties.Remove('workspacePath')
    $legacy.PSObject.Properties.Remove('buildPath')
    $legacy.PSObject.Properties.Remove('newProcessStartedAtUtc')
    $legacy | ConvertTo-Json -Compress | Set-Content -LiteralPath $legacyPath -Encoding utf8
    $calls = [Collections.Generic.List[string]]::new()
    $legacyResumed = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -Execute -TestSeams (New-ResumeSeams $calls @($candidateListener)) | ConvertFrom-Json
    Require ($legacyResumed.state -eq 'PROMOTED_CANONICAL_READY' -and ($calls -join ',') -eq 'handoff:catdesk.exe') 'the observed schema-1 handoff record remains bound by its workspace location and exact live listener identity'

    foreach ($override in @(
        @{ workspacePath = (Join-Path $workspace 'wrong-workspace') },
        @{ buildPath = (Join-Path $workspace 'reviewed\wrong.exe') },
        @{ buildSha256 = ('0' * 64) }
    )) {
        $fixture = Reset-Fixture $workspace
        $before = Hash $fixture.Canonical
        Write-Phase1Evidence $fixture $override
        $calls = [Collections.Generic.List[string]]::new()
        $blocked = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -Execute -TestSeams (New-ResumeSeams $calls @($candidateListener)) | ConvertFrom-Json
        Require ($blocked.state -eq 'OPERATOR_ATTENTION_PHASE1' -and (Hash $fixture.Canonical) -eq $before -and $calls.Count -eq 0) 'mismatched Phase 1 evidence fails closed without a new handoff or canonical mutation'
    }

    foreach ($unsafeListener in @(
        (New-CandidateListener $fixture ([DateTimeOffset]::UtcNow.AddMinutes(1))),
        [pscustomobject]@{ Pid = 42; ProcessName = 'catdesk'; ProcessStartedAtUtc = $null; BuildPath = $fixture.Candidate; BuildHash = Hash $fixture.Candidate }
    )) {
        $fixture = Reset-Fixture $workspace
        $completed = [DateTimeOffset]::UtcNow.AddMinutes(-30)
        Write-Phase1Evidence $fixture @{ completedAtUtc = $completed.ToString('o'); newProcessStartedAtUtc = $completed.AddSeconds(-10).ToString('o') }
        if ($null -ne $unsafeListener.ProcessStartedAtUtc) { $unsafeListener.BuildPath = $fixture.Candidate; $unsafeListener.BuildHash = Hash $fixture.Candidate }
        $calls = [Collections.Generic.List[string]]::new()
        $blocked = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -Execute -TestSeams (New-ResumeSeams $calls @($unsafeListener)) | ConvertFrom-Json
        Require ($blocked.state -eq 'OPERATOR_ATTENTION_PHASE1' -and $calls.Count -eq 0) 'reused PID or unreadable process start time fails closed before canonical mutation'
    }

    $fixture = Reset-Fixture $workspace
    $futureCompleted = [DateTimeOffset]::UtcNow.AddSeconds(60)
    Write-Phase1Evidence $fixture @{ completedAtUtc = $futureCompleted.ToString('o'); newProcessStartedAtUtc = $futureCompleted.AddSeconds(-10).ToString('o') }
    $calls = [Collections.Generic.List[string]]::new()
    $futureEvidence = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -Execute -TestSeams (New-ResumeSeams $calls @((New-CandidateListener $fixture $futureCompleted.AddSeconds(-10)))) | ConvertFrom-Json
    Require ($futureEvidence.state -eq 'OPERATOR_ATTENTION_PHASE1' -and $calls.Count -eq 0) 'future or clock-skewed handoff completion fails closed before canonical mutation'

    $fixture = Reset-Fixture $workspace
    $completed = [DateTimeOffset]::UtcNow.AddMinutes(-30)
    Write-Phase1Evidence $fixture @{ schemaVersion = 2; completedAtUtc = $completed.ToString('o'); newProcessStartedAtUtc = $null }
    $schemaTwoPath = Join-Path $workspace '.catdesk\restart-handoff\latest.json'
    $schemaTwo = Get-Content -LiteralPath $schemaTwoPath -Raw | ConvertFrom-Json
    $schemaTwo.PSObject.Properties.Remove('newProcessStartedAtUtc')
    $schemaTwo | ConvertTo-Json -Compress | Set-Content -LiteralPath $schemaTwoPath -Encoding utf8
    $calls = [Collections.Generic.List[string]]::new()
    $implausiblyOld = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -Execute -TestSeams (New-ResumeSeams $calls @((New-CandidateListener $fixture $completed.AddSeconds(-181)))) | ConvertFrom-Json
    Require ($implausiblyOld.state -eq 'OPERATOR_ATTENTION_PHASE1' -and $calls.Count -eq 0) 'implausibly old process start time fails closed before canonical mutation'

    foreach ($override in @(
        @{ newPid = 43 },
        @{ newProcessStartedAtUtc = 'malformed' }
    )) {
        $fixture = Reset-Fixture $workspace
        $completed = [DateTimeOffset]::UtcNow.AddMinutes(-30)
        $override.completedAtUtc = $completed.ToString('o')
        if (-not $override.ContainsKey('newProcessStartedAtUtc')) { $override.newProcessStartedAtUtc = $completed.AddSeconds(-10).ToString('o') }
        Write-Phase1Evidence $fixture $override
        $calls = [Collections.Generic.List[string]]::new()
        $blocked = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -Execute -TestSeams (New-ResumeSeams $calls @((New-CandidateListener $fixture $completed.AddSeconds(-10)))) | ConvertFrom-Json
        Require ($blocked.state -eq 'OPERATOR_ATTENTION_PHASE1' -and $calls.Count -eq 0) 'wrong handoff PID or malformed durable process-start identity fails closed before canonical mutation'
    }

    foreach ($invalidState in @('{malformed', '{"schemaVersion":99,"status":"UNKNOWN"}')) {
        $fixture = Reset-Fixture $workspace
        $directory = Join-Path $workspace '.catdesk\restart-handoff'; New-Item -ItemType Directory -Path $directory -Force | Out-Null
        Set-Content -LiteralPath (Join-Path $directory 'latest.json') -Value $invalidState -Encoding utf8
        $calls = [Collections.Generic.List[string]]::new()
        $blocked = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -Execute -TestSeams (New-ResumeSeams $calls @($candidateListener)) | ConvertFrom-Json
        Require ($blocked.state -eq 'OPERATOR_ATTENTION_PHASE1' -and $calls.Count -eq 0) 'malformed or unknown Phase 1 state fails closed without a new handoff'
    }

    foreach ($listeners in @(
        @(),
        @([pscustomobject]@{ Pid = 42; ProcessName = 'catdesk'; BuildPath = $fixture.Canonical; BuildHash = Hash $fixture.Canonical }),
        @([pscustomobject]@{ Pid = 42; ProcessName = 'catdesk'; BuildPath = $fixture.Candidate; BuildHash = Hash $fixture.Canonical }),
        @($candidateListener, [pscustomobject]@{ Pid = 43; ProcessName = 'catdesk'; BuildPath = $fixture.Candidate; BuildHash = Hash $fixture.Candidate })
    )) {
        $fixture = Reset-Fixture $workspace
        Write-Phase1Evidence $fixture
        $calls = [Collections.Generic.List[string]]::new()
        $blocked = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -Execute -TestSeams (New-ResumeSeams $calls $listeners) | ConvertFrom-Json
        Require ($blocked.state -eq 'OPERATOR_ATTENTION_PHASE1' -and $calls.Count -eq 0) 'non-sole, wrong-path, wrong-hash, or duplicate candidate listeners fail closed'
    }

    $fixture = Reset-Fixture $workspace
    Write-Phase1Evidence $fixture
    $priorHash = Hash $fixture.Canonical; $candidateHash = Hash $fixture.Candidate
    Write-InterruptedTransaction $fixture $priorHash $candidateHash
    $calls = [Collections.Generic.List[string]]::new()
    $transactionPrecedence = New-Seams $calls
    $transactionPrecedence.Phase1ListenerRecords = { throw 'resume listener evidence must not be examined while a transaction exists' }
    $interrupted = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -Execute -TestSeams $transactionPrecedence | ConvertFrom-Json
    Require ($interrupted.state -eq 'RECOVERED_PRIOR_CANONICAL' -and ($calls -join ',') -eq 'handoff:catdesk.exe') 'interrupted transaction recovery retains precedence over Phase 1 resume'

    $fixture = Reset-Fixture $workspace
    Write-Phase1Evidence $fixture
    [IO.File]::WriteAllText("$($fixture.Canonical).sha256", ('0' * 64), [Text.UTF8Encoding]::new($false))
    $calls = [Collections.Generic.List[string]]::new()
    $invalidCanonicalResume = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -Execute -TestSeams (New-ResumeSeams $calls @($candidateListener)) | ConvertFrom-Json
    Require ($invalidCanonicalResume.state -eq 'OPERATOR_ATTENTION_INVALID' -and @($calls | Where-Object { $_ -like 'handoff:*' }).Count -eq 0) 'invalid canonical evidence blocks resume before any promotion action'

    [IO.File]::WriteAllText("$($fixture.Canonical).sha256", ('0' * 64), [Text.UTF8Encoding]::new($false))
    $invalidManifest = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace | ConvertFrom-Json
    Require ($invalidManifest.state -eq 'OPERATOR_ATTENTION_INVALID') 'invalid canonical manifest fails closed'
    [IO.File]::WriteAllText("$($fixture.Canonical).sha256", "$(Hash $fixture.Canonical)`n", [Text.UTF8Encoding]::new($false))

    Copy-Item -LiteralPath $fixture.Canonical -Destination $fixture.Candidate -Force
    $alreadyCurrent = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace | ConvertFrom-Json
    Require ($alreadyCurrent.state -eq 'ALREADY_CURRENT' -and -not $alreadyCurrent.promotionRequired) 'equal reviewed build is reported already current'
    [IO.File]::WriteAllText($fixture.Candidate, 'candidate-new', [Text.UTF8Encoding]::new($false))

    $outside = Join-Path ([IO.Path]::GetTempPath()) ('catdesk-promotion-outside-' + [Guid]::NewGuid().ToString('N') + '.exe')
    [IO.File]::WriteAllText($outside, 'outside', [Text.UTF8Encoding]::new($false))
    $outsideResult = & $promoter -BuildPath $outside -Workspace $workspace | ConvertFrom-Json
    Require ($outsideResult.state -eq 'OPERATOR_ATTENTION_INVALID') 'candidate outside workspace is rejected'
    Remove-Item -LiteralPath $outside -Force

    $escapeTarget = Join-Path ([IO.Path]::GetTempPath()) ('catdesk-promotion-escape-' + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $escapeTarget -Force | Out-Null
    [IO.File]::WriteAllText((Join-Path $escapeTarget 'candidate.exe'), 'outside', [Text.UTF8Encoding]::new($false))
    $junction = Join-Path $workspace 'escape'
    New-Item -ItemType Junction -Path $junction -Target $escapeTarget | Out-Null
    $reparseResult = & $promoter -BuildPath (Join-Path $junction 'candidate.exe') -Workspace $workspace | ConvertFrom-Json
    Require ($reparseResult.state -eq 'OPERATOR_ATTENTION_INVALID') 'reparse traversal candidate is rejected'
    [IO.Directory]::Delete($junction)
    Require (Test-Path -LiteralPath $escapeTarget -PathType Container) 'fixture junction removal must not traverse into its target'
    Remove-Item -LiteralPath $escapeTarget -Recurse -Force

    $fixture = Reset-Fixture $workspace
    $priorHash = Hash $fixture.Canonical; $candidateHash = Hash $fixture.Candidate
    Write-InterruptedTransaction $fixture $priorHash $candidateHash
    Copy-Item -LiteralPath $fixture.Candidate -Destination $fixture.Canonical -Force
    $interruptedPlan = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace | ConvertFrom-Json
    Require ($interruptedPlan.state -eq 'INTERRUPTED_RECOVERY_REQUIRED' -and (Hash $fixture.Canonical) -eq $candidateHash -and (Test-Path -LiteralPath (TransactionPath $workspace))) 'plan detects binary-new manifest-old interruption without mutation'
    $calls = [Collections.Generic.List[string]]::new()
    $interruptedRecovered = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -Execute -TestSeams (New-Seams $calls) | ConvertFrom-Json
    Require ($interruptedRecovered.state -eq 'RECOVERED_PRIOR_CANONICAL' -and (Hash $fixture.Canonical) -eq $priorHash -and (Get-Content -LiteralPath "$($fixture.Canonical).sha256" -Raw).Trim() -eq $priorHash) 'execute recovers binary-new manifest-old to a validated prior pair'
    Require (-not (Test-Path -LiteralPath (TransactionPath $workspace)) -and ($calls -join ',') -eq 'handoff:catdesk.exe') 'recovery clears durable transaction only after the required canonical handoff'

    # Public one-command recovery owns listener reconciliation itself. Its bounded
    # promotion child must restore durable disk/provenance state without spawning
    # the promotion helper's detached restart worker, which can outlive the parent.
    $fixture = Reset-Fixture $workspace
    $priorHash = Hash $fixture.Canonical; $candidateHash = Hash $fixture.Candidate
    Write-InterruptedTransaction $fixture $priorHash $candidateHash
    Copy-Item -LiteralPath $fixture.Candidate -Destination $fixture.Canonical -Force
    $calls = [Collections.Generic.List[string]]::new()
    $rollbackOnlySeams = New-Seams $calls
    $rollbackOnlySeams.ListenerForBuild = { throw 'rollback-only listener observation must not run' }
    $rollbackOnly = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -Execute -RecoveryRollbackOnly -TestSeams $rollbackOnlySeams | ConvertFrom-Json
    Require ($rollbackOnly.state -eq 'RECOVERED_PRIOR_CANONICAL' -and (Hash $fixture.Canonical) -eq $priorHash -and (Get-Content -LiteralPath "$($fixture.Canonical).sha256" -Raw).Trim() -eq $priorHash) 'recovery rollback-only mode restores and validates the prior canonical pair without listener observation'
    Require (-not (Test-Path -LiteralPath (TransactionPath $workspace)) -and ($calls -join ',') -notmatch 'handoff:') 'recovery rollback-only mode clears the durable transaction without listener observation or a detached promotion handoff'

    $idempotentPlan = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace | ConvertFrom-Json
    Require ($idempotentPlan.state -eq 'PLAN_READY') 'second invocation after proven recovery is ordinary and idempotent'

    $fixture = Reset-Fixture $workspace
    $priorHash = Hash $fixture.Canonical; $candidateHash = Hash $fixture.Candidate
    Write-InterruptedTransaction $fixture $priorHash $candidateHash
    Copy-Item -LiteralPath $fixture.Candidate -Destination $fixture.Canonical -Force
    [IO.File]::WriteAllText("$($fixture.Canonical).sha256", "$candidateHash`n", [Text.UTF8Encoding]::new($false))
    $completedCandidate = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -Execute -TestSeams (New-Seams ([Collections.Generic.List[string]]::new())) | ConvertFrom-Json
    Require ($completedCandidate.state -eq 'RECOVERED_PRIOR_CANONICAL' -and (Hash $fixture.Canonical) -eq $priorHash) 'uncompleted durable candidate pair prefers proven rollback to prior release'

    $fixture = Reset-Fixture $workspace
    $priorHash = Hash $fixture.Canonical; $candidateHash = Hash $fixture.Candidate
    Write-InterruptedTransaction $fixture $priorHash $candidateHash
    $alreadyPrior = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -Execute -TestSeams (New-Seams ([Collections.Generic.List[string]]::new())) | ConvertFrom-Json
    Require ($alreadyPrior.state -eq 'RECOVERED_PRIOR_CANONICAL' -and (Hash $fixture.Canonical) -eq $priorHash) 'already-restored prior pair clears an otherwise valid interrupted transaction'

    $fixture = Reset-Fixture $workspace
    $directory = Join-Path $workspace '.catdesk\promotion-recovery'; New-Item -ItemType Directory -Path $directory -Force | Out-Null
    Set-Content -LiteralPath (TransactionPath $workspace) -Value '{malformed' -Encoding utf8
    $malformedJournal = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace | ConvertFrom-Json
    Require ($malformedJournal.state -in @('ROLLBACK_UNPROVEN_OPERATOR_ATTENTION', 'OPERATOR_ATTENTION_INVALID')) 'malformed durable journal fails closed without cleanup'
    Require (Test-Path -LiteralPath (TransactionPath $workspace)) 'malformed durable journal is retained for operator evidence'

    $fixture = Reset-Fixture $workspace
    $directory = Join-Path $workspace '.catdesk\promotion-recovery'; New-Item -ItemType Directory -Path $directory -Force | Out-Null
    @{ schemaVersion = 99; phase = 'UNKNOWN'; priorHash = ('0' * 64); candidateHash = ('1' * 64); candidateRelativePath = 'reviewed\catdesk-reviewed.exe' } | ConvertTo-Json -Compress | Set-Content -LiteralPath (TransactionPath $workspace) -Encoding utf8
    $unknownJournal = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace | ConvertFrom-Json
    Require ($unknownJournal.state -in @('ROLLBACK_UNPROVEN_OPERATOR_ATTENTION', 'OPERATOR_ATTENTION_INVALID')) 'unknown durable journal schema or phase fails closed without cleanup'
    Require (Test-Path -LiteralPath (TransactionPath $workspace)) 'unknown durable journal is retained for operator evidence'

    $fixture = Reset-Fixture $workspace
    $reparseTarget = Join-Path ([IO.Path]::GetTempPath()) ('catdesk-promotion-recovery-escape-' + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $reparseTarget -Force | Out-Null
    New-Item -ItemType Junction -Path (Join-Path $workspace '.catdesk') -Target $reparseTarget | Out-Null
    $reparseJournal = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace | ConvertFrom-Json
    Require ($reparseJournal.state -eq 'OPERATOR_ATTENTION_INVALID') 'reparse recovery location is rejected before transaction parsing'
    [IO.Directory]::Delete((Join-Path $workspace '.catdesk'))
    Require (Test-Path -LiteralPath $reparseTarget -PathType Container) 'fixture recovery junction removal must not traverse into its target'
    Remove-Item -LiteralPath $reparseTarget -Recurse -Force

    $fixture = Reset-Fixture $workspace
    $priorHash = Hash $fixture.Canonical; $candidateHash = Hash $fixture.Candidate
    Write-InterruptedTransaction $fixture ('0' * 64) $candidateHash
    $wrongHashes = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -Execute | ConvertFrom-Json
    Require ($wrongHashes.state -eq 'REVIEWED_PROMOTION_AUTHORIZATION_REQUIRED' -and (Test-Path -LiteralPath (TransactionPath $workspace))) 'wrong transaction hashes cannot resume without the exact protected authorization and retain the journal'

    $fixture = Reset-Fixture $workspace
    $priorHash = Hash $fixture.Canonical; $candidateHash = Hash $fixture.Candidate
    Write-InterruptedTransaction $fixture $priorHash $candidateHash
    Remove-Item -LiteralPath (Join-Path $workspace '.catdesk\promotion-recovery\previous-catdesk.exe') -Force
    $missingBackup = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -Execute | ConvertFrom-Json
    Require ($missingBackup.state -eq 'REVIEWED_PROMOTION_AUTHORIZATION_REQUIRED') 'interrupted recovery without protected authorization fails closed before backup handling'

    $fixture = Reset-Fixture $workspace
    $priorHash = Hash $fixture.Canonical; $candidateHash = Hash $fixture.Candidate
    Write-InterruptedTransaction $fixture $priorHash $candidateHash
    Copy-Item -LiteralPath $fixture.Candidate -Destination $fixture.Canonical -Force
    $calls = [Collections.Generic.List[string]]::new()
    $handoffFailure = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -Execute -TestSeams (New-Seams $calls @($false)) | ConvertFrom-Json
    Require ($handoffFailure.state -eq 'ROLLBACK_UNPROVEN_OPERATOR_ATTENTION' -and (Test-Path -LiteralPath (TransactionPath $workspace))) 'failed interrupted recovery handoff retains transaction and fails closed'

    $fixture = Reset-Fixture $workspace
    $before = Hash $fixture.Canonical
    $calls = [Collections.Generic.List[string]]::new()
    $success = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -Execute -TestSeams (New-Seams $calls) | ConvertFrom-Json
    Require ($success.state -eq 'PROMOTED_CANONICAL_READY' -and (Hash $fixture.Canonical) -eq (Hash $fixture.Candidate)) "two-stage promotion returns the daemon to canonical candidate-equivalent bytes (state=$($success.state); calls=$($calls -join ','))"
    Require (($calls -join ',') -eq 'handoff:catdesk-reviewed.exe,handoff:catdesk.exe') 'promotion invokes exactly candidate then canonical PID-scoped handoff'
    $backup = Join-Path $workspace '.catdesk\promotion-recovery\previous-catdesk.exe'
    Require ((Test-Path -LiteralPath $backup -PathType Leaf) -and (Hash $backup) -eq $before) 'one recoverable previous canonical backup is retained'
    Require (@(Get-ChildItem -LiteralPath (Split-Path -Parent $backup) -File).Count -eq 3) 'promotion recovery storage remains bounded to backup pair plus reviewed authority'
    $lkgPointerPath = Join-Path $workspace '.catdesk\release-recovery\current.json'
    Require (Test-Path -LiteralPath $lkgPointerPath -PathType Leaf) 'successful promotion persists a durable last-known-good pointer before transaction cleanup'
    $lkgPointer = Get-Content -LiteralPath $lkgPointerPath -Raw | ConvertFrom-Json
    $lkgSlotPath = Join-Path $workspace ".catdesk\release-recovery\$($lkgPointer.slot)"
    $lkgBinary = Join-Path $lkgSlotPath 'catdesk.exe'
    $lkgSha = (Get-Content -LiteralPath (Join-Path $lkgSlotPath 'catdesk.exe.sha256') -Raw).Trim()
    Require ((Hash $lkgBinary) -eq (Hash $fixture.Canonical) -and $lkgSha -eq (Hash $fixture.Canonical) -and [string]$lkgPointer.sha256 -eq (Hash $fixture.Canonical)) 'successful promotion advances LKG authority to the exact proven canonical hash'
    $reviewedAuthority = Get-Content -LiteralPath (Join-Path $workspace '.catdesk\promotion-recovery\reviewed-promotion.json') -Raw | ConvertFrom-Json
    Require ($reviewedAuthority.schemaVersion -eq 1 -and $reviewedAuthority.stage -eq 'CANONICAL_HANDOFF_PROVEN' -and $reviewedAuthority.transactionId -match '^[0-9a-f]{32}$' -and $reviewedAuthority.candidateHash -eq (Hash $fixture.Canonical) -and $reviewedAuthority.canonicalRelativePath -eq 'target\release\catdesk.exe' -and $reviewedAuthority.canonicalSha256 -eq (Hash $fixture.Canonical)) 'successful promotion persists exact reviewed-promotion authority before LKG transaction cleanup'

    $fixture = Reset-Fixture $workspace
    $calls = [Collections.Generic.List[string]]::new()
    $phaseOneHash = Hash $fixture.Canonical
    $phaseOneFailure = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -Execute -TestSeams (New-Seams $calls @($false)) | ConvertFrom-Json
    Require ($phaseOneFailure.state -eq 'OPERATOR_ATTENTION_PHASE1' -and (Hash $fixture.Canonical) -eq $phaseOneHash) 'first handoff failure leaves canonical disk release untouched'
    Require ($calls.Count -eq 1 -and -not (Test-Path -LiteralPath (Join-Path $workspace '.catdesk\promotion-recovery\previous-catdesk.exe'))) 'first handoff failure does not swap or back up canonical release'

    $fixture = Reset-Fixture $workspace
    $calls = [Collections.Generic.List[string]]::new()
    $swapSeams = New-Seams $calls
    $swapSeams.SwapCanonical = { param($root, $candidate, $hash, $canonical) throw 'fixture swap failure' }
    $swapFailure = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -Execute -TestSeams $swapSeams | ConvertFrom-Json
    Require ($swapFailure.state -eq 'OPERATOR_ATTENTION_SWAP' -and (Hash $fixture.Canonical) -ne (Hash $fixture.Candidate)) 'swap failure preserves current canonical release'

    $fixture = Reset-Fixture $workspace
    $oldHash = Hash $fixture.Canonical
    $calls = [Collections.Generic.List[string]]::new()
    $afterBinary = New-Seams $calls
    $afterBinary.Checkpoint = { param($name) if ($name -eq 'AFTER_CANONICAL_BINARY_REPLACEMENT') { throw 'fixture after binary replacement' } }
    $binaryWindow = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -Execute -TestSeams $afterBinary | ConvertFrom-Json
    Require ($binaryWindow.state -eq 'OPERATOR_ATTENTION_SWAP' -and (Hash $fixture.Canonical) -eq $oldHash) 'failure after canonical binary replacement restores the prior valid pair'
    Require ((Get-Content -LiteralPath "$($fixture.Canonical).sha256" -Raw).Trim() -eq $oldHash) 'restored manifest matches the restored prior binary'

    $fixture = Reset-Fixture $workspace
    $calls = [Collections.Generic.List[string]]::new()
    $afterManifest = New-Seams $calls
    $afterManifest.Checkpoint = { param($name) if ($name -eq 'AFTER_CANONICAL_MANIFEST_REPLACEMENT') { throw 'fixture after manifest replacement' } }
    $manifestWindow = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -Execute -TestSeams $afterManifest | ConvertFrom-Json
    Require ($manifestWindow.state -eq 'PROMOTED_CANONICAL_READY' -and (Hash $fixture.Canonical) -eq (Hash $fixture.Candidate)) 'failure after manifest replacement accepts only the fully valid candidate pair'

    $fixture = Reset-Fixture $workspace
    $calls = [Collections.Generic.List[string]]::new()
    $restoreFailure = New-Seams $calls
    $restoreFailure.Checkpoint = {
        param($name)
        if ($name -eq 'AFTER_CANONICAL_BINARY_REPLACEMENT') { throw 'fixture restore boundary failure' }
    }
    $restoreFailure.RestoreCanonical = { param($root, $backup, $canonical) throw 'fixture restoration unavailable' }
    $unproven = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -Execute -TestSeams $restoreFailure | ConvertFrom-Json
    Require ($unproven.state -eq 'ROLLBACK_UNPROVEN_OPERATOR_ATTENTION') "restoration failure is explicitly fail-closed and never reported as a safe swap attention state (state=$($unproven.state))"

    $fixture = Reset-Fixture $workspace
    $oldHash = Hash $fixture.Canonical
    $calls = [Collections.Generic.List[string]]::new()
    $rollback = & $promoter -BuildPath $fixture.Candidate -Workspace $workspace -Execute -TestSeams (New-Seams $calls @($true, $false, $true)) | ConvertFrom-Json
    Require ($rollback.state -eq 'ROLLED_BACK_OPERATOR_ATTENTION' -and (Hash $fixture.Canonical) -eq $oldHash) 'second handoff failure restores prior canonical binary once'
    Require (($calls -join ',') -eq 'handoff:catdesk-reviewed.exe,handoff:catdesk.exe,handoff:catdesk.exe') 'rollback makes at most one recovery handoff attempt'

    Require ($source -notmatch '(?im)^\s*(Stop-Process|Start-Process|Register-ScheduledTask|Unregister-ScheduledTask)\b|tunnel-client.*(connect|stop|remove)|runtimes\s+(connect|stop|rm)|^\s*(Invoke-WebRequest|Invoke-RestMethod)\b|chrome') 'promotion source contains no direct process, scheduler, tunnel, browser, or network operation'
} finally {
    if (Test-Path -LiteralPath $workspace) { Remove-Item -LiteralPath $workspace -Recurse -Force }
}

Write-Output 'reviewed build promotion fixture tests passed'
