pub mod journal;
mod logbus;
pub mod models;
pub mod ports;
pub mod runner;
pub mod storage;
pub mod util;

#[cfg(windows)]
mod win;

use logbus::LogBus;
use models::{ConfigStatus, Profile, RuntimeInfo};
use runner::{RunningRegistry, StartOptions};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tauri::Manager;
use tauri_plugin_log::{FileOpenStrategy, RotationStrategy, Target, TargetKind};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

/// Posisi & ukuran window terakhir (physical pixel), dipersist ke window.json.
#[derive(Serialize, Deserialize)]
struct WindowState {
    #[serde(default)]
    x: Option<i32>,
    #[serde(default)]
    y: Option<i32>,
    width: u32,
    height: u32,
}

fn window_state_path() -> std::path::PathBuf {
    storage::base_dir().join("window.json")
}

fn restore_window_state(app: &tauri::AppHandle) {
    let raw = match std::fs::read_to_string(window_state_path()) {
        Ok(raw) => raw,
        Err(_) => return,
    };
    let state: WindowState = match serde_json::from_str(&raw) {
        Ok(state) => state,
        Err(_) => return,
    };
    if state.width == 0 || state.height == 0 {
        return;
    }
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let _ = window.set_size(tauri::PhysicalSize {
        width: state.width,
        height: state.height,
    });
    if let (Some(x), Some(y)) = (state.x, state.y) {
        let _ = window.set_position(tauri::PhysicalPosition { x, y });
    }
}

fn save_window_state(app: &tauri::AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let Ok(size) = window.outer_size() else {
        return;
    };
    if size.width == 0 || size.height == 0 {
        return;
    }
    let position = window.outer_position().ok();
    let state = WindowState {
        x: position.map(|p| p.x),
        y: position.map(|p| p.y),
        width: size.width,
        height: size.height,
    };
    let _ = storage::write_json_atomic(&window_state_path(), &state);
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StartAllFailure {
    profile_id: String,
    name: String,
    error: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StartAllResult {
    /// Jumlah profile yang dipertimbangkan (termasuk yang sudah berjalan).
    total: usize,
    started: usize,
    /// Nama profile yang dilewati karena sudah berjalan.
    already_running: Vec<String>,
    failed: Vec<StartAllFailure>,
}

#[tauri::command]
fn list_profiles() -> Result<Vec<Profile>, String> {
    storage::with_store_read(|store| Ok(store.profiles.clone()))
}

#[tauri::command]
fn list_folders() -> Result<Vec<String>, String> {
    storage::with_store_read(|store| {
        let mut folders = store.folders.clone();
        folders.sort();
        Ok(folders)
    })
}

/// Status `conf.jsonc`: path, keberadaan file, dan error parse terakhir.
#[tauri::command]
fn get_config_status() -> ConfigStatus {
    storage::config_status()
}

/// Buka `conf.jsonc` di Windows Explorer. Kalau belum ada, dibuat dari
/// template kosong terlebih dahulu (contoh lengkap ada di `conf.example.jsonc`).
#[tauri::command]
fn open_config_file() -> Result<(), String> {
    let path = storage::ensure_config_file()?;
    let mut command = std::process::Command::new("explorer");
    command.arg(&path);
    #[cfg(windows)]
    command.creation_flags(util::CREATE_NO_WINDOW);
    command
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Gagal membuka {}: {e}", path.display()))
}

#[tauri::command]
fn create_folder(name: String) -> Result<(), String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("Nama folder kosong.".into());
    }
    storage::with_store_mut(|store| {
        if store.folders.iter().any(|f| f.eq_ignore_ascii_case(&name)) {
            return Err(format!("Folder '{name}' sudah ada."));
        }
        store.folders.push(name);
        store.folders.sort();
        Ok(())
    })
}

#[tauri::command]
fn rename_folder(old: String, new: String) -> Result<(), String> {
    let old = old.trim().to_string();
    let new = new.trim().to_string();
    if new.is_empty() {
        return Err("Nama folder kosong.".into());
    }
    storage::with_store_mut(|store| {
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
        Ok(())
    })
}

#[tauri::command]
fn delete_folder(name: String) -> Result<(), String> {
    let name = name.trim().to_string();
    storage::with_store_mut(|store| {
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
        Ok(())
    })
}

/// Pindahkan profile ke sebuah folder (atau `None` = Tanpa Folder).
#[tauri::command]
fn move_profile(profile_id: String, folder: Option<String>) -> Result<(), String> {
    storage::with_store_mut(|store| {
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
        Ok(())
    })
}

/// Simpan urutan profile (drag-reorder).
#[tauri::command]
fn reorder_profiles(profile_ids: Vec<String>) -> Result<(), String> {
    storage::with_store_mut(|store| {
        for (order, id) in profile_ids.iter().enumerate() {
            let p = store
                .profiles
                .iter_mut()
                .find(|p| p.id == *id)
                .ok_or_else(|| format!("Profile {id} tidak ditemukan"))?;
            p.order = order as u32;
        }
        Ok(())
    })
}

#[tauri::command]
fn save_profile(profile: Profile) -> Result<(), String> {
    let mut profile = profile;
    profile.name = profile.name.trim().to_string();
    profile.folder = profile
        .folder
        .map(|f| f.trim().to_string())
        .filter(|f| !f.is_empty());
    models::validate_profile(&profile)?;

    storage::with_store_mut(|store| {
        if let Some(folder) = &profile.folder {
            if !store.folders.iter().any(|f| f == folder) {
                store.folders.push(folder.clone());
                store.folders.sort();
            }
        }
        match store.profiles.iter_mut().find(|p| p.id == profile.id) {
            Some(existing) => {
                profile.order = existing.order;
                *existing = profile;
            }
            None => {
                let max_order = store
                    .profiles
                    .iter()
                    .filter(|p| p.folder == profile.folder)
                    .map(|p| p.order)
                    .max()
                    .unwrap_or(0);
                profile.order = max_order + 1;
                store.profiles.push(profile);
            }
        }
        Ok(())
    })
}

/// Hapus profile, log-nya, dan hentikan proses yang masih hidup.
#[tauri::command]
async fn delete_profile(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<RunningRegistry>>,
    logs: tauri::State<'_, LogBus>,
    profile_id: String,
) -> Result<(), String> {
    // Stop dulu: kalau gagal, profile tidak boleh hilang dari store sementara
    // prosesnya masih hidup (journal jadi tidak punya pemilik).
    let registry = state.inner().clone();
    let bus = logs.inner().clone();
    let id = profile_id.clone();
    tauri::async_runtime::spawn_blocking(move || {
        runner::stop_profile_blocking(&app, &registry, &bus, &id);
    })
    .await
    .map_err(|e| format!("Gagal menghentikan profile: {e}"))?;

    storage::with_store_mut(|store| {
        let before = store.profiles.len();
        store.profiles.retain(|p| p.id != profile_id);
        if store.profiles.len() == before {
            return Err(format!("Profile {profile_id} tidak ditemukan"));
        }
        Ok(())
    })?;

    logs.close_profile(&profile_id);
    let _ = storage::clear_log_file(&profile_id);
    Ok(())
}

/// Pre-flight lengkap: konflik port + error konfigurasi.
#[tauri::command]
async fn preflight_profile(profile_id: String) -> Result<models::PreflightResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        storage::with_store_read(|store| {
            let profile = store
                .profiles
                .iter()
                .find(|p| p.id == profile_id)
                .ok_or_else(|| format!("Profile {profile_id} tidak ditemukan"))?;
            runner::preflight(profile)
        })
    })
    .await
    .map_err(|e| format!("Gagal memeriksa profile: {e}"))?
}

/// Kompatibilitas: hanya daftar konflik.
#[tauri::command]
async fn check_port_conflicts(profile_id: String) -> Result<Vec<models::PortConflict>, String> {
    preflight_profile(profile_id)
        .await
        .map(|result| result.conflicts)
}

#[tauri::command]
fn read_profile_log(profile_id: String) -> Result<Vec<String>, String> {
    storage::read_log_file(&profile_id)
}

#[tauri::command]
fn clear_profile_log(logs: tauri::State<'_, LogBus>, profile_id: String) -> Result<(), String> {
    logs.close_profile(&profile_id);
    storage::clear_log_file(&profile_id)
}

#[tauri::command]
async fn start_profile(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<RunningRegistry>>,
    logs: tauri::State<'_, LogBus>,
    profile_id: String,
    port_overrides: Option<HashMap<usize, u16>>,
    force: Option<bool>,
    skip_steps: Option<Vec<usize>>,
) -> Result<(), String> {
    let options = StartOptions {
        port_overrides: port_overrides.unwrap_or_default(),
        force: force.unwrap_or(false),
        skip_steps: skip_steps.unwrap_or_default(),
    };
    let registry = state.inner().clone();
    let bus = logs.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        runner::begin_start(&app, &registry, &bus, &profile_id, options)
    })
    .await
    .map_err(|e| format!("Gagal memulai profile: {e}"))?
}

#[tauri::command]
async fn start_all(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<RunningRegistry>>,
    logs: tauri::State<'_, LogBus>,
) -> Result<StartAllResult, String> {
    let registry = state.inner().clone();
    let bus = logs.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let profiles = storage::with_store_read(|store| Ok(store.profiles.clone()))?;
        let total = profiles.len();
        log::info!("Start all: {total} profile dipertimbangkan");
        let running_ids: HashSet<String> = registry.lock().keys().cloned().collect();
        let mut already_running = Vec::new();
        let mut started = 0;
        let mut failed = Vec::new();

        for profile in profiles {
            if running_ids.contains(&profile.id) {
                already_running.push(profile.name);
                continue; // sudah jalan — biarkan apa adanya
            }
            match runner::begin_start(&app, &registry, &bus, &profile.id, StartOptions::default()) {
                Ok(()) => started += 1,
                Err(err) => {
                    let message = format!("Tidak bisa start '{}': {err}", profile.name);
                    bus.log(&profile.id, 0, "system", &message);
                    failed.push(StartAllFailure {
                        profile_id: profile.id,
                        name: profile.name,
                        error: err,
                    });
                }
            }
        }

        if !failed.is_empty() {
            log::warn!("Start all: {} profile gagal dijalankan", failed.len());
        }
        Ok::<StartAllResult, String>(StartAllResult {
            total,
            started,
            already_running,
            failed,
        })
    })
    .await
    .map_err(|e| format!("Gagal start semua profile: {e}"))?
}

#[tauri::command]
async fn stop_profile(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<RunningRegistry>>,
    logs: tauri::State<'_, LogBus>,
    profile_id: String,
) -> Result<(), String> {
    let registry = state.inner().clone();
    let bus = logs.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        runner::stop_profile_blocking(&app, &registry, &bus, &profile_id);
    })
    .await
    .map_err(|e| format!("Gagal menghentikan profile: {e}"))?;
    Ok(())
}

#[tauri::command]
async fn stop_all(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<RunningRegistry>>,
    logs: tauri::State<'_, LogBus>,
) -> Result<usize, String> {
    let ids: Vec<String> = state.lock().keys().cloned().collect();
    let registry = state.inner().clone();
    let bus = logs.inner().clone();
    let count = ids.len();
    tauri::async_runtime::spawn_blocking(move || {
        for id in ids {
            runner::stop_profile_blocking(&app, &registry, &bus, &id);
        }
    })
    .await
    .map_err(|e| format!("Gagal menghentikan semua profile: {e}"))?;
    Ok(count)
}

#[tauri::command]
fn get_runtime_state(state: tauri::State<'_, Arc<RunningRegistry>>) -> Vec<RuntimeInfo> {
    runner::runtime_state(state.inner())
}

#[tauri::command]
async fn kill_port_process(pid: u32, port: u16) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || runner::kill_port_owner(pid, port))
        .await
        .map_err(|e| format!("Gagal menghentikan proses: {e}"))?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(windows)]
    {
        if !win::acquire_single_instance() {
            win::message_box(
                "Dev Launcher sudah berjalan. Cek jendela aplikasi / taskbar.",
                "Dev Launcher",
            );
            return;
        }
    }

    let registry = Arc::new(RunningRegistry::default());

    let app = tauri::Builder::default()
        .manage(registry)
        .invoke_handler(tauri::generate_handler![
            list_profiles,
            list_folders,
            get_config_status,
            open_config_file,
            create_folder,
            rename_folder,
            delete_folder,
            move_profile,
            reorder_profiles,
            save_profile,
            delete_profile,
            ports::get_active_ports,
            ports::find_free_port,
            preflight_profile,
            check_port_conflicts,
            read_profile_log,
            clear_profile_log,
            start_profile,
            start_all,
            stop_profile,
            stop_all,
            get_runtime_state,
            kill_port_process,
        ])
        .setup(|app| {
            let data_dir = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| storage::base_dir());
            storage::init(data_dir).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

            // Plugin log aktif di debug maupun release. Target folder sengaja
            // terpisah dari `logs/` agar retention log profile tidak menghapus
            // log aplikasi. `storage::init()` sudah dipanggil di atas.
            let level = if cfg!(debug_assertions) {
                log::LevelFilter::Debug
            } else {
                log::LevelFilter::Info
            };
            app.handle().plugin(
                tauri_plugin_log::Builder::default()
                    .level(level)
                    .clear_targets()
                    .target(Target::new(TargetKind::Stdout))
                    .target(Target::new(TargetKind::Folder {
                        path: storage::base_dir().join("app-logs"),
                        file_name: Some("app".into()),
                    }))
                    .rotation_strategy(RotationStrategy::KeepSome(5))
                    .max_file_size(1_048_576)
                    .file_open_strategy(FileOpenStrategy::Append)
                    .build(),
            )?;

            if let Err(err) = storage::migrate_legacy() {
                log::warn!("Gagal memigrasi data lama: {err}");
            }
            if let Err(err) = storage::ensure_config_example() {
                log::warn!("Gagal menulis contoh konfigurasi: {err}");
            }
            if let Err(err) = storage::enforce_log_retention(50 * 1024 * 1024, 14) {
                log::warn!("Gagal membersihkan log profile lama: {err}");
            }
            runner::purge_temp_scripts();
            restore_window_state(app.handle());

            log::info!(
                "Dev Launcher mulai (mode: {})",
                if cfg!(debug_assertions) {
                    "debug"
                } else {
                    "release"
                }
            );
            if storage::config_status().error.is_some() {
                // Isi conf.jsonc bisa memuat data sensitif; hanya catat fakta
                // bahwa konfigurasi invalid, detailnya ada di status aplikasi.
                log::warn!("conf.jsonc tidak valid; cek status konfigurasi di aplikasi.");
            }

            // Pulihkan status proses yang masih hidup dari sesi sebelumnya.
            let registry = app.state::<Arc<RunningRegistry>>().inner().clone();
            runner::reconcile(&registry);

            let bus = logbus::init(app.handle().clone());
            app.manage(bus);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    let registry = app.state::<Arc<RunningRegistry>>().inner().clone();
    app.run(move |app_handle, event| {
        if let tauri::RunEvent::Exit = event {
            log::info!("Dev Launcher keluar; menyimpan state dan menghentikan proses.");
            // Simpan window state lebih dulu supaya tetap tersimpan walaupun
            // state LogBus tidak tersedia saat Exit.
            save_window_state(app_handle);

            // `setup()` baru jalan saat `app.run()`, jadi state LogBus belum
            // ada di sini kalau diakses sebelum run. Ambil dari handle saat
            // event Exit (setup sudah pasti selesai).
            let Some(bus) = app_handle.try_state::<LogBus>() else {
                return;
            };
            let bus = bus.inner().clone();

            // Cegah restart task menyalakan proses baru, lalu siram antrean log.
            runner::begin_shutdown();
            tauri::async_runtime::block_on(bus.shutdown());

            let pids: Vec<u32> = {
                let map = registry.lock();
                map.values()
                    .flat_map(|rp| rp.pids.iter().copied())
                    .collect()
            };
            let pid_count = pids.len();
            for pid in pids {
                runner::kill_tree(pid);
            }
            log::info!("Shutdown: {pid_count} proses dilacak untuk dihentikan.");
            // Entri yang masih hidup (mis. taskkill ditolak) disimpan supaya
            // bisa direkonsiliasi / dihentikan saat app dibuka lagi.
            let remaining = journal::load()
                .entries
                .into_iter()
                .filter(|entry| !runner::alive_pids(&[entry.pid]).is_empty())
                .collect::<Vec<_>>();
            journal::replace(remaining);
        }
    });
}
