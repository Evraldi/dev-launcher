use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Step {
    #[serde(rename = "exe")]
    Exe { path: String },
    #[serde(rename = "command")]
    Command { cwd: String, cmd: String, port: Option<u16> },
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

/// Hasil pre-flight pengecekan bentrok port untuk sebuah profile.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortConflict {
    pub step_index: usize,
    pub port: u16,
    pub pids: Vec<u32>,
    pub process_names: Vec<String>,
}
