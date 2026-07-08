<#
    Removes everything install.ps1 added - context menus, shortcuts, the PATH
    entry, and the installed binary. Pass -Purge to also delete your saved
    config and sessions.
#>
#Requires -Version 5.1
[CmdletBinding()]
param([switch]$Purge)

$ErrorActionPreference = 'Stop'
$InstallDir = Join-Path $env:LOCALAPPDATA 'Programs\plume'

function Info($m) { Write-Host "  $m" }
function Step($m) { Write-Host "`n$m" -ForegroundColor Cyan }

Step 'Removing context menus'
foreach ($sub in @('Directory', 'Directory\Background', '*')) {
    try {
        [Microsoft.Win32.Registry]::CurrentUser.DeleteSubKeyTree("Software\Classes\$sub\shell\plume", $false)
    } catch {}
}
Info 'Done.'

Step 'Removing shortcuts'
@(
    (Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\plume.lnk'),
    (Join-Path ([Environment]::GetFolderPath('Desktop')) 'plume.lnk')
) | ForEach-Object { if (Test-Path $_) { Remove-Item $_ -Force } }
Info 'Done.'

Step 'Removing PATH entry'
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($userPath) {
    $parts = @($userPath -split ';' | Where-Object { $_ -ne '' -and $_ -ne $InstallDir })
    [Environment]::SetEnvironmentVariable('Path', ($parts -join ';'), 'User')
}
Info 'Done.'

Step "Removing $InstallDir"
if (Test-Path $InstallDir) { Remove-Item $InstallDir -Recurse -Force }
Info 'Done.'

if ($Purge) {
    Step 'Purging config and sessions'
    @(
        (Join-Path $env:APPDATA 'plume'),      # config lives in %APPDATA%\plume
        (Join-Path $env:LOCALAPPDATA 'plume')  # sessions live in %LOCALAPPDATA%\plume
    ) | ForEach-Object { if (Test-Path $_) { Remove-Item $_ -Recurse -Force } }
    Info 'Removed config and session data.'
}

Step 'Done.'
Write-Host 'plume has been uninstalled. Open a new terminal for the PATH change to take effect.'
