# Dev Launcher

Desktop app untuk menjalankan semua proses development project dengan satu tombol — start Laragon, `npm run dev`, buka VS Code/browser, semuanya otomatis.

Target platform: **Windows 10/11** (x64).

## Features

- **Multi-step profiles** — Jalankan exe, command, atau buka URL secara berurutan; command bisa multi-baris dan pilih shell (auto/cmd/PowerShell/bash)
- **Start non-blocking** — UI tetap responsif; progres tiap step dikirim lewat event
- **Live log viewer** — stdout/stderr realtime (di-batch ~100ms), tab per step, filter pencarian, pause auto-scroll, salin/unduh log
- **Log persistence** — Log disimpan ke disk, rotasi 5MB, retensi otomatis 14 hari / 50MB
- **Port conflict detection** — Preflight di backend sebelum start, opsi ganti port, lewati step, atau kill proses (hanya kalau PID masih memegang port)
- **Readiness check** — Tunggu port siap via TCP atau HTTP request, timeout bisa diatur per step
- **Per-step control** — Environment variable, delay, opsional (lanjut walau gagal), restart otomatis saat crash dengan batas percobaan
- **Process management** — Graceful stop (tunggu ~2,5s lalu force), kill seluruh pohon proses, Stop All / Start All
- **Crash-safe** — Penyimpanan atomic + backup + auto-recovery; PID journal memulihkan status proses setelah app restart
- **Single instance** — Membuka app dua kali tidak membuat dua instance yang berebut file
- **Folder & reorder** — Kelompokkan profile, drag & drop antar folder, pencarian, alternatif keyboard `Alt+↑/↓`
- **UX hardening** — Toast + error inline, modal konfirmasi sendiri, guard perubahan belum disimpan, state fokus/loading/empty yang rapi
- **Log panel** — Auto-scroll bisa dijeda, pencarian, salin/unduh, toggle wrap
- **Quick restart** — Stop + start ulang satu klik; durasi dan exit code terakhir ditampilkan per profile
- **conf.jsonc** — Tambah profile lewat file JSONC (komentar & trailing comma); read-only di UI
- **Window state** — Ukuran dan posisi jendela diingat saat restart aplikasi
- **Hardening Fase 4** — CSP ketat di release (`devCsp` terpisah untuk dev), devtools mati di release, capability ACL minimal, validasi input backend terpusat
- **App log** — Log aplikasi sendiri di `app-logs/` dengan rotasi, terpisah dari log profile

## Tech Stack

- **Tauri v2** — Desktop app framework (Rust + WebView2)
- **Next.js 16** — Frontend (App Router, TypeScript, Tailwind v4), static export
- **Rust** — Backend logic (process spawning, port detection, log streaming)
- **TypeScript** — Type-safe frontend

## Development

```bash
# Install dependencies
npm install

# Cek kualitas kode
npm run lint
npm run typecheck

# Run dev mode (hot reload frontend + Rust backend)
npm run tauri dev
```

Frontend dev server: `http://localhost:3000` (lihat `src-tauri/tauri.conf.json`).

## Build

```bash
npm run tauri build
```

Output installer: `src-tauri/target/release/bundle/`.
Binary: `src-tauri/target/release/dev-launcher.exe`.

## Testing

```bash
# Frontend
npm run test:unit   # Vitest: helper format + wrapper Tauri
npm run test:e2e    # Playwright smoke (web preview di http://127.0.0.1:3000)

# Backend
cd src-tauri
cargo fmt --check
cargo test          # unit + integration: storage, validasi, journal, kill-tree, preflight
cargo clippy --all-targets -- -D warnings
```

Smoke Playwright menguji frontend lewat browser biasa, bukan binary Tauri (binary tidak bisa di-drive Playwright).

## CI & rilis

Workflow Windows ada di `.github/workflows/ci.yml` (lint, typecheck, unit test, build, cargo test/clippy, build binary, smoke Playwright). Panduan rilis ada di `docs/RELEASING.md`, riwayat perubahan di `CHANGELOG.md`.

## Project Structure

```
dev-launcher/
├── src/                      # Next.js frontend
│   ├── app/                  # App Router pages
│   ├── components/           # React components
│   ├── lib/                  # Tauri command wrapper + pure helper (format.ts)
│   └── types.ts              # TypeScript types
├── tests/                    # Vitest unit + Playwright smoke
├── src-tauri/                # Rust backend
│   ├── src/
│   │   ├── lib.rs            # Command registration & app setup
│   │   ├── models.rs         # Profile/Step structs
│   │   ├── storage.rs        # Atomic JSON persistence + log retention
│   │   ├── journal.rs        # PID journal untuk rekonsiliasi restart
│   │   ├── logbus.rs         # Batch log writer + event emitter
│   │   ├── ports.rs          # Port detection (netstat parsing)
│   │   ├── runner.rs         # Process runner, preflight, stop/restart
│   │   ├── util.rs           # ANSI stripping, timestamp
│   │   └── win.rs            # FFI Windows (single-instance, ShellExecute)
│   ├── tests/                # Integration test Rust
│   └── Cargo.toml
├── .github/workflows/ci.yml
├── docs/RELEASING.md
├── CHANGELOG.md
└── package.json
```

## How It Works

1. **Profile** berisi list of **steps** (exe/command/open_url)
2. Klik **Start** → preflight (konflik port + validasi) → backend spawn proses di background
3. Log di-batch dan di-stream via event `profile-log-batch`
4. **Stop** → kirim permintaan berhenti, tunggu 2,5 detik, lalu force kill pohon proses
5. PID dicatat di journal; kalau app crash, proses masih bisa dikenali dan dihentikan saat app dibuka lagi

## Storage

Lokasi data mengikuti direktori app-data Tauri:
`%APPDATA%\com.devlauncher.desktop\`

| File | Isi |
|---|---|
| `profiles.json` | Profile dan folder yang dikelola lewat UI |
| `profiles.json.bak` | Backup otomatis versi sebelumnya |
| `conf.jsonc` | Profile eksternal (opsional, dibuat manual) |
| `conf.example.jsonc` | Contoh konfigurasi (dibuat otomatis kalau belum ada) |
| `window.json` | Ukuran & posisi jendela terakhir |
| `running.json` | PID journal proses yang sedang berjalan |
| `logs/<id>.log` | Log per profile (rotasi ke `.1.log`) |
| `app-logs/app.log` | Log aplikasi (rotasi 1MB, maksimal 5 file) |

Data lama di `~/.dev-launcher` atau `%APPDATA%\com.devlauncher.app` dimigrasikan otomatis saat pertama kali dijalankan.
Penulisan `profiles.json` atomic (temp → fsync → rename); kalau file rusak, backup dipakai dan file rusak disimpan sebagai `*.corrupt-<timestamp>`.

Format:

```json
{
  "version": 2,
  "folders": ["Kerja"],
  "profiles": [
    {
      "id": "uuid",
      "name": "Project X",
      "folder": "Kerja",
      "order": 0,
      "steps": [
        { "type": "exe", "path": "C:\\laragon\\laragon.exe" },
        {
          "type": "command",
          "cwd": "C:\\project",
          "cmd": "npm run dev",
          "port": 3000,
          "shell": "auto",
          "readiness": "http",
          "readyTimeoutMs": 30000,
          "env": { "NODE_ENV": "development" },
          "autoRestart": true,
          "maxRestarts": 3
        },
        { "type": "open_url", "url": "http://localhost:3000" }
      ]
    }
  ]
}
```

## Konfigurasi lewat `conf.jsonc`

Klik **Buka conf.jsonc** di header untuk membuat/membuka file di direktori data aplikasi. File tidak dibuat otomatis saat startup (cuma `conf.example.jsonc` yang dibuat sebagai contoh). Setelah mengubah file, restart aplikasi supaya profile dimuat ulang.

```jsonc
{
  // Profil dari sini tampil dengan badge "JSON" dan read-only di UI.
  "folders": ["Kerja"],
  "profiles": [
    {
      "id": "conf-laragon",
      "name": "Laragon",
      "folder": "Kerja",
      "steps": [
        { "type": "exe", "path": "C:\\laragon\\laragon.exe" },
        {
          "type": "command",
          "cwd": "C:\\project",
          "cmd": "npm run dev",
          "port": 3000,
          "readiness": "http",
          "readyTimeoutMs": 30000
        },
        // URL lama tanpa scheme otomatis jadi http://
        { "type": "open_url", "url": "localhost:3000" }
      ]
    }
  ]
}
```

Bentuk array profile polos juga diterima. Komentar `//` / `/* */` dan trailing comma diperbolehkan. Kalau ID sama dengan profile UI, versi `conf.jsonc` yang menang. Error parse/validasi ditampilkan di panel konfigurasi (aplikasi tetap jalan).

### Untuk AI agent / edit otomatis

Kalau kamu minta agent menambah atau mengubah profile, **agent cukup mengedit `conf.jsonc`**, jangan pernah menulis `profiles.json` (itu store internal UI yang dipakai dengan lock + atomic write).

Lokasi: `%APPDATA%\com.devlauncher.desktop\conf.jsonc` (klik **Buka conf.jsonc** di UI kalau belum ada). Referensi lengkap: `conf.example.jsonc` di folder yang sama.

Aturan untuk agent:

- Pertahankan struktur `{ "folders": [...], "profiles": [...] }`; jangan hapus profile lain di file.
- `id` harus unik dan stabil (mis. `project-x`), `name` bebas, `folder` opsional, `order` opsional.
- `steps` minimal satu; tipe: `exe` (`path`), `command` (`cwd`, `cmd`, opsional `port`/`shell`/`readiness`/`readyTimeoutMs`/`env`/`autoRestart`/`maxRestarts`), `open_url` (`url` http/https).
- Setelah menyimpan, minta user **restart aplikasi** supaya profile dimuat ulang.
- Profile dari file tampil dengan badge `JSON` dan read-only di UI; untuk mengubah/hapusnya, edit kembali file yang sama.
- Kalau ID sama dengan profile UI, versi `conf.jsonc` yang menang. Error parse/validasi muncul di panel konfigurasi tanpa mematikan app.

Contoh aman untuk menambah satu profile:

```jsonc
{
  // Tambahkan entry ini ke array "profiles" yang sudah ada; jangan timpa file.
  "folders": ["Kerja"],
  "profiles": [
    {
      "id": "project-x",
      "name": "Project X",
      "folder": "Kerja",
      "steps": [
        { "type": "exe", "path": "C:\\laragon\\laragon.exe" },
        {
          "type": "command",
          "cwd": "C:\\project\\x",
          "cmd": "npm run dev",
          "port": 3000,
          "readiness": "http",
          "readyTimeoutMs": 30000
        },
        { "type": "open_url", "url": "http://localhost:3000" }
      ]
    }
  ]
}
```

## Notes

- **Windows**: `npm`/`npx` shim dijalankan via `cmd.exe /C` (Windows CreateProcess limitation)
- **Buka URL**: hanya `http://` dan `https://` yang diizinkan (dibuka lewat `ShellExecuteW`, tanpa parsing shell)
- **Kill tree**: `taskkill /T` untuk parent + seluruh child; proses diverifikasi lewat start time agar PID yang sudah di-reuse tidak ikut terbunuh
- **Tutup aplikasi**: semua proses yang dilacak dihentikan (kill tree) dan `running.json` dibersihkan; kalau app crash, journal dipakai untuk memulihkan status proses saat dibuka lagi
- **Auto port switch**: Tools seperti Next.js/Vite handle auto-increment port sendiri (3000 → 3001 → ...)
- **Bash di Windows**: memakai Git Bash kalau terdeteksi; bisa dioverride dengan env `DEV_LAUNCHER_BASH`
- **Keamanan**: CSP release ketat; mode dev pakai `devCsp` yang mengizinkan HMR. Jangan mengaktifkan `devtools` atau `dangerousDisableAssetCspModification` di release. Tombol "Open Next.js Dev Tools" yang muncul saat dev itu overlay Next.js, bukan Tauri devtools, dan tidak tampil di release
- **Belum ada**: updater, code signing, dan installer ter-sign — menunggu sertifikat/kunci + endpoint updater. Autostart, system tray, dan dark mode juga sengaja tidak ditambahkan

## License

MIT
