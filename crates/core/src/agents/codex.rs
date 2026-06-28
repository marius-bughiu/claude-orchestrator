//! Codex CLI adapter (sub-agent).
//!
//! Codex runs non-interactively with `codex exec "<prompt>"`. With `--json` it
//! emits newline-delimited JSON "thread events". The event schema is still
//! experimental, so we parse permissively: agent-message events become assistant
//! text, token-count events feed usage, and a terminal event yields the result.
//! `--full-auto` enables unattended execution.

use super::{AgentAdapter, AgentEvent, Invocation, RunSpec};
use crate::config::PermissionMode;
use crate::models::{AgentKind, TokenUsage};
use serde_json::Value;

pub struct CodexAdapter;

impl AgentAdapter for CodexAdapter {
    fn kind(&self) -> AgentKind {
        AgentKind::Codex
    }

    fn default_binary(&self) -> &'static str {
        "codex"
    }

    fn build_invocation(&self, spec: &RunSpec, binary: &str) -> Invocation {
        let mut args: Vec<String> = vec!["exec".into(), "--json".into()];

        if let Some(model) = &spec.model {
            args.push("--model".into());
            args.push(model.clone());
        }

        // Resume a prior rollout if we recorded its id.
        if let Some(resume) = &spec.resume_session_id {
            args.push("--session".into());
            args.push(resume.clone());
        }

        if spec.permission_mode != PermissionMode::Default {
            args.push("--full-auto".into());
        }

        args.extend(spec.extra_args.iter().cloned());

        // Prompt is the trailing positional for `codex exec`.
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
            return vec![AgentEvent::Raw {
                value: Value::String(line.to_string()),
            }];
        };

        // Codex nests the payload under `msg` in some versions, flat in others.
        let payload = value.get("msg").unwrap_or(&value);
        let ty = payload
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();

        match ty {
            "session_configured" | "thread.started" => vec![AgentEvent::Init {
                agent_session_id: payload
                    .get("session_id")
                    .or_else(|| payload.get("thread_id"))
                    .and_then(Value::as_str)
                    .map(String::from),
                model: payload.get("model").and_then(Value::as_str).map(String::from),
            }],
            "agent_message" | "assistant_message" => vec![AgentEvent::Assistant {
                text: payload
                    .get("message")
                    .or_else(|| payload.get("text"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            }],
            "agent_reasoning" | "reasoning" => vec![AgentEvent::Thinking {
                text: payload
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            }],
            "exec_command_begin" | "tool_call" | "command" => vec![AgentEvent::ToolUse {
                name: payload
                    .get("command")
                    .or_else(|| payload.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or("command")
                    .to_string(),
                input: payload.clone(),
            }],
            "token_count" | "usage" => vec![AgentEvent::Result {
                success: true,
                result_text: None,
                usage: parse_usage(payload),
            }],
            "task_complete" | "turn.completed" | "thread.completed" => {
                vec![AgentEvent::Result {
                    success: true,
                    result_text: payload
                        .get("last_agent_message")
                        .and_then(Value::as_str)
                        .map(String::from),
                    usage: parse_usage(payload),
                }]
            }
            "error" | "stream_error" => vec![AgentEvent::Error {
                message: payload
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("codex error")
                    .to_string(),
            }],
            _ => vec![AgentEvent::Raw { value }],
        }
    }
}

fn parse_usage(payload: &Value) -> TokenUsage {
    let info = payload.get("usage").or_else(|| payload.get("info"));
    let get = |k: &str| {
        info.and_then(|u| u.get(k))
            .and_then(Value::as_u64)
            .unwrap_or(0)
    };
    TokenUsage {
        input_tokens: get("input_tokens"),
        output_tokens: get("output_tokens"),
        cache_read_tokens: get("cached_input_tokens"),
        cache_creation_tokens: 0,
        total_cost_usd: 0.0,
        num_turns: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_exec_invocation() {
        let mut s = RunSpec::new("hi", "/repo");
        s.permission_mode = PermissionMode::BypassPermissions;
        let inv = CodexAdapter.build_invocation(&s, "codex");
        assert_eq!(inv.args[0], "exec");
        assert!(inv.args.contains(&"--json".to_string()));
        assert!(inv.args.contains(&"--full-auto".to_string()));
        assert_eq!(inv.args.last().unwrap(), "hi");
    }

    #[test]
    fn parses_agent_message_nested_under_msg() {
        let line = r#"{"msg":{"type":"agent_message","message":"hello there"}}"#;
        let evs = CodexAdapter.parse_line(line);
        assert_eq!(evs[0], AgentEvent::Assistant { text: "hello there".into() });
    }

    #[test]
    fn parses_task_complete() {
        let line = r#"{"type":"task_complete","last_agent_message":"all done","usage":{"input_tokens":5,"output_tokens":3}}"#;
        let evs = CodexAdapter.parse_line(line);
        match &evs[0] {
            AgentEvent::Result { result_text, usage, .. } => {
                assert_eq!(result_text.as_deref(), Some("all done"));
                assert_eq!(usage.input_tokens, 5);
            }
            other => panic!("expected result, got {other:?}"),
        }
    }
}
