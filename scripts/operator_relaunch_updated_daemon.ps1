[CmdletBinding()]
param(
    [string]$BuildPath = (Join-Path $PSScriptRoot "..\target\debug\catdesk.exe"),
    [string]$Workspace = (Join-Path $PSScriptRoot ".."),
    [switch]$Launch
)

$ErrorActionPreference = "Stop"

# This helper intentionally never discovers, prints, or copies authentication
# values.  It also never stops an existing CatDesk process: the operator must
# close the old daemon from its own desktop/session only after this preflight
# has succeeded.
if (-not (Test-Path -LiteralPath $BuildPath -PathType Leaf)) {
    throw "Updated CatDesk executable was not found: $BuildPath. Run cargo build first."
}
if (-not (Test-Path -LiteralPath $Workspace -PathType Container)) {
    throw "Workspace does not exist: $Workspace"
}
if (-not $env:CATDESK_CODEX_CLI_EXECUTABLE) {
    throw "CATDESK_CODEX_CLI_EXECUTABLE is not present in this operator session. Set it locally before relaunch; do not pass it through MCP."
}

$resolvedBuild = (Resolve-Path -LiteralPath $BuildPath).Path
$resolvedWorkspace = (Resolve-Path -LiteralPath $Workspace).Path
Write-Output "PREFLIGHT_OK updated_build=$resolvedBuild"
Write-Output "PREFLIGHT_OK workspace=$resolvedWorkspace"
Write-Output "PREFLIGHT_OK CATDESK_CODEX_CLI_EXECUTABLE=present (value redacted)"
if ($env:CATDESK_CODEX_HOME) {
    if (-not (Test-Path -LiteralPath $env:CATDESK_CODEX_HOME -PathType Container)) {
        throw "CATDESK_CODEX_HOME does not identify an existing operator-local config directory."
    }
    Write-Output "PREFLIGHT_OK CATDESK_CODEX_HOME=present (path and contents redacted)"
} else {
    Write-Output "PREFLIGHT_INFO CATDESK_CODEX_HOME=absent (child Codex processes use their inherited/default context)"
}
Write-Output "No existing CatDesk process was inspected, stopped, or replaced."

if (-not $Launch) {
    Write-Output "Run again with -Launch only after you have closed the old daemon outside its MCP control connection and confirmed the Secure MCP tunnel/reconnect procedure."
    exit 0
}

# Launching is explicit and inherits only the operator's local environment.
# It does not alter tunnel configuration, workspace configuration, billing, or
# authentication. The operator must start/reconnect MCP in the new instance
# and verify transport health before requesting any live acceptance.
Start-Process -FilePath $resolvedBuild -WorkingDirectory $resolvedWorkspace
Write-Output "LAUNCHED. Re-establish the Secure MCP connection and verify the updated autonomy tool list before resuming T-0033."
