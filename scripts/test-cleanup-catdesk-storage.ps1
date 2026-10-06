[CmdletBinding()]
param()

$ErrorActionPreference='Stop'
$workspaceRoot=(Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$fixture=Join-Path $workspaceRoot 'scripts\.tmp-t0057-storage-cleanup-fixture'
$cleanupScript=Join-Path $PSScriptRoot 'cleanup-catdesk-storage.ps1'

function Require([bool]$Condition,[string]$Message) {
 if(-not $Condition) { throw "assertion failed: $Message" }
}

function Require-Throws([scriptblock]$Action,[string]$Message) {
 try { & $Action } catch { return }
 throw "assertion failed: $Message"
}

function New-BuildTree([string]$Path) {
 New-Item -ItemType Directory -Path (Join-Path $Path 'debug') -Force | Out-Null
 Set-Content -LiteralPath (Join-Path $Path 'CACHEDIR.TAG') -Value 'fixture' -Encoding ascii
 Set-Content -LiteralPath (Join-Path $Path '.rustc_info.json') -Value '{}' -Encoding ascii
 Set-Content -LiteralPath (Join-Path $Path 'debug\artifact.bin') -Value 'rebuildable' -Encoding ascii
}

function Get-Sha256Hex([string]$Text) {
 $sha=[Security.Cryptography.SHA256]::Create()
 try { return (($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($Text)) | ForEach-Object { $_.ToString('x2') }) -join '') }
 finally { $sha.Dispose() }
}

function Save-Plan([object]$Plan,[string]$Path) {
 $basis=($Plan.entries | ConvertTo-Json -Depth 4 -Compress)
 $Plan.digest=Get-Sha256Hex $basis
 $Plan.confirmToken='T0057-'+$Plan.digest.Substring(0,16)
 $Plan | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $Path -Encoding utf8
}

try {
 if(Test-Path -LiteralPath $fixture) { Remove-Item -LiteralPath $fixture -Recurse -Force }
 New-Item -ItemType Directory -Path $fixture -Force | Out-Null
 $candidate=Join-Path $fixture 'target\candidate-a'
 $protected=Join-Path $fixture 'target\contains-protected-marker'
 $unknown=Join-Path $fixture 'target\t0048-name-only'
 $release=Join-Path $fixture 'target\release'
 $liveState=Join-Path $fixture '.catdesk\t0048-live-target'
 $junctionCandidate=Join-Path $fixture 'target\contains-junction'
 New-BuildTree $candidate
 New-BuildTree $protected
 New-Item -ItemType Directory -Path (Join-Path $protected 'autonomy') -Force | Out-Null
 New-Item -ItemType Directory -Path $unknown -Force | Out-Null
 Set-Content -LiteralPath (Join-Path $unknown 'notes.txt') -Value 'ticket-like name only' -Encoding ascii
 New-BuildTree $release
 New-BuildTree $liveState
 New-Item -ItemType Directory -Path (Join-Path $liveState 'wake-bridge') -Force | Out-Null
 New-BuildTree $junctionCandidate
 $junctionTarget=Join-Path $fixture 'junction-target'
 New-Item -ItemType Directory -Path $junctionTarget -Force | Out-Null
 New-Item -ItemType Junction -Path (Join-Path $junctionCandidate 'linked') -Target $junctionTarget -ErrorAction Stop | Out-Null

 $defaultPlanPath=Join-Path $fixture 'default-plan.json'
 $defaultPlanJson=& $cleanupScript -Workspace $fixture -PlanPath $defaultPlanPath
 $defaultPlan=$defaultPlanJson | ConvertFrom-Json
 Require (Test-Path -LiteralPath $candidate) 'default plan mode makes no mutation'
 Require ($defaultPlan.entries.Count -eq 1) 'only a structurally proven candidate is planned'
 Require ($defaultPlan.entries[0].path -eq 'target\candidate-a') 'the known Cargo-like tree is planned'
 Require ($defaultPlan.entries[0].protectedMarkerScan -eq 'CLEAR') 'manifest records a clear protected-marker scan'
 Require (-not ($defaultPlan.PSObject.Properties.Name -contains 'workspace')) 'persisted plan avoids an absolute workspace path'
 Require ($defaultPlan.confirmToken -match '^T0057-[0-9a-f]{16}$') 'plan confirmation is digest-bound'
 Require (-not (@($defaultPlan.entries.path) -contains 'target\release')) 'canonical release is rejected'
 Require (-not (@($defaultPlan.entries.path) -contains 'target\t0048-name-only')) 'ticket-like name alone is rejected'
 Require (-not (@($defaultPlan.entries.path) -contains '.catdesk\t0048-live-target')) 'live-state marker rejects historical .catdesk tree'
 Require (-not (@($defaultPlan.entries.path) -contains 'target\contains-junction')) 'junction descendants are rejected'

 $verifyCandidate=Join-Path $fixture 'target-verify\historical-build'
 New-BuildTree $verifyCandidate
 $verifyPlanPath=Join-Path $fixture 'verify-plan.json'
 $verifyPlanJson=& $cleanupScript -Workspace $fixture -PlanPath $verifyPlanPath
 $verifyPlan=$verifyPlanJson | ConvertFrom-Json
 Require (@($verifyPlan.entries.path) -contains 'target-verify\historical-build') 'historical target-verify Cargo tree is planned'
 Require ((Get-Content -LiteralPath $cleanupScript -Raw) -match 'Is-LiveBuildRuntime') 'cleanup requires an active build-owner check'
 Remove-Item -LiteralPath (Join-Path $fixture 'target-verify') -Recurse -Force

 $driftPlanPath=Join-Path $fixture 'drift-plan.json'
 Copy-Item -LiteralPath $defaultPlanPath -Destination $driftPlanPath
 Add-Content -LiteralPath (Join-Path $candidate 'debug\artifact.bin') -Value 'drift' -Encoding ascii
 Require-Throws { & $cleanupScript -Mode execute -Workspace $fixture -PlanPath $driftPlanPath -ConfirmToken $defaultPlan.confirmToken } 'byte drift rejects execution'
 Set-Content -LiteralPath (Join-Path $candidate 'debug\artifact.bin') -Value 'rebuildable' -Encoding ascii

 $planPath=Join-Path $fixture 'manifest.json'
 $planJson=& $cleanupScript -Mode plan -Workspace $fixture -PlanPath $planPath
 $plan=$planJson | ConvertFrom-Json
 $escapePlan=$planJson | ConvertFrom-Json
 $escapePlan.entries[0].path='..\outside'
 $escapePlanPath=Join-Path $fixture 'escape-plan.json'
 Save-Plan $escapePlan $escapePlanPath
 Require-Throws { & $cleanupScript -Mode execute -Workspace $fixture -PlanPath $escapePlanPath -ConfirmToken $escapePlan.confirmToken } 'path escape rejects execution'

 $resultJson=& $cleanupScript -Mode execute -Workspace $fixture -PlanPath $planPath -ConfirmToken $plan.confirmToken
 $result=$resultJson | ConvertFrom-Json
 Require ($result.status -eq 'REMOVED') 'exact frozen plan removes its approved root'
 Require (-not(Test-Path -LiteralPath $candidate)) 'approved candidate directory no longer exists'
 Require (Test-Path -LiteralPath $protected) 'protected marker retains the entire candidate'
 Require (Test-Path -LiteralPath $unknown) 'unknown data is retained'
 Require (Test-Path -LiteralPath $release) 'canonical release is retained'
 Require (Test-Path -LiteralPath $liveState) 'live-state tree is retained'
 Require ((Get-Content -LiteralPath $cleanupScript -Raw) -match 'FAILED_LOCKED_OR_ACCESS') 'locked/access failures have an explicit receipt status'
 Write-Output 'cleanup-catdesk-storage fixture tests passed'
} finally {
 if(Test-Path -LiteralPath $fixture) { Remove-Item -LiteralPath $fixture -Recurse -Force }
}
