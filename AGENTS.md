<!-- BEGIN:nextjs-agent-rules -->
# This is NOT the Next.js you know

This version has breaking changes — APIs, conventions, and file structure may all differ from your training data. Read the relevant guide in `node_modules/next/dist/docs/` before writing any code. Heed deprecation notices.
<!-- END:nextjs-agent-rules -->

## dev-launcher: profile storage

When asked to add or edit profiles for this launcher, edit **only**
`%APPDATA%\com.devlauncher.desktop\conf.jsonc`. Create it with the UI button
**Buka conf.jsonc** if it does not exist yet.

- Do **not** edit `profiles.json`: that is the internal UI store (atomic writes + lock).
- Preserve existing entries and use a unique, stable `id` per profile.
- Shape: `{ "folders": [...], "profiles": [...] }`; step types are `exe`, `command`, `open_url`.
- JSONC comments and trailing commas are allowed.
- Ask the user to restart the app after saving. Profiles from this file show a `JSON` badge and are read-only in the UI.
- Full example: README section "Untuk AI agent / edit otomatis" and `conf.example.jsonc` in the app-data folder.
