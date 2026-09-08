---
name: task_completion
description: Verification commands for considering a code change complete
metadata:
  type: project
---
- Run `pnpm build` for frontend type-check and Vite build.
- Run `cd src-tauri && cargo check` for Rust compilation checks.
- For UI changes, run the full app with `pnpm tauri dev` and exercise the affected flow; use the Tauri tooling when available.
- Run focused tests if the changed domain has them; do not claim UI behavior from type-checking alone.
