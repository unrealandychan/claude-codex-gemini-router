# Claude Code & OpenAI Codex → Gemini ADC Router (macOS · Windows · Linux)

A native Rust cross-platform **Notification Bar / System Tray** application and local HTTP/SSE translation proxy (`http://127.0.0.1:4000`) that routes **Claude Code (`claude`)** and **OpenAI Codex CLI (`codex`)** directly to **Google Cloud Vertex AI (`gemini-3.8-flash` & `gemini-4-argon`)** using **keyless Application Default Credentials (ADC)**.

- **Native Notification Bar Application (`gemini-router-tray`)**:
  - **macOS**: Sits in the **macOS Menu Bar / Notification Bar** (`NSStatusBar` with `ActivationPolicy::Accessory` and `LSUIElement=true` — no Dock icon clutter).
  - **Windows**: Sits in the **Windows System Tray / Notification Area** (`Shell_NotifyIconW` with `#![windows_subsystem = "windows"]` and skip-taskbar frameless popover).
  - **Linux**: Supports both the **AppIndicator / StatusNotifier System Tray** (`gemini-router-tray`) and the native **Omarchy / Quickshell Status Bar** (`omarchy-plugin/plugin/GeminiRouter.qml`).
- **Identical Catppuccin Mocha UI Across All Platforms**:
  - Same `380px` control panel layout, colors (`#1e1e2e` base, `#89b4fa` accent, `#a6e3a1` active green, `#f38ba8` alert red), status pill, 401/403 Auth Reset alert card, Client Routing Toggles (`Route Both to Gemini`, `Disable All (Direct)`, individual `Claude Code` & `OpenAI Codex` toggles), Model & Effort Selector (`Gemini 3.8 Flash`, `Gemini 4.0 Argon`, `LOW`/`MEDIUM`/`HIGH`/`XHIGH` thinking effort), and ADC & Daemon Overview card with collapsible live logs.
- **Zero API Keys Required**:
  - Uses Google Cloud **Application Default Credentials (ADC)** (`gcloud auth application-default login`) and automatically strips `GEMINI_API_KEY`, `GOOGLE_API_KEY`, `ANTHROPIC_API_KEY`, and `OPENAI_API_KEY`.
- **Full Tool Calling, Subagents, & Skills Support**:
  - Translates Anthropic Messages API (`/v1/messages`, `/v1/messages/count_tokens`), OpenAI Codex Responses API (`/v1/responses`), and OpenAI Chat Completions API (`/v1/chat/completions`) to Vertex AI `generateContent` / `streamGenerateContent` with full `thoughtSignature` preservation, JSON Schema sanitization, and `gemini-4-argon` turn normalization.

---

## Architecture & Binaries

This workspace compiles 4 native Rust binaries:

| Binary | Description |
| :--- | :--- |
| `claude-gemini-router` | Async Tokio + Reqwest (Rustls) HTTP/SSE translation proxy (`127.0.0.1:4000`) + embedded Web UI (`GET /ui`, `GET /api/status`, `POST /api/action`). |
| `gemini-router-tray` | Native **macOS Menu Bar**, **Windows System Tray Notification Area**, and **Linux AppIndicator** application (`tao` + `wry` + `tray-icon`) rendering the frameless Catppuccin Mocha popover control panel. |
| `gemini-router-ctl` | Cross-platform CLI controller (`status`, `enable`, `disable`, `toggle`, `set-model`, `start-router`, `stop-router`, `restart-router`, `logs`, `adc-login`, `refresh-auth`) with lossless config backup & restore. |
| `omarchy-gemini-router-ctl` | Drop-in compatible controller binary for the Linux Omarchy / Quickshell bar plugin. |

---

## Prerequisites

1. **Google Cloud SDK (`gcloud`)** authenticated with Application Default Credentials:
   ```bash
   gcloud auth application-default login
   gcloud config set project YOUR_GCP_PROJECT_ID
   ```
   - **macOS / Linux ADC location**: `~/.config/gcloud/application_default_credentials.json`
   - **Windows ADC location**: `%APPDATA%\gcloud\application_default_credentials.json`
2. **Rust Toolchain** (`rustup` / `cargo` stable) when building from source.
   - **macOS**: Uses built-in Cocoa `NSStatusBar` + `WKWebView` (no extra packages needed).
   - **Windows**: Uses built-in Win32 `Shell_NotifyIconW` + Edge `WebView2` Runtime (pre-installed on Windows 10/11).
   - **Linux**: Requires `gtk3`, `webkit2gtk-4.1`, and `libayatana-appindicator` (or `libappindicator-gtk3`).

---

## Quick Start Installation

### macOS (Menu Bar / Notification Bar App + LaunchAgent)

```bash
git clone https://github.com/unrealandychan/claude-codex-gemini-router.git
cd claude-codex-gemini-router
./scripts/install-macos.sh
```

What `install-macos.sh` sets up:
- Compiles release binaries and installs `claude-gemini-router`, `gemini-router-ctl`, `gemini-router-tray`, `claude-gemini`, and `codex-gemini` to `~/.local/bin`.
- Creates **`~/Applications/Gemini ADC Router.app`** (`LSUIElement=true` so it sits in the macOS Notification Bar / Menu Bar with no Dock icon).
- Installs and starts `launchd` agents in `~/Library/LaunchAgents/`:
  - `com.gemini.adc.router.plist` (background router daemon on `127.0.0.1:4000`)
  - `com.gemini.adc.tray.plist` (notification bar icon at login)

To uninstall on macOS and restore direct Claude/Codex configs:
```bash
./scripts/uninstall-macos.sh
```

---

### Windows (System Tray Notification Area App + Startup Shortcut)

Open **PowerShell** and run:

```powershell
git clone https://github.com/unrealandychan/claude-codex-gemini-router.git
cd claude-codex-gemini-router
powershell -ExecutionPolicy Bypass -File .\scripts\install-windows.ps1
```

What `install-windows.ps1` sets up:
- Compiles `claude-gemini-router.exe`, `gemini-router-ctl.exe`, and `gemini-router-tray.exe`.
- Installs binaries and Windows CLI wrappers (`claude-gemini.cmd`, `claude-gemini.ps1`, `codex-gemini.cmd`, `codex-gemini.ps1`) to `%USERPROFILE%\.local\bin` and adds it to your User `PATH`.
- Creates a Windows Startup shortcut (`%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup\Gemini ADC Router Tray.lnk`) and launches `gemini-router-tray.exe --autostart-router` in the Windows Notification Area.

To uninstall on Windows and restore direct Claude/Codex configs:
```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\uninstall-windows.ps1
```

---

### Linux (System Tray App or Omarchy Bar Plugin + systemd)

```bash
git clone https://github.com/unrealandychan/claude-codex-gemini-router.git
cd claude-codex-gemini-router
./scripts/install.sh
```

---

## Notification Bar UI & Controls

- **Left-click** the notification bar icon (macOS Menu Bar / Windows System Tray) to toggle the frameless **Gemini ADC Router** popover window anchored directly to the tray icon:
  - **Header**: Live status badge (`ADC Off`, `Claude -> G3.8`, `Codex -> G4`, `C+X -> G4`, or `Auth Reset Needed`) and refresh button.
  - **401/403 Auth Alert Card**: Automatically appears if your ADC token expires or encounters a `401`/`403` error, with one-click **Reset Credential (ADC Login)** (opens Terminal.app on macOS, Windows Terminal / PowerShell on Windows, or your Linux terminal emulator to run `gcloud auth application-default login`) and **Retry Auto-Heal**.
  - **Client Routing Toggles**:
    - **Route Both to Gemini** / **Disable All (Direct)** master buttons.
    - Individual **Claude Code (`~/.claude/settings.json`)** and **OpenAI Codex (`~/.codex/config.toml`)** routing switches with lossless backup & restoration of your original settings.
  - **Model & Effort Selector**:
    - Hot-swap between **Gemini 3.8 Flash** (`gemini-3.8-flash`) and **Gemini 4.0 Argon** (`gemini-4-argon`) without restarting Claude Code or Codex sessions.
    - Select reasoning effort (`LOW`, `MEDIUM`, `HIGH`, `XHIGH`).
  - **ADC & Daemon Overview**:
    - Live router health, GCP project & location, session request/token counters, **Start/Restart Router**, **Stop**, **ADC Re-login**, and collapsible **Logs** viewer.
- **Right-click** the notification bar icon for quick OS context-menu actions (`Toggle Claude Code`, `Toggle OpenAI Codex`, `Model: Gemini 3.8 Flash`, `Model: Gemini 4.0 Argon`, `Start / Restart Router Daemon`, `Re-authenticate ADC`, `Quit`).
- **Browser Fallback**: When `claude-gemini-router` is running, the exact same control panel is also served at `http://127.0.0.1:4000/ui`.

---

## GUI Claude Code & GUI OpenAI Codex Support + Live Model Switching

The plugin supports both **CLI** and **GUI** versions of **Claude Code** and **OpenAI Codex** on macOS, Windows, and Linux:

| Client Application | How Routing & Model Switching Works |
| :--- | :--- |
| **Claude Code GUI** (Claude Desktop App *Code* tab, VS Code extension, Cursor/Windsurf extension, JetBrains IDE plugin) & **Claude Code CLI** | Synchronizes `~/.claude/settings.json` (`%USERPROFILE%\.claude\settings.json` on Windows) including top-level `"model"` & `"effortLevel"` (watched by GUI extensions) and `"env"` (`ANTHROPIC_BASE_URL`, `ANTHROPIC_AUTH_TOKEN`, `ANTHROPIC_MODEL`, `ANTHROPIC_DEFAULT_SONNET_MODEL`, `ANTHROPIC_DEFAULT_OPUS_MODEL`, `ANTHROPIC_DEFAULT_HAIKU_MODEL`). |
| **OpenAI Codex GUI** (Codex Desktop App, VS Code Codex extension) & **OpenAI Codex CLI** | Synchronizes `~/.codex/config.toml` (`%USERPROFILE%\.codex\config.toml` on Windows) with `model_provider = "gemini_adc"`, `model`, `model_reasoning_effort`, and `[model_providers.gemini_adc]` pointing to `http://127.0.0.1:4000/v1`. |
| **Live Mid-Session Model & Effort Hot-Swapping** | Clicking **Gemini 3.8 Flash** / **Gemini 4.0 Argon** or **`LOW` / `MEDIUM` / `HIGH` / `XHIGH`** in the Notification Bar UI immediately pushes `POST /config` (`hot_swap: true`) to the router daemon. Even if an already-open GUI window sends a cached model ID (`gemini-3.8-flash[1m]`, `gemini-4-argon`, `claude-sonnet-4-6`, `claude-opus-4-6`, `gpt-5-codex`), the router intercepts and hot-swaps the primary model and Gemini `thinkingConfig.thinkingLevel` on the fly while keeping background `haiku`/`mini` tasks on `gemini-3.8-flash`. |

---

## CLI Usage (`gemini-router-ctl` & Wrappers)

### Persistent Routing via `gemini-router-ctl`

```bash
# View full JSON status (routing state, active model, ADC token validity, session stats)
gemini-router-ctl status

# Route both Claude Code and OpenAI Codex to Gemini 4.0 Argon with high reasoning effort
gemini-router-ctl enable --target both --model gemini-4-argon --effort high

# Hot-swap active model between gemini-3.8-flash and gemini-4-argon
gemini-router-ctl set-model --flip

# Disable routing and losslessly restore original ~/.claude/settings.json & ~/.codex/config.toml
gemini-router-ctl disable --target both
```

### Per-Session Launchers (`claude-gemini` & `codex-gemini`)

If you prefer launching a single terminal session routed through Gemini without modifying global config files:

```bash
# Launch Claude Code backed by Gemini 3.8 Flash (default) or Gemini 4.0 Argon
claude-gemini --flash
claude-gemini --argon

# Launch OpenAI Codex CLI backed by Gemini 4.0 Argon with high reasoning effort
codex-gemini --argon --effort=high
```

---

## Running Tests

```bash
# Run all Rust unit & E2E integration tests
cargo test

# Run the 55-assertion CLI controller & security test suite
./omarchy-plugin/tests/test-ctl.sh
```
