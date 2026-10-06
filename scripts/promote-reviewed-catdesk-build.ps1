[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$BuildPath,
    [string]$Workspace = '',
    [switch]$Execute,
    [switch]$ValidatePhase1,
    [string]$AuthorizationToken = '',
    [string]$TransactionId = '',
    [switch]$RecoveryRollbackOnly,
    [hashtable]$TestSeams = @{}
)

# Maintainer-only release promotion. Plan mode is deliberately the default.
# This script does not read configuration, credentials, command lines, browser
# state, or tunnel state, and it never owns the external Secure MCP runtime.
$ErrorActionPreference = 'Stop'
if (-not $Workspace) { $Workspace = Join-Path $PSScriptRoot '..' }
if ($Execute -and $ValidatePhase1) { throw 'ValidatePhase1 and Execute are mutually exclusive' }
$releaseRecoveryHelper = Join-Path $PSScriptRoot 'catdesk-release-recovery.ps1'
if (-not (Test-Path -LiteralPath $releaseRecoveryHelper -PathType Leaf)) { throw 'release recovery helper is unavailable' }
. $releaseRecoveryHelper
$script:PromotionSeams = $TestSeams
$script:PromotionMcpPort = 3200
$script:PromotionTimeoutSeconds = 120
$script:Phase1ResumeClockSkewToleranceSeconds = 30
$script:Phase1ResumeMaximumStartLeadSeconds = 180
$script:Phase1ResumeProcessStartMatchToleranceSeconds = 5

function Invoke-PromotionSeam {
    param([string]$Name, [object[]]$Arguments, [scriptblock]$Default)
    if ($script:PromotionSeams.ContainsKey($Name)) { return & $script:PromotionSeams[$Name] @Arguments }
    return & $Default @Arguments
}

function Get-PromotionHash([string]$Path) {
    (Get-FileHash -LiteralPath $Path -Algorithm SHA256 -ErrorAction Stop).Hash.ToLowerInvariant()
}

function Get-ContainedRegularFile([string]$Root, [string]$Path, [string]$Label) {
    $inputPath = [IO.Path]::GetFullPath($Path)
    $rootPrefix = $Root.TrimEnd('\') + '\'
    if (-not $inputPath.StartsWith($rootPrefix, [StringComparison]::OrdinalIgnoreCase)) { throw "$Label is outside the approved workspace" }
    $relative = $inputPath.Substring($rootPrefix.Length)
    $cursor = $Root
    foreach ($segment in $relative.Split('\', [StringSplitOptions]::RemoveEmptyEntries)) {
        $cursor = Join-Path $cursor $segment
        $traversed = Get-Item -LiteralPath $cursor -Force -ErrorAction Stop
        if ($traversed.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "$Label traverses a reparse point" }
    }
    $resolved = (Resolve-Path -LiteralPath $Path -ErrorAction Stop).Path
    $item = Get-Item -LiteralPath $resolved -Force -ErrorAction Stop
    if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw "$Label is not a regular workspace file" }
    if (-not $resolved.StartsWith($rootPrefix, [StringComparison]::OrdinalIgnoreCase)) { throw "$Label is outside the approved workspace" }
    return $resolved
}

function Get-CanonicalReleaseEvidence([string]$Root) {
    $releaseDirectory = Join-Path $Root 'target\release'
    $binary = Get-ContainedRegularFile -Root $Root -Path (Join-Path $releaseDirectory 'catdesk.exe') -Label 'canonical release binary'
    $manifest = Get-ContainedRegularFile -Root $Root -Path (Join-Path $releaseDirectory 'catdesk.exe.sha256') -Label 'canonical release manifest'
    $manifestItem = Get-Item -LiteralPath $manifest -ErrorAction Stop
    if ($manifestItem.Length -gt 128) { throw 'canonical release manifest is invalid' }
    $expected = [IO.File]::ReadAllText($manifest).Trim()
    if ($expected -notmatch '^[A-Fa-f0-9]{64}$') { throw 'canonical release manifest is invalid' }
    $actual = Get-PromotionHash $binary
    if ($actual -ne $expected.ToLowerInvariant()) { throw 'canonical release manifest does not match the binary' }
    [pscustomobject]@{ Binary = $binary; Manifest = $manifest; Hash = $actual }
}

function Get-PromotionBackupEvidence([string]$Root) {
    $directory = Join-Path $Root '.catdesk\promotion-recovery'
    if (-not (Test-Path -LiteralPath $directory)) { return $null }
    $item = Get-Item -LiteralPath $directory -Force -ErrorAction Stop
    if (-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'promotion backup location is invalid' }
    $binary = Join-Path $directory 'previous-catdesk.exe'
    $manifest = Join-Path $directory 'previous-catdesk.exe.sha256'
    $hasBinary = Test-Path -LiteralPath $binary -PathType Leaf
    $hasManifest = Test-Path -LiteralPath $manifest -PathType Leaf
    if ($hasBinary -ne $hasManifest) { throw 'promotion backup is incomplete' }
    if (-not $hasBinary) { return $null }
    $resolvedBinary = Get-ContainedRegularFile -Root $Root -Path $binary -Label 'promotion backup binary'
    $resolvedManifest = Get-ContainedRegularFile -Root $Root -Path $manifest -Label 'promotion backup manifest'
    $expected = [IO.File]::ReadAllText($resolvedManifest).Trim()
    if ($expected -notmatch '^[A-Fa-f0-9]{64}$' -or (Get-PromotionHash $resolvedBinary) -ne $expected.ToLowerInvariant()) { throw 'promotion backup is invalid' }
    [pscustomobject]@{ Binary = $resolvedBinary; Manifest = $resolvedManifest; Hash = $expected.ToLowerInvariant() }
}

function Get-PromotionRecoveryDirectory([string]$Root) {
    $controlDirectory = Join-Path $Root '.catdesk'
    if (Test-Path -LiteralPath $controlDirectory) {
        $controlItem = Get-Item -LiteralPath $controlDirectory -Force -ErrorAction Stop
        if (-not $controlItem.PSIsContainer -or ($controlItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'promotion recovery location is invalid' }
    }
    $directory = Join-Path $Root '.catdesk\promotion-recovery'
    if (Test-Path -LiteralPath $directory) {
        $item = Get-Item -LiteralPath $directory -Force -ErrorAction Stop
        if (-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'promotion recovery location is invalid' }
    }
    return $directory
}

function Get-WorkspaceRelativePath([string]$Root, [string]$Path, [string]$Label) {
    $resolvedRoot = [IO.Path]::GetFullPath($Root).TrimEnd('\')
    $resolvedPath = [IO.Path]::GetFullPath($Path)
    $prefix = $resolvedRoot + '\'
    if (-not $resolvedPath.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw "$Label is outside the approved workspace"
    }
    return $resolvedPath.Substring($prefix.Length)
}

function Get-PromotionTransaction([string]$Root) {
    $path = Join-Path (Get-PromotionRecoveryDirectory $Root) 'promotion-transaction.json'
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { return $null }
    try {
        $item = Get-Item -LiteralPath $path -Force -ErrorAction Stop
        if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint -or $item.Length -gt 2048) { throw 'invalid' }
        $record = Get-Content -LiteralPath $path -Raw -ErrorAction Stop | ConvertFrom-Json -ErrorAction Stop
        $names = @($record.PSObject.Properties.Name)
        $requiredV1 = @('schemaVersion', 'phase', 'priorHash', 'candidateHash', 'candidateRelativePath')
        $requiredV2 = @('schemaVersion', 'transactionId', 'authorizationId', 'phase', 'priorHash', 'candidateHash', 'candidateRelativePath')
        $required = if ([int]$record.schemaVersion -eq 2) { $requiredV2 } else { $requiredV1 }
        if ($names.Count -ne $required.Count -or @($names | Where-Object { $_ -notin $required }).Count -ne 0) { throw 'invalid' }
        if (($record.schemaVersion -isnot [int] -and $record.schemaVersion -isnot [long]) -or $record.schemaVersion -notin @(1, 2) -or $record.phase -ne 'CANONICAL_MUTATION_STARTED') { throw 'invalid' }
        if ([int]$record.schemaVersion -eq 2 -and ([string]$record.transactionId -notmatch '^[0-9a-f]{32}$' -or [string]$record.authorizationId -notmatch '^[0-9a-f]{32}$')) { throw 'invalid' }
        if ([string]$record.priorHash -notmatch '^[a-f0-9]{64}$' -or [string]$record.candidateHash -notmatch '^[a-f0-9]{64}$') { throw 'invalid' }
        $relative = [string]$record.candidateRelativePath
        if (-not $relative -or [IO.Path]::IsPathRooted($relative) -or $relative -match '(^|[\\/])\.\.([\\/]|$)' -or $relative -match '[\x00-\x1f]') { throw 'invalid' }
        [pscustomobject]@{ Path = $path; SchemaVersion = [int]$record.schemaVersion; TransactionId = if ([int]$record.schemaVersion -eq 2) { [string]$record.transactionId } else { $null }; AuthorizationId = if ([int]$record.schemaVersion -eq 2) { [string]$record.authorizationId } else { $null }; PriorHash = [string]$record.priorHash; CandidateHash = [string]$record.candidateHash; CandidateRelativePath = $relative }
    } catch { throw 'promotion transaction is invalid' }
}

function Write-PromotionTransaction([string]$Root, [string]$PriorHash, [string]$CandidateHash, [string]$CandidatePath) {
    $relative = Get-WorkspaceRelativePath -Root $Root -Path $CandidatePath -Label 'promotion candidate'
    if ([IO.Path]::IsPathRooted($relative) -or $relative -match '(^|[\\/])\.\.([\\/]|$)') { throw 'promotion candidate identity is invalid' }
    $directory = Get-PromotionRecoveryDirectory $Root
    New-Item -ItemType Directory -Path $directory -Force | Out-Null
    $path = Join-Path $directory 'promotion-transaction.json'
    $temporary = Join-Path $directory '.promotion-transaction.tmp'
    if (Test-Path -LiteralPath $temporary) { throw 'promotion transaction temporary state is unsafe' }
    if ([string]$script:PromotionAuthorization.transactionId -notmatch '^[0-9a-f]{32}$') { throw 'promotion transaction identity is unavailable' }
    $record = [ordered]@{ schemaVersion = 2; transactionId = [string]$script:PromotionAuthorization.transactionId; authorizationId = $script:PromotionAuthorization.authorizationId; phase = 'CANONICAL_MUTATION_STARTED'; priorHash = $PriorHash; candidateHash = $CandidateHash; candidateRelativePath = $relative }
    $bytes = [Text.UTF8Encoding]::new($false).GetBytes(($record | ConvertTo-Json -Compress))
    $stream = [IO.FileStream]::new($temporary, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    try { $stream.Write($bytes, 0, $bytes.Length); $stream.Flush($true) } finally { $stream.Dispose() }
    Move-Item -LiteralPath $temporary -Destination $path -Force
    return (Get-PromotionTransaction $Root)
}

function Get-ProtectedPromotionAuthorization([string]$Root, [string]$Token, [string]$TransactionId, [string]$Candidate, [string]$CandidateHash, [string]$PriorHash) {
    $seamed = Invoke-PromotionSeam -Name 'ProtectedAuthorization' -Arguments @($Root, $Token, $TransactionId, $Candidate, $CandidateHash, $PriorHash) -Default { param($ignoredRoot, $ignoredToken, $ignoredTransaction, $ignoredCandidate, $ignoredCandidateHash, $ignoredPriorHash) $null }
    if ($null -ne $seamed) { return $seamed }
    if ($Token -notmatch '^[0-9a-f]{32}$') { throw 'REVIEWED_PROMOTION_AUTHORIZATION_REQUIRED' }
    $path = Join-Path $Root '.catdesk\promotion-control\authorization.json'
    try {
        $path = Assert-CatDeskRecoveryContainedPath -Root $Root -Path $path -Label 'reviewed promotion authorization'
        $item = Get-Item -LiteralPath $path -Force -ErrorAction Stop
        if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -or $item.Length -lt 1 -or $item.Length -gt 4096) { throw 'invalid' }
        $record = [IO.File]::ReadAllText($path) | ConvertFrom-Json -ErrorAction Stop
        $required = @('schemaVersion','generation','authorizationId','expiresAtUnix','candidateRelativePath','candidateSha256','priorCanonicalSha256','promotionScriptSha256','trustedPowershellPath','trustedPowershellSha256','reviewSessionId','reviewRecordId','reviewRecordSha256','transactionId','claimId')
        $names = @($record.PSObject.Properties.Name)
        if ($names.Count -ne $required.Count -or @($names | Where-Object { $_ -notin $required }).Count -ne 0) { throw 'invalid' }
        if ($record.schemaVersion -ne 1 -or [string]$record.authorizationId -ne $Token -or [string]$record.transactionId -ne $TransactionId -or [string]$record.transactionId -notmatch '^[0-9a-f]{32}$' -or [int64]$record.generation -lt 1 -or [int64]$record.expiresAtUnix -lt [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()) { throw 'invalid' }
        if ([string]$record.candidateSha256 -ne $CandidateHash -or [string]$record.priorCanonicalSha256 -ne $PriorHash -or [string]$record.promotionScriptSha256 -ne (Get-PromotionHash $PSCommandPath)) { throw 'invalid' }
        if ([string]$record.candidateRelativePath -ne (Get-WorkspaceRelativePath -Root $Root -Path $Candidate -Label 'reviewed promotion candidate')) { throw 'invalid' }
        if (-not [string]$record.trustedPowershellPath -or [string]$record.trustedPowershellSha256 -notmatch '^[a-f0-9]{64}$' -or [string]$record.reviewSessionId -notmatch '^[A-Za-z0-9_-]{1,128}$' -or [string]$record.reviewRecordId -notmatch '^[A-Za-z0-9_-]{1,128}$' -or [string]$record.reviewRecordSha256 -notmatch '^[a-f0-9]{64}$') { throw 'invalid' }
        $claimPath = Assert-CatDeskRecoveryContainedPath -Root $Root -Path (Join-Path $Root '.catdesk\promotion-control\claim.json') -Label 'reviewed promotion claim'
        $claimItem = Get-Item -LiteralPath $claimPath -Force -ErrorAction Stop
        if ($claimItem.PSIsContainer -or ($claimItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -or $claimItem.Length -lt 1 -or $claimItem.Length -gt 1024) { throw 'invalid' }
        $claim = [IO.File]::ReadAllText($claimPath) | ConvertFrom-Json -ErrorAction Stop
        $claimRequired = @('schemaVersion','authorizationId','claimId','transactionId','candidateRelativePath','state')
        $claimNames = @($claim.PSObject.Properties.Name)
        if ($claimNames.Count -ne $claimRequired.Count -or @($claimNames | Where-Object { $_ -notin $claimRequired }).Count -ne 0 -or $claim.schemaVersion -ne 1 -or [string]$claim.authorizationId -ne [string]$record.authorizationId -or [string]$claim.claimId -ne [string]$record.claimId -or [string]$claim.transactionId -ne $TransactionId -or [string]$claim.candidateRelativePath -ne [string]$record.candidateRelativePath -or [string]$claim.state -ne 'CLAIMED_PENDING') { throw 'invalid' }
        return $record
    } catch { throw 'REVIEWED_PROMOTION_AUTHORIZATION_REQUIRED' }
}

function Write-ReviewedPromotionAuthority([string]$Root, $Transaction, $Canonical) {
    if ($null -eq $Transaction -or $Transaction.SchemaVersion -ne 2 -or [string]$Transaction.TransactionId -notmatch '^[0-9a-f]{32}$' -or [string]$Transaction.AuthorizationId -ne [string]$script:PromotionAuthorization.authorizationId) { throw 'reviewed promotion authority is unavailable' }
    $canonicalEvidence = Get-CatDeskCanonicalPairEvidence $Root
    if ($null -eq $Canonical -or [string]$Canonical.Hash -ne $canonicalEvidence.Hash -or [string]$Transaction.CandidateHash -ne $canonicalEvidence.Hash) { throw 'reviewed promotion authority did not bind the canonical pair' }
    $directory = Get-PromotionRecoveryDirectory $Root
    New-Item -ItemType Directory -Path $directory -Force -ErrorAction Stop | Out-Null
    $path = Join-Path $directory 'reviewed-promotion.json'
    $record = [ordered]@{
        schemaVersion = 1
        transactionId = [string]$Transaction.TransactionId
        authorizationId = [string]$script:PromotionAuthorization.authorizationId
        stage = 'CANONICAL_HANDOFF_PROVEN'
        candidateHash = [string]$Transaction.CandidateHash
        canonicalRelativePath = 'target\release\catdesk.exe'
        canonicalSha256 = [string]$canonicalEvidence.Hash
    }
    Write-CatDeskAtomicUtf8 -Path $path -Text ($record | ConvertTo-Json -Compress)
    [void](Get-CatDeskReviewedPromotionAuthority -Root $Root -Canonical $canonicalEvidence)
}

function Clear-PromotionTransaction([string]$Root) {
    $path = Join-Path (Get-PromotionRecoveryDirectory $Root) 'promotion-transaction.json'
    if (Test-Path -LiteralPath $path -PathType Leaf) { Remove-Item -LiteralPath $path -Force -ErrorAction Stop }
}

function Get-CanonicalPairDisposition([string]$Root, [string]$CandidateHash, [string]$PriorHash) {
    try {
        $current = Get-CanonicalReleaseEvidence $Root
        if ($current.Hash -eq $CandidateHash) { return [pscustomobject]@{ State = 'CANDIDATE'; Evidence = $current } }
        if ($current.Hash -eq $PriorHash) { return [pscustomobject]@{ State = 'PRIOR'; Evidence = $current } }
    } catch {}
    return [pscustomobject]@{ State = 'INCONSISTENT'; Evidence = $null }
}

function Invoke-PromotionCheckpoint([string]$Name) {
    Invoke-PromotionSeam -Name 'Checkpoint' -Arguments @($Name) -Default { param($ignored) }
}

function Write-PromotionResult([string]$State, [bool]$CandidateValidated, [bool]$CanonicalReleaseValidated, [bool]$PromotionRequired) {
    [pscustomobject][ordered]@{
        schemaVersion = 1
        command = 'promote-reviewed-catdesk-build'
        state = $State
        candidateValidated = $CandidateValidated
        canonicalReleaseValidated = $CanonicalReleaseValidated
        promotionRequired = $PromotionRequired
        tunnelAction = 'NONE'
    } | ConvertTo-Json -Compress
}

function Get-PromotionListener([string]$BuildPath, [string]$ExpectedHash) {
    Invoke-PromotionSeam -Name 'ListenerForBuild' -Arguments @($BuildPath, $ExpectedHash, $script:PromotionMcpPort) -Default {
        param($expectedPath, $expectedHash, $port)
        $listeners = @(Get-NetTCPConnection -State Listen -LocalPort $port -ErrorAction Stop |
            Where-Object { $_.LocalAddress -in @('127.0.0.1', '::1') } | Select-Object -First 3)
        if ($listeners.Count -lt 1 -or $listeners.Count -gt 2) { return $null }
        $ownerPids = @($listeners | ForEach-Object { [int]$_.OwningProcess } | Select-Object -Unique)
        if ($ownerPids.Count -ne 1 -or $ownerPids[0] -lt 1) { return $null }
        $process = Get-Process -Id ([int]$ownerPids[0]) -ErrorAction Stop
        $null = $process.Handle
        if ($process.ProcessName -ne 'catdesk' -or -not $process.Path) { return $null }
        $actualPath = (Resolve-Path -LiteralPath $process.Path -ErrorAction Stop).Path
        $actualStartedAtUtc = $process.StartTime.ToUniversalTime().ToString('o')
        if (-not $actualPath.Equals($expectedPath, [StringComparison]::OrdinalIgnoreCase)) { return $null }
        $actualHash = Get-PromotionHash $actualPath
        if ($actualHash -ne $expectedHash) { return $null }
        [pscustomobject]@{ Pid = [int]$process.Id; ProcessStartedAtUtc = $actualStartedAtUtc; BuildPath = $actualPath; BuildHash = $actualHash }
    }
}

function Get-Phase1HandoffRecord([string]$Root) {
    $controlDirectory = Join-Path $Root '.catdesk'
    $stateDirectory = Join-Path $controlDirectory 'restart-handoff'
    $statePath = Join-Path $Root '.catdesk\restart-handoff\latest.json'
    try {
        if (Test-Path -LiteralPath $controlDirectory) {
            $controlItem = Get-Item -LiteralPath $controlDirectory -Force -ErrorAction Stop
            if (-not $controlItem.PSIsContainer -or ($controlItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'restart handoff control location is invalid' }
        }
        if (-not (Test-Path -LiteralPath $stateDirectory)) {
            if (Test-Path -LiteralPath $statePath) { throw 'restart handoff state location is invalid' }
            return [pscustomobject]@{ Exists = $false; Record = $null }
        }
        $directoryItem = Get-Item -LiteralPath $stateDirectory -Force -ErrorAction Stop
        if (-not $directoryItem.PSIsContainer -or ($directoryItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'restart handoff state location is invalid' }
        if (-not (Test-Path -LiteralPath $statePath)) {
            return [pscustomobject]@{ Exists = $false; Record = $null }
        }
        if (-not (Test-Path -LiteralPath $statePath -PathType Leaf)) { throw 'restart handoff evidence is not a regular file' }
    } catch {
        return [pscustomobject]@{ Exists = $true; Record = $null }
    }
    try {
        $stateFile = Get-ContainedRegularFile -Root $Root -Path $statePath -Label 'restart handoff evidence'
        $item = Get-Item -LiteralPath $stateFile -Force -ErrorAction Stop
        if ($item.Length -gt 4096) { throw 'restart handoff evidence is oversized' }
        $record = Get-Content -LiteralPath $stateFile -Raw -ErrorAction Stop | ConvertFrom-Json -ErrorAction Stop
        return [pscustomobject]@{ Exists = $true; Record = $record }
    } catch {
        # A pre-existing but unusable handoff record is an unsafe resume
        # condition. Leave it intact for the operator; do not start a second
        # candidate daemon from ambiguous evidence.
        return [pscustomobject]@{ Exists = $true; Record = $null }
    }
}

function Test-Phase1HandoffRecord($Record, [string]$Root, [string]$Candidate, [string]$CandidateHash) {
    if ($null -eq $Record) { return $false }
    $version = $Record.schemaVersion
    $names = @($Record.PSObject.Properties.Name)
    $nativeRequired = @('schemaVersion', 'status', 'completedAtUnix', 'oldPid', 'newPid', 'port', 'replacementSha256', 'stage', 'rollbackAttempted', 'rollbackReady')
    $isNative = $names.Count -eq $nativeRequired.Count -and @($names | Where-Object { $_ -notin $nativeRequired }).Count -eq 0
    if ($isNative) {
        if (($version -isnot [int] -and $version -isnot [long]) -or $version -ne 1) { return $false }
        if ($Record.status -isnot [string] -or $Record.stage -isnot [string]) { return $false }
        if (($Record.completedAtUnix -isnot [int] -and $Record.completedAtUnix -isnot [long]) -or $Record.completedAtUnix -lt 1) { return $false }
        if (($Record.oldPid -isnot [int] -and $Record.oldPid -isnot [long]) -or ($Record.newPid -isnot [int] -and $Record.newPid -isnot [long])) { return $false }
        if (($Record.port -isnot [int] -and $Record.port -isnot [long]) -or $Record.oldPid -lt 1 -or $Record.newPid -lt 1) { return $false }
        if ($Record.rollbackAttempted -isnot [bool] -or $Record.rollbackReady -isnot [bool]) { return $false }
        if ($Record.status -ne 'REPLACEMENT_READY_PENDING_TRANSPORT_RECONNECT' -or $Record.stage -ne 'complete' -or $Record.port -ne $script:PromotionMcpPort) { return $false }
        if ($Record.rollbackAttempted -or $Record.rollbackReady) { return $false }
        if ($Record.replacementSha256 -isnot [string] -or $Record.replacementSha256 -notmatch '^[a-f0-9]{64}$' -or -not $Record.replacementSha256.Equals($CandidateHash, [StringComparison]::Ordinal)) { return $false }
        try { $completed = [DateTimeOffset]::FromUnixTimeSeconds([long]$Record.completedAtUnix) } catch { return $false }
        if ($completed -gt [DateTimeOffset]::UtcNow.AddSeconds($script:Phase1ResumeClockSkewToleranceSeconds)) { return $false }
        return $true
    }

    $requiredV1 = @('schemaVersion', 'status', 'completedAtUtc', 'oldPid', 'newPid', 'mcpPort', 'oldListenerPorts', 'buildSha256', 'stage', 'recovery')
    $requiredV2 = @('schemaVersion', 'status', 'completedAtUtc', 'oldPid', 'newPid', 'mcpPort', 'oldListenerPorts', 'workspacePath', 'buildPath', 'buildSha256', 'stage', 'recovery')
    $requiredV3 = @('schemaVersion', 'status', 'completedAtUtc', 'oldPid', 'newPid', 'newProcessStartedAtUtc', 'mcpPort', 'oldListenerPorts', 'workspacePath', 'buildPath', 'buildSha256', 'stage', 'recovery')
    $expected = if ($version -eq 1) { $requiredV1 } elseif ($version -eq 2) { $requiredV2 } elseif ($version -eq 3) { $requiredV3 } else { return $false }
    if ($names.Count -ne $expected.Count -or @($names | Where-Object { $_ -notin $expected }).Count -ne 0) { return $false }
    if (($version -isnot [int] -and $version -isnot [long]) -or $Record.status -isnot [string] -or $Record.stage -isnot [string] -or $Record.recovery -isnot [string]) { return $false }
    if (($Record.mcpPort -isnot [int] -and $Record.mcpPort -isnot [long]) -or $null -eq $Record.oldListenerPorts -or $Record.oldListenerPorts -is [string]) { return $false }
    if ($Record.status -ne 'RECOVERED_PENDING_TRANSPORT_CHECK' -or $Record.stage -ne 'complete' -or $Record.mcpPort -ne $script:PromotionMcpPort) { return $false }
    if ($Record.buildSha256 -isnot [string] -or $Record.buildSha256 -notmatch '^[a-f0-9]{64}$' -or -not $Record.buildSha256.Equals($CandidateHash, [StringComparison]::Ordinal)) { return $false }
    if ($Record.oldPid -isnot [int] -and $Record.oldPid -isnot [long]) { return $false }
    if ($Record.newPid -isnot [int] -and $Record.newPid -isnot [long]) { return $false }
    if ($Record.oldPid -lt 1 -or $Record.newPid -lt 1) { return $false }
    if ($Record.completedAtUtc -isnot [string]) { return $false }
    $completed = [DateTimeOffset]::MinValue
    if (-not [DateTimeOffset]::TryParse($Record.completedAtUtc, [Globalization.CultureInfo]::InvariantCulture, [Globalization.DateTimeStyles]::RoundtripKind, [ref]$completed)) { return $false }
    if ($completed.ToUniversalTime() -gt [DateTimeOffset]::UtcNow.AddSeconds($script:Phase1ResumeClockSkewToleranceSeconds)) { return $false }
    if ($version -ge 2) {
        try {
            $expectedRoot = [IO.Path]::GetFullPath($Root).TrimEnd('\\')
            $expectedCandidate = [IO.Path]::GetFullPath($Candidate)
            if (-not [IO.Path]::GetFullPath([string]$Record.workspacePath).TrimEnd('\\').Equals($expectedRoot, [StringComparison]::OrdinalIgnoreCase)) { return $false }
            if (-not [IO.Path]::GetFullPath([string]$Record.buildPath).Equals($expectedCandidate, [StringComparison]::OrdinalIgnoreCase)) { return $false }
        } catch { return $false }
    }
    return $true
}

function Get-Phase1ListenerRecords {
    Invoke-PromotionSeam -Name 'Phase1ListenerRecords' -Arguments @($script:PromotionMcpPort) -Default {
        param($port)
        $listeners = @(Get-NetTCPConnection -State Listen -LocalPort $port -ErrorAction Stop |
            Where-Object { $_.LocalAddress -in @('127.0.0.1', '::1') } | Select-Object -First 3)
        foreach ($listener in $listeners) {
            $record = [ordered]@{ Pid = [int]$listener.OwningProcess; ProcessName = $null; ProcessStartedAtUtc = $null; BuildPath = $null; BuildHash = $null }
            try {
                $process = Get-Process -Id $record.Pid -ErrorAction Stop
                $record.ProcessName = [string]$process.ProcessName
                $record.ProcessStartedAtUtc = $process.StartTime.ToUniversalTime().ToString('o')
                if ($process.Path) {
                    $resolved = (Resolve-Path -LiteralPath $process.Path -ErrorAction Stop).Path
                    $record.BuildPath = $resolved
                    $record.BuildHash = Get-PromotionHash $resolved
                }
            } catch {}
            [pscustomobject]$record
        }
    }
}

function ConvertTo-Phase1Utc([object]$Value) {
    if ($Value -isnot [string]) { return $null }
    $parsed = [DateTimeOffset]::MinValue
    if (-not [DateTimeOffset]::TryParse($Value, [Globalization.CultureInfo]::InvariantCulture, [Globalization.DateTimeStyles]::RoundtripKind, [ref]$parsed)) { return $null }
    return $parsed.ToUniversalTime()
}

function Test-Phase1ProcessContinuity($Record, $Listener) {
    $names = @($Record.PSObject.Properties.Name)
    $isNative = $names -contains 'completedAtUnix'
    if ($isNative) {
        try { $completed = [DateTimeOffset]::FromUnixTimeSeconds([long]$Record.completedAtUnix) } catch { return $false }
    } else {
        $completed = ConvertTo-Phase1Utc $Record.completedAtUtc
    }
    $started = ConvertTo-Phase1Utc $Listener.ProcessStartedAtUtc
    if ($null -eq $completed -or $null -eq $started) { return $false }
    $now = [DateTimeOffset]::UtcNow
    if ($completed -gt $now.AddSeconds($script:Phase1ResumeClockSkewToleranceSeconds) -or $started -gt $now.AddSeconds($script:Phase1ResumeClockSkewToleranceSeconds)) { return $false }
    $leadSeconds = ($completed - $started).TotalSeconds
    $negativeTolerance = if ($isNative) { $script:Phase1ResumeProcessStartMatchToleranceSeconds } else { $script:Phase1ResumeClockSkewToleranceSeconds }
    if ($leadSeconds -lt -$negativeTolerance -or $leadSeconds -gt $script:Phase1ResumeMaximumStartLeadSeconds) { return $false }
    if (-not $isNative -and $Record.schemaVersion -eq 3) {
        $recordedStart = ConvertTo-Phase1Utc $Record.newProcessStartedAtUtc
        if ($null -eq $recordedStart) { return $false }
        if ([Math]::Abs(($started - $recordedStart).TotalSeconds) -gt $script:Phase1ResumeProcessStartMatchToleranceSeconds) { return $false }
    }
    return $true
}

function Get-ProvenPhase1Resume([string]$Root, [string]$Candidate, [string]$CandidateHash) {
    $handoff = Get-Phase1HandoffRecord -Root $Root
    if (-not $handoff.Exists) { return [pscustomobject]@{ State = 'ABSENT'; Listener = $null } }
    if (-not (Test-Phase1HandoffRecord -Record $handoff.Record -Root $Root -Candidate $Candidate -CandidateHash $CandidateHash)) {
        return [pscustomobject]@{ State = 'UNSAFE'; Listener = $null }
    }
    try { $listeners = @(Get-Phase1ListenerRecords) } catch { return [pscustomobject]@{ State = 'UNSAFE'; Listener = $null } }
    if ($listeners.Count -lt 1 -or $listeners.Count -gt 2) { return [pscustomobject]@{ State = 'UNSAFE'; Listener = $null } }
    $ownerPids = @($listeners | ForEach-Object { $_.Pid } | Select-Object -Unique)
    if ($ownerPids.Count -ne 1) { return [pscustomobject]@{ State = 'UNSAFE'; Listener = $null } }
    foreach ($listener in $listeners) {
        if ($listener.Pid -isnot [int] -and $listener.Pid -isnot [long]) { return [pscustomobject]@{ State = 'UNSAFE'; Listener = $null } }
        if ($listener.Pid -lt 1 -or $listener.Pid -ne $handoff.Record.newPid -or $listener.ProcessName -ne 'catdesk') { return [pscustomobject]@{ State = 'UNSAFE'; Listener = $null } }
        try {
            $listenerPath = [IO.Path]::GetFullPath([string]$listener.BuildPath)
            if (-not $listenerPath.Equals([IO.Path]::GetFullPath($Candidate), [StringComparison]::OrdinalIgnoreCase)) { return [pscustomobject]@{ State = 'UNSAFE'; Listener = $null } }
        } catch { return [pscustomobject]@{ State = 'UNSAFE'; Listener = $null } }
        if ($listener.BuildHash -isnot [string] -or -not $listener.BuildHash.Equals($CandidateHash, [StringComparison]::Ordinal)) { return [pscustomobject]@{ State = 'UNSAFE'; Listener = $null } }
        if (-not (Test-Phase1ProcessContinuity -Record $handoff.Record -Listener $listener)) { return [pscustomobject]@{ State = 'UNSAFE'; Listener = $null } }
    }
    return [pscustomobject]@{ State = 'PROVEN'; Listener = [pscustomobject]@{ Pid = [int]$ownerPids[0] } }
}

function Invoke-PromotionHandoff([string]$Root, $From, [string]$TargetPath, [string]$TargetHash) {
    Invoke-PromotionSeam -Name 'RestartHandoff' -Arguments @($Root, $From, $TargetPath, $TargetHash, $script:PromotionMcpPort, $script:PromotionTimeoutSeconds) -Default {
        param($workspaceRoot, $source, $target, $hash, $port, $timeoutSeconds)
        if ($null -eq $source -or $source.Pid -lt 1 -or -not $source.ProcessStartedAtUtc -or -not $source.BuildPath -or $source.BuildHash -notmatch '^[a-f0-9]{64}$') { return $false }
        $helper = Join-Path $PSScriptRoot 'restart_catdesk_daemon.ps1'
        if (-not (Test-Path -LiteralPath $helper -PathType Leaf)) { return $false }
        $before = [DateTime]::UtcNow
        $raw = @(& $helper -OldPid ([int]$source.Pid) -ExpectedOldProcessStartedAtUtc ([string]$source.ProcessStartedAtUtc) -ExpectedOldProcessPath ([string]$source.BuildPath) -ExpectedOldProcessSha256 ([string]$source.BuildHash) -BuildPath $target -Workspace $workspaceRoot -McpPort $port -ReadyTimeoutSeconds $timeoutSeconds -Execute 2>$null)
        if ($LASTEXITCODE -ne 0 -or $raw.Count -eq 0) { return $false }
        $statePath = Join-Path $workspaceRoot '.catdesk\restart-handoff\latest.json'
        $deadline = [DateTime]::UtcNow.AddSeconds($timeoutSeconds)
        do {
            try {
                if (Test-Path -LiteralPath $statePath -PathType Leaf) {
                    $state = Get-Content -LiteralPath $statePath -Raw -ErrorAction Stop | ConvertFrom-Json -ErrorAction Stop
                    if ($state.status -eq 'RECOVERED_PENDING_TRANSPORT_CHECK' -and $state.completedAtUtc -and ([DateTime]$state.completedAtUtc).ToUniversalTime() -ge $before -and $state.buildSha256 -eq $hash) { return $true }
                    if ($state.status -eq 'FAILED_OPERATOR_ATTENTION') { return $false }
                }
            } catch { return $false }
            Start-Sleep -Milliseconds 500
        } while ([DateTime]::UtcNow -lt $deadline)
        return $false
    }
}

function Save-PromotionBackup([string]$Root, $Canonical) {
    Invoke-PromotionSeam -Name 'SaveBackup' -Arguments @($Root, $Canonical) -Default {
        param($workspaceRoot, $release)
        [void](Get-PromotionBackupEvidence $workspaceRoot)
        $directory = Join-Path $workspaceRoot '.catdesk\promotion-recovery'
        New-Item -ItemType Directory -Path $directory -Force | Out-Null
        $backupBinary = Join-Path $directory 'previous-catdesk.exe'
        $backupManifest = Join-Path $directory 'previous-catdesk.exe.sha256'
        $temporaryBinary = "$backupBinary.tmp"
        $temporaryManifest = "$backupManifest.tmp"
        Copy-Item -LiteralPath $release.Binary -Destination $temporaryBinary -Force -ErrorAction Stop
        [IO.File]::WriteAllText($temporaryManifest, "$($release.Hash)`n", [Text.UTF8Encoding]::new($false))
        Move-Item -LiteralPath $temporaryBinary -Destination $backupBinary -Force
        Move-Item -LiteralPath $temporaryManifest -Destination $backupManifest -Force
        return (Get-PromotionBackupEvidence $workspaceRoot)
    }
}

function Swap-PromotedCanonical([string]$Root, [string]$Candidate, [string]$CandidateHash, $Canonical) {
    Invoke-PromotionSeam -Name 'SwapCanonical' -Arguments @($Root, $Candidate, $CandidateHash, $Canonical) -Default {
        param($workspaceRoot, $candidatePath, $hash, $release)
        Invoke-PromotionCheckpoint 'BEFORE_CANONICAL_PAIR_REPLACEMENT'
        $prior = Get-PromotionBackupEvidence $workspaceRoot
        if ($null -eq $prior -or $prior.Hash -ne $release.Hash) { throw 'validated prior canonical backup is unavailable' }
        $temporaryBinary = "$($release.Binary).promotion.tmp"
        $temporaryManifest = "$($release.Manifest).promotion.tmp"
        try {
            Copy-Item -LiteralPath $candidatePath -Destination $temporaryBinary -Force -ErrorAction Stop
            [IO.File]::WriteAllText($temporaryManifest, "$hash`n", [Text.UTF8Encoding]::new($false))
            if ((Get-PromotionHash $temporaryBinary) -ne $hash -or [IO.File]::ReadAllText($temporaryManifest).Trim() -ne $hash) { throw 'promotion staging validation failed' }
            Move-Item -LiteralPath $temporaryBinary -Destination $release.Binary -Force
            Invoke-PromotionCheckpoint 'AFTER_CANONICAL_BINARY_REPLACEMENT'
            Move-Item -LiteralPath $temporaryManifest -Destination $release.Manifest -Force
            Invoke-PromotionCheckpoint 'AFTER_CANONICAL_MANIFEST_REPLACEMENT'
            $revalidated = Get-CanonicalReleaseEvidence $workspaceRoot
            if ($revalidated.Hash -ne $hash) { throw 'promoted canonical release validation failed' }
            return [pscustomobject]@{ State = 'CANDIDATE'; Evidence = $revalidated }
        } catch {
            Remove-Item -LiteralPath $temporaryBinary -Force -ErrorAction SilentlyContinue
            Remove-Item -LiteralPath $temporaryManifest -Force -ErrorAction SilentlyContinue
            $disposition = Get-CanonicalPairDisposition -Root $workspaceRoot -CandidateHash $hash -PriorHash $prior.Hash
            if ($disposition.State -eq 'CANDIDATE') { return $disposition }
            if ($disposition.State -eq 'PRIOR') { return $disposition }
            try {
                $restored = Restore-PriorCanonical -Root $workspaceRoot -Backup $prior -Canonical $release
                return [pscustomobject]@{ State = 'RESTORED'; Evidence = $restored }
            } catch { throw 'canonical pair rollback could not be proven' }
        }
    }
}

function Restore-PriorCanonical([string]$Root, $Backup, $Canonical) {
    Invoke-PromotionSeam -Name 'RestoreCanonical' -Arguments @($Root, $Backup, $Canonical) -Default {
        param($workspaceRoot, $prior, $release)
        $temporaryBinary = "$($release.Binary).rollback.tmp"
        $temporaryManifest = "$($release.Manifest).rollback.tmp"
        Copy-Item -LiteralPath $prior.Binary -Destination $temporaryBinary -Force -ErrorAction Stop
        [IO.File]::WriteAllText($temporaryManifest, "$($prior.Hash)`n", [Text.UTF8Encoding]::new($false))
        Move-Item -LiteralPath $temporaryBinary -Destination $release.Binary -Force
        Invoke-PromotionCheckpoint 'AFTER_ROLLBACK_BINARY_REPLACEMENT'
        Move-Item -LiteralPath $temporaryManifest -Destination $release.Manifest -Force
        Invoke-PromotionCheckpoint 'AFTER_ROLLBACK_MANIFEST_REPLACEMENT'
        $revalidated = Get-CanonicalReleaseEvidence $workspaceRoot
        if ($revalidated.Hash -ne $prior.Hash) { throw 'rollback validation failed' }
        return $revalidated
    }
}

function Invoke-InterruptedPromotionRecovery([string]$Root, $Transaction, [switch]$RecoveryRollbackOnly) {
    $backup = Get-PromotionBackupEvidence $Root
    if ($null -eq $backup -or $backup.Hash -ne $Transaction.PriorHash) { throw 'promotion recovery backup is invalid' }
    $release = [pscustomobject]@{
        Binary = Get-ContainedRegularFile -Root $Root -Path (Join-Path $Root 'target\release\catdesk.exe') -Label 'canonical release binary'
        Manifest = Get-ContainedRegularFile -Root $Root -Path (Join-Path $Root 'target\release\catdesk.exe.sha256') -Label 'canonical release manifest'
        Hash = $Transaction.PriorHash
    }
    $disposition = Get-CanonicalPairDisposition -Root $Root -CandidateHash $Transaction.CandidateHash -PriorHash $Transaction.PriorHash
    if ($disposition.State -ne 'PRIOR') { $restored = Restore-PriorCanonical -Root $Root -Backup $backup -Canonical $release } else { $restored = $disposition.Evidence }
    if ($restored.Hash -ne $Transaction.PriorHash) { throw 'promotion recovery pair validation failed' }
    if ($RecoveryRollbackOnly) {
        Clear-PromotionTransaction $Root
        return $restored
    }
    $candidatePath = Join-Path $Root $Transaction.CandidateRelativePath
    $candidateListener = $null
    if (Test-Path -LiteralPath $candidatePath -PathType Leaf) {
        try { $candidateListener = Get-PromotionListener -BuildPath (Get-ContainedRegularFile -Root $Root -Path $candidatePath -Label 'transaction candidate') -ExpectedHash $Transaction.CandidateHash } catch { throw 'promotion transaction candidate is invalid' }
    }
    if ($null -ne $candidateListener -and -not $RecoveryRollbackOnly) {
        if (-not (Invoke-PromotionHandoff -Root $Root -From $candidateListener -TargetPath $restored.Binary -TargetHash $restored.Hash)) { throw 'promotion recovery canonical handoff was not proven' }
        if ($null -eq (Get-PromotionListener -BuildPath $restored.Binary -ExpectedHash $restored.Hash)) { throw 'promotion recovery canonical listener was not proven' }
    }
    Clear-PromotionTransaction $Root
    return $restored
}

try {
    $root = (Resolve-Path -LiteralPath $Workspace -ErrorAction Stop).Path
    $interrupted = Get-PromotionTransaction $root
    if ($RecoveryRollbackOnly -and $null -eq $interrupted) { throw 'recovery rollback-only mode requires an interrupted promotion transaction' }
    if ($null -ne $interrupted) {
        if (-not $Execute) {
            Write-PromotionResult -State 'INTERRUPTED_RECOVERY_REQUIRED' -CandidateValidated $false -CanonicalReleaseValidated $false -PromotionRequired $false
            return
        }
        if ($AuthorizationToken -notmatch '^[0-9a-f]{32}$' -and -not $script:PromotionSeams.ContainsKey('ProtectedAuthorization')) {
            Write-PromotionResult -State 'REVIEWED_PROMOTION_AUTHORIZATION_REQUIRED' -CandidateValidated $false -CanonicalReleaseValidated $false -PromotionRequired $false
            return
        }
        try {
            $transactionCandidate = Get-ContainedRegularFile -Root $root -Path $BuildPath -Label 'transaction candidate'
            $script:PromotionAuthorization = Get-ProtectedPromotionAuthorization -Root $root -Token $AuthorizationToken -TransactionId $TransactionId -Candidate $transactionCandidate -CandidateHash (Get-PromotionHash $transactionCandidate) -PriorHash $interrupted.PriorHash
            if ($interrupted.SchemaVersion -ne 2 -or [string]$interrupted.AuthorizationId -ne [string]$script:PromotionAuthorization.authorizationId) { throw 'REVIEWED_PROMOTION_AUTHORIZATION_REQUIRED' }
        } catch {
            Write-PromotionResult -State 'REVIEWED_PROMOTION_AUTHORIZATION_REQUIRED' -CandidateValidated $false -CanonicalReleaseValidated $false -PromotionRequired $false
            return
        }
        try {
            [void](Invoke-InterruptedPromotionRecovery -Root $root -Transaction $interrupted -RecoveryRollbackOnly:$RecoveryRollbackOnly)
            Write-PromotionResult -State 'RECOVERED_PRIOR_CANONICAL' -CandidateValidated $false -CanonicalReleaseValidated $true -PromotionRequired $false
        } catch {
            Write-PromotionResult -State 'ROLLBACK_UNPROVEN_OPERATOR_ATTENTION' -CandidateValidated $false -CanonicalReleaseValidated $false -PromotionRequired $false
        }
        return
    }
    $candidate = Get-ContainedRegularFile -Root $root -Path $BuildPath -Label 'reviewed build candidate'
    $candidateHash = Get-PromotionHash $candidate
    $canonical = Get-CanonicalReleaseEvidence $root
    $required = $candidateHash -ne $canonical.Hash
    if ($ValidatePhase1) {
        if (-not $required) {
            Write-PromotionResult -State 'ALREADY_CURRENT' -CandidateValidated $true -CanonicalReleaseValidated $true -PromotionRequired $false
            return
        }
        # T-0127 remains the sole authority for native and legacy receipt,
        # listener, and process-continuity proof. Validation never performs a
        # handoff or reaches the promotion/rollback paths below.
        $phase1Resume = Get-ProvenPhase1Resume -Root $root -Candidate $candidate -CandidateHash $candidateHash
        if ($phase1Resume.State -eq 'PROVEN') {
            Write-PromotionResult -State 'PHASE1_PROVEN' -CandidateValidated $true -CanonicalReleaseValidated $true -PromotionRequired $true
            return
        }
        if ($phase1Resume.State -eq 'ABSENT') {
            $canonicalListener = Get-PromotionListener -BuildPath $canonical.Binary -ExpectedHash $canonical.Hash
            if ($null -ne $canonicalListener) {
                Write-PromotionResult -State 'PHASE1_NOT_STARTED' -CandidateValidated $true -CanonicalReleaseValidated $true -PromotionRequired $true
            } else {
                Write-PromotionResult -State 'OPERATOR_ATTENTION_PHASE1' -CandidateValidated $true -CanonicalReleaseValidated $true -PromotionRequired $true
            }
            return
        }
        Write-PromotionResult -State 'OPERATOR_ATTENTION_PHASE1' -CandidateValidated $true -CanonicalReleaseValidated $true -PromotionRequired $true
        return
    }
    if (-not $Execute) {
        Write-PromotionResult -State $(if ($required) { 'PLAN_READY' } else { 'ALREADY_CURRENT' }) -CandidateValidated $true -CanonicalReleaseValidated $true -PromotionRequired $required
        return
    }
    if (-not $required) {
        Write-PromotionResult -State 'ALREADY_CURRENT' -CandidateValidated $true -CanonicalReleaseValidated $true -PromotionRequired $false
        return
    }
    if ($AuthorizationToken -notmatch '^[0-9a-f]{32}$' -and -not $script:PromotionSeams.ContainsKey('ProtectedAuthorization')) {
        Write-PromotionResult -State 'REVIEWED_PROMOTION_AUTHORIZATION_REQUIRED' -CandidateValidated $true -CanonicalReleaseValidated $true -PromotionRequired $true
        return
    }
    $script:PromotionAuthorization = Get-ProtectedPromotionAuthorization -Root $root -Token $AuthorizationToken -TransactionId $TransactionId -Candidate $candidate -CandidateHash $candidateHash -PriorHash $canonical.Hash
    # If a prior invocation reached the candidate listener but did not reach
    # the disk promotion, resume only from bounded, identity-complete evidence.
    # Any existing but stale, malformed, or ambiguous evidence blocks a new
    # Phase 1 handoff so a second candidate daemon is never introduced.
    $phase1Resume = Get-ProvenPhase1Resume -Root $root -Candidate $candidate -CandidateHash $candidateHash
    if ($phase1Resume.State -eq 'PROVEN') {
        $candidateListener = $phase1Resume.Listener
    } elseif ($phase1Resume.State -eq 'UNSAFE') {
        Write-PromotionResult -State 'OPERATOR_ATTENTION_PHASE1' -CandidateValidated $true -CanonicalReleaseValidated $true -PromotionRequired $true
        return
    } else {
        $current = Get-PromotionListener -BuildPath $canonical.Binary -ExpectedHash $canonical.Hash
        if ($null -eq $current -or -not (Invoke-PromotionHandoff -Root $root -From $current -TargetPath $candidate -TargetHash $candidateHash)) {
            Write-PromotionResult -State 'OPERATOR_ATTENTION_PHASE1' -CandidateValidated $true -CanonicalReleaseValidated $true -PromotionRequired $true
            return
        }
        $candidateListener = Get-PromotionListener -BuildPath $candidate -ExpectedHash $candidateHash
        if ($null -eq $candidateListener) {
            Write-PromotionResult -State 'OPERATOR_ATTENTION_PHASE1' -CandidateValidated $true -CanonicalReleaseValidated $true -PromotionRequired $true
            return
        }
    }
    # A pre-existing canonical pair alone is not promotion approval. Preserve
    # any already-valid escrow, but do not mint a new LKG snapshot until this
    # transaction's exact candidate has been swapped and handed back.
    $backup = Save-PromotionBackup -Root $root -Canonical $canonical
    try {
        $promotionTransaction = Write-PromotionTransaction -Root $root -PriorHash $backup.Hash -CandidateHash $candidateHash -CandidatePath $candidate
        Invoke-PromotionCheckpoint 'TRANSACTION_DURABLY_RECORDED'
        $swap = Swap-PromotedCanonical -Root $root -Candidate $candidate -CandidateHash $candidateHash -Canonical $canonical
    } catch {
        $disposition = Get-CanonicalPairDisposition -Root $root -CandidateHash $candidateHash -PriorHash $canonical.Hash
        Write-PromotionResult -State $(if ($disposition.State -eq 'PRIOR') { 'OPERATOR_ATTENTION_SWAP' } else { 'ROLLBACK_UNPROVEN_OPERATOR_ATTENTION' }) -CandidateValidated $true -CanonicalReleaseValidated $true -PromotionRequired $true
        return
    }
    if ($swap.State -ne 'CANDIDATE') {
        Write-PromotionResult -State 'OPERATOR_ATTENTION_SWAP' -CandidateValidated $true -CanonicalReleaseValidated $true -PromotionRequired $true
        return
    }
    $promoted = $swap.Evidence
    if (Invoke-PromotionHandoff -Root $root -From $candidateListener -TargetPath $promoted.Binary -TargetHash $promoted.Hash) {
        $final = Get-PromotionListener -BuildPath $promoted.Binary -ExpectedHash $promoted.Hash
        if ($null -ne $final) {
            try {
                # This record is the sole migration authority for legacy LKG
                # escrow. It is written only after the promoted canonical pair
                # and exact listener handback have both been proven.
                Write-ReviewedPromotionAuthority -Root $root -Transaction $promotionTransaction -Canonical $promoted
                [void](Save-CatDeskLastKnownGoodRelease -Root $root -Canonical $promoted)
            } catch {
                # Keep the durable promotion transaction and prior backup. A
                # later recovery can still restore the prior canonical pair;
                # never report a fully production-ready promotion without a
                # persistent recovery snapshot of the newly proven release.
                Write-PromotionResult -State 'RECOVERY_SNAPSHOT_UNPROVEN_OPERATOR_ATTENTION' -CandidateValidated $true -CanonicalReleaseValidated $true -PromotionRequired $true
                return
            }
            Clear-PromotionTransaction $root
            Write-PromotionResult -State 'PROMOTED_CANONICAL_READY' -CandidateValidated $true -CanonicalReleaseValidated $true -PromotionRequired $true
            return
        }
    }
    try {
        $restored = Restore-PriorCanonical -Root $root -Backup $backup -Canonical $canonical
        $candidateStillLive = Get-PromotionListener -BuildPath $candidate -ExpectedHash $candidateHash
        if ($null -ne $candidateStillLive) {
            if (-not (Invoke-PromotionHandoff -Root $root -From $candidateStillLive -TargetPath $restored.Binary -TargetHash $restored.Hash) -or $null -eq (Get-PromotionListener -BuildPath $restored.Binary -ExpectedHash $restored.Hash)) { throw 'rollback canonical handoff was not proven' }
        }
        Clear-PromotionTransaction $root
        Write-PromotionResult -State 'ROLLED_BACK_OPERATOR_ATTENTION' -CandidateValidated $true -CanonicalReleaseValidated $true -PromotionRequired $true
    } catch {
        Write-PromotionResult -State 'ROLLBACK_UNPROVEN_OPERATOR_ATTENTION' -CandidateValidated $true -CanonicalReleaseValidated $true -PromotionRequired $true
    }
} catch {
    Invoke-PromotionSeam -Name 'Failure' -Arguments @($_.Exception.Message) -Default { param($ignored) }
    $transactionPath = if ($root) { Join-Path $root '.catdesk\promotion-recovery\promotion-transaction.json' } else { $null }
    $state = if ($_.Exception.Message -eq 'REVIEWED_PROMOTION_AUTHORIZATION_REQUIRED') { 'REVIEWED_PROMOTION_AUTHORIZATION_REQUIRED' } elseif ($transactionPath -and (Test-Path -LiteralPath $transactionPath -PathType Leaf)) { 'ROLLBACK_UNPROVEN_OPERATOR_ATTENTION' } else { 'OPERATOR_ATTENTION_INVALID' }
    Write-PromotionResult -State $state -CandidateValidated $false -CanonicalReleaseValidated $false -PromotionRequired $false
}
