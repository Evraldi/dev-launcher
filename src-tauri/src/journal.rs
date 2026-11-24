// Journal PID: mencatat proses yang di-spawn supaya aplikasi bisa
// merekonsiliasi status setelah restart/crash (tidak ada orphan yang hilang).
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalEntry {
    pub profile_id: String,
    pub step_index: usize,
    pub pid: u32,
    /// Waktu mulai proses (epoch detik dari OS) untuk verifikasi anti PID-reuse.
    #[serde(default)]
    pub started_at: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Journal {
    #[serde(default)]
    pub entries: Vec<JournalEntry>,
}

static LOCK: Mutex<()> = Mutex::new(());

fn path() -> PathBuf {
    crate::storage::base_dir().join("running.json")
}

pub fn load() -> Journal {
    let path = path();
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Journal::default();
    };
    match serde_json::from_str(&raw) {
        Ok(journal) => journal,
        Err(_) => {
            // Jangan diam-diam menganggap journal korup sebagai "kosong":
            // karantina dulu supaya jejak PID tetap bisa diaudit.
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or_default();
            let quarantine = path.with_file_name(format!("running.json.corrupt-{stamp}"));
            let _ = std::fs::rename(&path, &quarantine);
            Journal::default()
        }
    }
}

fn save(journal: &Journal) -> Result<(), String> {
    let path = path();
    if journal.entries.is_empty() {
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("Gagal menghapus journal: {e}")),
        }
    } else {
        crate::storage::write_json_atomic(&path, journal)
    }
}

pub fn add(profile_id: &str, step_index: usize, pid: u32, started_at: u64) {
    let _guard = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut journal = load();
    journal.entries.retain(|e| e.pid != pid);
    journal.entries.push(JournalEntry {
        profile_id: profile_id.to_string(),
        step_index,
        pid,
        started_at,
    });
    let _ = save(&journal);
}

pub fn remove_pid(profile_id: &str, pid: u32) {
    let _guard = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut journal = load();
    let before = journal.entries.len();
    journal
        .entries
        .retain(|e| !(e.pid == pid && e.profile_id == profile_id));
    if journal.entries.len() != before {
        let _ = save(&journal);
    }
}

pub fn remove_profile(profile_id: &str) {
    let _guard = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut journal = load();
    let before = journal.entries.len();
    journal.entries.retain(|e| e.profile_id != profile_id);
    if journal.entries.len() != before {
        let _ = save(&journal);
    }
}

pub fn replace(entries: Vec<JournalEntry>) {
    let _guard = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let _ = save(&Journal { entries });
}
