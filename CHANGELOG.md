# Changelog

Semua perubahan signifikan dicatat di file ini.
Format mengikuti [Keep a Changelog](https://keepachangelog.com/id/1.1.0/) dan
[Semantic Versioning](https://semver.org/lang/id/).

## [Unreleased]

### Notes
- Updater dan code signing **belum diaktifkan**: keduanya memerlukan sertifikat/kunci penandatanganan
  serta endpoint updater. Untuk saat ini yang dirilis adalah berkas `.exe` tanpa bundle.

## [0.2.0] - 2026-11-24

### Added
- Fase 3 — pengalaman pengguna & ketahanan: toast dan error inline, modal konfirmasi untuk aksi
  berisiko, log dengan wrapping baris, quick restart beserta durasi dan exit code, navigasi/a11y
  keyboard, error boundary, pemulihan window state, serta konfigurasi `conf.jsonc`.
- Fase 4 — keamanan & kualitas: CSP ketat dengan `devCsp` untuk development, devtools dimatikan di
  release, capability Tauri minimal, validasi backend terpusat, log file di release dengan rotasi,
  serta pengujian Rust (fmt/test/clippy), Vitest, dan Playwright smoke lewat CI Windows.

### Changed
- Pipeline CI Windows menjalankan lint, typecheck, unit test, build, pemeriksaan Rust, build Tauri
  `--no-bundle`, dan smoke test Playwright sebagai job terpisah.

## [0.1.0]

### Added
- Fase 1–2 — fondasi launcher: penyimpanan konfigurasi atomik, mekanisme lock untuk mencegah
  penulisan bersamaan, migrasi format konfigurasi, PID journal, single-instance guard, preflight
  sebelum menjalankan proyek, penghentian proses graceful, dan alur dasar UI Tauri + Next.js.

### Notes
- Rilis awal belum menyertakan installer, penandatanganan kode, atau pembaruan otomatis.
