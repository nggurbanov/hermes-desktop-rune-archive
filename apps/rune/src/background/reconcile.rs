use super::*;
pub(super) fn snapshot(entry: &mut Entry, value: &Value, now: f64) -> Vec<(String, String)> {
    let Some(rows) = value["processes"].as_array() else {
        return vec![];
    };
    let reported = rows
        .iter()
        .filter_map(|row| row["session_id"].as_str())
        .collect::<HashSet<_>>();
    let mut fresh = HashMap::new();
    let mut order = vec![];
    for row in rows {
        let Some(id) = row["session_id"].as_str().filter(|s| !s.is_empty()) else {
            continue;
        };
        if entry.dismissed.contains(id) {
            continue;
        }
        let item = item(row, id);
        if !fresh.contains_key(id) {
            order.push(id.to_string());
        }
        fresh.insert(id.to_string(), item);
    }
    let mut notices = vec![];
    for id in &order {
        let item = &fresh[id];
        if item.state != State::Running
            && entry
                .items
                .iter()
                .any(|old| old.id == *id && old.state == State::Running)
        {
            let mut body = item.title.clone();
            if let Some(code) = item.exit_code {
                body.push_str(&format!(" (exit {code})"));
            }
            if !item.output.trim().is_empty() {
                let lines = item.output.trim().lines().collect::<Vec<_>>();
                body.push('\n');
                body.push_str(&lines[lines.len().saturating_sub(3)..].join("\n"));
            }
            notices.push((
                if item.state == State::Failed {
                    "Background process failed"
                } else {
                    "Background process finished"
                }
                .into(),
                body,
            ));
        }
    }
    let old = std::mem::take(&mut entry.items);
    let mut next = vec![];
    for previous in old {
        if let Some(mut item) = fresh.remove(&previous.id) {
            settle(&mut item, Some(&previous), now);
            next.push(item);
        }
    }
    for id in order {
        if let Some(mut item) = fresh.remove(&id) {
            settle(&mut item, None, now);
            next.push(item);
        }
    }
    entry.items = next;
    entry.dismissed.retain(|id| reported.contains(id.as_str()));
    notices
}
fn item(value: &Value, id: &str) -> Item {
    let code = value["exit_code"].as_i64();
    let exited = value["status"].as_str() == Some("exited");
    let title = value["command"]
        .as_str()
        .unwrap_or("")
        .lines()
        .next()
        .unwrap_or("")
        .trim();
    Item {
        id: id.into(),
        title: if title.is_empty() {
            "background process".into()
        } else {
            title.into()
        },
        output: value["output_tail"].as_str().unwrap_or("").into(),
        exit_code: code,
        state: if !exited {
            State::Running
        } else if code.is_some_and(|code| code != 0) {
            State::Failed
        } else {
            State::Done
        },
        stopping: false,
        until: None,
    }
}
fn settle(item: &mut Item, previous: Option<&Item>, now: f64) {
    if item.state == State::Running {
        item.stopping = previous.is_some_and(|p| p.stopping);
        return;
    }
    item.until = previous
        .and_then(|p| p.until)
        .or_else(|| Some(now + if item.state == State::Failed { 12. } else { 4. }));
}
