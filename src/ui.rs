use serde_json::{Map, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayVisualState {
    Off,
    SingleRouted,
    BothRouted,
    Alert,
    Busy,
}

impl TrayVisualState {
    pub fn from_status(status: &Map<String, Value>) -> Self {
        let any_enabled = status
            .get("any_enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let both_enabled = status
            .get("both_enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let router_running = status
            .get("router_running")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let (adc_valid, adc_alert) = status
            .get("adc")
            .and_then(|v| v.as_object())
            .map(|adc| {
                let valid = adc
                    .get("token_valid")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let alert = adc
                    .get("auth_alert")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                (valid, alert)
            })
            .unwrap_or((false, false));

        if any_enabled && (!router_running || !adc_valid || adc_alert) {
            Self::Alert
        } else if both_enabled {
            Self::BothRouted
        } else if any_enabled {
            Self::SingleRouted
        } else {
            Self::Off
        }
    }

    pub fn rgba_color(self) -> [u8; 4] {
        match self {
            Self::Off => [0xcd, 0xd6, 0xf4, 0xff],          // #cdd6f4 (Catppuccin foreground)
            Self::SingleRouted => [0x89, 0xb4, 0xfa, 0xff], // #89b4fa (Catppuccin blue accent)
            Self::BothRouted => [0xa6, 0xe3, 0xa1, 0xff],   // #a6e3a1 (Catppuccin green)
            Self::Alert => [0xf3, 0x8b, 0xa8, 0xff],        // #f38ba8 (Catppuccin red)
            Self::Busy => [0xf9, 0xe2, 0xaf, 0xff],         // #f9e2af (Catppuccin yellow)
        }
    }
}

/// Generates a crisp 32x32 RGBA robot/router icon for the macOS Menu Bar & Windows System Tray.
pub fn generate_tray_icon_rgba(state: TrayVisualState) -> (Vec<u8>, u32, u32) {
    let width = 32u32;
    let height = 32u32;
    let mut rgba = vec![0u8; (width * height * 4) as usize];
    let fg = state.rgba_color();
    let bg = [0x1e, 0x1e, 0x2e, 0xf2]; // Dark Catppuccin Mocha badge plate

    let mut set_px = |x: i32, y: i32, color: [u8; 4]| {
        if x >= 0 && x < width as i32 && y >= 0 && y < height as i32 {
            let idx = ((y as u32 * width + x as u32) * 4) as usize;
            rgba[idx] = color[0];
            rgba[idx + 1] = color[1];
            rgba[idx + 2] = color[2];
            rgba[idx + 3] = color[3];
        }
    };

    // Rounded background pill (x: 2..30, y: 2..30)
    for y in 2..30 {
        for x in 2..30 {
            let dx = if x < 7 {
                7 - x
            } else if x > 24 {
                x - 24
            } else {
                0
            };
            let dy = if y < 7 {
                7 - y
            } else if y > 24 {
                y - 24
            } else {
                0
            };
            if dx * dx + dy * dy <= 25 {
                let is_border = dx * dx + dy * dy >= 16
                    || x == 2
                    || x == 29
                    || y == 2
                    || y == 29;
                if is_border {
                    set_px(x, y, fg);
                } else {
                    set_px(x, y, bg);
                }
            }
        }
    }

    // Robot antenna (top center)
    for y in 5..9 {
        set_px(15, y, fg);
        set_px(16, y, fg);
    }
    for x in 14..18 {
        set_px(x, 5, fg);
    }

    // Robot head outline (x: 8..24, y: 9..23)
    for x in 8..24 {
        set_px(x, 9, fg);
        set_px(x, 10, fg);
        set_px(x, 21, fg);
        set_px(x, 22, fg);
    }
    for y in 9..23 {
        set_px(8, y, fg);
        set_px(9, y, fg);
        set_px(22, y, fg);
        set_px(23, y, fg);
    }

    // Robot side ears
    for y in 13..19 {
        set_px(6, y, fg);
        set_px(7, y, fg);
        set_px(24, y, fg);
        set_px(25, y, fg);
    }

    // Robot eyes
    for y in 13..16 {
        for x in 12..15 {
            set_px(x, y, fg);
        }
        for x in 17..20 {
            set_px(x, y, fg);
        }
    }

    // Robot mouth / status bar
    for x in 12..20 {
        set_px(x, 18, fg);
        set_px(x, 19, fg);
    }

    // Status indicator dot at bottom right
    let dot_color = match state {
        TrayVisualState::Off => [0x6c, 0x70, 0x86, 0xff],
        _ => fg,
    };
    for y in 23..29 {
        for x in 23..29 {
            let dx = x - 26;
            let dy = y - 26;
            if dx * dx + dy * dy <= 9 {
                set_px(x, y, dot_color);
            }
        }
    }

    (rgba, width, height)
}

pub fn format_tray_tooltip(status: &Map<String, Value>) -> String {
    let status_label = status
        .get("status_label")
        .and_then(|v| v.as_str())
        .unwrap_or("Direct (Routing Off)");
    let primary_model = status
        .get("primary_model")
        .and_then(|v| v.as_str())
        .unwrap_or("gemini-3.8-flash");
    let reasoning_effort = status
        .get("reasoning_effort")
        .and_then(|v| v.as_str())
        .unwrap_or("medium");
    let claude_enabled = status
        .get("claude_enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let codex_enabled = status
        .get("codex_enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let router_running = status
        .get("router_running")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let router_url = status
        .get("router_url")
        .and_then(|v| v.as_str())
        .unwrap_or("http://127.0.0.1:4000");

    let (adc_valid, adc_project) = status
        .get("adc")
        .and_then(|v| v.as_object())
        .map(|adc| {
            let valid = adc
                .get("token_valid")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let proj = adc
                .get("project_id")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            (valid, proj.to_string())
        })
        .unwrap_or((false, "unknown".to_string()));

    let (total_reqs, total_tokens) = status
        .get("stats")
        .and_then(|v| v.as_object())
        .map(|s| {
            let reqs = s.get("total_requests").and_then(|v| v.as_u64()).unwrap_or(0);
            let toks = s.get("total_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
            (reqs, toks)
        })
        .unwrap_or((0, 0));

    let compact_tokens = if total_tokens >= 1_000_000 {
        format!("{:.1}M", total_tokens as f64 / 1_000_000.0)
    } else if total_tokens >= 1_000 {
        format!("{:.1}k", total_tokens as f64 / 1_000.0)
    } else {
        total_tokens.to_string()
    };

    format!(
        "Gemini ADC Router\n───────────────────────────\nMode: {}\nModel: {} ({})\nClaude Code: {}\nOpenAI Codex: {}\nRouter Daemon: {}\nVertex AI ADC: {}\nSession Stats: {} reqs · {} tokens\n───────────────────────────\nLeft-click: Open Control Panel\nRight-click: Quick Menu & Actions",
        status_label,
        primary_model,
        reasoning_effort,
        if claude_enabled { "Routed (/v1/messages)" } else { "Direct" },
        if codex_enabled { "Routed (/v1/responses)" } else { "Direct" },
        if router_running { format!("Online ({})", router_url) } else { "Stopped".to_string() },
        if adc_valid { format!("Valid ({})", adc_project) } else { "Auth required (401/403)".to_string() },
        total_reqs,
        compact_tokens
    )
}

/// Pixel-faithful replica of `omarchy-plugin/plugin/GeminiRouter.qml` for the macOS Menu Bar,
/// Windows Notification Bar (System Tray), and HTTP `/ui` endpoint.
pub const CONTROL_PANEL_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8" />
<meta name="viewport" content="width=device-width, initial-scale=1.0" />
<title>Gemini ADC Router</title>
<style>
  :root {
    --bg-popup: #1e1e2e;
    --bg-surface: rgba(205, 214, 244, 0.04);
    --border-surface: rgba(205, 214, 244, 0.10);
    --border-subtle: rgba(205, 214, 244, 0.12);
    --fg: #cdd6f4;
    --muted: #a6adc8;
    --accent: #89b4fa;
    --green: #a6e3a1;
    --red: #f38ba8;
    --yellow: #f9e2af;
    --font-sans: -apple-system, BlinkMacSystemFont, "Segoe UI", "Inter", "JetBrainsMono Nerd Font", Roboto, sans-serif;
    --font-mono: "JetBrainsMono Nerd Font", "SF Mono", "Cascadia Mono", "Consolas", monospace;
  }

  * {
    box-sizing: border-box;
    user-select: none;
    -webkit-user-select: none;
  }

  html, body {
    margin: 0;
    padding: 0;
    background: var(--bg-popup);
    color: var(--fg);
    font-family: var(--font-sans);
    font-size: 12.5px;
    line-height: 1.38;
    overflow-x: hidden;
  }

  .popup-card {
    width: 100%;
    max-width: 460px;
    margin: 0 auto;
    padding: 14px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    background: var(--bg-popup);
    min-height: 100vh;
  }

  /* ──────────────── Header ──────────────── */
  .header-row {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
  }

  .robot-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    color: var(--accent);
    width: 20px;
    height: 20px;
    flex-shrink: 0;
  }

  .header-title {
    font-size: 15px;
    font-weight: 700;
    color: var(--fg);
    flex: 1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .status-pill {
    height: 22px;
    padding: 0 10px;
    border-radius: 11px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    font-size: 11.5px;
    font-weight: 600;
    border: 1px solid var(--muted);
    background: rgba(205, 214, 244, 0.08);
    color: var(--muted);
    white-space: nowrap;
    transition: all 0.15s ease;
  }

  .status-pill.active {
    background: rgba(166, 227, 161, 0.20);
    border-color: var(--green);
    color: var(--green);
  }

  .status-pill.alert {
    background: rgba(243, 139, 168, 0.20);
    border-color: var(--red);
    color: var(--red);
  }

  .icon-btn {
    background: rgba(205, 214, 244, 0.06);
    border: 1px solid var(--border-surface);
    color: var(--fg);
    border-radius: 6px;
    padding: 4px 8px;
    cursor: pointer;
    font-size: 12px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    transition: all 0.14s ease;
  }

  .icon-btn:hover:not(:disabled) {
    background: rgba(137, 180, 250, 0.16);
    border-color: var(--accent);
    color: var(--accent);
  }

  /* ──────────────── Notification Banner ──────────────── */
  .notify-banner {
    display: none;
    width: 100%;
    padding: 6px 10px;
    border-radius: 6px;
    background: rgba(137, 180, 250, 0.15);
    border: 1px solid var(--accent);
    color: var(--accent);
    font-size: 12px;
    text-align: center;
  }

  .notify-banner.visible {
    display: block;
  }

  /* ──────────────── 401/403 Alert Banner ──────────────── */
  .auth-alert-card {
    display: none;
    width: 100%;
    padding: 10px 12px;
    border-radius: 6px;
    background: rgba(243, 139, 168, 0.14);
    border: 1px solid var(--red);
    flex-direction: column;
    gap: 6px;
  }

  .auth-alert-card.visible {
    display: flex;
  }

  .auth-alert-title {
    font-size: 12px;
    font-weight: 700;
    color: var(--red);
  }

  .auth-alert-desc {
    font-size: 11.5px;
    color: var(--fg);
    word-break: break-word;
  }

  /* ──────────────── Separator ──────────────── */
  .panel-separator {
    width: 100%;
    height: 1px;
    background: var(--border-surface);
    margin: 1px 0;
  }

  /* ──────────────── BorderSurface Cards ──────────────── */
  .border-surface {
    width: 100%;
    border-radius: 6px;
    background: var(--bg-surface);
    border: 1px solid var(--border-surface);
    padding: 10px 12px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .row-layout {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
  }

  .row-layout.tight {
    gap: 6px;
  }

  .col-layout {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex: 1;
    min-width: 0;
  }

  .client-title {
    font-weight: 700;
    color: var(--fg);
    font-size: 12px;
  }

  .client-sub {
    font-size: 11.5px;
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .client-sub.routed {
    color: var(--green);
  }

  .section-label {
    font-weight: 700;
    color: var(--muted);
    font-size: 11.5px;
    flex: 1;
  }

  .section-hint {
    font-size: 11px;
    color: var(--muted);
  }

  .section-hint.hotswap {
    color: var(--green);
  }

  /* ──────────────── Buttons ──────────────── */
  .btn {
    height: 28px;
    padding: 0 11px;
    border-radius: 6px;
    border: 1px solid rgba(205, 214, 244, 0.16);
    background: rgba(205, 214, 244, 0.05);
    color: var(--fg);
    font-family: var(--font-sans);
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    white-space: nowrap;
    transition: all 0.14s ease;
  }

  .btn.tall {
    height: 30px;
  }

  .btn.compact {
    height: 26px;
    padding: 0 9px;
    font-size: 11.5px;
  }

  .btn.fill {
    flex: 1;
  }

  .btn:hover:not(:disabled) {
    background: rgba(205, 214, 244, 0.11);
    border-color: rgba(205, 214, 244, 0.28);
  }

  .btn:disabled {
    opacity: 0.42;
    cursor: not-allowed;
  }

  .btn.selected-accent {
    background: rgba(137, 180, 250, 0.20);
    border-color: var(--accent);
    color: var(--accent);
  }

  .btn.selected-green {
    background: rgba(166, 227, 161, 0.20);
    border-color: var(--green);
    color: var(--green);
  }

  .btn.selected-red {
    background: rgba(243, 139, 168, 0.22);
    border-color: var(--red);
    color: var(--red);
  }

  .btn.selected-neutral {
    background: rgba(205, 214, 244, 0.12);
    border-color: var(--muted);
    color: var(--fg);
  }

  /* ──────────────── Info Rows ──────────────── */
  .info-row {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    font-size: 12px;
  }

  .info-key {
    font-weight: 700;
    color: var(--muted);
    width: 105px;
    flex-shrink: 0;
  }

  .info-val {
    flex: 1;
    color: var(--fg);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .info-val.ok {
    color: var(--green);
  }

  .info-val.err {
    color: var(--red);
  }

  .info-val.muted {
    color: var(--muted);
  }

  /* ──────────────── Logs Console ──────────────── */
  .logs-box {
    display: none;
    width: 100%;
    height: 130px;
    border-radius: 4px;
    background: rgba(0, 0, 0, 0.35);
    border: 1px solid var(--border-subtle);
    padding: 6px 8px;
    overflow-y: auto;
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--muted);
    white-space: pre-wrap;
    word-break: break-all;
    user-select: text;
    -webkit-user-select: text;
  }

  .logs-box.visible {
    display: block;
  }
</style>
</head>
<body>
  <div class="popup-card">
    <!-- ──────────────── Header ──────────────── -->
    <div class="header-row">
      <span class="robot-icon" title="Gemini ADC Router">
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <rect x="3" y="11" width="18" height="10" rx="2"></rect>
          <circle cx="12" cy="5" r="2"></circle>
          <path d="M12 7v4"></path>
          <line x1="8" y1="16" x2="8" y2="16"></line>
          <line x1="16" y1="16" x2="16" y2="16"></line>
        </svg>
      </span>
      <div class="header-title">Gemini ADC Router</div>
      <div id="statusPill" class="status-pill">○ Direct Mode</div>
      <button id="refreshBtn" class="icon-btn" title="Refresh status" onclick="refreshStatus()">↻</button>
    </div>

    <!-- ──────────────── Notification Banner ──────────────── -->
    <div id="notifyBanner" class="notify-banner"></div>

    <!-- ──────────────── 401 / 403 Credential Reset Alert Banner ──────────────── -->
    <div id="authAlertCard" class="auth-alert-card">
      <div id="authAlertTitle" class="auth-alert-title">
        ⚠ Vertex AI Auth Issue (401 / 403) — Reset Credential
      </div>
      <div id="authAlertDesc" class="auth-alert-desc">
        Auto-heal could not refresh Vertex AI ADC credentials. Please reset your ADC login below.
      </div>
      <div class="row-layout">
        <button class="btn fill selected-red" onclick="runAction(['adc-login'], 'Launching gcloud ADC credential reset…')">
          Reset Credential (ADC Login)
        </button>
        <button class="btn" onclick="runAction(['refresh-auth'], 'Clearing token cache & re-checking ADC…')">
          Retry Auto-Heal
        </button>
      </div>
    </div>

    <div class="panel-separator"></div>

    <!-- ──────────────── Client Routing Toggles ──────────────── -->
    <div class="border-surface">
      <!-- Master Quick Action Row -->
      <div class="row-layout">
        <button id="btnRouteBoth" class="btn tall fill" onclick="runAction(['enable', '--target', 'both'], 'Routing Claude & Codex to Gemini…')">
          <span>󰚩</span>
          <span>Route Both to Gemini</span>
        </button>
        <button id="btnDisableAll" class="btn tall fill" onclick="runAction(['disable', '--target', 'both'], 'Restoring direct Claude & Codex config…')">
          <span>✕</span>
          <span>Disable All (Direct)</span>
        </button>
      </div>

      <!-- Claude Code Toggle Row -->
      <div class="row-layout">
        <div class="col-layout">
          <div class="client-title">Claude Code (claude)</div>
          <div id="claudeSub" class="client-sub">Direct (~/.claude/settings.json)</div>
        </div>
        <button id="btnToggleClaude" class="btn" onclick="toggleClaude()">
          OFF (Direct)
        </button>
      </div>

      <!-- OpenAI Codex Toggle Row -->
      <div class="row-layout">
        <div class="col-layout">
          <div class="client-title">OpenAI Codex (codex)</div>
          <div id="codexSub" class="client-sub">Direct (~/.codex/config.toml)</div>
        </div>
        <button id="btnToggleCodex" class="btn" onclick="toggleCodex()">
          OFF (Direct)
        </button>
      </div>
    </div>

    <!-- ──────────────── Model & Effort Selector Card ──────────────── -->
    <div class="border-surface">
      <div class="row-layout tight">
        <div class="section-label">Active Vertex AI Gemini Model</div>
        <div id="hotSwapLabel" class="section-hint">Haiku -> 3.8 · Hot-Swap Ready</div>
      </div>

      <div class="row-layout">
        <button id="btnModel38" class="btn fill selected-accent" onclick="runAction(['set-model', '--model', 'gemini-3.8-flash'], 'Switched model to gemini-3.8-flash')">
          Gemini 3.8 Flash
        </button>
        <button id="btnModel40" class="btn fill" onclick="runAction(['set-model', '--model', 'gemini-4-argon'], 'Switched model to gemini-4-argon')">
          Gemini 4.0 Argon
        </button>
      </div>

      <div class="section-label" style="margin-top: 2px;">Reasoning Effort (Thinking Level)</div>

      <div class="row-layout tight">
        <button id="btnEffortLow" class="btn compact fill" onclick="runAction(['set-model', '--effort', 'low'], 'Set reasoning effort to low')">LOW</button>
        <button id="btnEffortMedium" class="btn compact fill selected-accent" onclick="runAction(['set-model', '--effort', 'medium'], 'Set reasoning effort to medium')">MEDIUM</button>
        <button id="btnEffortHigh" class="btn compact fill" onclick="runAction(['set-model', '--effort', 'high'], 'Set reasoning effort to high')">HIGH</button>
        <button id="btnEffortXhigh" class="btn compact fill" onclick="runAction(['set-model', '--effort', 'xhigh'], 'Set reasoning effort to xhigh')">XHIGH</button>
      </div>
    </div>

    <!-- ──────────────── ADC & Daemon Overview Card ──────────────── -->
    <div class="border-surface">
      <div class="info-row">
        <div class="info-key">Router Daemon:</div>
        <div id="daemonVal" class="info-val err">Stopped (http://127.0.0.1:4000)</div>
      </div>

      <div class="info-row">
        <div class="info-key">Vertex AI ADC:</div>
        <div id="adcVal" class="info-val err">Needs Login · unknown (global)</div>
      </div>

      <div class="info-row">
        <div class="info-key">Session Stats:</div>
        <div id="statsVal" class="info-val muted">0 requests routed this session</div>
      </div>

      <div class="row-layout tight" style="margin-top: 2px;">
        <button id="btnStartRestart" class="btn compact fill" onclick="startOrRestartDaemon()">
          Start Router
        </button>
        <button id="btnStopDaemon" class="btn compact" onclick="runAction(['stop-router'], 'Stopping ADC router daemon…')">
          Stop
        </button>
        <button class="btn compact fill" onclick="runAction(['adc-login'], 'Launching gcloud ADC login…')">
          ADC Re-login
        </button>
        <button id="btnLogsToggle" class="btn compact" onclick="toggleLogsPanel()">
          Logs
        </button>
      </div>

      <div id="logsBox" class="logs-box">Loading logs…</div>
    </div>
  </div>

<script>
  const state = {
    routerRunning: false,
    routerUrl: "http://127.0.0.1:4000",
    claudeEnabled: false,
    codexEnabled: false,
    anyEnabled: false,
    bothEnabled: false,
    primaryModel: "gemini-3.8-flash",
    reasoningEffort: "medium",
    hotSwapActive: false,
    badgeText: "ADC Off",
    adcValid: false,
    adcAuthAlert: false,
    adcStatusCode: 0,
    adcProject: "unknown",
    adcLocation: "global",
    adcError: "",
    totalRequests: 0,
    claudeRequests: 0,
    codexRequests: 0,
    totalTokens: 0,
    lastLatencyMs: 0,
    isBusy: false,
    showLogsPanel: false,
  };

  let notifyTimeout = null;

  function showNotification(msg) {
    const banner = document.getElementById("notifyBanner");
    if (!msg) {
      banner.classList.remove("visible");
      banner.textContent = "";
      return;
    }
    banner.textContent = msg;
    banner.classList.add("visible");
    if (notifyTimeout) clearTimeout(notifyTimeout);
    notifyTimeout = setTimeout(() => {
      banner.classList.remove("visible");
    }, 4000);
  }

  function formatCompactNumber(n) {
    const v = Number(n) || 0;
    if (v >= 1000000) return (v / 1000000).toFixed(1) + "M";
    if (v >= 1000) return (v / 1000).toFixed(1) + "k";
    return String(v);
  }

  function setBusy(busy) {
    state.isBusy = busy;
    document.querySelectorAll("button.btn").forEach((b) => {
      if (busy) {
        b.disabled = true;
      }
    });
    if (!busy) {
      renderState();
    }
  }

  function applyStatusData(data) {
    if (!data) return;
    if (data.error) {
      showNotification(data.error);
    }
    if (data.router_running !== undefined) state.routerRunning = data.router_running === true;
    if (data.router_url) state.routerUrl = data.router_url;
    if (data.claude_enabled !== undefined) state.claudeEnabled = data.claude_enabled === true;
    if (data.codex_enabled !== undefined) state.codexEnabled = data.codex_enabled === true;
    if (data.any_enabled !== undefined) state.anyEnabled = data.any_enabled === true;
    if (data.both_enabled !== undefined) state.bothEnabled = data.both_enabled === true;
    if (data.primary_model) state.primaryModel = data.primary_model;
    if (data.reasoning_effort) state.reasoningEffort = data.reasoning_effort;
    if (data.hot_swap_active !== undefined) state.hotSwapActive = data.hot_swap_active === true;
    if (data.badge_text) state.badgeText = data.badge_text;
    if (data.adc) {
      state.adcValid = data.adc.token_valid === true;
      state.adcAuthAlert = data.adc.auth_alert === true;
      state.adcStatusCode = data.adc.auth_status_code || 0;
      state.adcProject = data.adc.project_id || "unknown";
      state.adcLocation = data.adc.location || "global";
      state.adcError = data.adc.error || "";
    }
    if (data.stats) {
      state.totalRequests = data.stats.total_requests || 0;
      state.claudeRequests = data.stats.claude_requests || 0;
      state.codexRequests = data.stats.codex_requests || 0;
      state.totalTokens = data.stats.total_tokens || 0;
      state.lastLatencyMs = data.stats.last_latency_ms || 0;
    }
    renderState();
  }

  function renderState() {
    // Header status pill
    const pill = document.getElementById("statusPill");
    pill.className = "status-pill";
    if (state.anyEnabled) {
      if (!state.adcValid || state.adcAuthAlert || !state.routerRunning) {
        pill.classList.add("alert");
      } else {
        pill.classList.add("active");
      }
      pill.textContent = "● " + state.badgeText;
    } else {
      pill.textContent = "○ Direct Mode";
    }

    // 401 / 403 Auth Alert Card
    const authCard = document.getElementById("authAlertCard");
    if (!state.adcValid || state.adcAuthAlert) {
      authCard.classList.add("visible");
      const codeStr = state.adcStatusCode > 0 ? ("HTTP " + state.adcStatusCode) : "401 / 403";
      document.getElementById("authAlertTitle").textContent =
        "⚠ Vertex AI Auth Issue (" + codeStr + ") — Reset Credential";
      document.getElementById("authAlertDesc").textContent =
        state.adcError || "Auto-heal could not refresh Vertex AI ADC credentials. Please reset your ADC login below.";
    } else {
      authCard.classList.remove("visible");
    }

    // Master Routing Buttons
    const btnRouteBoth = document.getElementById("btnRouteBoth");
    btnRouteBoth.disabled = state.isBusy;
    btnRouteBoth.className = "btn tall fill" + (state.bothEnabled ? " selected-green" : "");

    const btnDisableAll = document.getElementById("btnDisableAll");
    btnDisableAll.disabled = state.isBusy || !state.anyEnabled;
    btnDisableAll.className = "btn tall fill" + (!state.anyEnabled ? " selected-neutral" : "");

    // Claude Row
    const claudeSub = document.getElementById("claudeSub");
    claudeSub.textContent = state.claudeEnabled
      ? ("Routed -> " + state.primaryModel + " (/v1/messages)")
      : "Direct (~/.claude/settings.json)";
    claudeSub.className = "client-sub" + (state.claudeEnabled ? " routed" : "");

    const btnToggleClaude = document.getElementById("btnToggleClaude");
    btnToggleClaude.disabled = state.isBusy;
    btnToggleClaude.textContent = state.claudeEnabled ? "ON (Routed)" : "OFF (Direct)";
    btnToggleClaude.className = "btn" + (state.claudeEnabled ? " selected-green" : "");

    // Codex Row
    const codexSub = document.getElementById("codexSub");
    codexSub.textContent = state.codexEnabled
      ? ("Routed -> " + state.primaryModel + " (/v1/responses)")
      : "Direct (~/.codex/config.toml)";
    codexSub.className = "client-sub" + (state.codexEnabled ? " routed" : "");

    const btnToggleCodex = document.getElementById("btnToggleCodex");
    btnToggleCodex.disabled = state.isBusy;
    btnToggleCodex.textContent = state.codexEnabled ? "ON (Routed)" : "OFF (Direct)";
    btnToggleCodex.className = "btn" + (state.codexEnabled ? " selected-green" : "");

    // Model & Effort
    const hotSwapLabel = document.getElementById("hotSwapLabel");
    hotSwapLabel.className = "section-hint" + (state.hotSwapActive ? " hotswap" : "");

    const btnModel38 = document.getElementById("btnModel38");
    btnModel38.disabled = state.isBusy;
    btnModel38.className = "btn fill" + (state.primaryModel === "gemini-3.8-flash" ? " selected-accent" : "");

    const btnModel40 = document.getElementById("btnModel40");
    btnModel40.disabled = state.isBusy;
    btnModel40.className = "btn fill" + (state.primaryModel === "gemini-4-argon" ? " selected-accent" : "");

    const efforts = [
      ["low", "btnEffortLow"],
      ["medium", "btnEffortMedium"],
      ["high", "btnEffortHigh"],
      ["xhigh", "btnEffortXhigh"],
    ];
    for (const [lvl, id] of efforts) {
      const el = document.getElementById(id);
      el.disabled = state.isBusy;
      el.className = "btn compact fill" + (state.reasoningEffort === lvl ? " selected-accent" : "");
    }

    // Daemon & ADC Overview
    const daemonVal = document.getElementById("daemonVal");
    daemonVal.textContent = (state.routerRunning ? "Online" : "Stopped") + " (" + state.routerUrl + ")";
    daemonVal.className = "info-val " + (state.routerRunning ? "ok" : "err");

    const adcVal = document.getElementById("adcVal");
    adcVal.textContent =
      (state.adcValid ? "Token Valid" : "Needs Login") + " · " + state.adcProject + " (" + state.adcLocation + ")";
    adcVal.className = "info-val " + (state.adcValid ? "ok" : "err");

    const statsVal = document.getElementById("statsVal");
    if (state.totalRequests > 0) {
      let s = state.totalRequests + " reqs (C:" + state.claudeRequests + " X:" + state.codexRequests + ") · "
        + formatCompactNumber(state.totalTokens) + " tok";
      if (state.lastLatencyMs > 0) s += " · " + state.lastLatencyMs + "ms";
      statsVal.textContent = s;
      statsVal.className = "info-val";
    } else {
      statsVal.textContent = "0 requests routed this session";
      statsVal.className = "info-val muted";
    }

    const btnStartRestart = document.getElementById("btnStartRestart");
    btnStartRestart.disabled = state.isBusy;
    btnStartRestart.textContent = state.routerRunning ? "Restart" : "Start Router";

    const btnStopDaemon = document.getElementById("btnStopDaemon");
    btnStopDaemon.disabled = state.isBusy || !state.routerRunning;

    const btnLogsToggle = document.getElementById("btnLogsToggle");
    btnLogsToggle.textContent = state.showLogsPanel ? "Hide Logs" : "Logs";
  }

  async function apiRequest(path, method = "GET", bodyObj = null) {
    const opts = { method, headers: {} };
    if (bodyObj !== null) {
      opts.headers["Content-Type"] = "application/json";
      opts.body = JSON.stringify(bodyObj);
    }
    const resp = await fetch(path, opts);
    return await resp.json();
  }

  async function refreshStatus() {
    try {
      const data = await apiRequest("/api/status", "GET");
      applyStatusData(data);
    } catch (e) {
      // ignore transient error
    }
  }

  async function runAction(argsList, notifyText) {
    if (state.isBusy) return;
    setBusy(true);
    if (notifyText) showNotification(notifyText);
    try {
      const data = await apiRequest("/api/action", "POST", { args: argsList });
      applyStatusData(data);
    } catch (e) {
      showNotification("Action error: " + e.message);
    } finally {
      setBusy(false);
    }
  }

  function toggleClaude() {
    runAction(
      ["toggle", "--target", "claude"],
      state.claudeEnabled ? "Disabling Claude routing…" : "Routing Claude Code to Gemini…"
    );
  }

  function toggleCodex() {
    runAction(
      ["toggle", "--target", "codex"],
      state.codexEnabled ? "Disabling Codex routing…" : "Routing OpenAI Codex to Gemini…"
    );
  }

  function startOrRestartDaemon() {
    runAction(
      [state.routerRunning ? "restart-router" : "start-router"],
      "Starting ADC router daemon…"
    );
  }

  async function fetchLogs() {
    try {
      const res = await apiRequest("/api/action", "POST", { args: ["logs", "--lines", "40"] });
      document.getElementById("logsBox").textContent = res.text || "No logs available.";
    } catch (e) {
      document.getElementById("logsBox").textContent = "Error loading logs.";
    }
  }

  function toggleLogsPanel() {
    state.showLogsPanel = !state.showLogsPanel;
    const box = document.getElementById("logsBox");
    if (state.showLogsPanel) {
      box.classList.add("visible");
      fetchLogs();
    } else {
      box.classList.remove("visible");
    }
    renderState();
  }

  window.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      fetch("/api/window/hide", { method: "POST" }).catch(() => {});
    }
  });

  // Initial status fetch & 4-second polling (matching GeminiRouter.qml pollTimer)
  refreshStatus();
  setInterval(refreshStatus, 4000);
</script>
</body>
</html>
"#;
