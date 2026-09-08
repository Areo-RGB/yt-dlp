---
name: backend/core
description: Rust/Tauri backend structure and subprocess/file integration invariants
metadata:
  type: project
---
- `src-tauri/src/lib.rs` builds the Tauri app and registers command modules/plugins.
- Domain commands are under `src-tauri/src/commands/`; local files and R2 integration are separate modules.
- Paths for yt-dlp, Deno, cookies, and app data are centralized under `src-tauri/src/utils/`.
- Progress events are emitted with `app.emit()` and consumed by Vue listeners.
- Platform-specific process behavior is centralized under `src-tauri/src/platform/`.
