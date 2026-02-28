use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};
use std::time::Instant;

// ── Stats Cache (from ~/.claude/stats-cache.json) ──

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StatsCache {
    pub version: Option<u32>,
    pub last_computed_date: Option<String>,
    pub daily_activity: Vec<DailyActivity>,
    pub daily_model_tokens: Vec<DailyModelTokens>,
    pub model_usage: HashMap<String, ModelUsage>,
    pub total_sessions: u64,
    pub total_messages: u64,
    pub longest_session: Option<LongestSession>,
    pub first_session_date: Option<String>,
    pub hour_counts: Option<HashMap<String, u64>>,
    pub total_speculation_time_saved_ms: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DailyActivity {
    pub date: String,
    pub message_count: u64,
    pub session_count: u64,
    pub tool_call_count: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DailyModelTokens {
    pub date: String,
    pub tokens_by_model: HashMap<String, u64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ModelUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_input_tokens: Option<u64>,
    pub cache_creation_input_tokens: Option<u64>,
    pub web_search_requests: Option<u64>,
    pub cost_usd: Option<f64>,
    pub context_window: Option<u64>,
    pub max_output_tokens: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LongestSession {
    pub session_id: String,
    pub duration: u64,
    pub message_count: u64,
    pub timestamp: String,
}

// ── Session Info ──

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub session_id: String,
    pub project: String,
    pub message_count: u64,
    pub last_active: String,
}

// ── Project Usage ──

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProjectUsage {
    pub project: String,
    pub session_count: u64,
    pub total_messages: u64,
}

fn claude_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".claude"))
}

#[tauri::command]
pub fn get_stats_cache() -> Result<StatsCache, String> {
    let path = claude_dir()
        .ok_or("Cannot find home directory")?
        .join("stats-cache.json");

    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Cannot read stats-cache.json: {}", e))?;

    serde_json::from_str(&content)
        .map_err(|e| format!("Cannot parse stats-cache.json: {}", e))
}

#[tauri::command]
pub fn get_active_sessions() -> Result<Vec<SessionInfo>, String> {
    let claude_dir = claude_dir().ok_or("Cannot find home directory")?;
    let projects_dir = claude_dir.join("projects");

    if !projects_dir.exists() {
        return Ok(vec![]);
    }

    let mut sessions: Vec<SessionInfo> = Vec::new();

    let pattern = projects_dir
        .join("*/*.jsonl")
        .to_string_lossy()
        .to_string();

    let paths: Vec<PathBuf> = glob::glob(&pattern)
        .map_err(|e| format!("Glob error: {}", e))?
        .filter_map(|p| p.ok())
        .collect();

    for path in paths {
        // Only include sessions modified in the last 48 hours
        if let Ok(modified_time) = fs::metadata(&path).and_then(|m| m.modified()) {
            let elapsed = modified_time.elapsed().unwrap_or_default();
            if elapsed.as_secs() > 172800 {
                continue;
            }

            let modified_str = {
                let dt: chrono::DateTime<chrono::Utc> = modified_time.into();
                dt.to_rfc3339()
            };

            let project = path
                .parent()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            let session_id = path
                .file_stem()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            // Count lines efficiently without reading entire file
            let content = fs::read_to_string(&path).unwrap_or_default();
            let message_count = content.lines().count() as u64;

            sessions.push(SessionInfo {
                session_id,
                project,
                message_count,
                last_active: modified_str,
            });
        }
    }

    sessions.sort_by(|a, b| b.last_active.cmp(&a.last_active));
    sessions.truncate(20);

    Ok(sessions)
}

#[tauri::command]
pub fn get_project_usage() -> Result<Vec<ProjectUsage>, String> {
    let claude_dir = claude_dir().ok_or("Cannot find home directory")?;
    let projects_dir = claude_dir.join("projects");

    if !projects_dir.exists() {
        return Ok(vec![]);
    }

    let pattern = projects_dir
        .join("*/*.jsonl")
        .to_string_lossy()
        .to_string();

    let mut project_map: HashMap<String, (u64, u64)> = HashMap::new();

    let paths: Vec<PathBuf> = glob::glob(&pattern)
        .map_err(|e| format!("Glob error: {}", e))?
        .filter_map(|p| p.ok())
        .collect();

    for path in paths {
        let project = path
            .parent()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let content = fs::read_to_string(&path).unwrap_or_default();
        let msgs = content.lines().count() as u64;

        let entry = project_map.entry(project).or_insert((0, 0));
        entry.0 += 1; // session count
        entry.1 += msgs; // message count
    }

    let mut usages: Vec<ProjectUsage> = project_map
        .into_iter()
        .map(|(project, (session_count, total_messages))| ProjectUsage {
            project,
            session_count,
            total_messages,
        })
        .collect();

    usages.sort_by(|a, b| b.total_messages.cmp(&a.total_messages));
    usages.truncate(10);

    Ok(usages)
}

// ── Realtime Stats from JSONL parsing ──

#[derive(Debug, Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_creation: u64,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RealtimeStats {
    pub last_activity: Option<String>,
    pub today_messages: u64,
    pub today_tokens: TokenUsage,
    pub week_messages: u64,
    pub week_tokens: TokenUsage,
    pub active_sessions: u64,
    pub plan_type: String,
    pub rate_limit_tier: String,
    pub today_model_tokens: HashMap<String, u64>,
    pub week_model_tokens: HashMap<String, u64>,
}

#[tauri::command]
pub fn get_realtime_stats() -> Result<RealtimeStats, String> {
    let claude_dir = claude_dir().ok_or("Cannot find home directory")?;
    let projects_dir = claude_dir.join("projects");

    // Read credentials for plan info
    let creds_path = claude_dir.join(".credentials.json");
    let (plan_type, rate_limit_tier) = read_credentials(&creds_path);

    if !projects_dir.exists() {
        return Ok(RealtimeStats {
            last_activity: None,
            today_messages: 0,
            today_tokens: TokenUsage::default(),
            week_messages: 0,
            week_tokens: TokenUsage::default(),
            active_sessions: 0,
            plan_type,
            rate_limit_tier,
            today_model_tokens: HashMap::new(),
            week_model_tokens: HashMap::new(),
        });
    }

    let pattern = projects_dir
        .join("*/*.jsonl")
        .to_string_lossy()
        .to_string();

    let paths: Vec<PathBuf> = glob::glob(&pattern)
        .map_err(|e| format!("Glob error: {}", e))?
        .filter_map(|p| p.ok())
        .collect();

    let now = chrono::Utc::now();
    let local_now = chrono::Local::now();
    let today_str = local_now.format("%Y-%m-%d").to_string();
    let week_ago = now - chrono::Duration::days(7);
    let five_hours_ago = now - chrono::Duration::hours(5);

    let mut last_activity: Option<chrono::DateTime<chrono::Utc>> = None;
    let mut today_messages: u64 = 0;
    let mut today_tokens = TokenUsage::default();
    let mut week_messages: u64 = 0;
    let mut week_tokens = TokenUsage::default();
    let mut active_sessions: u64 = 0;
    let mut today_model_tokens: HashMap<String, u64> = HashMap::new();
    let mut week_model_tokens: HashMap<String, u64> = HashMap::new();

    for path in &paths {
        // Only process files modified in the last 48h (not 7 days) for speed
        let modified = match fs::metadata(path).and_then(|m| m.modified()) {
            Ok(t) => t,
            Err(_) => continue,
        };
        let elapsed_secs = modified.elapsed().unwrap_or_default().as_secs();
        if elapsed_secs > 2 * 86400 {
            continue;
        }

        // Check if this session had recent activity (for active_sessions count)
        let modified_dt: chrono::DateTime<chrono::Utc> = modified.into();
        let session_is_active = modified_dt > five_hours_ago;
        if session_is_active {
            active_sessions += 1;
        }

        // Parse JSONL file
        let file = match fs::File::open(path) {
            Ok(f) => f,
            Err(_) => continue,
        };
        let reader = BufReader::new(file);

        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => continue,
            };
            if line.is_empty() {
                continue;
            }

            // Quick check: only parse lines that look like assistant messages with usage
            if !line.contains("\"type\":\"assistant\"") {
                continue;
            }

            let entry: serde_json::Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(_) => continue,
            };

            if entry.get("type").and_then(|v| v.as_str()) != Some("assistant") {
                continue;
            }

            let timestamp_str = match entry.get("timestamp").and_then(|v| v.as_str()) {
                Some(s) => s,
                None => continue,
            };

            let ts = match timestamp_str.parse::<chrono::DateTime<chrono::Utc>>() {
                Ok(t) => t,
                Err(_) => continue,
            };

            // Update last activity
            if last_activity.is_none() || ts > last_activity.unwrap() {
                last_activity = Some(ts);
            }

            // Check if within this week
            if ts < week_ago {
                continue;
            }

            let local_ts = ts.with_timezone(&chrono::Local);
            let is_today = local_ts.format("%Y-%m-%d").to_string() == today_str;

            // Extract usage from message.usage
            if let Some(usage) = entry
                .get("message")
                .and_then(|m| m.get("usage"))
            {
                let input = usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                let output = usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                let cache_read = usage.get("cache_read_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                let cache_creation = usage.get("cache_creation_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);

                // Extract model name for per-model tracking
                let model = entry.get("message")
                    .and_then(|m| m.get("model"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let total_tokens = input + output + cache_read + cache_creation;

                week_messages += 1;
                week_tokens.input += input;
                week_tokens.output += output;
                week_tokens.cache_read += cache_read;
                week_tokens.cache_creation += cache_creation;
                *week_model_tokens.entry(model.to_string()).or_insert(0) += total_tokens;

                if is_today {
                    today_messages += 1;
                    today_tokens.input += input;
                    today_tokens.output += output;
                    today_tokens.cache_read += cache_read;
                    today_tokens.cache_creation += cache_creation;
                    *today_model_tokens.entry(model.to_string()).or_insert(0) += total_tokens;
                }
            }
        }
    }

    Ok(RealtimeStats {
        last_activity: last_activity.map(|t| t.to_rfc3339()),
        today_messages,
        today_tokens,
        week_messages,
        week_tokens,
        active_sessions,
        plan_type,
        rate_limit_tier,
        today_model_tokens,
        week_model_tokens,
    })
}

// ── Plan Usage from Anthropic unified rate limit headers ──

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UsageClaim {
    pub utilization: f64,        // 0.0 - 1.0
    pub reset: Option<u64>,      // unix timestamp
    pub status: String,          // "allowed", "allowed_warning", "rejected"
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitInfo {
    pub status: String,                              // overall status
    pub representative_claim: Option<String>,        // "five_hour", "seven_day", etc.
    pub five_hour: Option<UsageClaim>,
    pub seven_day: Option<UsageClaim>,
    pub seven_day_sonnet: Option<UsageClaim>,
    pub overage_status: Option<String>,              // "allowed", "rejected", etc.
    pub overage_disabled_reason: Option<String>,
    pub overage_reset: Option<u64>,
    pub fallback_percentage: Option<f64>,
    pub checked_at: String,
}

static RATE_LIMIT_CACHE: LazyLock<Mutex<Option<(Instant, RateLimitInfo)>>> =
    LazyLock::new(|| Mutex::new(None));

pub fn get_access_token() -> Result<String, String> {
    let creds_path = claude_dir()
        .ok_or("Cannot find home directory")?
        .join(".credentials.json");
    let content = fs::read_to_string(&creds_path)
        .map_err(|e| format!("Cannot read credentials: {}", e))?;
    let creds: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Cannot parse credentials: {}", e))?;
    creds
        .get("claudeAiOauth")
        .and_then(|o| o.get("accessToken"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "No access token found".to_string())
}

#[tauri::command]
pub async fn get_rate_limits(force: Option<bool>) -> Result<RateLimitInfo, String> {
    let force = force.unwrap_or(false);

    // Check cache (valid for 60 seconds)
    if !force {
        if let Ok(cache) = RATE_LIMIT_CACHE.lock() {
            if let Some((instant, ref info)) = *cache {
                if instant.elapsed().as_secs() < 60 {
                    return Ok(info.clone());
                }
            }
        }
    }

    let token = get_access_token()?;

    let client = reqwest::Client::new();
    let resp = client
        .post("https://api.anthropic.com/v1/messages")
        .header("Authorization", format!("Bearer {}", token))
        .header("anthropic-version", "2023-06-01")
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("content-type", "application/json")
        .json(&serde_json::json!({
            "model": "claude-sonnet-4-20250514",
            "max_tokens": 1,
            "messages": [{"role": "user", "content": "."}]
        }))
        .send()
        .await
        .map_err(|e| format!("API call failed: {}", e))?;

    let headers = resp.headers().clone();

    let get_str = |name: &str| -> Option<String> {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
    };
    let get_f64 = |name: &str| -> Option<f64> {
        get_str(name).and_then(|s| s.parse().ok())
    };
    let get_u64 = |name: &str| -> Option<u64> {
        get_str(name).and_then(|s| s.parse().ok())
    };

    let parse_claim = |prefix: &str| -> Option<UsageClaim> {
        let utilization = get_f64(&format!("anthropic-ratelimit-unified-{}-utilization", prefix))?;
        Some(UsageClaim {
            utilization,
            reset: get_u64(&format!("anthropic-ratelimit-unified-{}-reset", prefix)),
            status: get_str(&format!("anthropic-ratelimit-unified-{}-status", prefix))
                .unwrap_or_else(|| "unknown".to_string()),
        })
    };

    let info = RateLimitInfo {
        status: get_str("anthropic-ratelimit-unified-status")
            .unwrap_or_else(|| "unknown".to_string()),
        representative_claim: get_str("anthropic-ratelimit-unified-representative-claim"),
        five_hour: parse_claim("5h"),
        seven_day: parse_claim("7d"),
        seven_day_sonnet: parse_claim("7d_sonnet"),
        overage_status: get_str("anthropic-ratelimit-unified-overage-status"),
        overage_disabled_reason: get_str("anthropic-ratelimit-unified-overage-disabled-reason"),
        overage_reset: get_u64("anthropic-ratelimit-unified-overage-reset"),
        fallback_percentage: get_f64("anthropic-ratelimit-unified-fallback-percentage"),
        checked_at: chrono::Utc::now().to_rfc3339(),
    };

    // Update cache
    if let Ok(mut cache) = RATE_LIMIT_CACHE.lock() {
        *cache = Some((Instant::now(), info.clone()));
    }

    Ok(info)
}

/// Read 5h utilization from the in-memory rate limit cache (non-async, for tray thread)
pub fn get_cached_utilization() -> Option<f64> {
    let cache = RATE_LIMIT_CACHE.lock().ok()?;
    let (_, ref info) = (*cache).as_ref()?;
    info.five_hour.as_ref().map(|c| c.utilization)
}

// ── Session Summaries for DevLog ──

use crate::storage::SessionSummary;
use crate::git::decode_project_path;

pub fn get_session_summaries(date: &str) -> Vec<SessionSummary> {
    let claude_dir = match claude_dir() {
        Some(d) => d,
        None => return vec![],
    };
    let projects_dir = claude_dir.join("projects");
    if !projects_dir.exists() {
        return vec![];
    }

    let pattern = projects_dir
        .join("*/*.jsonl")
        .to_string_lossy()
        .to_string();

    let paths: Vec<PathBuf> = match glob::glob(&pattern) {
        Ok(p) => p.filter_map(|p| p.ok()).collect(),
        Err(_) => return vec![],
    };

    let mut summaries = vec![];

    for path in &paths {
        // Only process files modified in the last 7 days
        if let Ok(modified) = std::fs::metadata(path).and_then(|m| m.modified()) {
            let elapsed = modified.elapsed().unwrap_or_default().as_secs();
            if elapsed > 7 * 86400 {
                continue;
            }
        }

        let project_dir_name = path
            .parent()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let session_id = path
            .file_stem()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let project_path = decode_project_path(&project_dir_name);

        let file = match fs::File::open(path) {
            Ok(f) => f,
            Err(_) => continue,
        };
        let reader = BufReader::new(file);

        let mut msg_count: u64 = 0;
        let mut input_tokens: u64 = 0;
        let mut output_tokens: u64 = 0;
        let mut cache_read: u64 = 0;
        let mut first_ts: Option<String> = None;
        let mut last_ts: Option<String> = None;
        let mut has_date_match = false;

        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => continue,
            };
            if line.is_empty() || !line.contains("\"type\":\"assistant\"") {
                continue;
            }

            let entry: serde_json::Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(_) => continue,
            };

            if entry.get("type").and_then(|v| v.as_str()) != Some("assistant") {
                continue;
            }

            let timestamp_str = match entry.get("timestamp").and_then(|v| v.as_str()) {
                Some(s) => s,
                None => continue,
            };

            if !timestamp_str.starts_with(date) {
                continue;
            }

            has_date_match = true;
            msg_count += 1;

            if first_ts.is_none() {
                first_ts = Some(timestamp_str.to_string());
            }
            last_ts = Some(timestamp_str.to_string());

            if let Some(usage) = entry.get("message").and_then(|m| m.get("usage")) {
                input_tokens += usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                output_tokens += usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                cache_read += usage.get("cache_read_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
            }
        }

        if has_date_match {
            // Calculate duration from first to last message
            let duration_minutes = match (&first_ts, &last_ts) {
                (Some(f), Some(l)) => {
                    let first = f.parse::<chrono::DateTime<chrono::Utc>>().ok();
                    let last = l.parse::<chrono::DateTime<chrono::Utc>>().ok();
                    match (first, last) {
                        (Some(f), Some(l)) => ((l - f).num_minutes().max(0)) as u64,
                        _ => 0,
                    }
                }
                _ => 0,
            };

            summaries.push(SessionSummary {
                session_id,
                project: project_dir_name,
                project_path,
                message_count: msg_count,
                input_tokens,
                output_tokens,
                cache_read,
                duration_minutes,
                first_message: first_ts,
                last_message: last_ts,
            });
        }
    }

    summaries
}

fn read_credentials(path: &PathBuf) -> (String, String) {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return ("unknown".to_string(), "unknown".to_string()),
    };
    let json: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return ("unknown".to_string(), "unknown".to_string()),
    };
    let oauth = json.get("claudeAiOauth");
    let plan = oauth
        .and_then(|o| o.get("subscriptionType"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    let tier = oauth
        .and_then(|o| o.get("rateLimitTier"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    (plan, tier)
}
