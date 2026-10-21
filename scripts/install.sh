#!/usr/bin/env bash
# Universal Installer for Claude & Codex -> Gemini ADC Router (macOS & Linux)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
OS_NAME="$(uname -s)"

if [[ "${OS_NAME}" == "Darwin" ]]; then
  exec "${SCRIPT_DIR}/install-macos.sh" "$@"
fi

BIN_DIR="${HOME}/.local/bin"
SYSTEMD_DIR="${HOME}/.config/systemd/user"
AUTOSTART_DIR="${HOME}/.config/autostart"
PLUGIN_DEST="${HOME}/.config/omarchy/plugins/arch.gemini-router"

echo "==> [1/4] Building release Rust binaries..."
(
  cd "${REPO_DIR}"
  cargo build --release \
    --bin claude-gemini-router \
    --bin gemini-router-ctl \
    --bin omarchy-gemini-router-ctl \
    --bin gemini-router-tray
)

echo "==> [2/4] Installing binaries into ${BIN_DIR}..."
mkdir -p "${BIN_DIR}"
install -m 0755 "${REPO_DIR}/target/release/claude-gemini-router" "${BIN_DIR}/claude-gemini-router"
install -m 0755 "${REPO_DIR}/target/release/gemini-router-ctl" "${BIN_DIR}/gemini-router-ctl"
install -m 0755 "${REPO_DIR}/target/release/omarchy-gemini-router-ctl" "${BIN_DIR}/omarchy-gemini-router-ctl"
install -m 0755 "${REPO_DIR}/target/release/gemini-router-tray" "${BIN_DIR}/gemini-router-tray"
install -m 0755 "${REPO_DIR}/bin/claude-gemini" "${BIN_DIR}/claude-gemini"
install -m 0755 "${REPO_DIR}/bin/codex-gemini" "${BIN_DIR}/codex-gemini"

echo "==> [3/4] Installing systemd user service..."
mkdir -p "${SYSTEMD_DIR}"
cp "${REPO_DIR}/omarchy-plugin/systemd/claude-gemini-router.service" "${SYSTEMD_DIR}/claude-gemini-router.service"
systemctl --user daemon-reload >/dev/null 2>&1 || true
systemctl --user enable --now claude-gemini-router.service >/dev/null 2>&1 || true

echo "==> [4/4] Configuring Desktop Notification Bar integration..."
if [[ -d "${HOME}/.config/omarchy" ]]; then
  mkdir -p "${PLUGIN_DEST}/plugin" "${PLUGIN_DEST}/bin" "${PLUGIN_DEST}/systemd"
  cp "${REPO_DIR}/omarchy-plugin/manifest.json" "${PLUGIN_DEST}/manifest.json"
  cp "${REPO_DIR}/omarchy-plugin/plugin/GeminiRouter.qml" "${PLUGIN_DEST}/plugin/GeminiRouter.qml"
  install -m 0755 "${REPO_DIR}/target/release/omarchy-gemini-router-ctl" "${PLUGIN_DEST}/bin/omarchy-gemini-router-ctl"
  cp "${REPO_DIR}/omarchy-plugin/systemd/claude-gemini-router.service" "${PLUGIN_DEST}/systemd/claude-gemini-router.service"
  echo "  Installed Omarchy bar plugin to ${PLUGIN_DEST}"
else
  mkdir -p "${AUTOSTART_DIR}"
  cat > "${AUTOSTART_DIR}/gemini-router-tray.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Gemini ADC Router Tray
Comment=Notification bar control panel for Claude Code & OpenAI Codex -> Gemini ADC Router
Exec=${BIN_DIR}/gemini-router-tray --autostart-router
Icon=utilities-terminal
Terminal=false
Categories=Development;Utility;
StartupNotify=false
EOF
  echo "  Installed XDG autostart notification bar entry at ${AUTOSTART_DIR}/gemini-router-tray.desktop"
fi

echo ""
echo "=================================================================="
echo " Gemini ADC Router Installed Successfully!"
echo "=================================================================="
echo " - Router Health: http://127.0.0.1:4000/health"
echo " - Web Control:   http://127.0.0.1:4000/ui"
echo " - Tray App:      ${BIN_DIR}/gemini-router-tray"
echo " - CLI Control:   ${BIN_DIR}/gemini-router-ctl status"
echo "=================================================================="
