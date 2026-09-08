---
name: core
description: Project map and cross-layer invariants for the yt-dlp-gui desktop app
metadata:
  type: project
---
- Tauri 2 desktop app: Vue 3 + TypeScript frontend, Rust backend.
- Frontend pages live under `src/pages/`; shared UI under `src/components/`; Pinia stores under `src/stores/`.
- Tauri commands are registered in `src-tauri/src/lib.rs` and grouped under `src-tauri/src/commands/`.
- Frontend/backend shared shapes are mirrored in `src/types/index.ts` and Rust command types.
- Backend emits progress through Tauri events; frontend invokes commands through `@tauri-apps/api/core`.
- Frontend module details: `mem:frontend/core` and backend details: `mem:backend/core`.
- Build and completion commands: `mem:suggested_commands`, `mem:task_completion`.
