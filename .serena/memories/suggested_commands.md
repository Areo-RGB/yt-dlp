---
name: suggested_commands
description: Commands used to install, develop, build, and inspect the project
metadata:
  type: project
---
- Install: `pnpm install`.
- Frontend dev: `pnpm dev`.
- Full app dev: `pnpm tauri dev`.
- Frontend production build/type-check: `pnpm build`.
- Production app bundle: `pnpm tauri build`.
- Rust check: `cd src-tauri && cargo check`.
- Use the repository scripts under `scripts/` when they wrap these operations for the local environment.
