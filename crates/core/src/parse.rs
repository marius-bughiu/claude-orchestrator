//! Extract the structured JSON contracts emitted by roadmap and verify sessions
//! from otherwise free-form agent output.

use crate::models::AgentKind;
use serde::Deserialize;
use serde_json::Value;

/// A task proposed by the roadmap loop.
#[derive(Debug, Clone, Deserialize)]
pub struct RoadmapTaskSpec {
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub priority: Option<i64>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

impl RoadmapTaskSpec {
    pub fn agent_kind(&self, default: AgentKind) -> AgentKind {
        self.agent
            .as_deref()
            .and_then(AgentKind::from_str)
            .unwrap_or(default)
    }
    pub fn priority_or_default(&self) -> i64 {
        self.priority.unwrap_or(50)
    }
}

/// The verifier's verdict for a finished task.
#[derive(Debug, Clone, Deserialize)]
pub struct VerifyVerdict {
    pub complete: bool,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub follow_up: String,
}

/// Find the last fenced ```json block, falling back to the last balanced JSON
/// array or object embedded in the text.
pub fn extract_last_json(text: &str) -> Option<Value> {
    if let Some(block) = last_fenced_json(text) {
        if let Ok(v) = serde_json::from_str::<Value>(block.trim()) {
            return Some(v);
        }
    }
    last_balanced_json(text)
}

fn last_fenced_json(text: &str) -> Option<String> {
    // Scan for ```json ... ``` (or ``` ... ```) and keep the last one.
    let mut result = None;
    let mut search_from = 0;
    while let Some(rel) = text[search_from..].find("```") {
        let open = search_from + rel;
        // Move past the opening fence and an optional language tag line.
        let after_fence = open + 3;
        let line_end = text[after_fence..]
            .find('\n')
            .map(|i| after_fence + i + 1)
            .unwrap_or(text.len());
        let Some(close_rel) = text[line_end..].find("```") else {
            break;
        };
        let close = line_end + close_rel;
        let body = &text[line_end..close];
        let lang = text[after_fence..line_end].trim();
        if lang.is_empty() || lang.eq_ignore_ascii_case("json") {
            result = Some(body.to_string());
        }
        search_from = close + 3;
    }
    result
}

/// Find the last top-level balanced `{...}` or `[...]` run that parses as JSON.
fn last_balanced_json(text: &str) -> Option<Value> {
    let bytes = text.as_bytes();
    // Try each closing bracket from the end, matching back to its opener.
    for end in (0..bytes.len()).rev() {
        let close = bytes[end];
        let open = match close {
            b'}' => b'{',
            b']' => b'[',
            _ => continue,
        };
        let mut depth = 0i32;
        let mut start = None;
        for i in (0..=end).rev() {
            let c = bytes[i];
            if c == close {
                depth += 1;
            } else if c == open {
                depth -= 1;
                if depth == 0 {
                    start = Some(i);
                    break;
                }
            }
        }
        if let Some(s) = start {
            if let Ok(v) = serde_json::from_str::<Value>(&text[s..=end]) {
                return Some(v);
            }
        }
    }
    None
}

/// Parse the roadmap loop's task batch from a session result.
pub fn parse_roadmap_tasks(text: &str) -> Vec<RoadmapTaskSpec> {
    let Some(value) = extract_last_json(text) else {
        return Vec::new();
    };
    // Accept either a bare array or an object with a `tasks` array.
    let array = match value {
        Value::Array(_) => value,
        Value::Object(ref map) => map.get("tasks").cloned().unwrap_or(Value::Null),
        _ => Value::Null,
    };
    serde_json::from_value::<Vec<RoadmapTaskSpec>>(array)
        .unwrap_or_default()
        .into_iter()
        .filter(|t| !t.title.trim().is_empty())
        .collect()
}

/// Parse the verifier's verdict from a session result.
pub fn parse_verdict(text: &str) -> Option<VerifyVerdict> {
    let value = extract_last_json(text)?;
    serde_json::from_value::<VerifyVerdict>(value).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_roadmap_from_fenced_block() {
        let text = r#"Here is my plan.

```json
[
  {"title": "Add login", "description": "do it", "priority": 100},
  {"title": "Add logout", "description": "do it too", "agent": "codex"}
]
```
"#;
        let tasks = parse_roadmap_tasks(text);
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].title, "Add login");
        assert_eq!(tasks[0].priority_or_default(), 100);
        assert_eq!(tasks[1].agent_kind(AgentKind::Claude), AgentKind::Codex);
    }

    #[test]
    fn empty_array_yields_no_tasks() {
        assert!(parse_roadmap_tasks("nothing to do\n```json\n[]\n```").is_empty());
    }

    #[test]
    fn picks_last_block_when_multiple() {
        let text = "```json\n[{\"title\":\"old\"}]\n```\nthen\n```json\n[{\"title\":\"new\"}]\n```";
        let tasks = parse_roadmap_tasks(text);
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].title, "new");
    }

    #[test]
    fn parses_verdict_complete() {
        let text = r#"Looks good.
```json
{"complete": true, "reason": "all criteria met", "follow_up": ""}
```"#;
        let v = parse_verdict(text).unwrap();
        assert!(v.complete);
        assert_eq!(v.reason, "all criteria met");
    }

    #[test]
    fn parses_verdict_incomplete_without_fence() {
        let text = r#"The tests are missing. {"complete": false, "reason": "no tests", "follow_up": "add unit tests for the parser"}"#;
        let v = parse_verdict(text).unwrap();
        assert!(!v.complete);
        assert_eq!(v.follow_up, "add unit tests for the parser");
    }

    #[test]
    fn ignores_prose_braces() {
        let text = "I considered {this} and {that}. Final:\n```json\n{\"complete\":true,\"reason\":\"ok\"}\n```";
        let v = parse_verdict(text).unwrap();
        assert!(v.complete);
    }
}
