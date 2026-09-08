#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR/.."

echo "==> Type-checking frontend..."
pnpm typecheck

echo "==> Building frontend..."
pnpm build

echo "==> Building Tauri production bundle..."
pnpm tauri build

echo "==> Production build complete!"
