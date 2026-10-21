# Native Windows Uninstaller for Claude & Codex -> Gemini ADC Router
$ErrorActionPreference = "Continue"

$BinDir = Join-Path $env:USERPROFILE ".local\bin"
$StartupDir = [Environment]::GetFolderPath("Startup")
$ShortcutPath = Join-Path $StartupDir "Gemini ADC Router Tray.lnk"
$CtlBin = Join-Path $BinDir "gemini-router-ctl.exe"

Write-Host "==> [1/3] Restoring direct Claude Code & OpenAI Codex configurations..." -ForegroundColor Cyan
if (Test-Path $CtlBin) {
    & $CtlBin disable --target both | Out-Null
    & $CtlBin stop-router | Out-Null
}

Write-Host "==> [2/3] Stopping background processes and removing Startup shortcut..." -ForegroundColor Cyan
Stop-Process -Name "gemini-router-tray" -Force -ErrorAction SilentlyContinue
Stop-Process -Name "claude-gemini-router" -Force -ErrorAction SilentlyContinue
if (Test-Path $ShortcutPath) {
    Remove-Item -Force $ShortcutPath
}

Write-Host "==> [3/3] Removing binaries from $BinDir..." -ForegroundColor Cyan
$Files = @(
    "claude-gemini-router.exe",
    "gemini-router-ctl.exe",
    "omarchy-gemini-router-ctl.exe",
    "gemini-router-tray.exe",
    "claude-gemini.ps1",
    "claude-gemini.cmd",
    "codex-gemini.ps1",
    "codex-gemini.cmd"
)
foreach ($f in $Files) {
    $p = Join-Path $BinDir $f
    if (Test-Path $p) { Remove-Item -Force $p }
}

Write-Host "Uninstalled Gemini ADC Router from Windows." -ForegroundColor Green
