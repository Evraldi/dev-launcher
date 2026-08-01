// Menjalankan step-step sebuah profile secara berurutan,
// menyimpan PID child process yang masih hidup, dan
// streaming stdout/stderr ke frontend lewat event Tauri.
use crate::models::{PortConflict, Profile, Step};
use crate::ports::get_active_ports;
use crate::util;
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command as TokioCommand;

/// PID child process yang masih hidup per profile.
#[derive(Default)]
pub struct RunningRegistry {
    pub processes: Mutex<HashMap<String, Vec<u32>>>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct LogPayload {
    profile_id: String,
    step_index: usize,
    source: String, // "stdout" | "stderr" | "system"
    message: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct StatusPayload {
    profile_id: String,
    status: String, // "starting" | "running" | "stopped" | "failed"
    message: String,
    step_index: Option<usize>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StepPid {
    pub step_index: usize,
    pub pid: u32,
    pub label: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StartResult {
    pub pids: Vec<StepPid>,
}

/// Pre-flight: cek semua port yang dipakai step bertipe command.
pub fn find_conflicts(profile: &Profile) -> Result<Vec<PortConflict>, String> {
    let active = get_active_ports()?;
    let mut conflicts: Vec<PortConflict> = Vec::new();

    for (idx, step) in profile.steps.iter().enumerate() {
        if let Step::Command {
            port: Some(port), ..
        } = step
        {
            let used: Vec<_> = active
                .iter()
                .filter(|p| p.port == *port && p.status == "LISTENING")
                .collect();
            if !used.is_empty() {
                conflicts.push(PortConflict {
                    step_index: idx,
                    port: *port,
                    pids: used.iter().map(|p| p.pid).collect(),
                    process_names: used.iter().map(|p| p.process_name.clone()).collect(),
                });
            }
        }
    }

    Ok(conflicts)
}

fn emit_log(app: &AppHandle, payload: LogPayload) {
    let _ = app.emit("profile-log", payload);
}

fn emit_status(app: &AppHandle, payload: StatusPayload) {
    let _ = app.emit("profile-status", payload);
}

fn emit_log_plain(app: &AppHandle, profile_id: &str, step_index: usize, source: &str, msg: &str) {
    emit_log(
        app,
        LogPayload {
            profile_id: profile_id.to_string(),
            step_index,
            source: source.to_string(),
            message: msg.to_string(),
        },
    );
}

async fn spawn_command_step(
    app: AppHandle,
    profile_id: &str,
    step_index: usize,
    cwd: &str,
    cmd: &str,
    port_override: Option<u16>,
    registry: Arc<RunningRegistry>,
) -> Result<u32, String> {
    let mut split = cmd.split_whitespace();
    let program = split
        .next()
        .ok_or_else(|| format!("Step {step_index}: perintah kosong"))?;
    let args: Vec<String> = split.map(|s| s.to_string()).collect();

    if !Path::new(cwd).exists() {
        return Err(format!(
            "Step {step_index}: direktori tidak ditemukan: {cwd}"
        ));
    }

    let mut command = TokioCommand::new(program);
    command.args(&args);
    command.current_dir(cwd);
    command.stdin(Stdio::null());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    if let Some(port) = port_override {
        // Vite/Next.js: PORT. Laravel: APP_PORT (server artisan).
        command.env("PORT", port.to_string());
        command.env("APP_PORT", port.to_string());
    }

    let mut child = command
        .spawn()
        .map_err(|e| format!("Step {step_index}: gagal spawn `{program}`: {e}"))?;

    let pid = child.id().unwrap_or(0);
    {
        let mut map = registry
            .processes
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        map.entry(profile_id.to_string()).or_default().push(pid);
    }

    let label = format!("{} {}", program, args.join(" "));
    emit_log_plain(
        &app,
        profile_id,
        step_index,
        "system",
        &format!("[step {step_index}] mulai: {label} (pid {pid})"),
    );

    // Stream stdout & stderr.
    if let Some(out) = child.stdout.take() {
        let app2 = app.clone();
        let pid2 = profile_id.to_string();
        let mut lines = BufReader::new(out).lines();
        tokio::spawn(async move {
            while let Ok(Some(line)) = lines.next_line().await {
                emit_log_plain(&app2, &pid2, step_index, "stdout", &line);
            }
        });
    }
    if let Some(err) = child.stderr.take() {
        let app2 = app.clone();
        let pid2 = profile_id.to_string();
        let mut lines = BufReader::new(err).lines();
        tokio::spawn(async move {
            while let Ok(Some(line)) = lines.next_line().await {
                emit_log_plain(&app2, &pid2, step_index, "stderr", &line);
            }
        });
    }

    // Hapus PID dari registry saat process keluar.
    let registry2 = registry.clone();
    let pid_str = profile_id.to_string();
    tokio::spawn(async move {
        let _ = child.wait().await;
        if let Ok(mut map) = registry2.processes.lock() {
            if let Some(pids) = map.get_mut(&pid_str) {
                pids.retain(|p| *p != pid);
                if pids.is_empty() {
                    map.remove(&pid_str);
                }
            }
        }
    });

    Ok(pid)
}

/// Jalankan semua step sebuah profile secara berurutan.
#[tauri::command]
pub async fn start_profile(
    app: AppHandle,
    state: tauri::State<'_, Arc<RunningRegistry>>,
    profile_id: String,
) -> Result<StartResult, String> {
    let profiles = crate::storage::load_profiles()?;
    let profile = profiles
        .iter()
        .find(|p| p.id == profile_id)
        .ok_or_else(|| format!("Profile {profile_id} tidak ditemukan"))?
        .clone();

    emit_status(
        &app,
        StatusPayload {
            profile_id: profile_id.clone(),
            status: "starting".into(),
            message: "Memulai profile…".into(),
            step_index: None,
        },
    );

    let mut pids: Vec<StepPid> = Vec::new();
    let registry = state.inner().clone();

    for (idx, step) in profile.steps.iter().enumerate() {
        emit_status(
            &app,
            StatusPayload {
                profile_id: profile_id.clone(),
                status: "starting".into(),
                message: format!("Menjalankan step {idx} ({})", step.kind()),
                step_index: Some(idx),
            },
        );

        match step {
            Step::Exe { path } => {
                if !Path::new(path).exists() {
                    emit_status(
                        &app,
                        StatusPayload {
                            profile_id: profile_id.clone(),
                            status: "failed".into(),
                            message: format!("Step {idx}: file tidak ditemukan: {path}"),
                            step_index: Some(idx),
                        },
                    );
                    return Err(format!("Step {idx}: file tidak ditemukan: {path}"));
                }
                match std::process::Command::new(path).spawn() {
                    Ok(child) => {
                        let pid = child.id();
                        pids.push(StepPid {
                            step_index: idx,
                            pid,
                            label: path.clone(),
                        });
                        {
                            let mut map = registry
                                .processes
                                .lock()
                                .unwrap_or_else(|p| p.into_inner());
                            map.entry(profile_id.clone())
                                .or_default()
                                .push(pid);
                        }
                        emit_log_plain(
                            &app,
                            &profile_id,
                            idx,
                            "system",
                            &format!("[step {idx}] exe: {path} (pid {pid})"),
                        );
                    }
                    Err(e) => {
                        emit_status(
                            &app,
                            StatusPayload {
                                profile_id: profile_id.clone(),
                                status: "failed".into(),
                                message: format!("Step {idx}: gagal spawn exe: {e}"),
                                step_index: Some(idx),
                            },
                        );
                        return Err(format!("Step {idx}: gagal spawn exe: {e}"));
                    }
                }
            }
            Step::Command { cwd, cmd, port } => {
                let pid = spawn_command_step(
                    app.clone(),
                    &profile_id,
                    idx,
                    cwd,
                    cmd,
                    *port,
                    registry.clone(),
                )
                .await?;
                pids.push(StepPid {
                    step_index: idx,
                    pid,
                    label: cmd.clone(),
                });
            }
            Step::OpenUrl { url } => {
                emit_log_plain(
                    &app,
                    &profile_id,
                    idx,
                    "system",
                    &format!("[step {idx}] membuka URL: {url}"),
                );
                let _ = open_url(url);
            }
        }
    }

    emit_status(
        &app,
        StatusPayload {
            profile_id: profile_id.clone(),
            status: "running".into(),
            message: "Semua step selesai dijalankan.".into(),
            step_index: None,
        },
    );

    Ok(StartResult { pids })
}

/// Kill semua child process milik sebuah profile.
#[tauri::command]
pub fn stop_profile(
    app: AppHandle,
    state: tauri::State<'_, Arc<RunningRegistry>>,
    profile_id: String,
) -> Result<(), String> {
    let killed = {
        let mut map = state
            .processes
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let pids = map.remove(&profile_id).unwrap_or_default();
        let mut killed = Vec::new();
        if !pids.is_empty() {
            // Satu kali refresh sysinfo untuk semua PID.
            let mut sys = sysinfo::System::new_all();
            sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
            for pid in pids {
                match sys.process(sysinfo::Pid::from_u32(pid)) {
                    Some(proc) => {
                        let _ = proc.kill();
                    }
                    None => {
                        let _ = crate::ports::kill_process(pid);
                    }
                }
                killed.push(pid);
            }
        }
        killed
    };

    emit_status(
        &app,
        StatusPayload {
            profile_id,
            status: "stopped".into(),
            message: format!("Dihentikan ({} proses).", killed.len()),
            step_index: None,
        },
    );

    Ok(())
}

/// Helper agar tidak memunculkan jendela console pada Windows.
#[cfg(windows)]
fn open_url(url: &str) -> std::io::Result<std::process::Child> {
    use std::os::windows::process::CommandExt;
    std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .creation_flags(util::CREATE_NO_WINDOW)
        .spawn()
}

#[cfg(not(windows))]
fn open_url(url: &str) -> std::io::Result<std::process::Child> {
    #[cfg(target_os = "macos")]
    let open = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "linux")]
    let open = std::process::Command::new("xdg-open").arg(url).spawn();
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let open = Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "unsupported platform",
    ));
    open
}
