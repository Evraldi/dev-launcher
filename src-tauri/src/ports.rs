// Deteksi port aktif + kill process — logic di-reuse dari project port-monitor.
use serde::Serialize;
use std::collections::HashMap;
use sysinfo::{Pid, ProcessesToUpdate, System, UpdateKind};

#[cfg(windows)]
use crate::util::CREATE_NO_WINDOW;

#[derive(Serialize, Clone)]
pub struct PortInfo {
    pub port: u16,
    pub protocol: String,
    pub status: String,
    pub pid: u32,
    pub process_name: String,
    pub exe_path: String,
    pub cwd: String,
}

#[tauri::command]
pub fn get_active_ports() -> Result<Vec<PortInfo>, String> {
    #[cfg(windows)]
    let mut cmd = {
        use std::os::windows::process::CommandExt;
        let mut c = std::process::Command::new("netstat");
        c.creation_flags(CREATE_NO_WINDOW);
        c
    };
    #[cfg(not(windows))]
    let mut cmd = std::process::Command::new("netstat");

    let output = cmd
        .arg("-ano")
        .output()
        .map_err(|e| format!("Gagal menjalankan netstat: {e}"))?;

    if !output.status.success() {
        return Err("netstat keluar dengan status non-zero".into());
    }

    let text = String::from_utf8_lossy(&output.stdout);

    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        sysinfo::ProcessRefreshKind::new()
            .with_exe(UpdateKind::OnlyIfNotSet)
            .with_cwd(UpdateKind::OnlyIfNotSet),
    );
    let pid_names: HashMap<u32, (String, String, String)> = sys
        .processes()
        .iter()
        .map(|(pid, process)| {
            (
                pid.as_u32(),
                (
                    process.name().to_string_lossy().to_string(),
                    process
                        .exe()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_default(),
                    process
                        .cwd()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_default(),
                ),
            )
        })
        .collect();

    let mut ports: Vec<PortInfo> = Vec::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split_whitespace().collect();

        if cols.len() < 4 {
            continue;
        }
        let protocol = cols[0].to_uppercase();
        if protocol != "TCP" && protocol != "UDP" {
            continue;
        }

        let local = cols[1];
        let port = match local.rsplit_once(':') {
            Some((_, port)) => port,
            None => continue,
        };
        let Ok(port) = port.parse::<u16>() else {
            continue;
        };

        let (status, pid_str) = if protocol == "TCP" && cols.len() >= 5 {
            (cols[3].to_string(), cols[4])
        } else {
            (String::new(), cols[3])
        };

        let Ok(pid) = pid_str.parse::<u32>() else {
            continue;
        };

        let (name, exe_path, cwd) = pid_names
            .get(&pid)
            .cloned()
            .unwrap_or_else(|| ("<unknown>".into(), String::new(), String::new()));

        ports.push(PortInfo {
            port,
            protocol,
            status,
            pid,
            process_name: name,
            exe_path,
            cwd,
        });
    }

    Ok(ports)
}

/// Cari port bebas mulai dari `start` (naik ke atas, wrap di 65535).
/// Port 0 dan ephemeral range di-skip supaya tidak dapet port sistem.
#[tauri::command]
pub fn find_free_port(start: u16) -> Result<u16, String> {
    let used: std::collections::HashSet<u16> = get_active_ports()?
        .iter()
        .map(|p| p.port)
        .collect();
    let start = start.max(1);
    for offset in 0..u16::MAX as u32 {
        let candidate = (start as u32 + offset) % 65536;
        if candidate == 0 {
            continue;
        }
        let candidate = candidate as u16;
        if !used.contains(&candidate) {
            return Ok(candidate);
        }
    }
    Err("Tidak ada port bebas yang ditemukan.".into())
}

#[tauri::command]
pub fn kill_process(pid: u32) -> Result<(), String> {
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All, true);
    match sys.process(Pid::from_u32(pid)) {
        Some(process) => {
            if process.kill() {
                Ok(())
            } else {
                Err(format!("Gagal mematikan proses {pid}"))
            }
        }
        None => Err(format!("Proses {pid} tidak ditemukan")),
    }
}
