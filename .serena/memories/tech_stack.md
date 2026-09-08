---
name: tech_stack
description: Languages, frameworks, tooling, and runtime dependencies used by the app
metadata:
  type: project
---
- Vue 3 SFCs with `<script setup>` and TypeScript.
- Naive UI components are auto-imported; Vue, Vue Router, VueUse APIs, and Naive UI composables are auto-imported via Vite config.
- Pinia with `pinia-plugin-persistedstate` for localStorage-backed state.
- Tauri 2 with Rust backend; `@tauri-apps/api` handles IPC/events.
- pnpm is the package manager; Vite serves frontend development on port 5688.
- yt-dlp and optional Deno are downloaded into the Tauri app-data directory at runtime.
