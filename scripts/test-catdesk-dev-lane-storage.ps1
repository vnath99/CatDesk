[CmdletBinding()]
param()
$ErrorActionPreference='Stop';Set-StrictMode -Version Latest
$source=[IO.File]::ReadAllText((Join-Path $PSScriptRoot 'catdesk-dev-lane.ps1')).Replace([string][char]13,'')
$tokens=$null;$errors=$null;$ast=[Management.Automation.Language.Parser]::ParseInput($source,[ref]$tokens,[ref]$errors)
if($errors.Count){throw 'SOURCE_PARSE_FAILED'}
$cases=[Collections.Generic.List[string]]::new()
function Check([bool]$Ok,[string]$Name){if(!$Ok){throw $Name};$cases.Add($Name)}
foreach($name in @('Get-DevLaneStorageStatus','Invoke-VerificationCommands')){
 $f=@($ast.FindAll({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -ceq $name},$true))
 if($f.Count -ne 1){throw 'HELPER_NOT_UNIQUE'};. ([scriptblock]::Create($f[0].Extent.Text))
}
$script:Volume=@([pscustomobject]@{FreeSpace=[uint64](64GB);Capacity=[uint64](128GB);DeviceID='\\?\Volume{00000000-0000-0000-0000-000000000001}\'})
$script:CounterFailure=$false
function Read-DevLaneVolume([string]$DriveLetter){if($script:CounterFailure){throw 'PRIVATE_PROVIDER_FAILURE'};if($DriveLetter -cne 'C:'){throw 'WRONG_VOLUME'};return $script:Volume}
foreach($t in @(
 @{name='below_default_warns';free=(64GB-1);threshold=64GB;warning=$true},
 @{name='boundary_not_warning';free=64GB;threshold=64GB;warning=$false},
 @{name='above_default';free=(64GB+1);threshold=64GB;warning=$false},
 @{name='custom_threshold';free=99;threshold=100;warning=$true},
 @{name='zero_threshold';free=0;threshold=0;warning=$false}
)){
 $script:Volume[0].FreeSpace=[uint64]$t.free;$r=Get-DevLaneStorageStatus 'C:\synthetic-project' $t.threshold
 Check ($r.state -ceq 'AVAILABLE' -and $r.warning -eq $t.warning -and $r.warningFreeBytes -eq $t.threshold -and $r.referenceTargetPath -ceq 'C:\synthetic-project\target' -and $r.volumeId -ceq $script:Volume[0].DeviceID -and !$r.admissionGate -and !$r.directoryScanPerformed -and !$r.effectiveCargoTargetVerified) $t.name
}
$script:CounterFailure=$true;$r=Get-DevLaneStorageStatus 'C:\synthetic-project' 64GB
Check ($r.state -ceq 'UNAVAILABLE' -and $null -eq $r.warning -and ($r|ConvertTo-Json -Compress) -notmatch 'PRIVATE_PROVIDER_FAILURE') 'counter_failure_private_unavailable'
$script:CounterFailure=$false;$original=$script:Volume
foreach($t in @(
 @{name='missing_volume';value=@()},
 @{name='ambiguous_volume';value=@($original[0],$original[0])},
 @{name='negative_free';value=@([pscustomobject]@{FreeSpace=-1;Capacity=128GB;DeviceID=$original[0].DeviceID})},
 @{name='over_capacity';value=@([pscustomobject]@{FreeSpace=129GB;Capacity=128GB;DeviceID=$original[0].DeviceID})},
 @{name='zero_capacity';value=@([pscustomobject]@{FreeSpace=0;Capacity=0;DeviceID=$original[0].DeviceID})},
 @{name='invalid_identity';value=@([pscustomobject]@{FreeSpace=1;Capacity=2;DeviceID='unknown'})}
)){$script:Volume=$t.value;$r=Get-DevLaneStorageStatus 'C:\synthetic-project' 64GB;Check ($r.state -ceq 'UNAVAILABLE' -and $null -eq $r.warning) $t.name}
$roundTrip=$source
$added=@($ast.FindAll({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -cin @('Read-DevLaneVolume','Get-DevLaneStorageStatus')},$true)|Sort-Object {$_.Extent.StartOffset} -Descending)
foreach($f in $added){$roundTrip=$roundTrip.Remove($f.Extent.StartOffset,$f.Extent.EndOffset-$f.Extent.StartOffset+2)}
$branch=@'
if ($Command -eq 'storage-status') {
    Get-DevLaneStorageStatus -SourceRoot $root -WarningFreeBytes $StorageWarningFreeBytes | ConvertTo-Json -Compress
    return
}

'@
$newParameter=@'
    [string]$Command,
    [ValidateRange(0, [long]::MaxValue)]
    [long]$StorageWarningFreeBytes = 64GB
'@
$roundTrip=$roundTrip.Replace(($branch.Replace([string][char]13,'') + [char]10),'').Replace("'status', 'build', 'verify', 'storage-status'","'status', 'build', 'verify'").Replace($newParameter.Replace([string][char]13,''),'    [string]$Command')
$hash=[Security.Cryptography.SHA256]::Create()
try{$digest=([BitConverter]::ToString($hash.ComputeHash([Text.Encoding]::UTF8.GetBytes($roundTrip)))).Replace('-','').ToLowerInvariant()}finally{$hash.Dispose()}
Check ($digest -ceq 'dc95643829f2a11b5fe4fa3c337647f943015c83c783a065344f95cef7e99a18') 'legacy_receipt_commands_failure_semantics_and_source_preserved'
$binding=[scriptblock]::Create($ast.ParamBlock.Extent.Text + '; [pscustomobject]@{ command=$Command; threshold=$StorageWarningFreeBytes }')
$bound=& $binding -Command storage-status
Check ($bound.threshold -eq 64GB) 'public_default_configuration'
$bound=& $binding -Command storage-status -StorageWarningFreeBytes 1234
Check ($bound.threshold -eq 1234) 'public_custom_configuration'
$rejected=$false
try{$null=& $binding -Command storage-status -StorageWarningFreeBytes -1}catch [Management.Automation.ParameterBindingException]{$rejected=$true}
Check $rejected 'negative_threshold_parameter_rejected'
$prior=Get-Item Env:CARGO_INCREMENTAL -ErrorAction SilentlyContinue;$priorSet=$null -ne $prior;$priorValue=if($priorSet){[string]$prior.Value}else{$null}
$script:Calls=$null;$script:FailAt=0
function Invoke-FixedCommand([string]$Program,[string[]]$Arguments){$script:Calls.Add(@{program=$Program;args=($Arguments -join ' ');incremental=$env:CARGO_INCREMENTAL});if($script:FailAt -eq $script:Calls.Count){throw 'EXPECTED_COMMAND_FAILURE'}}
$emptySupported=$false
try{
 $env:CARGO_INCREMENTAL=$null;Set-Item Env:CARGO_INCREMENTAL ''
 $emptySupported=$null -ne (Get-Item Env:CARGO_INCREMENTAL -ErrorAction SilentlyContinue)
 $configs=@(@{set=$false;value=$null},@{set=$true;value='0'},@{set=$true;value='1'},@{set=$true;value='custom'})
 if($emptySupported){$configs+=@{set=$true;value=''}}
 foreach($config in $configs){foreach($failAt in @(0,1,2,3)){
  $env:CARGO_INCREMENTAL=$null;if($config.set){Set-Item Env:CARGO_INCREMENTAL $config.value}
  $script:Calls=[Collections.Generic.List[object]]::new();$script:FailAt=$failAt;$caught=$false
  try{Invoke-VerificationCommands}catch{if($_.Exception.Message -cne 'EXPECTED_COMMAND_FAILURE'){throw};$caught=$true}
  $after=Get-Item Env:CARGO_INCREMENTAL -ErrorAction SilentlyContinue;$count=if($failAt){$failAt}else{3}
  $ok=$caught -eq ($failAt -gt 0) -and $script:Calls.Count -eq $count -and (($null -ne $after) -eq $config.set)
  if($config.set){$ok=$ok -and ([string]$after.Value -ceq $config.value)}
  $argv=@('fmt --all -- --check','clippy --locked --offline --workspace --all-targets --all-features -- -D warnings','test --locked --offline --bin catdesk')
  for($i=0;$i -lt $script:Calls.Count;$i++){$ok=$ok -and $script:Calls[$i].program -ceq 'cargo' -and $script:Calls[$i].incremental -ceq '0' -and $script:Calls[$i].args -ceq $argv[$i]}
  Check $ok ('restoration_set_'+$config.set+'_value_'+$config.value+'_failure_'+$failAt)
 }}
}finally{if($priorSet){Set-Item Env:CARGO_INCREMENTAL $priorValue}else{$env:CARGO_INCREMENTAL=$null}}
[pscustomobject]@{edition=$PSVersionTable.PSEdition;version=$PSVersionTable.PSVersion.ToString();passedCases=$cases.Count;cases=$cases;emptyValueSupported=$emptySupported;realCargoInvocations=0;realVolumeQueries=0;baselineRoundTripSha256=$digest}|ConvertTo-Json -Depth 6
