[CmdletBinding()]
param(
    [switch]$ElevatedChild
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# Exact ChatGPT-designated T-0366 release authority.
$ExpectedCandidateSha256 = 'd09c677f8ce5f14b3600a5435929aff31b4128d9a041cdaceae3213b5f0af36f'
$ExpectedCandidateLength = 26302464
$ExpectedEnvelopeSha256 = '42ababb6dcd47ec20db97775d7d441eb9fcadc806d3f304b7f221809bfda892c'
$ExpectedEnvelopeLength = 507
$ExpectedSuccess = 'REVIEWED_BUILD_MAIN_IMAGE_ROTATED_PENDING_T0212_ACCEPTANCE'
$RotationFlag = '--catdesk-reviewed-main-image-rotate-fixed-policy'

# Exact accepted predecessor and T-0372 protected-state readback.
$Epoch1InstalledSha256 = '2421a90aeb9ad7775ec9927f8dfbe294faa3cbfe11ec7a071a1ed554eee28459'
$Epoch1InstalledLength = 25134592
$Epoch1AcceptedEnvelopeSha256 = '2684020a2bb4c4be81a207af713cdb4239a41e65a48628c07f3bbbaca42b3a81'
$Epoch1AcceptedEnvelopeLength = 527

# Exact historical signed T-0217 epoch-2 incoming transport object.
$HistoricalT0217SignedEnvelopeSha256 = '50194cf9613e8b9b541db597045fb2fc73a70511cbe90efcf1b512b19ce02644'
$HistoricalT0217SignedEnvelopeLength = 519

$ProjectRoot = Split-Path -Parent $PSScriptRoot
$Candidate = Join-Path $ProjectRoot '.catdesk\candidates\t0366-build\release\catdesk.exe'
$Envelope = Join-Path $ProjectRoot 'docs\orchestrator\review_bundles\T-0366_T0324_ROTATION_SIGNED_ENVELOPE.v1'
$IncomingRoot = 'C:\ProgramData\CatDesk\reviewed-main-image\rotation\incoming'
$IncomingCandidate = Join-Path $IncomingRoot 'CatDesk.exe'
$IncomingEnvelope = Join-Path $IncomingRoot 'review-envelope.v1'
$InstalledCandidate = 'C:\Program Files\CatDesk\CatDesk.exe'
$AcceptedBootstrapEnvelope = 'C:\Program Files\CatDesk.reviewed-main-image-accepted.v1'
$PendingEnvelope = 'C:\Program Files\CatDesk.reviewed-main-image-rotation-pending.v1'
$InstalledEnvelope = 'C:\Program Files\CatDesk.reviewed-main-image-rotation-installed.v1'
$RotationStaging = 'C:\Program Files\CatDesk\CatDesk.rotation-next.exe'
$ResultFile = Join-Path $ProjectRoot '.catdesk\t0373-rotation-result.json'
$Archived = [System.Collections.Generic.List[string]]::new()

function Get-Sha256Lower([string]$Path) {
    (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Get-Evidence([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return $null }
    $item = Get-Item -LiteralPath $Path
    [pscustomobject]@{
        Length = [long]$item.Length
        Sha256 = Get-Sha256Lower $Path
    }
}

function Assert-ExactFile([string]$Path, [long]$Length, [string]$Sha256, [string]$Label) {
    $e = Get-Evidence $Path
    if ($null -eq $e) { throw "$Label is missing: $Path" }
    if ($e.Length -ne $Length) { throw "$Label length mismatch: expected $Length, found $($e.Length)" }
    if ($e.Sha256 -ne $Sha256) { throw "$Label SHA-256 mismatch: expected $Sha256, found $($e.Sha256)" }
}

function Assert-Absent([string]$Path, [string]$Label) {
    if (Test-Path -LiteralPath $Path) {
        $e = Get-Evidence $Path
        if ($null -eq $e) { throw "$Label exists but is not a regular file; refusing mutation: $Path" }
        throw "$Label unexpectedly exists. length=$($e.Length) sha256=$($e.Sha256). Refusing mutation."
    }
}

function Write-Result([string]$State, [string]$Message) {
    $record = [ordered]@{
        schemaVersion = 1
        task = 'T-0373'
        state = $State
        message = $Message
        candidateSha256 = $ExpectedCandidateSha256
        candidateLength = $ExpectedCandidateLength
        envelopeSha256 = $ExpectedEnvelopeSha256
        archivedKnownTransport = @($Archived)
        timestampUtc = [DateTime]::UtcNow.ToString('o')
    }
    $record | ConvertTo-Json -Compress -Depth 4 | Set-Content -LiteralPath $ResultFile -Encoding UTF8
}

# Prove project-side designated authority before elevation.
Assert-ExactFile $Candidate $ExpectedCandidateLength $ExpectedCandidateSha256 'T-0366 designated candidate'
Assert-ExactFile $Envelope $ExpectedEnvelopeLength $ExpectedEnvelopeSha256 'T-0366 signed envelope'

$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [Security.Principal.WindowsPrincipal]::new($identity)
$isAdmin = $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)

if (-not $isAdmin) {
    if ($ElevatedChild) { throw 'T-0373 elevated child did not receive an Administrator token.' }
    if (Test-Path -LiteralPath $ResultFile) { Remove-Item -LiteralPath $ResultFile -Force }
    Write-Host 'T-0373 requesting one UAC elevation and WAITING. It will consume epoch 2 only if protected pending/installed/staging state is still absent and exact T-0372 predecessor evidence remains unchanged.'
    $child = Start-Process -FilePath 'powershell.exe' -Verb RunAs -Wait -PassThru -ArgumentList @(
        '-NoProfile',
        '-ExecutionPolicy','Bypass',
        '-File',('"' + $PSCommandPath + '"'),
        '-ElevatedChild'
    )
    if (-not (Test-Path -LiteralPath $ResultFile -PathType Leaf)) {
        throw "T-0373 elevated process exited with code $($child.ExitCode) but produced no durable result receipt."
    }
    $result = Get-Content -LiteralPath $ResultFile -Raw | ConvertFrom-Json
    if ($child.ExitCode -ne 0 -or $result.state -ne 'SUCCESS') {
        throw "T-0373 elevated rotation failed. ExitCode=$($child.ExitCode). Result=$($result.message)"
    }
    Write-Host 'T0373_ROTATION_VERIFIED_SUCCESS'
    Write-Host $result.message
    if ($result.archivedKnownTransport.Count -gt 0) {
        Write-Host 'Archived exact historical transport:'
        $result.archivedKnownTransport | ForEach-Object { Write-Host $_ }
    }
    exit 0
}

try {
    # Revalidate every protected-state fact that justified keeping T-0366 at epoch 2.
    Assert-ExactFile $AcceptedBootstrapEnvelope $Epoch1AcceptedEnvelopeLength $Epoch1AcceptedEnvelopeSha256 'Accepted T-0215 bootstrap envelope'

    $installed = Get-Evidence $InstalledCandidate
    if ($null -eq $installed) { throw 'Installed reviewed main image is missing; refusing rotation.' }

    if ($installed.Length -eq $ExpectedCandidateLength -and $installed.Sha256 -eq $ExpectedCandidateSha256) {
        Assert-ExactFile $InstalledEnvelope $ExpectedEnvelopeLength $ExpectedEnvelopeSha256 'Installed T-0366 rotation envelope'
        Assert-Absent $PendingEnvelope 'Pending rotation envelope after installed T-0366 state'
        Assert-Absent $RotationStaging 'Rotation staging candidate after installed T-0366 state'
        $message = "Already installed and verified; Installed SHA256=$ExpectedCandidateSha256; Installed length=$ExpectedCandidateLength; Installed envelope SHA256=$ExpectedEnvelopeSha256"
        Write-Result 'SUCCESS' $message
        Write-Host $ExpectedSuccess
        exit 0
    }

    if ($installed.Length -ne $Epoch1InstalledLength -or $installed.Sha256 -ne $Epoch1InstalledSha256) {
        throw "Installed image is neither exact T-0215 predecessor nor exact T-0366 candidate. length=$($installed.Length) sha256=$($installed.Sha256). Refusing mutation."
    }

    Assert-Absent $PendingEnvelope 'Protected pending rotation envelope'
    Assert-Absent $InstalledEnvelope 'Protected installed rotation envelope'
    Assert-Absent $RotationStaging 'Protected rotation staging candidate'

    # T-0371 already placed the exact T-0366 payload in fixed incoming transport.
    Assert-ExactFile $IncomingCandidate $ExpectedCandidateLength $ExpectedCandidateSha256 'Incoming T-0366 candidate'

    $existingEnvelope = Get-Evidence $IncomingEnvelope
    if ($null -eq $existingEnvelope) {
        throw 'Incoming rotation envelope is missing; refusing to infer transport state.'
    }

    if ($existingEnvelope.Length -eq $HistoricalT0217SignedEnvelopeLength -and $existingEnvelope.Sha256 -eq $HistoricalT0217SignedEnvelopeSha256) {
        $stamp = [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfffZ')
        $archive = "$IncomingEnvelope.superseded-t0217-signed-$stamp"
        Move-Item -LiteralPath $IncomingEnvelope -Destination $archive
        Assert-ExactFile $archive $HistoricalT0217SignedEnvelopeLength $HistoricalT0217SignedEnvelopeSha256 'Archived signed T-0217 transport envelope'
        $Archived.Add($archive) | Out-Null
    } elseif (-not ($existingEnvelope.Length -eq $ExpectedEnvelopeLength -and $existingEnvelope.Sha256 -eq $ExpectedEnvelopeSha256)) {
        throw "Incoming envelope is neither exact signed T-0217 transport nor exact T-0366 envelope. length=$($existingEnvelope.Length) sha256=$($existingEnvelope.Sha256). Refusing mutation."
    }

    if (-not (Test-Path -LiteralPath $IncomingEnvelope -PathType Leaf)) {
        $next = "$IncomingEnvelope.t0373.next"
        if (Test-Path -LiteralPath $next) {
            Assert-ExactFile $next $ExpectedEnvelopeLength $ExpectedEnvelopeSha256 'Existing T-0373 envelope staging object'
        } else {
            Copy-Item -LiteralPath $Envelope -Destination $next
            Assert-ExactFile $next $ExpectedEnvelopeLength $ExpectedEnvelopeSha256 'Staged T-0366 envelope'
        }
        Move-Item -LiteralPath $next -Destination $IncomingEnvelope
    }

    Assert-ExactFile $IncomingEnvelope $ExpectedEnvelopeLength $ExpectedEnvelopeSha256 'Incoming T-0366 envelope'
    Assert-ExactFile $IncomingCandidate $ExpectedCandidateLength $ExpectedCandidateSha256 'Incoming T-0366 candidate before rotation'

    # Recheck protected absence immediately before calling the reviewed zero-input state machine.
    Assert-Absent $PendingEnvelope 'Protected pending rotation envelope before state-machine entry'
    Assert-Absent $InstalledEnvelope 'Protected installed rotation envelope before state-machine entry'
    Assert-Absent $RotationStaging 'Protected rotation staging candidate before state-machine entry'

    $output = & $Candidate $RotationFlag 2>&1 | Out-String
    $exitCode = $LASTEXITCODE
    $output = $output.Trim()
    if ($exitCode -ne 0) { throw "Reviewed main-image rotation returned exit code $exitCode. Output: $output" }
    if ($output -notmatch [regex]::Escape($ExpectedSuccess)) {
        throw "Reviewed main-image rotation did not return the accepted success receipt. Output: $output"
    }

    Assert-ExactFile $InstalledCandidate $ExpectedCandidateLength $ExpectedCandidateSha256 'Installed T-0366 reviewed main image'
    Assert-ExactFile $InstalledEnvelope $ExpectedEnvelopeLength $ExpectedEnvelopeSha256 'Installed T-0366 rotation envelope'
    Assert-Absent $PendingEnvelope 'Pending rotation envelope after successful T-0366 install'
    Assert-Absent $RotationStaging 'Rotation staging candidate after successful T-0366 install'
    Assert-ExactFile $AcceptedBootstrapEnvelope $Epoch1AcceptedEnvelopeLength $Epoch1AcceptedEnvelopeSha256 'Accepted T-0215 bootstrap envelope after rotation'

    $message = "$ExpectedSuccess; Installed SHA256=$ExpectedCandidateSha256; Installed length=$ExpectedCandidateLength; Installed envelope SHA256=$ExpectedEnvelopeSha256; accepted bootstrap envelope unchanged"
    Write-Result 'SUCCESS' $message
    Write-Host $ExpectedSuccess
    exit 0
} catch {
    $message = $_.Exception.Message
    try { Write-Result 'FAILED' $message } catch {}
    Write-Error $message
    exit 1
}
