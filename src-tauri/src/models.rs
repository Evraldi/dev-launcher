use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Versi skema penyimpanan. Naikkan saat ada perubahan format yang butuh migrasi.
pub const STORE_VERSION: u32 = 2;

// Batas validasi input backend. Frontend tidak dipercaya mentah-mentah.
const MAX_ID_LEN: usize = 128;
const MAX_NAME_LEN: usize = 200;
const MAX_PATH_LEN: usize = 4096;
const MAX_CMD_LEN: usize = 8192;
const MAX_ENV_ENTRIES: usize = 64;
const MAX_ENV_KEY_LEN: usize = 256;
const MAX_ENV_VALUE_LEN: usize = 4096;
const MAX_STEPS: usize = 100;
const MAX_READY_TIMEOUT_MS: u64 = 600_000;
const MAX_DELAY_MS: u64 = 60_000;
const MAX_RESTARTS: u32 = 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub folder: Option<String>,
    #[serde(default)]
    pub order: u32,
    /// Profile yang berasal dari `conf.jsonc` (read-only di UI).
    /// `#[serde(default)]` menjaga kompatibilitas dengan profil lama.
    #[serde(default)]
    pub external: bool,
    pub steps: Vec<Step>,
}

/// Bentuk penyimpanan profil + folder.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Store {
    /// Versi skema; file lama (tanpa field ini) otomatis dianggap versi 1.
    #[serde(default = "legacy_version")]
    pub version: u32,
    #[serde(default)]
    pub folders: Vec<String>,
    #[serde(default)]
    pub profiles: Vec<Profile>,
}

fn legacy_version() -> u32 {
    1
}

/// Status berkas konfigurasi eksternal (`conf.jsonc`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigStatus {
    pub path: String,
    pub exists: bool,
    pub error: Option<String>,
}

impl Default for Store {
    fn default() -> Self {
        Store {
            version: STORE_VERSION,
            folders: Vec::new(),
            profiles: Vec::new(),
        }
    }
}

impl Store {
    /// Migrasi file lama ke versi skema terkini. Idempoten.
    pub fn migrate(&mut self) -> bool {
        let mut changed = false;
        if self.version < STORE_VERSION {
            // v1 -> v2: field baru pada step punya serde default, tidak ada
            // transformasi data yang diperlukan. Versi tetap dinaikkan supaya
            // migrasi berikutnya bisa diasumsikan berjalan dari v2.
            for profile in self.profiles.iter_mut() {
                for step in profile.steps.iter_mut() {
                    if let Step::OpenUrl { url } = step {
                        *url = normalize_url(url);
                    }
                }
            }
            self.version = STORE_VERSION;
            changed = true;
        }
        changed
    }
}

/// Lengkapi URL lama yang belum punya skema (`localhost:3000` -> `http://…`).
fn normalize_url(url: &str) -> String {
    let trimmed = url.trim();
    if trimmed.is_empty() || trimmed.contains("://") {
        return trimmed.to_string();
    }
    // Jangan sentuh path Windows seperti `C:\Windows\...`.
    if trimmed.as_bytes().get(1) == Some(&b':') {
        return trimmed.to_string();
    }
    format!("http://{trimmed}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_url_adds_scheme_only_for_hosts() {
        assert_eq!(normalize_url("localhost:3000"), "http://localhost:3000");
        assert_eq!(normalize_url("  example.com  "), "http://example.com");
        assert_eq!(normalize_url("https://example.com"), "https://example.com");
        assert_eq!(
            normalize_url("C:\\Windows\\notepad.exe"),
            "C:\\Windows\\notepad.exe"
        );
    }

    #[test]
    fn migrate_upgrades_version_and_urls() {
        let mut store = Store {
            version: 1,
            folders: Vec::new(),
            profiles: vec![Profile {
                id: "p".into(),
                name: "P".into(),
                folder: None,
                order: 0,
                external: false,
                steps: vec![Step::OpenUrl {
                    url: "localhost:3000".into(),
                }],
            }],
        };
        assert!(store.migrate());
        assert_eq!(store.version, STORE_VERSION);
        assert!(matches!(
            &store.profiles[0].steps[0],
            Step::OpenUrl { url } if url == "http://localhost:3000"
        ));
        // Idempoten: migrasi kedua tidak mengubah apa pun.
        assert!(!store.migrate());
    }

    fn command_profile() -> Profile {
        Profile {
            id: "p1".into(),
            name: "Profile".into(),
            folder: None,
            order: 0,
            external: false,
            steps: vec![Step::Command {
                cwd: ".".into(),
                cmd: "npm run dev".into(),
                port: Some(3000),
                shell: None,
                env: None,
                readiness: None,
                ready_timeout_ms: None,
                delay_ms: None,
                optional: None,
                auto_restart: None,
                max_restarts: None,
            }],
        }
    }

    #[test]
    fn validate_profile_accepts_well_formed_input() {
        let mut profile = command_profile();
        profile.folder = Some("Kerja".into());
        if let Step::Command { shell, env, .. } = &mut profile.steps[0] {
            *shell = Some("powershell".into());
            let mut vars = HashMap::new();
            vars.insert("NODE_ENV".to_string(), "development".to_string());
            *env = Some(vars);
        }
        assert!(validate_profile(&profile).is_ok());
    }

    #[test]
    fn validate_profile_rejects_unknown_shell() {
        let mut profile = command_profile();
        if let Step::Command { shell, .. } = &mut profile.steps[0] {
            *shell = Some("fish".into());
        }
        let err = validate_profile(&profile).unwrap_err();
        assert!(err.contains("shell"), "{err}");
    }

    #[test]
    fn validate_profile_rejects_invalid_ports() {
        let mut profile = command_profile();
        if let Step::Command { port, .. } = &mut profile.steps[0] {
            *port = Some(0);
        }
        assert!(validate_profile(&profile).unwrap_err().contains("port"));

        let mut profile = command_profile();
        profile.steps.push(profile.steps[0].clone());
        let err = validate_profile(&profile).unwrap_err();
        assert!(err.contains("port"), "{err}");
    }

    #[test]
    fn validate_profile_rejects_too_many_env_entries() {
        let mut profile = command_profile();
        let mut vars = HashMap::new();
        for i in 0..65 {
            vars.insert(format!("KEY_{i}"), "value".to_string());
        }
        if let Step::Command { env, .. } = &mut profile.steps[0] {
            *env = Some(vars);
        }
        let err = validate_profile(&profile).unwrap_err();
        assert!(err.contains("environment"), "{err}");
    }

    #[test]
    fn validate_profile_rejects_out_of_range_timing() {
        let mut profile = command_profile();
        if let Step::Command {
            ready_timeout_ms, ..
        } = &mut profile.steps[0]
        {
            *ready_timeout_ms = Some(600_001);
        }
        let err = validate_profile(&profile).unwrap_err();
        assert!(err.contains("readyTimeoutMs"), "{err}");

        let mut profile = command_profile();
        if let Step::Command { delay_ms, .. } = &mut profile.steps[0] {
            *delay_ms = Some(120_000);
        }
        assert!(validate_profile(&profile).unwrap_err().contains("delayMs"));

        let mut profile = command_profile();
        if let Step::Command { max_restarts, .. } = &mut profile.steps[0] {
            *max_restarts = Some(21);
        }
        assert!(validate_profile(&profile)
            .unwrap_err()
            .contains("maxRestarts"));
    }

    #[test]
    fn validate_profile_rejects_missing_fields_and_long_command() {
        let mut profile = command_profile();
        profile.id = " ".into();
        assert!(validate_profile(&profile).is_err());

        let mut profile = command_profile();
        profile.folder = Some(" ".into());
        assert!(validate_profile(&profile).unwrap_err().contains("folder"));

        let mut profile = command_profile();
        if let Step::Command { cmd, .. } = &mut profile.steps[0] {
            *cmd = "x".repeat(8193);
        }
        assert!(validate_profile(&profile).unwrap_err().contains("panjang"));
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Step {
    #[serde(rename = "exe")]
    Exe { path: String },
    #[serde(rename = "command")]
    Command {
        cwd: String,
        cmd: String,
        #[serde(default)]
        port: Option<u16>,
        /// Shell yang dipakai untuk menjalankan `cmd`:
        /// `None`/`"auto"` = perilaku lama (pecah perintah, shim `.cmd` lewat cmd.exe),
        /// `"cmd"` = Command Prompt, `"powershell"` = PowerShell, `"bash"` = Bash.
        /// Command multi-baris otomatis ditulis ke script sementara.
        #[serde(default)]
        shell: Option<String>,
        /// Environment variable tambahan untuk proses ini.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        env: Option<HashMap<String, String>>,
        /// Cara memastikan step siap: `none` | `tcp` | `http`.
        /// Kosong = perilaku lama (tcp kalau `port` diisi, selain itu none).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        readiness: Option<String>,
        /// Timeout readiness dalam milidetik (default 30_000).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ready_timeout_ms: Option<u64>,
        /// Jeda sebelum step dijalankan, dalam milidetik.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        delay_ms: Option<u64>,
        /// Kalau `true`, kegagalan step ini tidak menggagalkan seluruh profile.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        optional: Option<bool>,
        /// Restart otomatis kalau proses keluar sendiri (crash).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        auto_restart: Option<bool>,
        /// Batas jumlah restart otomatis (default 3).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_restarts: Option<u32>,
    },
    #[serde(rename = "open_url")]
    OpenUrl { url: String },
}

impl Step {
    pub fn kind(&self) -> &'static str {
        match self {
            Step::Exe { .. } => "exe",
            Step::Command { .. } => "command",
            Step::OpenUrl { .. } => "open_url",
        }
    }
}

/// Validasi profile di sisi backend. Dipakai baik untuk profil dari UI
/// (`save_profile`) maupun dari `conf.jsonc` (`storage::validate_config_profiles`),
/// supaya aturannya tidak bisa berbeda antar jalur masuk.
pub fn validate_profile(profile: &Profile) -> Result<(), String> {
    let id = profile.id.trim();
    if id.is_empty() {
        return Err("ID profile kosong.".into());
    }
    if id.chars().count() > MAX_ID_LEN {
        return Err(format!(
            "ID profile terlalu panjang (maks. {MAX_ID_LEN} karakter)."
        ));
    }
    if id.chars().any(|c| c.is_control()) {
        return Err("ID profile mengandung karakter kontrol.".into());
    }

    let name = profile.name.trim();
    if name.is_empty() {
        return Err("Nama profile wajib diisi.".into());
    }
    if name.chars().count() > MAX_NAME_LEN {
        return Err(format!(
            "Nama profile terlalu panjang (maks. {MAX_NAME_LEN} karakter)."
        ));
    }

    if let Some(folder) = &profile.folder {
        let folder = folder.trim();
        if folder.is_empty() {
            return Err("Nama folder kosong.".into());
        }
        if folder.chars().count() > MAX_NAME_LEN {
            return Err(format!(
                "Nama folder terlalu panjang (maks. {MAX_NAME_LEN} karakter)."
            ));
        }
        if folder.chars().any(|c| c.is_control()) {
            return Err("Nama folder mengandung karakter kontrol.".into());
        }
    }

    if profile.steps.is_empty() {
        return Err("Profile minimal punya satu step.".into());
    }
    if profile.steps.len() > MAX_STEPS {
        return Err(format!(
            "Profile terlalu banyak step (maks. {MAX_STEPS} step)."
        ));
    }

    let mut ports: HashMap<u16, usize> = HashMap::new();
    for (idx, step) in profile.steps.iter().enumerate() {
        match step {
            Step::Exe { path } => {
                let path = path.trim();
                if path.is_empty() {
                    return Err(format!("Step {idx}: path exe kosong."));
                }
                if path.chars().count() > MAX_PATH_LEN {
                    return Err(format!("Step {idx}: path exe terlalu panjang."));
                }
                if path.chars().any(|c| c.is_control()) {
                    return Err(format!("Step {idx}: path exe mengandung karakter kontrol."));
                }
            }
            Step::OpenUrl { url } => {
                let url = url.trim();
                if url.is_empty() {
                    return Err(format!("Step {idx}: URL kosong."));
                }
                if url.chars().count() > MAX_PATH_LEN {
                    return Err(format!("Step {idx}: URL terlalu panjang."));
                }
                if url.chars().any(|c| c.is_control()) {
                    return Err(format!("Step {idx}: URL mengandung karakter kontrol."));
                }
                if !crate::util::is_safe_http_url(url) {
                    return Err(format!(
                        "Step {idx}: hanya URL http/https yang diizinkan (contoh: http://localhost:3000)."
                    ));
                }
            }
            Step::Command {
                cwd,
                cmd,
                port,
                shell,
                env,
                readiness,
                ready_timeout_ms,
                delay_ms,
                max_restarts,
                ..
            } => {
                let cwd = cwd.trim();
                if cwd.is_empty() {
                    return Err(format!("Step {idx}: working directory kosong."));
                }
                if cwd.chars().count() > MAX_PATH_LEN {
                    return Err(format!("Step {idx}: working directory terlalu panjang."));
                }
                if cwd.chars().any(|c| c.is_control()) {
                    return Err(format!(
                        "Step {idx}: working directory mengandung karakter kontrol."
                    ));
                }

                if cmd.trim().is_empty() {
                    return Err(format!("Step {idx}: perintah kosong."));
                }
                if cmd.chars().count() > MAX_CMD_LEN {
                    return Err(format!(
                        "Step {idx}: perintah terlalu panjang (maks. {MAX_CMD_LEN} karakter)."
                    ));
                }

                if let Some(shell) = shell {
                    if !matches!(shell.as_str(), "auto" | "cmd" | "powershell" | "bash") {
                        return Err(format!(
                            "Step {idx}: shell '{shell}' tidak didukung (auto|cmd|powershell|bash)."
                        ));
                    }
                }

                if let Some(port) = port {
                    if *port == 0 {
                        return Err(format!("Step {idx}: port 0 tidak valid."));
                    }
                    if let Some(prev) = ports.insert(*port, idx) {
                        return Err(format!(
                            "Step {prev} dan step {idx} memakai port {port} yang sama."
                        ));
                    }
                }

                if let Some(env) = env {
                    if env.len() > MAX_ENV_ENTRIES {
                        return Err(format!(
                            "Step {idx}: terlalu banyak environment variable (maks. {MAX_ENV_ENTRIES})."
                        ));
                    }
                    for (key, value) in env {
                        let key = key.trim();
                        if key.is_empty() {
                            return Err(format!("Step {idx}: kunci environment kosong."));
                        }
                        if key.chars().count() > MAX_ENV_KEY_LEN {
                            return Err(format!("Step {idx}: kunci environment terlalu panjang."));
                        }
                        if key.chars().any(|c| c.is_control() || c == '=' || c == '\0') {
                            return Err(format!(
                                "Step {idx}: kunci environment mengandung karakter tidak valid."
                            ));
                        }
                        if value.chars().count() > MAX_ENV_VALUE_LEN {
                            return Err(format!("Step {idx}: nilai environment terlalu panjang."));
                        }
                        if value.contains('\0') {
                            return Err(format!("Step {idx}: nilai environment mengandung NUL."));
                        }
                    }
                }

                if let Some(readiness) = readiness {
                    if !matches!(readiness.as_str(), "" | "none" | "tcp" | "http") {
                        return Err(format!(
                            "Step {idx}: readiness '{readiness}' tidak dikenal (none|tcp|http)."
                        ));
                    }
                }
                if let Some(timeout) = ready_timeout_ms {
                    if !(1_000..=MAX_READY_TIMEOUT_MS).contains(timeout) {
                        return Err(format!(
                            "Step {idx}: readyTimeoutMs harus antara 1000 dan {MAX_READY_TIMEOUT_MS}."
                        ));
                    }
                }
                if let Some(delay) = delay_ms {
                    if *delay > MAX_DELAY_MS {
                        return Err(format!(
                            "Step {idx}: delayMs tidak boleh lebih dari {MAX_DELAY_MS}."
                        ));
                    }
                }
                if let Some(max) = max_restarts {
                    if *max > MAX_RESTARTS {
                        return Err(format!(
                            "Step {idx}: maxRestarts tidak boleh lebih dari {MAX_RESTARTS}."
                        ));
                    }
                }
            }
        }
    }

    Ok(())
}

/// Hasil pre-flight pengecekan bentrok port untuk sebuah profile.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortConflict {
    pub step_index: usize,
    pub port: u16,
    pub pids: Vec<u32>,
    pub process_names: Vec<String>,
    pub exe_paths: Vec<String>,
    pub cwds: Vec<String>,
}

/// Hasil pre-flight lengkap: konflik port + kesalahan konfigurasi.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreflightResult {
    pub conflicts: Vec<PortConflict>,
    pub errors: Vec<String>,
}

/// Profil yang masih punya proses hidup (dipakai frontend saat app dibuka).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeInfo {
    pub profile_id: String,
    pub pids: Vec<u32>,
    pub started_at: u64,
}
