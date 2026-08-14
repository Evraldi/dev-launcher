mod models;
mod ports;
mod runner;
mod storage;
mod util;

use models::Profile;
use runner::RunningRegistry;
use std::sync::Arc;
use tauri::Emitter;
use tauri::Manager;

/// Ambil semua profile dari penyimpanan lokal.
#[tauri::command]
fn list_profiles() -> Result<Vec<Profile>, String> {
    Ok(storage::load_store()?.profiles)
}

/// Ambil daftar folder (urut abjad).
#[tauri::command]
fn list_folders() -> Result<Vec<String>, String> {
    let mut folders = storage::load_store()?.folders;
    folders.sort();
    Ok(folders)
}

/// Buat folder kosong baru.
#[tauri::command]
fn create_folder(name: String) -> Result<(), String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("Nama folder kosong.".into());
    }
    let mut store = storage::load_store()?;
    if store.folders.iter().any(|f| f.eq_ignore_ascii_case(&name)) {
        return Err(format!("Folder '{name}' sudah ada."));
    }
    store.folders.push(name);
    store.folders.sort();
    storage::save_store(&store)
}

/// Rename folder beserta seluruh profil di dalamnya.
#[tauri::command]
fn rename_folder(old: String, new: String) -> Result<(), String> {
    let old = old.trim().to_string();
    let new = new.trim().to_string();
    if new.is_empty() {
        return Err("Nama folder kosong.".into());
    }
    let mut store = storage::load_store()?;
    if !store.folders.iter().any(|f| f == &old) {
        return Err(format!("Folder '{old}' tidak ditemukan."));
    }
    if store
        .folders
        .iter()
        .any(|f| f != &old && f.eq_ignore_ascii_case(&new))
    {
        return Err(format!("Folder '{new}' sudah ada."));
    }
    if old == new {
        return Ok(());
    }
    for f in store.folders.iter_mut() {
        if f == &old {
            *f = new.clone();
        }
    }
    for p in store.profiles.iter_mut() {
        if p.folder.as_deref() == Some(old.as_str()) {
            p.folder = Some(new.clone());
        }
    }
    store.folders.sort();
    storage::save_store(&store)
}

/// Hapus folder; profil di dalamnya dipindah ke "Tanpa Folder".
#[tauri::command]
fn delete_folder(name: String) -> Result<(), String> {
    let name = name.trim().to_string();
    let mut store = storage::load_store()?;
    let before = store.folders.len();
    store.folders.retain(|f| f != &name);
    if store.folders.len() == before {
        return Err(format!("Folder '{name}' tidak ditemukan."));
    }
    for p in store.profiles.iter_mut() {
        if p.folder.as_deref() == Some(name.as_str()) {
            p.folder = None;
        }
    }
    storage::save_store(&store)
}

/// Pindahkan profile ke sebuah folder (atau `None` = Tanpa Folder).
/// Profil ditaruh di urutan paling bawah folder tujuan.
#[tauri::command]
fn move_profile(profile_id: String, folder: Option<String>) -> Result<(), String> {
    let mut store = storage::load_store()?;
    let idx = store
        .profiles
        .iter()
        .position(|p| p.id == profile_id)
        .ok_or_else(|| format!("Profile {profile_id} tidak ditemukan"))?;
    let target = folder
        .map(|f| f.trim().to_string())
        .filter(|f| !f.is_empty());
    if let Some(ref t) = target {
        if !store.folders.iter().any(|f| f == t) {
            store.folders.push(t.clone());
            store.folders.sort();
        }
    }
    let max_order = store
        .profiles
        .iter()
        .filter(|p| p.folder == target)
        .map(|p| p.order)
        .max()
        .unwrap_or(0);
    store.profiles[idx].folder = target;
    store.profiles[idx].order = max_order + 1;
    storage::save_store(&store)
}

/// Simpan urutan profile (drag-reorder). Order di-assign 0..n sesuai urutan id.
#[tauri::command]
fn reorder_profiles(profile_ids: Vec<String>) -> Result<(), String> {
    let mut store = storage::load_store()?;
    for (order, id) in profile_ids.iter().enumerate() {
        let p = store
            .profiles
            .iter_mut()
            .find(|p| p.id == *id)
            .ok_or_else(|| format!("Profile {id} tidak ditemukan"))?;
        p.order = order as u32;
    }
    storage::save_store(&store)
}

/// Simpan profile baru / update profile yang sudah ada.
#[tauri::command]
fn save_profile(profile: Profile) -> Result<(), String> {
    let mut store = storage::load_store()?;
    match store.profiles.iter_mut().find(|p| p.id == profile.id) {
        Some(existing) => *existing = profile,
        None => store.profiles.push(profile),
    }
    storage::save_store(&store)
}

/// Hapus profile berdasarkan id. Kalau profile masih punya proses hidup,
/// bunuh semua proses tersebut dulu (hindari orphan setelah profile hilang).
#[tauri::command]
fn delete_profile(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<RunningRegistry>>,
    profile_id: String,
) -> Result<(), String> {
    let mut store = storage::load_store()?;
    let before = store.profiles.len();
    store.profiles.retain(|p| p.id != profile_id);
    if store.profiles.len() == before {
        return Err(format!("Profile {profile_id} tidak ditemukan"));
    }
    storage::save_store(&store)?;

    let killed = {
        let mut map = state
            .processes
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let pids = map
            .remove(&profile_id)
            .map(|rp| rp.pids)
            .unwrap_or_default();
        for pid in &pids {
            runner::kill_tree(*pid);
        }
        pids
    };

    let _ = app.emit(
        "profile-status",
        runner::status_payload(
            &profile_id,
            "stopped",
            format!("Profile dihapus ({} proses dihentikan).", killed.len()),
            None,
        ),
    );

    Ok(())
}

/// Pre-flight cek bentrok port sebelum menjalankan profile.
#[tauri::command]
fn check_port_conflicts(profile_id: String) -> Result<Vec<models::PortConflict>, String> {
    let store = storage::load_store()?;
    let profile = store
        .profiles
        .iter()
        .find(|p| p.id == profile_id)
        .ok_or_else(|| format!("Profile {profile_id} tidak ditemukan"))?;
    runner::find_conflicts(profile)
}

/// Ambil log yang sudah dipersist ke disk untuk sebuah profile.
#[tauri::command]
fn read_profile_log(profile_id: String) -> Result<Vec<String>, String> {
    storage::read_log_file(&profile_id)
}

/// Hapus log yang sudah dipersist untuk sebuah profile.
#[tauri::command]
fn clear_profile_log(profile_id: String) -> Result<(), String> {
    storage::clear_log_file(&profile_id)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .manage(Arc::new(RunningRegistry::default()))
        .invoke_handler(tauri::generate_handler![
            list_profiles,
            list_folders,
            create_folder,
            rename_folder,
            delete_folder,
            move_profile,
            reorder_profiles,
            save_profile,
            delete_profile,
            ports::get_active_ports,
            ports::kill_process,
            ports::find_free_port,
            check_port_conflicts,
            read_profile_log,
            clear_profile_log,
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
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    // Saat app keluar, bunuh semua proses yang masih hidup (bukan orphan).
    let registry = app.state::<Arc<RunningRegistry>>().inner().clone();
    app.run(move |_app_handle, event| {
        if let tauri::RunEvent::Exit = event {
            let pids: Vec<u32> = {
                let map = registry
                    .processes
                    .lock()
                    .unwrap_or_else(|p| p.into_inner());
                map.values().flat_map(|rp| rp.pids.iter().copied()).collect()
            };
            for pid in pids {
                runner::kill_tree(pid);
            }
        }
    });
}
