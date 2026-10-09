[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Import-Module Microsoft.PowerShell.Utility -ErrorAction Stop
# The extracted bootstrap functions run in this isolated fixture scope.
# Keep SHA-256 verification deterministic without module-autoload dependence.
function Get-FixtureHash {
    param([string]$LiteralPath, [ValidateSet('SHA256')][string]$Algorithm = 'SHA256')
    $sha = [Security.Cryptography.SHA256]::Create()
    $stream = [IO.File]::OpenRead($LiteralPath)
    try { [pscustomobject]@{ Hash = ([BitConverter]::ToString($sha.ComputeHash($stream)).Replace('-', '')) } }
    finally { $sha.Dispose(); $stream.Dispose() }
}
Set-Alias -Name Get-FileHash -Value Get-FixtureHash -Scope Script
$workspace = Join-Path ([IO.Path]::GetTempPath()) ('catdesk-stale-daemon-fixture-' + [Guid]::NewGuid().ToString('N'))
$bootstrapPath = Join-Path $PSScriptRoot 'start-catdesk-stack.ps1'
$source = Get-Content -LiteralPath $bootstrapPath -Raw
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseInput($source, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'bootstrap script did not parse' }

function Require([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw "assertion failed: $Message" }
}

function Get-FreeLoopbackPort {
    $probe = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
    try {
        $probe.Start()
        return ([Net.IPEndPoint]$probe.LocalEndpoint).Port
    } finally {
        $probe.Stop()
    }
}

function Stop-ExactFixtureProcess([int]$ProcessId, [string]$ExpectedPath) {
    $process = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
    if ($null -eq $process) { return }
    $actual = (Resolve-Path -LiteralPath $process.Path -ErrorAction Stop).Path
    if (-not $actual.Equals($ExpectedPath, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'fixture cleanup refused a non-fixture process'
    }
    Stop-Process -Id $ProcessId -ErrorAction Stop
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    while ([DateTime]::UtcNow -lt $deadline -and (Get-Process -Id $ProcessId -ErrorAction SilentlyContinue)) {
        Start-Sleep -Milliseconds 100
    }
    if (Get-Process -Id $ProcessId -ErrorAction SilentlyContinue) { throw 'fixture process did not stop' }
}

foreach ($name in @('Invoke-BootstrapSeam', 'Resolve-TrustedBootstrapHelperPath', 'ConvertTo-NativeCommandLineArgument', 'Initialize-BoundedNativeProcessType', 'Resolve-TrustedWindowsPowerShellPath', 'Invoke-BoundedWindowsInventoryProbe', 'Get-LoopbackCatDeskListener', 'Test-CatDeskPinnedProcessMatchesRecoveryIdentity', 'Get-CatDeskListenerProcessInstanceForRecovery', 'Test-CatDeskListenerStillMatchesRecoveryCandidate', 'Stop-CatDeskListenerProcessForRecovery', 'Get-CatDeskDaemonProcessCandidates', 'Get-CatDeskDaemonProcessInstanceForRecovery', 'Stop-StaleCanonicalCatDeskDaemonForRecovery')) {
    $node = $ast.Find({ param($candidate) $candidate -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $candidate.Name -eq $name }, $true)
    if ($null -eq $node) { throw "missing production function $name" }
    . ([scriptblock]::Create($node.Extent.Text))
}
$script:MaxWindowsInventoryProbeMilliseconds = 5000
$script:MaxWindowsInventoryProbeOutputBytes = 65536
$script:WindowsInventoryProbeHelperPath = Join-Path $PSScriptRoot 'query-catdesk-windows-inventory.ps1'

$priorPort = $env:CATDESK_RECOVERY_FIXTURE_PORT
$hadPort = Test-Path Env:CATDESK_RECOVERY_FIXTURE_PORT
$priorMarker = $env:CATDESK_RECOVERY_FIXTURE_MARKER
$hadMarker = Test-Path Env:CATDESK_RECOVERY_FIXTURE_MARKER
$stale = $null
$replacement = $null
try {
    & cargo build --quiet --bin catdesk_recovery_fixture
    if ($LASTEXITCODE -ne 0) { throw 'recovery fixture binary did not build' }
    $fixtureBinary = Join-Path $PSScriptRoot '..\target\debug\catdesk_recovery_fixture.exe'
    if (-not (Test-Path -LiteralPath $fixtureBinary -PathType Leaf)) { throw 'recovery fixture binary is unavailable' }

    $release = Join-Path $workspace 'target\release'
    New-Item -ItemType Directory -Path $release -Force | Out-Null
    $canonicalPath = Join-Path $release 'catdesk.exe'
    Copy-Item -LiteralPath $fixtureBinary -Destination $canonicalPath -Force
    $canonicalPath = (Resolve-Path -LiteralPath $canonicalPath).Path
    $canonicalHash = (Get-FileHash -LiteralPath $canonicalPath -Algorithm SHA256).Hash.ToLowerInvariant()
    $port = Get-FreeLoopbackPort
    $marker = Join-Path $workspace 'stale-once.marker'
    [IO.File]::WriteAllText($marker, 'stale-once', [Text.UTF8Encoding]::new($false))
    $env:CATDESK_RECOVERY_FIXTURE_PORT = [string]$port
    $env:CATDESK_RECOVERY_FIXTURE_MARKER = $marker
    $script:BootstrapSeams = @{}
    $canonical = [pscustomobject]@{ Path = $canonicalPath; Sha256 = $canonicalHash }
    $localMcp = [pscustomobject]@{ Port = $port }

    $stale = Start-Process -FilePath $canonicalPath -ArgumentList '--catdesk-daemon' -WorkingDirectory $workspace -WindowStyle Hidden -PassThru
    Start-Sleep -Milliseconds 400
    Require ($null -ne (Get-Process -Id $stale.Id -ErrorAction SilentlyContinue)) 'stale fixture process remains alive'
    Require ($null -eq (Get-LoopbackCatDeskListener -Port $port -Canonical $canonical)) 'stale fixture has no listener'
    Require (-not (Test-Path -LiteralPath $marker)) 'stale process consumed the one-shot marker'
    $candidates = @()
    $candidateDeadline = [DateTime]::UtcNow.AddSeconds(5)
    while ([DateTime]::UtcNow -lt $candidateDeadline) {
        $candidates = @(Get-CatDeskDaemonProcessCandidates -Canonical $canonical)
        if (@($candidates | Where-Object { $_.MatchesCanonical }).Count -eq 1) { break }
        Start-Sleep -Milliseconds 100
    }
    $fixtureCandidates = @($candidates | Where-Object { $_.MatchesCanonical })
    Require ($fixtureCandidates.Count -eq 1 -and [int]$fixtureCandidates[0].Pid -eq [int]$stale.Id) 'production candidate enumeration finds the stale fixture beyond concurrent CatDesk rows'
    # The active host owns another independently verified CatDesk daemon. Model
    # the disposable fixture's isolated process set after proving the real
    # enumerator found this exact row; production keeps the real foreign-row
    # ambiguity refusal and never uses this seam.
    $script:BootstrapSeams['DaemonProcesses'] = { param($ignoredCanonical) @($fixtureCandidates) }.GetNewClosure()

    Require (Stop-StaleCanonicalCatDeskDaemonForRecovery -Canonical $canonical -LocalMcp $localMcp) 'production stale recovery retires the exact fixture process'
    Require ($null -eq (Get-Process -Id $stale.Id -ErrorAction SilentlyContinue)) 'stale fixture was stopped before replacement'

    $replacement = Start-Process -FilePath $canonicalPath -ArgumentList '--catdesk-daemon' -WorkingDirectory $workspace -WindowStyle Hidden -PassThru
    $listener = $null
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    while ([DateTime]::UtcNow -lt $deadline) {
        $listener = Get-LoopbackCatDeskListener -Port $port -Canonical $canonical
        if ($null -ne $listener) { break }
        Start-Sleep -Milliseconds 100
    }
    Require ($null -ne $listener -and $listener.MatchesCanonical -and [int]$listener.Pid -eq [int]$replacement.Id) 'replacement fixture binds one verified canonical listener'
    $changedListener=[pscustomobject]@{Pid=$listener.Pid;CreationTimeUtc='2000-01-01T00:00:00.0000000Z';Path=$listener.Path;Sha256=$listener.Sha256;MatchesCanonical=$listener.MatchesCanonical};try{$null=Get-CatDeskListenerProcessInstanceForRecovery -Candidate $changedListener;throw 'changed real listener process instance was accepted'}catch{if($_.Exception.Message -match 'was accepted'){throw};if($_.Exception.Message -ne 'CatDesk listener process instance changed before recovery mutation'){throw}};Require ($null -ne (Get-Process -Id $replacement.Id -ErrorAction SilentlyContinue)) 'changed listener identity did not terminate the real fixture'
    Require (Stop-CatDeskListenerProcessForRecovery -LocalMcp $localMcp -Canonical $canonical -Candidate $listener -FailureMessage 'real pinned listener did not stop') 'real listener stop used exact pinned process authority';Require ($null -eq (Get-Process -Id $replacement.Id -ErrorAction SilentlyContinue)) 'real pinned listener fixture was not stopped';$replacement=$null
} finally {
    if ($null -ne $replacement) { Stop-ExactFixtureProcess -ProcessId ([int]$replacement.Id) -ExpectedPath $canonicalPath }
    if ($null -ne $stale) { Stop-ExactFixtureProcess -ProcessId ([int]$stale.Id) -ExpectedPath $canonicalPath }
    if ($hadPort) { $env:CATDESK_RECOVERY_FIXTURE_PORT = $priorPort } else { Remove-Item Env:CATDESK_RECOVERY_FIXTURE_PORT -ErrorAction SilentlyContinue }
    if ($hadMarker) { $env:CATDESK_RECOVERY_FIXTURE_MARKER = $priorMarker } else { Remove-Item Env:CATDESK_RECOVERY_FIXTURE_MARKER -ErrorAction SilentlyContinue }
    if (Test-Path -LiteralPath $workspace) { Remove-Item -LiteralPath $workspace -Recurse -Force }
}

Write-Output 'stale canonical daemon recovery integration test passed'
