[CmdletBinding()]
param([string]$Workspace = (Join-Path $PSScriptRoot ".."))

$ErrorActionPreference = "Stop"
$root=(Resolve-Path -LiteralPath $Workspace -ErrorAction Stop).Path
$candidateTarget=Join-Path $root "target\catdesk-release-candidate"
Push-Location $root
try {
    # Candidate compilation is intentionally isolated from target\release.
    # target\release\catdesk.exe is the canonical recoverable image and may
    # only change through the reviewed promotion transaction, which also
    # advances its fingerprint, reviewed-promotion authority, and LKG escrow.
    cargo build --release --bin catdesk --target-dir $candidateTarget
    if($LASTEXITCODE -ne 0){throw "release candidate build failed"}
    $binary=Join-Path $candidateTarget "release\catdesk.exe"
    if(-not(Test-Path -LiteralPath $binary -PathType Leaf)){throw "release candidate is unavailable"}
    $fingerprint=(Get-FileHash -Algorithm SHA256 -LiteralPath $binary).Hash.ToLowerInvariant()
    [pscustomobject]@{
        State="RELEASE_CANDIDATE_READY"
        BuildPath=$binary
        Sha256=$fingerprint
        CanonicalMutated=$false
        NextAction="REVIEWED_PROMOTION_REQUIRED"
    } | ConvertTo-Json -Compress
} finally { Pop-Location }
