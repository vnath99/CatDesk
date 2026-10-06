param([Parameter(Mandatory)][string]$InstallDirectory)
$ErrorActionPreference = 'Stop'
$hostExe = Join-Path $InstallDirectory 'CatDeskWakeHost.exe'
$guiExe = Join-Path $InstallDirectory 'CatDeskBinagotchy.exe'
Add-Type -TypeDefinition @'
using System; using System.Text; using System.Runtime.InteropServices;
public static class WakeGuiTest {
 public delegate bool EnumProc(IntPtr hwnd,IntPtr param);
 [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr hwnd,EnumProc callback,IntPtr param);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr hwnd,StringBuilder text,int count);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr hwnd,StringBuilder text,int count);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern IntPtr SendMessage(IntPtr hwnd,uint message,IntPtr wp,string text);
 [DllImport("user32.dll",EntryPoint="SendMessageW")] public static extern IntPtr Click(IntPtr hwnd,uint message,IntPtr wp,IntPtr lp);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
}
'@
function Read-WakeStatus { & $hostExe status | ConvertFrom-Json }
function Wait-Wake([scriptblock]$Check) {
    for ($attempt=0; $attempt -lt 80; $attempt++) { if (& $Check) { return }; Start-Sleep -Milliseconds 100 }
    throw 'Live GUI condition timed out'
}
function Get-WakeControls {
    $gui = @(Get-Process CatDeskBinagotchy -ErrorAction Stop)
    if ($gui.Count -ne 1) { throw 'Expected one GUI' }
    $children = [Collections.Generic.List[object]]::new()
    $callback = [WakeGuiTest+EnumProc]{param($window,$unused)
        $text=[Text.StringBuilder]::new(4096); $class=[Text.StringBuilder]::new(256)
        [void][WakeGuiTest]::GetWindowText($window,$text,4096); [void][WakeGuiTest]::GetClassName($window,$class,256)
        $children.Add([pscustomobject]@{Handle=$window;Text=$text.ToString();Class=$class.ToString()}); return $true
    }
    [void][WakeGuiTest]::EnumChildWindows($gui[0].MainWindowHandle,$callback,[IntPtr]::Zero)
    return $children
}
function Click-Wake([string]$Name) {
    $button = @(Get-WakeControls | Where-Object { $_.Text -eq $Name -and $_.Class -match 'BUTTON' })
    if ($button.Count -ne 1) { throw "Button unavailable: $Name" }
    [void][WakeGuiTest]::Click($button[0].Handle,0xF5,[IntPtr]::Zero,[IntPtr]::Zero)
}
$initial = Read-WakeStatus
if (Test-Path -LiteralPath (Join-Path $env:LOCALAPPDATA 'CatDeskWake\activation.json')) { throw 'Use an inactive development installation for this GUI acceptance script' }
$originalUrl = $initial.targets.catdesk.url
if (-not $originalUrl) { throw 'Set the intended target through the GUI first' }
$hostPid = $initial.pid
$checks = [Collections.Generic.List[string]]::new()
$controls = @(Get-WakeControls)
foreach ($requiredControl in @('Designated Chat URL','Apply/Update','Refresh readback','Start Wake','Pause Wake','Resume Wake','Stop Wake')) {
    if (-not ($controls | Where-Object { $_.Text -eq $requiredControl })) { throw "Combined Binagotchy control unavailable: $requiredControl" }
}
$checks.Add('Combined Binagotchy exposes persisted target editor and wake controls')
Click-Wake 'Pause Wake'; Wait-Wake { (Read-WakeStatus).host -eq 'PAUSED' }; $checks.Add('Pause button changes host to PAUSED')
Click-Wake 'Resume Wake'; Wait-Wake { (Read-WakeStatus).host -eq 'RUNNING' }; $checks.Add('Resume button changes host to RUNNING')
$guiPid = (Get-Process CatDeskBinagotchy).Id
Start-Process -FilePath $guiExe -ArgumentList '--catdesk-binagotchy-gui' -WorkingDirectory $InstallDirectory
Start-Sleep -Milliseconds 800
if (@(Get-Process CatDeskBinagotchy).Count -ne 1 -or (Get-Process CatDeskBinagotchy).Id -ne $guiPid) { throw 'Duplicate GUI instance' }
if ([WakeGuiTest]::GetForegroundWindow() -ne (Get-Process CatDeskBinagotchy).MainWindowHandle) { throw 'Duplicate launch did not focus existing GUI' }
$checks.Add('Duplicate launch focuses existing GUI PID')
[void](Get-Process -Id $guiPid).CloseMainWindow(); Wait-Wake { -not (Get-Process -Id $guiPid -ErrorAction SilentlyContinue) }
if ((Read-WakeStatus).pid -ne $hostPid) { throw 'Closing GUI affected host PID' }
Start-Process -FilePath $guiExe -ArgumentList '--catdesk-binagotchy-gui' -WorkingDirectory $InstallDirectory
Wait-Wake { @(Get-Process CatDeskBinagotchy -ErrorAction SilentlyContinue).Count -eq 1 }
if ((Read-WakeStatus).targets.catdesk.url -ne $originalUrl -or (Read-WakeStatus).pid -ne $hostPid) { throw 'Close/reopen did not preserve target and host PID' }
$checks.Add('Close/reopen preserves exact target and host PID')
$guiPid = (Get-Process CatDeskBinagotchy).Id
Stop-Process -Id $guiPid -Force
if ((Read-WakeStatus).pid -ne $hostPid) { throw 'GUI crash affected host PID' }
Start-Process -FilePath $guiExe -ArgumentList '--catdesk-binagotchy-gui' -WorkingDirectory $InstallDirectory
Wait-Wake { @(Get-Process CatDeskBinagotchy -ErrorAction SilentlyContinue).Count -eq 1 }
if ((Read-WakeStatus).targets.catdesk.url -ne $originalUrl -or (Read-WakeStatus).pid -ne $hostPid) { throw 'GUI crash/restart did not preserve host PID and target' }
$checks.Add('GUI crash/restart preserves host PID and target')
Click-Wake 'Stop Wake'; Wait-Wake { (Read-WakeStatus).host -eq 'STOPPED' }; $checks.Add('Stop button stops host')
Click-Wake 'Start Wake'; Wait-Wake { (Read-WakeStatus).host -eq 'RUNNING' }; $checks.Add('Start button starts host')
if ((Read-WakeStatus).targets.catdesk.url -ne $originalUrl) { throw 'Lifecycle changed target' }
[pscustomobject]@{Utc=[DateTime]::UtcNow.ToString('o');Checks=$checks;Target=$originalUrl;Generation=(Read-WakeStatus).targets.catdesk.generation;HostPid=(Read-WakeStatus).pid;GuiPid=(Get-Process CatDeskBinagotchy).Id;BrowserDispatchActivated=$false} | ConvertTo-Json -Depth 4
