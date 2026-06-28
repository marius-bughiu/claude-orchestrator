//! Gemini CLI adapter (sub-agent).
//!
//! Gemini CLI runs non-interactively with `gemini -p "<prompt>"`. With
//! `--output-format json` it prints a single terminal JSON object of the shape
//! `{"response":"…","stats":{…}}`; without it, it prints plain text. We support
//! both: JSON lines are parsed for a final result + token stats, anything else is
//! treated as assistant text. Auto-approval uses `--yolo`.
//!
//! Gemini's streaming JSON surface is still evolving, so this adapter is
//! deliberately permissive and best-effort.

use super::{AgentAdapter, AgentEvent, Invocation, RunSpec};
use crate::config::PermissionMode;
use crate::models::{AgentKind, TokenUsage};
use serde_json::Value;

pub struct GeminiAdapter;

impl AgentAdapter for GeminiAdapter {
    fn kind(&self) -> AgentKind {
        AgentKind::Gemini
    }

    fn default_binary(&self) -> &'static str {
        "gemini"
    }

    fn build_invocation(&self, spec: &RunSpec, binary: &str) -> Invocation {
        let mut args: Vec<String> = Vec::new();
        args.push("--output-format".into());
        args.push("json".into());

        if let Some(model) = &spec.model {
            args.push("--model".into());
            args.push(model.clone());
        }

        // Gemini has no fine-grained permission modes; --yolo auto-approves.
        if spec.permission_mode != PermissionMode::Default {
            args.push("--yolo".into());
        }

        args.extend(spec.extra_args.iter().cloned());

        args.push("--prompt".into());
        args.push(spec.prompt.clone());

        Invocation {
            program: binary.to_string(),
            args,
            stdin: None,
        }
    }

    fn parse_line(&self, line: &str) -> Vec<AgentEvent> {
        let line = line.trim();
        if line.is_empty() {
            return Vec::new();
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            // Plain-text output mode: treat the line as assistant text.
            return vec![AgentEvent::Assistant {
                text: line.to_string(),
            }];
        };

        // Terminal JSON object: {"response": "...", "stats": {...}}
        if let Some(response) = value.get("response").and_then(Value::as_str) {
            return vec![AgentEvent::Result {
                success: value.get("error").is_none(),
                result_text: Some(response.to_string()),
                usage: parse_stats(value.get("stats")),
            }];
        }

        if let Some(err) = value.get("error") {
            return vec![AgentEvent::Error {
                message: err
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or(&err.to_string())
                    .to_string(),
            }];
        }

        vec![AgentEvent::Raw { value }]
    }
}

/// Sum token counts out of Gemini's `stats.models.<model>.tokens` tree.
fn parse_stats(stats: Option<&Value>) -> TokenUsage {
    let mut usage = TokenUsage::default();
    let Some(models) = stats.and_then(|s| s.get("models")).and_then(Value::as_object) else {
        return usage;
    };
    for model in models.values() {
        let tokens = model.get("tokens");
        let get = |k: &str| tokens.and_then(|t| t.get(k)).and_then(Value::as_u64).unwrap_or(0);
        usage.input_tokens += get("prompt");
        usage.output_tokens += get("candidates");
        usage.cache_read_tokens += get("cached");
    }
    usage
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_invocation_with_yolo() {
        let mut s = RunSpec::new("hi", "/repo");
        s.permission_mode = PermissionMode::BypassPermissions;
        let inv = GeminiAdapter.build_invocation(&s, "gemini");
        assert_eq!(inv.program, "gemini");
        assert!(inv.args.contains(&"--yolo".to_string()));
        assert_eq!(inv.args.last().unwrap(), "hi");
    }

    #[test]
    fn parses_response_and_stats() {
        let line = r#"{"response":"answer","stats":{"models":{"gemini-2.5-pro":{"tokens":{"prompt":12,"candidates":8,"cached":2}}}}}"#;
        let evs = GeminiAdapter.parse_line(line);
        match &evs[0] {
            AgentEvent::Result { result_text, usage, .. } => {
                assert_eq!(result_text.as_deref(), Some("answer"));
                assert_eq!(usage.input_tokens, 12);
                assert_eq!(usage.output_tokens, 8);
                assert_eq!(usage.cache_read_tokens, 2);
            }
            other => panic!("expected result, got {other:?}"),
        }
    }

    #[test]
    fn plain_text_is_assistant() {
        let evs = GeminiAdapter.parse_line("just some text");
        assert_eq!(evs[0], AgentEvent::Assistant { text: "just some text".into() });
    }
}
