[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$worker = Join-Path $PSScriptRoot 'reconcile-reviewed-catdesk-release-pair.ps1'
. $worker -LibraryOnly

function Assert([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}
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

$target = Join-Path $PSScriptRoot '..\target'
if (-not (Test-Path -LiteralPath $target -PathType Container)) { New-Item -ItemType Directory -Path $target -ErrorAction Stop | Out-Null }
$target = (Resolve-Path -LiteralPath $target).Path
$scratch = Join-Path $target ('.release-pair-fixture-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $scratch -ErrorAction Stop | Out-Null
try {
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
    Write-Output 'release-pair stage/rollback/commit authority fixtures passed'
} finally {
    $resolvedScratch = [IO.Path]::GetFullPath($scratch)
    $prefix = $target.TrimEnd('\') + '\'
    if (-not $resolvedScratch.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) { throw 'fixture cleanup path escaped target' }
    if (Test-Path -LiteralPath $resolvedScratch) { Remove-Item -LiteralPath $resolvedScratch -Recurse -Force -ErrorAction Stop }
}
