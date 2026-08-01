# Dev Launcher

Desktop app untuk menjalankan semua proses development project dengan satu tombol — start Laragon, `npm run dev`, buka VS Code/browser, semuanya otomatis.

## Features

- **Multi-step profiles** — Jalankan exe, command, atau buka URL secara berurutan
- **Live log viewer** — Lihat stdout/stderr realtime dengan tab per step
- **Port conflict detection** — Deteksi port yang bentrok sebelum start, bisa auto-kill
- **Process management** — Start/stop profile, kill tree seluruh child process
- **Log persistence** — Log tetap ada saat pindah antar profile

## Tech Stack

- **Tauri v2** — Desktop app framework (Rust + WebView)
- **Next.js 16** — Frontend (App Router, TypeScript, Tailwind v4)
- **Rust** — Backend logic (process spawning, port detection, log streaming)
- **TypeScript** — Type-safe frontend

## Development

```bash
# Install dependencies
npm install

# Run dev mode (hot reload frontend + Rust backend)
cargo tauri dev
```

Frontend: `http://localhost:1420` (atau port lain yang tersedia)

## Build

```bash
# Build production binary
cargo tauri build
```

Output: `src-tauri/target/release/dev-launcher.exe`

## Project Structure

```
dev-launcher/
├── src/                      # Next.js frontend
│   ├── app/                  # App Router pages
│   ├── components/           # React components
│   ├── lib/                  # Utilities (tauri wrapper)
│   └── types.ts              # TypeScript types
├── src-tauri/                # Rust backend
│   ├── src/
│   │   ├── lib.rs            # Tauri commands registration
│   │   ├── models.rs         # Profile/Step structs
│   │   ├── storage.rs        # JSON persistence (~/.dev-launcher/)
│   │   ├── ports.rs          # Port detection (netstat parsing)
│   │   └── runner.rs         # Process spawning & log streaming
│   └── Cargo.toml
└── package.json
```

## How It Works

1. **Profile** berisi list of **steps** (exe/command/open_url)
2. Klik **Start** → backend spawn proses berurutan
3. Log di-stream via Tauri event (`profile-log`) realtime
4. **Stop** → kill tree seluruh child process (Windows: `taskkill /T /F`)
5. Port conflict check sebelum start → modal "Kill & Continue" atau batal

## Storage

Profiles disimpan di: `~/.dev-launcher/profiles.json`

Format:
```json
{
  "profiles": [
    {
      "id": "uuid",
      "name": "Project X",
      "steps": [
        { "type": "exe", "path": "C:\\laragon\\laragon.exe" },
        { "type": "command", "cwd": "C:\\project", "cmd": "npm run dev", "port": 3000 },
        { "type": "open_url", "url": "http://localhost:3000" }
      ]
    }
  ]
}
```

## Notes

- **Windows**: `npm`/`npx` shim dijalankan via `cmd.exe /C` (Windows CreateProcess limitation)
- **Kill tree**: Gunakan `taskkill /T` untuk kill parent + semua child process
- **Auto port switch**: Tools seperti Next.js/Vite handle auto-increment port sendiri (3000 → 3001 → ...)

## License

MIT
