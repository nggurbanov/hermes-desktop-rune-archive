//! Background source acquisition and bounded decode. Partial copies never become originals.
use super::*;
use crate::{
    gateway::{Gateway, stream},
    pages::PageRequest,
};
use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
};
const LIMIT: u64 = 64 * 1024 * 1024;
pub(super) struct Loaded {
    pub original: Original,
    pub image: Result<Prepared, String>,
}
pub(super) enum Prepared {
    Raster {
        width: u32,
        height: u32,
        bytes: Arc<[u8]>,
    },
}
pub(super) fn load(
    connection: &Connection,
    target: &Target,
    cancel: &Cancel,
) -> Result<Loaded, String> {
    let mut directory = TempMedia::new();
    let path = directory.reserve(&target.label)?;
    let mut mime = String::new();
    let result = (|| {
        if cancel.cancelled() {
            return Err("File transfer cancelled".into());
        }
        match &target.source {
            Source::AgentPath(source) => {
                let mut file = create(&path)?;
                let mut gateway = Gateway::connect(connection)?;
                let request =
                    stream::Request::download(source.clone(), target.owner.session.clone(), LIMIT);
                let meta =
                    gateway.stream_file(&target.owner.profile, &request, cancel, &mut file)?;
                if meta.status == 404 || meta.status == 405 {
                    drop(file);
                    let request = PageRequest::get("/api/fs/read-data-url")
                        .query("path", source)
                        .query("session_id", target.owner.session.as_deref().unwrap_or(""));
                    let value = gateway.page_request(&target.owner.profile, &request)?;
                    let (kind, bytes) = data(value)?;
                    mime = kind;
                    if cancel.cancelled() {
                        return Err("File transfer cancelled".into());
                    }
                    crate::media::write_private(&path, &bytes)?;
                } else {
                    if meta.status != 200 {
                        return Err(format!("File preview refused (HTTP {})", meta.status));
                    }
                    mime = meta.content_type.unwrap_or_default();
                    file.sync_all()
                        .map_err(|_| "The preview file could not be completed")?;
                }
            }
            Source::RemoteUrl(source) => {
                let url = url::Url::parse(source).map_err(|_| "Invalid remote media URL")?;
                if !matches!(url.scheme(), "http" | "https")
                    || !url.username().is_empty()
                    || url.password().is_some()
                {
                    return Err("This remote media URL cannot be previewed".into());
                }
                let request = PageRequest::get("/api/media/proxy")
                    .query("url", source)
                    .query("session_id", target.owner.session.as_deref().unwrap_or(""));
                let value =
                    Gateway::connect(connection)?.page_request(&target.owner.profile, &request)?;
                let (kind, bytes) = data(value)?;
                mime = kind;
                if cancel.cancelled() {
                    return Err("File transfer cancelled".into());
                }
                crate::media::write_private(&path, &bytes)?;
            }
            Source::Inline { mime: kind, bytes } => {
                if bytes.len() as u64 > LIMIT {
                    return Err("The file exceeds the 64 MiB preview transfer limit".into());
                }
                mime = kind.clone();
                crate::media::write_private(&path, bytes)?;
            }
        }
        if cancel.cancelled() {
            return Err("File transfer cancelled".into());
        }
        let size = std::fs::metadata(&path)
            .map_err(|_| "The original preview file is missing")?
            .len();
        let image = decode(&path, &mime, &target.label);
        if cancel.cancelled() {
            return Err("File preview cancelled".into());
        }
        Ok((size, image))
    })();
    match result {
        Ok((size, image)) => Ok(Loaded {
            original: Original {
                path,
                size,
                _directory: directory,
            },
            image,
        }),
        Err(error) => Err(error),
    }
}
fn data(value: serde_json::Value) -> Result<(String, Vec<u8>), String> {
    let data = value
        .as_str()
        .or(value["data_url"].as_str())
        .or(value["dataUrl"].as_str())
        .or(value["data"].as_str())
        .ok_or("The agent returned no file data")?;
    crate::chat::media::decode_data_url(data)
}
fn create(path: &Path) -> Result<File, String> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .map_err(|_| "The private preview file could not be created".into())
}
fn decode(path: &Path, mime: &str, label: &str) -> Result<Prepared, String> {
    if mime
        .split(';')
        .next()
        .is_some_and(|m| m.trim().eq_ignore_ascii_case("image/svg+xml"))
        || label.to_ascii_lowercase().ends_with(".svg")
    {
        let bytes = std::fs::read(path).map_err(|_| "The SVG preview could not be read")?;
        // Rune's SVG entry point fixes raster dimensions instead of trusting an unbounded SVG viewbox.
        if bytes.len() > 8 * 1024 * 1024 {
            return Err(
                "The SVG exceeds the 8 MiB preview limit; save the original instead".into(),
            );
        }
        let (width, height, bytes) = super::image::rasterize_svg(&bytes, 2048)?;
        return Ok(Prepared::Raster {
            width,
            height,
            bytes: bytes.into(),
        });
    }
    let dimensions = ::image::ImageReader::open(path)
        .map_err(|_| "The image could not be read")?
        .with_guessed_format()
        .map_err(|_| "This image format cannot be previewed")?
        .into_dimensions()
        .map_err(|_| "This image format cannot be previewed")?;
    if dimensions.0 as u64 * dimensions.1 as u64 * 4 > 64 * 1024 * 1024 {
        return Err("Image exceeds the safe preview limit; save the original instead".into());
    }
    let mut reader = ::image::ImageReader::open(path)
        .map_err(|_| "The image could not be read")?
        .with_guessed_format()
        .map_err(|_| "This image format cannot be previewed")?;
    let mut limits = ::image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().map_err(
        |_| "Image exceeds the safe preview limit or is unsupported; save the original instead",
    )?;
    let (width, height) = (image.width(), image.height());
    let bytes = image.into_rgba8().into_raw().into();
    Ok(Prepared::Raster {
        width,
        height,
        bytes,
    })
}
pub(super) fn save(source: &Path, destination: &Path, cancel: &Cancel) -> Result<(), String> {
    let parent = destination
        .parent()
        .ok_or("The destination has no parent directory")?;
    let temporary = parent.join(format!(
        ".hermes-preview-{}.tmp",
        uuid::Uuid::new_v4().simple()
    ));
    let result: Result<(), String> = (|| {
        let mut input =
            File::open(source).map_err(|_| "The original file is no longer available")?;
        let mut output = create(&temporary)?;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            if cancel.cancelled() {
                return Err("File save cancelled".into());
            }
            let n = input
                .read(&mut buffer)
                .map_err(|_| "The original file could not be read")?;
            if n == 0 {
                break;
            }
            output
                .write_all(&buffer[..n])
                .map_err(|_| "The original file could not be saved")?;
        }
        output
            .sync_all()
            .map_err(|_| "The original file could not be saved")?;
        if cancel.cancelled() {
            return Err("File save cancelled".into());
        }
        std::fs::rename(&temporary, destination)
            .map_err(|_| "The original file could not replace the destination".to_owned())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}
