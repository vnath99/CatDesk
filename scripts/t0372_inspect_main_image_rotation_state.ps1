[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$ProjectRoot = Split-Path -Parent $PSScriptRoot
$ResultFile = Join-Path $ProjectRoot '.catdesk\t0372-rotation-state.json'

$Paths = [ordered]@{
    installedCandidate = 'C:\Program Files\CatDesk\CatDesk.exe'
    acceptedBootstrapEnvelope = 'C:\Program Files\CatDesk.reviewed-main-image-accepted.v1'
    pendingRotationEnvelope = 'C:\Program Files\CatDesk.reviewed-main-image-rotation-pending.v1'
    installedRotationEnvelope = 'C:\Program Files\CatDesk.reviewed-main-image-rotation-installed.v1'
    rotationStagingCandidate = 'C:\Program Files\CatDesk\CatDesk.rotation-next.exe'
    incomingCandidate = 'C:\ProgramData\CatDesk\reviewed-main-image\rotation\incoming\CatDesk.exe'
    incomingEnvelope = 'C:\ProgramData\CatDesk\reviewed-main-image\rotation\incoming\review-envelope.v1'
}

$Known = [ordered]@{
    t0215Epoch1Candidate = [ordered]@{
        length = 25134592
        sha256 = '2421a90aeb9ad7775ec9927f8dfbe294faa3cbfe11ec7a071a1ed554eee28459'
    }
    t0217Epoch2Candidate = [ordered]@{
        length = 25172480
        sha256 = '552cb917998d283e7d901593ffed5a9ceaa108223654aa69349fa7e567c21482'
    }
    t0217Epoch2SignedEnvelope = [ordered]@{
        length = 519
        sha256 = '50194cf9613e8b9b541db597045fb2fc73a70511cbe90efcf1b512b19ce02644'
    }
    t0366Epoch2Candidate = [ordered]@{
        length = 26302464
        sha256 = 'd09c677f8ce5f14b3600a5435929aff31b4128d9a041cdaceae3213b5f0af36f'
    }
    t0366Epoch2SignedEnvelope = [ordered]@{
        length = 507
        sha256 = '42ababb6dcd47ec20db97775d7d441eb9fcadc806d3f304b7f221809bfda892c'
    }
}

function Get-Sha256Lower([string]$Path) {
    (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Get-EnvelopeSummary([string]$Path) {
    try {
        $lines = Get-Content -LiteralPath $Path -Encoding UTF8
        $summary = [ordered]@{}
        foreach ($key in @('product','purpose','root_id','root_version','epoch','policy_sha256','payload_sha256','payload_length','review_id','build_id')) {
            $prefix = "$key="
            $line = $lines | Where-Object { $_.StartsWith($prefix, [System.StringComparison]::Ordinal) } | Select-Object -First 1
            if ($null -ne $line) {
                $summary[$key] = $line.Substring($prefix.Length)
            }
        }
        return $summary
    } catch {
        return [ordered]@{ parseError = $_.Exception.Message }
    }
}

function Classify-Known([long]$Length, [string]$Sha256) {
    foreach ($entry in $Known.GetEnumerator()) {
        if ([long]$entry.Value.length -eq $Length -and [string]$entry.Value.sha256 -eq $Sha256) {
            return $entry.Key
        }
    }
    return 'UNKNOWN'
}

function Inspect-Path([string]$Name, [string]$Path) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        return [ordered]@{
            name = $Name
            path = $Path
            exists = $false
        }
    }

    $item = Get-Item -LiteralPath $Path
    $sha = Get-Sha256Lower $Path
    $record = [ordered]@{
        name = $Name
        path = $Path
        exists = $true
        length = [long]$item.Length
        sha256 = $sha
        classification = Classify-Known ([long]$item.Length) $sha
    }

    if ($Name -match 'Envelope') {
        $record.envelope = Get-EnvelopeSummary $Path
    }
    return $record
}

$records = @()
foreach ($entry in $Paths.GetEnumerator()) {
    $records += Inspect-Path $entry.Key $entry.Value
}

$result = [ordered]@{
    schemaVersion = 1
    task = 'T-0372'
    state = 'READ_ONLY_COMPLETE'
    timestampUtc = [DateTime]::UtcNow.ToString('o')
    records = $records
}

$result | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $ResultFile -Encoding UTF8
Write-Host 'T0372_ROTATION_STATE_READBACK_COMPLETE'
$result | ConvertTo-Json -Depth 8
