use crate::models::{Profile, Store};
use std::fs;
use std::io::Write;
use std::path::PathBuf;

/// Lokasi berkas profil: `~/.dev-launcher/profiles.json`
pub fn profiles_path() -> Result<PathBuf, String> {
    let dir = dirs_home()
        .ok_or_else(|| "Gagal menemukan direktori home".to_string())?
        .join(".dev-launcher");
    fs::create_dir_all(&dir).map_err(|e| format!("Gagal membuat {}: {e}", dir.display()))?;
    Ok(dir.join("profiles.json"))
}

pub fn load_store() -> Result<Store, String> {
    let path = profiles_path()?;
    if !path.exists() {
        return Ok(Store::default());
    }
    let raw = fs::read_to_string(&path).map_err(|e| format!("Gagal membaca profil: {e}"))?;

    // Format baru: object dengan `folders` + `profiles`.
    if let Ok(store) = serde_json::from_str::<Store>(&raw) {
        return Ok(store);
    }

    // Migrasi legacy: dulu berkas berisi array `Profile` polos.
    let list: Vec<Profile> =
        serde_json::from_str(&raw).map_err(|e| format!("Format profil tidak valid: {e}"))?;
    Ok(Store {
        folders: derive_folders(&list),
        profiles: list,
    })
}

pub fn save_store(store: &Store) -> Result<(), String> {
    let path = profiles_path()?;
    let raw = serde_json::to_string_pretty(store)
        .map_err(|e| format!("Gagal serialize profil: {e}"))?;
    fs::write(&path, raw).map_err(|e| format!("Gagal menulis profil: {e}"))?;
    Ok(())
}

/// Kumpulkan nama folder yang dipakai dari daftar profile (untuk migrasi legacy).
fn derive_folders(profiles: &[Profile]) -> Vec<String> {
    let mut folders: Vec<String> = profiles
        .iter()
        .filter_map(|p| p.folder.clone())
        .collect();
    folders.sort();
    folders.dedup();
    folders
}

pub fn load_profiles() -> Result<Vec<Profile>, String> {
    Ok(load_store()?.profiles)
}

const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;

/// Lokasi direktori log: `~/.dev-launcher/logs/`
pub fn logs_dir() -> Result<PathBuf, String> {
    let dir = dirs_home()
        .ok_or_else(|| "Gagal menemukan direktori home".to_string())?
        .join(".dev-launcher")
        .join("logs");
    fs::create_dir_all(&dir).map_err(|e| format!("Gagal membuat {}: {e}", dir.display()))?;
    Ok(dir)
}

pub fn log_path(profile_id: &str) -> Result<PathBuf, String> {
    Ok(logs_dir()?.join(format!("{profile_id}.log")))
}

/// Append satu baris log ke file `<profile_id>.log`. File di-rotate saat
/// melebihi 5MB (file lama disimpan sebagai `.1.log`). Gagal menulis log
/// tidak pernah memblokir alur utama — cukup di-skip.
pub fn append_log_line(profile_id: &str, step_index: usize, source: &str, message: &str) {
    let Ok(path) = log_path(profile_id) else {
        return;
    };
    if let Ok(meta) = fs::metadata(&path) {
        if meta.len() > MAX_LOG_BYTES {
            let backup = path.with_extension("1.log");
            let _ = fs::remove_file(&backup);
            let _ = fs::rename(&path, backup);
        }
    }
    let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(&path) else {
        return;
    };
    let ts = crate::util::timestamp_now();
    let _ = writeln!(file, "[{ts}] [step {step_index}] [{source}] {message}");
}

/// Baca semua baris log yang sudah dipersist untuk sebuah profile.
pub fn read_log_file(profile_id: &str) -> Result<Vec<String>, String> {
    let path = log_path(profile_id)?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = fs::read_to_string(&path).map_err(|e| format!("Gagal membaca log: {e}"))?;
    Ok(raw.lines().map(|l| l.to_string()).collect())
}

/// Hapus file log sebuah profile (kalau ada).
pub fn clear_log_file(profile_id: &str) -> Result<(), String> {
    let path = log_path(profile_id)?;
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("Gagal menghapus log: {e}")),
    }
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}
