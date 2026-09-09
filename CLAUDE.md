# CLAUDE.md

IMPORTANT: When applicable, prefer using intellij-index MCP tools for code navigation and refactoring.

Rust backend builds are handled by Tauri automatically during `pnpm tauri dev` / `pnpm tauri build`. To check Rust code independently:
```bash
cd src-tauri && cargo check
```

## Architecture

### Frontend (`src/`)
- **Vue 3 + TypeScript** with `<script setup>` SFCs
- **Naive UI** component library, auto-imported via `unplugin-vue-components` (NaiveUiResolver)
- **Auto-imports** configured in `vite.config.ts`: Vue, Vue Router, VueUse APIs, and Naive UI composables are available without explicit imports
- **Pinia** for state with `pinia-plugin-persistedstate` for localStorage persistence
- **Path alias**: `@` maps to `src/`
- **Pages**: Home (video search/download UI), Downloads, Settings
- **Tauri IPC**: Frontend calls Rust commands via `invoke()` from `@tauri-apps/api/core`

### Backend (`src-tauri/src/`)
- `lib.rs` — Tauri app builder, registers all commands and plugins
- `commands/` — Tauri command handlers,按功能域拆分:
  - `mod.rs` — shared types (DownloadState, DownloadParams, YtdlpStatus etc.)
  - `setup.rs` — platform info, yt-dlp/Deno installation management
  - `video.rs` — video info fetching (`-J`), cookie management
  - `download.rs` — download task control (start/pause/resume/cancel/check_files_exist)
- `parser.rs` — yt-dlp `--progress-template` JSON output parsing
- `process.rs` — OS-level process control (suspend/resume/kill via Win32 API or signals)
- `utils.rs` — Path helpers (yt-dlp, Deno, cookie paths in app data dir), platform-specific download URLs, JS runtime args builder
- Binaries (yt-dlp, Deno) are downloaded to the Tauri app data directory at runtime, not bundled
- Progress events emitted to frontend via `app.emit()` (e.g., `ytdlp-download-progress`, `deno-download-progress`)
- Download progress uses `--progress-template` (structured JSON) instead of parsing stdout text
- Final output file path retrieved via `--print-to-file after_move:filepath` to avoid Windows GBK encoding issues

### Frontend-Backend Communication
- Tauri commands are invoked from Vue via `invoke<T>("command_name", { args })`
- Real-time progress uses Tauri event system (`app.emit` on Rust side)
- Shared types in `src/types/index.ts` mirror Rust structs in `commands/mod.rs`

## Key Conventions

- Windows builds use `CREATE_NO_WINDOW` flag (0x08000000) on all subprocess spawns to hide console windows
- All yt-dlp commands set `PYTHONUTF8=1` environment variable and use `--ignore-config --color never`
- Deno is optional — used as JS runtime for yt-dlp when installed (`--js-runtimes` flag)
- Cookie support: text (Netscape format saved to file) or direct file path
