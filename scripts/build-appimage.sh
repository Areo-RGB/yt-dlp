#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR/.."

# Avoid WebKit/Wayland renderer issues during the Linux build environment.
export GDK_BACKEND=x11
export WEBKIT_DISABLE_DMABUF_RENDERER=1
export WEBKIT_DISABLE_COMPOSITING_MODE=1

echo "==> Type-checking frontend..."
pnpm typecheck

# Auto-detect updater signing key if available
if [ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then
  if [ -f "$HOME/.tauri/yt-dlp-gui.key" ]; then
    export TAURI_SIGNING_PRIVATE_KEY="$(cat "$HOME/.tauri/yt-dlp-gui.key")"
    export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}"
  elif [ -f "$HOME/.tauri/tauri.key" ]; then
    export TAURI_SIGNING_PRIVATE_KEY="$(cat "$HOME/.tauri/tauri.key")"
    export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}"
  fi
fi

SIGN_ARG=()
if [ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then
  SIGN_ARG=("--no-sign")
fi

echo "==> Building AppImage..."
pnpm tauri build --bundles appimage "${SIGN_ARG[@]}" "$@"

echo "==> AppImage build complete!"
