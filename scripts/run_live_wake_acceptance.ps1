[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
$workspace = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$wakeRoot = Join-Path $workspace ".catdesk\wake-bridge"
$python = Join-Path $wakeRoot "venv\Scripts\python.exe"
$config = Join-Path $wakeRoot "config.json"
$mcpReadyReceipt = Join-Path $wakeRoot "mcp-ready.json"
$state = Join-Path $wakeRoot "state.json"
$inbox = Join-Path $workspace ".catdesk\autonomy\review-inbox.json"
$bridge = Join-Path $PSScriptRoot "wake_bridge.py"
$backup = Join-Path $wakeRoot "review-inbox-pre-r7.json"
$oldSyntheticId = "live-acceptance-1786304692"

function Require-File([string]$Path, [string]$Label) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "$Label is missing."
    }
}

function Require-Success([string]$Label) {
    if ($LASTEXITCODE -ne 0) {
        throw "$Label failed with exit code $LASTEXITCODE."
    }
}

Require-File $python "Project-local wake Python"
Require-File $config "Wake configuration"
Require-File $mcpReadyReceipt "Fresh one-time MCP readiness receipt"
Require-File $state "Wake durable state"
Require-File $inbox "CatDesk review inbox"
Require-File $bridge "Wake bridge"

Write-Host "R7_TESTS_START"
& $python -m unittest tests/test_wake_bridge.py -v
Require-Success "Wake-bridge deterministic Python tests"
Write-Host "R7_TESTS_PASSED"

# Validate the exact conversation/profile/receipt gate before creating any
# synthetic durable inbox record. This never launches a browser or consumes the
# receipt; the bridge consumes it only at the delivery boundary below.
Write-Host "R7_DELIVERY_PREFLIGHT_START"
& $python $bridge --workspace $workspace --config $config --preflight
Require-Success "Dedicated wake delivery preflight"
Write-Host "R7_DELIVERY_PREFLIGHT_PASSED"

# The first live-acceptance probe used a wake-bridge-minimal record that is not
# a valid AutonomousReviewInboxRecordV1. Preserve a backup, remove only that
# exact test-only record, and leave every real CatDesk record unchanged.
$raw = Get-Content -LiteralPath $inbox -Raw
$parsed = $raw | ConvertFrom-Json
$records = @()
foreach ($record in $parsed) {
    $records += $record
}
$old = @($records | Where-Object { $_.recordId -eq $oldSyntheticId })
if ($old.Count -gt 1) {
    throw "Historical synthetic wake record is duplicated; refusing repair."
}
if ($old.Count -eq 1) {
    $expectedCreatedAt = 1786304692.485701
    if (
        $old[0].recordId -ne $oldSyntheticId -or
        $old[0].unread -ne $true -or
        [math]::Abs(([double]$old[0].createdAtUnix) - $expectedCreatedAt) -gt 0.001 -or
        $null -ne $old[0].schemaVersion
    ) {
        throw "Historical synthetic wake record differs from the known test-only shape; refusing repair."
    }
}

# Validate every retained record has the fields required by CatDesk's v1 review
# inbox schema before writing anything.
$retained = @($records | Where-Object { $_.recordId -ne $oldSyntheticId })
foreach ($record in $retained) {
    foreach ($field in @("schemaVersion", "recordId", "projectId", "sessionId", "state", "nextAction", "reference", "createdAtUnix", "unread")) {
        if ($null -eq $record.PSObject.Properties[$field]) {
            throw "Existing review inbox contains another non-v1 record; refusing repair."
        }
    }
    if ($record.schemaVersion -ne 1) {
        throw "Existing review inbox schema version is unsupported; refusing repair."
    }
}

if (-not (Test-Path -LiteralPath $backup)) {
    [IO.File]::WriteAllText($backup, $raw, (New-Object Text.UTF8Encoding($false)))
}

$createdAt = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
$recordId = "live-acceptance-r7-$createdAt"
if (@($retained | Where-Object { $_.recordId -eq $recordId }).Count -ne 0) {
    throw "Fresh R7 record ID collision; refusing duplicate."
}

$fresh = [pscustomobject][ordered]@{
    schemaVersion = 1
    recordId = $recordId
    projectId = "catdesk"
    sessionId = "adc-t0039-r7-live-wake-20260810"
    state = "WAITING_FOR_CHATGPT"
    nextAction = "chatgpt_decision_required"
    reference = "artifacts/live-wake-acceptance-r7.json"
    createdAtUnix = $createdAt
    unread = $true
}
$updated = @($retained) + @($fresh)
$json = ConvertTo-Json -InputObject $updated -Depth 8 -Compress
$tempInbox = "$inbox.r7.tmp"
[IO.File]::WriteAllText($tempInbox, $json, (New-Object Text.UTF8Encoding($false)))
# Re-parse before replacing the durable inbox.
$null = Get-Content -LiteralPath $tempInbox -Raw | ConvertFrom-Json
Move-Item -LiteralPath $tempInbox -Destination $inbox -Force
Write-Host "R7_EVENT_READY:$recordId"

Write-Host "R7_LIVE_WAKE_START"
$first = & $python $bridge --workspace $workspace --config $config --clear-operator-attention --once 2>&1
$firstCode = $LASTEXITCODE
$first | ForEach-Object { Write-Host $_ }
if ($firstCode -ne 0) {
    Write-Host "R7_LIVE_WAKE_STOPPED"
    exit $firstCode
}

$durable = Get-Content -LiteralPath $state -Raw | ConvertFrom-Json
if ($null -ne $durable.operator_attention) {
    throw "Wake bridge returned success but durable operator attention is still set."
}
$accounting = @($durable.wake_accounting | Where-Object { $_.record_id -eq $recordId })
if ($accounting.Count -ne 1) {
    throw "Fresh wake accounting record is missing or duplicated."
}
if ($accounting[0].status -ne "SENT" -or $null -eq $accounting[0].browser_sent_at_unix) {
    throw "Fresh wake did not reach durable SENT state."
}
Write-Host "R7_LIVE_WAKE_SENT:$recordId"

# One additional poll proves the pre-claimed event cannot be written twice.
$second = & $python $bridge --workspace $workspace --config $config --once 2>&1
$secondCode = $LASTEXITCODE
$second | ForEach-Object { Write-Host $_ }
if ($secondCode -ne 0) {
    throw "Post-send idempotency poll failed."
}
$durable2 = Get-Content -LiteralPath $state -Raw | ConvertFrom-Json
$accounting2 = @($durable2.wake_accounting | Where-Object { $_.record_id -eq $recordId })
if ($accounting2.Count -ne 1 -or $accounting2[0].status -ne "SENT") {
    throw "Post-send idempotency evidence is invalid."
}
Write-Host "R7_IDEMPOTENCY_PASSED"
Write-Host "R7_LIVE_ACCEPTANCE_PASSED:$recordId"
