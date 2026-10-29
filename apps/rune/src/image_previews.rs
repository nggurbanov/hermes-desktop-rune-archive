//! Bounded background decoding for untrusted images. Rune only receives prepared RGBA pixels.
#[cfg(test)]
mod tests;
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
};
const MAX_SOURCE: usize = 32 * 1024 * 1024;
const MAX_QUEUED_BYTES: usize = 64 * 1024 * 1024;
const MAX_REQUESTS: usize = 32;
const MAX_PREPARED_BYTES: usize = 64 * 1024 * 1024;
pub struct Prepared {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}
pub struct Decoded {
    pub id: u64,
    pub result: Result<rune::Image, String>,
}
struct Job {
    id: u64,
    generation: u64,
    bytes: Arc<[u8]>,
    max_edge: u32,
    cancel: Arc<AtomicBool>,
}
pub struct SafeImages {
    send: mpsc::SyncSender<Job>,
    receive: mpsc::Receiver<Decoded>,
    generation: Arc<AtomicU64>,
    outstanding: HashMap<u64, (usize, usize, Arc<AtomicBool>, u64)>,
    prepared_bytes: usize,
    queued_bytes: usize,
    next: u64,
}
impl SafeImages {
    pub fn new(waker: rune::Waker) -> Self {
        let cache = Arc::new(rune::ImageCache::new(waker.clone()));
        let (send, tasks) = mpsc::sync_channel::<Job>(MAX_REQUESTS);
        let (results, receive) = mpsc::sync_channel(MAX_REQUESTS);
        let generation = Arc::new(AtomicU64::new(0));
        let active = generation.clone();
        std::thread::Builder::new()
            .name("hermes-image-previews".into())
            .spawn(move || {
                while let Ok(job) = tasks.recv() {
                    let result = if active.load(Ordering::Acquire) != job.generation
                        || job.cancel.load(Ordering::Acquire)
                    {
                        Err("Image preview cancelled".into())
                    } else {
                        prepare("", &job.bytes, job.max_edge).and_then(|prepared| {
                            if active.load(Ordering::Acquire) != job.generation
                                || job.cancel.load(Ordering::Acquire)
                            {
                                return Err("Image preview cancelled".into());
                            }
                            Ok(cache.rgba(
                                prepared.width,
                                prepared.height,
                                Arc::<[u8]>::from(prepared.rgba),
                            ))
                        })
                    };
                    if results.send(Decoded { id: job.id, result }).is_err() {
                        break;
                    }
                    waker();
                }
            })
            .expect("image preview worker");
        Self {
            send,
            receive,
            generation,
            outstanding: HashMap::new(),
            queued_bytes: 0,
            prepared_bytes: 0,
            next: 0,
        }
    }
    pub fn request(&mut self, _mime: &str, bytes: Arc<[u8]>, max_edge: u32) -> Result<u64, String> {
        if bytes.len() > MAX_SOURCE {
            return Err("This image exceeds the 32 MiB safe preview limit".into());
        }
        let predicted = max_edge.clamp(1, 2048) as usize;
        let predicted = predicted * predicted * 4;
        if self.outstanding.len() >= MAX_REQUESTS
            || self.prepared_bytes.saturating_add(predicted) > MAX_PREPARED_BYTES
            || self.queued_bytes.saturating_add(bytes.len()) > MAX_QUEUED_BYTES
        {
            return Err(
                "Image preview queue is full; retry after the current images finish".into(),
            );
        }
        self.next = self
            .next
            .checked_add(1)
            .ok_or("Image preview request IDs exhausted")?;
        let id = self.next;
        let generation = self.generation.load(Ordering::Acquire);
        let size = bytes.len();
        let cancel = Arc::new(AtomicBool::new(false));
        self.send
            .try_send(Job {
                id,
                generation,
                bytes,
                max_edge,
                cancel: cancel.clone(),
            })
            .map_err(|_| "The image preview worker is unavailable")?;
        self.outstanding
            .insert(id, (size, predicted, cancel, generation));
        self.prepared_bytes += predicted;
        self.queued_bytes += size;
        Ok(id)
    }
    pub fn poll(&mut self) -> Vec<Decoded> {
        let mut out = vec![];
        while let Ok(mut decoded) = self.receive.try_recv() {
            if let Some((size, predicted, cancel, generation)) =
                self.outstanding.remove(&decoded.id)
            {
                if cancel.load(Ordering::Acquire)
                    || generation != self.generation.load(Ordering::Acquire)
                {
                    decoded.result = Err("Image preview cancelled".into());
                }
                self.prepared_bytes = self.prepared_bytes.saturating_sub(predicted);
                self.queued_bytes = self.queued_bytes.saturating_sub(size);
                out.push(decoded);
            }
        }
        out
    }
    pub fn cancel(&mut self, id: u64) {
        if let Some((_, _, cancel, _)) = self.outstanding.get(&id) {
            cancel.store(true, Ordering::Release);
        }
    }
    pub fn cancel_all(&mut self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
    }
}
impl Drop for SafeImages {
    fn drop(&mut self) {
        self.cancel_all();
    }
}

/// The transport MIME is advisory. Detect the actual bytes before selecting any decoder.
/// This synchronous primitive is called only by background workers (also the chat media lane).
pub fn prepare(_mime: &str, bytes: &[u8], max_edge: u32) -> Result<Prepared, String> {
    if bytes.len() > MAX_SOURCE {
        return Err("This image exceeds the 32 MiB safe preview limit".into());
    }
    let max_edge = max_edge.clamp(1, 2048);
    if let Ok(format) = ::image::guess_format(bytes) {
        let dimensions = ::image::ImageReader::with_format(std::io::Cursor::new(bytes), format)
            .into_dimensions()
            .map_err(|_| "This image format cannot be previewed")?;
        if dimensions.0 == 0
            || dimensions.1 == 0
            || dimensions.0 > 8192
            || dimensions.1 > 8192
            || dimensions.0 as u64 * dimensions.1 as u64 * 4 > 64 * 1024 * 1024
        {
            return Err(
                "Image exceeds the safe preview limit; download the original instead".into(),
            );
        }
        let mut reader = ::image::ImageReader::with_format(std::io::Cursor::new(bytes), format);
        let mut limits = ::image::Limits::default();
        limits.max_image_width = Some(8192);
        limits.max_image_height = Some(8192);
        limits.max_alloc = Some(64 * 1024 * 1024);
        reader.limits(limits);
        let image=reader.decode().map_err(|_|"Image could not be decoded within the safe preview limit; download the original instead")?;
        let image = if image.width() > max_edge || image.height() > max_edge {
            image.resize(max_edge, max_edge, ::image::imageops::FilterType::Triangle)
        } else {
            image
        };
        return Ok(Prepared {
            width: image.width(),
            height: image.height(),
            rgba: image.into_rgba8().into_raw(),
        });
    }
    let (width, height, rgba) = crate::preview::image::rasterize_svg(bytes, max_edge)?;
    Ok(Prepared {
        width,
        height,
        rgba,
    })
}
