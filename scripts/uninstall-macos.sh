#!/usr/bin/env bash
# Native macOS Uninstaller for Claude & Codex -> Gemini ADC Router
set -euo pipefail

BIN_DIR="${HOME}/.local/bin"
LAUNCH_AGENTS_DIR="${HOME}/Library/LaunchAgents"
APP_BUNDLE="${HOME}/Applications/Gemini ADC Router.app"

echo "==> [1/4] Restoring direct Claude Code & OpenAI Codex configurations..."
if [[ -x "${BIN_DIR}/gemini-router-ctl" ]]; then
  "${BIN_DIR}/gemini-router-ctl" disable --target both >/dev/null 2>&1 || true
  "${BIN_DIR}/gemini-router-ctl" stop-router >/dev/null 2>&1 || true
fi

echo "==> [2/4] Unloading macOS LaunchAgents..."
for plist in com.gemini.adc.tray.plist com.gemini.adc.router.plist; do
  if [[ -f "${LAUNCH_AGENTS_DIR}/${plist}" ]]; then
    launchctl unload "${LAUNCH_AGENTS_DIR}/${plist}" >/dev/null 2>&1 || true
    rm -f "${LAUNCH_AGENTS_DIR}/${plist}"
  fi
done
pkill -f "gemini-router-tray" >/dev/null 2>&1 || true
pkill -f "claude-gemini-router" >/dev/null 2>&1 || true

echo "==> [3/4] Removing macOS Menu Bar Application Bundle..."
rm -rf "${APP_BUNDLE}"

echo "==> [4/4] Removing CLI binaries from ${BIN_DIR}..."
rm -f \
  "${BIN_DIR}/claude-gemini-router" \
  "${BIN_DIR}/gemini-router-ctl" \
  "${BIN_DIR}/omarchy-gemini-router-ctl" \
  "${BIN_DIR}/gemini-router-tray" \
  "${BIN_DIR}/claude-gemini" \
  "${BIN_DIR}/codex-gemini"

echo "Uninstalled Gemini ADC Router from macOS."
