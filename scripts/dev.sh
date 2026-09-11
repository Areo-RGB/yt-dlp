#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR/.."

# Avoid WebKit/Wayland renderer issues under Linux Wayland sessions
export WEBKIT_DISABLE_DMABUF_RENDERER=1
export __NV_DISABLE_EXPLICIT_SYNC=1

echo "==> Starting frontend dev server (Vite on port 15688, with automatic fallback)..."
pnpm dev
