use crate::models::Profile;
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

pub fn load_profiles() -> Result<Vec<Profile>, String> {
    let path = profiles_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = fs::read_to_string(&path).map_err(|e| format!("Gagal membaca profil: {e}"))?;
    let list: Vec<Profile> =
        serde_json::from_str(&raw).map_err(|e| format!("Format profil tidak valid: {e}"))?;
    Ok(list)
}

pub fn save_profiles(profiles: &[Profile]) -> Result<(), String> {
    let path = profiles_path()?;
    let raw = serde_json::to_string_pretty(profiles)
        .map_err(|e| format!("Gagal serialize profil: {e}"))?;
    fs::write(&path, raw).map_err(|e| format!("Gagal menulis profil: {e}"))?;
    Ok(())
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}
