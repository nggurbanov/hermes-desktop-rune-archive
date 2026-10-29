//! Avatar files are bounded and normalized off the UI thread.
use super::Bots;
use base64::Engine;
use rune::{ImageCache, Waker};
use std::{
    io::{Cursor, Read},
    path::PathBuf,
    sync::{
        Arc,
        mpsc::{self, Receiver, Sender},
    },
};
pub struct AvatarWorker {
    send: Sender<(u64, PathBuf)>,
    receive: Receiver<(u64, Result<String, String>)>,
}
impl AvatarWorker {
    pub fn new(wake: Waker) -> Self {
        let (send, requests) = mpsc::channel::<(u64, PathBuf)>();
        let (replies, receive) = mpsc::channel();
        std::thread::spawn(move || {
            for (epoch, path) in requests {
                let result = read_avatar(&path);
                if replies.send((epoch, result)).is_err() {
                    break;
                }
                wake();
            }
        });
        Self { send, receive }
    }
}
fn read_avatar(path: &std::path::Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|_| "The avatar file could not be opened")?;
    let mut bytes = vec![];
    file.by_ref()
        .take(15_000_001)
        .read_to_end(&mut bytes)
        .map_err(|_| "The avatar file could not be read")?;
    if bytes.len() > 15_000_000 {
        return Err("Choose an avatar smaller than 15 MB".into());
    }
    normalize(&bytes)
}
fn normalize(bytes: &[u8]) -> Result<String, String> {
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| "The avatar image type is unsupported")?;
    if !matches!(
        reader.format(),
        Some(
            image::ImageFormat::Png
                | image::ImageFormat::Jpeg
                | image::ImageFormat::WebP
                | image::ImageFormat::Gif
        )
    ) {
        return Err("Choose a PNG, JPEG, WebP or GIF avatar".into());
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(96 * 1024 * 1024);
    reader.limits(limits);
    let decoded = reader
        .decode()
        .map_err(|_| "The avatar could not be decoded within the image limits")?;
    let side = decoded.width().min(decoded.height());
    let cropped = decoded
        .crop_imm(
            (decoded.width() - side) / 2,
            (decoded.height() - side) / 2,
            side,
            side,
        )
        .resize_exact(256, 256, image::imageops::FilterType::Lanczos3);
    let mut output = Cursor::new(vec![]);
    cropped
        .write_to(&mut output, image::ImageFormat::Png)
        .map_err(|_| "The avatar could not be normalized")?;
    Ok(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(output.into_inner())
    ))
}
impl Bots {
    pub fn init(&mut self, waker: Waker) {
        if self.images.is_none() {
            self.images = Some(ImageCache::new(waker.clone()));
        }
        if self.avatar_worker.is_none() {
            self.avatar_worker = Some(AvatarWorker::new(waker));
        }
    }
    pub fn stage_avatar(&mut self, path: PathBuf) {
        if self.busy || self.normalizing {
            return;
        }
        let Some(worker) = &self.avatar_worker else {
            self.error = Some("The native image service is unavailable".into());
            return;
        };
        if worker.send.send((self.editor_epoch, path)).is_err() {
            self.error = Some("The avatar image service stopped".into());
            return;
        }
        self.normalizing = true;
    }
    pub(super) fn poll_avatar(&mut self) -> bool {
        let replies = self
            .avatar_worker
            .as_ref()
            .map(|w| w.receive.try_iter().collect::<Vec<_>>())
            .unwrap_or_default();
        let mut changed = false;
        for (epoch, result) in replies {
            if epoch != self.editor_epoch {
                continue;
            }
            self.normalizing = false;
            match result {
                Ok(data) => self.set_avatar(Some(data)),
                Err(e) => self.error = Some(e),
            }
            changed = true;
        }
        changed
    }
    pub(super) fn set_avatar(&mut self, data: Option<String>) {
        self.avatar_preview = data
            .as_deref()
            .and_then(|d| decode_data(d).ok())
            .and_then(|bytes| {
                self.images
                    .as_ref()
                    .map(|c| c.load(Arc::<[u8]>::from(bytes)))
            });
        self.avatar_data = data;
    }
    pub(super) fn seed_avatar(&mut self, data: Option<String>) {
        if self.avatar_data != self.avatar_saved {
            return;
        }
        self.avatar_saved = data.clone();
        self.set_avatar(data);
    }
}
pub fn decode_data(data: &str) -> Result<Vec<u8>, String> {
    if data.len() > 22_000_000 {
        return Err("The avatar is too large".into());
    }
    let (meta, body) = data.split_once(',').ok_or("Invalid avatar data URL")?;
    if !matches!(
        meta,
        "data:image/png;base64" | "data:image/jpeg;base64" | "data:image/webp;base64"
    ) {
        return Err("Unsupported avatar image type".into());
    }
    base64::engine::general_purpose::STANDARD
        .decode(body)
        .map_err(|_| "Invalid avatar encoding".into())
}
pub(super) fn fixture_cache() -> ImageCache {
    ImageCache::new(Arc::new(|| {}))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn supported_uploads_normalize_to_bounded_square_and_reject_junk() {
        let image = image::DynamicImage::new_rgba8(9, 4);
        let mut file = Cursor::new(vec![]);
        image.write_to(&mut file, image::ImageFormat::Png).unwrap();
        let data = normalize(&file.into_inner()).unwrap();
        let decoded = image::load_from_memory(&decode_data(&data).unwrap()).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (256, 256));
        assert!(normalize(b"not an image").is_err());
        assert!(decode_data("data:text/html;base64,AA==").is_err());
    }
}
