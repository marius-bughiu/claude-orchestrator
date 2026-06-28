//! Scheduled tasks: markdown files in a project's `.orchestrator/scheduled/`
//! directory whose front matter describes when to run them. This module is pure
//! (parsing + next-run math); the engine handles persistence and firing.
//!
//! Example file (`.orchestrator/scheduled/daily-deps.md`):
//!
//! ```markdown
//! ---
//! schedule: "0 9 * * *"      # 5-field cron (seconds optional), or `every: 6h`
//! agent: claude
//! model: opus
//! priority: high
//! enabled: true
//! title: Daily dependency check
//! ---
//! Check for outdated dependencies and open a task to update them...
//! ```

use crate::models::{AgentKind, ScheduledTask};
use chrono::{DateTime, Duration, Utc};
use std::collections::BTreeMap;
use std::path::Path;
use std::str::FromStr;

pub const SCHEDULED_DIR: &str = ".orchestrator/scheduled";

/// Split a markdown document into (front matter map, body). If there is no
/// `---`-delimited front matter, the map is empty and the body is the whole text.
pub fn split_front_matter(content: &str) -> (BTreeMap<String, String>, String) {
    let trimmed = content.trim_start_matches('\u{feff}');
    let bytes = trimmed.trim_start();
    if let Some(rest) = bytes.strip_prefix("---") {
        // Front matter starts after the first line.
        if let Some(nl) = rest.find('\n') {
            let after_open = &rest[nl + 1..];
            // Find the closing fence at the start of a line.
            if let Some(end) = find_closing_fence(after_open) {
                let fm = &after_open[..end.0];
                let body = &after_open[end.1..];
                return (parse_kv(fm), body.trim_start_matches('\n').to_string());
            }
        }
    }
    (BTreeMap::new(), content.to_string())
}

/// Returns (start_of_fence, end_after_fence_line) for a line that is exactly `---`.
fn find_closing_fence(s: &str) -> Option<(usize, usize)> {
    let mut idx = 0;
    for line in s.split_inclusive('\n') {
        let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
        if trimmed == "---" || trimmed == "..." {
            return Some((idx, idx + line.len()));
        }
        idx += line.len();
    }
    None
}

fn parse_kv(fm: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for line in fm.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let mut value = value.trim().to_string();
        // Strip surrounding quotes; otherwise strip a trailing `# comment`.
        if (value.starts_with('"') && value.ends_with('"') && value.len() >= 2)
            || (value.starts_with('\'') && value.ends_with('\'') && value.len() >= 2)
        {
            value = value[1..value.len() - 1].to_string();
        } else if let Some(hash) = value.find(" #") {
            value = value[..hash].trim().to_string();
        }
        if !key.is_empty() {
            map.insert(key, value);
        }
    }
    map
}

fn parse_bool(s: &str, default: bool) -> bool {
    match s.trim().to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => true,
        "false" | "no" | "off" | "0" => false,
        _ => default,
    }
}

fn parse_priority(s: &str) -> i64 {
    match s.trim().to_ascii_lowercase().as_str() {
        "low" => 0,
        "normal" => 50,
        "high" => 100,
        "urgent" => 200,
        other => other.parse().unwrap_or(50),
    }
}

/// Parse a duration like `30s`, `15m`, `6h`, `1d`, `2w` into a chrono Duration.
pub fn parse_duration(s: &str) -> Option<Duration> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (num, unit) = s.split_at(s.find(|c: char| c.is_alphabetic()).unwrap_or(s.len()));
    let n: i64 = num.trim().parse().ok()?;
    if n <= 0 {
        return None;
    }
    match unit.trim().to_ascii_lowercase().as_str() {
        "s" | "sec" | "secs" | "second" | "seconds" => Some(Duration::seconds(n)),
        "m" | "min" | "mins" | "minute" | "minutes" => Some(Duration::minutes(n)),
        "h" | "hr" | "hrs" | "hour" | "hours" => Some(Duration::hours(n)),
        "d" | "day" | "days" => Some(Duration::days(n)),
        "w" | "week" | "weeks" => Some(Duration::weeks(n)),
        _ => None,
    }
}

/// Normalize a cron expression to the 6-field form the `cron` crate wants. A
/// standard 5-field expression (`min hour dom month dow`) gets a `0` seconds
/// field prepended.
pub fn normalize_cron(expr: &str) -> String {
    let fields: Vec<&str> = expr.split_whitespace().collect();
    if fields.len() == 5 {
        format!("0 {expr}")
    } else {
        expr.to_string()
    }
}

/// Compute the next run after `after` for a given schedule.
pub fn next_run_after(
    schedule_kind: &str,
    schedule: &str,
    after: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    match schedule_kind {
        "interval" => parse_duration(schedule).map(|d| after + d),
        "cron" => {
            let normalized = normalize_cron(schedule);
            let sched = cron::Schedule::from_str(&normalized).ok()?;
            sched.after(&after).next()
        }
        _ => None,
    }
}

/// Build a `ScheduledTask` from a markdown file's contents. Always returns a
/// task; on parse failure `valid` is false and `error` is set.
pub fn parse_scheduled(
    project_id: &str,
    abs_path: &Path,
    rel_path: &str,
    content: &str,
    now: DateTime<Utc>,
) -> ScheduledTask {
    let (fm, body) = split_front_matter(content);
    let id = stable_id(project_id, rel_path);
    let stem = abs_path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "scheduled".into());

    let title = fm.get("title").cloned().unwrap_or(stem);
    let agent = fm.get("agent").and_then(|a| AgentKind::from_str(a));
    let model = fm.get("model").cloned().filter(|m| !m.is_empty());
    let priority = fm.get("priority").map(|p| parse_priority(p)).unwrap_or(50);
    let enabled = fm
        .get("enabled")
        .map(|e| parse_bool(e, true))
        .unwrap_or(true);

    let mut base = ScheduledTask {
        id,
        project_id: project_id.to_string(),
        path: abs_path.to_string_lossy().into_owned(),
        rel_path: rel_path.to_string(),
        title,
        schedule: String::new(),
        schedule_kind: String::new(),
        schedule_desc: String::new(),
        agent,
        model,
        priority,
        enabled,
        valid: false,
        error: None,
        body: body.trim().to_string(),
        last_run: None,
        next_run: None,
        created_at: now,
        updated_at: now,
    };

    // Determine the schedule: cron (`schedule`/`cron`) or interval (`every`/`interval`).
    let cron_expr = fm.get("schedule").or_else(|| fm.get("cron"));
    let interval_expr = fm.get("every").or_else(|| fm.get("interval"));

    if let Some(expr) = cron_expr.filter(|e| !e.is_empty()) {
        let normalized = normalize_cron(expr);
        if cron::Schedule::from_str(&normalized).is_ok() {
            base.schedule = expr.clone();
            base.schedule_kind = "cron".into();
            base.schedule_desc = format!("cron: {expr}");
            base.valid = true;
        } else {
            base.error = Some(format!("invalid cron expression: {expr}"));
        }
    } else if let Some(expr) = interval_expr.filter(|e| !e.is_empty()) {
        if parse_duration(expr).is_some() {
            base.schedule = expr.clone();
            base.schedule_kind = "interval".into();
            base.schedule_desc = format!("every {expr}");
            base.valid = true;
        } else {
            base.error = Some(format!("invalid interval: {expr}"));
        }
    } else {
        base.error = Some("missing `schedule` (cron) or `every` (interval) in front matter".into());
    }

    if base.body.is_empty() && base.valid {
        base.error = Some("scheduled task has no body / instructions".into());
        base.valid = false;
    }

    if base.valid {
        base.next_run = next_run_after(&base.schedule_kind, &base.schedule, now);
    }
    base
}

/// Deterministic id (FNV-1a) so the same file maps to the same row across runs.
pub fn stable_id(project_id: &str, rel_path: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in project_id
        .bytes()
        .chain(b"\0".iter().copied())
        .chain(rel_path.bytes())
    {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("sched_{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_front_matter_and_body() {
        let doc = "---\nschedule: \"0 9 * * *\"\nagent: codex\n---\nDo the thing.\n";
        let (fm, body) = split_front_matter(doc);
        assert_eq!(fm.get("schedule").unwrap(), "0 9 * * *");
        assert_eq!(fm.get("agent").unwrap(), "codex");
        assert_eq!(body.trim(), "Do the thing.");
    }

    #[test]
    fn no_front_matter_is_all_body() {
        let (fm, body) = split_front_matter("just a body");
        assert!(fm.is_empty());
        assert_eq!(body, "just a body");
    }

    #[test]
    fn parses_duration_units() {
        assert_eq!(parse_duration("30m"), Some(Duration::minutes(30)));
        assert_eq!(parse_duration("6h"), Some(Duration::hours(6)));
        assert_eq!(parse_duration("2w"), Some(Duration::weeks(2)));
        assert_eq!(parse_duration("bad"), None);
    }

    #[test]
    fn normalizes_five_field_cron() {
        assert_eq!(normalize_cron("0 9 * * *"), "0 0 9 * * *");
        assert_eq!(normalize_cron("0 0 9 * * *"), "0 0 9 * * *");
    }

    #[test]
    fn cron_and_interval_next_run() {
        let now = "2026-06-28T08:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let cron_next = next_run_after("cron", "0 9 * * *", now).unwrap();
        assert_eq!(cron_next.to_rfc3339(), "2026-06-28T09:00:00+00:00");
        let interval_next = next_run_after("interval", "30m", now).unwrap();
        assert_eq!(interval_next, now + Duration::minutes(30));
    }

    #[test]
    fn parse_scheduled_valid_cron() {
        let now = Utc::now();
        let doc = "---\nschedule: 0 9 * * *\nagent: gemini\nmodel: gemini-2.5-pro\npriority: high\n---\nReview the dependency tree.";
        let st = parse_scheduled(
            "p1",
            Path::new("/r/.orchestrator/scheduled/dep.md"),
            ".orchestrator/scheduled/dep.md",
            doc,
            now,
        );
        assert!(st.valid);
        assert_eq!(st.schedule_kind, "cron");
        assert_eq!(st.agent, Some(AgentKind::Gemini));
        assert_eq!(st.model.as_deref(), Some("gemini-2.5-pro"));
        assert_eq!(st.priority, 100);
        assert!(st.next_run.is_some());
        assert_eq!(st.title, "dep");
    }

    #[test]
    fn parse_scheduled_invalid_without_schedule() {
        let now = Utc::now();
        let st = parse_scheduled(
            "p1",
            Path::new("/r/x.md"),
            "x.md",
            "no front matter here",
            now,
        );
        assert!(!st.valid);
        assert!(st.error.is_some());
    }

    #[test]
    fn stable_id_is_deterministic() {
        assert_eq!(stable_id("p", "a/b.md"), stable_id("p", "a/b.md"));
        assert_ne!(stable_id("p", "a.md"), stable_id("p", "b.md"));
    }
}
