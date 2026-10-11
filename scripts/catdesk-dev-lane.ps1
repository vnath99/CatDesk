[CmdletBinding()]
param(
    [Parameter(Mandatory = $true, Position = 0)]
    [ValidateSet('status', 'build', 'verify')]
    [string]$Command
)

# T-0478 R1: source-only developer loop, deliberately NOT a runtime launcher.
# This facade never installs CatDesk, starts a daemon, changes production
# configuration, owns the official Secure MCP tunnel, or submits Wake events.
# Source build/test execution remains on the normal operator's Rust toolchain;
# a separately reviewed isolated instance is required before adding 'run'.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Assert-RegularFile([string]$Path) {
    $item = Get-Item -LiteralPath $Path -Force -ErrorAction Stop
    if (-not ($item -is [System.IO.FileInfo]) -or
        ($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw 'DEVELOPMENT_INPUT_UNSAFE'
    }
    return $item
}

function Invoke-FixedCommand([string]$Program, [string[]]$Arguments) {
    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw 'DEVELOPMENT_COMMAND_FAILED'
    }
}

$root = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$rootItem = Get-Item -LiteralPath $root -Force -ErrorAction Stop
if (-not ($rootItem -is [System.IO.DirectoryInfo]) -or
    ($rootItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
    throw 'DEVELOPMENT_ROOT_UNSAFE'
}
$null = Assert-RegularFile (Join-Path $root 'Cargo.toml')
$null = Assert-RegularFile (Join-Path $root 'Cargo.lock')
$null = Assert-RegularFile (Join-Path $root 'src\main.rs')

# No caller-provided cwd/path, process ID, URL, release image or credentials.
# Git's toplevel must agree with this fixed source root before any work.
Push-Location -LiteralPath $root
try {
    $gitRoot = (& git rev-parse --show-toplevel 2>$null)
    if ($LASTEXITCODE -ne 0 -or -not $gitRoot -or
        -not [string]::Equals(
            [System.IO.Path]::GetFullPath(([string]$gitRoot).Trim().Replace('/', '\')),
            $root,
            [System.StringComparison]::OrdinalIgnoreCase
        )) { throw 'DEVELOPMENT_GIT_ROOT_MISMATCH' }

    $head = (& git rev-parse --verify HEAD 2>$null)
    if ($LASTEXITCODE -ne 0 -or [string]$head -notmatch '^[0-9a-fA-F]{40}$') {
        throw 'DEVELOPMENT_COMMIT_ID_UNAVAILABLE'
    }
    $head = ([string]$head).ToLowerInvariant()
    $binary = Join-Path $root 'target\debug\catdesk.exe'
    $digest = $null
    if (Test-Path -LiteralPath $binary -PathType Leaf) {
        $null = Assert-RegularFile $binary
        $digest = (Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash.ToLowerInvariant()
    }
    if ($Command -eq 'build') {
        Invoke-FixedCommand 'cargo' @('build', '--locked', '--offline', '--bin', 'catdesk')
        $null = Assert-RegularFile $binary
        $digest = (Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash.ToLowerInvariant()
    }
    elseif ($Command -eq 'verify') {
        Invoke-FixedCommand 'cargo' @('fmt', '--all', '--', '--check')
        Invoke-FixedCommand 'cargo' @('clippy', '--locked', '--offline', '--workspace', '--all-targets', '--all-features', '--', '-D', 'warnings')
        Invoke-FixedCommand 'cargo' @('test', '--locked', '--offline', '--bin', 'catdesk')
    }

    $dirty = & git status --porcelain
    if ($LASTEXITCODE -ne 0) { throw 'DEVELOPMENT_GIT_STATUS_UNAVAILABLE' }
    [pscustomobject][ordered]@{
        schemaVersion = 1
        mode = 'DEVELOPMENT_SOURCE_ONLY'
        operation = $Command.ToUpperInvariant()
        sourceCommit = $head
        workingTreeClean = (@($dirty | Where-Object { -not [string]::IsNullOrWhiteSpace([string]$_) }).Count -eq 0)
        debugImageSha256 = $digest
        productionInstallationChanged = $false
        daemonOrBrowserStarted = $false
        isolatedRuntimeReady = $false
    } | ConvertTo-Json -Compress
} finally {
    Pop-Location
}
