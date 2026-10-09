$ErrorActionPreference = 'Stop'
# Test-only SHA256 helper independent of optional PowerShell module autoload.
function Get-FixtureHash {
    param([string]$LiteralPath, [ValidateSet('SHA256')][string]$Algorithm = 'SHA256')
    $sha = [Security.Cryptography.SHA256]::Create()
    $stream = [IO.File]::OpenRead($LiteralPath)
    try { [pscustomobject]@{ Hash = ([BitConverter]::ToString($sha.ComputeHash($stream)).Replace('-', '')) } }
    finally { $sha.Dispose(); $stream.Dispose() }
}
Set-Alias -Name Get-FileHash -Value Get-FixtureHash -Scope Script
$workspace = Join-Path ([IO.Path]::GetTempPath()) ('catdesk-provision-release-' + [Guid]::NewGuid().ToString('N'))
try {
    $release = Join-Path $workspace 'target\release'
    $candidateRelease = Join-Path $workspace 'target\catdesk-release-candidate\release'
    New-Item -ItemType Directory -Path $release -Force | Out-Null
    $canonical = Join-Path $release 'catdesk.exe'
    [IO.File]::WriteAllText($canonical, 'canonical-reviewed-image', [Text.UTF8Encoding]::new($false))
    $canonicalHash = (Get-FileHash -LiteralPath $canonical -Algorithm SHA256).Hash.ToLowerInvariant()
    [IO.File]::WriteAllText("$canonical.sha256", ($canonicalHash + [Environment]::NewLine), [Text.UTF8Encoding]::new($false))
    $canonicalBefore = [IO.File]::ReadAllBytes($canonical)
    $manifestBefore = [IO.File]::ReadAllBytes("$canonical.sha256")

    function global:cargo {
        param([Parameter(ValueFromRemainingArguments=$true)][string[]]$Arguments)
        if (($Arguments -join ' ') -notmatch 'build --release --bin catdesk --target-dir') {
            throw 'unexpected cargo invocation'
        }
        New-Item -ItemType Directory -Path $candidateRelease -Force | Out-Null
        [IO.File]::WriteAllText((Join-Path $candidateRelease 'catdesk.exe'), 'isolated-candidate-image', [Text.UTF8Encoding]::new($false))
        $global:LASTEXITCODE = 0
    }

    $resultText = & (Join-Path $PSScriptRoot 'provision-catdesk-release.ps1') -Workspace $workspace
    $result = $resultText | ConvertFrom-Json
    if ($result.State -ne 'RELEASE_CANDIDATE_READY' -or $result.CanonicalMutated -ne $false -or $result.NextAction -ne 'REVIEWED_PROMOTION_REQUIRED') {
        throw 'provision result did not remain candidate-only'
    }
    if (-not (Test-Path -LiteralPath $result.BuildPath -PathType Leaf)) {
        throw 'isolated candidate was not produced'
    }
    if ([IO.Path]::GetFullPath([string]$result.BuildPath).StartsWith([IO.Path]::GetFullPath($release), [StringComparison]::OrdinalIgnoreCase)) {
        throw 'candidate was built inside canonical release directory'
    }
    if (-not [Linq.Enumerable]::SequenceEqual([byte[]]$canonicalBefore, [byte[]][IO.File]::ReadAllBytes($canonical))) {
        throw 'candidate build mutated canonical binary'
    }
    if (-not [Linq.Enumerable]::SequenceEqual([byte[]]$manifestBefore, [byte[]][IO.File]::ReadAllBytes("$canonical.sha256"))) {
        throw 'candidate build mutated canonical fingerprint'
    }
    Write-Output 'PROVISION_RELEASE_ISOLATION_PASS'
} finally {
    Remove-Item Function:\global:cargo -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $workspace -Recurse -Force -ErrorAction SilentlyContinue
}
