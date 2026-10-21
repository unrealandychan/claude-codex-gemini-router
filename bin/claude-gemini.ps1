# Keyless (ADC-only) launcher for Claude Code routed to Gemini 3.8 & Gemini 4.0 (Windows PowerShell)
$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$ProjectDir = Split-Path -Parent $ScriptDir
$RouterPort = if ($env:ROUTER_PORT) { $env:ROUTER_PORT } else { "4000" }
$RouterUrl = "http://127.0.0.1:$RouterPort"
$PrimaryModel = if ($env:GEMINI_PRIMARY_MODEL) { $env:GEMINI_PRIMARY_MODEL } else { "gemini-3.8-flash" }
$PrintEnvOnly = $false
$StartOnly = $false
$PassThroughArgs = @()

for ($i = 0; $i -lt $args.Count; $i++) {
    $arg = $args[$i]
    if ($arg -eq "--3.8" -or $arg -eq "--flash") {
        $PrimaryModel = "gemini-3.8-flash"
    } elseif ($arg -eq "--4.0" -or $arg -eq "--argon" -or $arg -eq "--pro") {
        $PrimaryModel = "gemini-4-argon"
    } elseif ($arg -like "--model=*") {
        $PrimaryModel = $arg.Substring(8)
    } elseif ($arg -eq "--env") {
        $PrintEnvOnly = $true
    } elseif ($arg -eq "--serve") {
        $StartOnly = $true
    } else {
        $PassThroughArgs = $args[$i..($args.Count - 1)]
        break
    }
}

if ($PrintEnvOnly) {
    Write-Output @"
Remove-Item Env:ANTHROPIC_API_KEY -ErrorAction SilentlyContinue
Remove-Item Env:GEMINI_API_KEY -ErrorAction SilentlyContinue
Remove-Item Env:GOOGLE_API_KEY -ErrorAction SilentlyContinue
`$env:CLAUDE_CODE_USE_VERTEX = "0"
`$env:ANTHROPIC_BASE_URL = "$RouterUrl"
`$env:ANTHROPIC_AUTH_TOKEN = "adc-keyless"
`$env:ANTHROPIC_MODEL = "$PrimaryModel[1m]"
`$env:ANTHROPIC_SMALL_FAST_MODEL = "claude-haiku-4-5"
`$env:ANTHROPIC_DEFAULT_HAIKU_MODEL = "claude-haiku-4-5"
`$env:ANTHROPIC_DEFAULT_SONNET_MODEL = "$PrimaryModel[1m]"
`$env:ANTHROPIC_DEFAULT_OPUS_MODEL = "$PrimaryModel[1m]"
`$env:DISABLE_PROMPT_CACHING = "1"
"@
    exit 0
}

function Resolve-RouterBin {
    $candidates = @(
        (Join-Path $ProjectDir "target\release\claude-gemini-router.exe"),
        (Join-Path $env:USERPROFILE ".local\bin\claude-gemini-router.exe"),
        (Join-Path $env:USERPROFILE ".cargo\bin\claude-gemini-router.exe")
    )
    foreach ($c in $candidates) {
        if (Test-Path $c) { return $c }
    }
    $cmd = Get-Command "claude-gemini-router.exe" -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
    throw "claude-gemini-router.exe not found. Run scripts\install-windows.ps1 or cargo build --release."
}

if ($StartOnly) {
    $bin = Resolve-RouterBin
    & $bin --port $RouterPort @PassThroughArgs
    exit $LASTEXITCODE
}

function Test-RouterHealth {
    try {
        $res = Invoke-RestMethod -Uri "$RouterUrl/health" -TimeoutSec 1 -ErrorAction Stop
        return ($res.status -eq "ok")
    } catch {
        return $false
    }
}

if (-not (Test-RouterHealth)) {
    Write-Host "[claude-gemini] Starting Rust ADC router on $RouterUrl..." -ForegroundColor Cyan
    $bin = Resolve-RouterBin
    Start-Process -FilePath $bin -ArgumentList @("--port", $RouterPort) -WindowStyle Hidden
    for ($retry = 0; $retry -lt 30; $retry++) {
        if (Test-RouterHealth) { break }
        Start-Sleep -Milliseconds 200
    }
}

Remove-Item Env:ANTHROPIC_API_KEY -ErrorAction SilentlyContinue
Remove-Item Env:GEMINI_API_KEY -ErrorAction SilentlyContinue
Remove-Item Env:GOOGLE_API_KEY -ErrorAction SilentlyContinue
$env:CLAUDE_CODE_USE_VERTEX = "0"
$env:ANTHROPIC_BASE_URL = $RouterUrl
$env:ANTHROPIC_AUTH_TOKEN = "adc-keyless"
$env:ANTHROPIC_MODEL = "$PrimaryModel[1m]"
$env:ANTHROPIC_SMALL_FAST_MODEL = "claude-haiku-4-5"
$env:ANTHROPIC_DEFAULT_HAIKU_MODEL = "claude-haiku-4-5"
$env:ANTHROPIC_DEFAULT_SONNET_MODEL = "$PrimaryModel[1m]"
$env:ANTHROPIC_DEFAULT_OPUS_MODEL = "$PrimaryModel[1m]"
$env:DISABLE_PROMPT_CACHING = "1"

$SettingsOverride = (@{
    env = @{
        CLAUDE_CODE_USE_VERTEX = "0"
        ANTHROPIC_BASE_URL = $RouterUrl
        ANTHROPIC_AUTH_TOKEN = "adc-keyless"
        ANTHROPIC_MODEL = "$PrimaryModel[1m]"
        ANTHROPIC_SMALL_FAST_MODEL = "claude-haiku-4-5"
        ANTHROPIC_DEFAULT_HAIKU_MODEL = "claude-haiku-4-5"
        ANTHROPIC_DEFAULT_SONNET_MODEL = "$PrimaryModel[1m]"
        ANTHROPIC_DEFAULT_OPUS_MODEL = "$PrimaryModel[1m]"
        DISABLE_PROMPT_CACHING = "1"
    }
} | ConvertTo-Json -Compress)

if (Get-Command "claude" -ErrorAction SilentlyContinue) {
    & claude --settings $SettingsOverride @PassThroughArgs
    exit $LASTEXITCODE
} else {
    Write-Host "[claude-gemini] Router is live at $RouterUrl (model=$PrimaryModel)." -ForegroundColor Green
    Write-Host "[claude-gemini] 'claude' CLI not found on PATH." -ForegroundColor Yellow
}
