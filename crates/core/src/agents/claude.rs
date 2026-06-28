//! Claude Code CLI adapter.
//!
//! Invocation shape:
//! `claude -p --output-format stream-json --verbose [--model M] [perm flags] <prompt>`
//! For follow-ups we resume the agent's own session: `claude --resume <id> ...`.

use super::{AgentAdapter, AgentEvent, Invocation, RunSpec};
use crate::config::PermissionMode;
use crate::models::{AgentKind, TokenUsage};
use serde_json::Value;

pub struct ClaudeAdapter;

impl AgentAdapter for ClaudeAdapter {
    fn kind(&self) -> AgentKind {
        AgentKind::Claude
    }

    fn default_binary(&self) -> &'static str {
        "claude"
    }

    fn build_invocation(&self, spec: &RunSpec, binary: &str) -> Invocation {
        let mut args: Vec<String> = Vec::new();

        // Resume an existing conversation, or pin a fresh session id so we can
        // resume it later. These are mutually exclusive.
        if let Some(resume) = &spec.resume_session_id {
            args.push("--resume".into());
            args.push(resume.clone());
        } else if let Some(sid) = &spec.session_id {
            args.push("--session-id".into());
            args.push(sid.clone());
        }

        args.push("-p".into());
        args.push("--output-format".into());
        args.push("stream-json".into());
        // stream-json + --print requires --verbose to emit per-event lines.
        args.push("--verbose".into());

        if let Some(model) = &spec.model {
            args.push("--model".into());
            args.push(model.clone());
        }

        match spec.permission_mode {
            PermissionMode::BypassPermissions => {
                args.push("--dangerously-skip-permissions".into());
            }
            PermissionMode::Default => {
                args.push("--permission-mode".into());
                args.push("default".into());
            }
            PermissionMode::AcceptEdits => {
                args.push("--permission-mode".into());
                args.push("acceptEdits".into());
            }
            PermissionMode::Plan => {
                args.push("--permission-mode".into());
                args.push("plan".into());
            }
        }

        if let Some(sys) = &spec.system_prompt_append {
            args.push("--append-system-prompt".into());
            args.push(sys.clone());
        }

        for dir in &spec.add_dirs {
            args.push("--add-dir".into());
            args.push(dir.to_string_lossy().into_owned());
        }

        if let Some(mcp) = &spec.mcp_config {
            args.push("--mcp-config".into());
            args.push(mcp.to_string_lossy().into_owned());
        }

        args.extend(spec.extra_args.iter().cloned());

        // Prompt is the trailing positional argument.
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
        let value: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            // Non-JSON line (e.g. a stray log) — surface it as raw text.
            Err(_) => {
                return vec![AgentEvent::Raw {
                    value: Value::String(line.to_string()),
                }]
            }
        };

        let ty = value.get("type").and_then(Value::as_str).unwrap_or("");
        match ty {
            "system" => {
                let subtype = value.get("subtype").and_then(Value::as_str).unwrap_or("");
                if subtype == "init" {
                    vec![AgentEvent::Init {
                        agent_session_id: value
                            .get("session_id")
                            .and_then(Value::as_str)
                            .map(String::from),
                        model: value.get("model").and_then(Value::as_str).map(String::from),
                    }]
                } else {
                    vec![AgentEvent::Raw { value }]
                }
            }
            "assistant" => parse_message_content(&value, false),
            "user" => parse_message_content(&value, true),
            "result" => {
                let subtype = value.get("subtype").and_then(Value::as_str).unwrap_or("");
                let is_error = value
                    .get("is_error")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let success = subtype == "success" && !is_error;
                vec![AgentEvent::Result {
                    success,
                    result_text: value.get("result").and_then(Value::as_str).map(String::from),
                    usage: parse_usage(&value),
                }]
            }
            // Partial streaming deltas (only with --include-partial-messages).
            "stream_event" => Vec::new(),
            _ => vec![AgentEvent::Raw { value }],
        }
    }
}

/// Parse an assistant/user message envelope into content events.
fn parse_message_content(value: &Value, is_user: bool) -> Vec<AgentEvent> {
    let content = value
        .get("message")
        .and_then(|m| m.get("content"));
    let Some(content) = content else {
        return vec![AgentEvent::Raw {
            value: value.clone(),
        }];
    };

    // Content can be a plain string or an array of blocks.
    if let Some(text) = content.as_str() {
        if is_user {
            return vec![AgentEvent::ToolResult {
                content: text.to_string(),
                is_error: false,
            }];
        }
        return vec![AgentEvent::Assistant {
            text: text.to_string(),
        }];
    }

    let Some(blocks) = content.as_array() else {
        return vec![AgentEvent::Raw {
            value: value.clone(),
        }];
    };

    let mut out = Vec::new();
    for block in blocks {
        let bty = block.get("type").and_then(Value::as_str).unwrap_or("");
        match bty {
            "text" => {
                if let Some(t) = block.get("text").and_then(Value::as_str) {
                    if !t.is_empty() {
                        out.push(AgentEvent::Assistant {
                            text: t.to_string(),
                        });
                    }
                }
            }
            "thinking" => {
                if let Some(t) = block.get("thinking").and_then(Value::as_str) {
                    out.push(AgentEvent::Thinking {
                        text: t.to_string(),
                    });
                }
            }
            "tool_use" => {
                out.push(AgentEvent::ToolUse {
                    name: block
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("tool")
                        .to_string(),
                    input: block.get("input").cloned().unwrap_or(Value::Null),
                });
            }
            "tool_result" => {
                let is_error = block
                    .get("is_error")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                out.push(AgentEvent::ToolResult {
                    content: stringify_tool_content(block.get("content")),
                    is_error,
                });
            }
            _ => out.push(AgentEvent::Raw {
                value: block.clone(),
            }),
        }
    }
    out
}

/// Tool result content can be a string or an array of {type:text,text} blocks.
fn stringify_tool_content(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|b| b.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

fn parse_usage(value: &Value) -> TokenUsage {
    let usage = value.get("usage");
    let get = |k: &str| -> u64 {
        usage
            .and_then(|u| u.get(k))
            .and_then(Value::as_u64)
            .unwrap_or(0)
    };
    TokenUsage {
        input_tokens: get("input_tokens"),
        output_tokens: get("output_tokens"),
        cache_read_tokens: get("cache_read_input_tokens"),
        cache_creation_tokens: get("cache_creation_input_tokens"),
        total_cost_usd: value
            .get("total_cost_usd")
            .and_then(Value::as_f64)
            .unwrap_or(0.0),
        num_turns: value
            .get("num_turns")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> RunSpec {
        RunSpec::new("do the thing", "/repo")
    }

    #[test]
    fn builds_basic_invocation_with_bypass() {
        let inv = ClaudeAdapter.build_invocation(&spec(), "claude");
        assert_eq!(inv.program, "claude");
        assert!(inv.args.contains(&"-p".to_string()));
        assert!(inv.args.contains(&"stream-json".to_string()));
        assert!(inv.args.contains(&"--verbose".to_string()));
        assert!(inv.args.contains(&"--dangerously-skip-permissions".to_string()));
        // Prompt is the last positional.
        assert_eq!(inv.args.last().unwrap(), "do the thing");
    }

    #[test]
    fn resume_takes_precedence_over_session_id() {
        let mut s = spec();
        s.resume_session_id = Some("abc".into());
        s.session_id = Some("xyz".into());
        let inv = ClaudeAdapter.build_invocation(&s, "claude");
        assert!(inv.args.windows(2).any(|w| w == ["--resume", "abc"]));
        assert!(!inv.args.iter().any(|a| a == "--session-id"));
    }

    #[test]
    fn parses_init_event() {
        let line = r#"{"type":"system","subtype":"init","session_id":"s-1","model":"claude-x"}"#;
        let evs = ClaudeAdapter.parse_line(line);
        assert_eq!(
            evs,
            vec![AgentEvent::Init {
                agent_session_id: Some("s-1".into()),
                model: Some("claude-x".into())
            }]
        );
    }

    #[test]
    fn parses_assistant_text_and_tool_use() {
        let line = r#"{"type":"assistant","message":{"content":[
            {"type":"text","text":"hello"},
            {"type":"tool_use","name":"Bash","input":{"command":"ls"}}
        ]}}"#;
        let evs = ClaudeAdapter.parse_line(line);
        assert_eq!(evs.len(), 2);
        assert_eq!(evs[0], AgentEvent::Assistant { text: "hello".into() });
        match &evs[1] {
            AgentEvent::ToolUse { name, input } => {
                assert_eq!(name, "Bash");
                assert_eq!(input["command"], "ls");
            }
            other => panic!("expected tool_use, got {other:?}"),
        }
    }

    #[test]
    fn parses_result_with_usage() {
        let line = r#"{"type":"result","subtype":"success","is_error":false,
            "result":"done","num_turns":4,"total_cost_usd":0.0123,
            "usage":{"input_tokens":100,"output_tokens":50,
                     "cache_read_input_tokens":10,"cache_creation_input_tokens":5}}"#;
        let evs = ClaudeAdapter.parse_line(line);
        match &evs[0] {
            AgentEvent::Result { success, result_text, usage } => {
                assert!(success);
                assert_eq!(result_text.as_deref(), Some("done"));
                assert_eq!(usage.input_tokens, 100);
                assert_eq!(usage.output_tokens, 50);
                assert_eq!(usage.cache_read_tokens, 10);
                assert_eq!(usage.cache_creation_tokens, 5);
                assert!((usage.total_cost_usd - 0.0123).abs() < 1e-9);
                assert_eq!(usage.num_turns, 4);
            }
            other => panic!("expected result, got {other:?}"),
        }
    }

    #[test]
    fn error_result_is_not_success() {
        let line = r#"{"type":"result","subtype":"error_max_turns","is_error":true}"#;
        let evs = ClaudeAdapter.parse_line(line);
        assert!(matches!(evs[0], AgentEvent::Result { success: false, .. }));
    }

    #[test]
    fn non_json_line_becomes_raw() {
        let evs = ClaudeAdapter.parse_line("not json at all");
        assert!(matches!(evs[0], AgentEvent::Raw { .. }));
    }
}
