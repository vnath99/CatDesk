[CmdletBinding()]
param([switch]$NativeFixtures)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$facade = Join-Path $PSScriptRoot 'catdesk-dev-local-target.ps1'
$adapter = Join-Path $PSScriptRoot 'catdesk-dev-local-target-native.ps1'
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile($facade, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'FACADE_PARSE_FAILED' }
foreach ($name in @('Get-DevLocalHash','Get-DevLocalPlan')) {
    $definition = @($ast.FindAll({param($n) $n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -ceq $name}, $true))
    if ($definition.Count -ne 1) { throw 'DEFINITION_NOT_UNIQUE' }
    . ([scriptblock]::Create($definition[0].Extent.Text))
}
$mockCases = [Collections.Generic.List[string]]::new()
$nativeCases = [Collections.Generic.List[string]]::new()
function Check([bool]$Ok, [string]$Name, [bool]$Native = $false) {
    if (!$Ok) { throw $Name }
    if ($Native) { $nativeCases.Add($Name) } else { $mockCases.Add($Name) }
}
function Facts {
    [pscustomobject]@{
        sourceCommit = 'a' * 40; reason = $null; owner = 'S-1-5-21-111-222-333-1001'
        workspaceIdentity = '1' * 48; cargoSha256 = '2' * 64
        rustcSha256 = '3' * 64; lockSha256 = '4' * 64; managedBasePresent = $false
    }
}
$original = Get-DevLocalPlan (Facts)
Check ($original.state -ceq 'PROPOSED_UNINITIALIZED' -and !$original.executionQualified -and !$original.targetCreated -and !$original.cargoStarted -and !$original.existingTargetsChanged) 'proposal_is_nonexecuting_noncreating'
$f = Facts; $f.sourceCommit = 'b' * 40; $next = Get-DevLocalPlan $f
Check ($next.relativeTarget -ceq $original.relativeTarget -and $next.sourceCommit -cne $original.sourceCommit) 'commit_change_reuses_same_proposed_namespace'
foreach ($field in @('workspaceIdentity','owner','cargoSha256','rustcSha256','lockSha256')) {
    $f = Facts
    $f.$field = if ($field -ceq 'owner') { 'S-1-5-21-111-222-333-1002' } elseif ($field -ceq 'workspaceIdentity') { '5' * 48 } else { '5' * 64 }
    $next = Get-DevLocalPlan $f
    Check ($next.relativeTarget -cne $original.relativeTarget -and !$next.executionQualified) ('changed_' + $field + '_does_not_adopt_old_target')
}
$f = Facts; $f.managedBasePresent = $true; $next = Get-DevLocalPlan $f
Check ($next.state -ceq 'HOLD' -and $next.reason -ceq 'EXISTING_MANAGED_NAMESPACE_NOT_ADOPTED') 'unknown_namespace_never_adopted'
foreach ($reason in @('OWNER_MISMATCH','OWNER_OR_VOLUME_MISMATCH','SOURCE_TARGET_OVERLAP','CARGO_CONFIG_UNQUALIFIED','KNOWN_FOLDER_REDIRECTION_UNQUALIFIED','SYNCED_BASE_REFUSED','SOURCE_GIT_ROOT_MISMATCH','NATIVE_METADATA_UNVERIFIED')) {
    $f = Facts; $f.reason = $reason; $next = Get-DevLocalPlan $f
    Check ($next.state -ceq 'ATTENTION' -and $next.reason -ceq $reason -and $null -eq $next.relativeTarget) ('refusal_' + $reason)
}
$f = Facts; $f.reason = 'PRIVATE_PROVIDER_ERROR_WITH_DRAFT'; $next = Get-DevLocalPlan $f
Check (($next | ConvertTo-Json -Compress) -notmatch 'PRIVATE_PROVIDER' -and $next.reason -ceq 'METADATA_UNVERIFIED') 'unknown_exception_is_not_exported'
foreach ($field in @('workspaceIdentity','owner','cargoSha256','rustcSha256','lockSha256','sourceCommit')) {
    $f = Facts; $f.$field = 'invalid'; $next = Get-DevLocalPlan $f
    Check ($next.state -ceq 'ATTENTION' -and $null -eq $next.relativeTarget) ('malformed_' + $field + '_refused')
}
Check (($original | ConvertTo-Json -Compress) -notmatch 'S-1-5|C:|draft|credential') 'plan_does_not_export_paths_owner_or_content'
foreach ($command in @('build','verify')) {
    $captured = [Collections.Generic.List[string]]::new(); $refused = $false
    try { & $facade $command | ForEach-Object { $captured.Add([string]$_) } }
    catch { $refused = $_.Exception.Message -ceq 'DEVELOPER_TARGET_EXECUTION_OWNER_UNQUALIFIED' }
    Check ($refused -and $captured.Count -eq 1) ($command + '_explicitly_refused')
    $receipt = $captured[0] | ConvertFrom-Json
    Check (!$receipt.cargoStarted -and !$receipt.targetCreated -and !$receipt.existingTargetsChanged -and $receipt.state -ceq 'REFUSED') ($command + '_receipt_claims_no_work')
}
foreach ($variable in @('CARGO_TARGET_DIR','CARGO_BUILD_BUILD_DIR','RUSTC_WRAPPER','CARGO_ENCODED_RUSTFLAGS')) {
    $prior = Get-Item ('Env:' + $variable) -ErrorAction SilentlyContinue
    $hadPrior = $null -ne $prior; $priorValue = if ($hadPrior) { [string]$prior.Value } else { $null }
    try {
        Set-Item ('Env:' + $variable) 'private-unqualified-redirect'
        $receipt = (& $facade plan) | ConvertFrom-Json
        Check ($receipt.reason -ceq 'INHERITED_CARGO_POLICY_UNQUALIFIED' -and !$receipt.cargoStarted -and $null -eq $receipt.relativeTarget) ('environment_' + $variable + '_refused_before_native_observation')
    } finally {
        if ($hadPrior) { Set-Item ('Env:' + $variable) $priorValue } else { Remove-Item ('Env:' + $variable) -ErrorAction SilentlyContinue }
    }
}
# Compare normalized complete legacy files, not just their output key count.
foreach ($pin in @(
    @('catdesk-dev-lane.ps1','964cffd6d314b5edc33f286568150ddeef5a313070d4dae7d543964c5ca3c026'),
    @('test-catdesk-dev-lane-storage.ps1','b181b3e96977f88d0c1c588de3a96dd98e3ad880795b4c09f42929acb000e3ff')
)) {
    $actual = Get-DevLocalHash ([IO.File]::ReadAllText((Join-Path $PSScriptRoot $pin[0])).Replace([string][char]13, ''))
    Check ($actual -ceq $pin[1]) ('legacy_full_lf_source_preserved_' + $pin[0])
}

if ($NativeFixtures) {
    . $adapter
    Initialize-DevLocalTargetMetadata
    Initialize-DevLocalTargetMetadata
    foreach ($directory in @($false, $true)) {
        $flags = [CatDesk.DevLocalTarget.ReadOnlyScope]::OpenFlags($directory)
        Check (($flags -band 0x00300000) -eq 0x00300000 -and (($flags -band 0x02000000) -ne 0) -eq $directory) ('actual_windows_no_recall_and_no_follow_policy_directory_' + $directory) $true
    }
    Check ([CatDesk.DevLocalTarget.ReadOnlyScope]::TemplateSha256 -cmatch '\A[0-9a-f]{64}\z') 'actual_windows_same_native_template_reusable' $true
    $originalInitializer = (Get-Item Function:Initialize-DevLocalTargetMetadata).ScriptBlock
    $different = $originalInitializer.ToString().Replace('// DONT_VERIFY', '// CHANGED_TEMPLATE')
    $denied = $false
    try {
        Set-Item Function:Initialize-DevLocalTargetMetadata ([scriptblock]::Create($different))
        Initialize-DevLocalTargetMetadata
    } catch { $denied = $_.Exception.Message -ceq 'METADATA_ADAPTER_VERSION_MISMATCH' }
    finally { Set-Item Function:Initialize-DevLocalTargetMetadata $originalInitializer }
    Check $denied 'actual_windows_stale_native_template_refused' $true
    $fixture = Join-Path ([IO.Path]::GetTempPath()) ('catdesk-dev-local-target-fixture-' + [guid]::NewGuid().ToString('N'))
    if (Test-Path -LiteralPath $fixture) { throw 'FIXTURE_ALREADY_EXISTS' }
    $null = [IO.Directory]::CreateDirectory($fixture)
    $data = Join-Path $fixture 'tiny.txt'
    [IO.File]::WriteAllText($data, 'abc', [Text.UTF8Encoding]::new($false))
    $scope = $null; $junction = Join-Path $fixture 'alias'; $junctionCreated = $false
    try {
        $scope = New-Object CatDesk.DevLocalTarget.ReadOnlyScope
        $a = $scope.Directory($fixture, $false); $b = $scope.Directory($fixture, $false)
        Check ($a.Identity -ceq $b.Identity -and $a.Identity -cmatch '\A[0-9a-f]{48}\z' -and $a.Volume -cmatch '\A[0-9a-f]{16}\z') 'actual_windows_stable_file_identity' $true
        Check ($a.Owner -ceq [CatDesk.DevLocalTarget.ReadOnlyScope]::CurrentOwner()) 'actual_windows_owner_identity' $true
        $file = $scope.File($data, 3, $false)
        Check ($file.Length -eq 3 -and $file.Sha256 -ceq 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad') 'actual_windows_bounded_hash' $true
        $denied = $false
        try { $stream = [IO.File]::Open($data,[IO.FileMode]::Open,[IO.FileAccess]::Write,[IO.FileShare]::ReadWrite); $stream.Dispose() } catch { $denied = $true }
        Check $denied 'actual_windows_retained_file_blocks_writer' $true
        $denied = $false
        try { [IO.Directory]::Move($fixture, $fixture + '-unexpected') } catch { $denied = $true }
        Check $denied 'actual_windows_retained_ancestry_blocks_rename' $true
        $denied = $false
        try { $null = $scope.File($data, 2, $false) } catch { $denied = $true }
        Check $denied 'actual_windows_size_limit_refused' $true
        $missing = Join-Path $fixture 'never-created\target'
        Check ($null -eq $scope.Directory($missing, $true) -and !(Test-Path -LiteralPath $missing)) 'actual_windows_missing_target_not_created' $true
        Check ($scope.FilePresent($data)) 'actual_windows_configuration_probe_metadata_only' $true
        $null = New-Item -ItemType Junction -Path $junction -Target $fixture
        $junctionCreated = $true; $denied = $false
        try { $null = $scope.Directory($junction, $false) } catch { $denied = $true }
        Check $denied 'actual_windows_junction_refused' $true
    } finally {
        if ($null -ne $scope) { $scope.Dispose() }
        # Only this newly created fixture and its own junction are eligible.
        $resolved = [IO.Path]::GetFullPath($fixture)
        $parent = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\')
        if ([IO.Path]::GetDirectoryName($resolved) -cne $parent -or [IO.Path]::GetFileName($resolved) -cnotmatch '\Acatdesk-dev-local-target-fixture-[0-9a-f]{32}\z') { throw 'FIXTURE_CLEANUP_SCOPE_REFUSED' }
        if ($junctionCreated -or (Test-Path -LiteralPath $junction)) {
            $link = Get-Item -LiteralPath $junction -Force
            if (!($link.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'FIXTURE_JUNCTION_KIND_REFUSED' }
            [IO.Directory]::Delete($junction, $false)
        }
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
    Check (!(Test-Path -LiteralPath $fixture)) 'actual_windows_own_fixture_cleanup_verified' $true
}
[pscustomobject][ordered]@{
    schemaVersion = 1; mockPassed = $mockCases.Count; nativePassed = $nativeCases.Count
    mockCases = @($mockCases); nativeCases = @($nativeCases)
    realCargoRuns = 0; externalTargetCreated = $false; executionQualified = $false
} | ConvertTo-Json -Depth 4 -Compress
