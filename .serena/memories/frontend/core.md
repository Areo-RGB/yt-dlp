---
name: frontend/core
description: Frontend structure and integration boundaries for Vue pages and state
metadata:
  type: project
---
- App shell and navigation are in `src/App.vue`; routes are in `src/router/index.ts`.
- Pages are Vue SFCs under `src/pages/`; `LocalFiles.vue` is the local library UI.
- Tauri IPC calls use `invoke<T>(command_name, { args })`; event listeners handle backend progress and app lifecycle events.
- Shared frontend interfaces live in `src/types/index.ts`; local persistent state belongs in Pinia stores.
- User-visible copy belongs in locale JSON files and is accessed through the i18n setup.
