use super::*;
use crate::{gateway::Gateway, pages::PageRequest};
use base64::Engine;
pub(crate) fn decode_data_url(url: &str) -> Result<(String, Vec<u8>), String> {
    if url.len() > 48 * 1024 * 1024 {
        return Err("This media exceeds the 32 MiB inline limit".into());
    }
    let (head, encoded) = url.split_once(',').ok_or("Invalid media data")?;
    if !head.starts_with("data:") || !head.ends_with(";base64") {
        return Err("This media encoding cannot be opened".into());
    }
    let mime = head
        .trim_start_matches("data:")
        .trim_end_matches(";base64")
        .to_owned();
    if mime.is_empty() || mime.chars().any(char::is_control) {
        return Err("The media type is invalid".into());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| "The media encoding is invalid")?;
    if bytes.len() > 32 * 1024 * 1024 {
        return Err("This media exceeds the 32 MiB inline limit".into());
    }
    Ok((mime, bytes))
}
pub(crate) fn data(value: Value) -> Result<(String, Vec<u8>), String> {
    let url = value
        .as_str()
        .or(value["data_url"].as_str())
        .or(value["dataUrl"].as_str())
        .or(value["data"].as_str())
        .ok_or("The agent returned no image data")?;
    let (mime, bytes) = decode_data_url(url)?;
    if !mime.starts_with("image/") {
        return Err("This media no longer contains an image".into());
    }
    Ok((mime, bytes))
}
pub(super) fn request(chat: &mut Chat, connection: Connection, owner: DraftScope, source: String) {
    let key = (owner.clone(), source.clone());
    if chat.media.contains_key(&key) || chat.media_inflight >= 4 {
        return;
    }
    let Some(waker) = chat.waker.clone() else {
        return;
    };
    while chat.media.len() >= 64 {
        if let Some(old) = chat.media_order.pop_front() {
            chat.media.remove(&old);
        } else {
            break;
        }
    }
    chat.media_sequence = chat.media_sequence.saturating_add(1);
    let request_id = chat.media_sequence;
    chat.media_inflight += 1;
    chat.media_order.push_back(key.clone());
    chat.media.insert(
        key,
        Media {
            request_id,
            decoded: false,
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
                    PageRequest::get("/api/media/proxy")
                        .query("url", &source)
                        .query("session_id", owner.session.as_deref().unwrap_or(""))
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
        let result = result.and_then(|(mime, bytes)| preview(mime, bytes));
        if send
            .send(Reply::Media {
                owner,
                source,
                request_id,
                result,
            })
            .is_ok()
        {
            waker();
        }
    });
}

/// Raster previews have bounded decoded size; originals remain available for download.
fn preview(mime: String, bytes: Vec<u8>) -> Result<(String, Vec<u8>), String> {
    let prepared = crate::image_previews::prepare(&mime, &bytes, 1000)?;
    let image = image::RgbaImage::from_raw(prepared.width, prepared.height, prepared.rgba)
        .ok_or("The image preview could not be prepared")?;
    let mut output = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut output, image::ImageFormat::Png)
        .map_err(|_| "The image preview could not be prepared")?;
    Ok(("image/png".into(), output.into_inner()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Write},
        net::TcpListener,
        sync::Arc,
        time::{Duration, Instant},
    };
    fn owner() -> DraftScope {
        DraftScope {
            draft_origin: None,
            profile_intent: crate::source::ProfileIntent::Backend,
            connection: 7,
            profile: "work".into(),
            session: Some("child".into()),
        }
    }
    #[test]
    fn highly_compressed_large_raster_is_refused_before_preview_allocation() {
        // Valid 8192² RGB PNG, 192 MiB decoded, under 200 KiB on the wire.
        let bytes = include_bytes!("../../fixtures/oversized-preview.png");
        assert!(bytes.len() < 200 * 1024);
        let result = preview("image/png".into(), bytes.to_vec());
        assert!(result.unwrap_err().contains("safe preview limit"));
    }
    #[test]
    fn actual_image_read_is_authenticated_child_scoped_and_raster_cache_is_resized() {
        let pixels = image::RgbaImage::from_pixel(1600, 800, image::Rgba([32, 180, 140, 255]));
        let mut encoded = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(pixels)
            .write_to(&mut encoded, image::ImageFormat::Png)
            .unwrap();
        let data = base64::engine::general_purpose::STANDARD.encode(encoded.into_inner());
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let _ws = tungstenite::accept(stream).unwrap();
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(&mut stream);
            let mut request = String::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                request.push_str(&line);
            }
            let url = url::Url::parse(&format!(
                "http://localhost{}",
                request
                    .lines()
                    .next()
                    .unwrap()
                    .split_whitespace()
                    .nth(1)
                    .unwrap()
            ))
            .unwrap();
            let query: HashMap<_, _> = url.query_pairs().collect();
            assert_eq!(url.path(), "/api/fs/read-data-url");
            assert_eq!(query.get("profile").unwrap(), "work");
            assert_eq!(query.get("session_id").unwrap(), "child");
            assert_eq!(query.get("path").unwrap(), "/agent/image.png");
            assert_eq!(query.get("token").unwrap(), "synthetic");
            let body =
                serde_json::json!({"data_url":format!("data:image/png;base64,{data}")}).to_string();
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        });
        let mut chat = Chat::new();
        chat.init(Arc::new(|| {}));
        let key = (owner(), "/agent/image.png".to_owned());
        chat.ensure_media(
            Connection::Url {
                endpoint: format!("http://{addr}"),
                token: Some("synthetic".into()),
            },
            key.0.clone(),
            key.1.clone(),
        );
        let deadline = Instant::now() + Duration::from_secs(4);
        while chat.media.get(&key).unwrap().image.is_none()
            && chat.media.get(&key).unwrap().error.is_none()
            && Instant::now() < deadline
        {
            chat.poll();
            std::thread::sleep(Duration::from_millis(5));
        }
        let media = chat.media.get(&key).unwrap();
        assert!(media.error.is_none(), "{:?}", media.error);
        assert_eq!(media.image.as_ref().unwrap().wait().unwrap(), [1000, 500]);
        server.join().unwrap();
    }
    #[test]
    fn stale_and_foreign_media_receipts_never_replace_a_retried_child_image() {
        let mut chat = Chat::new();
        let owner = owner();
        let source = "/same/path.png".to_owned();
        chat.media_inflight = 2;
        chat.media.insert(
            (owner.clone(), source.clone()),
            Media {
                request_id: 2,
                decoded: false,
                image: None,
                error: None,
                loading: true,
            },
        );
        chat.send
            .send(Reply::Media {
                owner: owner.clone(),
                source: source.clone(),
                request_id: 1,
                result: Err("stale".into()),
            })
            .unwrap();
        chat.poll();
        assert!(
            chat.media
                .get(&(owner.clone(), source.clone()))
                .unwrap()
                .error
                .is_none()
        );
        assert!(
            chat.media
                .get(&(owner.clone(), source.clone()))
                .unwrap()
                .loading
        );
        chat.send
            .send(Reply::Media {
                owner: DraftScope {
                    profile: "other".into(),
                    ..owner.clone()
                },
                source: source.clone(),
                request_id: 2,
                result: Err("foreign".into()),
            })
            .unwrap();
        chat.poll();
        assert!(
            chat.media
                .get(&(owner.clone(), source.clone()))
                .unwrap()
                .error
                .is_none()
        );
        chat.send
            .send(Reply::Media {
                owner: owner.clone(),
                source: source.clone(),
                request_id: 2,
                result: Err("own failure".into()),
            })
            .unwrap();
        chat.poll();
        assert_eq!(
            chat.media.get(&(owner, source)).unwrap().error.as_deref(),
            Some("own failure")
        );
    }
    #[test]
    fn media_window_limits_transcript_cache_and_parallel_work_without_network_fallback() {
        let owner = owner();
        let connection = Connection::Url {
            endpoint: "http://127.0.0.1:1".into(),
            token: None,
        };
        let mut chat = Chat::new();
        chat.init(Arc::new(|| {}));
        let sources: Vec<_> = (0..100)
            .map(|i| {
                let pixels =
                    image::RgbaImage::from_pixel(2, 2, image::Rgba([i as u8, 40, 90, 255]));
                let mut bytes = std::io::Cursor::new(Vec::new());
                image::DynamicImage::ImageRgba8(pixels)
                    .write_to(&mut bytes, image::ImageFormat::Png)
                    .unwrap();
                format!(
                    "data:image/png;base64,{}",
                    base64::engine::general_purpose::STANDARD.encode(bytes.into_inner())
                )
            })
            .collect();
        let deadline = Instant::now() + Duration::from_secs(3);
        while chat.media.len() < 24 && Instant::now() < deadline {
            chat.prepare_media(connection.clone(), owner.clone(), sources.clone());
            assert!(chat.media_inflight <= 4);
            chat.poll();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(chat.media.len(), 24);
        assert!(
            chat.media
                .keys()
                .all(|(scope, source)| scope == &owner && sources[76..].contains(source))
        );
        chat.close_connection(owner.connection);
        assert!(chat.media.is_empty());
        chat.poll();
        assert!(
            chat.media.is_empty(),
            "Retired media results cannot repopulate a new connection"
        );
    }
}
