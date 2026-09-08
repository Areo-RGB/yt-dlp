---
name: conventions
description: Stable frontend/backend conventions that prevent integration mistakes
metadata:
  type: project
---
- UI text is localized through `src/locales/`; avoid hard-coding user-visible strings in new UI.
- Tauri command payloads use camelCase names from Vue and Rust serde-compatible types.
- Windows subprocesses use `CREATE_NO_WINDOW`; yt-dlp commands set `PYTHONUTF8=1`, `--ignore-config`, and `--color never`.
- Prefer structured yt-dlp progress JSON and `--print-to-file after_move:filepath` over parsing human-readable output.
- Rust functionality is split by domain in `src-tauri/src/commands/`, with shared command types in `commands/mod.rs`.
