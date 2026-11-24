use crate::models::{ConfigStatus, Profile, Store, STORE_VERSION};
use serde::Deserialize;
use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Direktori data aplikasi (di-set sekali saat startup lewat `init`).
static BASE_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Lock tunggal untuk semua operasi baca-modif-tulis store. Ini mencegah
/// lost update kalau dua command jalan hampir bersamaan.
static STORE_LOCK: Mutex<()> = Mutex::new(());

const PROFILES_FILE: &str = "profiles.json";
const BACKUP_SUFFIX: &str = ".bak";
const CONFIG_FILE: &str = "conf.jsonc";
const CONFIG_EXAMPLE_FILE: &str = "conf.example.jsonc";

/// Error parse/validasi konfigurasi eksternal terakhir (bukan error fatal:
/// aplikasi tetap jalan memakai profil dari `profiles.json`).
static CONFIG_ERROR: OnceLock<Mutex<Option<String>>> = OnceLock::new();

/// Inisialisasi lokasi data. `base` biasanya `app_data_dir()` dari Tauri.
pub fn init(base: PathBuf) -> Result<(), String> {
    fs::create_dir_all(&base)
        .map_err(|e| format!("Gagal membuat direktori data {}: {e}", base.display()))?;
    let _ = BASE_DIR.set(base);
    Ok(())
}

/// Direktori data aktif. Fallback ke `~/.dev-launcher` kalau `init` belum
/// dipanggil (mis. pada unit test helper lama).
pub fn base_dir() -> PathBuf {
    BASE_DIR.get().cloned().unwrap_or_else(legacy_dir)
}

fn legacy_dir() -> PathBuf {
    dirs_home()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".dev-launcher")
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

// ---------------------------------------------------------------------------
// Konfigurasi eksternal (conf.jsonc)
// ---------------------------------------------------------------------------

/// Path berkas konfigurasi yang diedit user.
pub fn config_path() -> PathBuf {
    base_dir().join(CONFIG_FILE)
}

fn config_example_path() -> PathBuf {
    base_dir().join(CONFIG_EXAMPLE_FILE)
}

fn set_config_error(error: Option<String>) {
    let slot = CONFIG_ERROR.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = slot.lock() {
        *guard = error;
    }
}

/// Status berkas konfigurasi untuk ditampilkan ke frontend.
pub fn config_status() -> ConfigStatus {
    let path = config_path();
    let error = CONFIG_ERROR
        .get()
        .and_then(|slot| slot.lock().ok())
        .and_then(|guard| guard.clone());
    ConfigStatus {
        path: path.to_string_lossy().into_owned(),
        exists: path.exists(),
        error,
    }
}

/// Buang komentar `//` dan `/* ... */` di luar string, lalu buang trailing
/// comma sebelum `}` / `]`. String seperti `"http://localhost:3000"` aman.
fn strip_jsonc(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;

    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }

        if c == '"' {
            in_string = true;
            out.push(c);
            continue;
        }

        if c == '/' {
            match chars.peek().copied() {
                Some('/') => {
                    chars.next();
                    // Buang sampai akhir baris, tapi pertahankan newline
                    // supaya posisi error tetap masuk akal.
                    for next in chars.by_ref() {
                        if next == '\n' {
                            out.push('\n');
                            break;
                        }
                    }
                    continue;
                }
                Some('*') => {
                    chars.next();
                    let mut prev = '\0';
                    for next in chars.by_ref() {
                        if prev == '*' && next == '/' {
                            break;
                        }
                        if next == '\n' {
                            out.push('\n');
                        }
                        prev = next;
                    }
                    continue;
                }
                _ => {}
            }
        }
        out.push(c);
    }

    strip_trailing_commas(&out)
}

fn strip_trailing_commas(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    let mut in_string = false;
    let mut escaped = false;

    while i < chars.len() {
        let c = chars[i];
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if c == '"' {
            in_string = true;
            out.push(c);
            i += 1;
            continue;
        }
        if c == ',' {
            let mut next = i + 1;
            while next < chars.len() && chars[next].is_whitespace() {
                next += 1;
            }
            if next < chars.len() && matches!(chars[next], '}' | ']') {
                i += 1;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

#[derive(Deserialize, Default)]
struct ConfigObject {
    #[serde(default)]
    folders: Vec<String>,
    #[serde(default)]
    profiles: Vec<Profile>,
}

fn parse_config(raw: &str) -> Result<(Vec<String>, Vec<Profile>), String> {
    let cleaned = strip_jsonc(raw);
    let value: serde_json::Value =
        serde_json::from_str(&cleaned).map_err(|e| format!("conf.jsonc tidak valid: {e}"))?;

    if value.is_array() {
        let profiles: Vec<Profile> = serde_json::from_value(value)
            .map_err(|e| format!("conf.jsonc: daftar profile tidak valid: {e}"))?;
        return Ok((Vec::new(), profiles));
    }
    if value.is_object() {
        let config: ConfigObject = serde_json::from_value(value)
            .map_err(|e| format!("conf.jsonc: konfigurasi tidak valid: {e}"))?;
        return Ok((config.folders, config.profiles));
    }
    Err("conf.jsonc harus berupa object atau array profile.".into())
}

fn validate_config_profiles(profiles: &[Profile]) -> Result<(), String> {
    let mut ids: HashSet<&str> = HashSet::new();
    for profile in profiles {
        // Aturan field memakai validator yang sama dengan jalur UI.
        crate::models::validate_profile(profile)
            .map_err(|err| format!("conf.jsonc: profile '{}': {err}", profile.id))?;
        // Duplikat id hanya relevan untuk berkas konfigurasi eksternal.
        if !ids.insert(profile.id.as_str()) {
            return Err(format!("conf.jsonc: id '{}' duplikat.", profile.id));
        }
    }
    Ok(())
}

fn apply_external_config_inner(store: &mut Store, path: &Path) -> Result<(), String> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Berkas dihapus: profile external lama tidak boleh ikut tersimpan.
            store.profiles.retain(|p| !p.external);
            return Ok(());
        }
        Err(e) => return Err(format!("Gagal membaca {}: {e}", path.display())),
    };

    let (folders, mut profiles) = parse_config(&raw)?;
    for profile in &mut profiles {
        profile.external = true;
    }
    // Versi lama dipakai supaya `migrate()` selalu menormalkan URL lama.
    let mut config_store = Store {
        version: STORE_VERSION - 1,
        folders,
        profiles,
    };
    config_store.migrate();
    let profiles = config_store.profiles;
    validate_config_profiles(&profiles)?;

    let config_ids: HashSet<&str> = profiles.iter().map(|p| p.id.as_str()).collect();
    // Profil config menang atas profile UI dengan ID yang sama, dan profile
    // external lama selalu diganti dari berkas.
    store
        .profiles
        .retain(|p| !p.external && !config_ids.contains(p.id.as_str()));
    store.profiles.extend(profiles);

    let mut merged_folders = config_store.folders;
    merged_folders.extend(store.folders.iter().cloned());
    merged_folders.extend(store.profiles.iter().filter_map(|p| p.folder.clone()));
    merged_folders.sort();
    merged_folders.dedup();
    store.folders = merged_folders;
    Ok(())
}

/// Terapkan `conf.jsonc` setelah store dibaca. Error tidak fatal: profil dari
/// `profiles.json` tetap dipakai dan error disimpan untuk command status.
pub(crate) fn apply_external_config(store: &mut Store, dir: &Path) {
    let path = dir.join(CONFIG_FILE);
    match apply_external_config_inner(store, &path) {
        Ok(()) => set_config_error(None),
        Err(err) => set_config_error(Some(err)),
    }
}

const CONFIG_EXAMPLE: &str = r#"// Dev Launcher - contoh konfigurasi eksternal (JSONC).
// Salin/sunting berkas ini menjadi conf.jsonc untuk menambah profile.
// Profile dari conf.jsonc bersifat read-only di UI dan menggantikan profile
// dengan ID yang sama. Komentar dan trailing comma diperbolehkan.
{
  "folders": ["Contoh"], // opsional
  "profiles": [
    {
      "id": "external-example",
      "name": "Contoh External",
      "folder": "Contoh",
      "order": 0,
      "steps": [
        { "type": "exe", "path": "C:\\Path\\Ke\\App.exe" },
        { "type": "command", "cwd": ".", "cmd": "npm run dev", "port": 3000 },
        // Buka browser setelah server siap
        { "type": "open_url", "url": "http://localhost:3000" },
      ]
    }
  ]
}
"#;

/// Pastikan contoh konfigurasi ada. Tidak pernah membuat `conf.jsonc` otomatis.
pub fn ensure_config_example() -> Result<(), String> {
    let path = config_example_path();
    if path.exists() {
        return Ok(());
    }
    write_atomic(&path, CONFIG_EXAMPLE.as_bytes())
}

/// Template `conf.jsonc` baru: sengaja kosong supaya tidak menyisipkan
/// profile contoh yang gagal dijalankan. Contoh lengkap tetap ada di
/// `conf.example.jsonc`.
const CONFIG_TEMPLATE: &str = r#"// Dev Launcher - konfigurasi profile eksternal.
// Tambahkan profile ke array "profiles" lalu restart aplikasi.
// Contoh lengkap: conf.example.jsonc
// Komentar // dan /* */ serta trailing comma diperbolehkan.
{
  "folders": [],
  "profiles": []
}
"#;

/// Pastikan `conf.jsonc` ada, memakai template kosong kalau belum ada.
pub fn ensure_config_file() -> Result<PathBuf, String> {
    let path = config_path();
    if path.exists() {
        return Ok(path);
    }
    write_atomic(&path, CONFIG_TEMPLATE.as_bytes())?;
    Ok(path)
}

/// Salin data dari lokasi lama ke lokasi baru kalau lokasi baru masih kosong.
/// Sumber yang dicek: direktori app-data identifier lama
/// (`%APPDATA%\com.devlauncher.app`) dan `~/.dev-launcher`.
/// Dipanggil sekali saat startup.
pub fn migrate_legacy() -> Result<bool, String> {
    let target = base_dir();
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(appdata) = std::env::var_os("APPDATA") {
        candidates.push(PathBuf::from(appdata).join("com.devlauncher.app"));
    }
    candidates.push(legacy_dir());

    for legacy in candidates {
        if legacy == target || !legacy.join(PROFILES_FILE).exists() {
            continue;
        }
        if target.join(PROFILES_FILE).exists() {
            return Ok(false);
        }
        copy_legacy_dir(&legacy, &target)?;
        return Ok(true);
    }
    Ok(false)
}

fn copy_legacy_dir(legacy: &Path, target: &Path) -> Result<(), String> {
    fs::create_dir_all(target)
        .map_err(|e| format!("Gagal membuat direktori data {}: {e}", target.display()))?;
    fs::copy(legacy.join(PROFILES_FILE), target.join(PROFILES_FILE))
        .map_err(|e| format!("Gagal memindahkan profil lama: {e}"))?;

    let legacy_logs = legacy.join("logs");
    if legacy_logs.is_dir() {
        let target_logs = target.join("logs");
        fs::create_dir_all(&target_logs)
            .map_err(|e| format!("Gagal membuat direktori log: {e}"))?;
        if let Ok(entries) = fs::read_dir(&legacy_logs) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if let Some(name) = path.file_name() {
                        let _ = fs::copy(&path, target_logs.join(name));
                    }
                }
            }
        }
    }
    Ok(())
}

fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(BACKUP_SUFFIX);
    path.with_file_name(name)
}

fn corrupt_path(path: &Path) -> PathBuf {
    let ts = crate::util::timestamp_compact();
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".corrupt-{ts}"));
    path.with_file_name(name)
}

/// Tulis file secara atomic: tulis ke temp, fsync, backup file lama, lalu
/// rename. Kalau proses mati di tengah penulisan, file lama tetap utuh.
pub(crate) fn write_atomic(path: &Path, contents: &[u8]) -> Result<(), String> {
    write_atomic_with_backup(path, contents, true)
}

/// `backup: false` dipakai saat recovery: file utama yang rusak sudah
/// dikarantina, jadi menyalinnya ke `.bak` hanya akan merusak cadangan bagus.
fn write_atomic_with_backup(path: &Path, contents: &[u8], backup: bool) -> Result<(), String> {
    let dir = path
        .parent()
        .ok_or_else(|| "Path tanpa direktori induk".to_string())?;
    fs::create_dir_all(dir).map_err(|e| format!("Gagal membuat {}: {e}", dir.display()))?;

    let tmp = path.with_file_name(format!(
        ".{}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy()
    ));
    {
        let mut file =
            fs::File::create(&tmp).map_err(|e| format!("Gagal menulis {}: {e}", tmp.display()))?;
        file.write_all(contents)
            .map_err(|e| format!("Gagal menulis {}: {e}", tmp.display()))?;
        file.sync_all()
            .map_err(|e| format!("Gagal flush {}: {e}", tmp.display()))?;
    }

    if backup && path.exists() {
        let _ = fs::copy(path, backup_path(path));
    }
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("Gagal menyimpan {}: {e}", path.display())
    })
}

/// Jalankan operasi baca-modif-tulis dengan lock global.
pub fn with_store_mut<T, F>(f: F) -> Result<T, String>
where
    F: FnOnce(&mut Store) -> Result<T, String>,
{
    let _guard = STORE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut store = load_store()?;
    store.migrate();
    let out = f(&mut store)?;
    save_store(&store)?;
    Ok(out)
}

/// Jalankan operasi baca dengan lock global (konsisten dengan penulis).
pub fn with_store_read<T, F>(f: F) -> Result<T, String>
where
    F: FnOnce(&Store) -> Result<T, String>,
{
    let _guard = STORE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let store = load_store()?;
    f(&store)
}

pub fn load_store() -> Result<Store, String> {
    let dir = base_dir();
    load_store_from(&dir)
}

pub fn load_store_from(dir: &Path) -> Result<Store, String> {
    let path = dir.join(PROFILES_FILE);
    let mut store = if !path.exists() {
        Store::default()
    } else {
        match read_store(&path) {
            Ok(mut store) => {
                store.migrate();
                store
            }
            Err(primary_err) => {
                // Karantina file rusak SEBELUM menulis. Kalau tidak,
                // `write_atomic` akan menyalin file rusak itu ke `.bak` dan
                // cadangan bagus hilang.
                let quarantined = corrupt_path(&path);
                let _ = fs::rename(&path, &quarantined);

                let backup = backup_path(&path);
                if let Ok(mut recovered) = read_store(&backup) {
                    recovered.migrate();
                    save_store_to_with_backup(dir, &recovered, false)?;
                    recovered
                } else {
                    return Err(format!(
                        "File profil rusak dan backup tidak bisa dibaca: {primary_err}. \
                         Salinan file rusak disimpan di {}.",
                        quarantined.display()
                    ));
                }
            }
        }
    };

    // config selalu jadi source of truth, termasuk saat store kosong atau
    // baru dipulihkan dari backup. `with_store_mut` menyimpan hasil gabungan
    // ini (termasuk flag `external`) saat closure selesai.
    apply_external_config(&mut store, dir);
    Ok(store)
}

fn read_store(path: &Path) -> Result<Store, String> {
    let raw =
        fs::read_to_string(path).map_err(|e| format!("Gagal membaca {}: {e}", path.display()))?;

    // Format baru: object dengan `folders` + `profiles`.
    if let Ok(store) = serde_json::from_str::<Store>(&raw) {
        if store.version > STORE_VERSION {
            return Err(format!(
                "Skema profil versi {} belum didukung (app ini mendukung hingga v{STORE_VERSION}).",
                store.version
            ));
        }
        // Semua field punya default, jadi `{}` (atau file yang terpotong
        // menjadi objek kosong) bisa tersamar jadi "tidak ada profile" dan
        // ditimpa saat save berikutnya. Anggap itu korupsi.
        if raw.trim() == "{}" {
            return Err("File profil kosong/tidak valid.".into());
        }
        return Ok(store);
    }

    // Migrasi legacy: dulu berkas berisi array `Profile` polos.
    let list: Vec<Profile> =
        serde_json::from_str(&raw).map_err(|e| format!("Format profil tidak valid: {e}"))?;
    Ok(Store {
        // Biarkan `Store::migrate()` yang menaikkan ke STORE_VERSION supaya
        // URL lama seperti `localhost:3000` ikut dinormalisasi.
        version: STORE_VERSION - 1,
        folders: derive_folders(&list),
        profiles: normalize_orders(list),
    })
}

fn normalize_orders(mut profiles: Vec<Profile>) -> Vec<Profile> {
    // Pastikan order unik & berurutan supaya drag-reorder legacy tetap stabil.
    profiles.sort_by(|a, b| {
        (a.folder.as_deref().unwrap_or(""))
            .cmp(b.folder.as_deref().unwrap_or(""))
            .then(a.order.cmp(&b.order))
            .then(a.name.cmp(&b.name))
    });
    let mut last_folder: Option<Option<String>> = None;
    let mut next = 0u32;
    for p in profiles.iter_mut() {
        if last_folder.as_ref() != Some(&p.folder) {
            last_folder = Some(p.folder.clone());
            next = 0;
        }
        p.order = next;
        next += 1;
    }
    profiles
}

pub fn save_store(store: &Store) -> Result<(), String> {
    save_store_to(&base_dir(), store)
}

pub fn save_store_to(dir: &Path, store: &Store) -> Result<(), String> {
    save_store_to_with_backup(dir, store, true)
}

fn save_store_to_with_backup(dir: &Path, store: &Store, backup: bool) -> Result<(), String> {
    let path = dir.join(PROFILES_FILE);
    let raw =
        serde_json::to_string_pretty(store).map_err(|e| format!("Gagal serialize profil: {e}"))?;
    write_atomic_with_backup(&path, raw.as_bytes(), backup)
}

/// Kumpulkan nama folder yang dipakai dari daftar profile (untuk migrasi legacy).
fn derive_folders(profiles: &[Profile]) -> Vec<String> {
    let mut folders: Vec<String> = profiles.iter().filter_map(|p| p.folder.clone()).collect();
    folders.sort();
    folders.dedup();
    folders
}

/// Tulis JSON lain (mis. journal) secara atomic di direktori data.
pub(crate) fn write_json_atomic(path: &Path, value: &impl serde::Serialize) -> Result<(), String> {
    let raw = serde_json::to_vec_pretty(value)
        .map_err(|e| format!("Gagal serialize {}: {e}", path.display()))?;
    write_atomic(path, &raw)
}

/// Nama file log yang aman (id dari frontend tidak dipercaya mentah-mentah).
pub fn safe_log_stem(profile_id: &str) -> String {
    let clean: String = profile_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if clean.is_empty() {
        "profile".to_string()
    } else {
        clean
    }
}

/// Lokasi direktori log: `<data>/logs/`
pub fn logs_dir() -> Result<PathBuf, String> {
    let dir = base_dir().join("logs");
    fs::create_dir_all(&dir).map_err(|e| format!("Gagal membuat {}: {e}", dir.display()))?;
    Ok(dir)
}

pub fn log_path(profile_id: &str) -> Result<PathBuf, String> {
    Ok(logs_dir()?.join(format!("{}.log", safe_log_stem(profile_id))))
}

/// Baca semua baris log yang sudah dipersist untuk sebuah profile.
pub fn read_log_file(profile_id: &str) -> Result<Vec<String>, String> {
    let path = log_path(profile_id)?;
    let path = if path.exists() {
        path
    } else {
        // Setelah rotasi, file lama ada di `.1.log`.
        let rotated = rotated_log_path(&path);
        if rotated.exists() {
            rotated
        } else {
            return Ok(Vec::new());
        }
    };
    let raw = fs::read_to_string(&path).map_err(|e| format!("Gagal membaca log: {e}"))?;
    Ok(raw.lines().map(crate::util::strip_ansi).collect())
}

pub(crate) fn rotated_log_path(path: &Path) -> PathBuf {
    path.with_extension("1.log")
}

/// Hapus file log sebuah profile (termasuk hasil rotasi).
pub fn clear_log_file(profile_id: &str) -> Result<(), String> {
    let path = log_path(profile_id)?;
    remove_if_exists(&path)?;
    remove_if_exists(&rotated_log_path(&path))?;
    Ok(())
}

fn remove_if_exists(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("Gagal menghapus {}: {e}", path.display())),
    }
}

/// Bersihkan log lama: hapus yang lebih tua dari `max_age_days`, lalu kalau
/// total masih melebihi `max_total_bytes`, hapus yang paling lama dulu.
pub fn enforce_log_retention(max_total_bytes: u64, max_age_days: u64) -> Result<(), String> {
    enforce_log_retention_excluding(max_total_bytes, max_age_days, &HashSet::new())
}

/// Sama seperti `enforce_log_retention`, tapi tidak menghapus file log yang
/// sedang dibuka `LogSink` (di Windows, menghapus file yang terbuka membuat
/// tulisan berikutnya masuk ke file tak terlihat).
pub fn enforce_log_retention_excluding(
    max_total_bytes: u64,
    max_age_days: u64,
    active_ids: &HashSet<String>,
) -> Result<(), String> {
    let dir = logs_dir()?;
    let mut files: Vec<(PathBuf, u64, std::time::SystemTime)> = Vec::new();
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let stem = name.split('.').next().unwrap_or("");
        if active_ids.iter().any(|id| safe_log_stem(id) == stem) {
            continue;
        }
        let modified = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
        files.push((path, meta.len(), modified));
    }

    let now = std::time::SystemTime::now();
    let max_age = std::time::Duration::from_secs(max_age_days * 24 * 60 * 60);
    files.retain(|(path, _, modified)| {
        let old = now.duration_since(*modified).unwrap_or_default() > max_age;
        if old {
            let _ = fs::remove_file(path);
        }
        !old
    });

    let total: u64 = files.iter().map(|(_, len, _)| *len).sum();
    if total > max_total_bytes {
        files.sort_by_key(|(_, _, modified)| *modified);
        let mut current = total;
        for (path, len, _) in files {
            if current <= max_total_bytes {
                break;
            }
            if fs::remove_file(&path).is_ok() {
                current = current.saturating_sub(len);
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Step;

    /// Serialisasi test yang menyentuh `CONFIG_ERROR` (global).
    static CONFIG_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn load_store_serial(dir: &Path) -> Result<Store, String> {
        let _guard = CONFIG_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        load_store_from(dir)
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("dev-launcher-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample_profile(id: &str) -> Profile {
        Profile {
            id: id.to_string(),
            name: "Test".into(),
            folder: None,
            order: 0,
            external: false,
            steps: vec![Step::Command {
                cwd: ".".into(),
                cmd: "echo hi".into(),
                port: Some(3000),
                shell: None,
                env: None,
                readiness: None,
                ready_timeout_ms: None,
                delay_ms: None,
                optional: None,
                auto_restart: None,
                max_restarts: None,
            }],
        }
    }

    #[test]
    fn roundtrip_store() {
        let dir = temp_dir("roundtrip");
        let store = Store {
            version: STORE_VERSION,
            folders: vec!["Kerja".into()],
            profiles: vec![sample_profile("a")],
        };
        save_store_to(&dir, &store).unwrap();
        let loaded = load_store_serial(&dir).unwrap();
        assert_eq!(loaded.version, STORE_VERSION);
        assert_eq!(loaded.folders, vec!["Kerja".to_string()]);
        assert_eq!(loaded.profiles.len(), 1);
        assert!(dir.join("profiles.json").exists());
    }

    #[test]
    fn recovers_from_corrupt_file() {
        let dir = temp_dir("corrupt");
        let store = Store {
            version: STORE_VERSION,
            folders: vec![],
            profiles: vec![sample_profile("b")],
        };
        save_store_to(&dir, &store).unwrap();
        // Simpan sekali lagi supaya file lama tersalin ke .bak, lalu rusak.
        save_store_to(&dir, &store).unwrap();
        fs::write(dir.join("profiles.json"), b"{ not json").unwrap();

        let loaded = load_store_serial(&dir).unwrap();
        assert_eq!(loaded.profiles.len(), 1);
        assert!(fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .any(|e| e.file_name().to_string_lossy().contains(".corrupt-")));
    }

    #[test]
    fn migrates_legacy_array() {
        let dir = temp_dir("legacy");
        let legacy = serde_json::to_string(&vec![sample_profile("c")]).unwrap();
        fs::write(dir.join("profiles.json"), legacy).unwrap();
        let loaded = load_store_serial(&dir).unwrap();
        assert_eq!(loaded.version, STORE_VERSION);
        assert_eq!(loaded.profiles.len(), 1);
    }

    #[test]
    fn missing_file_is_empty_store() {
        let dir = temp_dir("missing");
        let loaded = load_store_serial(&dir).unwrap();
        assert!(loaded.profiles.is_empty());
        assert_eq!(loaded.version, STORE_VERSION);
    }

    #[test]
    fn sanitizes_log_names() {
        assert_eq!(safe_log_stem("../../etc/passwd"), "etcpasswd");
        assert_eq!(safe_log_stem(""), "profile");
    }

    #[test]
    fn recovery_does_not_destroy_good_backup() {
        let dir = temp_dir("recovery-backup");
        let store = Store {
            version: STORE_VERSION,
            folders: vec![],
            profiles: vec![sample_profile("keep")],
        };
        save_store_to(&dir, &store).unwrap();
        save_store_to(&dir, &store).unwrap(); // isi `.bak` = data bagus
        fs::write(dir.join("profiles.json"), b"{ not json").unwrap();

        let loaded = load_store_serial(&dir).unwrap();
        assert_eq!(loaded.profiles.len(), 1);
        assert_eq!(loaded.profiles[0].id, "keep");
        let backup_raw = fs::read_to_string(dir.join("profiles.json.bak")).unwrap();
        let backup: Store = serde_json::from_str(&backup_raw).unwrap();
        assert_eq!(backup.profiles[0].id, "keep");
    }

    #[test]
    fn legacy_array_normalizes_urls() {
        let dir = temp_dir("legacy-url");
        let mut profile = sample_profile("legacy");
        profile.steps = vec![Step::OpenUrl {
            url: "localhost:3000".into(),
        }];
        fs::write(
            dir.join("profiles.json"),
            serde_json::to_string(&vec![profile]).unwrap(),
        )
        .unwrap();

        let loaded = load_store_serial(&dir).unwrap();
        assert!(matches!(
            &loaded.profiles[0].steps[0],
            Step::OpenUrl { url } if url == "http://localhost:3000"
        ));
    }

    #[test]
    fn rejects_empty_and_future_schema() {
        let dir = temp_dir("schema-guard");
        fs::write(dir.join("profiles.json"), b"{}").unwrap();
        assert!(load_store_serial(&dir).is_err());

        let store = Store {
            version: STORE_VERSION + 1,
            ..Store::default()
        };
        save_store_to(&dir, &store).unwrap();
        assert!(load_store_serial(&dir).is_err());
    }

    #[test]
    fn strip_jsonc_handles_comments_strings_and_trailing_commas() {
        let raw = r#"{
            // komentar satu baris
            "url": "http://localhost:3000", /* komentar blok */
            "text": "a // b /* c */",
            "list": [1, 2,],
        }"#;
        let cleaned = strip_jsonc(raw);
        let value: serde_json::Value = serde_json::from_str(&cleaned).unwrap();
        assert_eq!(value["url"], "http://localhost:3000");
        assert_eq!(value["text"], "a // b /* c */");
        assert_eq!(value["list"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn example_config_is_valid_jsonc() {
        let (folders, profiles) = parse_config(CONFIG_EXAMPLE).unwrap();
        assert!(folders.iter().any(|f| f == "Contoh"));
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].steps.len(), 3);
        assert_eq!(profiles[0].steps[0].kind(), "exe");
        assert_eq!(profiles[0].steps[1].kind(), "command");
        assert_eq!(profiles[0].steps[2].kind(), "open_url");
    }

    #[test]
    fn array_config_adds_external_profiles() {
        let _guard = CONFIG_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let dir = temp_dir("config-array");
        let mut profile = sample_profile("ext");
        profile.steps = vec![Step::OpenUrl {
            url: "localhost:3000".into(),
        }];
        fs::write(
            dir.join(CONFIG_FILE),
            serde_json::to_string(&vec![profile]).unwrap(),
        )
        .unwrap();

        let loaded = load_store_from(&dir).unwrap();
        assert_eq!(loaded.profiles.len(), 1);
        assert!(loaded.profiles[0].external);
        assert!(matches!(
            &loaded.profiles[0].steps[0],
            Step::OpenUrl { url } if url == "http://localhost:3000"
        ));
    }

    #[test]
    fn config_profile_replaces_ui_profile_with_same_id() {
        let _guard = CONFIG_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let dir = temp_dir("config-replace");
        let mut ui = sample_profile("dup");
        ui.name = "Dari UI".into();
        ui.folder = Some("UI".into());
        let mut store = Store {
            version: STORE_VERSION,
            folders: vec!["UI".into()],
            profiles: vec![ui, sample_profile("ui-only")],
        };
        save_store_to(&dir, &store).unwrap();

        let raw = r#"{
            "folders": ["Cfg", "UI",],
            "profiles": [{
                "id": "dup", "name": "Dari Config", "folder": "Cfg",
                "steps": [{ "type": "exe", "path": "app.exe" }]
            }]
        }"#;
        fs::write(dir.join(CONFIG_FILE), raw).unwrap();

        let loaded = load_store_from(&dir).unwrap();
        assert_eq!(loaded.profiles.len(), 2);
        let config_profile = loaded.profiles.iter().find(|p| p.id == "dup").unwrap();
        assert_eq!(config_profile.name, "Dari Config");
        assert!(config_profile.external);
        assert!(
            !loaded
                .profiles
                .iter()
                .find(|p| p.id == "ui-only")
                .unwrap()
                .external
        );
        assert_eq!(loaded.folders, vec!["Cfg".to_string(), "UI".to_string()]);

        // Save biasa tidak boleh membuang profile external.
        store = loaded;
        save_store_to(&dir, &store).unwrap();
        let saved_raw = fs::read_to_string(dir.join(PROFILES_FILE)).unwrap();
        let saved: Store = serde_json::from_str(&saved_raw).unwrap();
        assert_eq!(saved.profiles.len(), 2);
        assert!(saved.profiles.iter().any(|p| p.id == "dup" && p.external));
    }

    #[test]
    fn config_error_is_recorded_and_cleared() {
        let _guard = CONFIG_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let dir = temp_dir("config-error");
        fs::write(dir.join(CONFIG_FILE), b"{ not really json").unwrap();
        let loaded = load_store_from(&dir).unwrap();
        assert!(loaded.profiles.is_empty());
        let status = config_status();
        assert!(status.error.is_some(), "error parse harus tercatat");
        assert!(
            status.error.unwrap_or_default().contains("conf.jsonc"),
            "error harus menyebut nama berkas"
        );

        fs::write(dir.join(CONFIG_FILE), b"{ \"profiles\": [] }").unwrap();
        let _ = load_store_from(&dir).unwrap();
        assert!(config_status().error.is_none());
    }

    #[test]
    fn config_jsonc_syntax_error_is_reported() {
        let raw = r#"{
            // komentar diperbolehkan
            "profiles": [{ "id": "p", "name": "P", "steps": [ }]
        }"#;
        let err = parse_config(raw).unwrap_err();
        assert!(err.contains("conf.jsonc tidak valid"), "{err}");
    }

    #[test]
    fn config_rejects_invalid_profile_fields() {
        let raw = r#"{
            "profiles": [{
                "id": "ext",
                "name": "External",
                "steps": [{
                    "type": "command",
                    "cwd": ".",
                    "cmd": "echo hi",
                    "shell": "fish",
                    "port": 0
                }]
            }]
        }"#;
        let (_, profiles) = parse_config(raw).unwrap();
        let err = validate_config_profiles(&profiles).unwrap_err();
        assert!(err.contains("conf.jsonc"), "{err}");
        assert!(err.contains("shell"), "{err}");

        let duplicate = r#"{
            "profiles": [
                { "id": "dup", "name": "A", "steps": [{ "type": "exe", "path": "a.exe" }] },
                { "id": "dup", "name": "B", "steps": [{ "type": "exe", "path": "b.exe" }] }
            ]
        }"#;
        let (_, profiles) = parse_config(duplicate).unwrap();
        assert!(validate_config_profiles(&profiles)
            .unwrap_err()
            .contains("duplikat"));
    }

    #[test]
    fn retention_does_not_delete_app_logs_directory() {
        // `BASE_DIR` di-set sekali; tidak ada test lain yang memanggil `init`.
        let base = temp_dir("retention-app-logs");
        init(base.clone()).unwrap();
        let app_logs = base.join("app-logs");
        fs::create_dir_all(&app_logs).unwrap();
        let app_log = app_logs.join("app.log");
        fs::write(&app_log, b"application log").unwrap();

        let old_profile_log = logs_dir().unwrap().join("profile.log");
        fs::write(&old_profile_log, b"old profile log").unwrap();

        // Age 0 hari + limit 0 byte: semua file di `logs/` harus terhapus.
        enforce_log_retention_excluding(0, 0, &HashSet::new()).unwrap();

        assert!(app_log.exists(), "log aplikasi tidak boleh ikut terhapus");
        assert!(!old_profile_log.exists(), "log profile lama harus terhapus");
    }
}
