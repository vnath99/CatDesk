[CmdletBinding()]
param()
$ErrorActionPreference='Stop'
$source=Join-Path $PSScriptRoot 'promote-reviewed-catdesk-build.ps1'
$tokens=$null;$errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile($source,[ref]$tokens,[ref]$errors)
if($errors.Count){throw 'promoter parse failed'}
$functions=@(foreach($name in @('Invoke-PromotionSeam','Invoke-PromotionHandoff')){
 $found=@($ast.FindAll({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -ceq $name},$true))
 if($found.Count -ne 1){throw 'handoff function is ambiguous'}
 $found[0].Extent.Text
}) -join "`n"
$temp=[IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\')
$root=Join-Path $temp ('CatDesk-HandoffStatus-'+[guid]::NewGuid().ToString('N'))
$null=New-Item -ItemType Directory -Path $root
try{
 $helper=@'
param($OldPid,$ExpectedOldProcessStartedAtUtc,$ExpectedOldProcessPath,$ExpectedOldProcessSha256,$BuildPath,$Workspace,$McpPort,$ReadyTimeoutSeconds,[switch]$BackendOnly,[switch]$Execute)
$mode=[IO.File]::ReadAllText((Join-Path $Workspace 'mode.txt'))
if($mode -eq 'throw'){throw 'controlled fixture launcher failure'}
if($mode -eq 'empty'){return}
if($mode -eq 'exit'){exit 9}
$dir=Join-Path $Workspace '.catdesk/restart-handoff';$null=New-Item -ItemType Directory -Path $dir -Force
@{schemaVersion=4;profile='LOCAL_BACKEND_ONLY';mcpAuthenticated=$true;status='RECOVERED_PENDING_TRANSPORT_CHECK';completedAtUtc=[DateTime]::UtcNow.ToString('o');buildSha256=('a'*64)}|ConvertTo-Json -Compress|Set-Content -LiteralPath (Join-Path $dir 'latest.json') -Encoding UTF8
'HANDOFF_DETACHED fixture only; no process launched'
'@
 $runner=@'
param([string]$Workspace,[string]$Seed)
$ErrorActionPreference='Stop'
$BackendOnly=$true;$script:PromotionSeams=@{};$script:PromotionMcpPort=3200;$script:PromotionTimeoutSeconds=1
function ConvertFrom-CatDeskLiteralJson([string]$Text){$Text|ConvertFrom-Json}
'@
 $runner+="`n"+$functions+"`n"+@'
if($Seed -eq 'unset'){Remove-Variable LASTEXITCODE -Scope Global -ErrorAction SilentlyContinue}else{$global:LASTEXITCODE=7}
$from=[pscustomobject]@{Pid=42;ProcessStartedAtUtc='2026-10-28T00:00:00.0000000Z';BuildPath=(Join-Path $Workspace 'old.exe');BuildHash=('b'*64)}
$threw=$false;$result=$false
try{$result=Invoke-PromotionHandoff $Workspace $from (Join-Path $Workspace 'candidate.exe') ('a'*64)}catch{$threw=$true}
@{result=$result;threw=$threw}|ConvertTo-Json -Compress
'@
 [IO.File]::WriteAllText((Join-Path $root 'restart_catdesk_daemon.ps1'),$helper)
 $runnerPath=Join-Path $root 'runner.ps1';[IO.File]::WriteAllText($runnerPath,$runner)
 $engine=Join-Path $PSHOME $(if($PSEdition -eq 'Core'){'pwsh.exe'}else{'powershell.exe'})
 foreach($case in @(@{mode='success';seed='unset';expected=$true},@{mode='success';seed='stale';expected=$true},@{mode='empty';seed='unset';expected=$false},@{mode='throw';seed='unset';expected=$false},@{mode='exit';seed='unset';expected=$false})){
  $workspace=Join-Path $root ($case.mode+'-'+$case.seed);$null=New-Item -ItemType Directory -Path $workspace
  [IO.File]::WriteAllText((Join-Path $workspace 'mode.txt'),$case.mode)
  $info=[Diagnostics.ProcessStartInfo]::new();$info.FileName=$engine;$info.Arguments='-NoProfile -NonInteractive -File "'+$runnerPath+'" -Workspace "'+$workspace+'" -Seed '+$case.seed
  $info.UseShellExecute=$false;$info.CreateNoWindow=$true;$info.RedirectStandardOutput=$true;$info.RedirectStandardError=$true;$info.EnvironmentVariables.Remove('PSModulePath')
  $process=[Diagnostics.Process]::Start($info);$null=$process.Handle
  try{
   $output=$process.StandardOutput.ReadToEndAsync();$err=$process.StandardError.ReadToEndAsync()
   if(!$process.WaitForExit(15000)){throw 'fixture engine timeout'}
   if(!$output.Wait(1000) -or !$err.Wait(1000) -or $process.ExitCode -ne 0 -or $err.Result.Length -or $output.Result.Length -gt 4096){throw 'fixture engine failed'}
   $reply=$output.Result|ConvertFrom-Json
   if([bool]$reply.result -ne $case.expected -or ($case.expected -and $reply.threw)){throw ('handoff script status failed: '+$case.mode+'/'+$case.seed)}
   Write-Output ('PASS '+$case.mode+'/'+$case.seed)
  }finally{if(!$process.HasExited){$process.Kill();[void]$process.WaitForExit(5000)};$process.Dispose()}
 }
}finally{
 $resolved=[IO.Path]::GetFullPath($root)
 if([IO.Path]::GetDirectoryName($resolved) -cne $temp -or [IO.Path]::GetFileName($resolved) -cnotmatch '^CatDesk-HandoffStatus-[0-9a-f]{32}$'){throw 'fixture cleanup scope refused'}
 Remove-Item -LiteralPath $resolved -Recurse -Force
}
