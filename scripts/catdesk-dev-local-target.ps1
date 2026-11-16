[CmdletBinding()]
param(
    [Parameter(Mandatory = $true, Position = 0)]
    [ValidateSet('plan', 'build', 'verify')]
    [string]$Command
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Get-DevLocalHash([string]$Text) {
    $hash = [Security.Cryptography.SHA256]::Create()
    try { return ([BitConverter]::ToString($hash.ComputeHash([Text.Encoding]::UTF8.GetBytes($Text)))).Replace('-', '').ToLowerInvariant() }
    finally { $hash.Dispose() }
}

function Get-DevLocalPlan([object]$Facts) {
    $report = [ordered]@{
        schemaVersion = 1
        mode = 'ORDINARY_DEVELOPER_LOCAL_TARGET_PLAN'
        state = 'ATTENTION'
        reason = 'METADATA_UNVERIFIED'
        sourceCommit = $Facts.sourceCommit
        baseKind = 'WINDOWS_KNOWN_FOLDER_LOCAL_APP_DATA'
        relativeTarget = $null
        workspaceKey = $null
        recipeKey = $null
        executionQualified = $false
        executionBlocker = 'WHOLE_JOB_AND_OUTPUT_DRAIN_OWNER_UNQUALIFIED'
        targetCreated = $false
        cargoStarted = $false
        existingTargetsChanged = $false
        productionInstallationChanged = $false
    }
    if ($Facts.reason) {
        $known = @('WINDOWS_X64_REQUIRED','INHERITED_CARGO_POLICY_UNQUALIFIED','KNOWN_FOLDER_REDIRECTION_UNQUALIFIED','SYNCED_BASE_REFUSED','OWNER_MISMATCH','OWNER_OR_VOLUME_MISMATCH','SOURCE_TARGET_OVERLAP','CARGO_CONFIG_UNQUALIFIED','SOURCE_COMMIT_UNAVAILABLE','SOURCE_GIT_ROOT_MISMATCH','NATIVE_METADATA_UNVERIFIED')
        if ($known -ccontains $Facts.reason) { $report.reason = $Facts.reason }
        return [pscustomobject]$report
    }
    if ($Facts.workspaceIdentity -isnot [string] -or $Facts.workspaceIdentity -cnotmatch '\A[0-9a-f]{48}\z') { return [pscustomobject]$report }
    foreach ($digest in @($Facts.cargoSha256, $Facts.rustcSha256, $Facts.lockSha256)) {
        if ($digest -isnot [string] -or $digest -cnotmatch '\A[0-9a-f]{64}\z') { return [pscustomobject]$report }
    }
    if ($Facts.sourceCommit -cnotmatch '\A[0-9a-f]{40}\z' -or
        $Facts.owner -isnot [string] -or $Facts.owner -cnotmatch '\AS-1-[0-9-]{3,180}\z') { return [pscustomobject]$report }
    $workspaceKey = Get-DevLocalHash ("ordinary-worktree-v1|" + $Facts.owner + '|' + $Facts.workspaceIdentity)
    # Content changes do not allocate a target per commit/run. An incompatible
    # recipe is a proposed separate namespace, never automatically adopted.
    $recipe = [ordered]@{
        schemaVersion = 1
        cargoSha256 = $Facts.cargoSha256
        rustcSha256 = $Facts.rustcSha256
        cargoLockSha256 = $Facts.lockSha256
        hostTarget = 'x86_64-pc-windows-msvc'
        buildProfile = 'dev'
        verification = 'workspace-all-targets-all-features-clippy;bin-catdesk-test'
        environmentPolicy = 'fixed-offline-locked-incremental0-v1'
        configPolicy = 'no-inherited-redirects-no-cargo-config-v1'
    }
    $recipeKey = Get-DevLocalHash ($recipe | ConvertTo-Json -Compress)
    $report.workspaceKey = $workspaceKey
    $report.recipeKey = $recipeKey
    $report.relativeTarget = "CatDeskDev\ordinary-targets-v1\$workspaceKey\$recipeKey\target"
    if ($Facts.managedBasePresent) {
        $report.state = 'HOLD'
        $report.reason = 'EXISTING_MANAGED_NAMESPACE_NOT_ADOPTED'
    } else {
        $report.state = 'PROPOSED_UNINITIALIZED'
        $report.reason = 'NO_TARGET_CREATED_OR_REUSE_AUTHORIZED'
    }
    return [pscustomobject]$report
}

function Get-DevLocalFacts([string]$SourceRoot) {
    $facts = [pscustomobject]@{
        sourceCommit = $null; reason = 'METADATA_UNVERIFIED'; owner = $null
        workspaceIdentity = $null; cargoSha256 = $null; rustcSha256 = $null
        lockSha256 = $null; managedBasePresent = $false
    }
    $scope = $null
    try {
        if ($env:OS -cne 'Windows_NT' -or ![Environment]::Is64BitProcess) { throw 'WINDOWS_X64_REQUIRED' }
        # No raw values, arbitrary environment, config or credentials exported.
        foreach ($item in Get-ChildItem Env:) {
            if ($item.Name -match '^(CARGO_HOME|RUSTUP_HOME|RUSTUP_TOOLCHAIN|RUSTC|RUSTC_WRAPPER|RUSTC_WORKSPACE_WRAPPER|RUSTFLAGS|CARGO_ENCODED_RUSTFLAGS|CARGO_TARGET_DIR|CARGO_BUILD_.*|CARGO_TARGET_.*|CARGO_PROFILE_.*)$' -and $item.Value) {
                throw 'INHERITED_CARGO_POLICY_UNQUALIFIED'
            }
            if ($item.Name -ceq 'CARGO_INCREMENTAL' -and $item.Value -and $item.Value -cne '0') { throw 'INHERITED_CARGO_POLICY_UNQUALIFIED' }
        }
        Initialize-DevLocalTargetMetadata
        if (![CatDesk.DevLocalTarget.ReadOnlyScope]::Amd64Host()) { throw 'WINDOWS_X64_REQUIRED' }
        $scope = New-Object CatDesk.DevLocalTarget.ReadOnlyScope
        $owner = [CatDesk.DevLocalTarget.ReadOnlyScope]::CurrentOwner()
        $profile = [CatDesk.DevLocalTarget.ReadOnlyScope]::KnownFolder($false)
        $local = [CatDesk.DevLocalTarget.ReadOnlyScope]::KnownFolder($true)
        if (![string]::Equals($local.TrimEnd('\'), (Join-Path $profile 'AppData\Local'), [StringComparison]::OrdinalIgnoreCase)) { throw 'KNOWN_FOLDER_REDIRECTION_UNQUALIFIED' }
        if ($local -match '(?i)(^|[\\/])OneDrive(?:[^\\/]*)?([\\/]|$)') { throw 'SYNCED_BASE_REFUSED' }
        $localId = $scope.Directory($local, $false)
        $sourceId = $scope.Directory($SourceRoot, $false)
        if ($localId.Owner -cne $owner -or $sourceId.Owner -cne $owner) { throw 'OWNER_MISMATCH' }
        $base = Join-Path $local 'CatDeskDev'
        $sourcePath = [IO.Path]::GetFullPath($SourceRoot).TrimEnd('\')
        if ($sourcePath.StartsWith($base + '\', [StringComparison]::OrdinalIgnoreCase) -or
            $base.StartsWith($sourcePath + '\', [StringComparison]::OrdinalIgnoreCase) -or
            [string]::Equals($sourcePath, $base, [StringComparison]::OrdinalIgnoreCase)) { throw 'SOURCE_TARGET_OVERLAP' }
        $existing = $scope.Directory($base, $true)
        if ($null -ne $existing -and ($existing.Owner -cne $owner -or $existing.Volume -cne $localId.Volume)) { throw 'OWNER_OR_VOLUME_MISMATCH' }
        # Refuse existing Cargo config rather than guessing at TOML overrides.
        $cursor = [IO.Path]::GetFullPath($SourceRoot)
        while ($cursor) {
            foreach ($name in @('config', 'config.toml')) {
                if ($scope.FilePresent((Join-Path $cursor ('.cargo\' + $name)))) { throw 'CARGO_CONFIG_UNQUALIFIED' }
            }
            $parent = [IO.Directory]::GetParent($cursor)
            $cursor = if ($null -ne $parent) { $parent.FullName } else { $null }
        }
        foreach ($name in @('config', 'config.toml')) {
            if ($scope.FilePresent((Join-Path $profile ('.cargo\' + $name)))) { throw 'CARGO_CONFIG_UNQUALIFIED' }
        }
        $toolRoot = Join-Path $profile '.rustup\toolchains\stable-x86_64-pc-windows-msvc\bin'
        $cargo = $scope.File((Join-Path $toolRoot 'cargo.exe'), 134217728, $false)
        $rustc = $scope.File((Join-Path $toolRoot 'rustc.exe'), 134217728, $false)
        $lock = $scope.File((Join-Path $SourceRoot 'Cargo.lock'), 16777216, $false)
        if ($cargo.Owner -cne $owner -or $rustc.Owner -cne $owner -or $lock.Owner -cne $owner) { throw 'OWNER_MISMATCH' }
        $gitRoot = & git --no-optional-locks -c core.fsmonitor=false -C $SourceRoot rev-parse --show-toplevel 2>$null
        if ($LASTEXITCODE -ne 0 -or !$gitRoot -or ![string]::Equals([IO.Path]::GetFullPath(([string]$gitRoot).Replace('/', '\')), $sourcePath, [StringComparison]::OrdinalIgnoreCase)) { throw 'SOURCE_GIT_ROOT_MISMATCH' }
        $head = & git --no-optional-locks -c core.fsmonitor=false -C $SourceRoot rev-parse --verify HEAD 2>$null
        if ($LASTEXITCODE -ne 0 -or [string]$head -cnotmatch '\A[0-9a-f]{40}\z') { throw 'SOURCE_COMMIT_UNAVAILABLE' }
        $facts.sourceCommit = [string]$head
        $facts.owner = $owner
        $facts.workspaceIdentity = $sourceId.Identity
        $facts.cargoSha256 = $cargo.Sha256
        $facts.rustcSha256 = $rustc.Sha256
        $facts.lockSha256 = $lock.Sha256
        $facts.managedBasePresent = $null -ne $existing
        $facts.reason = $null
    } catch {
        # Fixed categories only; no arbitrary provider exceptions/paths exported.
        $known = @('WINDOWS_X64_REQUIRED','INHERITED_CARGO_POLICY_UNQUALIFIED','KNOWN_FOLDER_REDIRECTION_UNQUALIFIED','SYNCED_BASE_REFUSED','OWNER_MISMATCH','OWNER_OR_VOLUME_MISMATCH','SOURCE_TARGET_OVERLAP','CARGO_CONFIG_UNQUALIFIED','SOURCE_COMMIT_UNAVAILABLE','SOURCE_GIT_ROOT_MISMATCH')
        $facts.reason = if ($known -ccontains $_.Exception.Message) { $_.Exception.Message } else { 'NATIVE_METADATA_UNVERIFIED' }
    } finally { if ($null -ne $scope) { $scope.Dispose() } }
    return $facts
}

if ($Command -ne 'plan') {
    [pscustomobject][ordered]@{
        schemaVersion = 1; mode = 'ORDINARY_DEVELOPER_LOCAL_TARGET_REFUSAL'
        operation = $Command.ToUpperInvariant(); state = 'REFUSED'
        reason = 'WHOLE_JOB_AND_OUTPUT_DRAIN_OWNER_UNQUALIFIED'
        cargoStarted = $false; targetCreated = $false; existingTargetsChanged = $false
    } | ConvertTo-Json -Compress
    throw 'DEVELOPER_TARGET_EXECUTION_OWNER_UNQUALIFIED'
}
$adapter = Join-Path $PSScriptRoot 'catdesk-dev-local-target-native.ps1'
$item = Get-Item -LiteralPath $adapter -Force
if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'METADATA_ADAPTER_UNSAFE' }
. $adapter
$sourceRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
Get-DevLocalPlan (Get-DevLocalFacts $sourceRoot) | ConvertTo-Json -Compress
