use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DevLog {
    pub id: String,
    pub date: String,
    pub log_type: String,
    pub generated_at: String,
    pub summary: String,
    pub highlights: Vec<String>,
    pub projects_worked: Vec<ProjectWork>,
    pub stats: DevLogStats,
    pub sprint_score: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProjectWork {
    pub name: String,
    pub path: String,
    pub commits: u32,
    pub messages: u64,
    pub tokens: u64,
    pub duration_minutes: u64,
    pub key_changes: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DevLogStats {
    pub total_commits: u32,
    pub total_messages: u64,
    pub total_tokens: u64,
    pub total_files_changed: u32,
    pub total_insertions: u32,
    pub total_deletions: u32,
    pub active_hours: f64,
    pub projects_count: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub session_id: String,
    pub project: String,
    pub project_path: String,
    pub message_count: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read: u64,
    pub duration_minutes: u64,
    pub first_message: Option<String>,
    pub last_message: Option<String>,
}

fn sprt_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".claude").join("sprt"))
}

fn devlogs_dir(log_type: &str) -> Option<PathBuf> {
    sprt_dir().map(|d| d.join("devlogs").join(log_type))
}

fn filename_for_log(date: &str, log_type: &str) -> String {
    match log_type {
        "monthly" => format!("{}.json", &date[..7]),
        _ => format!("{}.json", date),
    }
}

pub fn save_devlog(log: &DevLog) -> Result<(), String> {
    let dir = devlogs_dir(&log.log_type).ok_or("Cannot determine storage directory")?;
    fs::create_dir_all(&dir).map_err(|e| format!("Cannot create directory: {}", e))?;

    let filename = filename_for_log(&log.date, &log.log_type);
    let path = dir.join(filename);
    let content = serde_json::to_string_pretty(log).map_err(|e| format!("Serialize error: {}", e))?;
    fs::write(path, content).map_err(|e| format!("Write error: {}", e))
}

pub fn get_devlog(date: &str, log_type: &str) -> Result<Option<DevLog>, String> {
    let dir = devlogs_dir(log_type).ok_or("Cannot determine storage directory")?;
    let filename = filename_for_log(date, log_type);
    let path = dir.join(filename);

    if !path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(&path).map_err(|e| format!("Read error: {}", e))?;
    let log: DevLog =
        serde_json::from_str(&content).map_err(|e| format!("Parse error: {}", e))?;
    Ok(Some(log))
}

pub fn list_devlogs(log_type: &str, limit: usize) -> Result<Vec<DevLog>, String> {
    let dir = devlogs_dir(log_type).ok_or("Cannot determine storage directory")?;
    if !dir.exists() {
        return Ok(vec![]);
    }

    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .map_err(|e| format!("Read dir error: {}", e))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|e| e == "json").unwrap_or(false))
        .collect();

    // Sort by filename descending (newest first)
    files.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    files.truncate(limit);

    let mut logs = vec![];
    for path in files {
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(log) = serde_json::from_str::<DevLog>(&content) {
                logs.push(log);
            }
        }
    }

    Ok(logs)
}

pub fn devlog_exists(date: &str, log_type: &str) -> bool {
    devlogs_dir(log_type)
        .map(|dir| dir.join(filename_for_log(date, log_type)).exists())
        .unwrap_or(false)
}
