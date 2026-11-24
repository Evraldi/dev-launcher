# Panduan Rilis

Target rilis saat ini: **Windows x86_64**, aplikasi Tauri v2 + Next.js 16.

## 1. Prasyarat

- Node.js 22 dan npm (lockfile: `package-lock.json`).
- Rust stable dengan komponen `rustfmt` dan `clippy`.
- Dependensi terpasang: `npm ci`.

## 2. Samakan versi

Versi saat ini **0.2.0**. Untuk rilis berikutnya, samakan versi di ketiga berkas berikut:

| Berkas | Kunci |
| --- | --- |
| `package.json` | `"version"` |
| `src-tauri/Cargo.toml` | `version` di `[package]` |
| `src-tauri/tauri.conf.json` | `"version"` |

Jalankan `npm install` bila `package-lock.json` perlu menyinkronkan versi root.

## 3. Update CHANGELOG

- `0.2.0` sudah tercatat di `CHANGELOG.md` dengan tanggal 2026-11-24; tidak perlu dipindah lagi.
- Untuk versi berikutnya, pindahkan catatan dari `## [Unreleased]` ke bagian versi baru dan kelompokkan di bawah `Added` / `Changed` / `Fixed` sesuai format Keep a Changelog.
- Pertahankan catatan bahwa updater dan code signing belum diaktifkan sampai ada kunci/sertifikat.

## 4. Verifikasi lokal

```powershell
npm run lint
npm run typecheck
npm run test:unit
npm run build
npm run test:e2e
cd src-tauri
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
cd ..
```

## 5. Build

```powershell
npm run tauri build -- --no-bundle
```

Artifact: `src-tauri/target/release/dev-launcher.exe`. CI mengunggah berkas yang sama dengan nama
`dev-launcher-windows-x86_64-<sha>` (retensi 14 hari), seperti didefinisikan di
`.github/workflows/ci.yml`.

## 6. Catatan

Installer (MSI/NSIS), code signing, dan updater otomatis **menyusul** setelah tersedia sertifikat
penandatanganan dan endpoint updater. Sampai saat itu, distribusikan berkas `.exe` tanpa bundle.
