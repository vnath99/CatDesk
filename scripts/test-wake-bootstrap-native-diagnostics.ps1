#Requires -Version 7
$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '../wake/install.ps1'
$tokens = $null
$parseErrors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile($source, [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count -ne 0) { throw 'BOOTSTRAP_INSTALL_SCRIPT_PARSE_ERROR' }
$definitions = @($ast.FindAll({
    param($node)
    $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
        $node.Name -ceq 'New-CatDeskWakeBootstrapNativeFailure'
}, $true))
if ($definitions.Count -ne 1) { throw 'BOOTSTRAP_NATIVE_FAILURE_FUNCTION_NOT_FOUND' }
# Load only the diagnostic function. The installation entry point is never run.
. ([scriptblock]::Create($definitions[0].Extent.Text))

$cases = @(
    @{ Name = 'native JSON on stdout'; Out = '{"error":"WAKE_BOOTSTRAP_SCM_SECURITY_QUARANTINED"}'
       Err = ''; Exit = 1; Action = 'bootstrap'; Message = 'WAKE_BOOTSTRAP_SCM_SECURITY_QUARANTINED'; Hex = '0x00000001' }
    @{ Name = 'legacy stderr code'; Out = ''; Err = "WAKE_BOOTSTRAP_SERVICE_ALREADY_EXISTS`r`n"
       Exit = 1; Action = 'bootstrap-preflight'; Message = 'WAKE_BOOTSTRAP_SERVICE_ALREADY_EXISTS'; Hex = '0x00000001' }
    @{ Name = 'crash with no structured frame'; Out = ''; Err = ''
       Exit = -1073741819; Action = 'bootstrap'; Message = 'WAKE_BOOTSTRAP_NATIVE_REFUSED_NO_RETRY (bootstrap exit -1073741819 / 0xC0000005)'; Hex = '0xC0000005' }
    @{ Name = 'conflicting frames'; Out = '{"error":"WAKE_CODE_ONE"}'; Err = 'WAKE_CODE_TWO'
       Exit = 1; Action = 'bootstrap'; Message = 'WAKE_BOOTSTRAP_NATIVE_REFUSED_NO_RETRY (bootstrap exit 1 / 0x00000001)'; Hex = '0x00000001' }
    @{ Name = 'extra private data'; Out = '{"error":"WAKE_CODE_ONE","private":"must not surface"}'; Err = 'must not surface'
       Exit = 1; Action = 'bootstrap-preflight'; Message = 'WAKE_BOOTSTRAP_NATIVE_REFUSED_NO_RETRY (bootstrap-preflight exit 1 / 0x00000001)'; Hex = '0x00000001' }
    @{ Name = 'malformed concatenation'; Out = '{"error":"WAKE_CODE_ONE"}{"error":"WAKE_CODE_TWO"}'; Err = ''
       Exit = 1; Action = 'bootstrap'; Message = 'WAKE_BOOTSTRAP_NATIVE_REFUSED_NO_RETRY (bootstrap exit 1 / 0x00000001)'; Hex = '0x00000001' }
)
foreach ($case in $cases) {
    $failure = New-CatDeskWakeBootstrapNativeFailure -Stdout $case.Out -Stderr $case.Err -ExitCode $case.Exit -Action $case.Action
    if ($failure.Message -cne $case.Message -or
        $failure.Data['NativeAction'] -cne $case.Action -or
        $failure.Data['NativeExitCode'] -ne $case.Exit -or
        $failure.Data['NativeExitHex'] -cne $case.Hex) {
        throw "BOOTSTRAP_DIAGNOSTIC_CASE_FAILED: $($case.Name)"
    }
}
"Wake bootstrap native diagnostics: $($cases.Count) cases passed."
