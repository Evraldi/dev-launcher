use crate::models::{Profile, Store};
use std::fs;
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

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}
