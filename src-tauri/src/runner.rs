// Menjalankan step-step sebuah profile secara berurutan,
// menyimpan PID child process yang masih hidup, dan
// streaming stdout/stderr ke frontend lewat event Tauri.
use crate::models::{PortConflict, Profile, Step};
use crate::ports::get_active_ports;
use crate::util;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use tokio::io::BufReader;
use tokio::net::TcpStream;
use tokio::process::Command as TokioCommand;

/// Profil yang sedang berjalan: daftar PID child + flag apakah urutan
/// start sudah selesai (untuk deteksi "keluar sendiri" tanpa false positive
/// saat start masih berlangsung).
#[derive(Default)]
pub struct RunningProfile {
    pub pids: Vec<u32>,
    pub started: bool,
}

/// PID child process yang masih hidup per profile.
#[derive(Default)]
pub struct RunningRegistry {
    pub processes: Mutex<HashMap<String, RunningProfile>>,
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
pub(crate) struct StatusPayload {
    profile_id: String,
    status: String, // "starting" | "running" | "stopped" | "failed"
    message: String,
    step_index: Option<usize>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct StepStatusPayload {
    profile_id: String,
    step_index: usize,
    state: String, // "spawned" | "ready" | "timeout"
    port: Option<u16>,
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
            // TCP: hanya peduli status LISTENING. UDP tidak punya status, tapi
            // tetap dianggap bentrok kalau port dipakai. PID 0 (System Idle /
            // listener kernel) tidak bisa dibunuh dan tak relevan sebagai konflik.
            let used: Vec<_> = active
                .iter()
                .filter(|p| {
                    p.port == *port
                        && p.pid != 0
                        && (p.protocol == "UDP" || p.status == "LISTENING")
                })
                .collect();
            if !used.is_empty() {
                let mut pids = used.iter().map(|p| p.pid).collect::<Vec<_>>();
                pids.sort_unstable();
                pids.dedup();
                conflicts.push(PortConflict {
                    step_index: idx,
                    port: *port,
                    pids,
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

fn emit_step_status(app: &AppHandle, payload: StepStatusPayload) {
    let _ = app.emit("profile-step-status", payload);
}

/// Tunggu sampai port bisa di-connect (poll tiap 500ms) atau timeout.
/// Coba `127.0.0.1` dulu, lalu `::1` (server yang bind ke 0.0.0.0 tetap
/// kebaca lewat loopback).
async fn wait_for_port(port: u16, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
            return true;
        }
        if TcpStream::connect(("::1", port)).await.is_ok() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// Health check port sebuah step command: emit `spawned` → poll → `ready`/`timeout`.
/// Timeout hanya log peringatan, profile tetap lanjut.
async fn check_step_port(
    app: &AppHandle,
    profile_id: &str,
    step_index: usize,
    port: u16,
) {
    emit_step_status(
        app,
        StepStatusPayload {
            profile_id: profile_id.to_string(),
            step_index,
            state: "spawned".into(),
            port: Some(port),
        },
    );
    let ready = wait_for_port(port, Duration::from_secs(30)).await;
    let state = if ready { "ready" } else { "timeout" };
    emit_step_status(
        app,
        StepStatusPayload {
            profile_id: profile_id.to_string(),
            step_index,
            state: state.into(),
            port: Some(port),
        },
    );
    if ready {
        emit_log_plain(
            app,
            profile_id,
            step_index,
            "system",
            &format!("[step {step_index}] port {port} siap menerima koneksi."),
        );
    } else {
        emit_log_plain(
            app,
            profile_id,
            step_index,
            "system",
            &format!(
                "[step {step_index}] port {port} belum siap dalam 30 detik — lanjut ke step berikutnya."
            ),
        );
    }
}

/// Helper agar modul lain (mis. `lib.rs`) bisa mengirim status profile.
pub(crate) fn status_payload(
    profile_id: &str,
    status: &str,
    message: String,
    step_index: Option<usize>,
) -> StatusPayload {
    StatusPayload {
        profile_id: profile_id.to_string(),
        status: status.to_string(),
        message,
        step_index,
    }
}

fn emit_log_plain(app: &AppHandle, profile_id: &str, step_index: usize, source: &str, msg: &str) {
    crate::storage::append_log_line(profile_id, step_index, source, msg);
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

/// Baca stream stdout/stderr baris per baris dengan fallback lossy untuk byte
/// non-UTF8 (umum terjadi pada output Windows). Tanpa ini, satu baris invalid
/// akan menghentikan seluruh streaming karena `AsyncBufReadExt::lines()`
/// mengembalikan `Err` dan loop berhenti diam-diam.
async fn stream_lines<R>(
    reader: R,
    app: AppHandle,
    profile_id: String,
    step_index: usize,
    source: &'static str,
) where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncBufReadExt;
    let mut reader = BufReader::new(reader);
    let mut buf = Vec::with_capacity(256);
    loop {
        buf.clear();
        let n = match reader.read_until(b'\n', &mut buf).await {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => break,
        };
        let line = String::from_utf8_lossy(&buf[..n]);
        let line = line.strip_suffix('\n').unwrap_or(&line);
        let line = line.strip_suffix('\r').unwrap_or(line);
        // Buang kode warna/kursor ANSI supaya log enak dibaca.
        let line = util::strip_ansi(line);
        emit_log_plain(&app, &profile_id, step_index, source, &line);
    }
}

/// Bangun `TokioCommand` untuk menjalankan sebuah command step.
/// Di Windows, `npm`/`npx`/`yarn`/`pnpm` adalah shim `.cmd`/`.bat` yang tidak
/// bisa di-spawn langsung oleh `CreateProcess` (error "program not found").
/// Semua program non-`.exe` dijalankan lewat `cmd.exe /C` agar shim tersebut
/// berfungsi; program `.exe` di-spawn langsung.
fn build_command(program: &str, args: &[String], full_cmd: &str) -> TokioCommand {
    #[cfg(windows)]
    {
        let is_direct_exe = program.to_lowercase().ends_with(".exe");
        if is_direct_exe {
            let mut c = TokioCommand::new(program);
            c.args(args);
            c
        } else {
            let mut c = TokioCommand::new("cmd");
            c.arg("/D").arg("/S").arg("/C").arg(full_cmd);
            c
        }
    }
    #[cfg(not(windows))]
    {
        let _ = full_cmd;
        let mut c = TokioCommand::new(program);
        c.args(args);
        c
    }
}

/// Tentukan shell efektif untuk sebuah step command.
/// Shell yang dipilih user menang; kalau kosong/`"auto"` dan perintahnya
/// multi-baris (tidak bisa dipecah per spasi dengan benar), fallback ke shell
/// default platform supaya tetap jalan.
fn resolve_shell<'a>(shell: Option<&'a str>, cmd: &str) -> &'a str {
    match shell {
        Some(s) if !s.is_empty() && s != "auto" => s,
        _ => {
            if cmd.contains('\n') {
                if cfg!(windows) {
                    "cmd"
                } else {
                    "bash"
                }
            } else {
                "auto"
            }
        }
    }
}

/// Normalisasi line ending: CRLF untuk cmd/PowerShell, LF untuk bash.
fn normalize_newlines(content: &str, crlf: bool) -> String {
    let lf = content.replace("\r\n", "\n").replace('\r', "\n");
    if crlf {
        lf.replace('\n', "\r\n")
    } else {
        lf
    }
}

/// Tulis script multi-baris ke file sementara dan kembalikan path-nya.
/// File dibersihkan setelah proses selesai (lihat `spawn_command_step`).
fn write_temp_script(step_index: usize, ext: &str, content: &str, crlf: bool) -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join("dev-launcher");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Gagal membuat folder temp: {e}"))?;
    let path = dir.join(format!("step-{step_index}-{}.{ext}", uuid::Uuid::new_v4()));
    std::fs::write(&path, normalize_newlines(content, crlf))
        .map_err(|e| format!("Gagal menulis script sementara: {e}"))?;
    Ok(path)
}

/// Cari executable Bash. Di Windows, `bash` bisa jadi WSL (yang tidak bisa
/// dipakai untuk path Windows), jadi utamakan Git Bash kalau ada.
/// Bisa dioverride lewat env `DEV_LAUNCHER_BASH`.
fn find_bash() -> String {
    if let Ok(p) = std::env::var("DEV_LAUNCHER_BASH") {
        if !p.trim().is_empty() {
            return p;
        }
    }
    #[cfg(windows)]
    {
        let candidates = [
            std::env::var("ProgramFiles")
                .ok()
                .map(|p| format!("{p}\\Git\\bin\\bash.exe")),
            std::env::var("ProgramFiles(x86)")
                .ok()
                .map(|p| format!("{p}\\Git\\bin\\bash.exe")),
            std::env::var("LOCALAPPDATA")
                .ok()
                .map(|p| format!("{p}\\Programs\\Git\\bin\\bash.exe")),
        ];
        for candidate in candidates.into_iter().flatten() {
            if Path::new(&candidate).exists() {
                return candidate;
            }
        }
    }
    "bash".to_string()
}

/// Label singkat untuk log: baris pertama saja, beri tanda `…` kalau multi-baris.
fn shell_label(shell: &str, cmd: &str) -> String {
    let mut lines = cmd.lines();
    let first = lines.next().unwrap_or("").trim();
    if lines.next().is_some() {
        format!("[{shell}] {first} …")
    } else {
        format!("[{shell}] {first}")
    }
}

/// Bangun `TokioCommand` sesuai shell yang dipilih. Untuk cmd/PowerShell/bash,
/// perintah ditulis ke script sementara supaya perintah multi-baris aman
/// (cmd.exe membuang baris setelah newline bila dikirim inline).
/// Return: (command, file temp untuk dibersihkan, label log).
fn build_step_command(
    shell: &str,
    cmd: &str,
    step_index: usize,
) -> Result<(TokioCommand, Option<PathBuf>, String), String> {
    match shell {
        "auto" => {
            let mut split = cmd.split_whitespace();
            let program = split
                .next()
                .ok_or_else(|| format!("Step {step_index}: perintah kosong"))?;
            let args: Vec<String> = split.map(|s| s.to_string()).collect();
            let command = build_command(program, &args, cmd);
            Ok((command, None, format!("{} {}", program, args.join(" "))))
        }
        "cmd" | "cmd.exe" => {
            let path = write_temp_script(step_index, "bat", cmd, true)?;
            let mut command = TokioCommand::new("cmd");
            #[cfg(windows)]
            {
                // `call` agar path dengan spasi tetap valid dan script dijalankan.
                command.raw_arg(format!("/D /C call \"{}\"", path.display()));
            }
            #[cfg(not(windows))]
            {
                command.arg("/D").arg("/C").arg(format!("call \"{}\"", path.display()));
            }
            Ok((command, Some(path), shell_label("cmd", cmd)))
        }
        "powershell" | "pwsh" => {
            let path = write_temp_script(step_index, "ps1", cmd, true)?;
            let exe = if shell == "pwsh" || !cfg!(windows) {
                "pwsh"
            } else {
                "powershell"
            };
            let mut command = TokioCommand::new(exe);
            command.arg("-NoProfile");
            #[cfg(windows)]
            {
                // Bypass agar script aktivasi venv dsb. tidak diblok execution policy.
                command.arg("-ExecutionPolicy").arg("Bypass");
            }
            command.arg("-File").arg(&path);
            Ok((command, Some(path), shell_label("powershell", cmd)))
        }
        "bash" | "sh" => {
            let path = write_temp_script(step_index, "sh", cmd, false)?;
            let mut command = TokioCommand::new(find_bash());
            command.arg(&path);
            Ok((command, Some(path), shell_label("bash", cmd)))
        }
        other => Err(format!("Step {step_index}: shell tidak dikenal: {other}")),
    }
}

async fn spawn_command_step(
    app: AppHandle,
    profile_id: &str,
    step_index: usize,
    cwd: &str,
    cmd: &str,
    port_override: Option<u16>,
    shell: Option<&str>,
    registry: Arc<RunningRegistry>,
) -> Result<u32, String> {
    if !Path::new(cwd).exists() {
        return Err(format!(
            "Step {step_index}: direktori tidak ditemukan: {cwd}"
        ));
    }

    let resolved = resolve_shell(shell, cmd);
    let (mut command, temp_file, label) = build_step_command(resolved, cmd, step_index)?;
    command.current_dir(cwd);
    command.stdin(Stdio::null());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    if let Some(port) = port_override {
        // Vite/Next.js: PORT. Laravel: APP_PORT (server artisan).
        command.env("PORT", port.to_string());
        command.env("APP_PORT", port.to_string());
    }

    // Hindari munculnya console window di Windows
    #[cfg(windows)]
    {
        command.creation_flags(util::CREATE_NO_WINDOW);
    }

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            // Jangan tinggalkan script temp kalau gagal spawn.
            if let Some(path) = &temp_file {
                let _ = std::fs::remove_file(path);
            }
            return Err(format!("Step {step_index}: gagal spawn `{resolved}`: {e}"));
        }
    };

    let pid = child.id().unwrap_or(0);
    {
        let mut map = registry
            .processes
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        map.entry(profile_id.to_string()).or_default().pids.push(pid);
    }

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
        tokio::spawn(stream_lines(out, app2, pid2, step_index, "stdout"));
    }
    if let Some(err) = child.stderr.take() {
        let app2 = app.clone();
        let pid2 = profile_id.to_string();
        tokio::spawn(stream_lines(err, app2, pid2, step_index, "stderr"));
    }

    // Hapus PID dari registry saat process keluar. Kalau ini proses terakhir
    // dan profile sudah selesai di-start, anggap profile selesai (emit stopped).
    let registry2 = registry.clone();
    let app2 = app.clone();
    let pid_str = profile_id.to_string();
    tokio::spawn(async move {
        let _ = child.wait().await;
        if let Some(path) = &temp_file {
            let _ = std::fs::remove_file(path);
        }
        let finished = {
            if let Ok(mut map) = registry2.processes.lock() {
                let done = if let Some(rp) = map.get_mut(&pid_str) {
                    rp.pids.retain(|p| *p != pid);
                    rp.pids.is_empty() && rp.started
                } else {
                    false
                };
                if done {
                    map.remove(&pid_str);
                }
                done
            } else {
                false
            }
        };
        if finished {
            emit_status(
                &app2,
                status_payload(
                    &pid_str,
                    "stopped",
                    "Semua proses selesai.".into(),
                    None,
                ),
            );
        }
    });

    Ok(pid)
}

/// Jalankan semua step sebuah profile secara berurutan.
/// `port_overrides` memetakan index step ke port pengganti (opsional).
#[tauri::command]
pub async fn start_profile(
    app: AppHandle,
    state: tauri::State<'_, Arc<RunningRegistry>>,
    profile_id: String,
    port_overrides: Option<HashMap<usize, u16>>,
) -> Result<StartResult, String> {
    let profiles = crate::storage::load_profiles()?;
    let profile = profiles
        .iter()
        .find(|p| p.id == profile_id)
        .ok_or_else(|| format!("Profile {profile_id} tidak ditemukan"))?
        .clone();

    // Guard: jangan start ulang kalau profil ini masih punya proses hidup.
    {
        let registry = state.inner().clone();
        let alive = registry
            .processes
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&profile_id)
            .map(|p| p.pids.len())
            .unwrap_or(0);
        if alive > 0 {
            return Err(format!(
                "Profile {profile_id} sedang berjalan ({alive} proses) — stop dulu sebelum start ulang."
            ));
        }
    }

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
    let mut checked_ports = false;
    let registry = state.inner().clone();

    // Kalau ada step yang gagal di tengah, bunuh semua proses yang sudah
    // dijalankan supaya tidak meninggalkan orphan (rollback).
    let result = async {
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

                // Build command dengan CREATE_NO_WINDOW flag untuk hindari console window
                let mut cmd = TokioCommand::new(path);
                #[cfg(windows)]
                {
                    cmd.creation_flags(util::CREATE_NO_WINDOW);
                }

                match cmd.spawn() {
                    Ok(mut child) => {
                        let pid = child.id().unwrap_or(0);
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
                                .pids
                                .push(pid);
                        }
                        emit_log_plain(
                            &app,
                            &profile_id,
                            idx,
                            "system",
                            &format!("[step {idx}] exe: {path} (pid {pid})"),
                        );

                        // Hapus PID dari registry saat exe selesai keluar,
                        // agar stop tidak menyasar proses yang sudah mati.
                        let registry2 = registry.clone();
                        let app2 = app.clone();
                        let pid_str = profile_id.clone();
                        tokio::spawn(async move {
                            let _ = child.wait().await;
                            let finished = {
                                if let Ok(mut map) = registry2.processes.lock() {
                                    let done = if let Some(rp) = map.get_mut(&pid_str) {
                                        rp.pids.retain(|p| *p != pid);
                                        rp.pids.is_empty() && rp.started
                                    } else {
                                        false
                                    };
                                    if done {
                                        map.remove(&pid_str);
                                    }
                                    done
                                } else {
                                    false
                                }
                            };
                            if finished {
                                emit_status(
                                    &app2,
                                    status_payload(
                                        &pid_str,
                                        "stopped",
                                        "Semua proses selesai.".into(),
                                        None,
                                    ),
                                );
                            }
                        });
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
            Step::Command {
                cwd,
                cmd,
                port,
                shell,
            } => {
                let effective_port = port_overrides
                    .as_ref()
                    .and_then(|m| m.get(&idx).copied())
                    .or(*port);
                if let Some(override_port) = port_overrides.as_ref().and_then(|m| m.get(&idx)) {
                    if *port != Some(*override_port) {
                        emit_log_plain(
                            &app,
                            &profile_id,
                            idx,
                            "system",
                            &format!(
                                "[step {idx}] port diganti {port:?} -> {override_port} (env PORT/APP_PORT). \
                                 Step open_url yang mengarah ke port lama tidak otomatis berubah."
                            ),
                        );
                    }
                }
                let pid = spawn_command_step(
                    app.clone(),
                    &profile_id,
                    idx,
                    cwd,
                    cmd,
                    effective_port,
                    shell.as_deref(),
                    registry.clone(),
                )
                .await?;
                pids.push(StepPid {
                    step_index: idx,
                    pid,
                    label: cmd.clone(),
                });
                if let Some(p) = effective_port {
                    checked_ports = true;
                    check_step_port(&app, &profile_id, idx, p).await;
                }
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
        Ok(())
    }
    .await;

    if let Err(err) = result {
        // Rollback: bunuh semua proses yang sudah dijalankan step sebelumnya.
        for sp in pids {
            kill_tree(sp.pid);
        }
        emit_status(
            &app,
            StatusPayload {
                profile_id: profile_id.clone(),
                status: "failed".into(),
                message: format!("Gagal: {err}"),
                step_index: None,
            },
        );
        return Err(err);
    }

    // Tandai start sudah selesai. Kalau tidak ada proses yang masih hidup
    // (semua step short-lived, atau profile kosong), anggap profile langsung
    // selesai — hindari UI nyangkut "running" padahal tidak ada proses.
    let still_running = {
        let mut map = registry
            .processes
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let has_pids = map
            .get(&profile_id)
            .map(|rp| !rp.pids.is_empty())
            .unwrap_or(false);
        if has_pids {
            map.get_mut(&profile_id).unwrap().started = true;
        } else {
            map.remove(&profile_id);
        }
        has_pids
    };

    if still_running {
        emit_status(
            &app,
            StatusPayload {
                profile_id: profile_id.clone(),
                status: "running".into(),
                message: if checked_ports {
                    "Semua step siap.".into()
                } else {
                    "Semua step selesai dijalankan.".into()
                },
                step_index: None,
            },
        );
    } else {
        emit_status(
            &app,
            status_payload(
                &profile_id,
                "stopped",
                "Semua proses selesai.".into(),
                None,
            ),
        );
    }

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
        let pids = map
            .remove(&profile_id)
            .map(|rp| rp.pids)
            .unwrap_or_default();
        let mut killed = Vec::new();
        for pid in pids {
            kill_tree(pid);
            killed.push(pid);
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

/// Kill sebuah proses beserta seluruh anaknya (pohon proses).
/// Di Windows pakai `taskkill /T /F` karena process yang di-spawn bisa berupa
/// `cmd.exe` yang membungkus `npm.cmd` yang membungkus `node` — membunuh
/// hanya satu PID saja tidak cukup.
#[cfg(windows)]
pub(crate) fn kill_tree(pid: u32) {
    use std::os::windows::process::CommandExt;
    let _ = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .creation_flags(util::CREATE_NO_WINDOW)
        .output();
}

#[cfg(not(windows))]
pub(crate) fn kill_tree(pid: u32) {
    let mut sys = sysinfo::System::new_all();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    if let Some(proc) = sys.process(sysinfo::Pid::from_u32(pid)) {
        let _ = proc.kill();
    }
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
