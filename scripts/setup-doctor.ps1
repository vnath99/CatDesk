param(
    [string]$Workspace = (Get-Location).Path,
    [string]$Model = "qwen3.6:35b-a3b",
    [string]$McpHost = "127.0.0.1",
    [int]$McpPort = 38765,
    [string]$TransportMode = "",
    [string]$TunnelClientPath = "",
    [string]$OpenAiTunnelProfile = ""
)

$ErrorActionPreference = "Continue"

function Command-Candidates {
    param([string]$Name)
    $paths = New-Object System.Collections.Generic.List[string]
    $cmd = Get-Command $Name -ErrorAction SilentlyContinue
    if ($null -ne $cmd) {
        $paths.Add($cmd.Source)
    }
    $homeDir = [Environment]::GetFolderPath("UserProfile")
    $localAppData = [Environment]::GetFolderPath("LocalApplicationData")
    $programFiles = [Environment]::GetFolderPath("ProgramFiles")
    if ($Name -eq "cargo" -or $Name -eq "rustc") {
        $paths.Add((Join-Path $homeDir ".cargo\bin\$Name.exe"))
    }
    if ($Name -eq "git") {
        $paths.Add((Join-Path $programFiles "Git\cmd\git.exe"))
        $paths.Add((Join-Path $programFiles "Git\bin\git.exe"))
    }
    if ($Name -eq "ollama") {
        $paths.Add((Join-Path $localAppData "Programs\Ollama\ollama.exe"))
    }
    if ($Name -eq "catdesk") {
        $paths.Add((Join-Path (Get-Location).Path "target\release\catdesk.exe"))
        $paths.Add((Join-Path (Get-Location).Path "target\debug\catdesk.exe"))
    }
    if ($Name -eq "tunnel-client") {
        if ($TunnelClientPath) {
            $paths.Add($TunnelClientPath)
        }
        $paths.Add((Join-Path $homeDir ".catdesk\tools\tunnel-client\tunnel-client.exe"))
        $paths.Add((Join-Path $homeDir ".catdesk\tools\tunnel-client\tunnel-client"))
    }
    $paths | Select-Object -Unique
}

function Command-Info {
    param([string]$Name)
    $path = $null
    foreach ($candidate in (Command-Candidates $Name)) {
        if ($candidate -and (Test-Path -LiteralPath $candidate -PathType Leaf)) {
            $path = $candidate
            break
        }
    }
    if ($null -eq $path) {
        return [pscustomobject][ordered]@{
            Name = $Name
            Found = $false
            Path = $null
            Version = $null
        }
    }
    $version = "executable found"
    try {
        if ($Name -in @("cargo", "rustc", "git", "ollama")) {
            $version = (& $path --version 2>$null) -join "`n"
        }
        if ($Name -eq "tunnel-client") {
            $version = (& $path --version 2>$null) -join "`n"
            if (-not $version) {
                $version = (& $path --help 2>$null | Select-Object -First 1) -join "`n"
            }
        }
    } catch {
        $version = "version check failed"
    }
    return [pscustomobject][ordered]@{
        Name = $Name
        Found = $true
        Path = $path
        Version = $version
    }
}

function Optional-Provider {
    param([string]$Name)
    [pscustomobject][ordered]@{
        Name = $Name
        Configured = [bool][Environment]::GetEnvironmentVariable($Name)
        Value = "<redacted>"
    }
}

$workspacePath = Resolve-Path -LiteralPath $Workspace -ErrorAction SilentlyContinue
$cargo = Command-Info "cargo"
$rustc = Command-Info "rustc"
$git = Command-Info "git"
$ollama = Command-Info "ollama"
$catdesk = Command-Info "catdesk"
$tunnelClient = Command-Info "tunnel-client"

$ollamaModels = @()
if ($ollama.Found) {
    try {
        $ollamaModels = (& $ollama.Path list 2>$null) -split "`r?`n"
    } catch {
        $ollamaModels = @("ollama list failed")
    }
}

$modelFound = $false
foreach ($line in $ollamaModels) {
    if ($line -match [regex]::Escape($Model)) {
        $modelFound = $true
    }
}

$report = [pscustomobject][ordered]@{
    Workspace = if ($workspacePath) { $workspacePath.Path } else { $Workspace }
    CatDesk = $catdesk
    Rust = [pscustomobject][ordered]@{
        Cargo = $cargo
        Rustc = $rustc
    }
    Git = $git
    Ollama = $ollama
    SelectedQwenModel = [pscustomobject][ordered]@{
        Name = $Model
        Present = $modelFound
    }
    JournalLocation = if ($workspacePath) {
        Join-Path $workspacePath.Path ".catdesk\delegated\journal"
    } else {
        "(workspace missing)"
    }
    LoopbackMcpPosture = [pscustomobject][ordered]@{
        Host = $McpHost
        Port = $McpPort
        LoopbackOnly = ($McpHost -eq "127.0.0.1" -or $McpHost -eq "localhost" -or $McpHost -eq "::1")
    }
    Transport = [pscustomobject][ordered]@{
        Mode = if ($TransportMode) { $TransportMode } else { "(not supplied)" }
        TunnelClient = $tunnelClient
        OpenAiTunnelProfile = [pscustomobject][ordered]@{
            Supplied = [bool]$OpenAiTunnelProfile
            Value = if ($OpenAiTunnelProfile) { "<redacted>" } else { $null }
        }
        RuntimeCredential = [pscustomobject][ordered]@{
            Name = "CONTROL_PLANE_API_KEY"
            Present = [bool][Environment]::GetEnvironmentVariable("CONTROL_PLANE_API_KEY")
            Value = "<redacted>"
        }
        LiveOpenAiTunnel = "not checked by setup-doctor"
    }
    OptionalProviders = @(
        Optional-Provider "CATDESK_REMOTE_API_KEY"
        Optional-Provider "OPENAI_API_KEY"
        Optional-Provider "ANTHROPIC_API_KEY"
        Optional-Provider "DEEPSEEK_API_KEY"
    )
}

$report | ConvertTo-Json -Depth 8
