[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$helper = Join-Path $PSScriptRoot 'catdesk-dev-local-target-owner.ps1'
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile($helper, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'OWNER_SOURCE_PARSE_FAILED' }
# Only function definitions may execute when this helper is dot-sourced.
foreach ($statement in $ast.EndBlock.Statements) {
    if ($statement -isnot [Management.Automation.Language.FunctionDefinitionAst]) { throw 'OWNER_LOAD_NOT_DEFINE_ONLY' }
}
. $helper
$cases = [Collections.Generic.List[string]]::new()
function Check([bool]$Ok, [string]$Name) { if (!$Ok) { throw $Name }; $cases.Add($Name) }
function Refuses([scriptblock]$Action, [string]$Reason) {
    try { $null = & $Action; return $false } catch { return $_.Exception.Message -ceq $Reason }
}
Check ($ast.EndBlock.Statements.Count -eq 4) 'helper_load_has_only_four_definitions'
$unicode = [string][char]0x65e5 + [char]0x672c + [char]0x8a9e
foreach ($row in @(
    @{ value=''; expected='""' },
    @{ value='plain'; expected='plain' },
    @{ value='with space'; expected='"with space"' },
    @{ value='a"b'; expected='"a\"b"' },
    @{ value='C:\with space\'; expected='"C:\with space\\"' },
    @{ value=$unicode; expected=$unicode }
)) {
    Check ((ConvertTo-DevLocalOwnerArgument $row.value) -ceq $row.expected) ('windows_quoting_case_' + $cases.Count)
}
Check (Refuses { ConvertTo-DevLocalOwnerArgument ('a' + [char]0 + 'b') } 'OWNER_ARGUMENT_INVALID') 'argument_nul_refused'
Check (Refuses { ConvertTo-DevLocalOwnerArgument $null } 'OWNER_ARGUMENT_INVALID') 'argument_null_refused'
Check (Refuses { ConvertTo-DevLocalOwnerArgument ('x' * 32767) } 'OWNER_ARGUMENT_INVALID') 'argument_limit_refused_before_quoting'
$entries = @([pscustomobject]@{Name='Z';Value='last'},[pscustomobject]@{Name='A';Value=$unicode})
$block = New-DevLocalOwnerEnvironmentBlock $entries
Check ($block -ceq ('A=' + $unicode + [char]0 + 'Z=last' + [char]0 + [char]0)) 'environment_sorted_unicode_double_nul'
Check ((New-DevLocalOwnerEnvironmentBlock @()) -ceq ([string][char]0 + [char]0)) 'empty_environment_double_nul'
foreach ($bad in @(
    @([pscustomobject]@{Name='A';Value='one'},[pscustomobject]@{Name='A';Value='two'}),
    @([pscustomobject]@{Name='lower';Value='one'}),
    @([pscustomobject]@{Name='A=B';Value='one'}),
    @([pscustomobject]@{Name='A';Value=('one' + [char]0)})
)) { Check (Refuses { New-DevLocalOwnerEnvironmentBlock $bad } 'OWNER_ENVIRONMENT_INVALID') ('invalid_environment_' + $cases.Count) }
Check (Refuses { New-DevLocalOwnerEnvironmentBlock @([pscustomobject]@{Name='A';Value=('x' * 32765)}) } 'OWNER_ENVIRONMENT_TOO_LARGE') 'environment_limit_refused'
$boundary = New-DevLocalOwnerEnvironmentBlock @([pscustomobject]@{Name='A';Value=('x' * 32763)})
Check ($boundary.Length -eq 32767 -and $boundary.EndsWith([string][char]0 + [char]0)) 'environment_single_entry_exact_boundary'
$boundary = New-DevLocalOwnerEnvironmentBlock @([pscustomobject]@{Name='A';Value=('x' * 16380)},[pscustomobject]@{Name='B';Value=('x' * 16380)})
Check ($boundary.Length -eq 32767 -and $boundary.EndsWith([string][char]0 + [char]0)) 'environment_multi_entry_exact_boundary'
Check (Refuses { New-DevLocalOwnerEnvironmentBlock @([pscustomobject]@{Name='A';Value=('x' * 32764)}) } 'OWNER_ENVIRONMENT_TOO_LARGE') 'environment_one_character_over_boundary'
$visited = [Collections.Generic.List[string]]::new()
$sentinel = [pscustomobject]@{Value='unused'}
$getter = { $visited.Add('late'); return 'lower' }.GetNewClosure()
$sentinel | Add-Member -MemberType ScriptProperty -Name Name -Value $getter
$early = Refuses {
    New-DevLocalOwnerEnvironmentBlock @([pscustomobject]@{Name='A';Value=('x' * 16380)},[pscustomobject]@{Name='B';Value=('x' * 16381)},$sentinel)
} 'OWNER_ENVIRONMENT_TOO_LARGE'
Check ($early -and $visited.Count -eq 0) 'environment_overflow_stops_before_later_getter'
foreach ($operation in @('build','clippy','test','fmt')) {
    $p = New-DevLocalOwnerPreparation -Operation $operation -TargetPath 'C:\synthetic-owned-target'
    Check ($p.operation -ceq $operation -and !$p.executionQualified -and !$p.targetReuseAuthorized -and $p.state -ceq 'PREPARED_INPUT_ONLY') ($operation + '_preparation_is_not_authority')
    Check ($p.environmentOverrides.CARGO_INCREMENTAL -ceq '0') ($operation + '_incremental_off_policy')
    if ($operation -ceq 'fmt') {
        Check (($p.arguments -join ' ') -ceq 'fmt --all -- --check') 'fmt_target_flag_not_invented'
    } else {
        $at = [array]::IndexOf($p.arguments, '--target-dir')
        Check ($at -ge 0 -and $p.arguments[$at+1] -ceq 'C:\synthetic-owned-target' -and $p.arguments -ccontains '--offline' -and $p.arguments -ccontains '--locked' -and $p.arguments -ccontains '-j1') ($operation + '_fixed_offline_locked_explicit_target')
    }
}
foreach ($path in @('relative','C:\','\\server\share','C:\bad:stream','C:\bad\..\target','C:\bad\target.','C:\bad\target ','C:\bad*target','C:\NUL','C:\COM1.txt',('C:\bad' + [char]0))) {
    Check (Refuses { New-DevLocalOwnerPreparation -Operation build -TargetPath $path } 'OWNER_TARGET_INPUT_INVALID') ('invalid_target_' + $cases.Count)
}
$refused = $false
try { $null = New-DevLocalOwnerPreparation -Operation run -TargetPath 'C:\synthetic' } catch [Management.Automation.ParameterBindingException] { $refused = $true }
Check $refused 'arbitrary_operation_not_available'
$contract = Get-DevLocalOwnerContract
Check (!$contract.nativeEntryPointAvailable -and !$contract.executionQualified -and !$contract.targetReuseAuthorized -and $contract.blocker -ceq 'OWNER_OVERLAPPED_DRAIN_NOT_IMPLEMENTED') 'contract_does_not_offer_native_entry'
$commands = @($ast.FindAll({param($n) $n -is [Management.Automation.Language.CommandAst]}, $true) | ForEach-Object { $_.GetCommandName() })
Check (!(@($commands | Where-Object { $_ -cin @('Add-Type','Start-Process','Invoke-Expression','cargo','rustc') }).Count)) 'no_native_compile_or_child_launch_command'
[pscustomobject][ordered]@{
    schemaVersion = 1; classification = 'PURE_SOURCE_PREPARATION_ONLY'
    hostVersion = $PSVersionTable.PSVersion.ToString(); hostEdition = $PSVersionTable.PSEdition
    passed = $cases.Count; nativeCompilations = 0; nativeChildFixtures = 0
    realCargoRuns = 0; targetCreated = $false; executionQualified = $false
    blocker = 'OWNER_OVERLAPPED_DRAIN_NOT_IMPLEMENTED'
    cases = @($cases)
} | ConvertTo-Json -Depth 4 -Compress
