#!/usr/bin/env bash
# Universal Uninstaller for Claude & Codex -> Gemini ADC Router (macOS & Linux)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OS_NAME="$(uname -s)"

if [[ "${OS_NAME}" == "Darwin" ]]; then
  exec "${SCRIPT_DIR}/uninstall-macos.sh" "$@"
fi

BIN_DIR="${HOME}/.local/bin"
SYSTEMD_DIR="${HOME}/.config/systemd/user"
AUTOSTART_DIR="${HOME}/.config/autostart"

if [[ -x "${BIN_DIR}/gemini-router-ctl" ]]; then
  "${BIN_DIR}/gemini-router-ctl" disable --target both >/dev/null 2>&1 || true
  "${BIN_DIR}/gemini-router-ctl" stop-router >/dev/null 2>&1 || true
fi

systemctl --user disable --now claude-gemini-router.service >/dev/null 2>&1 || true
rm -f "${SYSTEMD_DIR}/claude-gemini-router.service"
systemctl --user daemon-reload >/dev/null 2>&1 || true

pkill -f "gemini-router-tray" >/dev/null 2>&1 || true
rm -f "${AUTOSTART_DIR}/gemini-router-tray.desktop"

rm -f \
  "${BIN_DIR}/claude-gemini-router" \
  "${BIN_DIR}/gemini-router-ctl" \
  "${BIN_DIR}/omarchy-gemini-router-ctl" \
  "${BIN_DIR}/gemini-router-tray" \
  "${BIN_DIR}/claude-gemini" \
  "${BIN_DIR}/codex-gemini"

echo "Uninstalled Gemini ADC Router."
