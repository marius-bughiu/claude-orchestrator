//! Spawns an agent CLI process, streams its stdout line-by-line through the
//! agent's adapter, and accumulates a final [`RunOutcome`]. Cancellable and
//! bounded by an optional total deadline.

use crate::agents::{AgentAdapter, AgentEvent, RunSpec};
use crate::error::{CoreError, Result};
use crate::models::TokenUsage;
use std::future::pending;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::Notify;

/// A cooperative cancellation token used to stop a running session.
#[derive(Clone, Default)]
pub struct CancelToken {
    inner: Arc<CancelInner>,
}

#[derive(Default)]
struct CancelInner {
    cancelled: AtomicBool,
    notify: Notify,
}

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.inner.cancelled.store(true, Ordering::SeqCst);
        self.inner.notify.notify_waiters();
    }
    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::SeqCst)
    }
    /// Resolves once the token is cancelled.
    pub async fn cancelled(&self) {
        loop {
            if self.is_cancelled() {
                return;
            }
            let notified = self.inner.notify.notified();
            if self.is_cancelled() {
                return;
            }
            notified.await;
        }
    }
}

/// Final result of an agent invocation.
#[derive(Debug, Clone)]
pub struct RunOutcome {
    pub success: bool,
    pub agent_session_id: Option<String>,
    pub model: Option<String>,
    pub result_text: Option<String>,
    pub usage: TokenUsage,
    pub exit_code: Option<i32>,
    pub error: Option<String>,
    pub cancelled: bool,
    pub timed_out: bool,
}

/// Why the read loop stopped.
enum StopReason {
    Eof,
    Cancelled,
    TimedOut,
}

/// Run an agent and stream its events to `on_event`, returning the final outcome.
///
/// `on_event` is invoked synchronously for every normalized event as it arrives.
pub async fn run_agent<F>(
    adapter: &dyn AgentAdapter,
    binary: &str,
    spec: &RunSpec,
    cancel: CancelToken,
    timeout: Option<Duration>,
    mut input: Option<tokio::sync::mpsc::UnboundedReceiver<String>>,
    mut on_event: F,
) -> Result<RunOutcome>
where
    F: FnMut(&AgentEvent),
{
    use tokio::io::AsyncWriteExt;
    let invocation = adapter.build_invocation(spec, binary);
    let needs_stdin = invocation.stdin.is_some() || input.is_some();

    let mut cmd = Command::new(&invocation.program);
    cmd.args(&invocation.args)
        .current_dir(&spec.cwd)
        .stdin(if needs_stdin {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            CoreError::AgentUnavailable(invocation.program.clone())
        } else {
            CoreError::Io(e)
        }
    })?;

    // The child's stdin: kept open in live mode so messages can be injected;
    // dropping it sends EOF, which lets the agent finish and exit.
    let mut child_stdin = child.stdin.take();
    if input.is_none() {
        // One-shot stdin payload (if any), then close.
        if let (Some(payload), Some(mut si)) = (invocation.stdin.clone(), child_stdin.take()) {
            let _ = si.write_all(payload.as_bytes()).await;
            drop(si);
        }
    }

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| CoreError::other("failed to capture stdout"))?;
    let stderr = child.stderr.take();

    // Drain stderr into a buffer concurrently for error reporting.
    let stderr_task = stderr.map(|err| {
        tokio::spawn(async move {
            let mut buf = String::new();
            let mut reader = BufReader::new(err).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                buf.push_str(&line);
                buf.push('\n');
                if buf.len() > 16_000 {
                    break;
                }
            }
            buf
        })
    });

    let mut outcome = RunOutcome {
        success: false,
        agent_session_id: None,
        model: None,
        result_text: None,
        usage: TokenUsage::default(),
        exit_code: None,
        error: None,
        cancelled: false,
        timed_out: false,
    };
    let mut assistant_buf = String::new();
    let mut saw_result = false;

    let mut lines = BufReader::new(stdout).lines();
    let deadline = timeout.map(|d| tokio::time::Instant::now() + d);

    let stop = loop {
        let timer = async {
            match deadline {
                Some(dl) => tokio::time::sleep_until(dl).await,
                None => pending::<()>().await,
            }
        };
        // Pending forever once the input channel is gone, so the branch is inert.
        let next_input = async {
            match input.as_mut() {
                Some(rx) => rx.recv().await,
                None => pending::<Option<String>>().await,
            }
        };

        tokio::select! {
            biased;
            _ = cancel.cancelled() => break StopReason::Cancelled,
            _ = timer => break StopReason::TimedOut,
            msg = next_input => {
                match msg {
                    Some(text) => {
                        if let Some(si) = child_stdin.as_mut() {
                            let line = adapter.format_input_message(&text);
                            let _ = si.write_all(line.as_bytes()).await;
                            let _ = si.flush().await;
                        }
                    }
                    // Channel closed: send EOF so the agent finishes and exits.
                    None => {
                        input = None;
                        child_stdin = None;
                    }
                }
            }
            line = lines.next_line() => {
                match line {
                    Ok(Some(line)) => {
                        for event in adapter.parse_line(&line) {
                            apply_event(&event, &mut outcome, &mut assistant_buf, &mut saw_result);
                            on_event(&event);
                        }
                    }
                    Ok(None) => break StopReason::Eof,
                    Err(e) => {
                        outcome.error = Some(format!("read error: {e}"));
                        break StopReason::Eof;
                    }
                }
            }
        }
    };

    match stop {
        StopReason::Cancelled => {
            let _ = child.kill().await;
            outcome.cancelled = true;
        }
        StopReason::TimedOut => {
            let _ = child.kill().await;
            outcome.timed_out = true;
        }
        StopReason::Eof => {}
    }

    let status = child.wait().await.ok();
    outcome.exit_code = status.and_then(|s| s.code());

    let stderr_text = match stderr_task {
        Some(handle) => handle.await.unwrap_or_default(),
        None => String::new(),
    };

    // Fall back to accumulated assistant text if no explicit result was given.
    if outcome.result_text.is_none() && !assistant_buf.trim().is_empty() {
        outcome.result_text = Some(assistant_buf.trim().to_string());
    }

    // Determine success: explicit result wins; otherwise a clean exit.
    if !outcome.cancelled && !outcome.timed_out {
        if saw_result {
            // success flag already set by the result event
        } else {
            outcome.success = matches!(outcome.exit_code, Some(0));
        }
    }

    if !outcome.success && outcome.error.is_none() {
        let trimmed = stderr_text.trim();
        if !trimmed.is_empty() {
            outcome.error = Some(trimmed.to_string());
        } else if outcome.cancelled {
            outcome.error = Some("cancelled".into());
        } else if outcome.timed_out {
            outcome.error = Some("timed out".into());
        }
    }

    Ok(outcome)
}

fn apply_event(
    event: &AgentEvent,
    outcome: &mut RunOutcome,
    assistant_buf: &mut String,
    saw_result: &mut bool,
) {
    match event {
        AgentEvent::Init {
            agent_session_id,
            model,
        } => {
            if agent_session_id.is_some() {
                outcome.agent_session_id = agent_session_id.clone();
            }
            if model.is_some() {
                outcome.model = model.clone();
            }
        }
        AgentEvent::Assistant { text } => {
            if !assistant_buf.is_empty() {
                assistant_buf.push('\n');
            }
            assistant_buf.push_str(text);
        }
        AgentEvent::Result {
            success,
            result_text,
            usage,
        } => {
            *saw_result = true;
            outcome.success = *success;
            if let Some(t) = result_text {
                outcome.result_text = Some(t.clone());
            }
            outcome.usage.add(usage);
        }
        AgentEvent::Error { message } => {
            outcome.error = Some(message.clone());
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::ClaudeAdapter;

    /// A fake adapter that runs a shell script emitting canned stream-json so we
    /// can exercise the runner without the real CLI.
    struct FakeAdapter {
        script: String,
    }

    impl AgentAdapter for FakeAdapter {
        fn kind(&self) -> crate::models::AgentKind {
            crate::models::AgentKind::Claude
        }
        fn default_binary(&self) -> &'static str {
            "sh"
        }
        fn build_invocation(&self, _spec: &RunSpec, binary: &str) -> crate::agents::Invocation {
            crate::agents::Invocation {
                program: binary.to_string(),
                args: vec!["-c".into(), self.script.clone()],
                stdin: None,
            }
        }
        fn parse_line(&self, line: &str) -> Vec<AgentEvent> {
            ClaudeAdapter.parse_line(line)
        }
    }

    #[tokio::test]
    async fn runs_and_collects_outcome() {
        let script = r#"
printf '%s\n' '{"type":"system","subtype":"init","session_id":"s-9","model":"m"}'
printf '%s\n' '{"type":"assistant","message":{"content":[{"type":"text","text":"working"}]}}'
printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"result":"finished","num_turns":2,"total_cost_usd":0.01,"usage":{"input_tokens":10,"output_tokens":5}}'
"#;
        let adapter = FakeAdapter {
            script: script.into(),
        };
        let spec = RunSpec::new("x", std::env::temp_dir());
        let mut kinds = Vec::new();
        let outcome = run_agent(
            &adapter,
            "sh",
            &spec,
            CancelToken::new(),
            Some(Duration::from_secs(10)),
            None,
            |e| kinds.push(e.kind()),
        )
        .await
        .unwrap();

        assert!(outcome.success);
        assert_eq!(outcome.agent_session_id.as_deref(), Some("s-9"));
        assert_eq!(outcome.result_text.as_deref(), Some("finished"));
        assert_eq!(outcome.usage.input_tokens, 10);
        assert!(kinds.contains(&"init"));
        assert!(kinds.contains(&"result"));
    }

    #[tokio::test]
    async fn missing_binary_is_unavailable() {
        let adapter = FakeAdapter {
            script: String::new(),
        };
        let spec = RunSpec::new("x", std::env::temp_dir());
        let err = run_agent(
            &adapter,
            "definitely-not-a-real-binary-xyz",
            &spec,
            CancelToken::new(),
            None,
            None,
            |_| {},
        )
        .await
        .unwrap_err();
        assert!(matches!(err, CoreError::AgentUnavailable(_)));
    }

    #[tokio::test]
    async fn cancellation_kills_process() {
        let adapter = FakeAdapter {
            script: "sleep 30".into(),
        };
        let spec = RunSpec::new("x", std::env::temp_dir());
        let cancel = CancelToken::new();
        let c2 = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            c2.cancel();
        });
        let outcome = run_agent(&adapter, "sh", &spec, cancel, None, None, |_| {})
            .await
            .unwrap();
        assert!(outcome.cancelled);
        assert!(!outcome.success);
    }

    #[tokio::test]
    async fn live_input_is_injected_and_eof_completes() {
        // Echo a fixed assistant event for every stdin line, then exit on EOF.
        let script = r#"while IFS= read -r line; do printf '%s\n' '{"type":"assistant","message":{"content":[{"type":"text","text":"got"}]}}'; done"#;
        let adapter = FakeAdapter {
            script: script.into(),
        };
        let mut spec = RunSpec::new("hello", std::env::temp_dir());
        spec.live = true;

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        tx.send("first message".into()).unwrap();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(120)).await;
            drop(tx); // EOF -> child exits
        });

        let mut texts = Vec::new();
        let outcome = run_agent(
            &adapter,
            "sh",
            &spec,
            CancelToken::new(),
            Some(Duration::from_secs(5)),
            Some(rx),
            |e| {
                if let AgentEvent::Assistant { text } = e {
                    texts.push(text.clone());
                }
            },
        )
        .await
        .unwrap();

        assert!(
            texts.iter().any(|t| t == "got"),
            "injected stdin line should produce output"
        );
        assert!(!outcome.cancelled && !outcome.timed_out);
    }
}
