param(
    [switch]$Plan,
    [switch]$Install,
    [switch]$Update,
    [switch]$Rollback,
    [switch]$Status
)

$ErrorActionPreference = "Stop"

if (-not ($Plan -or $Install -or $Update -or $Rollback -or $Status)) {
    $Plan = $true
}

$RepoApi = "https://api.github.com/repos/openai/tunnel-client/releases"
$InstallRoot = Join-Path ([Environment]::GetFolderPath("UserProfile")) ".catdesk\tools\tunnel-client"
$VersionsDir = Join-Path $InstallRoot "versions"
$CurrentDir = Join-Path $InstallRoot "current"
$PreviousDir = Join-Path $InstallRoot "previous"
$MetadataPath = Join-Path $InstallRoot "install-metadata.json"
$UserProfile = [Environment]::GetFolderPath("UserProfile")

function Redact-UserPath {
    param([string]$Path)
    if (-not $Path) { return $null }
    return $Path.Replace($UserProfile, "%USERPROFILE%")
}

function Get-OsArchToken {
    $arch = [Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLowerInvariant()
    switch ($arch) {
        "x64" { "amd64"; break }
        "arm64" { "arm64"; break }
        default { throw "Unsupported Windows architecture for tunnel-client: $arch" }
    }
}

function Get-LatestStableRelease {
    $releases = Invoke-RestMethod -Uri $RepoApi -Headers @{ "User-Agent" = "CatDesk tunnel-client installer" }
    $stable = $releases | Where-Object { -not $_.draft -and -not $_.prerelease } | Select-Object -First 1
    if (-not $stable) { throw "No stable openai/tunnel-client release found" }
    return $stable
}

function Assert-OfficialUrl {
    param([string]$Url)
    $uri = [Uri]$Url
    if ($uri.Scheme -ne "https" -or $uri.Host -ne "github.com" -or -not $uri.AbsolutePath.StartsWith("/openai/tunnel-client/releases/download/")) {
        throw "Release asset URL is not an official openai/tunnel-client GitHub release URL"
    }
}

function Select-ReleaseAssets {
    param($Release)
    $arch = Get-OsArchToken
    $assetNamePattern = "windows-$arch.zip"
    $archive = $Release.assets | Where-Object { $_.name.ToLowerInvariant().EndsWith($assetNamePattern) } | Select-Object -First 1
    $checksum = $Release.assets | Where-Object { $_.name -eq "SHA256SUMS.txt" } | Select-Object -First 1
    if (-not $archive) { throw "No Windows $arch tunnel-client archive found in latest stable release" }
    if (-not $checksum) { throw "No SHA256SUMS.txt asset found; refusing unattended install" }
    if ($archive.size -le 0 -or $archive.size -gt 134217728) { throw "Archive size is outside CatDesk bounds" }
    Assert-OfficialUrl $archive.browser_download_url
    Assert-OfficialUrl $checksum.browser_download_url
    [pscustomobject][ordered]@{
        Architecture = $arch
        Archive = $archive
        Checksum = $checksum
    }
}

function Get-ChecksumForAsset {
    param([string]$Text, [string]$AssetName)
    foreach ($line in ($Text -split "`r?`n")) {
        $trimmed = $line.Trim()
        if (-not $trimmed -or $trimmed.StartsWith("#")) { continue }
        $parts = $trimmed -split "\s+"
        if ($parts.Count -lt 2) { continue }
        $digest = $parts[0].ToLowerInvariant()
        $name = $parts[-1].TrimStart("*")
        if ($name.EndsWith($AssetName) -and $digest -match "^[0-9a-f]{64}$") {
            return $digest
        }
    }
    throw "No matching SHA-256 entry found for $AssetName"
}

function Get-CurrentBinary {
    $path = Join-Path $CurrentDir "tunnel-client.exe"
    if (Test-Path -LiteralPath $path -PathType Leaf) { return $path }
    $legacy = Join-Path $InstallRoot "tunnel-client.exe"
    if (Test-Path -LiteralPath $legacy -PathType Leaf) { return $legacy }
    return $null
}

function Get-BinaryInfo {
    param([string]$Path)
    if (-not $Path -or -not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        return $null
    }
    $version = $null
    try { $version = (& $Path --version 2>$null) -join "`n" } catch { $version = "version check failed" }
    [pscustomobject][ordered]@{
        Path = Redact-UserPath $Path
        Version = $version
        Sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash.ToLowerInvariant()
    }
}

function Write-Metadata {
    param($Release, $Assets, [string]$InstalledPath, [string]$ArchiveSha)
    New-Item -ItemType Directory -Force -Path $InstallRoot | Out-Null
    [pscustomobject][ordered]@{
        installedAt = (Get-Date).ToUniversalTime().ToString("o")
        releaseTag = $Release.tag_name
        architecture = $Assets.Architecture
        archiveName = $Assets.Archive.name
        archiveSha256 = $ArchiveSha
        installedPath = Redact-UserPath $InstalledPath
        source = "https://github.com/openai/tunnel-client/releases"
        pathModified = $false
        adminRequired = $false
    } | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $MetadataPath -Encoding UTF8
}

function Install-Release {
    param($Release, $Assets)
    $tempRoot = Join-Path ([IO.Path]::GetTempPath()) ("catdesk-tunnel-client-install-" + [Guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Force -Path $tempRoot | Out-Null
    try {
        $archivePath = Join-Path $tempRoot $Assets.Archive.name
        $checksumPath = Join-Path $tempRoot "SHA256SUMS.txt"
        Invoke-WebRequest -Uri $Assets.Archive.browser_download_url -OutFile $archivePath -Headers @{ "User-Agent" = "CatDesk tunnel-client installer" }
        Invoke-WebRequest -Uri $Assets.Checksum.browser_download_url -OutFile $checksumPath -Headers @{ "User-Agent" = "CatDesk tunnel-client installer" }
        $checksums = Get-Content -LiteralPath $checksumPath -Raw
        $expected = Get-ChecksumForAsset -Text $checksums -AssetName $Assets.Archive.name
        $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $archivePath).Hash.ToLowerInvariant()
        if ($actual -ne $expected) { throw "Downloaded tunnel-client archive checksum did not match SHA256SUMS.txt" }

        $extractDir = Join-Path $tempRoot "extract"
        Expand-Archive -LiteralPath $archivePath -DestinationPath $extractDir
        $candidate = Get-ChildItem -LiteralPath $extractDir -Recurse -File -Filter "tunnel-client.exe" | Select-Object -First 1
        if (-not $candidate) { throw "Archive did not contain tunnel-client.exe" }
        $version = (& $candidate.FullName --version 2>$null) -join "`n"
        $rootHelp = (& $candidate.FullName --help 2>$null) -join "`n"
        $runtimeHelp = (& $candidate.FullName runtimes --help 2>$null) -join "`n"
        $connectHelp = (& $candidate.FullName runtimes connect --help 2>$null) -join "`n"
        $statusHelp = (& $candidate.FullName runtimes status --help 2>$null) -join "`n"
        if ($rootHelp -notmatch "OpenAI MCP control plane") { throw "Extracted executable help did not identify the official tunnel-client" }
        if ($connectHelp -notmatch "--mcp-server-url") { throw "Extracted tunnel-client does not expose HTTP MCP setup support" }
        if ($runtimeHelp -notmatch "connect" -or $runtimeHelp -notmatch "status" -or $runtimeHelp -notmatch "stop" -or $runtimeHelp -notmatch "rm") {
            throw "Extracted tunnel-client does not expose required runtime supervision commands"
        }
        if ($statusHelp -notmatch "--json" -and $runtimeHelp -notmatch "--json") {
            throw "Extracted tunnel-client does not expose JSON runtime status"
        }

        $versionDir = Join-Path $VersionsDir $Release.tag_name
        $versionBinary = Join-Path $versionDir "tunnel-client.exe"
        $stagedDir = Join-Path $InstallRoot ("staged-" + [Guid]::NewGuid().ToString("N"))
        New-Item -ItemType Directory -Force -Path $stagedDir | Out-Null
        Copy-Item -LiteralPath $candidate.FullName -Destination (Join-Path $stagedDir "tunnel-client.exe")
        New-Item -ItemType Directory -Force -Path $VersionsDir | Out-Null
        if (Test-Path -LiteralPath $versionDir) {
            Remove-Item -LiteralPath $versionDir -Recurse -Force
        }
        Move-Item -LiteralPath $stagedDir -Destination $versionDir

        if (Test-Path -LiteralPath $PreviousDir) {
            Remove-Item -LiteralPath $PreviousDir -Recurse -Force
        }
        if (Test-Path -LiteralPath $CurrentDir) {
            Move-Item -LiteralPath $CurrentDir -Destination $PreviousDir
        }
        New-Item -ItemType Directory -Force -Path $CurrentDir | Out-Null
        Copy-Item -LiteralPath $versionBinary -Destination (Join-Path $CurrentDir "tunnel-client.exe")
        Write-Metadata -Release $Release -Assets $Assets -InstalledPath (Join-Path $CurrentDir "tunnel-client.exe") -ArchiveSha $actual
        return [pscustomobject][ordered]@{
            Installed = $true
            Release = $Release.tag_name
            Architecture = $Assets.Architecture
            InstalledPath = Redact-UserPath (Join-Path $CurrentDir "tunnel-client.exe")
            Version = $version
            ArchiveSha256 = $actual
            BinarySha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $CurrentDir "tunnel-client.exe")).Hash.ToLowerInvariant()
            RollbackAvailable = (Test-Path -LiteralPath (Join-Path $PreviousDir "tunnel-client.exe") -PathType Leaf)
        }
    } finally {
        if (Test-Path -LiteralPath $tempRoot) {
            Remove-Item -LiteralPath $tempRoot -Recurse -Force
        }
    }
}

function Rollback-Client {
    $previous = Join-Path $PreviousDir "tunnel-client.exe"
    if (-not (Test-Path -LiteralPath $previous -PathType Leaf)) {
        throw "No rollback tunnel-client binary is available"
    }
    $rollbackStaging = Join-Path $InstallRoot ("rollback-" + [Guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Force -Path $rollbackStaging | Out-Null
    Copy-Item -LiteralPath $previous -Destination (Join-Path $rollbackStaging "tunnel-client.exe")
    if (Test-Path -LiteralPath $CurrentDir) {
        Remove-Item -LiteralPath $CurrentDir -Recurse -Force
    }
    Move-Item -LiteralPath $rollbackStaging -Destination $CurrentDir
    [pscustomobject][ordered]@{
        RolledBack = $true
        Current = Get-BinaryInfo (Join-Path $CurrentDir "tunnel-client.exe")
    }
}

$release = Get-LatestStableRelease
$assets = Select-ReleaseAssets -Release $release
$current = Get-BinaryInfo (Get-CurrentBinary)

if ($Plan) {
    [pscustomobject][ordered]@{
        Mode = "plan"
        LatestStable = $release.tag_name
        Architecture = $assets.Architecture
        Archive = $assets.Archive.name
        ChecksumAsset = $assets.Checksum.name
        InstallRoot = Redact-UserPath $InstallRoot
        Current = $current
        MutationsPerformed = @()
    } | ConvertTo-Json -Depth 8
    exit 0
}

if ($Status) {
    [pscustomobject][ordered]@{
        Mode = "status"
        LatestStable = $release.tag_name
        InstallRoot = Redact-UserPath $InstallRoot
        Current = $current
        PreviousAvailable = (Test-Path -LiteralPath (Join-Path $PreviousDir "tunnel-client.exe") -PathType Leaf)
        MetadataPath = if (Test-Path -LiteralPath $MetadataPath) { Redact-UserPath $MetadataPath } else { $null }
        MutationsPerformed = @()
    } | ConvertTo-Json -Depth 8
    exit 0
}

if ($Rollback) {
    Rollback-Client | ConvertTo-Json -Depth 8
    exit 0
}

if ($Install -or $Update) {
    Install-Release -Release $release -Assets $assets | ConvertTo-Json -Depth 8
    exit 0
}
