# Internal CatDesk reviewed-release recovery primitives.
# Public/operator lifecycle remains catdesk.ps1. This file intentionally knows
# nothing about tunnel endpoints, browser/authentication state, or provider
# credentials; it only manages workspace-contained reviewed binary+SHA pairs.

$script:CatDeskReleaseRecoverySchemaVersion = 1
$script:CatDeskReleaseRecoveryMaxManifestBytes = 4096
$script:CatDeskReleaseRecoveryMaxPointerBytes = 1024
$script:CatDeskReleaseRecoveryMaxBinaryBytes = 128MB
$script:CatDeskReleaseRecoverySlots = @('slot-a', 'slot-b')

function ConvertFrom-CatDeskLiteralJson([string]$Text) {
    # PowerShell7.5+ otherwise coerces ISO identity witnesses to DateTime,
    # changing their literal process-creation proof. Windows PowerShell5.1
    # already keeps JSON strings and does not support the DateKind parameter.
    $reader=Get-Command ConvertFrom-Json -CommandType Cmdlet -ErrorAction Stop
    if($reader.Parameters.ContainsKey('DateKind')) {
        return $Text | ConvertFrom-Json -DateKind String -ErrorAction Stop
    }
    return $Text | ConvertFrom-Json -ErrorAction Stop
}

function Get-CatDeskRecoverySha256([string]$Path) {
    # The fixed Windows PowerShell worker may inherit a Core-only module path.
    # Hash through the framework directly, retaining the existing helper/API.
    $stream = [IO.File]::OpenRead($Path)
    $sha = $null
    try {
        $sha = [Security.Cryptography.SHA256]::Create()
        return ([BitConverter]::ToString($sha.ComputeHash($stream)).Replace('-', '')).ToLowerInvariant()
    } finally {
        if ($null -ne $sha) { $sha.Dispose() }
        $stream.Dispose()
    }
}

function Resolve-CatDeskRecoveryRoot([string]$Root) {
    $resolved = (Resolve-Path -LiteralPath $Root -ErrorAction Stop).Path
    $item = Get-Item -LiteralPath $resolved -Force -ErrorAction Stop
    if (-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw 'release recovery workspace is invalid'
    }
    return $resolved
}

function Assert-CatDeskRecoveryContainedPath([string]$Root, [string]$Path, [string]$Label, [switch]$AllowMissingLeaf) {
    $rootPath = Resolve-CatDeskRecoveryRoot $Root
    $full = [IO.Path]::GetFullPath($Path)
    $prefix = $rootPath.TrimEnd('\') + '\'
    if (-not $full.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw "$Label is outside the approved workspace"
    }
    $relative = $full.Substring($prefix.Length)
    if ([string]::IsNullOrWhiteSpace($relative) -or $relative -match '(^|\\)\.\.?($|\\)') {
        throw "$Label path is invalid"
    }
    $cursor = $rootPath
    $segments = $relative.Split('\', [StringSplitOptions]::RemoveEmptyEntries)
    for ($i = 0; $i -lt $segments.Count; $i++) {
        $cursor = Join-Path $cursor $segments[$i]
        if (-not (Test-Path -LiteralPath $cursor)) {
            if ($AllowMissingLeaf -and $i -eq ($segments.Count - 1)) { break }
            if ($AllowMissingLeaf) { continue }
            throw "$Label is unavailable"
        }
        $item = Get-Item -LiteralPath $cursor -Force -ErrorAction Stop
        if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) {
            throw "$Label traverses a reparse point"
        }
    }
    return $full
}

function Get-CatDeskReleaseRecoveryDirectory([string]$Root) {
    $rootPath = Resolve-CatDeskRecoveryRoot $Root
    $control = Join-Path $rootPath '.catdesk'
    if (Test-Path -LiteralPath $control) {
        $controlItem = Get-Item -LiteralPath $control -Force -ErrorAction Stop
        if (-not $controlItem.PSIsContainer -or ($controlItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            throw 'release recovery control directory is invalid'
        }
    }
    return Join-Path $control 'release-recovery'
}

function Write-CatDeskDurableBytes([string]$Path, [byte[]]$Bytes) {
    $parent = Split-Path -Parent $Path
    if (-not (Test-Path -LiteralPath $parent -PathType Container)) {
        New-Item -ItemType Directory -Path $parent -Force -ErrorAction Stop | Out-Null
    }
    $stream = [IO.FileStream]::new($Path, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    try {
        $stream.Write($Bytes, 0, $Bytes.Length)
        $stream.Flush($true)
    } finally {
        $stream.Dispose()
    }
}

function Copy-CatDeskDurableFile([string]$Source, [string]$Destination) {
    $parent = Split-Path -Parent $Destination
    if (-not (Test-Path -LiteralPath $parent -PathType Container)) {
        New-Item -ItemType Directory -Path $parent -Force -ErrorAction Stop | Out-Null
    }
    $input = [IO.FileStream]::new($Source, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
    $output = $null
    try {
        $output = [IO.FileStream]::new($Destination, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
        $input.CopyTo($output)
        $output.Flush($true)
    } finally {
        if ($null -ne $output) { $output.Dispose() }
        $input.Dispose()
    }
}

function Write-CatDeskAtomicUtf8([string]$Path, [string]$Text) {
    $parent = Split-Path -Parent $Path
    if (-not (Test-Path -LiteralPath $parent -PathType Container)) {
        New-Item -ItemType Directory -Path $parent -Force -ErrorAction Stop | Out-Null
    }
    $temporary = "$Path.tmp"
    $backup = "$Path.previous"
    foreach ($stale in @($temporary, $backup)) {
        if (Test-Path -LiteralPath $stale) {
            $item = Get-Item -LiteralPath $stale -Force -ErrorAction Stop
            if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
                throw 'release recovery atomic state is unsafe'
            }
            Remove-Item -LiteralPath $stale -Force -ErrorAction Stop
        }
    }
    $bytes = [Text.UTF8Encoding]::new($false).GetBytes($Text)
    Write-CatDeskDurableBytes -Path $temporary -Bytes $bytes
    if (Test-Path -LiteralPath $Path -PathType Leaf) {
        [IO.File]::Replace($temporary, $Path, $backup, $true)
        if (Test-Path -LiteralPath $backup -PathType Leaf) { Remove-Item -LiteralPath $backup -Force -ErrorAction Stop }
    } else {
        Move-Item -LiteralPath $temporary -Destination $Path -ErrorAction Stop
    }
}

function Read-CatDeskLkgSlot([string]$Root, [string]$Slot) {
    if ($Slot -notin $script:CatDeskReleaseRecoverySlots) { throw 'release recovery slot is invalid' }
    $base = Join-Path (Get-CatDeskReleaseRecoveryDirectory $Root) $Slot
    if (-not (Test-Path -LiteralPath $base)) { return $null }
    $baseItem = Get-Item -LiteralPath $base -Force -ErrorAction Stop
    if (-not $baseItem.PSIsContainer -or ($baseItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw 'release recovery slot is invalid'
    }
    $binary = Assert-CatDeskRecoveryContainedPath -Root $Root -Path (Join-Path $base 'catdesk.exe') -Label 'release recovery binary'
    $shaPath = Assert-CatDeskRecoveryContainedPath -Root $Root -Path (Join-Path $base 'catdesk.exe.sha256') -Label 'release recovery fingerprint'
    $manifestPath = Assert-CatDeskRecoveryContainedPath -Root $Root -Path (Join-Path $base 'manifest.json') -Label 'release recovery manifest'
    foreach ($path in @($binary, $shaPath, $manifestPath)) {
        $item = Get-Item -LiteralPath $path -Force -ErrorAction Stop
        if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'release recovery slot contains a non-regular file' }
    }
    $binaryItem = Get-Item -LiteralPath $binary -ErrorAction Stop
    $shaItem = Get-Item -LiteralPath $shaPath -ErrorAction Stop
    $manifestItem = Get-Item -LiteralPath $manifestPath -ErrorAction Stop
    if ($binaryItem.Length -lt 1 -or $binaryItem.Length -gt $script:CatDeskReleaseRecoveryMaxBinaryBytes) {
        throw 'release recovery binary size is invalid'
    }
    if ($shaItem.Length -gt 128 -or $manifestItem.Length -gt $script:CatDeskReleaseRecoveryMaxManifestBytes) {
        throw 'release recovery slot metadata is oversized'
    }
    $expected = [IO.File]::ReadAllText($shaPath).Trim().ToLowerInvariant()
    if ($expected -notmatch '^[a-f0-9]{64}$') { throw 'release recovery fingerprint is malformed' }
    $actual = Get-CatDeskRecoverySha256 $binary
    if ($actual -ne $expected) { throw 'release recovery binary fingerprint did not match' }
    $manifest = ConvertFrom-CatDeskLiteralJson ([IO.File]::ReadAllText($manifestPath))
    $properties = @($manifest.PSObject.Properties.Name | Sort-Object)
    $required = @('capturedAtUtc', 'generation', 'schemaVersion', 'sha256', 'source')
    if ([string]::Join('|', $properties) -ne [string]::Join('|', ($required | Sort-Object))) { throw 'release recovery manifest fields are invalid' }
    if ([int]$manifest.schemaVersion -ne $script:CatDeskReleaseRecoverySchemaVersion) { throw 'release recovery manifest schema is invalid' }
    $generation = 0
    if (-not [int]::TryParse([string]$manifest.generation, [ref]$generation) -or $generation -lt 1) { throw 'release recovery generation is invalid' }
    # Only independently reviewed promotion evidence is rollback authority.
    # Older builds could persist `operational_verified` here from runtime health;
    # those legacy snapshots must now fail closed rather than remain trusted LKG.
    if ([string]$manifest.source -notin @('reviewed_promotion', 'reviewed_reconciliation', 'reviewed_qualified_stopped', 'operational_verified')) { throw 'release recovery source is invalid' }
    if ([string]$manifest.source -eq 'reviewed_qualified_stopped') {
        if ($Slot -ne 'slot-b' -or $generation -ne 2) { throw 'stopped recovery slot identity is invalid' }
        [void](Get-CatDeskQualifiedStoppedAuthority -Root $Root -ExpectedHash $expected)
    } elseif ([string]$manifest.source -eq 'reviewed_reconciliation') {
        if ($Slot -ne 'slot-b' -or $generation -ne 2) { throw 'reconciliation recovery slot identity is invalid' }
        [void](Get-CatDeskReviewedReconciliationAuthority -Root $Root -ExpectedHash $expected)
    } elseif ([string]$manifest.source -ne 'reviewed_promotion') {
        throw 'release recovery source is not authoritative'
    }
    if ([string]$manifest.sha256 -ne $expected) { throw 'release recovery manifest fingerprint did not match' }
    try { [void]([DateTime]::Parse([string]$manifest.capturedAtUtc).ToUniversalTime()) } catch { throw 'release recovery timestamp is invalid' }
    [pscustomobject]@{ Slot = $Slot; Generation = $generation; Hash = $actual; Binary = $binary; Manifest = $shaPath; Metadata = $manifestPath }
}

# This is a second, first-generation reviewed authority. It is accepted only
# after the native owner has released its private Job, a fresh exact listener
# and process-instance probe has completed, and the fixed transaction has
# journaled slot-b as complete. Runtime health alone cannot mint this record.
function Get-CatDeskReviewedReconciliationAuthority([string]$Root, [string]$ExpectedHash) {
    $control = Join-Path (Resolve-CatDeskRecoveryRoot $Root) '.catdesk\release-pair-control'
    $records = @(
        @{ Name = 'authorization.json'; Limit = 8192 },
        @{ Name = 'claim.json'; Limit = 1024 },
        @{ Name = 'transaction.json'; Limit = 4096 },
        @{ Name = 'job-release.json'; Limit = 1024 },
        @{ Name = 'reconciled-release.json'; Limit = 2048 }
    )
    $read = @{}
    foreach ($entry in $records) {
        $path = Assert-CatDeskRecoveryContainedPath -Root $Root -Path (Join-Path $control $entry.Name) -Label 'reconciliation recovery authority'
        $item = Get-Item -LiteralPath $path -Force -ErrorAction Stop
        if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -or $item.Length -lt 1 -or $item.Length -gt $entry.Limit) { throw 'reconciliation recovery authority is unsafe' }
        $read[$entry.Name] = ConvertFrom-CatDeskLiteralJson ([IO.File]::ReadAllText($path))
    }
    $a = $read['authorization.json']; $c = $read['claim.json']; $j = $read['transaction.json']; $r = $read['job-release.json']; $proof = $read['reconciled-release.json']
    $shapes = @(
        @{ Value = $a; Fields = @('schemaVersion','authorizationId','transactionId','expiresAtUnix','reviewedPreflightSha256','reviewedAuthorizationId','candidateRelativePath','candidateSha256','originalBinarySha256','originalSidecarFileSha256','originalSidecarValue','slotABinarySha256','slotASidecarFileSha256','slotAManifestSha256','originalPointerSha256','scriptSha256','recoveryHelperSha256','powershellSha256') },
        @{ Value = $c; Fields = @('schemaVersion','authorizationId','transactionId','state') },
        @{ Value = $j; Fields = @('schemaVersion','authorizationId','transactionId','phase','candidateSha256','originalBinarySha256','originalSidecarFileSha256') },
        @{ Value = $r; Fields = @('schemaVersion','state','authorizationId','transactionId','ownerPid','ownerStartedAtUtc') },
        @{ Value = $proof; Fields = @('schemaVersion','state','authorizationId','transactionId','candidateSha256','originalBinarySha256','originalSidecarFileSha256','ownerPid','ownerStartedAtUtc','postReleaseVerifiedAtUtc') }
    )
    foreach ($shape in $shapes) {
        $names = @($shape.Value.PSObject.Properties.Name)
        if ($names.Count -ne $shape.Fields.Count -or @($names | Where-Object { $_ -notin $shape.Fields }).Count -ne 0 -or $shape.Value.schemaVersion -ne 1) { throw 'reconciliation recovery authority fields are invalid' }
    }
    if ($ExpectedHash -cnotmatch '^[a-f0-9]{64}$' -or [string]$a.candidateSha256 -cne $ExpectedHash -or [string]$j.candidateSha256 -cne $ExpectedHash -or [string]$proof.candidateSha256 -cne $ExpectedHash -or [string]$c.state -cne 'CLAIMED_PENDING' -or [string]$j.phase -cne 'COMPLETED' -or [string]$r.state -cne 'JOB_RELEASED_PENDING_POSTPROOF' -or [string]$proof.state -cne 'RECONCILED_CANONICAL_READY') { throw 'reconciliation recovery authority is incomplete' }
    if ([string]$a.authorizationId -cnotmatch '^[0-9a-f]{32}$' -or [string]$a.transactionId -cnotmatch '^[0-9a-f]{32}$' -or [string]$a.originalBinarySha256 -cnotmatch '^[a-f0-9]{64}$' -or [string]$a.originalSidecarFileSha256 -cnotmatch '^[a-f0-9]{64}$') { throw 'reconciliation recovery authority is malformed' }
    if ([string]$a.authorizationId -cne [string]$c.authorizationId -or [string]$a.authorizationId -cne [string]$j.authorizationId -or [string]$a.authorizationId -cne [string]$r.authorizationId -or [string]$a.authorizationId -cne [string]$proof.authorizationId -or [string]$a.transactionId -cne [string]$c.transactionId -or [string]$a.transactionId -cne [string]$j.transactionId -or [string]$a.transactionId -cne [string]$r.transactionId -or [string]$a.transactionId -cne [string]$proof.transactionId) { throw 'reconciliation recovery authority identity drifted' }
    if ([string]$a.originalBinarySha256 -cne [string]$j.originalBinarySha256 -or [string]$a.originalBinarySha256 -cne [string]$proof.originalBinarySha256 -or [string]$a.originalSidecarFileSha256 -cne [string]$j.originalSidecarFileSha256 -or [string]$a.originalSidecarFileSha256 -cne [string]$proof.originalSidecarFileSha256 -or [int]$r.ownerPid -lt 1 -or [int]$r.ownerPid -ne [int]$proof.ownerPid -or [string]$r.ownerStartedAtUtc -cne [string]$proof.ownerStartedAtUtc) { throw 'reconciliation recovery authority owner drifted' }
    try { [void]([DateTime]::Parse([string]$proof.postReleaseVerifiedAtUtc).ToUniversalTime()); [void]([DateTime]::Parse([string]$proof.ownerStartedAtUtc).ToUniversalTime()) } catch { throw 'reconciliation recovery authority timestamp is invalid' }
    return $proof
}

function Read-CatDeskLkgPointer([string]$Root) {
    $path = Join-Path (Get-CatDeskReleaseRecoveryDirectory $Root) 'current.json'
    if (-not (Test-Path -LiteralPath $path)) { return $null }
    $path = Assert-CatDeskRecoveryContainedPath -Root $Root -Path $path -Label 'release recovery pointer'
    $item = Get-Item -LiteralPath $path -Force -ErrorAction Stop
    if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -or $item.Length -gt $script:CatDeskReleaseRecoveryMaxPointerBytes) {
        throw 'release recovery pointer is invalid'
    }
    $pointer = ConvertFrom-CatDeskLiteralJson ([IO.File]::ReadAllText($path))
    $properties = @($pointer.PSObject.Properties.Name | Sort-Object)
    $required = @('generation', 'schemaVersion', 'sha256', 'slot')
    if ([string]::Join('|', $properties) -ne [string]::Join('|', ($required | Sort-Object))) { throw 'release recovery pointer fields are invalid' }
    if ([int]$pointer.schemaVersion -ne $script:CatDeskReleaseRecoverySchemaVersion) { throw 'release recovery pointer schema is invalid' }
    $generation = 0
    if (-not [int]::TryParse([string]$pointer.generation, [ref]$generation) -or $generation -lt 1) { throw 'release recovery pointer generation is invalid' }
    $slot = [string]$pointer.slot
    $sha = ([string]$pointer.sha256).ToLowerInvariant()
    if ($slot -notin $script:CatDeskReleaseRecoverySlots -or $sha -notmatch '^[a-f0-9]{64}$') { throw 'release recovery pointer is invalid' }
    [pscustomobject]@{ Slot = $slot; Generation = $generation; Hash = $sha; Path = $path }
}

function Get-CatDeskLastKnownGoodRelease([string]$Root) {
    $valid = @()
    foreach ($slot in $script:CatDeskReleaseRecoverySlots) {
        try {
            $evidence = Read-CatDeskLkgSlot -Root $Root -Slot $slot
            if ($null -ne $evidence) { $valid += $evidence }
        } catch {
            # A damaged inactive slot is not authority. A pointer to it will be
            # rejected below; an intact alternate slot may still be recovered.
        }
    }
    if ($valid.Count -eq 0) { throw 'no trusted release recovery snapshot is available' }
    $pointer = $null
    try { $pointer = Read-CatDeskLkgPointer $Root } catch { $pointer = $null }
    if ($null -ne $pointer) {
        $selected = @($valid | Where-Object { $_.Slot -eq $pointer.Slot -and $_.Generation -eq $pointer.Generation -and $_.Hash -eq $pointer.Hash })
        if ($selected.Count -eq 1) { return $selected[0] }
    }
    $highest = ($valid | Measure-Object -Property Generation -Maximum).Maximum
    $candidates = @($valid | Where-Object { $_.Generation -eq $highest })
    if ($candidates.Count -ne 1) { throw 'release recovery snapshot authority is ambiguous' }
    return $candidates[0]
}

function Get-CatDeskCanonicalPairEvidence([string]$Root) {
    $rootPath = Resolve-CatDeskRecoveryRoot $Root
    $release = Join-Path $rootPath 'target\release'
    $binary = Assert-CatDeskRecoveryContainedPath -Root $rootPath -Path (Join-Path $release 'catdesk.exe') -Label 'canonical release binary'
    $manifest = Assert-CatDeskRecoveryContainedPath -Root $rootPath -Path (Join-Path $release 'catdesk.exe.sha256') -Label 'canonical release fingerprint'
    $manifestItem = Get-Item -LiteralPath $manifest -Force -ErrorAction Stop
    if ($manifestItem.PSIsContainer -or ($manifestItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -or $manifestItem.Length -gt 128) { throw 'canonical release fingerprint is invalid' }
    $expected = [IO.File]::ReadAllText($manifest).Trim().ToLowerInvariant()
    if ($expected -notmatch '^[a-f0-9]{64}$') { throw 'canonical release fingerprint is invalid' }
    $binaryItem = Get-Item -LiteralPath $binary -Force -ErrorAction Stop
    if ($binaryItem.PSIsContainer -or ($binaryItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -or $binaryItem.Length -lt 1 -or $binaryItem.Length -gt $script:CatDeskReleaseRecoveryMaxBinaryBytes) {
        throw 'canonical release binary is invalid'
    }
    $actual = Get-CatDeskRecoverySha256 $binary
    if ($actual -ne $expected) { throw 'canonical release pair does not match' }
    [pscustomobject]@{ Binary = $binary; Manifest = $manifest; Hash = $actual }
}

# A legacy installation may bootstrap escrow only from the durable authority
# record written by the reviewed promotion success boundary.  Reload receipts,
# process continuity, health, and review-bundle files are deliberately not
# inputs to this proof.
function Get-CatDeskQualifiedStoppedAuthority([string]$Root, [string]$ExpectedHash) {
    $rootPath=Resolve-CatDeskRecoveryRoot $Root
    $records=@{}
    foreach($name in @('authorization.json','claim.json','transaction.json','qualification.json','stopped-quiescence.json','qualified-stopped.json')) {
        $path=Assert-CatDeskRecoveryContainedPath $rootPath (Join-Path $rootPath ('.catdesk\release-pair-control\'+$name)) 'stopped authority'
        $item=Get-Item -LiteralPath $path -Force
        if($item.PSIsContainer -or $item.Length -lt 1 -or $item.Length -gt 12288){throw 'stopped authority is unavailable'}
        $records[$name]=ConvertFrom-CatDeskLiteralJson ([IO.File]::ReadAllText($path))
    }
    $outer=$records['authorization.json'];$a=$outer.authorization;$c=$records['claim.json'];$j=$records['transaction.json']
    $q=$records['qualification.json'];$exit=$records['stopped-quiescence.json'];$receipt=$records['qualified-stopped.json']
    $shapes=@(
        @{Value=$outer;Fields=@('schemaVersion','mode','controllerSha256','controllerSessionId','authorization')},
        @{Value=$a;Fields=@('schemaVersion','authorizationId','transactionId','expiresAtUnix','reviewedPreflightSha256','reviewedAuthorizationId','candidateRelativePath','candidateSha256','originalBinarySha256','originalSidecarFileSha256','originalSidecarValue','slotABinarySha256','slotASidecarFileSha256','slotAManifestSha256','originalPointerSha256','scriptSha256','recoveryHelperSha256','powershellSha256')},
        @{Value=$c;Fields=@('schemaVersion','mode','authorizationId','transactionId','state')},
        @{Value=$j;Fields=@('schemaVersion','mode','authorizationId','transactionId','phase','candidateSha256','originalBinarySha256','originalSidecarFileSha256')},
        @{Value=$q;Fields=@('schemaVersion','mode','profile','authorizationId','transactionId','candidateSha256','ownerPid','ownerStartedAtUtc','unauthorizedStatus','authorizedStatus','initializeVerified','toolsListVerified','toolCount','qualifiedAtUnix')},
        @{Value=$exit;Fields=@('schemaVersion','mode','state','authorizationId','transactionId','qualificationSha256','candidateJob','helperJob')},
        @{Value=$receipt;Fields=@('schemaVersion','mode','state','profile','authorizationId','transactionId','candidateSha256','qualificationSha256','quiescenceSha256')}
    )
    foreach($shape in $shapes){$names=@($shape.Value.PSObject.Properties.Name);if($names.Count -ne $shape.Fields.Count -or @($names|Where-Object{$_ -notin $shape.Fields}).Count){throw 'stopped authority fields differ'}}
    foreach($value in @($outer,$c,$j,$q,$exit,$receipt)) {if($value.schemaVersion -ne 2 -or $value.mode -cne 'QUALIFIED_STOPPED'){throw 'stopped authority mode differs'}}
    if($a.schemaVersion -ne 1 -or $a.authorizationId -cnotmatch '^[a-f0-9]{32}$' -or $a.transactionId -cnotmatch '^[a-f0-9]{32}$' -or
        $outer.controllerSha256 -cnotmatch '^[a-f0-9]{64}$' -or $ExpectedHash -cnotmatch '^[a-f0-9]{64}$' -or $a.candidateSha256 -cne $ExpectedHash){throw 'stopped authority identity differs'}
    foreach($value in @($c,$j,$q,$exit,$receipt)){if($value.authorizationId -cne $a.authorizationId -or $value.transactionId -cne $a.transactionId){throw 'stopped authority transaction differs'}}
    foreach($value in @($j,$q,$receipt)){if($value.candidateSha256 -cne $ExpectedHash){throw 'stopped authority candidate differs'}}
    if($c.state -cne 'STOPPED_CLAIMED_PENDING' -or $j.phase -cne 'COMPLETED' -or $exit.state -cne 'OWNED_TREE_EXITED' -or
        $receipt.state -cne 'QUALIFIED_STOPPED' -or $receipt.profile -cne 'HEADLESS_LOCAL' -or $q.profile -cne 'HEADLESS_LOCAL' -or
        $q.ownerPid -lt 1 -or $q.unauthorizedStatus -ne 401 -or $q.authorizedStatus -ne 200 -or $q.initializeVerified -isnot [bool] -or
        -not $q.initializeVerified -or $q.toolsListVerified -isnot [bool] -or -not $q.toolsListVerified -or $q.toolCount -lt 2 -or $q.qualifiedAtUnix -lt 1 -or
        $j.originalBinarySha256 -cne $a.originalBinarySha256 -or $j.originalSidecarFileSha256 -cne $a.originalSidecarFileSha256){throw 'stopped qualification or exit proof is incomplete'}
    [void]([DateTime]::Parse([string]$q.ownerStartedAtUtc))
    foreach($name in @('candidateSha256','originalBinarySha256','originalSidecarFileSha256','originalSidecarValue','slotABinarySha256','slotASidecarFileSha256','slotAManifestSha256','originalPointerSha256','scriptSha256','recoveryHelperSha256','powershellSha256','reviewedPreflightSha256')){if($a.$name -cnotmatch '^[a-f0-9]{64}$'){throw 'stopped authority digest differs'}}
    $control=Join-Path $rootPath '.catdesk\release-pair-control'
    $qualificationHash=Get-CatDeskRecoverySha256 (Join-Path $control 'qualification.json')
    if($receipt.qualificationSha256 -cne $qualificationHash -or $exit.qualificationSha256 -cne $qualificationHash -or
        $receipt.quiescenceSha256 -cne (Get-CatDeskRecoverySha256 (Join-Path $control 'stopped-quiescence.json')) -or
        $exit.candidateJob -cne "Local\CatDesk.ReleasePairStopped.$($a.transactionId).candidate" -or
        $exit.helperJob -cne "Local\CatDesk.ReleasePairStopped.$($a.transactionId).helper"){throw 'stopped receipt evidence digest differs'}
    # This authority means a reviewed image actually passed bounded local MCP
    # qualification and its owned tree exited. It asserts no serving lifetime,
    # ordinary native startup, Secure MCP ingress, or unattended operation.
    return $receipt
}

function Enter-CatDeskStoppedLifecycleLease([string]$Root) {
    $rootPath=Resolve-CatDeskRecoveryRoot $Root
    $control=Assert-CatDeskRecoveryContainedPath $rootPath (Join-Path $rootPath '.catdesk\release-pair-control') 'release-pair control' -AllowMissingLeaf
    if(-not(Test-Path -LiteralPath $control)){[void](New-Item -ItemType Directory -Path $control -Force -ErrorAction Stop)}
    [void](Assert-CatDeskRecoveryContainedPath $rootPath $control 'release-pair control')
    $lease=Assert-CatDeskRecoveryContainedPath $rootPath (Join-Path $control 'stopped-owner.lock') 'release-pair lifecycle lease' -AllowMissingLeaf
    return [IO.File]::Open($lease,[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
}

function Test-CatDeskStoppedReconciliationStartupReady {
    param([string]$Root,[System.IO.FileStream]$HeldLifecycleLease=$null)
    try{
        $rootPath=Resolve-CatDeskRecoveryRoot $Root
        $control=Join-Path $rootPath '.catdesk\release-pair-control'
        $lease=Join-Path $control 'stopped-owner.lock'
        # A typed internal lease lets the executing lifecycle inspect metadata
        # without mistaking its own exclusive handle for a competing owner.
        # No public CLI parameter can provide or synthesize this handle.
        if($null -ne $HeldLifecycleLease){
            if(-not $HeldLifecycleLease.CanRead -or -not $HeldLifecycleLease.CanWrite -or $HeldLifecycleLease.SafeFileHandle.IsClosed -or
                -not ([IO.Path]::GetFullPath($HeldLifecycleLease.Name)).Equals([IO.Path]::GetFullPath($lease),[StringComparison]::OrdinalIgnoreCase)){return $false}
        }elseif(Test-Path -LiteralPath $lease){
            [void](Assert-CatDeskRecoveryContainedPath $rootPath $lease 'release-pair lifecycle lease')
            $probe=[IO.File]::Open($lease,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::None)
            $probe.Dispose()
        }
        if(-not(Test-Path -LiteralPath $control)){return $true}
        [void](Assert-CatDeskRecoveryContainedPath $rootPath $control 'release-pair control')
        $entries=@(Get-ChildItem -LiteralPath $control -Force -ErrorAction Stop)
        if($entries.Count -gt 128){return $false}
        $jsonNames=@('preflight.json','authorization.json','claim.json','transaction.json','serving-pending.json','job-release.json','quiescence.json','result.json','reconciled-release.json','stopped-owner.json','stopped-recovery-exit.json','qualification.json','stopped-quiescence.json','qualified-stopped.json')
        $outer=$null;$hasStopped=$false;$hasLegacy=$false;$hasAuxiliary=$false;$legacyPreparedOnly=$true
        foreach($entry in $entries){
            [void](Assert-CatDeskRecoveryContainedPath $rootPath $entry.FullName 'release-pair residual')
            if($entry.Name -ceq 'stopped-owner.lock'){continue}
            if($entry.PSIsContainer){if($entry.Name -ceq 'escrow'){$hasAuxiliary=$true;continue};return $false}
            if($entry.Name -cnotin $jsonNames){
                if($entry.Name -ceq 'worker-owner' -or $entry.Name -ceq 'qualification-config.toml' -or
                    $entry.Name -cmatch '^(?:\.stopped-[a-f0-9]{32}\.tmp|[a-z-]+\.json\.(?:tmp|previous)\.interrupted-[a-f0-9]{32})$'){$hasAuxiliary=$true;continue}
                return $false
            }
            if($entry.Length -lt 1 -or $entry.Length -gt 12288){return $false}
            $record=ConvertFrom-CatDeskLiteralJson ([IO.File]::ReadAllText($entry.FullName))
            if($null -eq $record -or $record -is [array] -or -not $record.PSObject.Properties['schemaVersion']){return $false}
            if($record.schemaVersion -isnot [int] -and $record.schemaVersion -isnot [long]){return $false}
            if($record.schemaVersion -eq 2 -and $record.PSObject.Properties['mode'] -and $record.mode -ceq 'QUALIFIED_STOPPED'){$hasStopped=$true}
            elseif($record.schemaVersion -eq 1 -and -not $record.PSObject.Properties['mode']){$hasLegacy=$true}
            else{return $false}
            if($entry.Name -cne 'preflight.json'){$legacyPreparedOnly=$false}
            if($entry.Name -ceq 'authorization.json'){$outer=$record}
        }
        if($null -eq $outer){return (-not $hasStopped -and -not $hasAuxiliary -and (-not $hasLegacy -or $legacyPreparedOnly))}
        if($outer.schemaVersion -eq 1){
            $fields=@('schemaVersion','authorizationId','transactionId','expiresAtUnix','reviewedPreflightSha256','reviewedAuthorizationId','candidateRelativePath','candidateSha256','originalBinarySha256','originalSidecarFileSha256','originalSidecarValue','slotABinarySha256','slotASidecarFileSha256','slotAManifestSha256','originalPointerSha256','scriptSha256','recoveryHelperSha256','powershellSha256')
            $names=@($outer.PSObject.Properties.Name)
            if($names.Count -ne $fields.Count -or @($names|Where-Object{$_ -notin $fields}).Count){return $false}
            return (-not $hasStopped)
        }
        if($hasLegacy){return $false}
        [void](Get-CatDeskQualifiedStoppedAuthority $rootPath $outer.authorization.candidateSha256)
        return $true
    }catch{return $false}
}

function Get-CatDeskReviewedPromotionAuthority([string]$Root, $Canonical) {
    $rootPath = Resolve-CatDeskRecoveryRoot $Root
    if ($null -eq $Canonical -or [string]$Canonical.Hash -notmatch '^[a-f0-9]{64}$') { throw 'LKG_AUTHORITY_MISSING' }
    $path = Join-Path $rootPath '.catdesk\promotion-recovery\reviewed-promotion.json'
    $authorityItem = Get-Item -LiteralPath $path -Force -ErrorAction SilentlyContinue
    if ($null -eq $authorityItem) { throw 'LKG_AUTHORITY_MISSING' }
    if ($authorityItem.PSIsContainer -or ($authorityItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'LKG_AUTHORITY_AMBIGUOUS_OR_DAMAGED' }
    try {
        $path = Assert-CatDeskRecoveryContainedPath -Root $rootPath -Path $path -Label 'reviewed promotion authority'
        $item = Get-Item -LiteralPath $path -Force -ErrorAction Stop
        if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -or $item.Length -lt 1 -or $item.Length -gt 2048) { throw 'invalid' }
        $record = ConvertFrom-CatDeskLiteralJson ([IO.File]::ReadAllText($path))
        $required = @('schemaVersion','transactionId','authorizationId','stage','candidateHash','canonicalRelativePath','canonicalSha256')
        $names = @($record.PSObject.Properties.Name)
        if ($names.Count -ne $required.Count -or @($names | Where-Object { $_ -notin $required }).Count -ne 0) { throw 'invalid' }
        if (($record.schemaVersion -isnot [int] -and $record.schemaVersion -isnot [long]) -or $record.schemaVersion -ne 1 -or [string]$record.stage -ne 'CANONICAL_HANDOFF_PROVEN') { throw 'invalid' }
        if ([string]$record.transactionId -notmatch '^[0-9a-f]{32}$' -or [string]$record.authorizationId -notmatch '^[0-9a-f]{32}$' -or [string]$record.candidateHash -notmatch '^[a-f0-9]{64}$' -or [string]$record.canonicalSha256 -notmatch '^[a-f0-9]{64}$') { throw 'invalid' }
        if ([string]$record.canonicalRelativePath -ne 'target\release\catdesk.exe') { throw 'invalid' }
        if (-not ([string]$record.candidateHash).Equals([string]$record.canonicalSha256,[StringComparison]::Ordinal) -or -not ([string]$record.canonicalSha256).Equals([string]$Canonical.Hash,[StringComparison]::Ordinal)) { throw 'mismatch' }
        return $record
    } catch {
        if ($_.Exception.Message -eq 'mismatch') { throw 'LKG_AUTHORITY_MISSING' }
        throw 'LKG_AUTHORITY_AMBIGUOUS_OR_DAMAGED'
    }
}

function Initialize-CatDeskLastKnownGoodFromReviewedPromotion([string]$Root, $Canonical) {
    try { return Get-CatDeskLastKnownGoodRelease $Root } catch {}
    [void](Get-CatDeskReviewedPromotionAuthority -Root $Root -Canonical $Canonical)
    return Save-CatDeskLastKnownGoodRelease -Root $Root -Canonical $Canonical
}

function Get-CatDeskLkgAuthorityFailure([string]$Root) {
    try {
        $directory = Get-CatDeskReleaseRecoveryDirectory $Root
        if (-not (Test-Path -LiteralPath $directory)) { return 'LKG_AUTHORITY_MISSING' }
        $items = @(Get-ChildItem -LiteralPath $directory -Force -ErrorAction Stop)
        if ($items.Count -eq 0) { return 'LKG_AUTHORITY_MISSING' }

        # Older CatDesk releases legitimately wrote operational_verified slots.
        # Current policy correctly refuses those as rollback authority, but a
        # well-formed legacy-only escrow is not evidence that reviewed authority
        # is corrupt or ambiguous. Classify it as missing reviewed authority so
        # diagnostics distinguish migration debt from damaged trusted state.
        $legacySlots = 0
        foreach ($slot in $script:CatDeskReleaseRecoverySlots) {
            $slotPath = Join-Path $directory $slot
            if (-not (Test-Path -LiteralPath $slotPath)) { continue }
            $slotItem = Get-Item -LiteralPath $slotPath -Force -ErrorAction Stop
            if (-not $slotItem.PSIsContainer -or ($slotItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
                return 'LKG_AUTHORITY_AMBIGUOUS_OR_DAMAGED'
            }
            $manifestPath = Join-Path $slotPath 'manifest.json'
            if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
                return 'LKG_AUTHORITY_AMBIGUOUS_OR_DAMAGED'
            }
            $manifest = ConvertFrom-CatDeskLiteralJson ([IO.File]::ReadAllText($manifestPath))
            if ([string]$manifest.source -eq 'operational_verified') {
                $legacySlots++
                continue
            }
            return 'LKG_AUTHORITY_AMBIGUOUS_OR_DAMAGED'
        }
        $unexpected = @($items | Where-Object { $_.Name -notin @('current.json','slot-a','slot-b') })
        if ($legacySlots -gt 0 -and $unexpected.Count -eq 0) {
            return 'LKG_AUTHORITY_MISSING'
        }
    } catch {}
    return 'LKG_AUTHORITY_AMBIGUOUS_OR_DAMAGED'
}

function Save-CatDeskLastKnownGoodRelease([string]$Root, $Canonical, [string]$Source = 'reviewed_promotion') {
    if ($Source -notin @('reviewed_promotion', 'operational_verified')) { throw 'release recovery source is invalid' }
    $rootPath = Resolve-CatDeskRecoveryRoot $Root
    $canonicalEvidence = Get-CatDeskCanonicalPairEvidence $rootPath
    if ($null -eq $Canonical -or [string]$Canonical.Hash -ne $canonicalEvidence.Hash) { throw 'reviewed canonical evidence did not match disk' }
    if ($Canonical.PSObject.Properties['Binary'] -and (Resolve-Path -LiteralPath $Canonical.Binary -ErrorAction Stop).Path -ne $canonicalEvidence.Binary) { throw 'reviewed canonical binary path did not match disk' }

    # Operational health proves that the current canonical pair can keep running;
    # it is not reviewed-promotion authority and must never mint or advance LKG.
    # A matching pre-existing LKG may be re-used, but a missing/different escrow
    # remains a separate recovery-authority condition.
    if ($Source -eq 'operational_verified') {
        try { $operationalLkg = Get-CatDeskLastKnownGoodRelease $rootPath } catch { throw 'LKG_AUTHORITY_MISSING' }
        if ($null -eq $operationalLkg -or $operationalLkg.Hash -ne $canonicalEvidence.Hash) { throw 'LKG_AUTHORITY_MISSING' }
        return $operationalLkg
    }

    $recovery = Get-CatDeskReleaseRecoveryDirectory $rootPath
    $control = Join-Path $rootPath '.catdesk'
    if (-not (Test-Path -LiteralPath $control)) {
        New-Item -ItemType Directory -Path $control -ErrorAction Stop | Out-Null
    }
    $controlItem = Get-Item -LiteralPath $control -Force -ErrorAction Stop
    if (-not $controlItem.PSIsContainer -or ($controlItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw 'release recovery control directory is invalid'
    }
    if (-not (Test-Path -LiteralPath $recovery -PathType Container)) { New-Item -ItemType Directory -Path $recovery -ErrorAction Stop | Out-Null }
    $recoveryItem = Get-Item -LiteralPath $recovery -Force -ErrorAction Stop
    if (-not $recoveryItem.PSIsContainer -or ($recoveryItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'release recovery directory is invalid' }

    $valid = @()
    foreach ($slot in $script:CatDeskReleaseRecoverySlots) {
        try { $item = Read-CatDeskLkgSlot -Root $rootPath -Slot $slot; if ($null -ne $item) { $valid += $item } } catch {}
    }
    $maxGeneration = if ($valid.Count) { [int](($valid | Measure-Object -Property Generation -Maximum).Maximum) } else { 0 }
    $current = $null
    try { $current = Get-CatDeskLastKnownGoodRelease $rootPath } catch {}
    if ($null -ne $current -and $current.Hash -eq $canonicalEvidence.Hash) {
        $pointerMatches = $false
        try {
            $pointer = Read-CatDeskLkgPointer $rootPath
            $pointerMatches = $null -ne $pointer -and $pointer.Slot -eq $current.Slot -and $pointer.Generation -eq $current.Generation -and $pointer.Hash -eq $current.Hash
        } catch { $pointerMatches = $false }
        if (-not $pointerMatches) {
            $pointer = [ordered]@{ schemaVersion = $script:CatDeskReleaseRecoverySchemaVersion; slot = $current.Slot; generation = $current.Generation; sha256 = $current.Hash }
            Write-CatDeskAtomicUtf8 -Path (Join-Path $recovery 'current.json') -Text ($pointer | ConvertTo-Json -Compress)
            $repairedPointer = Read-CatDeskLkgPointer $rootPath
            if ($null -eq $repairedPointer -or $repairedPointer.Slot -ne $current.Slot -or $repairedPointer.Generation -ne $current.Generation -or $repairedPointer.Hash -ne $current.Hash) {
                throw 'release recovery pointer repair validation failed'
            }
        }
        return $current
    }
    $inactive = if ($null -ne $current -and $current.Slot -eq 'slot-a') { 'slot-b' } else { 'slot-a' }
    $slotPath = Join-Path $recovery $inactive
    if (Test-Path -LiteralPath $slotPath) {
        $slotItem = Get-Item -LiteralPath $slotPath -Force -ErrorAction Stop
        if (-not $slotItem.PSIsContainer -or ($slotItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'inactive release recovery slot is unsafe' }
        Remove-Item -LiteralPath $slotPath -Recurse -Force -ErrorAction Stop
    }
    New-Item -ItemType Directory -Path $slotPath -ErrorAction Stop | Out-Null
    $generation = $maxGeneration + 1
    $binary = Join-Path $slotPath 'catdesk.exe'
    $shaPath = Join-Path $slotPath 'catdesk.exe.sha256'
    $manifestPath = Join-Path $slotPath 'manifest.json'
    Copy-CatDeskDurableFile -Source $canonicalEvidence.Binary -Destination $binary
    if ((Get-CatDeskRecoverySha256 $binary) -ne $canonicalEvidence.Hash) { throw 'release recovery staged binary validation failed' }
    Write-CatDeskDurableBytes -Path $shaPath -Bytes ([Text.UTF8Encoding]::new($false).GetBytes("$($canonicalEvidence.Hash)`n"))
    $manifest = [ordered]@{ schemaVersion = $script:CatDeskReleaseRecoverySchemaVersion; generation = $generation; sha256 = $canonicalEvidence.Hash; source = $Source; capturedAtUtc = [DateTime]::UtcNow.ToString('o') }
    Write-CatDeskDurableBytes -Path $manifestPath -Bytes ([Text.UTF8Encoding]::new($false).GetBytes(($manifest | ConvertTo-Json -Compress)))
    $staged = Read-CatDeskLkgSlot -Root $rootPath -Slot $inactive
    if ($staged.Generation -ne $generation -or $staged.Hash -ne $canonicalEvidence.Hash) { throw 'release recovery staged slot validation failed' }
    $pointer = [ordered]@{ schemaVersion = $script:CatDeskReleaseRecoverySchemaVersion; slot = $inactive; generation = $generation; sha256 = $canonicalEvidence.Hash }
    Write-CatDeskAtomicUtf8 -Path (Join-Path $recovery 'current.json') -Text ($pointer | ConvertTo-Json -Compress)
    $final = Get-CatDeskLastKnownGoodRelease $rootPath
    if ($final.Slot -ne $inactive -or $final.Generation -ne $generation -or $final.Hash -ne $canonicalEvidence.Hash) { throw 'release recovery pointer validation failed' }
    return $final
}

function Replace-CatDeskRecoveryFile([string]$Source, [string]$Destination, [string]$ExpectedHash = '') {
    $temporary = "$Destination.recovering"
    $backup = "$Destination.pre-recovery"
    foreach ($stale in @($temporary, $backup)) {
        if (Test-Path -LiteralPath $stale) {
            $item = Get-Item -LiteralPath $stale -Force -ErrorAction Stop
            if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'canonical recovery temporary state is unsafe' }
            Remove-Item -LiteralPath $stale -Force -ErrorAction Stop
        }
    }
    Copy-CatDeskDurableFile -Source $Source -Destination $temporary
    if ($ExpectedHash -and (Get-CatDeskRecoverySha256 $temporary) -ne $ExpectedHash) { throw 'canonical recovery staged binary validation failed' }
    if (Test-Path -LiteralPath $Destination -PathType Leaf) {
        [IO.File]::Replace($temporary, $Destination, $backup, $true)
    } else {
        Move-Item -LiteralPath $temporary -Destination $Destination -ErrorAction Stop
    }
}

function Restore-CatDeskCanonicalFromLastKnownGood([string]$Root, [string]$ExpectedHash = '') {
    $rootPath = Resolve-CatDeskRecoveryRoot $Root
    $trusted = Get-CatDeskLastKnownGoodRelease $rootPath
    if ($ExpectedHash -and $ExpectedHash.ToLowerInvariant() -ne $trusted.Hash) { throw 'trusted release recovery snapshot does not match requested hash' }
    $release = Join-Path $rootPath 'target\release'
    if (-not (Test-Path -LiteralPath $release -PathType Container)) { New-Item -ItemType Directory -Path $release -Force -ErrorAction Stop | Out-Null }
    $releaseItem = Get-Item -LiteralPath $release -Force -ErrorAction Stop
    if (-not $releaseItem.PSIsContainer -or ($releaseItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'canonical release directory is invalid' }
    $binary = Join-Path $release 'catdesk.exe'
    $manifest = Join-Path $release 'catdesk.exe.sha256'

    $binaryMatches = $false
    if (Test-Path -LiteralPath $binary -PathType Leaf) {
        try { $binaryMatches = (Get-CatDeskRecoverySha256 $binary) -eq $trusted.Hash } catch { $binaryMatches = $false }
    }
    if (-not $binaryMatches) {
        Replace-CatDeskRecoveryFile -Source $trusted.Binary -Destination $binary -ExpectedHash $trusted.Hash
    }

    $manifestMatches = $false
    if (Test-Path -LiteralPath $manifest -PathType Leaf) {
        try { $manifestMatches = ([IO.File]::ReadAllText($manifest).Trim().ToLowerInvariant() -eq $trusted.Hash) } catch { $manifestMatches = $false }
    }
    if (-not $manifestMatches) {
        $manifestTemp = "$manifest.recovering"
        $manifestBackup = "$manifest.pre-recovery"
        foreach ($stale in @($manifestTemp, $manifestBackup)) {
            if (Test-Path -LiteralPath $stale) {
                $item = Get-Item -LiteralPath $stale -Force -ErrorAction Stop
                if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'canonical recovery manifest temporary state is unsafe' }
                Remove-Item -LiteralPath $stale -Force -ErrorAction Stop
            }
        }
        Write-CatDeskDurableBytes -Path $manifestTemp -Bytes ([Text.UTF8Encoding]::new($false).GetBytes("$($trusted.Hash)`n"))
        if (Test-Path -LiteralPath $manifest -PathType Leaf) {
            [IO.File]::Replace($manifestTemp, $manifest, $manifestBackup, $true)
        } else {
            Move-Item -LiteralPath $manifestTemp -Destination $manifest -ErrorAction Stop
        }
    }

    $canonical = Get-CatDeskCanonicalPairEvidence $rootPath
    if ($canonical.Hash -ne $trusted.Hash) { throw 'canonical release recovery validation failed' }
    foreach ($backup in @("$binary.pre-recovery", "$manifest.pre-recovery")) {
        if (Test-Path -LiteralPath $backup -PathType Leaf) { Remove-Item -LiteralPath $backup -Force -ErrorAction Stop }
    }
    [pscustomobject]@{ Binary = $canonical.Binary; Manifest = $canonical.Manifest; Hash = $canonical.Hash; RestoredBinary = (-not $binaryMatches); RestoredManifest = (-not $manifestMatches); Generation = $trusted.Generation; Slot = $trusted.Slot }
}
