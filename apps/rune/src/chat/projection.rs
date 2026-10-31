//! Display projection only: opaque native replay carriers never become prose.
use crate::gateway::HistoryMessage;
use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub enum Part {
    Text(String),
    Reasoning(String),
    Tool {
        id: String,
        name: String,
        args: String,
        output: Option<String>,
        process: Option<String>,
    },
    Image {
        source: String,
        label: String,
    },
    File {
        path: String,
        label: String,
    },
}
#[derive(Clone)]
pub struct Message {
    pub key: String,
    pub row: Option<u64>,
    pub through: Option<u64>,
    pub timestamp: Option<f64>,
    pub role: String,
    pub text: String,
    pub parts: Vec<Part>,
}

fn printable(value: &Value) -> String {
    value.as_str().map(str::to_owned).unwrap_or_else(|| {
        if value.is_null() {
            String::new()
        } else {
            serde_json::to_string_pretty(value).unwrap_or_default()
        }
    })
}
fn readable_reasoning(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) if !s.trim().is_empty() => {
            if let Ok(parsed) = serde_json::from_str::<Value>(s) {
                if parsed.is_array() || parsed.is_object() {
                    readable_reasoning(&parsed, out);
                    return;
                }
            }
            out.push(s.clone());
        }
        Value::Array(rows) => {
            for row in rows {
                readable_reasoning(row, out);
            }
        }
        Value::Object(row) => {
            // reason/summary payloads only. Native assistant/provider sidecars
            // contain hidden model context and are deliberately not traversed.
            let kind = row.get("type").and_then(Value::as_str).unwrap_or("");
            if kind.contains("reasoning") || kind == "summary_text" || kind == "text" {
                for key in ["text", "summary", "content"] {
                    if let Some(value) = row.get(key) {
                        readable_reasoning(value, out);
                    }
                }
            }
        }
        _ => {}
    }
}
fn content_parts(v: &Value, out: &mut Vec<Part>) {
    let Some(rows) = v.as_array() else {
        return;
    };
    for row in rows {
        let kind = row["type"].as_str().unwrap_or("");
        match kind {
            "text" | "input_text" | "output_text" => {
                if let Some(text) = row["text"].as_str().filter(|s| !s.is_empty()) {
                    out.push(Part::Text(text.into()));
                }
            }
            "reasoning" | "thinking" | "reasoning_text" => {
                if let Some(text) = row["text"].as_str().or(row["thinking"].as_str()) {
                    out.push(Part::Reasoning(text.into()));
                }
            }
            "image" | "image_url" | "input_image" => {
                let source = row["image_url"]["url"]
                    .as_str()
                    .or(row["image_url"].as_str())
                    .or(row["url"].as_str())
                    .or(row["source"]["url"].as_str())
                    .or(row["path"].as_str());
                if let Some(source) = source {
                    out.push(Part::Image {
                        source: source.into(),
                        label: row["name"].as_str().unwrap_or("Image").into(),
                    });
                } else if let (Some(data), Some(mime)) = (
                    row["source"]["data"].as_str(),
                    row["source"]["media_type"].as_str(),
                ) {
                    out.push(Part::Image {
                        source: format!("data:{mime};base64,{data}"),
                        label: "Image".into(),
                    });
                }
            }
            "file" | "file_ref" | "attachment" | "input_file" => {
                if let Some(path) = row["path"]
                    .as_str()
                    .or(row["file_path"].as_str())
                    .or(row["url"].as_str())
                {
                    out.push(Part::File {
                        path: path.into(),
                        label: row["name"]
                            .as_str()
                            .or(row["filename"].as_str())
                            .unwrap_or(path)
                            .into(),
                    });
                }
            }
            "tool-call" | "tool_use" | "function_call" => {
                out.push(tool(row));
            }
            _ => {}
        }
    }
}
fn tool(v: &Value) -> Part {
    let function = v.get("function").unwrap_or(v);
    Part::Tool {
        id: v["id"]
            .as_str()
            .or(v["tool_call_id"].as_str())
            .unwrap_or("")
            .into(),
        name: function["name"]
            .as_str()
            .or(v["toolName"].as_str())
            .unwrap_or("Tool")
            .into(),
        args: printable(
            function
                .get("arguments")
                .or(function.get("input"))
                .or(v.get("args"))
                .unwrap_or(&Value::Null),
        ),
        output: v.get("result").or(v.get("output")).map(printable),
        process: v["process_id"].as_str().map(str::to_owned),
    }
}
/// Public tool activity projection is shared by main and spectator transcripts.
pub fn activity(value: &Value) -> Part {
    let mut part = tool(value);
    if let Part::Tool { output, .. } = &mut part {
        if output.is_none() && value["state"] == "complete" {
            *output = Some(
                value["summary"]
                    .as_str()
                    .or(value["text"].as_str())
                    .unwrap_or("")
                    .to_owned(),
            );
        }
    }
    part
}
pub fn project(rows: &[HistoryMessage]) -> Vec<Message> {
    let mut messages: Vec<Message> = vec![];
    for row in rows {
        let raw = &row.raw;
        if raw["display_kind"] == "hidden" {
            continue;
        }
        if row.role == "tool" {
            let id = raw["tool_call_id"].as_str().unwrap_or("");
            let mut attached = false;
            for message in messages.iter_mut().rev() {
                for part in &mut message.parts {
                    if let Part::Tool {
                        id: held,
                        output,
                        process,
                        ..
                    } = part
                    {
                        if !id.is_empty() && held == id {
                            *output = Some(row.content.clone());
                            *process = raw["process_id"]
                                .as_str()
                                .map(str::to_owned)
                                .or(process.clone());
                            attached = true;
                            break;
                        }
                    }
                }
                if attached {
                    message.through = message.through.max(row.row_id());
                    break;
                }
                if message.role == "user" {
                    break;
                }
            }
            if attached {
                continue;
            }
        }
        let mut parts = vec![];
        let displayed = raw.get("display_content").unwrap_or(&raw["content"]);
        let mut reason = vec![];
        for key in ["reasoning", "reasoning_content", "reasoning_details"] {
            readable_reasoning(&raw[key], &mut reason);
        }
        reason.dedup();
        if !reason.is_empty() {
            parts.push(Part::Reasoning(reason.join("\n\n")));
        }
        content_parts(raw.get("parts").unwrap_or(displayed), &mut parts);
        if !parts.iter().any(|p| matches!(p, Part::Text(_)))
            && !row.content.is_empty()
            && row.role != "tool"
        {
            parts.push(Part::Text(row.content.clone()));
        }
        if let Some(calls) = raw["tool_calls"].as_array() {
            for call in calls {
                parts.push(tool(call));
            }
        }
        if row.role == "tool" {
            let mut p = tool(raw);
            if let Part::Tool { output, .. } = &mut p {
                *output = Some(row.content.clone());
            }
            parts.push(p);
        }
        if let Some(attachments) = raw["attachments"].as_array() {
            content_parts(&Value::Array(attachments.clone()), &mut parts);
        }
        messages.push(Message {
            key: row.display_key(),
            row: row.row_id(),
            through: row.row_id(),
            timestamp: raw["timestamp"].as_f64(),
            role: row.role.clone(),
            text: row.content.clone(),
            parts,
        });
    }
    messages
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn tools_join_by_identity_and_private_replay_never_becomes_reasoning() {
        let rows = [
            json!({"row_id":10,"role":"assistant","text":"Answer","reasoning_details":{"type":"native_assistant","text":"private"},"tool_calls":[{"id":"call-a","function":{"name":"terminal","arguments":"{\"command\":\"pwd\"}"}}]}),
            json!({"row_id":11,"role":"tool","tool_call_id":"call-a","text":"/project"}),
        ];
        let projected = project(
            &rows
                .iter()
                .map(HistoryMessage::from_row)
                .collect::<Vec<_>>(),
        );
        assert_eq!(projected.len(), 1);
        assert_eq!(projected[0].through, Some(11));
        assert!(
            !projected[0]
                .parts
                .iter()
                .any(|p| matches!(p, Part::Reasoning(_)))
        );
        assert!(
            projected[0]
                .parts
                .iter()
                .any(|p| matches!(p,Part::Tool{output:Some(s),..}if s=="/project"))
        );
    }
    #[test]
    fn ordered_multimodal_parts_keep_durable_identity_and_readable_thoughts() {
        let rows = [
            HistoryMessage::from_row(
                &json!({"row_id":91,"role":"user","content":[{"type":"text","text":"Compare"},{"type":"image_url","image_url":{"url":"data:image/png;base64,eA=="}},{"type":"file","path":"/remote/readme.md","name":"Notes"}]}),
            ),
            HistoryMessage::from_row(
                &json!({"row_id":92,"role":"assistant","text":"Done","reasoning_details":"[{\"type\":\"reasoning.summary\",\"summary\":\"Checked.\"}]"}),
            ),
        ];
        let p = project(&rows);
        assert_eq!(p[0].row, Some(91));
        assert!(matches!(
            &p[0].parts[..],
            [Part::Text(_), Part::Image { .. }, Part::File { .. }]
        ));
        assert!(matches!(&p[1].parts[0],Part::Reasoning(s)if s=="Checked."));
    }
    #[test]
    fn search_identity_is_retained_for_duplicate_rowless_messages() {
        let first = HistoryMessage::text("assistant", "same");
        let second = HistoryMessage::text("assistant", "same");
        let before = project(&[first.clone(), second.clone()]);
        assert_ne!(before[0].key, before[1].key);
        let mut streamed = second.clone();
        streamed.content.push_str(" extended");
        let after = project(&[HistoryMessage::text("user", "earlier"), first, streamed]);
        assert_eq!(before[0].key, after[1].key);
        assert_eq!(before[1].key, after[2].key);
    }
}
