use super::*;
use crate::gateway::conversation::SessionTransport;
use serde_json::json;
use std::time::{Duration, Instant};
impl Chat {
    pub fn request_completions(
        &mut self,
        connection: Connection,
        owner: DraftScope,
        query: String,
        runtime: Option<String>,
    ) -> Option<Duration> {
        let view = self.views.entry(owner.clone()).or_default();
        if query.len() > 16 * 1024 {
            return None;
        }
        if let Some((name, arg)) = parse_slash(&query) {
            if !arg.is_empty() && view.slash_catalog["commands"][name]["argument_mode"] == "text" {
                return None;
            }
        }
        if view.completion_query != query {
            view.completion_query = query.clone();
            view.completion_ready.clear();
            view.completion_items.clear();
            view.completion_due = Some(Instant::now() + Duration::from_millis(120));
            view.completion_pending = false;
            view.slash_highlight = 0;
        }
        if view.completion_pending || view.completion_ready == query {
            return None;
        }
        let due = view.completion_due?;
        if let Some(wait) = due.checked_duration_since(Instant::now()) {
            return Some(wait);
        }
        if self.completion_inflight >= 2 {
            return Some(Duration::from_millis(80));
        }
        let waker = self.waker.clone()?;
        let send = self.send.clone();
        view.completion_pending = true;
        view.completion_due = None;
        self.completion_inflight += 1;
        std::thread::spawn(move || {
            let result = (|| {
                let mut transport = SessionTransport::connect(&connection, &owner.profile)?;
                let mut params = json!({"text":query});
                if let Some(runtime) = runtime {
                    params["session_id"] = json!(runtime);
                }
                transport
                    .call_with_timeout("complete.slash", params, Duration::from_secs(10))
                    .map_err(|e| e.message)
            })();
            if send
                .send(Reply::Completions {
                    owner,
                    query,
                    result,
                })
                .is_ok()
            {
                waker();
            }
        });
        None
    }
    pub fn apply_completions(
        &mut self,
        owner: &DraftScope,
        query: &str,
        result: Result<Value, String>,
    ) {
        let Some(view) = self.views.get_mut(owner) else {
            return;
        };
        if view.completion_query != query {
            return;
        }
        view.completion_pending = false;
        view.completion_ready = query.into();
        if let Ok(value) = result {
            view.completion_items = rows(query, &value, &view.slash_catalog);
        }
    }
}
fn prefix_utf16(text: &str, units: usize) -> Option<&str> {
    if units == 0 {
        return Some("");
    }
    let mut used = 0;
    for (index, ch) in text.char_indices() {
        if used == units {
            return Some(&text[..index]);
        }
        used += ch.len_utf16();
        if used > units {
            return None;
        }
    }
    (used == units).then_some(text)
}
fn rows(query: &str, value: &Value, catalog: &Value) -> Vec<SlashRow> {
    let replace = value["replace_from"].as_u64().unwrap_or(1) as usize;
    let Some(prefix) = prefix_utf16(query, replace) else {
        return vec![];
    };
    value["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| {
            let item = row["text"].as_str()?;
            let text = if replace > 1 {
                format!("{prefix}{item}")
            } else if item.starts_with('/') {
                item.into()
            } else {
                format!("/{item}")
            };
            let (name, arg) = parse_slash(&text)?;
            if super::slash::desktop_refusal(catalog, &name, &arg).is_some() {
                return None;
            }
            Some(SlashRow {
                name: text.trim_start_matches('/').to_owned(),
                description: row["meta"].as_str().unwrap_or("").into(),
                skill: row["kind"] == "skill" || catalog["skills"].get(&name).is_some(),
            })
        })
        .take(100)
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn argument_completions_replace_the_correct_unicode_boundary_and_keep_extensions() {
        let query = "/personality Али";
        let options = rows(
            query,
            &json!({"replace_from":13,"items":[{"text":"Алиса","meta":"Assistant"}]}),
            &json!({}),
        );
        assert_eq!(options[0].name, "personality Алиса");
        let rows = rows(
            "/w",
            &json!({"replace_from":1,"items":[{"text":"/work","kind":"skill"},{"text":"/exit"}]}),
            &json!({}),
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "work");
        assert!(rows[0].skill);
        assert!(prefix_utf16("😀name", 1).is_none());
        assert_eq!(prefix_utf16("😀name", 2), Some("😀"));
    }
    #[test]
    fn completion_replies_cannot_replace_newer_query_other_profile_or_closed_view() {
        let first = DraftScope {
            draft_origin: None,
            profile_intent: crate::source::ProfileIntent::Backend,
            connection: 4,
            profile: "work".into(),
            session: Some("same-id".into()),
        };
        let second = DraftScope {
            profile: "personal".into(),
            ..first.clone()
        };
        let mut chat = Chat::new();
        for owner in [&first, &second] {
            chat.presentation(owner.clone()).completion_query = "/new-query".into();
        }
        chat.apply_completions(
            &first,
            "/old-query",
            Ok(json!({"items":[{"text":"/wrong"}]})),
        );
        assert!(chat.views[&first].completion_items.is_empty());
        chat.apply_completions(
            &first,
            "/new-query",
            Ok(json!({"items":[{"text":"/right"}]})),
        );
        assert_eq!(chat.views[&first].completion_items[0].name, "right");
        assert!(chat.views[&second].completion_items.is_empty());
        chat.close_profile(4, "work");
        chat.apply_completions(
            &first,
            "/new-query",
            Ok(json!({"items":[{"text":"/late"}]})),
        );
        assert!(!chat.views.contains_key(&first));
        assert!(chat.views.contains_key(&second));
    }
}
