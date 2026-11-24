// Bus log terpusat: menampung baris log dari banyak proses, menulis ke disk
// secara batch, dan mengirim satu event per batch ke frontend.
//
// Kenapa batch? Dev server (Vite/Next/webpack) bisa mencetak ratusan baris per
// detik. Satu event IPC per baris membuat WebView kewalahan; satu event per
// ~100ms jauh lebih ringan dan tetap terasa realtime.
use crate::storage;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;

const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;
const CHANNEL_CAPACITY: usize = 8192;
const FLUSH_INTERVAL_MS: u64 = 100;
const MAX_BATCH: usize = 500;
const RETENTION_INTERVAL: Duration = Duration::from_secs(10 * 60);
const MAX_TOTAL_LOG_BYTES: u64 = 50 * 1024 * 1024;
const LOG_RETENTION_DAYS: u64 = 14;

enum Msg {
    Line(LogPayload),
    Shutdown,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogPayload {
    pub profile_id: String,
    pub step_index: usize,
    pub source: String,
    pub message: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LogBatchPayload {
    entries: Vec<LogPayload>,
}

#[derive(Clone)]
pub struct LogBus {
    tx: mpsc::Sender<Msg>,
    sink: Arc<Mutex<LogSink>>,
}

impl LogBus {
    /// Antrekan satu baris log. Tidak pernah memblokir; kalau antrean penuh
    /// (proses spam output), baris dibuang supaya aplikasi tetap responsif.
    pub fn log(
        &self,
        profile_id: &str,
        step_index: usize,
        source: &str,
        message: impl Into<String>,
    ) {
        let payload = LogPayload {
            profile_id: profile_id.to_string(),
            step_index,
            source: source.to_string(),
            message: message.into(),
        };
        let _ = self.tx.try_send(Msg::Line(payload));
    }

    /// Siram antrean ke disk sebelum aplikasi keluar. Dipanggil sekali dari
    /// `RunEvent::Exit`; semua baris sebelum sinyal ini dijamin terkirim.
    pub async fn shutdown(&self) {
        let _ = self.tx.send(Msg::Shutdown).await;
    }

    /// Tutup file log sebuah profile (wajib sebelum file dihapus / di-rotate
    /// dari luar) agar tidak ada handle yang menahan file di Windows.
    pub fn close_profile(&self, profile_id: &str) {
        if let Ok(mut sink) = self.sink.lock() {
            sink.close(profile_id);
        }
    }
}

pub fn init(app: AppHandle) -> LogBus {
    let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);
    let sink = Arc::new(Mutex::new(LogSink::default()));
    let bus = LogBus {
        tx,
        sink: sink.clone(),
    };
    tauri::async_runtime::spawn(flush_loop(rx, sink, app));
    bus
}

async fn flush_loop(mut rx: mpsc::Receiver<Msg>, sink: Arc<Mutex<LogSink>>, app: AppHandle) {
    let mut batch: Vec<LogPayload> = Vec::with_capacity(MAX_BATCH);
    let mut last_retention = Instant::now();

    loop {
        let mut stop = false;
        let first = match rx.recv().await {
            Some(Msg::Line(payload)) => Some(payload),
            Some(Msg::Shutdown) | None => {
                stop = true;
                while let Ok(Msg::Line(payload)) = rx.try_recv() {
                    batch.push(payload);
                }
                None
            }
        };
        if let Some(payload) = first {
            batch.push(payload);
        } else if batch.is_empty() {
            break;
        }

        if !stop {
            let deadline = tokio::time::Instant::now() + Duration::from_millis(FLUSH_INTERVAL_MS);
            while batch.len() < MAX_BATCH {
                match tokio::time::timeout_at(deadline, rx.recv()).await {
                    Ok(Some(Msg::Line(payload))) => batch.push(payload),
                    Ok(Some(Msg::Shutdown)) => {
                        stop = true;
                        while let Ok(Msg::Line(payload)) = rx.try_recv() {
                            batch.push(payload);
                        }
                        break;
                    }
                    _ => break,
                }
            }
        }

        // Retensi juga jalan saat app menyala, tapi jangan menyentuh file log
        // yang sedang dibuka (di Windows file tersebut tidak bisa dihapus).
        if last_retention.elapsed() >= RETENTION_INTERVAL {
            last_retention = Instant::now();
            let active: HashSet<String> = sink
                .lock()
                .map(|guard| guard.files.keys().cloned().collect())
                .unwrap_or_default();
            let _ = storage::enforce_log_retention_excluding(
                MAX_TOTAL_LOG_BYTES,
                LOG_RETENTION_DAYS,
                &active,
            );
        }

        if let Ok(mut guard) = sink.lock() {
            for entry in &batch {
                guard.write(entry);
            }
            guard.flush();
        }
        let _ = app.emit(
            "profile-log-batch",
            LogBatchPayload {
                entries: batch.clone(),
            },
        );
        batch.clear();

        if stop {
            break;
        }
    }
}

#[derive(Default)]
struct LogSink {
    files: HashMap<String, BufWriter<File>>,
    sizes: HashMap<String, u64>,
}

impl LogSink {
    fn write(&mut self, entry: &LogPayload) {
        let id = entry.profile_id.as_str();
        let size = self.sizes.get(id).copied().unwrap_or(0);
        if size > MAX_LOG_BYTES {
            self.rotate(id);
        }
        if !self.files.contains_key(id) && !self.open(id) {
            return;
        }
        let Some(writer) = self.files.get_mut(id) else {
            return;
        };
        let line = format!(
            "[{}] [step {}] [{}] {}\n",
            crate::util::timestamp_now(),
            entry.step_index,
            entry.source,
            entry.message.replace('\0', "")
        );
        if writer.write_all(line.as_bytes()).is_ok() {
            *self.sizes.entry(id.to_string()).or_insert(0) += line.len() as u64;
        }
    }

    fn open(&mut self, id: &str) -> bool {
        let Ok(path) = storage::log_path(id) else {
            return false;
        };
        let mut size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        if size > MAX_LOG_BYTES {
            let rotated = storage::rotated_log_path(&path);
            let _ = std::fs::remove_file(&rotated);
            let _ = std::fs::rename(&path, &rotated);
            size = 0;
        }
        let Ok(file) = OpenOptions::new().create(true).append(true).open(&path) else {
            return false;
        };
        self.files.insert(id.to_string(), BufWriter::new(file));
        self.sizes.insert(id.to_string(), size);
        true
    }

    fn rotate(&mut self, id: &str) {
        self.close(id);
        let Ok(path) = storage::log_path(id) else {
            return;
        };
        let rotated = storage::rotated_log_path(&path);
        let _ = std::fs::remove_file(&rotated);
        let _ = std::fs::rename(&path, &rotated);
        self.sizes.insert(id.to_string(), 0);
    }

    fn close(&mut self, id: &str) {
        if let Some(mut writer) = self.files.remove(id) {
            let _ = writer.flush();
        }
        self.sizes.remove(id);
    }

    fn flush(&mut self) {
        for writer in self.files.values_mut() {
            let _ = writer.flush();
        }
    }
}
