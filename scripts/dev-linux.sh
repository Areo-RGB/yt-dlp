#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR/.."

# Fix WebKit/Wayland crash (Gdk-Message Error 71) — force X11 backend via XWayland
export GDK_BACKEND=x11
export WEBKIT_DISABLE_DMABUF_RENDERER=1
export WEBKIT_DISABLE_COMPOSITING_MODE=1

echo "==> Starting Tauri dev (frontend + Rust backend, X11 backend)..."
pnpm tauri dev
