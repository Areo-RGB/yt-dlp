# AGENTS.md — AI Agent Instructions for yt-dlp-gui

## Project Overview

Desktop video downloader built with **Tauri 2** (Rust backend) + **Vue 3** (TypeScript frontend).
UI language is Chinese. See `CLAUDE.md` for full architecture details.

## Platform-Specific Notes

### Linux (Wayland / CachyOS / Arch-based)

The app's WebKit webview crashes under native Wayland with `Gdk-Message: Error 71 (Protokollfehler)`.
WebkitGTK also fails to create a GBM buffer under XWayland without disabling compositing.
**Fix:** Force X11 backend and disable WebKit's DMA-BUF renderer and compositing mode:

```bash
export GDK_BACKEND=x11
export WEBKIT_DISABLE_DMABUF_RENDERER=1
export WEBKIT_DISABLE_COMPOSITING_MODE=1
```

This is already set in all Linux-specific scripts and `package.json` Tauri scripts.

When running directly on Linux, use:

```bash
pnpm run tauri:dev
```

Or manually:

```bash
GDK_BACKEND=x11 WEBKIT_DISABLE_DMABUF_RENDERER=1 WEBKIT_DISABLE_COMPOSITING_MODE=1 pnpm tauri dev
```

### MCP Bridge Plugin

The project uses `tauri-plugin-mcp-bridge` for AI-assisted development.
The plugin exposes a WebSocket on `0.0.0.0:9223` in debug builds only (`#[cfg(debug_assertions)]`).

To connect:

```bash
tauri-mcp driver-session start --port 9223
```

## Quick Reference

| Task | Command |
|------|---------|
| Install deps | `pnpm install` |
| Dev (Linux) | `./scripts/dev-linux.sh` or `pnpm run tauri:dev` |
| Dev (generic) | `pnpm tauri dev` |
| Build (Linux) | `./scripts/build-linux.sh` or `pnpm run tauri:build` |
| Build (Windows) | `.\scripts\build-release.ps1` or `.\scripts\build.ps1` |
| Build debug (Windows) | `.\scripts\build-debug.ps1` |
| Build (generic) | `pnpm tauri build` |
| Type-check | `pnpm typecheck` |
| Lint | `pnpm lint` |
| Cargo check | `cd src-tauri && cargo check` |
