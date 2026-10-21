# Keyless (ADC-only) launcher for OpenAI Codex CLI routed to Gemini 3.8 & Gemini 4.0 (Windows PowerShell)
$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$ProjectDir = Split-Path -Parent $ScriptDir
$RouterPort = if ($env:ROUTER_PORT) { $env:ROUTER_PORT } else { "4000" }
$RouterUrl = "http://127.0.0.1:$RouterPort"
$PrimaryModel = if ($env:GEMINI_PRIMARY_MODEL) { $env:GEMINI_PRIMARY_MODEL } else { "gemini-3.8-flash" }
$ReasoningEffort = if ($env:GEMINI_REASONING_EFFORT) { $env:GEMINI_REASONING_EFFORT } else { "medium" }
$PrintEnvOnly = $false
$PrintTomlOnly = $false
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
    } elseif ($arg -like "--effort=*") {
        $ReasoningEffort = $arg.Substring(9)
    } elseif ($arg -eq "--env") {
        $PrintEnvOnly = $true
    } elseif ($arg -eq "--config-toml") {
        $PrintTomlOnly = $true
    } elseif ($arg -eq "--serve") {
        $StartOnly = $true
    } else {
        $PassThroughArgs = $args[$i..($args.Count - 1)]
        break
    }
}

if ($PrintEnvOnly) {
    Write-Output @"
Remove-Item Env:OPENAI_API_KEY -ErrorAction SilentlyContinue
Remove-Item Env:GEMINI_API_KEY -ErrorAction SilentlyContinue
Remove-Item Env:GOOGLE_API_KEY -ErrorAction SilentlyContinue
`$env:OPENAI_BASE_URL = "$RouterUrl/v1"
`$env:GEMINI_PRIMARY_MODEL = "$PrimaryModel"
`$env:GEMINI_REASONING_EFFORT = "$ReasoningEffort"
"@
    exit 0
}

if ($PrintTomlOnly) {
    Write-Output @"
model_provider = "gemini_adc"
model = "$PrimaryModel"
model_reasoning_effort = "$ReasoningEffort"

[model_providers.gemini_adc]
name = "Gemini ADC Router (Vertex AI)"
base_url = "$RouterUrl/v1"
wire_api = "responses"
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
    Write-Host "[codex-gemini] Starting Rust ADC router on $RouterUrl..." -ForegroundColor Cyan
    $bin = Resolve-RouterBin
    Start-Process -FilePath $bin -ArgumentList @("--port", $RouterPort) -WindowStyle Hidden
    for ($retry = 0; $retry -lt 30; $retry++) {
        if (Test-RouterHealth) { break }
        Start-Sleep -Milliseconds 200
    }
}

Remove-Item Env:OPENAI_API_KEY -ErrorAction SilentlyContinue
Remove-Item Env:GEMINI_API_KEY -ErrorAction SilentlyContinue
Remove-Item Env:GOOGLE_API_KEY -ErrorAction SilentlyContinue

if (Get-Command "codex" -ErrorAction SilentlyContinue) {
    & codex `
        -c 'model_provider="gemini_adc"' `
        -c "model=`"$PrimaryModel`"" `
        -c "model_reasoning_effort=`"$ReasoningEffort`"" `
        -c 'model_providers.gemini_adc.name="Gemini ADC Router (Vertex AI)"' `
        -c "model_providers.gemini_adc.base_url=`"$RouterUrl/v1`"" `
        -c 'model_providers.gemini_adc.wire_api="responses"' `
        @PassThroughArgs
    exit $LASTEXITCODE
} else {
    Write-Host "[codex-gemini] Router is live at $RouterUrl/v1 (model=$PrimaryModel)." -ForegroundColor Green
    Write-Host "[codex-gemini] 'codex' CLI not found on PATH." -ForegroundColor Yellow
}
