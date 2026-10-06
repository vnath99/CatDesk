[CmdletBinding()]
param()

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
    $next = "$Destination.t0368.next"
    if (Test-Path -LiteralPath $next) {
        Assert-ExactFile $next $Length $Sha256 "$Label staged fixed input"
    } else {
        Copy-Item -LiteralPath $Source -Destination $next
        Assert-ExactFile $next $Length $Sha256 "$Label staged fixed input"
    }
    Move-Item -LiteralPath $next -Destination $Destination
    Assert-ExactFile $Destination $Length $Sha256 "$Label fixed input"
}

$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [Security.Principal.WindowsPrincipal]::new($identity)
$isAdmin = $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Host 'T-0368 requires the reviewed administrator boundary. Requesting one UAC elevation for this exact fixed-purpose script.'
    Start-Process -FilePath 'powershell.exe' -Verb RunAs -ArgumentList @('-NoProfile','-ExecutionPolicy','Bypass','-File',('"' + $PSCommandPath + '"'))
    exit 0
}

Assert-ExactFile $Candidate $ExpectedCandidateLength $ExpectedCandidateSha256 'T-0366 designated candidate'
Assert-ExactFile $Envelope $ExpectedEnvelopeLength $ExpectedEnvelopeSha256 'T-0366 signed envelope'

New-Item -ItemType Directory -Force -Path $IncomingRoot | Out-Null
Install-ExactIncoming $Candidate $IncomingCandidate $ExpectedCandidateLength $ExpectedCandidateSha256 'Candidate'
Install-ExactIncoming $Envelope $IncomingEnvelope $ExpectedEnvelopeLength $ExpectedEnvelopeSha256 'Envelope'

# Revalidate both fixed transport objects immediately before crossing the zero-input
# signed rotation boundary. The candidate executable itself is the designated,
# measured T-0366 image containing the reviewed fixed-policy consumer.
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

Write-Host $ExpectedSuccess
Write-Host "Installed SHA256=$ExpectedCandidateSha256"
Write-Host "Installed length=$ExpectedCandidateLength"
Write-Host "Installed envelope SHA256=$ExpectedEnvelopeSha256"
