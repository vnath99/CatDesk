[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$worker = Join-Path $PSScriptRoot 'reconcile-reviewed-catdesk-release-pair.ps1'
. $worker -LibraryOnly

function Assert([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}
$literal='2026-10-18T00:00:00.1234567Z'
$literalRead=ConvertFrom-CatDeskLiteralJson ('{"ownerStartedAtUtc":"'+$literal+'"}')
Assert ($literalRead.ownerStartedAtUtc -is [string] -and $literalRead.ownerStartedAtUtc -ceq $literal) 'JSON reader changed the literal process-instance witness'
function Save-Json([string]$Path, $Value) {
    $parent = Split-Path -Parent $Path
    if (-not (Test-Path -LiteralPath $parent -PathType Container)) { New-Item -ItemType Directory -Path $parent -Force -ErrorAction Stop | Out-Null }
    [IO.File]::WriteAllText($Path, ($Value | ConvertTo-Json -Depth 12 -Compress), [Text.UTF8Encoding]::new($false))
}
function Save-Bytes([string]$Path, [string]$Value) {
    $parent = Split-Path -Parent $Path
    if (-not (Test-Path -LiteralPath $parent -PathType Container)) { New-Item -ItemType Directory -Path $parent -Force -ErrorAction Stop | Out-Null }
    [IO.File]::WriteAllText($Path, $Value, [Text.UTF8Encoding]::new($false))
}

# These seam functions are replaced only inside this disposable fixture
# process. The fixed operator invocation never accepts a seam argument.
function Get-ReconcileListeners { return @() }
function Get-ReconcileOwnerProof([string]$Root, [string]$ExpectedHash, [int]$ExpectedPid, [string]$ExpectedStart) {
    if ($ExpectedPid -ne 424242 -or ($ExpectedStart -and $ExpectedStart -cne '2026-10-18T00:00:00.0000000Z')) { throw 'test owner changed' }
    return [pscustomobject]@{ pid = 424242; startedAtUtc = '2026-10-18T00:00:00.0000000Z'; imageSha256 = $ExpectedHash }
}

function New-Fixture([string]$Root) {
    $release = Join-Path $Root 'target\release'
    $slotA = Join-Path $Root '.catdesk\release-recovery\slot-a'
    $control = Join-Path $Root '.catdesk\release-pair-control'
    $build = Join-Path $Root 'build'
    foreach ($directory in @($release, $slotA, $control, $build)) { New-Item -ItemType Directory -Path $directory -Force -ErrorAction Stop | Out-Null }
    $binary = Join-Path $release 'catdesk.exe'
    $sidecar = Join-Path $release 'catdesk.exe.sha256'
    $candidate = Join-Path $build 'candidate.exe'
    Save-Bytes $binary 'divergent-original-image'
    Save-Bytes $candidate 'independently-reviewed-candidate-image'
    Save-Bytes (Join-Path $slotA 'catdesk.exe') 'operational-escrow-image'
    $legacyHash = Get-ReconcileHash (Join-Path $slotA 'catdesk.exe')
    Save-Bytes $sidecar "$legacyHash`n"
    Save-Bytes (Join-Path $slotA 'catdesk.exe.sha256') "$legacyHash`n"
    Save-Json (Join-Path $slotA 'manifest.json') ([ordered]@{schemaVersion=1;generation=1;sha256=$legacyHash;source='operational_verified';capturedAtUtc='2026-10-18T00:00:00Z'})
    $pointer = Join-Path $Root '.catdesk\release-recovery\current.json'
    Save-Json $pointer ([ordered]@{schemaVersion=1;slot='slot-a';generation=1;sha256=$legacyHash})
    $authId = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
    $txId = 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb'
    $candidateHash = Get-ReconcileHash $candidate
    $helper = Join-Path $PSScriptRoot 'catdesk-release-recovery.ps1'
    $powerShellImage = (Get-Process -Id $PID -ErrorAction Stop).Path
    $auth = [ordered]@{
        schemaVersion=1;authorizationId=$authId;transactionId=$txId
        expiresAtUnix=[DateTimeOffset]::UtcNow.AddMinutes(2).ToUnixTimeSeconds()
        reviewedPreflightSha256=('c'*64);reviewedAuthorizationId=('d'*32)
        candidateRelativePath='build/candidate.exe';candidateSha256=$candidateHash
        originalBinarySha256=(Get-ReconcileHash $binary)
        originalSidecarFileSha256=(Get-ReconcileHash $sidecar)
        originalSidecarValue=$legacyHash
        slotABinarySha256=$legacyHash
        slotASidecarFileSha256=(Get-ReconcileHash (Join-Path $slotA 'catdesk.exe.sha256'))
        slotAManifestSha256=(Get-ReconcileHash (Join-Path $slotA 'manifest.json'))
        originalPointerSha256=(Get-ReconcileHash $pointer)
        scriptSha256=(Get-ReconcileHash $worker)
        recoveryHelperSha256=(Get-ReconcileHash $helper)
        powershellSha256=(Get-ReconcileHash $powerShellImage)
    }
    Save-Json (Join-Path $control 'authorization.json') $auth
    Save-Json (Join-Path $control 'claim.json') ([ordered]@{schemaVersion=1;authorizationId=$authId;transactionId=$txId;state='CLAIMED_PENDING'})
    return [pscustomobject]@{ Paths=(Get-ReconcilePaths $Root); Authority=$auth; Binary=$binary; Sidecar=$sidecar; Candidate=$candidate; CandidateHash=$candidateHash; Control=$control }
}

function New-StoppedFixture([string]$Root) {
    $f=New-Fixture $Root
    Save-Json (Join-Path $f.Control 'authorization.json') ([ordered]@{schemaVersion=2;mode='QUALIFIED_STOPPED';controllerSha256=('e'*64);controllerSessionId=[Diagnostics.Process]::GetCurrentProcess().SessionId;authorization=$f.Authority})
    Save-Json (Join-Path $f.Control 'claim.json') ([ordered]@{schemaVersion=2;mode='QUALIFIED_STOPPED';authorizationId=$f.Authority.authorizationId;transactionId=$f.Authority.transactionId;state='STOPPED_CLAIMED_PENDING'})
    $a=Get-StoppedReconcileAuthority $f.Paths $f.Authority.authorizationId $f.Authority.transactionId
    return [pscustomobject]@{Fixture=$f;Authority=$a}
}

function Add-StoppedFixtureProof($Stopped) {
    $f=$Stopped.Fixture;$a=$Stopped.Authority.Record
    # Synthetic source fixture only. Native qualification performs the actual
    #401/200 MCP calls and Job accounting; these records exercise their consumers.
    Save-Json (Join-Path $f.Control 'qualification.json') ([ordered]@{schemaVersion=2;mode='QUALIFIED_STOPPED';profile='HEADLESS_LOCAL';authorizationId=$a.authorizationId;transactionId=$a.transactionId;candidateSha256=$a.candidateSha256;ownerPid=424242;ownerStartedAtUtc='2026-10-18T00:00:00Z';unauthorizedStatus=401;authorizedStatus=200;initializeVerified=$true;toolsListVerified=$true;toolCount=79;qualifiedAtUnix=1792281600})
    $jobs=@{candidateJob="Local\CatDesk.ReleasePairStopped.$($a.transactionId).candidate";helperJob="Local\CatDesk.ReleasePairStopped.$($a.transactionId).helper"}
    Save-Json (Join-Path $f.Control 'stopped-quiescence.json') ([ordered]@{schemaVersion=2;mode='QUALIFIED_STOPPED';state='OWNED_TREE_EXITED';authorizationId=$a.authorizationId;transactionId=$a.transactionId;qualificationSha256=(Get-ReconcileHash (Join-Path $f.Control 'qualification.json'));candidateJob=$jobs.candidateJob;helperJob=$jobs.helperJob})
    Save-Json (Join-Path $f.Control 'stopped-recovery-exit.json') ([ordered]@{schemaVersion=2;mode='QUALIFIED_STOPPED';state='OWNED_TREE_EXITED';authorizationId=$a.authorizationId;transactionId=$a.transactionId;candidateJob=$jobs.candidateJob;helperJob=$jobs.helperJob})
}

$script:StoppedInterrupt=''
function Invoke-StoppedCheckpoint([string]$Name) {if($script:StoppedInterrupt -ceq $Name){throw 'fixture interruption'}}

$target = Join-Path $PSScriptRoot '..\target'
if (-not (Test-Path -LiteralPath $target -PathType Container)) { New-Item -ItemType Directory -Path $target -ErrorAction Stop | Out-Null }
$target = (Resolve-Path -LiteralPath $target).Path
$scratch = Join-Path $target ('.release-pair-fixture-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $scratch -ErrorAction Stop | Out-Null
try {
    $fenceRoot=Join-Path $scratch 'startup-fence';[void](New-Item -ItemType Directory -Path $fenceRoot)
    Assert (Test-CatDeskStoppedReconciliationStartupReady $fenceRoot) 'empty workspace fence did not preserve legacy startup'
    Assert (-not(Test-Path -LiteralPath (Join-Path $fenceRoot '.catdesk'))) 'read-only fence created control state'
    $held=Enter-CatDeskStoppedLifecycleLease $fenceRoot
    try{
        Assert (-not(Test-CatDeskStoppedReconciliationStartupReady $fenceRoot)) 'missing authority bypassed an active stopped lease'
        Assert (Test-CatDeskStoppedReconciliationStartupReady $fenceRoot -HeldLifecycleLease $held) 'internal held lease was mistaken for a competing owner'
    }finally{$held.Dispose()}
    $control=Join-Path $fenceRoot '.catdesk\release-pair-control'
    foreach($name in @('stopped-owner.json','transaction.json','claim.json')){
        $hint=Join-Path $control $name
        Save-Json $hint ([ordered]@{schemaVersion=2;mode='QUALIFIED_STOPPED';state='PENDING'})
        Assert (-not(Test-CatDeskStoppedReconciliationStartupReady $fenceRoot)) 'missing authority bypassed a durable stopped hint'
        Remove-Item -LiteralPath $hint -Force
    }
    $hint=Join-Path $control 'transaction.json';Save-Bytes $hint '{incomplete'
    Assert (-not(Test-CatDeskStoppedReconciliationStartupReady $fenceRoot)) 'unknown residual journal allowed startup'
    Remove-Item -LiteralPath $hint -Force
    $legacyFence=New-Fixture (Join-Path $scratch 'legacy-startup-fence')
    Assert (Test-CatDeskStoppedReconciliationStartupReady $legacyFence.Paths.Root) 'genuine legacy authority was blocked'
    $held=Enter-CatDeskStoppedLifecycleLease $legacyFence.Paths.Root
    try{Assert (-not(Test-CatDeskStoppedReconciliationStartupReady $legacyFence.Paths.Root)) 'legacy authority bypassed an active lease'}finally{$held.Dispose()}
    Save-Json (Join-Path $legacyFence.Control 'stopped-owner.json') ([ordered]@{schemaVersion=2;mode='QUALIFIED_STOPPED';state='PENDING'})
    Assert (-not(Test-CatDeskStoppedReconciliationStartupReady $legacyFence.Paths.Root)) 'legacy authority hid a pending stopped witness'
    $drift = New-Fixture (Join-Path $scratch 'drift')
    $driftAuthority = Get-ReconcileAuthority $drift.Paths.Root $drift.Authority.authorizationId $drift.Authority.transactionId $true
    Save-Bytes $drift.Binary 'changed-unreviewed-original'
    $refusedDrift = $false
    try { [void](Invoke-ReconcileStage $drift.Paths $driftAuthority) } catch { $refusedDrift = $true }
    Assert $refusedDrift 'Stage accepted changed original bytes'
    Assert (-not (Test-Path -LiteralPath $drift.Paths.Journal)) 'Stage journaled after original-byte drift'

    $rollback = New-Fixture (Join-Path $scratch 'rollback')
    $a = Get-ReconcileAuthority $rollback.Paths.Root $rollback.Authority.authorizationId $rollback.Authority.transactionId $true
    $staged = Invoke-ReconcileStage $rollback.Paths $a
    Assert ($staged.state -eq 'STAGED') 'stage did not report staged'
    Assert ((Get-ReconcileHash $rollback.Binary) -eq $rollback.CandidateHash) 'candidate binary was not staged'
    Assert ([IO.File]::ReadAllText($rollback.Sidecar).Trim() -eq $rollback.CandidateHash) 'candidate sidecar was not staged'
    Assert ((Get-ReconcileHash $rollback.Paths.Pointer) -eq $rollback.Authority.originalPointerSha256) 'stage changed the pointer'
    Assert ((Get-ReconcileHash (Join-Path $rollback.Paths.SlotA 'catdesk.exe')) -eq $rollback.Authority.slotABinarySha256) 'stage changed slot-a'
    $untrusted = $false
    try { [void](Get-CatDeskLastKnownGoodRelease $rollback.Paths.Root) } catch { $untrusted = $true }
    Assert $untrusted 'staged candidate became trusted LKG before handoff'
    Save-Json (Join-Path $rollback.Control 'quiescence.json') ([ordered]@{schemaVersion=1;state='OWNED_TREE_EXITED';authorizationId=$rollback.Authority.authorizationId;transactionId=$rollback.Authority.transactionId})
    Remove-Item -LiteralPath $rollback.Candidate -Force -ErrorAction Stop
    $rollbackAuthority = Get-ReconcileAuthority $rollback.Paths.Root $rollback.Authority.authorizationId $rollback.Authority.transactionId $false $false
    Assert ((Invoke-ReconcileEmpty $rollback.Paths $rollbackAuthority).state -eq 'CANONICAL_OWNER_EMPTY') 'empty owner proof depended on missing candidate'
    $restored = Invoke-ReconcileRollback $rollback.Paths $rollbackAuthority
    Assert ($restored.state -eq 'RESTORED_ORIGINAL_OPERATOR_ATTENTION') 'rollback did not report restored'
    Assert ((Get-ReconcileHash $rollback.Binary) -eq $rollback.Authority.originalBinarySha256) 'rollback did not restore divergent binary bytes'
    Assert ((Get-ReconcileHash $rollback.Sidecar) -eq $rollback.Authority.originalSidecarFileSha256) 'rollback did not restore original sidecar bytes'
    Assert ((Get-ReconcileHash $rollback.Paths.Pointer) -eq $rollback.Authority.originalPointerSha256) 'rollback changed original pointer'
    Assert ((Get-ReconcileHash (Join-Path $rollback.Paths.SlotA 'catdesk.exe')) -eq $rollback.Authority.slotABinarySha256) 'rollback changed slot-a'

    $success = New-Fixture (Join-Path $scratch 'success')
    $b = Get-ReconcileAuthority $success.Paths.Root $success.Authority.authorizationId $success.Authority.transactionId $true
    [void](Invoke-ReconcileStage $success.Paths $b)
    $blocked = $false
    try { [void](Invoke-ReconcileCommit $success.Paths $b 424242 '2026-10-18T00:00:00.0000000Z') } catch { $blocked = $true }
    Assert $blocked 'commit accepted no persistent Job-release marker'
    $probe = Invoke-ReconcileProbe $success.Paths $b 424242 ''
    Assert ($probe.state -eq 'SERVING_VERIFIED' -and $probe.ownerPid -eq 424242) 'owner probe failed'
    Save-Json (Join-Path $success.Control 'job-release.json') ([ordered]@{schemaVersion=1;state='JOB_RELEASED_PENDING_POSTPROOF';authorizationId=$success.Authority.authorizationId;transactionId=$success.Authority.transactionId;ownerPid=424242;ownerStartedAtUtc='2026-10-18T00:00:00.0000000Z'})
    $refusedRollback = $false
    try { [void](Invoke-ReconcileRollback $success.Paths $b) } catch { $refusedRollback = $true }
    Assert $refusedRollback 'Rollback accepted a released Job marker'
    $refusedInstance = $false
    try { [void](Invoke-ReconcileCommit $success.Paths $b 424242 '2026-10-18T00:00:01.0000000Z') } catch { $refusedInstance = $true }
    Assert $refusedInstance 'Commit accepted a changed owner instance'
    $committed = Invoke-ReconcileCommit $success.Paths $b 424242 '2026-10-18T00:00:00.0000000Z'
    Assert ($committed.state -eq 'RECONCILED_CANONICAL_READY') 'commit did not report exact handoff'
    $lkg = Get-CatDeskLastKnownGoodRelease $success.Paths.Root
    Assert ($lkg.Slot -eq 'slot-b' -and $lkg.Generation -eq 2 -and $lkg.Hash -eq $success.CandidateHash) 'completed reconciliation did not authorize slot-b'
    Assert ((Get-ReconcileHash (Join-Path $success.Paths.SlotA 'catdesk.exe')) -eq $success.Authority.slotABinarySha256) 'commit changed slot-a'
    $journal = Get-ReconcileJournal $success.Paths.Root
    $journal.phase = 'STAGED'
    Save-Json $success.Paths.Journal $journal
    $rejected = $false
    try { [void](Get-CatDeskLastKnownGoodRelease $success.Paths.Root) } catch { $rejected = $true }
    Assert $rejected 'incomplete journal retained reviewed recovery authority'
    foreach($point in @('ESCROW_COMPLETE','BINARY_REPLACED','SIDECAR_REPLACED','STAGED')) {
        $s=New-StoppedFixture (Join-Path $scratch ('stopped-stage-'+$point));$f=$s.Fixture
        $script:StoppedInterrupt=$point;$interrupted=$false
        try{[void](Invoke-StoppedStage $f.Paths $s.Authority)}catch{if($_.Exception.Message -cne 'fixture interruption'){throw};$interrupted=$true}
        Assert $interrupted 'stopped stage checkpoint was not reached'
        $script:StoppedInterrupt=''
        Assert ((Invoke-StoppedStage $f.Paths $s.Authority).state -ceq 'STAGED') 'same transaction did not recover partial stage'
        Assert ((Get-StoppedJournal $f.Paths $s.Authority).transactionId -ceq $f.Authority.transactionId) 'stage recovery changed transaction'
        Assert (-not(Test-CatDeskStoppedReconciliationStartupReady $f.Paths.Root)) 'pending stopped stage allowed public startup'
        Add-StoppedFixtureProof $s
        [void](Invoke-StoppedCommit $f.Paths $s.Authority)
        Assert ((Get-CatDeskLastKnownGoodRelease $f.Paths.Root).Hash -ceq $f.CandidateHash) 'stopped recovered stage did not admit exact candidate'
        Assert ((& {param($workspace)Set-StrictMode -Version Latest;Read-CatDeskLkgSlot $workspace 'slot-b'} $f.Paths.Root).Hash -ceq $f.CandidateHash) 'cold stopped slot reader depended on caller scope'
        Assert (-not(Test-Path -LiteralPath $f.Paths.Released)) 'stopped mode fabricated serving release marker'
        $legacyAccepted=$false
        try{[void](Get-CatDeskReviewedReconciliationAuthority $f.Paths.Root $f.CandidateHash);$legacyAccepted=$true}catch{}
        Assert (-not $legacyAccepted) 'stopped authority was accepted as legacy serving proof'
    }
    foreach($point in @('RECEIPT_WRITTEN','SLOT_BINARY_WRITTEN','SLOT_METADATA_WRITTEN','POINTER_WRITTEN','JOURNAL_COMPLETED')) {
        $s=New-StoppedFixture (Join-Path $scratch ('stopped-commit-'+$point));$f=$s.Fixture
        [void](Invoke-StoppedStage $f.Paths $s.Authority);Add-StoppedFixtureProof $s
        $script:StoppedInterrupt=$point;$interrupted=$false
        try{[void](Invoke-StoppedCommit $f.Paths $s.Authority)}catch{if($_.Exception.Message -cne 'fixture interruption'){throw};$interrupted=$true}
        Assert $interrupted 'stopped commit checkpoint was not reached'
        $script:StoppedInterrupt=''
        $unknownTemporary=$null
        if($point -ceq 'SLOT_METADATA_WRITTEN'){$unknownTemporary=$f.Paths.Pointer+'.tmp';Save-Bytes $unknownTemporary 'preserved-unknown-temporary'}
        [void](Invoke-StoppedStage $f.Paths $s.Authority)
        Assert ((Invoke-StoppedCommit $f.Paths $s.Authority).state -ceq 'QUALIFIED_STOPPED') 'same transaction did not recover partial commit'
        $receipt=Get-ReconcileHash (Join-Path $f.Control 'qualified-stopped.json')
        [void](Invoke-StoppedCommit $f.Paths $s.Authority)
        Assert ((Get-ReconcileHash (Join-Path $f.Control 'qualified-stopped.json')) -ceq $receipt) 'idempotent commit changed receipt'
        if($unknownTemporary){$preserved=@(Get-ChildItem -LiteralPath (Split-Path -Parent $unknownTemporary) -Filter 'current.json.tmp.interrupted-*');Assert ($preserved.Count -eq 1 -and [IO.File]::ReadAllText($preserved[0].FullName) -ceq 'preserved-unknown-temporary') 'interrupted pointer temporary was discarded'}
        Assert (Test-CatDeskStoppedReconciliationStartupReady $f.Paths.Root) 'completed stopped proof was not admitted'
        $lockPath=Join-Path $f.Control 'stopped-owner.lock'
        $lease=[IO.File]::Open($lockPath,[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
        try{Assert (-not(Test-CatDeskStoppedReconciliationStartupReady $f.Paths.Root)) 'active stopped controller allowed public startup'}finally{$lease.Dispose()}
        $bad=Read-ReconcileJson $f.Paths.Root '.catdesk\release-pair-control\qualification.json' 4096
        $bad.unauthorizedStatus=200;Save-Json (Join-Path $f.Control 'qualification.json') $bad
        $refused=$false;try{[void](Get-CatDeskLastKnownGoodRelease $f.Paths.Root)}catch{$refused=$true}
        Assert $refused 'unauthorized status or changed proof retained stopped trust'
    }
    $s=New-StoppedFixture (Join-Path $scratch 'stopped-partial-copy');$f=$s.Fixture;$a=$s.Authority.Record
    Save-Json $f.Paths.Journal ([ordered]@{schemaVersion=2;mode='QUALIFIED_STOPPED';authorizationId=$a.authorizationId;transactionId=$a.transactionId;phase='PREPARING';candidateSha256=$a.candidateSha256;originalBinarySha256=$a.originalBinarySha256;originalSidecarFileSha256=$a.originalSidecarFileSha256})
    $partial=Join-Path $f.Paths.Escrow 'original-catdesk.exe.stopped.tmp';Save-Bytes $partial 'partial-original-copy'
    [void](Invoke-StoppedStage $f.Paths $s.Authority)
    $preserved=@(Get-ChildItem -LiteralPath $f.Paths.Escrow -Filter 'original-catdesk.exe.stopped.tmp.interrupted-*')
    Assert ($preserved.Count -eq 1 -and [IO.File]::ReadAllText($preserved[0].FullName) -ceq 'partial-original-copy') 'interrupted escrow copy was discarded'
    Add-StoppedFixtureProof $s
    Save-Bytes (Join-Path $f.Paths.SlotB 'catdesk.exe') 'unknown-slot-b-image'
    $beforeJournal=Get-ReconcileHash $f.Paths.Journal;$refused=$false
    try{[void](Invoke-StoppedCommit $f.Paths $s.Authority)}catch{$refused=$true}
    Assert ($refused -and (Get-ReconcileHash $f.Paths.Journal) -ceq $beforeJournal -and -not(Test-Path -LiteralPath (Join-Path $f.Control 'qualified-stopped.json'))) 'unknown slot output caused commit writes'
    $s=New-StoppedFixture (Join-Path $scratch 'stopped-unknown');$f=$s.Fixture
    $script:StoppedInterrupt='BINARY_REPLACED'
    try{[void](Invoke-StoppedStage $f.Paths $s.Authority)}catch{}
    $script:StoppedInterrupt='';Save-Bytes $f.Sidecar 'foreign-sidecar'
    $unknownHash=Get-ReconcileHash $f.Sidecar;$refused=$false
    try{[void](Invoke-StoppedStage $f.Paths $s.Authority)}catch{$refused=$true}
    Assert ($refused -and (Get-ReconcileHash $f.Sidecar) -ceq $unknownHash) 'unknown partial pair was overwritten'
    $s=New-StoppedFixture (Join-Path $scratch 'stopped-rollback');$f=$s.Fixture
    [void](Invoke-StoppedStage $f.Paths $s.Authority)
    $refused=$false;try{[void](Invoke-StoppedRollback $f.Paths $s.Authority)}catch{$refused=$true}
    Assert $refused 'stopped rollback lacked whole-tree exit evidence'
    Add-StoppedFixtureProof $s
    $knownSidecar=[IO.File]::ReadAllBytes($f.Sidecar)
    Save-Bytes $f.Sidecar (" $($f.CandidateHash)`n")
    $unknownHash=Get-ReconcileHash $f.Sidecar;$refused=$false
    try{[void](Invoke-StoppedRollback $f.Paths $s.Authority)}catch{$refused=$true}
    Assert ($refused -and (Get-ReconcileHash $f.Sidecar) -ceq $unknownHash) 'rollback overwrote unknown whitespace sidecar'
    [IO.File]::WriteAllBytes($f.Sidecar,$knownSidecar)
    Assert ((Invoke-StoppedRollback $f.Paths $s.Authority).state -ceq 'RESTORED_ORIGINAL_STOPPED') 'stopped rollback failed'
    Assert ((Get-ReconcileHash $f.Binary) -ceq $f.Authority.originalBinarySha256) 'stopped rollback lost original divergent image'
    Assert ((Get-ReconcileHash $f.Paths.Pointer) -ceq $f.Authority.originalPointerSha256) 'stopped rollback changed legacy pointer'
    $outer=Read-ReconcileJson $f.Paths.Root '.catdesk\release-pair-control\authorization.json' 12288
    $outer.mode='SERVING';Save-Json (Join-Path $f.Control 'authorization.json') $outer
    $refused=$false;try{[void](Get-StoppedReconcileAuthority $f.Paths $f.Authority.authorizationId $f.Authority.transactionId)}catch{$refused=$true}
    Assert $refused 'stopped helper accepted foreign mode'
    $outer.mode='QUALIFIED_STOPPED';$outer.controllerSessionId=[Diagnostics.Process]::GetCurrentProcess().SessionId+1;Save-Json (Join-Path $f.Control 'authorization.json') $outer
    $refused=$false;try{[void](Get-StoppedReconcileAuthority $f.Paths $f.Authority.authorizationId $f.Authority.transactionId)}catch{$refused=$true}
    Assert $refused 'stopped helper accepted another Windows session'
    Write-Output 'release-pair legacy and stopped interrupted-recovery authority fixtures passed'
} finally {
    $resolvedScratch = [IO.Path]::GetFullPath($scratch)
    $prefix = $target.TrimEnd('\') + '\'
    if (-not $resolvedScratch.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) { throw 'fixture cleanup path escaped target' }
    if (Test-Path -LiteralPath $resolvedScratch) { Remove-Item -LiteralPath $resolvedScratch -Recurse -Force -ErrorAction Stop }
}
