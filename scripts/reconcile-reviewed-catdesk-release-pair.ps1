[CmdletBinding()]
param(
    [string]$Workspace = '',
    [ValidateSet('Empty','Stage','Probe','Commit','Rollback','StoppedEmpty','StoppedStage','StoppedProbe','StoppedCommit','StoppedRollback')][string]$Phase = '',
    [string]$AuthorizationId = '',
    [string]$TransactionId = '',
    [int]$OwnerPid = 0,
    [string]$OwnerStartedAtUtc = '',
    [switch]$LibraryOnly
)

# Called only by the one-shot native reconciliation worker. The script never
# launches a daemon; the worker retains the entire candidate process Job.
$ErrorActionPreference = 'Stop'
$recoveryHelper = Join-Path $PSScriptRoot 'catdesk-release-recovery.ps1'
if (-not (Test-Path -LiteralPath $recoveryHelper -PathType Leaf)) { throw 'release-pair recovery helper is unavailable' }
. $recoveryHelper

function Get-ReconcileHash([string]$Path) { Get-CatDeskRecoverySha256 $Path }

function Get-ReconcileContainedHash([string]$Root, [string]$Path, [string]$Label) {
    $regular = Assert-CatDeskRecoveryContainedPath -Root $Root -Path $Path -Label $Label
    $item = Get-Item -LiteralPath $regular -Force -ErrorAction Stop
    if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw "$Label is unsafe" }
    return Get-ReconcileHash $regular
}

function Read-ReconcileJson([string]$Root, [string]$Relative, [int]$Limit) {
    $path = Assert-CatDeskRecoveryContainedPath -Root $Root -Path (Join-Path $Root $Relative) -Label 'reconciliation evidence'
    $item = Get-Item -LiteralPath $path -Force -ErrorAction Stop
    if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -or $item.Length -lt 1 -or $item.Length -gt $Limit) { throw 'reconciliation evidence is unsafe' }
    return ConvertFrom-CatDeskLiteralJson ([IO.File]::ReadAllText($path))
}

function Assert-ReconcileFields($Record, [string[]]$Required) {
    $names = @($Record.PSObject.Properties.Name)
    if ($names.Count -ne $Required.Count -or @($names | Where-Object { $_ -notin $Required }).Count -ne 0) { throw 'reconciliation evidence fields are invalid' }
}

function Get-ReconcilePaths([string]$Root) {
    $rootPath = Resolve-CatDeskRecoveryRoot $Root
    $control = Join-Path $rootPath '.catdesk\release-pair-control'
    $item = Get-Item -LiteralPath $control -Force -ErrorAction Stop
    if (-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'reconciliation control is unsafe' }
    [pscustomobject]@{
        Root = $rootPath; Control = $control
        Binary = Join-Path $rootPath 'target\release\catdesk.exe'
        Sidecar = Join-Path $rootPath 'target\release\catdesk.exe.sha256'
        SlotA = Join-Path $rootPath '.catdesk\release-recovery\slot-a'
        SlotB = Join-Path $rootPath '.catdesk\release-recovery\slot-b'
        Pointer = Join-Path $rootPath '.catdesk\release-recovery\current.json'
        Escrow = Join-Path $control 'escrow'
        Journal = Join-Path $control 'transaction.json'
        Receipt = Join-Path $control 'reconciled-release.json'
        Released = Join-Path $control 'job-release.json'
        Quiescence = Join-Path $control 'quiescence.json'
    }
}

function Get-ReconcileAuthority([string]$Root, [string]$AuthId, [string]$TxId, [bool]$RequireFresh, [bool]$RequireCandidate = $true) {
    if ($AuthId -notmatch '^[0-9a-f]{32}$' -or $TxId -notmatch '^[0-9a-f]{32}$') { throw 'reconciliation identity is invalid' }
    $a = Read-ReconcileJson $Root '.catdesk\release-pair-control\authorization.json' 8192
    $c = Read-ReconcileJson $Root '.catdesk\release-pair-control\claim.json' 1024
    Assert-ReconcileFields $a @('schemaVersion','authorizationId','transactionId','expiresAtUnix','reviewedPreflightSha256','reviewedAuthorizationId','candidateRelativePath','candidateSha256','originalBinarySha256','originalSidecarFileSha256','originalSidecarValue','slotABinarySha256','slotASidecarFileSha256','slotAManifestSha256','originalPointerSha256','scriptSha256','recoveryHelperSha256','powershellSha256')
    Assert-ReconcileFields $c @('schemaVersion','authorizationId','transactionId','state')
    if ($a.schemaVersion -ne 1 -or $c.schemaVersion -ne 1 -or [string]$a.authorizationId -ne $AuthId -or [string]$a.transactionId -ne $TxId -or [string]$c.authorizationId -ne $AuthId -or [string]$c.transactionId -ne $TxId -or [string]$c.state -ne 'CLAIMED_PENDING') { throw 'reconciliation authority is unavailable' }
    if ($RequireFresh -and [long]$a.expiresAtUnix -lt [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()) { throw 'reconciliation authority expired' }
    foreach ($name in @('candidateSha256','originalBinarySha256','originalSidecarFileSha256','originalSidecarValue','slotABinarySha256','slotASidecarFileSha256','slotAManifestSha256','originalPointerSha256','scriptSha256','recoveryHelperSha256','powershellSha256','reviewedPreflightSha256')) {
        if ([string]$a.$name -cnotmatch '^[a-f0-9]{64}$') { throw 'reconciliation authority is malformed' }
    }
    if ([string]$a.scriptSha256 -cne (Get-ReconcileHash $PSCommandPath) -or [string]$a.recoveryHelperSha256 -cne (Get-ReconcileHash $recoveryHelper)) { throw 'reconciliation fixed source changed' }
    $powerShellImage = (Get-Process -Id $PID -ErrorAction Stop).Path
    if (-not $powerShellImage -or [string]$a.powershellSha256 -cne (Get-ReconcileHash $powerShellImage)) { throw 'reconciliation fixed interpreter changed' }
    $relative = [string]$a.candidateRelativePath
    if (-not $relative -or [IO.Path]::IsPathRooted($relative) -or $relative -match '(^|[\\/])\.\.([\\/]|$)' -or $relative -match '[\x00-\x1f]') { throw 'reconciliation candidate path is unsafe' }
    $candidate = $null
    if ($RequireCandidate) {
        $candidate = Assert-CatDeskRecoveryContainedPath -Root $Root -Path (Join-Path $Root $relative) -Label 'reviewed reconciliation candidate'
        if ((Get-ReconcileHash $candidate) -cne [string]$a.candidateSha256) { throw 'reconciliation candidate changed' }
    }
    return [pscustomobject]@{ Record = $a; Candidate = $candidate }
}

function Get-ReconcileJournal([string]$Root) {
    $j = Read-ReconcileJson $Root '.catdesk\release-pair-control\transaction.json' 4096
    Assert-ReconcileFields $j @('schemaVersion','authorizationId','transactionId','phase','candidateSha256','originalBinarySha256','originalSidecarFileSha256')
    if ($j.schemaVersion -ne 1 -or [string]$j.phase -notin @('STAGING','STAGED','COMPLETED','ROLLED_BACK')) { throw 'reconciliation journal is invalid' }
    return $j
}

function Assert-ReconcileJournalBinding($Journal, $Authority) {
    if ([string]$Journal.authorizationId -cne [string]$Authority.authorizationId -or [string]$Journal.transactionId -cne [string]$Authority.transactionId -or [string]$Journal.candidateSha256 -cne [string]$Authority.candidateSha256 -or [string]$Journal.originalBinarySha256 -cne [string]$Authority.originalBinarySha256 -or [string]$Journal.originalSidecarFileSha256 -cne [string]$Authority.originalSidecarFileSha256) { throw 'reconciliation journal belongs to another attempt' }
}

function Set-ReconcileJournalPhase($Paths, $Journal, [string]$Next) {
    $Journal.phase = $Next
    Write-CatDeskAtomicUtf8 -Path $Paths.Journal -Text ($Journal | ConvertTo-Json -Compress)
    $saved = Get-ReconcileJournal $Paths.Root
    if ([string]$saved.phase -cne $Next) { throw 'reconciliation journal readback failed' }
}

function Get-ReconcileListeners {
    return @(Get-NetTCPConnection -State Listen -ErrorAction Stop | Where-Object { [int]$_.LocalPort -eq 3200 })
}

function Assert-ReconcilePortEmpty {
    if (@(Get-ReconcileListeners).Count -ne 0) { throw 'canonical MCP listener is already occupied' }
}

function Assert-ReconcileNoCanonicalProcess([string]$Root) {
    $canonical = Assert-CatDeskRecoveryContainedPath -Root $Root -Path (Join-Path $Root 'target\release\catdesk.exe') -Label 'canonical process image'
    $processes = @(Get-Process -ErrorAction Stop | Where-Object { [string]$_.ProcessName -ceq 'catdesk' })
    foreach ($process in $processes) {
        # An inaccessible CatDesk image is ambiguous, including a detached
        # daemon that no longer owns port 3200.
        if (-not $process.Path) { throw 'canonical process image inventory is ambiguous' }
        $path = (Resolve-Path -LiteralPath $process.Path -ErrorAction Stop).Path
        if ($path.Equals($canonical, [StringComparison]::OrdinalIgnoreCase)) { throw 'canonical process is still mapped' }
    }
}

function Invoke-ReconcileEmpty($Paths, $Authority) {
    Assert-ReconcilePortEmpty
    Assert-ReconcileNoCanonicalProcess $Paths.Root
    return [pscustomobject]@{ state = 'CANONICAL_OWNER_EMPTY'; transactionId = [string]$Authority.Record.transactionId }
}

function Get-ReconcileOwnerProof([string]$Root, [string]$ExpectedHash, [int]$ExpectedPid, [string]$ExpectedStart) {
    if ($ExpectedPid -lt 1) { throw 'reconciliation owner PID is invalid' }
    $process = Get-Process -Id $ExpectedPid -ErrorAction Stop
    if ([string]$process.ProcessName -cne 'catdesk' -or -not $process.Path) { throw 'reconciliation owner image is unavailable' }
    $expected = Assert-CatDeskRecoveryContainedPath -Root $Root -Path (Join-Path $Root 'target\release\catdesk.exe') -Label 'canonical release image'
    $actual = (Resolve-Path -LiteralPath $process.Path -ErrorAction Stop).Path
    if (-not $actual.Equals($expected,[StringComparison]::OrdinalIgnoreCase) -or (Get-ReconcileHash $actual) -cne $ExpectedHash) { throw 'reconciliation owner image changed' }
    $started = $process.StartTime.ToUniversalTime().ToString('o')
    if ($ExpectedStart -and $started -cne $ExpectedStart) { throw 'reconciliation owner process instance changed' }
    $listeners = @(Get-ReconcileListeners)
    if ($listeners.Count -lt 1 -or $listeners.Count -gt 2 -or @($listeners | Where-Object { [int]$_.OwningProcess -ne $ExpectedPid -or [string]$_.LocalAddress -notin @('127.0.0.1','::1') }).Count -ne 0) { throw 'reconciliation listener owner changed' }
    return [pscustomobject]@{ pid = $ExpectedPid; startedAtUtc = $started; imageSha256 = $ExpectedHash }
}

function Assert-ReconcileOriginal($Paths, $Authority) {
    $binary = Assert-CatDeskRecoveryContainedPath -Root $Paths.Root -Path $Paths.Binary -Label 'divergent original binary'
    $sidecar = Assert-CatDeskRecoveryContainedPath -Root $Paths.Root -Path $Paths.Sidecar -Label 'divergent original sidecar'
    if ((Get-ReconcileHash $binary) -cne [string]$Authority.originalBinarySha256 -or (Get-ReconcileHash $sidecar) -cne [string]$Authority.originalSidecarFileSha256 -or [IO.File]::ReadAllText($sidecar).Trim().ToLowerInvariant() -cne [string]$Authority.originalSidecarValue -or [string]$Authority.originalBinarySha256 -ceq [string]$Authority.originalSidecarValue) { throw 'divergent original pair changed' }
    if ((Get-ReconcileContainedHash $Paths.Root (Join-Path $Paths.SlotA 'catdesk.exe') 'operational slot-a binary') -cne [string]$Authority.slotABinarySha256 -or (Get-ReconcileContainedHash $Paths.Root (Join-Path $Paths.SlotA 'catdesk.exe.sha256') 'operational slot-a sidecar') -cne [string]$Authority.slotASidecarFileSha256 -or (Get-ReconcileContainedHash $Paths.Root (Join-Path $Paths.SlotA 'manifest.json') 'operational slot-a manifest') -cne [string]$Authority.slotAManifestSha256 -or (Get-ReconcileContainedHash $Paths.Root $Paths.Pointer 'operational slot-a pointer') -cne [string]$Authority.originalPointerSha256) { throw 'operational escrow changed' }
    if (Test-Path -LiteralPath $Paths.SlotB) { throw 'slot-b must remain empty before reconciliation' }
    if (Test-Path -LiteralPath (Join-Path $Paths.Root '.catdesk\promotion-recovery\promotion-transaction.json')) { throw 'promotion transaction conflicts with reconciliation' }
}

function Invoke-ReconcileStage($Paths, $Authority) {
    if (Test-Path -LiteralPath $Paths.Journal) { throw 'reconciliation transaction already exists' }
    [void](Invoke-ReconcileEmpty $Paths $Authority)
    Assert-ReconcileOriginal $Paths $Authority.Record
    if (Test-Path -LiteralPath $Paths.Escrow) { throw 'reconciliation escrow already exists' }
    New-Item -ItemType Directory -Path $Paths.Escrow -ErrorAction Stop | Out-Null
    Copy-CatDeskDurableFile -Source $Paths.Binary -Destination (Join-Path $Paths.Escrow 'original-catdesk.exe')
    Copy-CatDeskDurableFile -Source $Paths.Sidecar -Destination (Join-Path $Paths.Escrow 'original-catdesk.exe.sha256')
    Copy-CatDeskDurableFile -Source $Paths.Pointer -Destination (Join-Path $Paths.Escrow 'original-pointer.json')
    if ((Get-ReconcileHash (Join-Path $Paths.Escrow 'original-catdesk.exe')) -cne [string]$Authority.Record.originalBinarySha256 -or (Get-ReconcileHash (Join-Path $Paths.Escrow 'original-catdesk.exe.sha256')) -cne [string]$Authority.Record.originalSidecarFileSha256 -or (Get-ReconcileHash (Join-Path $Paths.Escrow 'original-pointer.json')) -cne [string]$Authority.Record.originalPointerSha256) { throw 'reconciliation escrow validation failed' }
    $j = [ordered]@{ schemaVersion = 1; authorizationId = [string]$Authority.Record.authorizationId; transactionId = [string]$Authority.Record.transactionId; phase = 'STAGING'; candidateSha256 = [string]$Authority.Record.candidateSha256; originalBinarySha256 = [string]$Authority.Record.originalBinarySha256; originalSidecarFileSha256 = [string]$Authority.Record.originalSidecarFileSha256 }
    Write-CatDeskDurableBytes -Path $Paths.Journal -Bytes ([Text.UTF8Encoding]::new($false).GetBytes(($j | ConvertTo-Json -Compress)))
    $binaryTemp = "$($Paths.Binary).reconcile.tmp"
    $sidecarTemp = "$($Paths.Sidecar).reconcile.tmp"
    if ((Test-Path -LiteralPath $binaryTemp) -or (Test-Path -LiteralPath $sidecarTemp)) { throw 'reconciliation staging files are occupied' }
    Copy-CatDeskDurableFile -Source $Authority.Candidate -Destination $binaryTemp
    Write-CatDeskDurableBytes -Path $sidecarTemp -Bytes ([Text.UTF8Encoding]::new($false).GetBytes("$($Authority.Record.candidateSha256)`n"))
    if ((Get-ReconcileHash $binaryTemp) -cne [string]$Authority.Record.candidateSha256 -or [IO.File]::ReadAllText($sidecarTemp).Trim() -cne [string]$Authority.Record.candidateSha256) { throw 'reconciliation staged pair changed' }
    [IO.File]::Replace($binaryTemp, $Paths.Binary, (Join-Path $Paths.Escrow 'replaced-original-catdesk.exe'), $true)
    [IO.File]::Replace($sidecarTemp, $Paths.Sidecar, (Join-Path $Paths.Escrow 'replaced-original-catdesk.exe.sha256'), $true)
    $pair = Get-CatDeskCanonicalPairEvidence $Paths.Root
    if ($pair.Hash -cne [string]$Authority.Record.candidateSha256) { throw 'reconciliation canonical pair readback failed' }
    Set-ReconcileJournalPhase $Paths (Get-ReconcileJournal $Paths.Root) 'STAGED'
    return [pscustomobject]@{ state = 'STAGED'; transactionId = [string]$Authority.Record.transactionId }
}

function Invoke-ReconcileProbe($Paths, $Authority, [int]$ProcessId, [string]$StartedAt) {
    $journal = Get-ReconcileJournal $Paths.Root
    Assert-ReconcileJournalBinding $journal $Authority.Record
    if ([string]$journal.phase -cne 'STAGED') { throw 'reconciliation is not staged' }
    $pair = Get-CatDeskCanonicalPairEvidence $Paths.Root
    if ($pair.Hash -cne [string]$Authority.Record.candidateSha256) { throw 'reconciliation canonical pair changed' }
    $proof = Get-ReconcileOwnerProof $Paths.Root ([string]$Authority.Record.candidateSha256) $ProcessId $StartedAt
    return [pscustomobject]@{ state = 'SERVING_VERIFIED'; transactionId = [string]$Authority.Record.transactionId; ownerPid = [int]$proof.pid; ownerStartedAtUtc = [string]$proof.startedAtUtc; candidateSha256 = [string]$proof.imageSha256 }
}

function Invoke-ReconcileCommit($Paths, $Authority, [int]$ProcessId, [string]$StartedAt) {
    $journal = Get-ReconcileJournal $Paths.Root
    Assert-ReconcileJournalBinding $journal $Authority.Record
    if ([string]$journal.phase -cne 'STAGED' -or (Test-Path -LiteralPath $Paths.Receipt) -or (Test-Path -LiteralPath $Paths.SlotB)) { throw 'reconciliation commit boundary is not empty' }
    $released = Read-ReconcileJson $Paths.Root '.catdesk\release-pair-control\job-release.json' 1024
    Assert-ReconcileFields $released @('schemaVersion','state','authorizationId','transactionId','ownerPid','ownerStartedAtUtc')
    if ($released.schemaVersion -ne 1 -or [string]$released.state -cne 'JOB_RELEASED_PENDING_POSTPROOF' -or [string]$released.authorizationId -cne [string]$Authority.Record.authorizationId -or [string]$released.transactionId -cne [string]$Authority.Record.transactionId -or [int]$released.ownerPid -ne $ProcessId -or [string]$released.ownerStartedAtUtc -cne $StartedAt) { throw 'persistent owner release is unproven' }
    $proof = Invoke-ReconcileProbe $Paths $Authority $ProcessId $StartedAt
    if ([string]$proof.ownerStartedAtUtc -cne $StartedAt) { throw 'post-release owner instance changed' }
    $stamp = [DateTime]::UtcNow.ToString('o')
    $receipt = [ordered]@{ schemaVersion = 1; state = 'RECONCILED_CANONICAL_READY'; authorizationId = [string]$Authority.Record.authorizationId; transactionId = [string]$Authority.Record.transactionId; candidateSha256 = [string]$Authority.Record.candidateSha256; originalBinarySha256 = [string]$Authority.Record.originalBinarySha256; originalSidecarFileSha256 = [string]$Authority.Record.originalSidecarFileSha256; ownerPid = $ProcessId; ownerStartedAtUtc = $StartedAt; postReleaseVerifiedAtUtc = $stamp }
    Write-CatDeskDurableBytes -Path $Paths.Receipt -Bytes ([Text.UTF8Encoding]::new($false).GetBytes(($receipt | ConvertTo-Json -Compress)))
    New-Item -ItemType Directory -Path $Paths.SlotB -ErrorAction Stop | Out-Null
    Copy-CatDeskDurableFile -Source $Paths.Binary -Destination (Join-Path $Paths.SlotB 'catdesk.exe')
    Write-CatDeskDurableBytes -Path (Join-Path $Paths.SlotB 'catdesk.exe.sha256') -Bytes ([Text.UTF8Encoding]::new($false).GetBytes("$($Authority.Record.candidateSha256)`n"))
    $manifest = [ordered]@{ schemaVersion = 1; generation = 2; sha256 = [string]$Authority.Record.candidateSha256; source = 'reviewed_reconciliation'; capturedAtUtc = $stamp }
    Write-CatDeskDurableBytes -Path (Join-Path $Paths.SlotB 'manifest.json') -Bytes ([Text.UTF8Encoding]::new($false).GetBytes(($manifest | ConvertTo-Json -Compress)))
    if ((Get-ReconcileHash (Join-Path $Paths.SlotB 'catdesk.exe')) -cne [string]$Authority.Record.candidateSha256) { throw 'reviewed slot-b staged binary changed' }
    if ((Get-ReconcileContainedHash $Paths.Root (Join-Path $Paths.SlotA 'catdesk.exe') 'operational slot-a binary') -cne [string]$Authority.Record.slotABinarySha256 -or (Get-ReconcileContainedHash $Paths.Root (Join-Path $Paths.SlotA 'catdesk.exe.sha256') 'operational slot-a sidecar') -cne [string]$Authority.Record.slotASidecarFileSha256 -or (Get-ReconcileContainedHash $Paths.Root (Join-Path $Paths.SlotA 'manifest.json') 'operational slot-a manifest') -cne [string]$Authority.Record.slotAManifestSha256) { throw 'operational slot-a changed during commit' }
    $pointer = [ordered]@{ schemaVersion = 1; slot = 'slot-b'; generation = 2; sha256 = [string]$Authority.Record.candidateSha256 }
    Write-CatDeskAtomicUtf8 -Path $Paths.Pointer -Text ($pointer | ConvertTo-Json -Compress)
    Set-ReconcileJournalPhase $Paths $journal 'COMPLETED'
    $accepted = Get-CatDeskLastKnownGoodRelease $Paths.Root
    if ($accepted.Slot -cne 'slot-b' -or $accepted.Generation -ne 2 -or $accepted.Hash -cne [string]$Authority.Record.candidateSha256) { throw 'reviewed slot-b acceptance readback failed' }
    return [pscustomobject]@{ state = 'RECONCILED_CANONICAL_READY'; transactionId = [string]$Authority.Record.transactionId; ownerPid = $ProcessId; candidateSha256 = [string]$Authority.Record.candidateSha256 }
}

function Invoke-ReconcileRollback($Paths, $Authority) {
    $journal = Get-ReconcileJournal $Paths.Root
    Assert-ReconcileJournalBinding $journal $Authority.Record
    if ([string]$journal.phase -notin @('STAGING','STAGED') -or (Test-Path -LiteralPath $Paths.Receipt) -or (Test-Path -LiteralPath $Paths.SlotB) -or (Test-Path -LiteralPath $Paths.Released)) { throw 'reconciliation rollback requires an unreleased transaction' }
    $quiescent = Read-ReconcileJson $Paths.Root '.catdesk\release-pair-control\quiescence.json' 1024
    Assert-ReconcileFields $quiescent @('schemaVersion','state','authorizationId','transactionId')
    if ($quiescent.schemaVersion -ne 1 -or [string]$quiescent.state -cne 'OWNED_TREE_EXITED' -or [string]$quiescent.authorizationId -cne [string]$Authority.Record.authorizationId -or [string]$quiescent.transactionId -cne [string]$Authority.Record.transactionId) { throw 'owned process tree exit is unproven' }
    [void](Invoke-ReconcileEmpty $Paths $Authority)
    $backupBinary = Assert-CatDeskRecoveryContainedPath -Root $Paths.Root -Path (Join-Path $Paths.Escrow 'original-catdesk.exe') -Label 'original binary escrow'
    $backupSidecar = Assert-CatDeskRecoveryContainedPath -Root $Paths.Root -Path (Join-Path $Paths.Escrow 'original-catdesk.exe.sha256') -Label 'original sidecar escrow'
    $backupPointer = Assert-CatDeskRecoveryContainedPath -Root $Paths.Root -Path (Join-Path $Paths.Escrow 'original-pointer.json') -Label 'original pointer escrow'
    if ((Get-ReconcileHash $backupBinary) -cne [string]$Authority.Record.originalBinarySha256 -or (Get-ReconcileHash $backupSidecar) -cne [string]$Authority.Record.originalSidecarFileSha256 -or (Get-ReconcileHash $backupPointer) -cne [string]$Authority.Record.originalPointerSha256) { throw 'original divergent escrow changed' }
    $binaryTemp = "$($Paths.Binary).rollback.tmp"
    $sidecarTemp = "$($Paths.Sidecar).rollback.tmp"
    if ((Test-Path -LiteralPath $binaryTemp) -or (Test-Path -LiteralPath $sidecarTemp)) { throw 'reconciliation rollback staging is occupied' }
    Copy-CatDeskDurableFile -Source $backupBinary -Destination $binaryTemp
    Copy-CatDeskDurableFile -Source $backupSidecar -Destination $sidecarTemp
    [IO.File]::Replace($binaryTemp, $Paths.Binary, (Join-Path $Paths.Escrow 'rollback-replaced-catdesk.exe'), $true)
    [IO.File]::Replace($sidecarTemp, $Paths.Sidecar, (Join-Path $Paths.Escrow 'rollback-replaced-catdesk.exe.sha256'), $true)
    if ((Get-ReconcileHash $Paths.Binary) -cne [string]$Authority.Record.originalBinarySha256 -or (Get-ReconcileHash $Paths.Sidecar) -cne [string]$Authority.Record.originalSidecarFileSha256 -or (Get-ReconcileHash $Paths.Pointer) -cne [string]$Authority.Record.originalPointerSha256) { throw 'divergent original readback failed' }
    Set-ReconcileJournalPhase $Paths $journal 'ROLLED_BACK'
    return [pscustomobject]@{ state = 'RESTORED_ORIGINAL_OPERATOR_ATTENTION'; transactionId = [string]$Authority.Record.transactionId }
}

function Get-StoppedReconcileAuthority($Paths, [string]$AuthId, [string]$TxId) {
    $outer = Read-ReconcileJson $Paths.Root '.catdesk\release-pair-control\authorization.json' 12288
    Assert-ReconcileFields $outer @('schemaVersion','mode','controllerSha256','controllerSessionId','authorization')
    $a = $outer.authorization
    $c = Read-ReconcileJson $Paths.Root '.catdesk\release-pair-control\claim.json' 2048
    Assert-ReconcileFields $a @('schemaVersion','authorizationId','transactionId','expiresAtUnix','reviewedPreflightSha256','reviewedAuthorizationId','candidateRelativePath','candidateSha256','originalBinarySha256','originalSidecarFileSha256','originalSidecarValue','slotABinarySha256','slotASidecarFileSha256','slotAManifestSha256','originalPointerSha256','scriptSha256','recoveryHelperSha256','powershellSha256')
    Assert-ReconcileFields $c @('schemaVersion','mode','authorizationId','transactionId','state')
    if ($outer.schemaVersion -ne 2 -or $outer.mode -cne 'QUALIFIED_STOPPED' -or $a.schemaVersion -ne 1 -or
        $c.schemaVersion -ne 2 -or $c.mode -cne 'QUALIFIED_STOPPED' -or $c.state -cne 'STOPPED_CLAIMED_PENDING' -or
        $AuthId -cnotmatch '^[a-f0-9]{32}$' -or $TxId -cnotmatch '^[a-f0-9]{32}$' -or
        $a.authorizationId -cne $AuthId -or $a.transactionId -cne $TxId -or
        $c.authorizationId -cne $AuthId -or $c.transactionId -cne $TxId -or
        $outer.controllerSha256 -cnotmatch '^[a-f0-9]{64}$' -or $outer.controllerSessionId -ne [Diagnostics.Process]::GetCurrentProcess().SessionId) { throw 'stopped authority mode or identity differs' }
    foreach($name in @('candidateSha256','originalBinarySha256','originalSidecarFileSha256','originalSidecarValue','slotABinarySha256','slotASidecarFileSha256','slotAManifestSha256','originalPointerSha256','scriptSha256','recoveryHelperSha256','powershellSha256','reviewedPreflightSha256')) {
        if ($a.$name -cnotmatch '^[a-f0-9]{64}$') { throw 'stopped authority digest is invalid' }
    }
    if ($a.scriptSha256 -cne (Get-ReconcileHash $PSCommandPath) -or $a.recoveryHelperSha256 -cne (Get-ReconcileHash $recoveryHelper) -or
        $a.powershellSha256 -cne (Get-ReconcileHash (Get-Process -Id $PID -ErrorAction Stop).Path)) { throw 'stopped fixed helper identity changed' }
    $relative = [string]$a.candidateRelativePath
    if (-not $relative -or [IO.Path]::IsPathRooted($relative) -or $relative -match '(^|[\\/])\.\.([\\/]|$)|[\x00-\x1f]') { throw 'stopped candidate path is invalid' }
    $candidate = Assert-CatDeskRecoveryContainedPath $Paths.Root (Join-Path $Paths.Root $relative) 'stopped reviewed candidate'
    if ((Get-ReconcileHash $candidate) -cne $a.candidateSha256) { throw 'stopped candidate changed' }
    return [pscustomobject]@{Record=$a;Candidate=$candidate;Outer=$outer}
}

function Invoke-StoppedCheckpoint([string]$Name) { }

function New-StoppedReplacementBackup($Paths, $Authority, [string]$Role) {
    # Escrow is already bound to this exact transaction. Keep each replaced
    # tuple member as a separate real backup; never overwrite another backup.
    $name='s-'+([string]$Authority.Record.transactionId).Substring(0,8)+'-'+$Role+'-'+[guid]::NewGuid().ToString('N').Substring(0,8)+'.bak'
    $path=Assert-CatDeskRecoveryContainedPath $Paths.Root (Join-Path $Paths.Escrow $name) 'stopped replacement backup' -AllowMissingLeaf
    if(Test-Path -LiteralPath $path){throw 'stopped replacement backup is already occupied'}
    return $path
}

function Write-StoppedAtomic([string]$Root,[string]$Path,[string]$Text) {
    [void](Assert-CatDeskRecoveryContainedPath $Root $Path 'stopped atomic output' -AllowMissingLeaf)
    foreach($suffix in @('.tmp','.previous')) {
        $temp=$Path+$suffix
        if(Test-Path -LiteralPath $temp){
            $item=Get-Item -LiteralPath $temp -Force
            if($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)){throw 'stopped atomic temporary is unsafe'}
            Move-Item -LiteralPath $temp -Destination ($temp+'.interrupted-'+[guid]::NewGuid().ToString('N')) -ErrorAction Stop
        }
    }
    Write-CatDeskAtomicUtf8 $Path $Text
}

function Write-StoppedJson([string]$Path, $Value, [string]$Root) {
    [void](Assert-CatDeskRecoveryContainedPath $Root $Path 'stopped JSON output' -AllowMissingLeaf)
    # Keep an interrupted temporary as evidence, then reuse the established
    # atomic writer. Never replace a published evidence file through this API.
    $text = $Value | ConvertTo-Json -Compress -Depth 12
    if (Test-Path -LiteralPath $Path) {
        if ([IO.File]::ReadAllText($Path) -cne $text) { throw 'stopped published evidence changed' }
        return
    }
    foreach($suffix in @('.tmp','.previous')) {
        $temporary = $Path + $suffix
        if (Test-Path -LiteralPath $temporary) {
            $item = Get-Item -LiteralPath $temporary -Force
            if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'stopped temporary is unsafe' }
            Move-Item -LiteralPath $temporary -Destination ($temporary+'.interrupted-'+[guid]::NewGuid().ToString('N')) -ErrorAction Stop
        }
    }
    Write-CatDeskAtomicUtf8 -Path $Path -Text $text
}

function Write-StoppedBytes([string]$Path, [byte[]]$Bytes, [string]$Root) {
    [void](Assert-CatDeskRecoveryContainedPath $Root $Path 'stopped bytes output' -AllowMissingLeaf)
    $sha=[Security.Cryptography.SHA256]::Create()
    try{$expected=([BitConverter]::ToString($sha.ComputeHash($Bytes)).Replace('-','')).ToLowerInvariant()}finally{$sha.Dispose()}
    if (Test-Path -LiteralPath $Path) {if((Get-ReconcileHash $Path) -cne $expected){throw 'stopped published bytes changed'};return}
    $temp=$Path+'.stopped.tmp'
    if(Test-Path -LiteralPath $temp){
        $item=Get-Item -LiteralPath $temp -Force
        if($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)){throw 'stopped bytes temporary is unsafe'}
        Move-Item -LiteralPath $temp -Destination ($temp+'.interrupted-'+[guid]::NewGuid().ToString('N')) -ErrorAction Stop
    }
    Write-CatDeskDurableBytes $temp $Bytes
    if((Get-ReconcileHash $temp) -cne $expected){throw 'stopped bytes readback differs'}
    Move-Item -LiteralPath $temp -Destination $Path -ErrorAction Stop
}

function Copy-StoppedVerified([string]$Source, [string]$Destination, [string]$ExpectedHash, [string]$Root) {
    [void](Assert-CatDeskRecoveryContainedPath $Root $Source 'stopped copy source')
    [void](Assert-CatDeskRecoveryContainedPath $Root $Destination 'stopped copy output' -AllowMissingLeaf)
    if ((Get-ReconcileHash $Source) -cne $ExpectedHash) { throw 'stopped copy source changed' }
    if (Test-Path -LiteralPath $Destination) {
        if ((Get-ReconcileHash $Destination) -cne $ExpectedHash) { throw 'stopped published copy changed' }
        return
    }
    $temporary = $Destination+'.stopped.tmp'
    if (Test-Path -LiteralPath $temporary) {
        $item = Get-Item -LiteralPath $temporary -Force
        if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'stopped copy temporary is unsafe' }
        Move-Item -LiteralPath $temporary -Destination ($temporary+'.interrupted-'+[guid]::NewGuid().ToString('N')) -ErrorAction Stop
    }
    Copy-CatDeskDurableFile $Source $temporary
    if ((Get-ReconcileHash $temporary) -cne $ExpectedHash) { throw 'stopped copy readback differs' }
    Move-Item -LiteralPath $temporary -Destination $Destination -ErrorAction Stop
}

function Get-StoppedJournal($Paths, $Authority) {
    $j = Read-ReconcileJson $Paths.Root '.catdesk\release-pair-control\transaction.json' 4096
    Assert-ReconcileFields $j @('schemaVersion','mode','authorizationId','transactionId','phase','candidateSha256','originalBinarySha256','originalSidecarFileSha256')
    if ($j.schemaVersion -ne 2 -or $j.mode -cne 'QUALIFIED_STOPPED' -or
        $j.phase -cnotin @('PREPARING','STAGING','STAGED','COMMITTING','COMPLETED','ROLLED_BACK')) { throw 'stopped journal mode differs' }
    Assert-ReconcileJournalBinding $j $Authority.Record
    return $j
}

function Set-StoppedJournalPhase($Paths, $Journal, [string]$Next) {
    $Journal.phase = $Next
    Write-StoppedAtomic $Paths.Root $Paths.Journal ($Journal|ConvertTo-Json -Compress)
    if ((Get-StoppedJournal $Paths ([pscustomobject]@{Record=$Journal})).phase -cne $Next) { throw 'stopped journal readback differs' }
}

function Assert-StoppedEscrow($Paths, $Authority) {
    $a = $Authority.Record
    foreach($entry in @(@('original-catdesk.exe',$a.originalBinarySha256),@('original-catdesk.exe.sha256',$a.originalSidecarFileSha256),@('original-pointer.json',$a.originalPointerSha256))) {
        $path = Assert-CatDeskRecoveryContainedPath $Paths.Root (Join-Path $Paths.Escrow $entry[0]) 'stopped escrow'
        if ((Get-ReconcileHash $path) -cne $entry[1]) { throw 'stopped original escrow differs' }
    }
    foreach($entry in @(@('catdesk.exe',$a.slotABinarySha256),@('catdesk.exe.sha256',$a.slotASidecarFileSha256),@('manifest.json',$a.slotAManifestSha256))) {
        if ((Get-ReconcileContainedHash $Paths.Root (Join-Path $Paths.SlotA $entry[0]) 'legacy operational slot') -cne $entry[1]) { throw 'stopped legacy escrow differs' }
    }
}

function Invoke-StoppedStage($Paths, $Authority) {
    [void](Invoke-ReconcileEmpty $Paths $Authority)
    $a = $Authority.Record
    if (-not(Test-Path -LiteralPath $Paths.Journal)) {
        Assert-ReconcileOriginal $Paths $a
        $j = [ordered]@{schemaVersion=2;mode='QUALIFIED_STOPPED';authorizationId=$a.authorizationId;transactionId=$a.transactionId;phase='PREPARING';candidateSha256=$a.candidateSha256;originalBinarySha256=$a.originalBinarySha256;originalSidecarFileSha256=$a.originalSidecarFileSha256}
        Write-StoppedJson $Paths.Journal $j -Root $Paths.Root
    }
    $journal = Get-StoppedJournal $Paths $Authority
    if ($journal.phase -ceq 'ROLLED_BACK') { throw 'stopped transaction is already rolled back' }
    if ($journal.phase -ceq 'PREPARING') {
        Assert-ReconcileOriginal $Paths $a
        Copy-StoppedVerified $Paths.Binary (Join-Path $Paths.Escrow 'original-catdesk.exe') $a.originalBinarySha256 -Root $Paths.Root
        Copy-StoppedVerified $Paths.Sidecar (Join-Path $Paths.Escrow 'original-catdesk.exe.sha256') $a.originalSidecarFileSha256 -Root $Paths.Root
        Copy-StoppedVerified $Paths.Pointer (Join-Path $Paths.Escrow 'original-pointer.json') $a.originalPointerSha256 -Root $Paths.Root
        Invoke-StoppedCheckpoint 'ESCROW_COMPLETE'
        Set-StoppedJournalPhase $Paths $journal 'STAGING'
        $journal = Get-StoppedJournal $Paths $Authority
    }
    Assert-StoppedEscrow $Paths $Authority
    if ($journal.phase -cin @('STAGING','STAGED') -and (Get-ReconcileHash $Paths.Pointer) -cne $a.originalPointerSha256) {throw 'stopped precommit pointer changed'}
    $binaryHash = Get-ReconcileContainedHash $Paths.Root $Paths.Binary 'stopped canonical binary'
    $sidecarHash = Get-ReconcileContainedHash $Paths.Root $Paths.Sidecar 'stopped canonical sidecar'
    $newSidecar = [Text.UTF8Encoding]::new($false).GetBytes("$($a.candidateSha256)`n")
    $sha = [Security.Cryptography.SHA256]::Create()
    try{$newSidecarHash=([BitConverter]::ToString($sha.ComputeHash($newSidecar)).Replace('-','')).ToLowerInvariant()}finally{$sha.Dispose()}
    if ($binaryHash -cnotin @($a.originalBinarySha256,$a.candidateSha256) -or $sidecarHash -cnotin @($a.originalSidecarFileSha256,$newSidecarHash)) { throw 'stopped partial pair contains unknown bytes' }
    if ($journal.phase -ceq 'STAGING') {
        if ($binaryHash -cne $a.candidateSha256) {
            $temp = $Paths.Binary+'.reconcile-stopped.tmp'
            Copy-StoppedVerified $Authority.Candidate $temp $a.candidateSha256 -Root $Paths.Root
            [IO.File]::Replace($temp,$Paths.Binary,(New-StoppedReplacementBackup $Paths $Authority 'b'),$true)
            Invoke-StoppedCheckpoint 'BINARY_REPLACED'
        }
        if ($sidecarHash -cne $newSidecarHash) {
            $temp = $Paths.Sidecar+'.reconcile-stopped.tmp'
            Write-StoppedBytes $temp $newSidecar -Root $Paths.Root
            [IO.File]::Replace($temp,$Paths.Sidecar,(New-StoppedReplacementBackup $Paths $Authority 's'),$true)
            Invoke-StoppedCheckpoint 'SIDECAR_REPLACED'
        }
        if ((Get-CatDeskCanonicalPairEvidence $Paths.Root).Hash -cne $a.candidateSha256) { throw 'stopped canonical pair readback failed' }
        Set-StoppedJournalPhase $Paths $journal 'STAGED'
        Invoke-StoppedCheckpoint 'STAGED'
    } elseif ((Get-CatDeskCanonicalPairEvidence $Paths.Root).Hash -cne $a.candidateSha256) { throw 'stopped staged pair drifted' }
    return [pscustomobject]@{state='STAGED';transactionId=$a.transactionId}
}

function Get-StoppedQualification($Paths, $Authority) {
    $a=$Authority.Record
    $q=Read-ReconcileJson $Paths.Root '.catdesk\release-pair-control\qualification.json' 4096
    Assert-ReconcileFields $q @('schemaVersion','mode','profile','authorizationId','transactionId','candidateSha256','ownerPid','ownerStartedAtUtc','unauthorizedStatus','authorizedStatus','initializeVerified','toolsListVerified','toolCount','qualifiedAtUnix')
    if ($q.schemaVersion -ne 2 -or $q.mode -cne 'QUALIFIED_STOPPED' -or $q.profile -cne 'HEADLESS_LOCAL' -or
        $q.authorizationId -cne $a.authorizationId -or $q.transactionId -cne $a.transactionId -or $q.candidateSha256 -cne $a.candidateSha256 -or
        $q.ownerPid -lt 1 -or -not $q.ownerStartedAtUtc -or $q.unauthorizedStatus -ne 401 -or $q.authorizedStatus -ne 200 -or
        $q.initializeVerified -isnot [bool] -or -not $q.initializeVerified -or $q.toolsListVerified -isnot [bool] -or -not $q.toolsListVerified -or $q.toolCount -lt 2 -or $q.qualifiedAtUnix -lt 1) { throw 'stopped MCP qualification is incomplete' }
    return $q
}

function Assert-StoppedExpectedText([string]$Root,[string]$Path,[string]$Text) {
    [void](Assert-CatDeskRecoveryContainedPath $Root $Path 'stopped planned output' -AllowMissingLeaf)
    if((Test-Path -LiteralPath $Path) -and [IO.File]::ReadAllText($Path) -cne $Text){throw 'stopped planned output contains unknown bytes'}
}

function Invoke-StoppedCommit($Paths, $Authority) {
    [void](Invoke-ReconcileEmpty $Paths $Authority)
    Assert-StoppedEscrow $Paths $Authority
    $j=Get-StoppedJournal $Paths $Authority
    if ($j.phase -cnotin @('STAGED','COMMITTING','COMPLETED')) {throw 'stopped commit phase differs'}
    $a=$Authority.Record
    if ((Get-CatDeskCanonicalPairEvidence $Paths.Root).Hash -cne $a.candidateSha256) {throw 'stopped candidate pair changed'}
    $qualified=Get-StoppedQualification $Paths $Authority
    $q=Read-ReconcileJson $Paths.Root '.catdesk\release-pair-control\stopped-quiescence.json' 4096
    Assert-ReconcileFields $q @('schemaVersion','mode','state','authorizationId','transactionId','qualificationSha256','candidateJob','helperJob')
    if ($q.schemaVersion -ne 2 -or $q.mode -cne 'QUALIFIED_STOPPED' -or $q.state -cne 'OWNED_TREE_EXITED' -or
        $q.authorizationId -cne $a.authorizationId -or $q.transactionId -cne $a.transactionId -or
        $q.qualificationSha256 -cne (Get-ReconcileHash (Join-Path $Paths.Control 'qualification.json')) -or
        $q.candidateJob -cne "Local\CatDesk.ReleasePairStopped.$($a.transactionId).candidate" -or
        $q.helperJob -cne "Local\CatDesk.ReleasePairStopped.$($a.transactionId).helper") {throw 'stopped whole-tree proof differs'}
    $receipt=[ordered]@{schemaVersion=2;mode='QUALIFIED_STOPPED';state='QUALIFIED_STOPPED';profile='HEADLESS_LOCAL';authorizationId=$a.authorizationId;transactionId=$a.transactionId;candidateSha256=$a.candidateSha256;qualificationSha256=$q.qualificationSha256;quiescenceSha256=(Get-ReconcileHash (Join-Path $Paths.Control 'stopped-quiescence.json'))}
    $manifest=[ordered]@{schemaVersion=1;generation=2;sha256=$a.candidateSha256;source='reviewed_qualified_stopped';capturedAtUtc=([DateTimeOffset]::FromUnixTimeSeconds([long]$qualified.qualifiedAtUnix).UtcDateTime.ToString('o'))}
    Assert-StoppedExpectedText $Paths.Root (Join-Path $Paths.Control 'qualified-stopped.json') ($receipt|ConvertTo-Json -Compress)
    Assert-StoppedExpectedText $Paths.Root (Join-Path $Paths.SlotB 'manifest.json') ($manifest|ConvertTo-Json -Compress)
    Assert-StoppedExpectedText $Paths.Root (Join-Path $Paths.SlotB 'catdesk.exe.sha256') "$($a.candidateSha256)`n"
    $slotBinary=Assert-CatDeskRecoveryContainedPath $Paths.Root (Join-Path $Paths.SlotB 'catdesk.exe') 'stopped slot binary' -AllowMissingLeaf
    if((Test-Path -LiteralPath $slotBinary) -and (Get-ReconcileHash $slotBinary) -cne $a.candidateSha256){throw 'stopped slot binary contains unknown bytes'}
    [void](Assert-CatDeskRecoveryContainedPath $Paths.Root $Paths.Pointer 'stopped pointer')
    $oldHash=Get-ReconcileHash $Paths.Pointer
    if ($oldHash -cne $a.originalPointerSha256) {
        $actual=ConvertFrom-CatDeskLiteralJson ([IO.File]::ReadAllText($Paths.Pointer))
        Assert-ReconcileFields $actual @('schemaVersion','slot','generation','sha256')
        if ($actual.schemaVersion -ne 1 -or $actual.slot -cne 'slot-b' -or $actual.generation -ne 2 -or $actual.sha256 -cne $a.candidateSha256) {throw 'stopped pointer contains unknown state'}
    }
    if ($j.phase -ceq 'STAGED') {Set-StoppedJournalPhase $Paths $j 'COMMITTING'}
    Write-StoppedJson (Join-Path $Paths.Control 'qualified-stopped.json') $receipt -Root $Paths.Root
    Invoke-StoppedCheckpoint 'RECEIPT_WRITTEN'
    Copy-StoppedVerified $Authority.Candidate (Join-Path $Paths.SlotB 'catdesk.exe') $a.candidateSha256 -Root $Paths.Root
    Invoke-StoppedCheckpoint 'SLOT_BINARY_WRITTEN'
    $sidecar = Join-Path $Paths.SlotB 'catdesk.exe.sha256'
    if (Test-Path -LiteralPath $sidecar) {if ([IO.File]::ReadAllText($sidecar) -cne "$($a.candidateSha256)`n") {throw 'stopped slot sidecar differs'}}
    else {Write-StoppedAtomic $Paths.Root $sidecar "$($a.candidateSha256)`n"}
    Write-StoppedJson (Join-Path $Paths.SlotB 'manifest.json') $manifest -Root $Paths.Root
    Invoke-StoppedCheckpoint 'SLOT_METADATA_WRITTEN'
    $pointer=[ordered]@{schemaVersion=1;slot='slot-b';generation=2;sha256=$a.candidateSha256}
    if ($oldHash -cne $a.originalPointerSha256) {
        $actual=ConvertFrom-CatDeskLiteralJson ([IO.File]::ReadAllText($Paths.Pointer))
        Assert-ReconcileFields $actual @('schemaVersion','slot','generation','sha256')
        if ($actual.schemaVersion -ne 1 -or $actual.slot -cne 'slot-b' -or $actual.generation -ne 2 -or $actual.sha256 -cne $a.candidateSha256) {throw 'stopped pointer contains unknown state'}
    } else {Write-StoppedAtomic $Paths.Root $Paths.Pointer ($pointer|ConvertTo-Json -Compress)}
    Invoke-StoppedCheckpoint 'POINTER_WRITTEN'
    $j=Get-StoppedJournal $Paths $Authority
    if ($j.phase -cne 'COMPLETED') {Set-StoppedJournalPhase $Paths $j 'COMPLETED'}
    Invoke-StoppedCheckpoint 'JOURNAL_COMPLETED'
    $accepted=Get-CatDeskLastKnownGoodRelease $Paths.Root
    if ($accepted.Hash -cne $a.candidateSha256 -or $accepted.Slot -cne 'slot-b') {throw 'stopped release trust readback failed'}
    return [pscustomobject]@{state='QUALIFIED_STOPPED';transactionId=$a.transactionId}
}

function Invoke-StoppedRollback($Paths, $Authority) {
    [void](Invoke-ReconcileEmpty $Paths $Authority)
    $a=$Authority.Record;$j=Get-StoppedJournal $Paths $Authority
    if($j.phase -cnotin @('PREPARING','STAGING','STAGED','ROLLED_BACK') -or
        (Test-Path -LiteralPath $Paths.SlotB) -or (Test-Path -LiteralPath (Join-Path $Paths.Control 'qualified-stopped.json'))){throw 'stopped rollback is beyond the unreleased boundary'}
    $exit=Read-ReconcileJson $Paths.Root '.catdesk\release-pair-control\stopped-recovery-exit.json' 4096
    Assert-ReconcileFields $exit @('schemaVersion','mode','state','authorizationId','transactionId','candidateJob','helperJob')
    if($exit.schemaVersion -ne 2 -or $exit.mode -cne 'QUALIFIED_STOPPED' -or $exit.state -cne 'OWNED_TREE_EXITED' -or
        $exit.authorizationId -cne $a.authorizationId -or $exit.transactionId -cne $a.transactionId -or
        $exit.candidateJob -cne "Local\CatDesk.ReleasePairStopped.$($a.transactionId).candidate" -or
        $exit.helperJob -cne "Local\CatDesk.ReleasePairStopped.$($a.transactionId).helper"){throw 'stopped rollback exit proof differs'}
    if($j.phase -ceq 'PREPARING' -or $j.phase -ceq 'ROLLED_BACK'){Assert-ReconcileOriginal $Paths $a}
    else {
        Assert-StoppedEscrow $Paths $Authority
        $binaryHash=Get-ReconcileHash $Paths.Binary
        $sidecarHash=Get-ReconcileHash $Paths.Sidecar
        $sha=[Security.Cryptography.SHA256]::Create()
        try{$newSidecarHash=([BitConverter]::ToString($sha.ComputeHash([Text.UTF8Encoding]::new($false).GetBytes("$($a.candidateSha256)`n"))).Replace('-','')).ToLowerInvariant()}finally{$sha.Dispose()}
        if($binaryHash -cnotin @($a.originalBinarySha256,$a.candidateSha256) -or
            $sidecarHash -cnotin @($a.originalSidecarFileSha256,$newSidecarHash) -or
            (Get-ReconcileHash $Paths.Pointer) -cne $a.originalPointerSha256){throw 'stopped rollback found unknown bytes'}
        foreach($entry in @(@($Paths.Binary,'original-catdesk.exe',$a.originalBinarySha256),@($Paths.Sidecar,'original-catdesk.exe.sha256',$a.originalSidecarFileSha256))){
            if((Get-ReconcileHash $entry[0]) -cne $entry[2]){
                $temp=$entry[0]+'.stopped-rollback.tmp'
                Copy-StoppedVerified (Join-Path $Paths.Escrow $entry[1]) $temp $entry[2] -Root $Paths.Root
                [IO.File]::Replace($temp,$entry[0],(New-StoppedReplacementBackup $Paths $Authority 'r'),$true)
            }
        }
        Assert-ReconcileOriginal $Paths $a
    }
    if($j.phase -cne 'ROLLED_BACK'){Set-StoppedJournalPhase $Paths $j 'ROLLED_BACK'}
    return [pscustomobject]@{state='RESTORED_ORIGINAL_STOPPED';transactionId=$a.transactionId}
}

if (-not $LibraryOnly) {
    if (-not $Workspace -or -not $Phase) { throw 'reconciliation requires a fixed phase and workspace' }
    $paths = Get-ReconcilePaths $Workspace
    $stopped = $Phase.StartsWith('Stopped',[StringComparison]::Ordinal)
    $authority = if($stopped){Get-StoppedReconcileAuthority $paths $AuthorizationId $TransactionId}else{Get-ReconcileAuthority $paths.Root $AuthorizationId $TransactionId ($Phase -eq 'Stage') ($Phase -in @('Stage','Probe','Commit'))}
    $result = switch ($Phase) {
        'Empty' { Invoke-ReconcileEmpty $paths $authority }
        'Stage' { Invoke-ReconcileStage $paths $authority }
        'Probe' { Invoke-ReconcileProbe $paths $authority $OwnerPid $OwnerStartedAtUtc }
        'Commit' { Invoke-ReconcileCommit $paths $authority $OwnerPid $OwnerStartedAtUtc }
        'Rollback' { Invoke-ReconcileRollback $paths $authority }
        'StoppedEmpty' { Invoke-ReconcileEmpty $paths $authority }
        'StoppedStage' { Invoke-StoppedStage $paths $authority }
        'StoppedProbe' {
            $j=Get-StoppedJournal $paths $authority
            if($j.phase -cne 'STAGED'){throw 'stopped qualification requires staged pair'}
            $proof=Get-ReconcileOwnerProof $paths.Root $authority.Record.candidateSha256 $OwnerPid $OwnerStartedAtUtc
            [pscustomobject]@{state='SERVING_VERIFIED';transactionId=$authority.Record.transactionId;ownerPid=$proof.pid;ownerStartedAtUtc=$proof.startedAtUtc;candidateSha256=$proof.imageSha256}
        }
        'StoppedCommit' { Invoke-StoppedCommit $paths $authority }
        'StoppedRollback' { Invoke-StoppedRollback $paths $authority }
    }
    $result | ConvertTo-Json -Compress
}
