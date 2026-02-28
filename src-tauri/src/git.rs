use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GitActivity {
    pub repo_path: String,
    pub repo_name: String,
    pub branch: String,
    pub commits: Vec<GitCommit>,
    pub files_changed: u32,
    pub insertions: u32,
    pub deletions: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GitCommit {
    pub hash: String,
    pub message: String,
    pub author: String,
    pub timestamp: String,
    pub files_changed: u32,
    pub insertions: u32,
    pub deletions: u32,
}

/// Decode a Claude projects directory name to a filesystem path.
/// e.g. "-Users-sooyoungbae-butter" → "/Users/sooyoungbae/butter"
pub fn decode_project_path(dir_name: &str) -> String {
    if dir_name.is_empty() {
        return String::new();
    }
    // The directory name is the absolute path with "/" replaced by "-"
    // e.g., "-Users-sooyoungbae-butter" represents "/Users/sooyoungbae/butter"
    // We try to reconstruct by greedily matching existing directories.
    let parts: Vec<&str> = dir_name.split('-').collect();
    // Skip first empty segment (leading dash)
    let segments: Vec<&str> = if parts.first() == Some(&"") {
        parts[1..].to_vec()
    } else {
        parts.clone()
    };

    // Greedy path reconstruction: try longest matching segments
    let mut path = PathBuf::from("/");
    let mut i = 0;
    while i < segments.len() {
        // Try joining multiple segments (for names containing dashes)
        let mut best_len = 0;
        for j in (i + 1..=segments.len()).rev() {
            let candidate = segments[i..j].join("-");
            let test_path = path.join(&candidate);
            if test_path.exists() {
                path = test_path;
                best_len = j - i;
                break;
            }
        }
        if best_len == 0 {
            // No match — just use single segment
            path = path.join(segments[i]);
            i += 1;
        } else {
            i += best_len;
        }
    }
    path.to_string_lossy().to_string()
}

/// Discover project paths from ~/.claude/projects/
pub fn discover_project_paths() -> Vec<(String, String)> {
    let claude_dir = match dirs::home_dir() {
        Some(h) => h.join(".claude").join("projects"),
        None => return vec![],
    };
    if !claude_dir.exists() {
        return vec![];
    }

    let mut results = vec![];
    if let Ok(entries) = std::fs::read_dir(&claude_dir) {
        for entry in entries.flatten() {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                let dir_name = entry.file_name().to_string_lossy().to_string();
                let decoded = decode_project_path(&dir_name);
                if Path::new(&decoded).join(".git").exists() {
                    results.push((dir_name, decoded));
                }
            }
        }
    }
    results
}

/// Get current branch for a git repo
fn get_branch(repo_path: &str) -> String {
    Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(repo_path)
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "unknown".to_string())
}

/// Get the repo name from path (last component)
fn repo_name_from_path(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

/// Collect git activity for a specific date across all known projects.
pub fn collect_git_activity(date: &str) -> Vec<GitActivity> {
    let projects = discover_project_paths();
    let mut activities = vec![];

    for (_dir_name, repo_path) in &projects {
        if let Some(activity) = collect_repo_activity(repo_path, date) {
            if !activity.commits.is_empty() {
                activities.push(activity);
            }
        }
    }

    activities
}

/// Collect git activity for a date range (for weekly reports).
pub fn collect_git_activity_range(since: &str, until: &str) -> Vec<GitActivity> {
    let projects = discover_project_paths();
    let mut activities = vec![];

    for (_dir_name, repo_path) in &projects {
        if let Some(activity) = collect_repo_activity_range(repo_path, since, until) {
            if !activity.commits.is_empty() {
                activities.push(activity);
            }
        }
    }

    activities
}

fn collect_repo_activity(repo_path: &str, date: &str) -> Option<GitActivity> {
    let since = format!("{}T00:00:00", date);
    let until = format!("{}T23:59:59", date);
    collect_repo_activity_range(repo_path, &since, &until)
}

fn collect_repo_activity_range(repo_path: &str, since: &str, until: &str) -> Option<GitActivity> {
    // Get commits with stats
    let output = Command::new("git")
        .args([
            "log",
            &format!("--since={}", since),
            &format!("--until={}", until),
            "--format=%H|%s|%an|%aI",
            "--shortstat",
        ])
        .current_dir(repo_path)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let raw = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = raw.lines().collect();

    let mut commits = vec![];
    let mut total_files: u32 = 0;
    let mut total_ins: u32 = 0;
    let mut total_del: u32 = 0;
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i].trim();
        if line.is_empty() {
            i += 1;
            continue;
        }

        // Try to parse as commit line (hash|message|author|timestamp)
        let parts: Vec<&str> = line.splitn(4, '|').collect();
        if parts.len() == 4 && parts[0].len() == 40 {
            let mut fc: u32 = 0;
            let mut ins: u32 = 0;
            let mut del: u32 = 0;

            // Next non-empty line might be shortstat
            if i + 1 < lines.len() {
                let stat_line = lines[i + 1].trim();
                if stat_line.contains("changed") {
                    let (f, a, d) = parse_shortstat(stat_line);
                    fc = f;
                    ins = a;
                    del = d;
                    i += 1;
                }
            }

            total_files += fc;
            total_ins += ins;
            total_del += del;

            commits.push(GitCommit {
                hash: parts[0].to_string(),
                message: parts[1].to_string(),
                author: parts[2].to_string(),
                timestamp: parts[3].to_string(),
                files_changed: fc,
                insertions: ins,
                deletions: del,
            });
        }

        i += 1;
    }

    Some(GitActivity {
        repo_path: repo_path.to_string(),
        repo_name: repo_name_from_path(repo_path),
        branch: get_branch(repo_path),
        commits,
        files_changed: total_files,
        insertions: total_ins,
        deletions: total_del,
    })
}

/// Parse git shortstat line like "3 files changed, 120 insertions(+), 45 deletions(-)"
fn parse_shortstat(line: &str) -> (u32, u32, u32) {
    let mut files: u32 = 0;
    let mut ins: u32 = 0;
    let mut del: u32 = 0;

    for part in line.split(',') {
        let part = part.trim();
        let num: u32 = part
            .split_whitespace()
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        if part.contains("file") {
            files = num;
        } else if part.contains("insertion") {
            ins = num;
        } else if part.contains("deletion") {
            del = num;
        }
    }

    (files, ins, del)
}
