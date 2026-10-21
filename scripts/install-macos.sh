#!/usr/bin/env bash
# Native macOS Installer for Claude & Codex -> Gemini ADC Router + Notification Bar App
set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN_DIR="${HOME}/.local/bin"
CONFIG_DIR="${HOME}/.config/gemini-router"
LAUNCH_AGENTS_DIR="${HOME}/Library/LaunchAgents"
APPS_DIR="${HOME}/Applications"
APP_BUNDLE="${APPS_DIR}/Gemini ADC Router.app"

echo "==> [1/5] Building release Rust binaries (claude-gemini-router, gemini-router-ctl, gemini-router-tray)..."
(
  cd "${REPO_DIR}"
  cargo build --release \
    --bin claude-gemini-router \
    --bin gemini-router-ctl \
    --bin omarchy-gemini-router-ctl \
    --bin gemini-router-tray
)

echo "==> [2/5] Installing binaries and CLI wrappers into ${BIN_DIR}..."
mkdir -p "${BIN_DIR}" "${CONFIG_DIR}"
install -m 0755 "${REPO_DIR}/target/release/claude-gemini-router" "${BIN_DIR}/claude-gemini-router"
install -m 0755 "${REPO_DIR}/target/release/gemini-router-ctl" "${BIN_DIR}/gemini-router-ctl"
install -m 0755 "${REPO_DIR}/target/release/omarchy-gemini-router-ctl" "${BIN_DIR}/omarchy-gemini-router-ctl"
install -m 0755 "${REPO_DIR}/target/release/gemini-router-tray" "${BIN_DIR}/gemini-router-tray"
install -m 0755 "${REPO_DIR}/bin/claude-gemini" "${BIN_DIR}/claude-gemini"
install -m 0755 "${REPO_DIR}/bin/codex-gemini" "${BIN_DIR}/codex-gemini"

echo "==> [3/5] Creating macOS Menu Bar Application Bundle at ${APP_BUNDLE}..."
mkdir -p "${APP_BUNDLE}/Contents/MacOS" "${APP_BUNDLE}/Contents/Resources"
install -m 0755 "${REPO_DIR}/target/release/gemini-router-tray" "${APP_BUNDLE}/Contents/MacOS/gemini-router-tray"

cat > "${APP_BUNDLE}/Contents/Info.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>Gemini ADC Router</string>
    <key>CFBundleDisplayName</key>
    <string>Gemini ADC Router</string>
    <key>CFBundleIdentifier</key>
    <string>com.gemini.adc.tray</string>
    <key>CFBundleVersion</key>
    <string>0.2.0</string>
    <key>CFBundleShortVersionString</key>
    <string>0.2.0</string>
    <key>CFBundleExecutable</key>
    <string>gemini-router-tray</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>LSUIElement</key>
    <true/>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
EOF

echo "==> [4/5] Configuring macOS LaunchAgents for Router Daemon & Notification Bar Tray..."
mkdir -p "${LAUNCH_AGENTS_DIR}"

ROUTER_PLIST="${LAUNCH_AGENTS_DIR}/com.gemini.adc.router.plist"
cat > "${ROUTER_PLIST}" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.gemini.adc.router</string>
    <key>ProgramArguments</key>
    <array>
        <string>${BIN_DIR}/claude-gemini-router</string>
        <string>--port</string>
        <string>4000</string>
    </array>
    <key>EnvironmentVariables</key>
    <dict>
        <key>PATH</key>
        <string>${BIN_DIR}:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin</string>
        <key>GOOGLE_API_USE_CLIENT_CERTIFICATE</key>
        <string>false</string>
    </dict>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>${CONFIG_DIR}/router.log</string>
    <key>StandardErrorPath</key>
    <string>${CONFIG_DIR}/router.log</string>
</dict>
</plist>
EOF

TRAY_PLIST="${LAUNCH_AGENTS_DIR}/com.gemini.adc.tray.plist"
cat > "${TRAY_PLIST}" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.gemini.adc.tray</string>
    <key>ProgramArguments</key>
    <array>
        <string>${APP_BUNDLE}/Contents/MacOS/gemini-router-tray</string>
        <string>--autostart-router</string>
    </array>
    <key>EnvironmentVariables</key>
    <dict>
        <key>PATH</key>
        <string>${BIN_DIR}:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin</string>
    </dict>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <false/>
</dict>
</plist>
EOF

echo "==> [5/5] Starting macOS LaunchAgents..."
launchctl unload "${ROUTER_PLIST}" >/dev/null 2>&1 || true
launchctl load -w "${ROUTER_PLIST}" >/dev/null 2>&1 || true
launchctl unload "${TRAY_PLIST}" >/dev/null 2>&1 || true
launchctl load -w "${TRAY_PLIST}" >/dev/null 2>&1 || true

echo ""
echo "=================================================================="
echo " Gemini ADC Router + macOS Notification Bar App Installed!"
echo "=================================================================="
echo " - Menu Bar App:  ${APP_BUNDLE}"
echo " - Router Daemon: http://127.0.0.1:4000/health"
echo " - Browser UI:    http://127.0.0.1:4000/ui"
echo " - CLI Control:   ${BIN_DIR}/gemini-router-ctl status"
echo " - Ensure ${BIN_DIR} is in your PATH (~/.zshrc):"
echo "     export PATH=\"\$HOME/.local/bin:\$PATH\""
echo "=================================================================="
