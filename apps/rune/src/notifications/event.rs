use super::{Button, Kind, Notice, PromptAction};
use crate::composer::DraftScope;
use serde_json::Value;

fn field<'a>(value: &'a Value, keys: &[&str]) -> &'a str {
    keys.iter()
        .find_map(|key| {
            value
                .get(key)
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
        })
        .unwrap_or("")
}
fn shortened(value: &str, max: usize) -> String {
    let mut out: String = value.chars().take(max).collect();
    if value.chars().count() > max {
        out.push('…');
    }
    out
}
fn approval_buttons(value: &Value) -> Vec<Button> {
    let id = field(value, &["prompt_id", "request_id"]);
    let Some(generation) = value.get("execution_generation").and_then(Value::as_u64) else {
        return vec![];
    };
    if id.is_empty() {
        return vec![];
    }
    let choices: Vec<&str> = value
        .get("choices")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    // Only expose a definite allow/deny pair offered by this exact prompt.
    let allow = choices
        .iter()
        .copied()
        .find(|s| matches!(*s, "once" | "approve" | "allow"));
    let deny = choices
        .iter()
        .copied()
        .find(|s| matches!(*s, "deny" | "reject"));
    allow
        .zip(deny)
        .map(|(allow, deny)| {
            [("Approve", allow), ("Reject", deny)]
                .into_iter()
                .map(|(label, answer)| Button {
                    label: label.into(),
                    action: PromptAction {
                        prompt_id: id.into(),
                        execution_generation: generation,
                        answer: answer.into(),
                    },
                })
                .collect()
        })
        .unwrap_or_default()
}
pub(super) fn notice(
    scope: &DraftScope,
    kind: &str,
    p: &Value,
    runtime: &str,
    session_title: &str,
) -> Option<Notice> {
    let (kind, title, body, global) = match kind {
        "message.complete" => (
            Kind::TurnDone,
            "Hermes finished",
            field(p, &["text", "rendered"]),
            false,
        ),
        "message.error" | "error" => (
            Kind::TurnError,
            "Hermes needs attention",
            field(p, &["message", "error", "text"]),
            false,
        ),
        "approval.request" => (
            Kind::Approval,
            "Approval needed",
            field(p, &["command", "description"]),
            false,
        ),
        "clarify.request" | "input.request" => (
            Kind::Input,
            "Input needed",
            field(p, &["question", "description", "message"]),
            false,
        ),
        "notification.show"
            if matches!(
                field(p, &["key"]),
                "credits.depleted" | "credits.window_limit" | "credits.restored"
            ) =>
        {
            (Kind::Credits, "Hermes account", field(p, &["text"]), true)
        }
        _ => return None,
    };
    if kind == Kind::Credits && body.trim().is_empty() {
        return None;
    }
    let mut title = title.to_string();
    if matches!(kind, Kind::Approval | Kind::Input) {
        let label = if session_title.trim().is_empty() {
            scope.session.as_deref().unwrap_or(runtime)
        } else {
            session_title
        };
        if !label.is_empty() {
            title = format!("{title} — {}", shortened(label, 80));
        }
    }
    let body = if body.is_empty() {
        match kind {
            Kind::TurnDone => "Your reply is ready.",
            Kind::Input => "Open this conversation to answer.",
            Kind::Approval => "Open this conversation to review the request.",
            _ => "Open Hermes for details.",
        }
    } else {
        body
    };
    Some(Notice {
        plugin: None,
        kind,
        scope: if global {
            DraftScope {
                session: None,
                ..scope.clone()
            }
        } else {
            scope.clone()
        },
        runtime: if global {
            String::new()
        } else {
            runtime.into()
        },
        title,
        body: if kind == Kind::TurnDone {
            body.trim().chars().take(140).collect()
        } else {
            shortened(body.trim(), 600)
        },
        global,
        tag: None,
        silent: false,
        buttons: if kind == Kind::Approval {
            approval_buttons(p)
        } else {
            vec![]
        },
    })
}
