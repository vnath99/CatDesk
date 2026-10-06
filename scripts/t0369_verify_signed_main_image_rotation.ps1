[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$ExpectedCandidateSha256 = 'd09c677f8ce5f14b3600a5435929aff31b4128d9a041cdaceae3213b5f0af36f'
$ExpectedCandidateLength = 26302464
$ExpectedEnvelopeSha256 = '42ababb6dcd47ec20db97775d7d441eb9fcadc806d3f304b7f221809bfda892c'
$ExpectedEnvelopeLength = 507
$InstalledCandidate = 'C:\Program Files\CatDesk\CatDesk.exe'
$InstalledEnvelope = 'C:\Program Files\CatDesk.reviewed-main-image-rotation-installed.v1'

function Get-Sha256Lower([string]$Path) {
    (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Assert-ExactFile([string]$Path, [long]$Length, [string]$Sha256, [string]$Label) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "$Label is missing"
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

Assert-ExactFile $InstalledCandidate $ExpectedCandidateLength $ExpectedCandidateSha256 'Installed reviewed main image'
Assert-ExactFile $InstalledEnvelope $ExpectedEnvelopeLength $ExpectedEnvelopeSha256 'Installed rotation envelope'

$statusOutput = & $InstalledCandidate operator supervisor status 2>&1 | Out-String
$statusCode = $LASTEXITCODE
$statusOutput = $statusOutput.Trim()
if ($statusCode -ne 0) {
    throw "Installed reviewed image supervisor status failed with exit code $statusCode. Output: $statusOutput"
}

Write-Host 'T0369_ROTATION_READBACK_VERIFIED'
Write-Host "Installed SHA256=$ExpectedCandidateSha256"
Write-Host "Installed length=$ExpectedCandidateLength"
Write-Host "Installed envelope SHA256=$ExpectedEnvelopeSha256"
Write-Host "Supervisor status=$statusOutput"
