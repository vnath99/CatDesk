$ErrorActionPreference='Stop';$audit=Join-Path $PSScriptRoot 'audit-catdesk-storage.ps1';$root=Join-Path ([IO.Path]::GetTempPath()) ('catdesk-audit-'+[Guid]::NewGuid().ToString('N'));New-Item -ItemType Directory -Path $root|Out-Null
try {
  New-Item -ItemType Directory -Path "$root\target\staging","$root\.catdesk\autonomy","$root\mystery" -Force|Out-Null
  [IO.File]::WriteAllBytes("$root\target\staging\blob.bin",[byte[]](1..20));[IO.File]::WriteAllBytes("$root\.catdesk\autonomy\state.bin",[byte[]](1..10));[IO.File]::WriteAllBytes("$root\mystery\x.bin",[byte[]](1..5))
  $json=Join-Path $root 'report.json';$output=& powershell -NoProfile -ExecutionPolicy Bypass -File $audit -Workspace $root -JsonPath $json|Out-String;$report=Get-Content $json -Raw|ConvertFrom-Json
  if($report.reparsePointsFollowed -ne $false -or $report.workspaceBytes -lt 35){throw 'size or reparse policy failed'}
  $target=@($report.entries|Where-Object path -eq 'target')[0];$runtime=@($report.entries|Where-Object path -eq '.catdesk')[0];$unknown=@($report.entries|Where-Object path -eq 'mystery')[0]
  if($target.classification -ne 'REBUILDABLE' -or -not $target.deletionEverEligible){throw 'target classification failed'};if($runtime.classification -ne 'PROTECTED_RUNTIME' -or $runtime.deletionEverEligible){throw 'runtime classification failed'};if($unknown.classification -ne 'UNKNOWN' -or $unknown.deletionEverEligible){throw 'unknown was not fail closed'}
  if(-not(Test-Path "$root\target\staging\blob.bin") -or -not($output -match 'schemaVersion')){throw 'audit mutated fixture or JSON output missing'}
} finally {Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue}
Write-Output 'storage audit fixture tests passed'
