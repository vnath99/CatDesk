[CmdletBinding()]
param(
    [switch]$ElevatedChild
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$ExpectedCandidateSha256 = 'd09c677f8ce5f14b3600a5435929aff31b4128d9a041cdaceae3213b5f0af36f'
$ExpectedCandidateLength = 26302464
$ExpectedEnvelopeSha256 = '42ababb6dcd47ec20db97775d7d441eb9fcadc806d3f304b7f221809bfda892c'
$ExpectedEnvelopeLength = 507
$ExpectedSuccess = 'REVIEWED_BUILD_MAIN_IMAGE_ROTATED_PENDING_T0212_ACCEPTANCE'
$RotationFlag = '--catdesk-reviewed-main-image-rotate-fixed-policy'

# Known historical predecessor and stale transport evidence from accepted project history.
$Epoch1InstalledSha256 = '2421a90aeb9ad7775ec9927f8dfbe294faa3cbfe11ec7a071a1ed554eee28459'
$Epoch1InstalledLength = 25134592
$HistoricalT0217CandidateSha256 = '552cb917998d283e7d901593ffed5a9ceaa108223654aa69349fa7e567c21482'
$HistoricalT0217CandidateLength = 25172480
$HistoricalT0217UnsignedPayloadSha256 = 'eaad9c085257a9fa459d2f4771f69c360dda6930b120778c2efeaaa5d2e8a308'
$HistoricalT0217UnsignedPayloadLength = 422

$ProjectRoot = Split-Path -Parent $PSScriptRoot
$Candidate = Join-Path $ProjectRoot '.catdesk\candidates\t0366-build\release\catdesk.exe'
$Envelope = Join-Path $ProjectRoot 'docs\orchestrator\review_bundles\T-0366_T0324_ROTATION_SIGNED_ENVELOPE.v1'
$IncomingRoot = 'C:\ProgramData\CatDesk\reviewed-main-image\rotation\incoming'
$IncomingCandidate = Join-Path $IncomingRoot 'CatDesk.exe'
$IncomingEnvelope = Join-Path $IncomingRoot 'review-envelope.v1'
$InstalledCandidate = 'C:\Program Files\CatDesk\CatDesk.exe'
$InstalledEnvelope = 'C:\Program Files\CatDesk.reviewed-main-image-rotation-installed.v1'
$ResultFile = Join-Path $ProjectRoot '.catdesk\t0371-rotation-result.json'
$Archived = [System.Collections.Generic.List[string]]::new()

function Get-Sha256Lower([string]$Path) {
    (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Get-Evidence([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        return $null
    }
    $item = Get-Item -LiteralPath $Path
    [pscustomobject]@{
        Length = [long]$item.Length
        Sha256 = Get-Sha256Lower $Path
    }
}

function Assert-ExactFile([string]$Path, [long]$Length, [string]$Sha256, [string]$Label) {
    $e = Get-Evidence $Path
    if ($null -eq $e) {
        throw "$Label is missing: $Path"
    }
    if ($e.Length -ne $Length) {
        throw "$Label length mismatch: expected $Length, found $($e.Length)"
    }
    if ($e.Sha256 -ne $Sha256) {
        throw "$Label SHA-256 mismatch: expected $Sha256, found $($e.Sha256)"
    }
}

function Archive-KnownStaleFile([string]$Path, [long]$Length, [string]$Sha256, [string]$Label) {
    $e = Get-Evidence $Path
    if ($null -eq $e) { return $false }
    if ($e.Length -ne $Length -or $e.Sha256 -ne $Sha256) {
        throw "$Label is not the known historical stale object. Observed length=$($e.Length) sha256=$($e.Sha256). Refusing mutation."
    }
    $stamp = [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfffZ')
    $archive = "$Path.superseded-t0217-$stamp"
    Move-Item -LiteralPath $Path -Destination $archive
    Assert-ExactFile $archive $Length $Sha256 "$Label archived evidence"
    $Archived.Add($archive) | Out-Null
    return $true
}

function Install-ExactIncoming(
    [string]$Source,
    [string]$Destination,
    [long]$Length,
    [string]$Sha256,
    [string]$Label,
    [long]$KnownStaleLength,
    [string]$KnownStaleSha256
) {
    $existing = Get-Evidence $Destination
    if ($null -ne $existing) {
        if ($existing.Length -eq $Length -and $existing.Sha256 -eq $Sha256) {
            return
        }
        Archive-KnownStaleFile $Destination $KnownStaleLength $KnownStaleSha256 "$Label existing fixed input" | Out-Null
    }

    $next = "$Destination.t0371.next"
    $staged = Get-Evidence $next
    if ($null -ne $staged) {
        if ($staged.Length -ne $Length -or $staged.Sha256 -ne $Sha256) {
            throw "$Label staged next object is conflicting. Observed length=$($staged.Length) sha256=$($staged.Sha256). Refusing mutation."
        }
    } else {
        Copy-Item -LiteralPath $Source -Destination $next
        Assert-ExactFile $next $Length $Sha256 "$Label staged fixed input"
    }

    Move-Item -LiteralPath $next -Destination $Destination
    Assert-ExactFile $Destination $Length $Sha256 "$Label fixed input"
}

function Write-Result([string]$State, [string]$Message) {
    $record = [ordered]@{
        schemaVersion = 1
        task = 'T-0371'
        state = $State
        message = $Message
        candidateSha256 = $ExpectedCandidateSha256
        candidateLength = $ExpectedCandidateLength
        envelopeSha256 = $ExpectedEnvelopeSha256
        archivedKnownStaleInputs = @($Archived)
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
    if ($ElevatedChild) {
        throw 'T-0371 elevated child did not receive an Administrator token.'
    }
    if (Test-Path -LiteralPath $ResultFile) {
        Remove-Item -LiteralPath $ResultFile -Force
    }
    Write-Host 'T-0371 requesting one UAC elevation, reconciling only exact known stale T-0217 transport, and WAITING for completion.'
    $child = Start-Process -FilePath 'powershell.exe' -Verb RunAs -Wait -PassThru -ArgumentList @(
        '-NoProfile',
        '-ExecutionPolicy','Bypass',
        '-File',('"' + $PSCommandPath + '"'),
        '-ElevatedChild'
    )
    if (-not (Test-Path -LiteralPath $ResultFile -PathType Leaf)) {
        throw "T-0371 elevated process exited with code $($child.ExitCode) but produced no durable result receipt."
    }
    $result = Get-Content -LiteralPath $ResultFile -Raw | ConvertFrom-Json
    if ($child.ExitCode -ne 0 -or $result.state -ne 'SUCCESS') {
        throw "T-0371 elevated rotation failed. ExitCode=$($child.ExitCode). Result=$($result.message)"
    }
    Write-Host 'T0371_ROTATION_VERIFIED_SUCCESS'
    Write-Host $result.message
    if ($result.archivedKnownStaleInputs.Count -gt 0) {
        Write-Host 'Archived exact historical transport:'
        $result.archivedKnownStaleInputs | ForEach-Object { Write-Host $_ }
    }
    exit 0
}

try {
    # The current installed image must either already be the designated candidate or
    # exactly match the signed epoch-1 predecessor from T-0215 before any transport reconciliation.
    $installed = Get-Evidence $InstalledCandidate
    if ($null -eq $installed) {
        throw 'Installed reviewed main image is missing; refusing transport mutation.'
    }

    if ($installed.Length -eq $ExpectedCandidateLength -and $installed.Sha256 -eq $ExpectedCandidateSha256) {
        Assert-ExactFile $InstalledEnvelope $ExpectedEnvelopeLength $ExpectedEnvelopeSha256 'Installed rotation envelope'
        $message = "Already installed and verified; Installed SHA256=$ExpectedCandidateSha256; Installed length=$ExpectedCandidateLength; Installed envelope SHA256=$ExpectedEnvelopeSha256"
        Write-Result 'SUCCESS' $message
        Write-Host $ExpectedSuccess
        exit 0
    }

    if ($installed.Length -ne $Epoch1InstalledLength -or $installed.Sha256 -ne $Epoch1InstalledSha256) {
        throw "Installed predecessor is not the accepted T-0215 epoch-1 image. Observed length=$($installed.Length) sha256=$($installed.Sha256). Refusing transport mutation."
    }

    New-Item -ItemType Directory -Force -Path $IncomingRoot | Out-Null

    # The T-0215 threat model defines this incoming directory as transport only, not accepted authority.
    # Reconcile only exact, independently known historical T-0217 bytes. Unknown bytes fail closed.
    Install-ExactIncoming $Candidate $IncomingCandidate $ExpectedCandidateLength $ExpectedCandidateSha256 'Candidate' $HistoricalT0217CandidateLength $HistoricalT0217CandidateSha256

    $existingEnvelope = Get-Evidence $IncomingEnvelope
    if ($null -ne $existingEnvelope -and -not ($existingEnvelope.Length -eq $ExpectedEnvelopeLength -and $existingEnvelope.Sha256 -eq $ExpectedEnvelopeSha256)) {
        Archive-KnownStaleFile $IncomingEnvelope $HistoricalT0217UnsignedPayloadLength $HistoricalT0217UnsignedPayloadSha256 'Envelope existing fixed input' | Out-Null
    }
    Install-ExactIncoming $Envelope $IncomingEnvelope $ExpectedEnvelopeLength $ExpectedEnvelopeSha256 'Envelope' $HistoricalT0217UnsignedPayloadLength $HistoricalT0217UnsignedPayloadSha256

    Assert-ExactFile $IncomingCandidate $ExpectedCandidateLength $ExpectedCandidateSha256 'Candidate fixed input'
    Assert-ExactFile $IncomingEnvelope $ExpectedEnvelopeLength $ExpectedEnvelopeSha256 'Envelope fixed input'

    $output = & $Candidate $RotationFlag 2>&1 | Out-String
    $exitCode = $LASTEXITCODE
    $output = $output.Trim()
    if ($exitCode -ne 0) {
        throw "Reviewed main-image rotation returned exit code $exitCode. Output: $output"
    }
    if ($output -notmatch [regex]::Escape($ExpectedSuccess)) {
        throw "Reviewed main-image rotation did not return the accepted success receipt. Output: $output"
    }

    Assert-ExactFile $InstalledCandidate $ExpectedCandidateLength $ExpectedCandidateSha256 'Installed reviewed main image'
    Assert-ExactFile $InstalledEnvelope $ExpectedEnvelopeLength $ExpectedEnvelopeSha256 'Installed rotation envelope'

    $message = "$ExpectedSuccess; Installed SHA256=$ExpectedCandidateSha256; Installed length=$ExpectedCandidateLength; Installed envelope SHA256=$ExpectedEnvelopeSha256"
    Write-Result 'SUCCESS' $message
    Write-Host $ExpectedSuccess
    exit 0
} catch {
    $message = $_.Exception.Message
    try { Write-Result 'FAILED' $message } catch {}
    Write-Error $message
    exit 1
}
