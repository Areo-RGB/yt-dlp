#!/usr/bin/env bash
set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PORT=9223

# If port 9223 is not listening, start the Tauri dev server in the background
if ! ss -tlnp 2>/dev/null | grep -q ":${PORT} "; then
  echo "[mcp-tauri] Port ${PORT} not active — starting dev-linux.sh in background..." >&2
  cd "$PROJECT_DIR"
  export GDK_BACKEND=x11
  export WEBKIT_DISABLE_DMABUF_RENDERER=1
  export WEBKIT_DISABLE_COMPOSITING_MODE=1
  pnpm tauri dev &>/dev/null &

  # Wait up to 120s for the port to come up (Rust compile can be slow)
  for i in $(seq 1 120); do
    if ss -tlnp 2>/dev/null | grep -q ":${PORT} "; then
      echo "[mcp-tauri] Port ${PORT} is ready." >&2
      break
    fi
    sleep 1
  done

  if ! ss -tlnp 2>/dev/null | grep -q ":${PORT} "; then
    echo "[mcp-tauri] ERROR: Port ${PORT} did not come up in 120s." >&2
    exit 1
  fi
else
  echo "[mcp-tauri] Port ${PORT} already active." >&2
fi

# Launch the MCP server
exec npx -y @hypothesi/tauri-mcp-server
