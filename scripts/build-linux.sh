#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR/.."

# Fix WebKit/Wayland crash (Gdk-Message Error 71) — force X11 backend via XWayland
export GDK_BACKEND=x11
export WEBKIT_DISABLE_DMABUF_RENDERER=1
export WEBKIT_DISABLE_COMPOSITING_MODE=1

echo "==> Type-checking frontend..."
pnpm typecheck

echo "==> Building frontend..."
pnpm build

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

EXTRA_ARGS=()
if [ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then
  echo "==> No updater signing key found; building bundle without updater artifacts..."
  EXTRA_ARGS=("-c" '{"bundle":{"createUpdaterArtifacts":false}}')
fi

echo "==> Building Tauri production bundle..."
pnpm tauri build "${EXTRA_ARGS[@]}" "$@"

echo "==> Production build complete!"
