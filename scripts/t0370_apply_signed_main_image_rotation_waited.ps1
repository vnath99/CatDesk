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

$ProjectRoot = Split-Path -Parent $PSScriptRoot
$Candidate = Join-Path $ProjectRoot '.catdesk\candidates\t0366-build\release\catdesk.exe'
$Envelope = Join-Path $ProjectRoot 'docs\orchestrator\review_bundles\T-0366_T0324_ROTATION_SIGNED_ENVELOPE.v1'
$IncomingRoot = 'C:\ProgramData\CatDesk\reviewed-main-image\rotation\incoming'
$IncomingCandidate = Join-Path $IncomingRoot 'CatDesk.exe'
$IncomingEnvelope = Join-Path $IncomingRoot 'review-envelope.v1'
$InstalledCandidate = 'C:\Program Files\CatDesk\CatDesk.exe'
$InstalledEnvelope = 'C:\Program Files\CatDesk.reviewed-main-image-rotation-installed.v1'
$ResultFile = Join-Path $ProjectRoot '.catdesk\t0370-rotation-result.json'

function Get-Sha256Lower([string]$Path) {
    (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Assert-ExactFile([string]$Path, [long]$Length, [string]$Sha256, [string]$Label) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "$Label is missing: $Path"
    }
    $item = Get-Item -LiteralPath $Path
    if ($item.Length -ne $Length) {
        throw "$Label length mismatch: expected $Length, found $($item.Length)"
    }
    $actual = Get-Sha256Lower $Path
    if ($actual -ne $Sha256) {
        throw "$Label SHA-256 mismatch: expected $Sha256, found $actual"
    }
}

function Install-ExactIncoming([string]$Source, [string]$Destination, [long]$Length, [string]$Sha256, [string]$Label) {
    if (Test-Path -LiteralPath $Destination) {
        Assert-ExactFile $Destination $Length $Sha256 "$Label existing fixed input"
        return
    }
    $next = "$Destination.t0370.next"
    if (Test-Path -LiteralPath $next) {
        Assert-ExactFile $next $Length $Sha256 "$Label staged fixed input"
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
        task = 'T-0370'
        state = $State
        message = $Message
        candidateSha256 = $ExpectedCandidateSha256
        candidateLength = $ExpectedCandidateLength
        envelopeSha256 = $ExpectedEnvelopeSha256
        timestampUtc = [DateTime]::UtcNow.ToString('o')
    }
    $record | ConvertTo-Json -Compress | Set-Content -LiteralPath $ResultFile -Encoding UTF8
}

# Always prove that the immutable project-side authority bytes are still exact
# before requesting or using elevation.
Assert-ExactFile $Candidate $ExpectedCandidateLength $ExpectedCandidateSha256 'T-0366 designated candidate'
Assert-ExactFile $Envelope $ExpectedEnvelopeLength $ExpectedEnvelopeSha256 'T-0366 signed envelope'

$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [Security.Principal.WindowsPrincipal]::new($identity)
$isAdmin = $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)

if (-not $isAdmin) {
    if ($ElevatedChild) {
        throw 'T-0370 elevated child did not receive an Administrator token.'
    }

    if (Test-Path -LiteralPath $ResultFile) {
        Remove-Item -LiteralPath $ResultFile -Force
    }

    Write-Host 'T-0370 requesting one UAC elevation and WAITING for the exact fixed-purpose rotation to finish.'
    $child = Start-Process -FilePath 'powershell.exe' -Verb RunAs -Wait -PassThru -ArgumentList @(
        '-NoProfile',
        '-ExecutionPolicy','Bypass',
        '-File',('"' + $PSCommandPath + '"'),
        '-ElevatedChild'
    )

    if (-not (Test-Path -LiteralPath $ResultFile -PathType Leaf)) {
        throw "T-0370 elevated process exited with code $($child.ExitCode) but produced no durable result receipt."
    }

    $result = Get-Content -LiteralPath $ResultFile -Raw | ConvertFrom-Json
    if ($child.ExitCode -ne 0 -or $result.state -ne 'SUCCESS') {
        throw "T-0370 elevated rotation failed. ExitCode=$($child.ExitCode). Result=$($result.message)"
    }

    Write-Host 'T0370_ROTATION_VERIFIED_SUCCESS'
    Write-Host $result.message
    exit 0
}

try {
    New-Item -ItemType Directory -Force -Path $IncomingRoot | Out-Null
    Install-ExactIncoming $Candidate $IncomingCandidate $ExpectedCandidateLength $ExpectedCandidateSha256 'Candidate'
    Install-ExactIncoming $Envelope $IncomingEnvelope $ExpectedEnvelopeLength $ExpectedEnvelopeSha256 'Envelope'

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
