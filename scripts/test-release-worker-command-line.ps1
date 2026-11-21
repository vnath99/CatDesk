[CmdletBinding()]
param()
$ErrorActionPreference='Stop'
$source=[IO.File]::ReadAllText((Join-Path $PSScriptRoot '../src/promotion_retirement.rs'))
$marker='const INSPECTION: &str = r#"';$start=$source.IndexOf($marker,[StringComparison]::Ordinal)
if($start -lt 0){throw 'inspection source not found'}
$start+=$marker.Length;$end=$source.IndexOf('"#;',$start,[StringComparison]::Ordinal)
$tokens=$null;$errors=$null;$ast=[Management.Automation.Language.Parser]::ParseInput($source.Substring($start,$end-$start),[ref]$tokens,[ref]$errors)
if($errors.Count){throw 'inspection source parse failed'}
$function=@($ast.FindAll({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -ceq 'Test-CatDeskReleaseWorkerCommandLine'},$true))
if($function.Count -ne 1){throw 'worker classifier ambiguous'}
Invoke-Expression $function[0].Extent.Text
$root='C:\fixture root'
$worker=Join-Path $root 'scripts/restart_catdesk_daemon_worker.ps1'
$promoter=Join-Path $root 'scripts/promote-reviewed-catdesk-build.ps1'
$cases=@(
 @{line=('powershell.exe -NoProfile -File "'+$worker+'" -OldPid 42');expected=$true},
 @{line=('"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "'+$promoter+'" -Execute');expected=$true},
 @{line=('pwsh.exe -Command "Write-Output ''-File '+$worker+'''"');expected=$false},
 @{line=('pwsh.exe -Command "Get-Content '''+$promoter+'''"');expected=$false},
 @{line=('pwsh.exe -EncodedCommand Zml4dHVyZQ==');expected=$false},
 @{line='powershell.exe -NoProfile -File C:\other\scripts\restart_catdesk_daemon_worker.ps1';expected=$false},
 @{line=('powershell.exe -NoProfile -File "'+$worker+'.unrelated"');expected=$false}
)
foreach($case in $cases){if((Test-CatDeskReleaseWorkerCommandLine $case.line $root) -ne $case.expected){throw 'worker classifier result differs'}}
'PASS seven actual-file versus command-text classifier cases'
