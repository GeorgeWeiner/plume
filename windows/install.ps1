<#
    plume Windows installer.

    Installs the built binary to %LOCALAPPDATA%\Programs\plume, adds it to the
    user PATH, creates Start-menu and desktop shortcuts, and registers the
    Explorer "Open in plume" / "Edit with plume" context menus. Every change is
    per-user (no admin needed) and is undone by uninstall.ps1.

    Switches opt out of individual pieces.
#>
#Requires -Version 5.1
[CmdletBinding()]
param(
    [switch]$NoBuild,
    [switch]$NoPath,
    [switch]$NoShortcuts,
    [switch]$NoContextMenu
)

$ErrorActionPreference = 'Stop'

$ScriptDir  = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot   = Split-Path -Parent $ScriptDir
$InstallDir = Join-Path $env:LOCALAPPDATA 'Programs\plume'
$Exe        = Join-Path $InstallDir 'plume.exe'
$Launcher   = Join-Path $InstallDir 'plume-open.js'
$Wscript    = Join-Path $env:SystemRoot 'System32\wscript.exe'

function Info($m) { Write-Host "  $m" }
function Step($m) { Write-Host "`n$m" -ForegroundColor Cyan }

# 1. Build the release binary, unless told to reuse an existing one.
Step 'Building release binary'
if ($NoBuild) {
    Info 'Skipped (-NoBuild).'
} else {
    Push-Location $RepoRoot
    try { & cargo build --release } finally { Pop-Location }
    if ($LASTEXITCODE -ne 0) { throw 'cargo build failed.' }
}
$BuiltExe = Join-Path $RepoRoot 'target\release\plume.exe'
if (-not (Test-Path $BuiltExe)) {
    throw "Binary not found at $BuiltExe. Build first with: cargo build --release"
}

# 2. Copy the binary and launcher into place.
Step "Installing to $InstallDir"
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
Copy-Item $BuiltExe $Exe -Force
Copy-Item (Join-Path $ScriptDir 'plume-open.js') $Launcher -Force
Info 'Copied plume.exe and plume-open.js.'

# 3. Add the install dir to the user PATH.
Step 'Updating PATH'
if ($NoPath) {
    Info 'Skipped (-NoPath).'
} else {
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    $parts = @()
    if ($userPath) { $parts = @($userPath -split ';' | Where-Object { $_ -ne '' }) }
    if ($parts -contains $InstallDir) {
        Info 'Already on PATH.'
    } else {
        $newPath = (@($parts) + $InstallDir) -join ';'
        [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
        Info "Added to your user PATH. Open a new terminal to run 'plume'."
    }
}

# 4. Shortcuts (Start menu + desktop), launched via the flash-free helper.
Step 'Creating shortcuts'
if ($NoShortcuts) {
    Info 'Skipped (-NoShortcuts).'
} else {
    $wsh = New-Object -ComObject WScript.Shell
    $links = @(
        (Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\plume.lnk'),
        (Join-Path ([Environment]::GetFolderPath('Desktop')) 'plume.lnk')
    )
    foreach ($lnk in $links) {
        $sc = $wsh.CreateShortcut($lnk)
        $sc.TargetPath       = $Wscript
        $sc.Arguments        = ('"{0}" -r' -f $Launcher)
        $sc.IconLocation     = $Exe
        $sc.WorkingDirectory = $env:USERPROFILE
        $sc.Description       = 'plume - a feather-light terminal IDE'
        $sc.Save()
    }
    Info 'Start-menu and desktop shortcuts created (resume last project).'
}

# 5. Explorer context menus, per-user under HKCU\Software\Classes.
Step 'Registering context menus'
if ($NoContextMenu) {
    Info 'Skipped (-NoContextMenu).'
} else {
    function Set-Verb($subkey, $label, $macro) {
        $base = "Software\Classes\$subkey\shell\plume"
        $k = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey($base)
        $k.SetValue('', $label)
        $k.SetValue('Icon', $Exe)
        $c = $k.CreateSubKey('command')
        $c.SetValue('', ('"{0}" "{1}" "{2}"' -f $Wscript, $Launcher, $macro))
        $c.Close(); $k.Close()
    }
    Set-Verb 'Directory'            'Open in plume'   '%V'
    Set-Verb 'Directory\Background' 'Open in plume'   '%V'
    Set-Verb '*'                    'Edit with plume' '%1'
    Info 'Right-click a folder for "Open in plume", a file for "Edit with plume".'
}

Step 'Done.'
Write-Host "plume is installed. Try right-clicking a folder, or run 'plume' in a new terminal."
