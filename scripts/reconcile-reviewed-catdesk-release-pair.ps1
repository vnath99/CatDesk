[CmdletBinding()]
param(
    [string]$Workspace = '',
    [ValidateSet('Empty','Stage','Probe','Commit','Rollback')][string]$Phase = '',
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
    return [IO.File]::ReadAllText($path) | ConvertFrom-Json -ErrorAction Stop
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

if (-not $LibraryOnly) {
    if (-not $Workspace -or -not $Phase) { throw 'reconciliation requires a fixed phase and workspace' }
    $paths = Get-ReconcilePaths $Workspace
    $authority = Get-ReconcileAuthority $paths.Root $AuthorizationId $TransactionId ($Phase -eq 'Stage') ($Phase -in @('Stage','Probe','Commit'))
    $result = switch ($Phase) {
        'Empty' { Invoke-ReconcileEmpty $paths $authority }
        'Stage' { Invoke-ReconcileStage $paths $authority }
        'Probe' { Invoke-ReconcileProbe $paths $authority $OwnerPid $OwnerStartedAtUtc }
        'Commit' { Invoke-ReconcileCommit $paths $authority $OwnerPid $OwnerStartedAtUtc }
        'Rollback' { Invoke-ReconcileRollback $paths $authority }
    }
    $result | ConvertTo-Json -Compress
}
