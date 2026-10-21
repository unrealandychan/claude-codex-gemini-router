# Native Windows Installer for Claude & Codex -> Gemini ADC Router + System Tray Notification Bar App
$ErrorActionPreference = "Stop"

$RepoDir = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$BinDir = Join-Path $env:USERPROFILE ".local\bin"
$ConfigDir = Join-Path $env:APPDATA "gemini-router"
$StartupDir = [Environment]::GetFolderPath("Startup")
$ShortcutPath = Join-Path $StartupDir "Gemini ADC Router Tray.lnk"

Write-Host "==> [1/5] Building release Rust binaries (claude-gemini-router.exe, gemini-router-ctl.exe, gemini-router-tray.exe)..." -ForegroundColor Cyan
Push-Location $RepoDir
try {
    cargo build --release `
        --bin claude-gemini-router `
        --bin gemini-router-ctl `
        --bin omarchy-gemini-router-ctl `
        --bin gemini-router-tray
} finally {
    Pop-Location
}

Write-Host "==> [2/5] Installing binaries and wrappers to $BinDir..." -ForegroundColor Cyan
New-Item -ItemType Directory -Force -Path $BinDir | Out-Null
New-Item -ItemType Directory -Force -Path $ConfigDir | Out-Null

Stop-Process -Name "gemini-router-tray" -Force -ErrorAction SilentlyContinue
Stop-Process -Name "claude-gemini-router" -Force -ErrorAction SilentlyContinue

Copy-Item -Force (Join-Path $RepoDir "target\release\claude-gemini-router.exe") (Join-Path $BinDir "claude-gemini-router.exe")
Copy-Item -Force (Join-Path $RepoDir "target\release\gemini-router-ctl.exe") (Join-Path $BinDir "gemini-router-ctl.exe")
Copy-Item -Force (Join-Path $RepoDir "target\release\omarchy-gemini-router-ctl.exe") (Join-Path $BinDir "omarchy-gemini-router-ctl.exe")
Copy-Item -Force (Join-Path $RepoDir "target\release\gemini-router-tray.exe") (Join-Path $BinDir "gemini-router-tray.exe")

Copy-Item -Force (Join-Path $RepoDir "bin\claude-gemini.ps1") (Join-Path $BinDir "claude-gemini.ps1")
Copy-Item -Force (Join-Path $RepoDir "bin\claude-gemini.cmd") (Join-Path $BinDir "claude-gemini.cmd")
Copy-Item -Force (Join-Path $RepoDir "bin\codex-gemini.ps1") (Join-Path $BinDir "codex-gemini.ps1")
Copy-Item -Force (Join-Path $RepoDir "bin\codex-gemini.cmd") (Join-Path $BinDir "codex-gemini.cmd")

Write-Host "==> [3/5] Ensuring $BinDir is in your Windows User PATH..." -ForegroundColor Cyan
$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if (-not $UserPath) { $UserPath = "" }
if ($UserPath.Split(";") -notcontains $BinDir) {
    $NewPath = if ($UserPath) { "$BinDir;$UserPath" } else { $BinDir }
    [Environment]::SetEnvironmentVariable("Path", $NewPath, "User")
    $env:Path = "$BinDir;$env:Path"
}

Write-Host "==> [4/5] Creating Windows Startup shortcut for System Tray Notification Bar App..." -ForegroundColor Cyan
$WshShell = New-Object -ComObject WScript.Shell
$Shortcut = $WshShell.CreateShortcut($ShortcutPath)
$Shortcut.TargetPath = (Join-Path $BinDir "gemini-router-tray.exe")
$Shortcut.Arguments = "--autostart-router"
$Shortcut.WorkingDirectory = $BinDir
$Shortcut.Description = "Gemini ADC Router Notification Bar Tray Application"
$Shortcut.Save()

Write-Host "==> [5/5] Launching Gemini ADC Router in Windows Notification Area..." -ForegroundColor Cyan
Start-Process -FilePath (Join-Path $BinDir "gemini-router-tray.exe") -ArgumentList @("--autostart-router")

Write-Host ""
Write-Host "==================================================================" -ForegroundColor Green
Write-Host " Gemini ADC Router + Windows System Tray App Installed!" -ForegroundColor Green
Write-Host "==================================================================" -ForegroundColor Green
Write-Host " - Tray App:      $BinDir\gemini-router-tray.exe"
Write-Host " - Router Daemon: http://127.0.0.1:4000/health"
Write-Host " - Browser UI:    http://127.0.0.1:4000/ui"
Write-Host " - CLI Control:   gemini-router-ctl status"
Write-Host "==================================================================" -ForegroundColor Green
