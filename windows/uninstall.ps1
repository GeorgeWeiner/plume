<#
    Removes everything install.ps1 added - context menus, shortcuts, the
    launcher, and the cargo-installed binary. The ~/.cargo/bin PATH entry is
    left alone, since that is shared Rust infrastructure. Pass -Purge to also
    delete your config and sessions.
#>
#Requires -Version 5.1
[CmdletBinding()]
param([switch]$Purge)

$ErrorActionPreference = 'Stop'
$SupportDir = Join-Path $env:LOCALAPPDATA 'Programs\plume'

function Info($m) { Write-Host "  $m" }
function Step($m) { Write-Host "`n$m" -ForegroundColor Cyan }

Step 'Removing context menus'
foreach ($sub in @('Directory', 'Directory\Background', '*')) {
    try { [Microsoft.Win32.Registry]::CurrentUser.DeleteSubKeyTree("Software\Classes\$sub\shell\plume", $false) } catch {}
}
Info 'Done.'

Step 'Removing shortcuts'
@(
    (Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\plume.lnk'),
    (Join-Path ([Environment]::GetFolderPath('Desktop')) 'plume.lnk')
) | ForEach-Object { if (Test-Path $_) { Remove-Item $_ -Force } }
Info 'Done.'

Step 'Removing launcher'
if (Test-Path $SupportDir) { Remove-Item $SupportDir -Recurse -Force }
Info 'Done.'

Step 'Uninstalling the binary'
$cargo = Get-Command cargo -ErrorAction SilentlyContinue
if ($cargo) {
    & cargo uninstall plume
    if ($LASTEXITCODE -eq 0) { Info 'Removed via cargo uninstall.' }
    else { Info 'Not a cargo-installed binary or already gone - nothing to do.' }
} else {
    Info 'cargo not found - remove plume.exe by hand if you copied it somewhere.'
}

if ($Purge) {
    Step 'Purging config and sessions'
    @(
        (Join-Path $env:APPDATA 'plume'),      # config lives in %APPDATA%\plume
        (Join-Path $env:LOCALAPPDATA 'plume')  # sessions live in %LOCALAPPDATA%\plume
    ) | ForEach-Object { if (Test-Path $_) { Remove-Item $_ -Recurse -Force } }
    Info 'Removed config and session data.'
}

Step 'Done.'
Write-Host 'plume has been uninstalled.'
