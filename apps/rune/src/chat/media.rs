use super::*;
use crate::{gateway::Gateway, pages::PageRequest};
use base64::Engine;
fn data(value: Value) -> Result<(String, Vec<u8>), String> {
    let url = value
        .as_str()
        .or(value["data_url"].as_str())
        .or(value["dataUrl"].as_str())
        .ok_or("The agent returned no image data")?;
    if url.len() > 48 * 1024 * 1024 {
        return Err("This image exceeds the 32 MiB preview limit".into());
    }
    let (head, encoded) = url.split_once(',').ok_or("Invalid image data")?;
    if !head.starts_with("data:image/") || !head.ends_with(";base64") {
        return Err("This image format cannot be previewed".into());
    }
    let mime = head
        .trim_start_matches("data:")
        .trim_end_matches(";base64")
        .to_owned();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| "The image encoding is invalid")?;
    if bytes.len() > 32 * 1024 * 1024 {
        return Err("This image exceeds the preview limit".into());
    }
    Ok((mime, bytes))
}
pub(super) fn request(chat: &mut Chat, connection: Connection, owner: DraftScope, source: String) {
    let key = (owner.clone(), source.clone());
    if chat.media.contains_key(&key) {
        return;
    }
    let Some(waker) = chat.waker.clone() else {
        return;
    };
    chat.media.insert(
        key,
        Media {
            image: None,
            error: None,
            loading: true,
        },
    );
    let send = chat.send.clone();
    std::thread::spawn(move || {
        let result = if source.starts_with("data:") {
            data(serde_json::json!(source))
        } else {
            (|| {
                let request = if source.starts_with("https://") || source.starts_with("http://") {
                    PageRequest::get("/api/media/proxy").query("url", &source)
                } else {
                    PageRequest::get("/api/fs/read-data-url")
                        .query("path", &source)
                        .query("session_id", owner.session.as_deref().unwrap_or(""))
                };
                let value =
                    Gateway::connect(&connection)?.page_request(&owner.profile, &request)?;
                data(value)
            })()
        };
        if send
            .send(Reply::Media {
                owner,
                source,
                result,
            })
            .is_ok()
        {
            waker();
        }
    });
}
