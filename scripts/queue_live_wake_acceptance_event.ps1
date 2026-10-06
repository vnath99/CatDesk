[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
$workspace = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$inbox = Join-Path $workspace ".catdesk\autonomy\review-inbox.json"
$wakeState = Join-Path $workspace ".catdesk\wake-bridge\state.json"
$receipt = Join-Path $workspace ".catdesk\wake-bridge\mcp-ready.json"

foreach ($required in @($inbox, $wakeState, $receipt)) {
    if (-not (Test-Path -LiteralPath $required -PathType Leaf)) {
        throw "Required acceptance file is missing: $required"
    }
}

$raw = Get-Content -LiteralPath $inbox -Raw
$parsed = $raw | ConvertFrom-Json
$records = @()
foreach ($record in $parsed) { $records += $record }
if ($records.Count -ge 512) { throw "Review inbox is at its bounded 512-record limit." }

$requiredFields = @("schemaVersion", "recordId", "projectId", "sessionId", "state", "nextAction", "reference", "createdAtUnix", "unread")
foreach ($record in $records) {
    foreach ($field in $requiredFields) {
        if ($null -eq $record.PSObject.Properties[$field]) {
            throw "Existing review inbox contains a non-v1 record; refusing to modify it."
        }
    }
    if ($record.schemaVersion -ne 1) { throw "Unsupported review inbox schema version." }
}

$state = Get-Content -LiteralPath $wakeState -Raw | ConvertFrom-Json
$sent = @($state.sent_record_ids)
$existingPending = @($records | Where-Object {
    $_.recordId -like "live-acceptance-r11-*" -and
    $_.unread -eq $true -and
    $sent -notcontains $_.recordId
})
if ($existingPending.Count -gt 1) { throw "Multiple pending R11 acceptance events exist; refusing ambiguity." }
if ($existingPending.Count -eq 1) {
    Write-Output "WAKE_EVENT_READY:$($existingPending[0].recordId)"
    exit 0
}

$createdAt = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
$recordId = "live-acceptance-r11-$createdAt"
if (@($records | Where-Object { $_.recordId -eq $recordId }).Count -ne 0) {
    throw "Acceptance record ID collision; rerun after one second."
}

$fresh = [pscustomobject][ordered]@{
    schemaVersion = 1
    recordId = $recordId
    projectId = "catdesk"
    sessionId = "wake-live-acceptance-r11-20260810"
    state = "WAITING_FOR_CHATGPT"
    nextAction = "chatgpt_decision_required"
    reference = "artifacts/live-wake-acceptance-r11.json"
    createdAtUnix = $createdAt
    unread = $true
}

$updated = @($records) + @($fresh)
$json = ConvertTo-Json -InputObject $updated -Depth 8
$temp = "$inbox.r11.tmp"
[IO.File]::WriteAllText($temp, $json + [Environment]::NewLine, (New-Object Text.UTF8Encoding($false)))
$null = Get-Content -LiteralPath $temp -Raw | ConvertFrom-Json
Move-Item -LiteralPath $temp -Destination $inbox -Force
Write-Output "WAKE_EVENT_READY:$recordId"
