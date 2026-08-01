mod models;
mod ports;
mod runner;
mod storage;
mod util;

use models::Profile;
use runner::RunningRegistry;
use std::sync::Arc;

/// Ambil semua profile dari penyimpanan lokal.
#[tauri::command]
fn list_profiles() -> Result<Vec<Profile>, String> {
    storage::load_profiles()
}

/// Simpan profile baru / update profile yang sudah ada.
#[tauri::command]
fn save_profile(profile: Profile) -> Result<(), String> {
    let mut profiles = storage::load_profiles()?;
    match profiles.iter_mut().find(|p| p.id == profile.id) {
        Some(existing) => *existing = profile,
        None => profiles.push(profile),
    }
    storage::save_profiles(&profiles)
}

/// Hapus profile berdasarkan id.
#[tauri::command]
fn delete_profile(profile_id: String) -> Result<(), String> {
    let mut profiles = storage::load_profiles()?;
    let before = profiles.len();
    profiles.retain(|p| p.id != profile_id);
    if profiles.len() == before {
        return Err(format!("Profile {profile_id} tidak ditemukan"));
    }
    storage::save_profiles(&profiles)
}

/// Pre-flight cek bentrok port sebelum menjalankan profile.
#[tauri::command]
fn check_port_conflicts(profile_id: String) -> Result<Vec<models::PortConflict>, String> {
    let profiles = storage::load_profiles()?;
    let profile = profiles
        .iter()
        .find(|p| p.id == profile_id)
        .ok_or_else(|| format!("Profile {profile_id} tidak ditemukan"))?;
    runner::find_conflicts(profile)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(Arc::new(RunningRegistry::default()))
        .invoke_handler(tauri::generate_handler![
            list_profiles,
            save_profile,
            delete_profile,
            ports::get_active_ports,
            ports::kill_process,
            check_port_conflicts,
            runner::start_profile,
            runner::stop_profile,
        ])
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
