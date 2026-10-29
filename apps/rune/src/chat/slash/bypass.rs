//! Session approval bypass is a value-setting control, never a profile policy write.
use super::*;
use crate::gateway::conversation::Snapshot;

pub(super) fn execute(
    transport: &mut SessionTransport,
    intent: &mut SlashIntent,
    persist: &impl Fn(&SlashIntent) -> Result<(), String>,
) -> Result<Value, CallError> {
    if !intent.params.as_object().is_some_and(|p| {
        p.len() == 3
            && p.keys()
                .all(|k| matches!(k.as_str(), "session_id" | "key" | "value"))
    }) {
        return Err(
            "A conversation bypass request cannot change profile policy or another scope".into(),
        );
    }
    let stored = intent
        .stored
        .as_deref()
        .ok_or("Choose an attached conversation")?;
    let requested = intent.params["value"]
        .as_str()
        .filter(|v| matches!(*v, "0" | "1"))
        .ok_or("Invalid session bypass value")?
        .to_owned();
    let snapshot = transport.resume(stored).map_err(|error| {
        if intent.attempted {
            after_write(error)
        } else {
            error
        }
    })?;
    if snapshot.stored != stored || intent.runtime.as_deref() != Some(snapshot.runtime.as_str()) {
        return Err(preflight_error(
            intent,
            "The conversation owner changed before its bypass control was sent",
        ));
    }
    if intent.params["session_id"].as_str() != Some(snapshot.runtime.as_str()) {
        return Err(preflight_error(
            intent,
            "The bypass request belongs to a different runtime",
        ));
    }
    // Canonical recovery observes current truth; it never replays a state-setting
    // request over a possible later choice from another Hermes client.
    if intent.attempted && transport.canonical {
        let actual = read(transport, &snapshot.runtime).map_err(after_write)?;
        let warning = (actual != requested).then_some("The saved bypass change is not the current state. Choose the switch again to make a fresh request.");
        return Ok(receipt(&snapshot, &actual, Some(actual == "1"), warning));
    }
    if intent.attempted && !transport.canonical && intent.config_ack.is_some() {
        return Ok(legacy_receipt(&snapshot, &requested));
    }
    if intent.generation != Some(snapshot.generation) {
        return Err(preflight_error(
            intent,
            "The conversation started a different turn. Review its current bypass state and choose again",
        ));
    }
    intent.attempted = true;
    persist(intent)?;
    let ack = transport.call("config.set", intent.params.clone())?;
    if ack["key"] != "yolo"
        || ack["scope"] != "session"
        || ack["value"].as_str() != Some(requested.as_str())
    {
        return Err(unknown(
            "The session bypass acknowledgement is incomplete; its outcome must be reviewed",
        ));
    }
    intent.config_ack = Some(ack);
    persist(intent).map_err(|message| unknown(&message))?;
    if transport.canonical {
        let actual = read(transport, &snapshot.runtime).map_err(after_write)?;
        let warning = (actual != requested).then_some(
            "Hermes did not keep the requested bypass state. The current confirmed state is shown.",
        );
        return Ok(receipt(&snapshot, &actual, Some(actual == "1"), warning));
    }
    // Legacy has no config.get yolo. Its snapshot reports effective bypass,
    // including profile Off and a frozen process --yolo, which this control cannot revoke.
    let current = transport.resume(stored).map_err(after_write)?;
    if current.runtime != snapshot.runtime || current.stored != snapshot.stored {
        return Err(unknown(
            "The conversation owner changed after its bypass acknowledgement",
        ));
    }
    Ok(legacy_receipt(&current, &requested))
}
fn legacy_receipt(current: &Snapshot, requested: &str) -> Value {
    let reported = current.info["yolo"].as_bool();
    let warning = if requested == "0" && reported == Some(true) {
        Some(
            "The session flag was disabled, but Hermes still reports approval bypass active. The profile policy or operator's process setting may keep it enabled.",
        )
    } else if reported.is_none() {
        Some(
            "The session flag was acknowledged, but this agent did not report its effective bypass state.",
        )
    } else {
        None
    };
    receipt(current, requested, reported, warning)
}
fn preflight_error(intent: &SlashIntent, message: &str) -> CallError {
    if intent.attempted {
        unknown(message)
    } else {
        message.into()
    }
}
fn after_write(mut error: CallError) -> CallError {
    error.uncertain = true;
    error
}

fn read(transport: &mut SessionTransport, runtime: &str) -> Result<String, CallError> {
    let value = transport.call("config.get", json!({"session_id":runtime,"key":"yolo"}))?;
    if value["key"] != "yolo" || value["scope"] != "session" {
        return Err(unknown(
            "The gateway did not identify its session bypass readback",
        ));
    }
    value["value"]
        .as_str()
        .filter(|v| matches!(*v, "0" | "1"))
        .map(str::to_owned)
        .ok_or_else(|| unknown("The gateway returned an unknown session bypass value"))
}
fn receipt(
    snapshot: &Snapshot,
    flag: &str,
    reported: Option<bool>,
    warning: Option<&str>,
) -> Value {
    json!({"type":"exec","session_bypass":{"session_id":snapshot.runtime,"stored_session_id":snapshot.stored,"execution_generation":snapshot.generation,"value":flag,"reported_yolo":reported},"warning":warning,"output":warning.unwrap_or(if flag == "1" {"Conversation approval bypass enabled"} else {"Conversation approval bypass disabled"})})
}
