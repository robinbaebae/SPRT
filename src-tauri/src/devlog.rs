use crate::claude;
use crate::git;
use crate::storage::{self, DevLog, DevLogStats, ProjectWork, SessionSummary};

use std::collections::HashMap;

const DEVLOG_SYSTEM_PROMPT: &str = r#"You are a development journal writer for SPRT (Sprint), a developer productivity tool.
Given git commits, Claude Code session data, and code statistics, write a concise daily development log.

Respond ONLY with valid JSON (no markdown fences, no extra text) in this exact format:
{
  "summary": "2-3 sentence overview of what was accomplished",
  "highlights": ["key accomplishment 1", "key accomplishment 2", ...],
  "sprint_score": 75,
  "project_notes": {
    "project-name": ["key change 1", "key change 2"]
  }
}

Guidelines:
- summary: Focus on outcomes, not process. Be specific about features/fixes.
- highlights: 3-5 bullet points of the most important accomplishments.
- sprint_score: 0-100 based on productivity. Consider commits, code volume, session duration.
  - 90-100: Exceptional day (many commits, major features)
  - 70-89: Productive day (steady progress)
  - 50-69: Moderate day (some progress)
  - 30-49: Light day (minor work)
  - 0-29: Minimal activity
- project_notes: Key changes per project (used for project cards).
- Write in English. Keep it factual and concise."#;

const WEEKLY_SYSTEM_PROMPT: &str = r#"You are a development journal writer for SPRT (Sprint).
Given a collection of daily development logs, write a weekly summary report.

Respond ONLY with valid JSON (no markdown fences, no extra text) in this exact format:
{
  "summary": "3-5 sentence weekly overview",
  "highlights": ["weekly highlight 1", "weekly highlight 2", ...],
  "sprint_score": 75,
  "project_notes": {
    "project-name": ["key change 1", "key change 2"]
  }
}

Guidelines:
- Summarize the week's progress at a high level.
- Identify patterns (e.g., "focused heavily on X project").
- sprint_score is the week's average productivity.
- Write in English."#;

#[tauri::command]
pub async fn generate_devlog(date: String, log_type: String) -> Result<DevLog, String> {
    // Check if already exists
    if let Ok(Some(existing)) = storage::get_devlog(&date, &log_type) {
        return Ok(existing);
    }

    match log_type.as_str() {
        "daily" => generate_daily(&date).await,
        "weekly" => generate_weekly(&date).await,
        _ => Err(format!("Unknown log type: {}", log_type)),
    }
}

#[tauri::command]
pub fn get_devlog(date: String, log_type: String) -> Result<Option<DevLog>, String> {
    storage::get_devlog(&date, &log_type)
}

#[tauri::command]
pub fn list_devlogs(log_type: String, limit: Option<usize>) -> Result<Vec<DevLog>, String> {
    storage::list_devlogs(&log_type, limit.unwrap_or(30))
}

#[tauri::command]
pub fn get_git_activity(date: String) -> Result<Vec<git::GitActivity>, String> {
    Ok(git::collect_git_activity(&date))
}

async fn generate_daily(date: &str) -> Result<DevLog, String> {
    // 1. Collect data
    let git_data = git::collect_git_activity(date);
    let session_data = claude::get_session_summaries(date);

    // If no data at all, return an empty-ish log
    if git_data.is_empty() && session_data.is_empty() {
        return Err("No activity found for this date. Nothing to generate.".to_string());
    }

    // 2. Build stats
    let stats = build_stats(&git_data, &session_data);
    let projects_worked = build_project_work(&git_data, &session_data);

    // 3. Build prompt
    let prompt = build_daily_prompt(date, &git_data, &session_data, &stats);

    // 4. Call Claude API
    let ai_response = call_claude_api(DEVLOG_SYSTEM_PROMPT, &prompt).await?;

    // 5. Parse response
    let parsed: serde_json::Value = serde_json::from_str(&ai_response)
        .map_err(|e| format!("Failed to parse AI response: {}. Raw: {}", e, ai_response))?;

    let summary = parsed
        .get("summary")
        .and_then(|v| v.as_str())
        .unwrap_or("No summary generated.")
        .to_string();

    let highlights: Vec<String> = parsed
        .get("highlights")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let sprint_score = parsed
        .get("sprint_score")
        .and_then(|v| v.as_u64())
        .unwrap_or(50) as u32;

    // Merge project_notes from AI into projects_worked
    let project_notes: HashMap<String, Vec<String>> = parsed
        .get("project_notes")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    let projects_worked = projects_worked
        .into_iter()
        .map(|mut pw| {
            if let Some(notes) = project_notes.get(&pw.name) {
                pw.key_changes = notes.clone();
            }
            pw
        })
        .collect();

    let id = format!(
        "{}-{}",
        date,
        chrono::Utc::now().timestamp_millis() % 10000
    );

    let devlog = DevLog {
        id,
        date: date.to_string(),
        log_type: "daily".to_string(),
        generated_at: chrono::Utc::now().to_rfc3339(),
        summary,
        highlights,
        projects_worked,
        stats,
        sprint_score,
    };

    storage::save_devlog(&devlog)?;
    Ok(devlog)
}

async fn generate_weekly(week_start: &str) -> Result<DevLog, String> {
    // Get daily logs for the past 7 days
    let start_date =
        chrono::NaiveDate::parse_from_str(week_start, "%Y-%m-%d").map_err(|e| e.to_string())?;

    let mut daily_logs = vec![];
    let mut all_stats = DevLogStats {
        total_commits: 0,
        total_messages: 0,
        total_tokens: 0,
        total_files_changed: 0,
        total_insertions: 0,
        total_deletions: 0,
        active_hours: 0.0,
        projects_count: 0,
    };

    let mut all_projects: HashMap<String, ProjectWork> = HashMap::new();

    for i in 0..7 {
        let d = start_date + chrono::Duration::days(i);
        let ds = d.format("%Y-%m-%d").to_string();
        if let Ok(Some(log)) = storage::get_devlog(&ds, "daily") {
            // Accumulate stats
            all_stats.total_commits += log.stats.total_commits;
            all_stats.total_messages += log.stats.total_messages;
            all_stats.total_tokens += log.stats.total_tokens;
            all_stats.total_files_changed += log.stats.total_files_changed;
            all_stats.total_insertions += log.stats.total_insertions;
            all_stats.total_deletions += log.stats.total_deletions;
            all_stats.active_hours += log.stats.active_hours;

            for pw in &log.projects_worked {
                let entry = all_projects
                    .entry(pw.name.clone())
                    .or_insert_with(|| ProjectWork {
                        name: pw.name.clone(),
                        path: pw.path.clone(),
                        commits: 0,
                        messages: 0,
                        tokens: 0,
                        duration_minutes: 0,
                        key_changes: vec![],
                    });
                entry.commits += pw.commits;
                entry.messages += pw.messages;
                entry.tokens += pw.tokens;
                entry.duration_minutes += pw.duration_minutes;
            }

            daily_logs.push(log);
        }
    }

    if daily_logs.is_empty() {
        return Err("No daily logs found for this week. Generate daily logs first.".to_string());
    }

    all_stats.projects_count = all_projects.len() as u32;

    // Build weekly prompt from daily summaries
    let prompt = build_weekly_prompt(&daily_logs);
    let ai_response = call_claude_api(WEEKLY_SYSTEM_PROMPT, &prompt).await?;

    let parsed: serde_json::Value = serde_json::from_str(&ai_response)
        .map_err(|e| format!("Failed to parse AI response: {}", e))?;

    let summary = parsed
        .get("summary")
        .and_then(|v| v.as_str())
        .unwrap_or("No summary generated.")
        .to_string();
    let highlights: Vec<String> = parsed
        .get("highlights")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let sprint_score = parsed
        .get("sprint_score")
        .and_then(|v| v.as_u64())
        .unwrap_or(50) as u32;

    let id = format!(
        "w-{}-{}",
        week_start,
        chrono::Utc::now().timestamp_millis() % 10000
    );

    let devlog = DevLog {
        id,
        date: week_start.to_string(),
        log_type: "weekly".to_string(),
        generated_at: chrono::Utc::now().to_rfc3339(),
        summary,
        highlights,
        projects_worked: all_projects.into_values().collect(),
        stats: all_stats,
        sprint_score,
    };

    storage::save_devlog(&devlog)?;
    Ok(devlog)
}

fn build_stats(
    git_data: &[git::GitActivity],
    session_data: &[SessionSummary],
) -> DevLogStats {
    let total_commits: u32 = git_data.iter().map(|g| g.commits.len() as u32).sum();
    let total_messages: u64 = session_data.iter().map(|s| s.message_count).sum();
    let total_tokens: u64 = session_data
        .iter()
        .map(|s| s.input_tokens + s.output_tokens + s.cache_read)
        .sum();
    let total_files: u32 = git_data.iter().map(|g| g.files_changed).sum();
    let total_ins: u32 = git_data.iter().map(|g| g.insertions).sum();
    let total_del: u32 = git_data.iter().map(|g| g.deletions).sum();
    let total_duration_min: u64 = session_data.iter().map(|s| s.duration_minutes).sum();
    let active_hours = total_duration_min as f64 / 60.0;
    let projects_count = {
        let mut names: Vec<&str> = git_data.iter().map(|g| g.repo_name.as_str()).collect();
        names.sort();
        names.dedup();
        names.len() as u32
    };

    DevLogStats {
        total_commits,
        total_messages,
        total_tokens,
        total_files_changed: total_files,
        total_insertions: total_ins,
        total_deletions: total_del,
        active_hours,
        projects_count,
    }
}

fn build_project_work(
    git_data: &[git::GitActivity],
    session_data: &[SessionSummary],
) -> Vec<ProjectWork> {
    let mut projects: HashMap<String, ProjectWork> = HashMap::new();

    for g in git_data {
        let entry = projects
            .entry(g.repo_name.clone())
            .or_insert_with(|| ProjectWork {
                name: g.repo_name.clone(),
                path: g.repo_path.clone(),
                commits: 0,
                messages: 0,
                tokens: 0,
                duration_minutes: 0,
                key_changes: vec![],
            });
        entry.commits += g.commits.len() as u32;
    }

    // Match sessions to projects by path
    for s in session_data {
        // Find matching project by path
        let repo_name = std::path::Path::new(&s.project_path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| s.project.clone());

        let entry = projects
            .entry(repo_name.clone())
            .or_insert_with(|| ProjectWork {
                name: repo_name,
                path: s.project_path.clone(),
                commits: 0,
                messages: 0,
                tokens: 0,
                duration_minutes: 0,
                key_changes: vec![],
            });
        entry.messages += s.message_count;
        entry.tokens += s.input_tokens + s.output_tokens + s.cache_read;
        entry.duration_minutes += s.duration_minutes;
    }

    projects.into_values().collect()
}

fn build_daily_prompt(
    date: &str,
    git_data: &[git::GitActivity],
    session_data: &[SessionSummary],
    stats: &DevLogStats,
) -> String {
    let mut prompt = format!("Generate a daily development log for {}.\n\n", date);

    prompt.push_str(&format!(
        "## Overview Stats\n- Commits: {}\n- Messages: {}\n- Tokens: {}\n- Files changed: {} (+{} -{})\n- Active hours: {:.1}\n- Projects: {}\n\n",
        stats.total_commits, stats.total_messages, stats.total_tokens,
        stats.total_files_changed, stats.total_insertions, stats.total_deletions,
        stats.active_hours, stats.projects_count
    ));

    if !git_data.is_empty() {
        prompt.push_str("## Git Activity\n");
        for g in git_data {
            prompt.push_str(&format!(
                "### {} (branch: {})\n",
                g.repo_name, g.branch
            ));
            for c in &g.commits {
                prompt.push_str(&format!(
                    "- [{}] {} (+{} -{})\n",
                    &c.hash[..7],
                    c.message,
                    c.insertions,
                    c.deletions
                ));
            }
            prompt.push('\n');
        }
    }

    if !session_data.is_empty() {
        prompt.push_str("## Claude Code Sessions\n");
        for s in session_data {
            let repo_name = std::path::Path::new(&s.project_path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| s.project.clone());
            prompt.push_str(&format!(
                "- Project: {}, Messages: {}, Duration: {}min, Tokens: {}\n",
                repo_name,
                s.message_count,
                s.duration_minutes,
                s.input_tokens + s.output_tokens
            ));
        }
    }

    prompt
}

fn build_weekly_prompt(daily_logs: &[DevLog]) -> String {
    let mut prompt = String::from("Generate a weekly summary from these daily logs:\n\n");

    for log in daily_logs {
        prompt.push_str(&format!(
            "## {}\nScore: {}/100\nSummary: {}\nHighlights:\n",
            log.date, log.sprint_score, log.summary
        ));
        for h in &log.highlights {
            prompt.push_str(&format!("- {}\n", h));
        }
        prompt.push_str(&format!(
            "Stats: {} commits, {} messages, {:.1}h active\n\n",
            log.stats.total_commits, log.stats.total_messages, log.stats.active_hours
        ));
    }

    prompt
}

async fn call_claude_api(system: &str, prompt: &str) -> Result<String, String> {
    let token = claude::get_access_token()?;

    let client = reqwest::Client::new();
    let resp = client
        .post("https://api.anthropic.com/v1/messages")
        .header("Authorization", format!("Bearer {}", token))
        .header("anthropic-version", "2023-06-01")
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("content-type", "application/json")
        .json(&serde_json::json!({
            "model": "claude-sonnet-4-20250514",
            "max_tokens": 2048,
            "system": system,
            "messages": [{"role": "user", "content": prompt}]
        }))
        .send()
        .await
        .map_err(|e| format!("API call failed: {}", e))?;

    let status = resp.status();
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to read response: {}", e))?;

    if !status.is_success() {
        let err_msg = body
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .unwrap_or("Unknown API error");
        return Err(format!("API error ({}): {}", status, err_msg));
    }

    // Extract text from first content block
    body.get("content")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|block| block.get("text"))
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "No text in API response".to_string())
}
