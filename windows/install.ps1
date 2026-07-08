<#
    plume Windows installer.

    Installs plume with cargo (into ~/.cargo/bin, the standard Rust location
    that is already on your PATH), then layers on the Windows extras - Start-menu
    and desktop shortcuts plus the Explorer "Open in plume" / "Edit with plume"
    context menus. Everything is per-user (no admin) and is undone by
    uninstall.ps1.

    If plume is already on your PATH (e.g. you ran cargo install yourself) the
    binary step is skipped and that copy is reused. Use -Reinstall to rebuild
    from this source tree.
#>
#Requires -Version 5.1
[CmdletBinding()]
param(
    [switch]$Reinstall,
    [switch]$NoShortcuts,
    [switch]$NoContextMenu
)

$ErrorActionPreference = 'Stop'

$ScriptDir  = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot   = Split-Path -Parent $ScriptDir
$SupportDir = Join-Path $env:LOCALAPPDATA 'Programs\plume'
$Launcher   = Join-Path $SupportDir 'plume-open.js'
$Wscript    = Join-Path $env:SystemRoot 'System32\wscript.exe'

function Info($m) { Write-Host "  $m" }
function Step($m) { Write-Host "`n$m" -ForegroundColor Cyan }

# 1. Install the binary with cargo, or reuse one already on PATH.
Step 'Installing the binary'
$existing = Get-Command plume.exe -ErrorAction SilentlyContinue
if ($existing -and -not $Reinstall) {
    $Exe = $existing.Source
    Info "Reusing the plume already on your PATH: $Exe"
    Info 'Pass -Reinstall to rebuild it from this source tree.'
} else {
    $cargoBin = if ($env:CARGO_HOME) { Join-Path $env:CARGO_HOME 'bin' }
                else { Join-Path $env:USERPROFILE '.cargo\bin' }
    $env:CARGO_TARGET_DIR = Join-Path $RepoRoot 'target'   # reuse existing build artifacts
    Push-Location $RepoRoot
    try { & cargo install --path . --force } finally { Pop-Location }
    if ($LASTEXITCODE -ne 0) { throw 'cargo install failed.' }
    $Exe = Join-Path $cargoBin 'plume.exe'
    if (-not (Test-Path $Exe)) { throw "cargo did not produce $Exe" }
    Info "Installed to $Exe"
}

# 2. Make sure the binary's folder is on the user PATH (cargo's usually is).
Step 'Checking PATH'
$ExeDir = Split-Path -Parent $Exe
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$parts = @()
if ($userPath) { $parts = @($userPath -split ';' | Where-Object { $_ -ne '' }) }
if ($parts -contains $ExeDir) {
    Info "Already on PATH ($ExeDir)."
} else {
    [Environment]::SetEnvironmentVariable('Path', ((@($parts) + $ExeDir) -join ';'), 'User')
    Info "Added $ExeDir. Open a new terminal to run 'plume'."
}

# 3. Install the flash-free launcher the shortcuts and menus call.
Step 'Installing launcher'
New-Item -ItemType Directory -Force -Path $SupportDir | Out-Null
Copy-Item (Join-Path $ScriptDir 'plume-open.js') $Launcher -Force
Info "Copied plume-open.js to $SupportDir."

# 4. Shortcuts (Start menu + desktop).
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
