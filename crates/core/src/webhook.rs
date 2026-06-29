//! Outbound notification webhooks (Slack / Discord / generic JSON).
//!
//! Posts via the `curl` CLI rather than pulling in an HTTP/TLS stack, keeping
//! the core dependency-light and consistent with the rest of the crate's
//! shell-out style (git, gh). Failures are best-effort and never propagate into
//! the scheduling loop.

use crate::config::WebhookConfig;
use std::process::Command;

/// A notification to deliver. `event` is a stable key ("task_complete" /
/// "task_fail"); `title` and `body` are human-readable.
#[derive(Debug, Clone, Default)]
pub struct Notification {
    pub event: String,
    pub title: String,
    pub body: String,
    /// Optional related link (e.g. a PR URL).
    pub link: Option<String>,
    /// Project name (for templates).
    pub project: String,
    /// Task title (for templates).
    pub task: String,
    /// Task status (for templates), e.g. "completed" / "failed".
    pub status: String,
}

impl Notification {
    pub fn new(event: &str, title: impl Into<String>, body: impl Into<String>) -> Self {
        Notification {
            event: event.to_string(),
            title: title.into(),
            body: body.into(),
            ..Default::default()
        }
    }
}

/// Render a template by substituting `{key}` placeholders from the notification.
fn render_template(template: &str, n: &Notification) -> String {
    template
        .replace("{event}", &n.event)
        .replace("{title}", &n.title)
        .replace("{body}", &n.body)
        .replace("{project}", &n.project)
        .replace("{task}", &n.task)
        .replace("{status}", &n.status)
        .replace("{link}", n.link.as_deref().unwrap_or(""))
}

/// True if this webhook wants the given event.
pub fn wants(cfg: &WebhookConfig, event: &str) -> bool {
    if !cfg.enabled {
        return false;
    }
    match event {
        "task_complete" => cfg.on_task_complete,
        "task_fail" => cfg.on_task_fail,
        _ => true,
    }
}

/// Build the JSON payload for a webhook target. When the webhook has a custom
/// template, the rendered text drives the message; otherwise a built-in format.
pub fn payload(cfg: &WebhookConfig, n: &Notification) -> serde_json::Value {
    let line = if cfg.template.trim().is_empty() {
        let mut l = format!("{}: {}", n.title, n.body);
        if let Some(link) = &n.link {
            l.push_str(&format!("\n{link}"));
        }
        l
    } else {
        render_template(&cfg.template, n)
    };
    match cfg.kind.as_str() {
        // Slack incoming webhook.
        "slack" => serde_json::json!({ "text": line }),
        // Discord webhook.
        "discord" => serde_json::json!({ "content": line }),
        // Generic: full structured payload, with the rendered message included.
        _ => serde_json::json!({
            "event": n.event,
            "title": n.title,
            "body": n.body,
            "project": n.project,
            "task": n.task,
            "status": n.status,
            "link": n.link,
            "message": line,
        }),
    }
}

/// Deliver one notification to one webhook (blocking). Returns Ok on a 2xx-ish
/// exit, Err with a short reason otherwise. Intended to be called from a
/// blocking context (e.g. `spawn_blocking`).
pub fn deliver(cfg: &WebhookConfig, n: &Notification) -> Result<(), String> {
    let body = payload(cfg, n).to_string();
    let out = Command::new("curl")
        .args([
            "-sS",
            "-X",
            "POST",
            "-H",
            "Content-Type: application/json",
            "-d",
        ])
        .arg(&body)
        .arg(&cfg.url)
        .output()
        .map_err(|e| format!("failed to run curl: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(kind: &str) -> WebhookConfig {
        WebhookConfig {
            id: "w1".into(),
            name: "test".into(),
            url: "https://example.invalid/hook".into(),
            kind: kind.into(),
            enabled: true,
            on_task_complete: true,
            on_task_fail: false,
            project_ids: vec![],
            template: String::new(),
        }
    }

    #[test]
    fn slack_and_discord_shapes() {
        let n = Notification::new("task_complete", "Task done", "Build green");
        assert_eq!(payload(&cfg("slack"), &n)["text"], "Task done: Build green");
        assert_eq!(
            payload(&cfg("discord"), &n)["content"],
            "Task done: Build green"
        );
        assert_eq!(payload(&cfg("generic"), &n)["event"], "task_complete");
    }

    #[test]
    fn custom_template_is_rendered() {
        let mut c = cfg("slack");
        c.template = "[{project}] {task} → {status}".into();
        let mut n = Notification::new("task_complete", "ignored", "ignored");
        n.project = "web".into();
        n.task = "Add auth".into();
        n.status = "completed".into();
        assert_eq!(payload(&c, &n)["text"], "[web] Add auth → completed");
        // Generic includes the rendered message and structured fields.
        let mut g = cfg("generic");
        g.template = "{task}: {status}".into();
        let p = payload(&g, &n);
        assert_eq!(p["message"], "Add auth: completed");
        assert_eq!(p["project"], "web");
    }

    #[test]
    fn event_filtering() {
        assert!(wants(&cfg("slack"), "task_complete"));
        assert!(!wants(&cfg("slack"), "task_fail"));
    }
}
