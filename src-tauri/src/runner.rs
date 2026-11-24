// Menjalankan step-step sebuah profile, menyimpan PID child process yang masih
// hidup, streaming log lewat `logbus`, dan menyimpan journal PID supaya status
// bisa direkonsiliasi setelah aplikasi restart.
use crate::journal;
use crate::logbus::LogBus;
use crate::models::{PortConflict, PreflightResult, Profile, RuntimeInfo, Step};
use crate::ports::{get_active_ports_blocking, PortInfo};
use crate::storage;
use crate::util;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};
use sysinfo::{Pid, ProcessesToUpdate, System};
use tauri::{AppHandle, Emitter};
use tokio::io::BufReader;
use tokio::net::TcpStream;
use tokio::process::Command as TokioCommand;

const DEFAULT_READY_TIMEOUT_MS: u64 = 30_000;
const MAX_READY_TIMEOUT_MS: u64 = 600_000;
const RESTART_BACKOFF_MS: u64 = 2_000;
const STOP_GRACE_MS: u64 = 2_500;
/// Proses yang sudah hidup lebih lama dari ini dianggap stabil; hitungan
/// restart-nya di-reset, agar crash berikutnya tidak dihitung sebagai loop.
const STABLE_UPTIME_MS: u64 = 60_000;

/// Ditandai saat aplikasi keluar. Mencegah restart task menyalakan proses baru
/// setelah exit handler selesai membunuh semua proses yang dilacak.
static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);
/// Token generasi untuk membedakan sesi start yang lama dari yang baru.
static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

pub fn begin_shutdown() {
    SHUTTING_DOWN.store(true, Ordering::SeqCst);
}

fn shutting_down() -> bool {
    SHUTTING_DOWN.load(Ordering::SeqCst)
}

/// Profil yang sedang berjalan (atau sedang start).
#[derive(Default)]
pub struct RunningProfile {
    pub pids: Vec<u32>,
    pub started: bool,
    pub stopping: bool,
    /// Jumlah restart otomatis per index step.
    pub restarts: HashMap<usize, u32>,
    /// Berubah tiap kali profile di-start; dipakai restart task supaya tidak
    /// menyuntik proses ke sesi start yang baru.
    pub generation: u64,
}

/// Registry proses hidup per profile.
#[derive(Default)]
pub struct RunningRegistry {
    pub processes: Mutex<HashMap<String, RunningProfile>>,
}

impl RunningRegistry {
    pub fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, RunningProfile>> {
        self.processes.lock().unwrap_or_else(|p| p.into_inner())
    }
}

#[derive(Clone, Default)]
pub struct StartOptions {
    pub port_overrides: HashMap<usize, u16>,
    pub force: bool,
    pub skip_steps: Vec<usize>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StatusPayload {
    profile_id: String,
    status: String, // "starting" | "running" | "stopped" | "failed"
    message: String,
    step_index: Option<usize>,
    /// Kode keluar proses pada event terminal (null kalau tidak tersedia).
    exit_code: Option<i32>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct StepStatusPayload {
    profile_id: String,
    step_index: usize,
    state: String, // "spawned" | "ready" | "timeout" | "skipped"
    port: Option<u16>,
}

fn emit_status(app: &AppHandle, payload: StatusPayload) {
    let _ = app.emit("profile-status", payload);
}

fn emit_step_status(app: &AppHandle, payload: StepStatusPayload) {
    let _ = app.emit("profile-step-status", payload);
}

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
        exit_code: None,
    }
}

/// Varian `status_payload` untuk event terminal yang membawa exit code proses.
pub(crate) fn status_payload_with_exit(
    profile_id: &str,
    status: &str,
    message: String,
    step_index: Option<usize>,
    exit_code: Option<i32>,
) -> StatusPayload {
    StatusPayload {
        exit_code,
        ..status_payload(profile_id, status, message, step_index)
    }
}

// ---------------------------------------------------------------------------
// Preflight (konflik port + validasi konfigurasi)
// ---------------------------------------------------------------------------

pub fn preflight(profile: &Profile) -> Result<PreflightResult, String> {
    let active = get_active_ports_blocking()?;
    Ok(preflight_with_ports(profile, &active))
}

pub(crate) fn preflight_with_ports(profile: &Profile, active: &[PortInfo]) -> PreflightResult {
    let mut conflicts: Vec<PortConflict> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    let mut seen: HashMap<u16, usize> = HashMap::new();

    for (idx, step) in profile.steps.iter().enumerate() {
        match step {
            Step::Exe { path } => {
                if path.trim().is_empty() {
                    errors.push(format!("Step {idx}: path exe kosong."));
                } else if !Path::new(path).exists() {
                    errors.push(format!("Step {idx}: file exe tidak ditemukan: {path}"));
                }
            }
            Step::OpenUrl { url } => {
                if url.trim().is_empty() {
                    errors.push(format!("Step {idx}: URL kosong."));
                } else if !crate::util::is_safe_http_url(url) {
                    errors.push(format!(
                        "Step {idx}: hanya URL http/https yang diizinkan: {url}"
                    ));
                }
            }
            Step::Command { cwd, cmd, port, .. } => {
                if cwd.trim().is_empty() {
                    errors.push(format!("Step {idx}: working directory kosong."));
                } else if !Path::new(cwd).exists() {
                    errors.push(format!("Step {idx}: direktori tidak ditemukan: {cwd}"));
                }
                if cmd.trim().is_empty() {
                    errors.push(format!("Step {idx}: perintah kosong."));
                }
                let Some(port) = port else { continue };
                if *port == 0 {
                    errors.push(format!("Step {idx}: port 0 tidak valid."));
                    continue;
                }
                if let Some(prev) = seen.insert(*port, idx) {
                    errors.push(format!(
                        "Step {prev} dan step {idx} memakai port {port} yang sama."
                    ));
                }

                // TCP: hanya peduli status LISTENING. UDP tidak punya status,
                // tapi tetap dianggap bentrok kalau port dipakai. PID 0 (System
                // Idle / listener kernel) tidak bisa dibunuh dan tak relevan.
                let used: Vec<&PortInfo> = active
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
                    let mut exe_paths: Vec<String> =
                        used.iter().map(|p| p.exe_path.clone()).collect();
                    exe_paths.sort();
                    exe_paths.dedup();
                    let mut cwds: Vec<String> = used.iter().map(|p| p.cwd.clone()).collect();
                    cwds.sort();
                    cwds.dedup();
                    conflicts.push(PortConflict {
                        step_index: idx,
                        port: *port,
                        pids,
                        process_names: used.iter().map(|p| p.process_name.clone()).collect(),
                        exe_paths,
                        cwds,
                    });
                }
            }
        }
    }

    PreflightResult { conflicts, errors }
}

// ---------------------------------------------------------------------------
// Deteksi proses
// ---------------------------------------------------------------------------

/// Waktu mulai proses (epoch detik). Dipakai untuk memastikan PID belum
/// di-reuse oleh proses lain sebelum kita kill.
fn process_start_time(pid: u32) -> Option<u64> {
    let mut sys = System::new();
    let pid = Pid::from_u32(pid);
    sys.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
    sys.process(pid).map(|p| p.start_time())
}

pub fn alive_pids(pids: &[u32]) -> Vec<u32> {
    if pids.is_empty() {
        return Vec::new();
    }
    let targets: Vec<Pid> = pids.iter().map(|p| Pid::from_u32(*p)).collect();
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::Some(&targets), true);
    pids.iter()
        .copied()
        .filter(|pid| sys.process(Pid::from_u32(*pid)).is_some())
        .collect()
}

/// Filter PID yang masih hidup DAN (kalau tercatat) start time-nya cocok.
fn verified_pids(profile_id: &str, pids: &[u32]) -> Vec<u32> {
    let journal = journal::load();
    let alive = alive_pids(pids);
    alive
        .into_iter()
        .filter(|pid| {
            let expected = journal
                .entries
                .iter()
                .find(|e| e.pid == *pid && e.profile_id == profile_id)
                .map(|e| e.started_at);
            match (process_start_time(*pid), expected) {
                (Some(actual), Some(expected)) if expected != 0 => actual == expected,
                (Some(_), _) => true,
                (None, _) => false,
            }
        })
        .collect()
}

/// Rekonsiliasi journal saat startup: PID yang masih hidup dipulihkan ke
/// registry, yang sudah mati dibersihkan.
pub fn reconcile(registry: &RunningRegistry) {
    let mut journal = journal::load();
    let mut alive: HashMap<String, Vec<u32>> = HashMap::new();
    let mut kept = Vec::new();
    let mut skipped = 0;

    for entry in journal.entries.drain(..) {
        let matches = match process_start_time(entry.pid) {
            Some(actual) => entry.started_at == 0 || entry.started_at == actual,
            None => false,
        };
        if matches {
            alive
                .entry(entry.profile_id.clone())
                .or_default()
                .push(entry.pid);
            kept.push(entry);
        } else {
            skipped += 1;
        }
    }
    journal::replace(kept);
    if skipped > 0 {
        // Hanya jumlah entri yang dicatat: identitas proses tidak dipublikasikan.
        log::warn!("Rekonsiliasi journal: {skipped} entri proses tidak ditemukan dan dibuang.");
    }

    let mut map = registry.lock();
    for (profile_id, pids) in alive {
        let rp = map.entry(profile_id).or_default();
        rp.pids.extend(pids);
        rp.pids.sort_unstable();
        rp.pids.dedup();
        rp.started = true;
    }
}

pub fn runtime_state(registry: &RunningRegistry) -> Vec<RuntimeInfo> {
    let journal = journal::load();
    let map = registry.lock();
    map.iter()
        .filter(|(_, rp)| !rp.pids.is_empty())
        .map(|(profile_id, rp)| RuntimeInfo {
            profile_id: profile_id.clone(),
            pids: rp.pids.clone(),
            started_at: journal
                .entries
                .iter()
                .find(|e| &e.profile_id == profile_id)
                .map(|e| e.started_at)
                .unwrap_or(0),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Helpers proses
// ---------------------------------------------------------------------------

#[cfg(windows)]
fn taskkill(pid: u32, force: bool) {
    use std::os::windows::process::CommandExt;
    let mut cmd = std::process::Command::new("taskkill");
    cmd.args(["/PID", &pid.to_string(), "/T"]);
    if force {
        cmd.arg("/F");
    }
    match cmd.creation_flags(util::CREATE_NO_WINDOW).output() {
        Ok(output) if !output.status.success() => {
            log::warn!("taskkill gagal untuk proses {pid} (force: {force}).");
        }
        Err(err) => log::warn!("taskkill tidak bisa dijalankan untuk {pid}: {err}"),
        Ok(_) => {}
    }
}

#[cfg(not(windows))]
fn taskkill(pid: u32, force: bool) {
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All, true);
    if let Some(proc) = sys.process(Pid::from_u32(pid)) {
        if let Err(err) = proc.kill() {
            log::warn!("Gagal menghentikan proses {pid} (force: {force}): {err}");
        }
    }
}

/// Hentikan satu pohon proses dengan sopan, tunggu sebentar, lalu paksa.
fn stop_tree(pid: u32) {
    if alive_pids(&[pid]).is_empty() {
        return;
    }
    taskkill(pid, false);
    let deadline = Instant::now() + Duration::from_millis(STOP_GRACE_MS);
    while Instant::now() < deadline {
        if alive_pids(&[pid]).is_empty() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    taskkill(pid, true);
}

fn stop_pids(profile_id: &str, pids: &[u32]) {
    if pids.is_empty() {
        return;
    }
    // Kirim permintaan berhenti ke semua dulu, baru tunggu bersama.
    for pid in pids {
        taskkill(*pid, false);
    }
    let deadline = Instant::now() + Duration::from_millis(STOP_GRACE_MS);
    while Instant::now() < deadline {
        if alive_pids(pids).is_empty() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    // Cek ulang start time sebelum force kill: PID bisa saja di-reuse dalam
    // jendela grace 2,5 detik di atas.
    for pid in verified_pids(profile_id, pids) {
        if !alive_pids(&[pid]).is_empty() {
            taskkill(pid, true);
        }
    }
}

/// Kill sebuah proses beserta anak-cucunya (dipakai saat app exit).
pub fn kill_tree(pid: u32) {
    taskkill(pid, true);
}

fn register_pid(
    registry: &RunningRegistry,
    profile_id: &str,
    step_index: usize,
    pid: u32,
    expected_generation: Option<u64>,
) -> bool {
    // Kalau sysinfo gagal, pakai waktu sekarang daripada 0: nilai 0 mematikan
    // verifikasi PID-reuse di `verified_pids` / `reconcile`.
    let started_at = process_start_time(pid).unwrap_or_else(|| {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    });
    let accepted = {
        let mut map = registry.lock();
        match map.get_mut(profile_id) {
            Some(rp)
                if !rp.stopping
                    && !shutting_down()
                    && expected_generation
                        .map(|gen| rp.generation == gen)
                        .unwrap_or(true) =>
            {
                rp.pids.push(pid);
                true
            }
            _ => false,
        }
    };
    if accepted {
        journal::add(profile_id, step_index, pid, started_at);
    }
    accepted
}

async fn stream_lines<R>(
    reader: R,
    bus: LogBus,
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
        let line = util::strip_ansi(line);
        bus.log(&profile_id, step_index, source, line);
    }
}

/// Bangun `TokioCommand` untuk menjalankan sebuah command step.
/// Di Windows, `npm`/`npx`/`yarn`/`pnpm` adalah shim `.cmd`/`.bat` yang tidak
/// bisa di-spawn langsung oleh `CreateProcess` (error "program not found").
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

fn normalize_newlines(content: &str, crlf: bool) -> String {
    let lf = content.replace("\r\n", "\n").replace('\r', "\n");
    if crlf {
        lf.replace('\n', "\r\n")
    } else {
        lf
    }
}

fn write_temp_script(
    step_index: usize,
    ext: &str,
    content: &str,
    crlf: bool,
) -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join("dev-launcher");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Gagal membuat folder temp: {e}"))?;
    let path = dir.join(format!("step-{step_index}-{}.{ext}", uuid::Uuid::new_v4()));
    std::fs::write(&path, normalize_newlines(content, crlf))
        .map_err(|e| format!("Gagal menulis script sementara: {e}"))?;
    Ok(path)
}

/// Bersihkan script sementara yang tertinggal setelah crash. Yang masih baru
/// sengaja dibiarkan karena proses dari sesi sebelumnya mungkin masih memakainya.
pub fn purge_temp_scripts() {
    let dir = std::env::temp_dir().join("dev-launcher");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if !path.is_file() || !name.starts_with("step-") {
            continue;
        }
        let age = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|m| now.duration_since(m).ok())
            .unwrap_or_default();
        if age > Duration::from_secs(3600) {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Tokenisasi perintah `auto` yang mendukung tanda kutip, sehingga path dengan
/// spasi seperti `"C:\\Program Files\\node\\node.exe"` tidak terpotong.
fn split_command(input: &str) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut has_token = false;
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            } else if c == '\\' && q == '"' && chars.peek() == Some(&'"') {
                // Hanya escape tanda kutip; backslash path Windows tetap utuh.
                chars.next();
                current.push('"');
            } else {
                current.push(c);
            }
            continue;
        }
        match c {
            '\'' | '"' => {
                quote = Some(c);
                has_token = true;
            }
            '\\' => {
                if let Some(next) = chars.next() {
                    current.push(next);
                    has_token = true;
                }
            }
            c if c.is_whitespace() => {
                if has_token {
                    args.push(std::mem::take(&mut current));
                    has_token = false;
                }
            }
            c => {
                current.push(c);
                has_token = true;
            }
        }
    }
    if quote.is_some() {
        return Err("Tanda kutip perintah tidak tertutup.".into());
    }
    if has_token {
        args.push(current);
    }
    Ok(args)
}

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

fn shell_label(shell: &str, cmd: &str) -> String {
    let mut lines = cmd.lines();
    let first = lines.next().unwrap_or("").trim();
    if lines.next().is_some() {
        format!("[{shell}] {first} …")
    } else {
        format!("[{shell}] {first}")
    }
}

fn build_step_command(
    shell: &str,
    cmd: &str,
    step_index: usize,
) -> Result<(TokioCommand, Option<PathBuf>, String), String> {
    match shell {
        "auto" => {
            let parts = split_command(cmd).map_err(|e| format!("Step {step_index}: {e}"))?;
            let program = parts
                .first()
                .ok_or_else(|| format!("Step {step_index}: perintah kosong"))?;
            let args = parts[1..].to_vec();
            let command = build_command(program, &args, cmd);
            Ok((command, None, format!("{} {}", program, args.join(" "))))
        }
        "cmd" | "cmd.exe" => {
            let path = write_temp_script(step_index, "bat", cmd, true)?;
            let mut command = TokioCommand::new("cmd");
            #[cfg(windows)]
            {
                command.raw_arg(format!("/D /C call \"{}\"", path.display()));
            }
            #[cfg(not(windows))]
            {
                command
                    .arg("/D")
                    .arg("/C")
                    .arg(format!("call \"{}\"", path.display()));
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

#[cfg(windows)]
fn open_url(url: &str) -> Result<(), String> {
    crate::win::open_url(url)
}

#[cfg(not(windows))]
fn open_url(url: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "linux")]
    let result = std::process::Command::new("xdg-open").arg(url).spawn();
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let result: std::io::Result<std::process::Child> = Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "unsupported platform",
    ));
    result
        .map(|_| ())
        .map_err(|e| format!("Gagal membuka URL: {e}"))
}

// ---------------------------------------------------------------------------
// Readiness
// ---------------------------------------------------------------------------

async fn wait_ready(kind: &str, port: u16, timeout: Duration, pid: u32) -> bool {
    match kind {
        "none" => true,
        "http" => wait_http(port, timeout, pid).await,
        _ => wait_tcp(port, timeout, pid).await,
    }
}

/// Proses masih hidup? Dipakai agar readiness berhenti menunggu kalau proses
/// yang ditunggu sudah mati (mis. `npm run dev` gagal start).
fn process_alive(pid: u32) -> bool {
    pid == 0 || !alive_pids(&[pid]).is_empty()
}

/// `pid` atau salah satu turunannya (npm -> node, php artisan -> php, ...).
fn process_in_tree(pid: u32, root: u32) -> bool {
    if pid == 0 || root == 0 {
        return false;
    }
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All, true);
    let mut current = Pid::from_u32(pid);
    for _ in 0..32 {
        if current.as_u32() == root {
            return true;
        }
        let Some(process) = sys.process(current) else {
            return false;
        };
        let Some(parent) = process.parent() else {
            return false;
        };
        if parent == current || parent.as_u32() == 0 {
            return false;
        }
        current = parent;
    }
    false
}

/// Port benar-benar dipegang oleh proses yang kita spawn (atau turunannya).
/// Mencegah readiness "sukses" karena port lama dari aplikasi lain masih aktif.
fn port_owned_by_tree(port: u16, root: u32) -> bool {
    let Ok(active) = get_active_ports_blocking() else {
        return false;
    };
    active.iter().any(|p| {
        p.port == port
            && p.pid != 0
            && (p.protocol == "UDP" || p.status == "LISTENING")
            && process_in_tree(p.pid, root)
    })
}

async fn wait_tcp(port: u16, timeout: Duration, pid: u32) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        let connected = TcpStream::connect(("127.0.0.1", port)).await.is_ok()
            || TcpStream::connect(("::1", port)).await.is_ok();
        if connected && port_owned_by_tree(port, pid) {
            return true;
        }
        if Instant::now() >= deadline || !process_alive(pid) {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// Cek kesiapan HTTP sederhana tanpa dependency HTTP client: kirim GET dan
/// anggap siap kalau server membalas apa pun.
async fn wait_http(port: u16, timeout: Duration, pid: u32) -> bool {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let deadline = Instant::now() + timeout;
    loop {
        for host in ["127.0.0.1", "::1"] {
            let connect =
                tokio::time::timeout(Duration::from_secs(2), TcpStream::connect((host, port)))
                    .await;
            if let Ok(Ok(mut stream)) = connect {
                let request =
                    format!("GET / HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\n\r\n");
                if stream.write_all(request.as_bytes()).await.is_ok() {
                    let mut buf = [0u8; 32];
                    match tokio::time::timeout(Duration::from_secs(2), stream.read(&mut buf)).await
                    {
                        Ok(Ok(n)) if n > 0 => {
                            return port_owned_by_tree(port, pid);
                        }
                        _ => return port_owned_by_tree(port, pid),
                    }
                }
            }
        }
        if Instant::now() >= deadline || !process_alive(pid) {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

fn effective_readiness(step: &Step, port: Option<u16>) -> &'static str {
    let requested = match step {
        Step::Command { readiness, .. } => readiness.as_deref().unwrap_or(""),
        _ => "",
    };
    match requested {
        "none" => "none",
        "http" => "http",
        "tcp" => "tcp",
        _ => {
            if port.is_some() {
                "tcp"
            } else {
                "none"
            }
        }
    }
}

fn ready_timeout(step: &Step) -> Duration {
    let ms = match step {
        Step::Command {
            ready_timeout_ms, ..
        } => ready_timeout_ms.unwrap_or(DEFAULT_READY_TIMEOUT_MS),
        _ => DEFAULT_READY_TIMEOUT_MS,
    };
    Duration::from_millis(ms.clamp(1_000, MAX_READY_TIMEOUT_MS))
}

// ---------------------------------------------------------------------------
// Spawn
// ---------------------------------------------------------------------------

fn spawn_exe(
    app: AppHandle,
    registry: Arc<RunningRegistry>,
    bus: LogBus,
    profile_id: String,
    step_index: usize,
    path: String,
    expected_generation: Option<u64>,
) -> Result<u32, String> {
    if !Path::new(&path).exists() {
        return Err(format!("Step {step_index}: file tidak ditemukan: {path}"));
    }

    let mut cmd = TokioCommand::new(&path);
    #[cfg(windows)]
    {
        cmd.creation_flags(util::CREATE_NO_WINDOW);
    }

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => return Err(format!("Step {step_index}: gagal spawn exe: {e}")),
    };
    let pid = child.id().unwrap_or(0);
    if !register_pid(&registry, &profile_id, step_index, pid, expected_generation) {
        kill_tree(pid);
        return Err(format!(
            "Step {step_index}: profile sudah dihentikan sebelum proses siap."
        ));
    }
    bus.log(
        &profile_id,
        step_index,
        "system",
        format!("[step {step_index}] exe: {path} (pid {pid})"),
    );

    tokio::spawn(async move {
        let exit_code = child.wait().await.ok().and_then(|exit| exit.code());
        journal::remove_pid(&profile_id, pid);
        let all_done = {
            let mut map = registry.lock();
            match map.get_mut(&profile_id) {
                Some(rp) => {
                    rp.pids.retain(|p| *p != pid);
                    if rp.pids.is_empty() && rp.started && !rp.stopping {
                        map.remove(&profile_id);
                        true
                    } else {
                        false
                    }
                }
                None => false,
            }
        };
        if all_done {
            bus.log(&profile_id, 0, "system", "Semua proses selesai.");
            emit_status(
                &app,
                status_payload_with_exit(
                    &profile_id,
                    "stopped",
                    "Semua proses selesai.".into(),
                    None,
                    exit_code,
                ),
            );
        }
    });

    Ok(pid)
}

#[allow(clippy::too_many_arguments)]
fn spawn_command_step(
    app: AppHandle,
    registry: Arc<RunningRegistry>,
    bus: LogBus,
    profile_id: String,
    step_index: usize,
    step: Step,
    port_override: Option<u16>,
    expected_generation: Option<u64>,
) -> Result<u32, String> {
    let Step::Command {
        cwd,
        cmd,
        shell,
        env,
        auto_restart,
        max_restarts,
        ..
    } = &step
    else {
        return Err("Bukan step command".into());
    };

    if !Path::new(cwd).exists() {
        return Err(format!(
            "Step {step_index}: direktori tidak ditemukan: {cwd}"
        ));
    }

    let resolved = resolve_shell(shell.as_deref(), cmd);
    let (mut command, temp_file, label) = build_step_command(resolved, cmd, step_index)?;
    command.current_dir(cwd);
    command.stdin(Stdio::null());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    if let Some(env_map) = env {
        for (key, value) in env_map {
            if !key.trim().is_empty() {
                command.env(key, value);
            }
        }
    }
    if let Some(port) = port_override {
        // Vite/Next.js: PORT & VITE_PORT. Laravel: APP_PORT (server artisan).
        command.env("PORT", port.to_string());
        command.env("VITE_PORT", port.to_string());
        command.env("APP_PORT", port.to_string());
    }

    #[cfg(windows)]
    {
        command.creation_flags(util::CREATE_NO_WINDOW);
    }

    let spawned_at = Instant::now();
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            if let Some(path) = &temp_file {
                let _ = std::fs::remove_file(path);
            }
            return Err(format!("Step {step_index}: gagal spawn `{resolved}`: {e}"));
        }
    };

    let pid = child.id().unwrap_or(0);
    if !register_pid(&registry, &profile_id, step_index, pid, expected_generation) {
        if let Some(path) = &temp_file {
            let _ = std::fs::remove_file(path);
        }
        kill_tree(pid);
        return Err(format!(
            "Step {step_index}: profile sudah dihentikan sebelum proses siap."
        ));
    }

    bus.log(
        &profile_id,
        step_index,
        "system",
        format!("[step {step_index}] mulai: {label} (pid {pid})"),
    );

    if let Some(out) = child.stdout.take() {
        tokio::spawn(stream_lines(
            out,
            bus.clone(),
            profile_id.clone(),
            step_index,
            "stdout",
        ));
    }
    if let Some(err) = child.stderr.take() {
        tokio::spawn(stream_lines(
            err,
            bus.clone(),
            profile_id.clone(),
            step_index,
            "stderr",
        ));
    }

    let restart_allowed = auto_restart.unwrap_or(false);
    let restart_limit = max_restarts.unwrap_or(3).min(20);
    let app2 = app.clone();
    let registry2 = registry.clone();
    let bus2 = bus.clone();
    let id2 = profile_id.clone();
    let step2 = step.clone();
    tokio::spawn(async move {
        let exit = match child.wait().await {
            Ok(status) => status,
            Err(_) => return,
        };
        let exit_code = exit.code();
        let exit_ok = exit.success();
        let exit_label = exit_code_label(exit);
        if let Some(path) = &temp_file {
            let _ = std::fs::remove_file(path);
        }
        journal::remove_pid(&id2, pid);

        let action = {
            let mut map = registry2.lock();
            let Some(rp) = map.get_mut(&id2) else {
                return; // profile sudah dihentikan / dihapus
            };
            if expected_generation
                .map(|gen| rp.generation != gen)
                .unwrap_or(false)
            {
                return; // proses dari sesi start yang sudah usang
            }
            if spawned_at.elapsed() >= Duration::from_millis(STABLE_UPTIME_MS) {
                rp.restarts.remove(&step_index);
            }
            rp.pids.retain(|p| *p != pid);
            if rp.stopping {
                0
            } else if restart_allowed {
                let count = *rp.restarts.get(&step_index).unwrap_or(&0);
                if count < restart_limit {
                    rp.restarts.insert(step_index, count + 1);
                    1
                } else {
                    2
                }
            } else {
                2
            }
        };

        if action == 1 {
            let attempt = {
                let map = registry2.lock();
                map.get(&id2)
                    .map(|rp| *rp.restarts.get(&step_index).unwrap_or(&1))
                    .unwrap_or(1)
            };
            bus2.log(
                &id2,
                step_index,
                "system",
                format!(
                    "[step {step_index}] proses keluar ({}). Restart otomatis {attempt}/{restart_limit} dalam {} detik…",
                    exit_label,
                    RESTART_BACKOFF_MS / 1000
                ),
            );
            tokio::time::sleep(Duration::from_millis(RESTART_BACKOFF_MS)).await;
            let still_wanted = {
                let map = registry2.lock();
                map.get(&id2)
                    .map(|rp| {
                        !rp.stopping
                            && !shutting_down()
                            && expected_generation
                                .map(|gen| rp.generation == gen)
                                .unwrap_or(true)
                    })
                    .unwrap_or(false)
            };
            if still_wanted {
                if let Err(e) = spawn_command_step(
                    app2.clone(),
                    registry2.clone(),
                    bus2.clone(),
                    id2.clone(),
                    step_index,
                    step2,
                    port_override,
                    expected_generation,
                ) {
                    bus2.log(
                        &id2,
                        step_index,
                        "system",
                        format!("[step {step_index}] restart gagal: {e}"),
                    );
                    let empty = {
                        let mut map = registry2.lock();
                        match map.get_mut(&id2) {
                            Some(rp) if rp.pids.is_empty() => {
                                map.remove(&id2);
                                true
                            }
                            _ => false,
                        }
                    };
                    if empty {
                        emit_status(
                            &app2,
                            status_payload(
                                &id2,
                                "failed",
                                format!("Restart gagal: {e}"),
                                Some(step_index),
                            ),
                        );
                    }
                }
            }
            return;
        }

        let all_done = {
            let mut map = registry2.lock();
            match map.get_mut(&id2) {
                Some(rp) if rp.pids.is_empty() && rp.started && !rp.stopping => {
                    map.remove(&id2);
                    true
                }
                _ => false,
            }
        };
        if all_done {
            let message = if exit_ok {
                format!("Semua proses selesai ({exit_label}).")
            } else {
                format!("Proses berhenti dengan {exit_label}. Cek log untuk detail.")
            };
            bus2.log(&id2, step_index, "system", &message);
            emit_status(
                &app2,
                status_payload_with_exit(
                    &id2,
                    if exit_ok { "stopped" } else { "failed" },
                    message,
                    Some(step_index),
                    exit_code,
                ),
            );
        }
    });

    Ok(pid)
}

fn exit_code_label(exit: std::process::ExitStatus) -> String {
    match exit.code() {
        Some(code) => format!("exit code {code}"),
        None => "dihentikan".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Start
// ---------------------------------------------------------------------------

/// Validasi port pengganti dari frontend: bukan 0, unik, dan belum dipakai.
fn validate_port_overrides(
    overrides: &HashMap<usize, u16>,
    active: &[PortInfo],
) -> Result<(), String> {
    let mut seen = HashSet::new();
    for (step_index, port) in overrides {
        if *port == 0 {
            return Err(format!("Step {step_index}: port pengganti 0 tidak valid."));
        }
        if !seen.insert(*port) {
            return Err(format!(
                "Port pengganti {port} dipakai lebih dari satu step."
            ));
        }
        let occupied = active.iter().any(|p| {
            p.port == *port && p.pid != 0 && (p.protocol == "UDP" || p.status == "LISTENING")
        });
        if occupied {
            return Err(format!(
                "Port pengganti {port} (step {step_index}) juga sedang dipakai."
            ));
        }
    }
    Ok(())
}

pub fn begin_start(
    app: &AppHandle,
    registry: &Arc<RunningRegistry>,
    bus: &LogBus,
    profile_id: &str,
    options: StartOptions,
) -> Result<(), String> {
    let profile = storage::with_store_read(|store| {
        store
            .profiles
            .iter()
            .find(|p| p.id == profile_id)
            .cloned()
            .ok_or_else(|| format!("Profile {profile_id} tidak ditemukan"))
    })?;

    if profile.steps.is_empty() {
        return Err(format!("Profile '{}' tidak punya step.", profile.name));
    }

    // Preflight di backend: menutup celah antara pengecekan di UI dan eksekusi.
    let active = get_active_ports_blocking()?;
    let result = preflight_with_ports(&profile, &active);
    if !result.errors.is_empty() {
        return Err(result.errors.join(" "));
    }

    // Port pengganti juga harus bebas dan unik — kalau tidak, start tetap
    // gagal walaupun konflik asalnya sudah dianggap "selesai" oleh UI.
    validate_port_overrides(&options.port_overrides, &active)?;

    let unresolved: Vec<String> = result
        .conflicts
        .iter()
        .filter(|c| {
            !options.force
                && !options.skip_steps.contains(&c.step_index)
                && !options.port_overrides.contains_key(&c.step_index)
        })
        .map(|c| format!("port {} (step {})", c.port, c.step_index))
        .collect();
    if !unresolved.is_empty() {
        return Err(format!(
            "Port bentrok: {}. Pilih kill, ganti port, atau lewati step.",
            unresolved.join(", ")
        ));
    }

    let generation = NEXT_GENERATION.fetch_add(1, Ordering::SeqCst);
    {
        let mut map = registry.lock();
        if map.contains_key(profile_id) {
            return Err(format!(
                "Profile '{}' sedang berjalan — stop dulu sebelum start ulang.",
                profile.name
            ));
        }
        map.insert(
            profile_id.to_string(),
            RunningProfile {
                generation,
                ..Default::default()
            },
        );
    }

    let app = app.clone();
    let registry = registry.clone();
    let bus = bus.clone();
    tauri::async_runtime::spawn(async move {
        run_profile(app, registry, bus, profile, options, generation).await;
    });

    Ok(())
}

async fn run_profile(
    app: AppHandle,
    registry: Arc<RunningRegistry>,
    bus: LogBus,
    profile: Profile,
    options: StartOptions,
    generation: u64,
) {
    let profile_id = profile.id.clone();
    let mut failure: Option<String> = None;

    emit_status(
        &app,
        status_payload(
            &profile_id,
            "starting",
            format!("Memulai profile '{}'…", profile.name),
            None,
        ),
    );
    bus.log(
        &profile_id,
        0,
        "system",
        format!("=== Start profile: {} ===", profile.name),
    );

    for (idx, step) in profile.steps.iter().enumerate() {
        // Stop bisa datang kapan saja selama start; jangan lanjut ke step
        // berikutnya (apalagi membuka URL) setelah user menekan Stop.
        let cancelled = {
            let map = registry.lock();
            map.get(&profile_id)
                .map(|rp| rp.stopping || shutting_down() || rp.generation != generation)
                .unwrap_or(true)
        };
        if cancelled {
            journal::remove_profile(&profile_id);
            bus.log(
                &profile_id,
                idx,
                "system",
                "=== Start dibatalkan (profile dihentikan) ===",
            );
            return;
        }

        if options.skip_steps.contains(&idx) {
            emit_step_status(
                &app,
                StepStatusPayload {
                    profile_id: profile_id.clone(),
                    step_index: idx,
                    state: "skipped".into(),
                    port: match step {
                        Step::Command { port, .. } => *port,
                        _ => None,
                    },
                },
            );
            bus.log(
                &profile_id,
                idx,
                "system",
                format!("[step {idx}] dilewati — port sudah dipakai proses lain."),
            );
            continue;
        }

        emit_status(
            &app,
            status_payload(
                &profile_id,
                "starting",
                format!("Menjalankan step {idx} ({})", step.kind()),
                Some(idx),
            ),
        );

        match step {
            Step::Exe { path } => match spawn_exe(
                app.clone(),
                registry.clone(),
                bus.clone(),
                profile_id.clone(),
                idx,
                path.clone(),
                Some(generation),
            ) {
                Ok(_) => {}
                Err(e) => {
                    bus.log(&profile_id, idx, "system", format!("[step {idx}] {e}"));
                    failure = Some(e);
                }
            },
            Step::Command {
                port,
                delay_ms,
                optional,
                ..
            } => {
                if let Some(delay) = delay_ms {
                    if *delay > 0 {
                        tokio::time::sleep(Duration::from_millis((*delay).min(60_000))).await;
                    }
                }

                let override_port = options.port_overrides.get(&idx).copied();
                let effective_port = override_port.or(*port);
                if let (Some(old), Some(new)) = (*port, override_port) {
                    if old != new {
                        bus.log(
                            &profile_id,
                            idx,
                            "system",
                            format!(
                                "[step {idx}] port diganti {old} -> {new} (env PORT/VITE_PORT/APP_PORT). \
                                 Step open_url ke port lama tidak otomatis berubah."
                            ),
                        );
                    }
                }

                match spawn_command_step(
                    app.clone(),
                    registry.clone(),
                    bus.clone(),
                    profile_id.clone(),
                    idx,
                    step.clone(),
                    override_port,
                    Some(generation),
                ) {
                    Ok(pid) => {
                        let readiness = effective_readiness(step, effective_port);
                        if let Some(port) = effective_port {
                            emit_step_status(
                                &app,
                                StepStatusPayload {
                                    profile_id: profile_id.clone(),
                                    step_index: idx,
                                    state: "spawned".into(),
                                    port: Some(port),
                                },
                            );
                            if readiness != "none" {
                                let ready =
                                    wait_ready(readiness, port, ready_timeout(step), pid).await;
                                let state = if ready { "ready" } else { "timeout" };
                                emit_step_status(
                                    &app,
                                    StepStatusPayload {
                                        profile_id: profile_id.clone(),
                                        step_index: idx,
                                        state: state.into(),
                                        port: Some(port),
                                    },
                                );
                                if ready {
                                    bus.log(
                                        &profile_id,
                                        idx,
                                        "system",
                                        format!("[step {idx}] port {port} siap ({readiness})."),
                                    );
                                } else {
                                    bus.log(
                                        &profile_id,
                                        idx,
                                        "system",
                                        format!(
                                            "[step {idx}] port {port} belum siap dalam {} detik — lanjut.",
                                            ready_timeout(step).as_secs()
                                        ),
                                    );
                                }
                            }
                        }
                    }
                    Err(e) => {
                        if optional.unwrap_or(false) {
                            bus.log(
                                &profile_id,
                                idx,
                                "system",
                                format!("[step {idx}] dilewati (opsional): {e}"),
                            );
                        } else {
                            bus.log(&profile_id, idx, "system", format!("[step {idx}] {e}"));
                            failure = Some(e);
                        }
                    }
                }
            }
            Step::OpenUrl { url } => {
                bus.log(
                    &profile_id,
                    idx,
                    "system",
                    format!("[step {idx}] membuka URL: {url}"),
                );
                if let Err(e) = open_url(url) {
                    bus.log(&profile_id, idx, "system", format!("[step {idx}] {e}"));
                }
            }
        }

        if let Some(err) = failure {
            // Kalau profile sudah dihentikan user saat masih start, jangan
            // laporkan sebagai "failed" — prosesnya memang dibatalkan.
            let cancelled = {
                let map = registry.lock();
                map.get(&profile_id)
                    .map(|rp| rp.stopping || shutting_down() || rp.generation != generation)
                    .unwrap_or(true)
            };
            if cancelled {
                journal::remove_profile(&profile_id);
                bus.log(
                    &profile_id,
                    idx,
                    "system",
                    "=== Start dibatalkan (profile dihentikan) ===",
                );
                emit_status(
                    &app,
                    status_payload(&profile_id, "stopped", "Dibatalkan.".into(), None),
                );
                return;
            }

            // Rollback: hentikan semua yang sudah sempat jalan.
            let pids = {
                let mut map = registry.lock();
                match map.get_mut(&profile_id) {
                    Some(rp) => {
                        rp.stopping = true;
                        rp.pids.clone()
                    }
                    None => Vec::new(),
                }
            };
            let filtered = verified_pids(&profile_id, &pids);
            let rollback_id = profile_id.clone();
            let _ =
                tauri::async_runtime::spawn_blocking(move || stop_pids(&rollback_id, &filtered))
                    .await;
            registry.lock().remove(&profile_id);
            journal::remove_profile(&profile_id);
            bus.log(
                &profile_id,
                idx,
                "system",
                format!("=== Profile gagal: {err} ==="),
            );
            emit_status(
                &app,
                status_payload(&profile_id, "failed", format!("Gagal: {err}"), None),
            );
            return;
        }
    }

    let still_running = {
        let mut map = registry.lock();
        match map.get_mut(&profile_id) {
            Some(rp) if !rp.pids.is_empty() => {
                rp.started = true;
                true
            }
            Some(_) => {
                map.remove(&profile_id);
                false
            }
            None => false,
        }
    };

    if still_running {
        bus.log(&profile_id, 0, "system", "=== Semua step siap ===");
        emit_status(
            &app,
            status_payload(&profile_id, "running", "Semua step siap.".into(), None),
        );
    } else {
        journal::remove_profile(&profile_id);
        bus.log(
            &profile_id,
            0,
            "system",
            "=== Semua step selesai dijalankan ===",
        );
        emit_status(
            &app,
            status_payload(&profile_id, "stopped", "Semua proses selesai.".into(), None),
        );
    }
}

// ---------------------------------------------------------------------------
// Stop & kill
// ---------------------------------------------------------------------------

/// Hentikan profile secara sinkron (dipakai juga oleh `delete_profile` dan
/// saat aplikasi keluar). Verifikasi start time mencegah kill PID yang sudah
/// di-reuse proses lain.
pub fn stop_profile_blocking(
    app: &AppHandle,
    registry: &RunningRegistry,
    bus: &LogBus,
    profile_id: &str,
) -> usize {
    let pids = {
        let mut map = registry.lock();
        match map.get_mut(profile_id) {
            Some(rp) => {
                rp.stopping = true;
                rp.pids.clone()
            }
            None => Vec::new(),
        }
    };
    let filtered = verified_pids(profile_id, &pids);
    stop_pids(profile_id, &filtered);
    registry.lock().remove(profile_id);
    journal::remove_profile(profile_id);

    bus.log(
        profile_id,
        0,
        "system",
        format!("=== Stop profile ({} proses) ===", filtered.len()),
    );
    emit_status(
        app,
        status_payload(
            profile_id,
            "stopped",
            format!("Dihentikan ({} proses).", filtered.len()),
            None,
        ),
    );

    filtered.len()
}

/// Kill proses yang memegang sebuah port, setelah memastikan PID-nya memang
/// masih pemilik port tersebut (anti salah bunuh).
pub fn kill_port_owner(pid: u32, port: u16) -> Result<(), String> {
    let active = get_active_ports_blocking()?;
    let owns = active.iter().any(|p| {
        p.port == port && p.pid == pid && (p.protocol == "UDP" || p.status == "LISTENING")
    });
    if !owns {
        return Err(format!(
            "Proses {pid} sudah tidak memegang port {port}. Cek ulang konflik."
        ));
    }
    stop_tree(pid);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Step;
    use std::collections::HashMap;

    fn cmd_step(port: Option<u16>, cwd: &str, cmd: &str) -> Step {
        Step::Command {
            cwd: cwd.into(),
            cmd: cmd.into(),
            port,
            shell: None,
            env: Some(HashMap::new()),
            readiness: None,
            ready_timeout_ms: None,
            delay_ms: None,
            optional: None,
            auto_restart: None,
            max_restarts: None,
        }
    }

    fn profile(steps: Vec<Step>) -> Profile {
        Profile {
            id: "p1".into(),
            name: "Test".into(),
            folder: None,
            order: 0,
            external: false,
            steps,
        }
    }

    fn port_info(port: u16, pid: u32, status: &str) -> PortInfo {
        PortInfo {
            port,
            protocol: "TCP".into(),
            status: status.into(),
            pid,
            process_name: "node.exe".into(),
            exe_path: "C:/node.exe".into(),
            cwd: "C:/project".into(),
        }
    }

    #[test]
    fn detects_listening_conflict() {
        let p = profile(vec![cmd_step(Some(3000), ".", "npm run dev")]);
        let active = vec![
            port_info(3000, 4242, "LISTENING"),
            port_info(3000, 4242, "ESTABLISHED"),
        ];
        let result = preflight_with_ports(&p, &active);
        assert_eq!(result.conflicts.len(), 1);
        assert_eq!(result.conflicts[0].pids, vec![4242]);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn ignores_established_only_connections() {
        let p = profile(vec![cmd_step(Some(3000), ".", "npm run dev")]);
        let active = vec![port_info(3000, 4242, "ESTABLISHED")];
        let result = preflight_with_ports(&p, &active);
        assert!(result.conflicts.is_empty());
    }

    #[test]
    fn flags_duplicate_ports_in_profile() {
        let p = profile(vec![
            cmd_step(Some(3000), ".", "npm run dev"),
            cmd_step(Some(3000), ".", "npm run api"),
        ]);
        let result = preflight_with_ports(&p, &[]);
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].contains("3000"));
    }

    #[test]
    fn flags_empty_command_fields() {
        let p = profile(vec![cmd_step(None, " ", " ")]);
        let result = preflight_with_ports(&p, &[]);
        assert_eq!(result.errors.len(), 2);
    }

    #[test]
    fn readiness_defaults_follow_port() {
        let with_port = cmd_step(Some(3000), ".", "npm run dev");
        assert_eq!(effective_readiness(&with_port, Some(3000)), "tcp");
        let without_port = cmd_step(None, ".", "npm run dev");
        assert_eq!(effective_readiness(&without_port, None), "none");
    }

    #[test]
    fn tokenizer_keeps_quoted_paths_and_args() {
        let parts = split_command(r#""C:\Program Files\node\node.exe" --flag "a b""#).unwrap();
        assert_eq!(
            parts,
            vec![
                r"C:\Program Files\node\node.exe".to_string(),
                "--flag".to_string(),
                "a b".to_string()
            ]
        );
        assert!(split_command(r#""unclosed"#).is_err());
    }

    #[test]
    fn rejects_invalid_port_overrides() {
        assert!(validate_port_overrides(&HashMap::from([(0, 0)]), &[]).is_err());

        let duplicates = HashMap::from([(0, 3000u16), (1, 3000u16)]);
        assert!(validate_port_overrides(&duplicates, &[]).is_err());

        let occupied = HashMap::from([(0, 3000u16)]);
        let active = vec![port_info(3000, 4242, "LISTENING")];
        assert!(validate_port_overrides(&occupied, &active).is_err());

        let free = HashMap::from([(0, 3001u16)]);
        assert!(validate_port_overrides(&free, &active).is_ok());
    }

    #[test]
    fn status_payload_exposes_exit_code() {
        let terminal = status_payload_with_exit("p", "stopped", "done".into(), Some(1), Some(7));
        let value = serde_json::to_value(terminal).unwrap();
        assert_eq!(value["exitCode"], 7);
        assert_eq!(value["status"], "stopped");

        let plain = status_payload("p", "running", "ok".into(), None);
        assert!(serde_json::to_value(plain).unwrap()["exitCode"].is_null());
    }
}
