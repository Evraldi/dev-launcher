# Planning: Dev Launcher (One-Click Project Starter)

## Overview
Aplikasi desktop untuk menjalankan seluruh proses yang dibutuhkan sebuah project (start Laragon, `npm run dev`, buka VS Code, buka browser, dll) sekaligus lewat satu tombol. Dilengkapi live log viewer per project, status indicator, dan deteksi bentrok port sebelum start.

## Tech Stack
- **Backend/Shell**: Tauri v2 (Rust) — process spawning, kill, event streaming
- **Frontend**: Next.js (App Router, TypeScript, Tailwind CSS), static export
- **Process management**: `std::process::Command` / `tokio::process` (Rust)
- **Port detection**: reuse logic dari project [[port-monitor-tool]] — command `get_active_ports()` yang sama
- **Storage**: dua berkas JSON lokal di `%APPDATA%\com.devlauncher.desktop\` — `profiles.json` (store internal UI, atomic + backup) dan `conf.jsonc` (override eksternal yang boleh diedit user/AI agent), tidak perlu database

## Setup / Install Commands

Prasyarat (kalau belum ada dari setup port-monitor sebelumnya, skip yang sudah terpasang):
```bash
# 1. Cek Rust
rustc --version
cargo --version

# 2. Cek Tauri CLI (di project ini pakai CLI lewat npm, bukan cargo tauri)
npx tauri --version
```

Catatan: `cargo tauri` tidak terpasang di mesin ini. Selalu pakai `npm run tauri dev` / `npm run tauri build` dari root project.

Riwayat pembuatan project (sudah dilakukan, tidak perlu dijalankan ulang):
```bash
# 1. Buat folder & masuk
cd D:\projek
mkdir dev-launcher
cd dev-launcher

# 2. Init Next.js
npx create-next-app@latest .
# pilihan: TypeScript Yes, ESLint Yes, React Compiler No,
# Tailwind Yes, src/ dir Yes, App Router Yes, import alias default @/*

# 3. Init Tauri di folder yang sama (dulu memakai cargo tauri init;
#    sekarang cukup pakai npm run tauri ... dari root project)
cargo tauri init
# App name: dev-launcher
# Window title: dev launcher
# Web assets: ../out
# Dev server URL: http://localhost:3000
# Dev command: npm run dev
# Build command: npm run build
```

Tambahan dependency (setelah project ke-generate):
```bash
cd src-tauri
```
Tambahkan di `Cargo.toml`:
```toml
[dependencies]
sysinfo = "0.32"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["full"] }
uuid = { version = "1", features = ["v4"] }
```

Frontend (dari root project):
```bash
npm install @tauri-apps/api
```

Setting `next.config.ts` untuk static export:
```typescript
const nextConfig: NextConfig = {
  output: "export",
  images: { unoptimized: true },
};
```

## Data Model

Dua berkas, bukan duplikat data:

| Berkas | Ditulis oleh | Fungsi |
| --- | --- | --- |
| `%APPDATA%\com.devlauncher.desktop\profiles.json` | UI aplikasi | Store internal (schema v2, atomic write + backup + recovery) |
| `%APPDATA%\com.devlauncher.desktop\conf.jsonc` | User / AI agent | Profile & folder eksternal, read-only di UI, boleh pakai komentar & trailing comma |

Aturan penting: agent cukup mengedit `conf.jsonc` untuk menambah profile/folder ke project tertentu; **jangan** menulis `profiles.json`. Kalau ID sama, versi `conf.jsonc` yang menang. Setelah edit, restart aplikasi. Format lengkap ada di README bagian "Konfigurasi lewat `conf.jsonc`" dan di `conf.example.jsonc`.

## Fitur Inti

### 1. Manajemen profile
- Tambah/edit/hapus profile project
- Tiap profile berisi list step (jalankan exe, jalankan command dengan cwd tertentu, buka URL)
- Step bertipe `command` bisa ditandai port yang dipakai, buat keperluan cek bentrok

### 2. Start / Stop
- Tombol "Start" jalanin semua step berurutan, simpan PID tiap child process yang di-spawn
- Tombol "Stop" kill semua PID terkait profile itu
- Status per profile: `stopped` / `running` / `failed` (ditampilkan via status dot)

### 3. Deteksi bentrok port (fitur utama)
- Sebelum menjalankan step bertipe `command` yang punya `port`, panggil `get_active_ports()`
- Kalau port sudah dipakai, tampilkan banner/modal peringatan dengan 3 pilihan:
  - **Kill & Continue** — kill proses yang pegang port, lanjut start
  - **Ganti port** — kalau command support env var port, jalankan dengan port alternatif
  - **Batal** — hentikan proses start untuk profile itu

### 4. Live log viewer
- Tiap child process, stdout/stderr di-stream ke frontend lewat Tauri event (`emit`/`listen`)
- Panel log per profile, bisa switch tampilan antar profile yang sedang aktif

### 5. Status indicator
- Dot hijau (running), abu (stopped), merah (failed/error)
- Checklist step yang sudah selesai vs yang masih jalan

## Struktur Tauri Commands (Rust)

```rust
#[tauri::command]
async fn start_profile(profile_id: String) -> Result<(), String>

#[tauri::command]
async fn stop_profile(profile_id: String) -> Result<(), String>

#[tauri::command]
fn get_active_ports() -> Vec<PortInfo> // reuse dari port-monitor

#[tauri::command]
fn kill_process(pid: u32) -> Result<(), String>

#[tauri::command]
fn list_profiles() -> Vec<Profile>

#[tauri::command]
fn save_profile(profile: Profile) -> Result<(), String>

#[tauri::command]
fn delete_profile(profile_id: String) -> Result<(), String>
```

## Urutan Pengerjaan (Disarankan)
1. Setup project (sudah ada command di atas)
2. Backend: struct `Profile`/`Step`, command `list_profiles`/`save_profile`/`delete_profile` (read/write JSON lokal)
3. Frontend: UI form untuk bikin/edit profile
4. Backend: command `start_profile` — spawn process berurutan sesuai step, simpan PID
5. Backend: streaming stdout/stderr via Tauri event
6. Frontend: card list profile + log panel (kayak mockup yang sudah didesain)
7. Integrasi deteksi port bentrok (reuse `get_active_ports` dari project port-monitor)
8. Fitur stop/kill process
9. Testing: `npm run tauri dev`
10. Build: `npm run tauri build`

## Catatan
- Kalau project [[port-monitor-tool]] sudah selesai duluan, logic Rust buat baca port (`netstat -ano` parsing) tinggal di-copy langsung ke project ini — nggak perlu bikin ulang dari nol.
- Auto-kill proses sebaiknya selalu minta konfirmasi user dulu, jangan otomatis diam-diam (bisa berbahaya kalau proses itu bukan dev server sembarangan).
- Step bertipe `command` yang portnya hardcoded di kode (bukan lewat env var) tidak bisa auto-ganti port — user harus edit source project itu sendiri.

## Status Implementasi (v0.2.0)

- **Fase 1–2**: atomic storage + backup/recovery, store lock, schema v2 & migrasi app-data, PID journal + reconcile, single instance, safe URL, kill-tree + verifikasi, log cleanup, start non-blocking, preflight, stop/start all, skip-existing, log batching.
- **Fase 3**: toast/error inline, modal konfirmasi + unsaved guard, LogPanel pause/search/copy/export/wrap, busy per profile + quick restart + durasi/exit code, a11y/ARIA/focus trap + shortcut `Alt+↑/↓`, error boundary/empty/loading state, window state, `conf.jsonc`.
- **Fase 4**: CSP ketat + `devCsp`, devtools mati di release, capability ACL minimal, validasi input backend terpusat, app log release dengan rotasi, test Rust/Vitest/Playwright, CI Windows, `CHANGELOG.md` + `docs/RELEASING.md`.
- **Sengaja tidak masuk**: autostart, system tray, dark mode, updater, code signing, installer ter-sign (butuh sertifikat/kunci + endpoint updater).
