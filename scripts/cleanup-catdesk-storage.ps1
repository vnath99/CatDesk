[CmdletBinding()]
param(
 [ValidateSet('plan','execute')][string]$Mode='plan',
 [string]$Workspace=(Join-Path $PSScriptRoot '..'),
 [string]$PlanPath=(Join-Path $PSScriptRoot '..\docs\orchestrator\T-0057_SAFE_STORAGE_CLEANUP_MANIFEST_PRE.json'),
 [string]$ConfirmToken=''
)

$ErrorActionPreference='Stop'

function Is-ReparsePoint([string]$Path) {
 return ((Get-Item -LiteralPath $Path -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0
}

function Get-DirectoryBytes([string]$Path) {
 $bytes=[Int64]0
 Get-ChildItem -LiteralPath $Path -Force -File -Recurse -ErrorAction Stop | ForEach-Object {
  $bytes=[Int64]($bytes+[Int64]$_.Length)
 }
 return $bytes
}

function Get-BuildSignatures([string]$Path) {
 $signatures=[Collections.Generic.List[string]]::new()
 foreach($name in @('CACHEDIR.TAG','.rustc_info.json','debug','release','deps','.fingerprint','incremental')) {
  if(Test-Path -LiteralPath (Join-Path $Path $name)) { $signatures.Add($name) }
 }
 return @($signatures)
}

function Get-WorkspaceFingerprint([string]$WorkspaceRoot) {
 $raw=[Text.Encoding]::UTF8.GetBytes($WorkspaceRoot)
 return Get-Sha256Hex $raw
}

function Get-Sha256Hex([byte[]]$Bytes) {
 $sha=[Security.Cryptography.SHA256]::Create()
 try {
  return (($sha.ComputeHash($Bytes) | ForEach-Object { $_.ToString('x2') }) -join '')
 } finally {
  $sha.Dispose()
 }
}

function Is-ProtectedPath([string]$WorkspaceRoot,[string]$Path) {
 $relative=$Path.Substring($WorkspaceRoot.Length).TrimStart('\')
 $blocked=@(
  '.git','target\release','.catdesk\autonomy','.catdesk\wake-bridge',
  '.catdesk\restart-handoff','.catdesk\current_plan.md','.catdesk\project.md',
  '.catdesk\decisions.md','.catdesk\todo.md','.catdesk\session.md',
  '.catdesk\repo_map.md','.catdesk\projects','.catdesk\logs'
 )
 return @($blocked | Where-Object {
  $relative.Equals($_,[StringComparison]::OrdinalIgnoreCase) -or
  $relative.StartsWith($_+'\',[StringComparison]::OrdinalIgnoreCase)
 }).Count -gt 0
}

function Contains-ProtectedMarker([string]$Path) {
 $blockedDirectories=@('.git','autonomy','wake-bridge','restart-handoff','projects','logs','diagnostics-src','recovery','migration','review_bundles','src','tests','scripts','docs','assets')
 $blockedFiles=@('current_plan.md','project.md','decisions.md','todo.md','session.md','repo_map.md','Cargo.toml','Cargo.lock','config.toml','config.json')
 try {
  foreach($item in (Get-ChildItem -LiteralPath $Path -Force -Recurse -ErrorAction Stop)) {
   if(Is-ReparsePoint $item.FullName) { return $true }
   if($item.PSIsContainer -and $blockedDirectories -contains $item.Name) { return $true }
   if(-not $item.PSIsContainer -and $blockedFiles -contains $item.Name) { return $true }
  }
  return $false
 } catch { return $true }
}

function Is-LiveCatDeskRuntime([string]$Path) {
 try {
  $needle=$Path.TrimEnd('\')+'\'
  foreach($process in (Get-CimInstance Win32_Process -ErrorAction Stop | Where-Object { $_.Name -match '^catdesk(\.exe)?$' })) {
   if($process.ExecutablePath -and $process.ExecutablePath.StartsWith($needle,[StringComparison]::OrdinalIgnoreCase)) { return $true }
  }
 } catch {
  # Inspection failure is deliberately conservative for any generated build
  # tree that could plausibly host the currently serving controller.
  if($Path -match '[\\/](target|target-verify|target-debug)([\\/]|$)') { return $true }
 }
 return $false
}

function Is-LiveBuildRuntime([string]$Path) {
 try {
  $needle=$Path.TrimEnd('\')
  foreach($process in (Get-CimInstance Win32_Process -ErrorAction Stop | Where-Object { $_.Name -match '^(cargo|rustc|link)(\.exe)?$' })) {
   $commandLine=[string]$process.CommandLine
   if($commandLine -and $commandLine.IndexOf($needle,[StringComparison]::OrdinalIgnoreCase) -ge 0) { return $true }
  }
 } catch {
  # Process-inspection failure never converts a generated build tree into a
  # deletion candidate.
  if($Path -match '[\\/](target|target-verify|target-debug)([\\/]|$)') { return $true }
 }
 return $false
}

function Test-CleanupCandidate([string]$WorkspaceRoot,[string]$Path) {
 $full=(Resolve-Path -LiteralPath $Path -ErrorAction Stop).Path
 if(-not $full.StartsWith($WorkspaceRoot+'\',[StringComparison]::OrdinalIgnoreCase)) { return $false }
 if((Is-ReparsePoint $full) -or (Is-ProtectedPath $WorkspaceRoot $full)) { return $false }
 if(Contains-ProtectedMarker $full) { return $false }
 if((Get-BuildSignatures $full).Count -lt 2) { return $false }
 if(Is-LiveCatDeskRuntime $full) { return $false }
 if(Is-LiveBuildRuntime $full) { return $false }
 return $true
}

function Get-Candidates([string]$WorkspaceRoot) {
 $candidates=@()
 $target=Join-Path $WorkspaceRoot 'target'
 if(Test-Path -LiteralPath $target -PathType Container) {
  Get-ChildItem -LiteralPath $target -Directory -Force | ForEach-Object {
   if($_.Name -ne 'release') { $candidates+=$_.FullName }
  }
 }
 foreach($rootName in @('target-verify','target-debug')) {
  $verificationRoot=Join-Path $WorkspaceRoot $rootName
  if(Test-Path -LiteralPath $verificationRoot -PathType Container) {
   Get-ChildItem -LiteralPath $verificationRoot -Directory -Force | ForEach-Object {
    $candidates+=$_.FullName
   }
  }
 }
 $catdesk=Join-Path $WorkspaceRoot '.catdesk'
 if(Test-Path -LiteralPath $catdesk -PathType Container) {
  Get-ChildItem -LiteralPath $catdesk -Directory -Force | ForEach-Object {
   if($_.Name -match '^(t004[678]|reload-target)') { $candidates+=$_.FullName }
  }
 }
 return @($candidates)
}

$workspaceRoot=(Resolve-Path -LiteralPath $Workspace -ErrorAction Stop).Path
if($Mode -eq 'plan') {
 $entries=@()
 foreach($candidate in (Get-Candidates $workspaceRoot)) {
  if(Test-CleanupCandidate $workspaceRoot $candidate) {
   $entries += [pscustomobject]@{
    path=$candidate.Substring($workspaceRoot.Length).TrimStart('\')
    bytes=Get-DirectoryBytes $candidate
    signatures=Get-BuildSignatures $candidate
    protectedMarkerScan='CLEAR'
    classification='REBUILDABLE'
    rationale='Multiple Cargo/build signatures; no reparse point, protected marker, or live CatDesk runtime.'
   }
  }
 }
 $entries=@($entries | Sort-Object path)
 $basis=($entries | ConvertTo-Json -Depth 4 -Compress)
 $digest=Get-Sha256Hex ([Text.Encoding]::UTF8.GetBytes($basis))
 $plan=[pscustomobject]@{
  schemaVersion=1
  workspaceFingerprint=Get-WorkspaceFingerprint $workspaceRoot
  entries=$entries
  digest=$digest
  confirmToken=('T0057-'+$digest.Substring(0,16))
 }
 $directory=Split-Path -Parent $PlanPath
 if(-not(Test-Path -LiteralPath $directory)) { New-Item -ItemType Directory -Path $directory -Force | Out-Null }
 $plan | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $PlanPath -Encoding utf8
 $plan | ConvertTo-Json -Depth 5
 exit 0
}

if(-not(Test-Path -LiteralPath $PlanPath -PathType Leaf)) { throw 'frozen cleanup plan is unavailable' }
$plan=Get-Content -LiteralPath $PlanPath -Raw | ConvertFrom-Json
if($plan.workspaceFingerprint -ne (Get-WorkspaceFingerprint $workspaceRoot)) { throw 'cleanup plan workspace mismatch' }
if($ConfirmToken -ne $plan.confirmToken) { throw 'cleanup confirmation token mismatch' }
$basis=($plan.entries | ConvertTo-Json -Depth 4 -Compress)
$digest=Get-Sha256Hex ([Text.Encoding]::UTF8.GetBytes($basis))
if($digest -ne $plan.digest) { throw 'cleanup plan digest mismatch' }

$results=@()
foreach($entry in $plan.entries) {
 $path=Join-Path $workspaceRoot $entry.path
 if(-not(Test-CleanupCandidate $workspaceRoot $path)) { throw "cleanup candidate drift: $($entry.path)" }
 if((Get-DirectoryBytes $path) -ne [Int64]$entry.bytes) { throw "cleanup candidate size drift: $($entry.path)" }
 try {
  Remove-Item -LiteralPath $path -Recurse -Force -ErrorAction Stop
  $results += [pscustomobject]@{path=$entry.path;status='REMOVED';bytes=$entry.bytes}
 } catch {
  $results += [pscustomobject]@{path=$entry.path;status='FAILED_LOCKED_OR_ACCESS';bytes=$entry.bytes}
 }
}
$results | ConvertTo-Json -Depth 3
