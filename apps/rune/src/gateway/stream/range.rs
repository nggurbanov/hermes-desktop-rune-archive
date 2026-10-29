//! A decoder-thread source: bounded authenticated ranges or one sequential representation.
use super::{Cancel, Gateway, Metadata, Method, Request, Route};
use crate::gateway::Connection;
#[cfg(test)]
mod external_tests;
mod http;
use std::{
    collections::VecDeque,
    io::{self, Read, Seek, SeekFrom, Write},
    sync::{Mutex, mpsc},
    time::Duration,
};
const BLOCK: u64 = 256 * 1024;
const CACHE_BLOCKS: usize = 8;
const PIPE_CHUNK: usize = 64 * 1024;
const PIPE_DEPTH: usize = 4;

pub struct RangeSource {
    state: Mutex<State>,
    length: Option<u64>,
    seekable: bool,
    cancel: Cancel,
}
struct State {
    position: u64,
    failed: Option<String>,
    mode: Mode,
}
enum Packet {
    Data(Vec<u8>),
    End(u64),
    Error(String),
}
enum Transport {
    Agent {
        gateway: Box<Gateway>,
        profile: String,
        path: String,
        session: Option<String>,
    },
    External(http::Http),
}
impl Transport {
    fn request(&self, limit: u64) -> Request {
        match self {
            Self::Agent { path, session, .. } => media_request(path, session.clone(), limit),
            Self::External(_) => media_request("", None, limit),
        }
    }
    fn transfer(
        &mut self,
        request: &Request,
        cancel: &Cancel,
        validate: impl FnMut(&Metadata) -> Result<(), String>,
        output: &mut impl Write,
    ) -> Result<Metadata, String> {
        match self {
            Self::Agent {
                gateway, profile, ..
            } => transfer(gateway, profile, request, cancel, validate, output),
            Self::External(http) => http.transfer(request, cancel, validate, output),
        }
    }
}
enum Mode {
    Ranges {
        transport: Transport,
        etag: String,
        cache: VecDeque<(u64, Vec<u8>)>,
    },
    Sequential {
        receiver: mpsc::Receiver<Packet>,
        chunk: Vec<u8>,
        offset: usize,
        ended: bool,
    },
}
impl RangeSource {
    /// Must be called on the decoder/IO worker, never on a UI or audio callback thread.
    pub fn open(
        connection: &Connection,
        profile: &str,
        path: &str,
        session: Option<String>,
        cancel: Cancel,
    ) -> Result<Self, String> {
        let gateway = Gateway::connect(connection)?;
        Self::open_transport(
            Transport::Agent {
                gateway: Box::new(gateway),
                profile: profile.into(),
                path: path.into(),
                session,
            },
            cancel,
        )
    }
    /// External media never receives gateway credentials or a gateway transport.
    pub fn open_external(url: &str, cancel: Cancel) -> Result<Self, String> {
        Self::open_transport(Transport::External(http::Http::new(url)?), cancel)
    }
    fn open_transport(mut transport: Transport, cancel: Cancel) -> Result<Self, String> {
        let mut request = transport.request(0);
        request.method = Method::Head;
        let head = transport.transfer(&request, &cancel, |_| Ok(()), &mut io::sink())?;
        if head.status == 200
            && head.content_length.is_some()
            && head
                .accept_ranges
                .as_deref()
                .is_some_and(|s| s.eq_ignore_ascii_case("bytes"))
            && head.etag.as_deref().is_some_and(strong_etag)
        {
            let length = head.content_length.unwrap();
            let etag = head.etag.unwrap();
            let mut cache = VecDeque::new();
            if length > 0 {
                let end = (BLOCK - 1).min(length - 1);
                let mut probe = transport.request(end + 1);
                probe.range = Some(format!("bytes=0-{end}"));
                probe.if_range = Some(etag.clone());
                let mut bytes = Vec::with_capacity((end + 1) as usize);
                let mut ignored = false;
                let result = transport.transfer(
                    &probe,
                    &cancel,
                    |metadata| {
                        if metadata.status == 200 {
                            ignored = true;
                            return Err("The source does not honor ranges".into());
                        }
                        validate_range(metadata, 0, end, length, &etag)
                    },
                    &mut bytes,
                );
                if ignored {
                    // No bytes have escaped. Start one whole response, whose own headers define
                    // its representation, rather than stitching ranges from an unproven source.
                    return Self::sequential(transport, cancel);
                }
                let metadata = result?;
                if metadata.received != end + 1 || bytes.len() as u64 != end + 1 {
                    return Err("The media range body was truncated".into());
                }
                cache.push_back((0, bytes));
            }
            return Ok(Self {
                length: head.content_length,
                seekable: true,
                cancel,
                state: Mutex::new(State {
                    position: 0,
                    failed: None,
                    mode: Mode::Ranges {
                        transport,
                        etag,
                        cache,
                    },
                }),
            });
        }
        if !matches!(head.status, 200 | 204 | 405 | 501) {
            return Err(format!(
                "Media metadata request returned HTTP {}",
                head.status
            ));
        }
        Self::sequential(transport, cancel)
    }
    fn sequential(mut transport: Transport, cancel: Cancel) -> Result<Self, String> {
        let (ready_send, ready) = mpsc::sync_channel(1);
        let (send, receiver) = mpsc::sync_channel(PIPE_DEPTH);
        let worker_cancel = cancel.clone();
        std::thread::Builder::new()
            .name("hermes-media-stream".into())
            .spawn(move || {
                let mut ready_send = Some(ready_send);
                let request = transport.request(u64::MAX);
                let mut writer = Pipe {
                    send: send.clone(),
                    cancel: worker_cancel.clone(),
                };
                let result = transport.transfer(
                    &request,
                    &worker_cancel,
                    |metadata| {
                        if metadata.status != 200 {
                            return Err(format!("Media stream returned HTTP {}", metadata.status));
                        }
                        if metadata.content_range.is_some() {
                            return Err("Unexpected partial response for sequential media".into());
                        }
                        if let Some(ready) = ready_send.take() {
                            ready
                                .send(Ok(metadata.clone()))
                                .map_err(|_| "Media source closed")?;
                        }
                        Ok(())
                    },
                    &mut writer,
                );
                match result {
                    Ok(metadata) => {
                        let _ = send_packet(&send, Packet::End(metadata.received), &worker_cancel);
                    }
                    Err(error) => {
                        if let Some(ready) = ready_send.take() {
                            let _ = ready.send(Err(error));
                        } else {
                            let _ = send_packet(&send, Packet::Error(error), &worker_cancel);
                        }
                    }
                }
            })
            .map_err(|_| "The media stream worker could not start")?;
        let metadata = loop {
            if cancel.cancelled() {
                return Err("Media source cancelled".into());
            }
            match ready.recv_timeout(Duration::from_millis(50)) {
                Ok(result) => break result?,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => return Err("The media stream ended before its headers".into()),
            }
        };
        Ok(Self {
            length: metadata.content_length,
            seekable: false,
            cancel,
            state: Mutex::new(State {
                position: 0,
                failed: None,
                mode: Mode::Sequential {
                    receiver,
                    chunk: Vec::new(),
                    offset: 0,
                    ended: false,
                },
            }),
        })
    }
    pub fn byte_len(&self) -> Option<u64> {
        self.length
    }
    pub fn seekable(&self) -> bool {
        self.seekable
    }
}
impl Drop for RangeSource {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}
fn transfer(
    gateway: &mut Gateway,
    profile: &str,
    request: &Request,
    cancel: &Cancel,
    mut validate: impl FnMut(&Metadata) -> Result<(), String>,
    output: &mut impl Write,
) -> Result<Metadata, String> {
    #[cfg(target_os = "linux")]
    let native = matches!(&gateway.auth, crate::gateway::Auth::Native(_));
    #[cfg(not(target_os = "linux"))]
    let native = false;
    for attempt in 0..2 {
        let metadata = gateway.stream_file_events(
            profile,
            request,
            cancel,
            |metadata| {
                // A rejected native ticket cannot have written a body. Mint a fresh purpose-bound
                // ticket once; permission errors and ordinary bearer credentials never retry.
                if native && attempt == 0 && metadata.status == 401 {
                    Ok(())
                } else {
                    validate(metadata)
                }
            },
            output,
        )?;
        if !native || attempt != 0 || metadata.status != 401 {
            return Ok(metadata);
        }
    }
    unreachable!()
}
fn media_request(path: &str, session: Option<String>, limit: u64) -> Request {
    let mut request = Request::download(path.into(), session, limit);
    request.route = Route::Media;
    request.timeout = None;
    request
}
fn strong_etag(value: &str) -> bool {
    value.len() >= 2
        && value.starts_with('"')
        && value.ends_with('"')
        && !value[1..value.len() - 1]
            .chars()
            .any(|c| c == '"' || c.is_control())
}
fn content_range(value: &str) -> Option<(u64, u64, u64)> {
    let (span, total) = value.strip_prefix("bytes ")?.split_once('/')?;
    let (start, end) = span.split_once('-')?;
    let values = (start.parse().ok()?, end.parse().ok()?, total.parse().ok()?);
    (values.0 <= values.1 && values.1 < values.2).then_some(values)
}
fn validate_range(
    metadata: &Metadata,
    start: u64,
    end: u64,
    length: u64,
    etag: &str,
) -> Result<(), String> {
    if metadata.status != 206 {
        return Err(format!(
            "The media source changed or ignored byte ranges (HTTP {}); reload it",
            metadata.status
        ));
    }
    if metadata.etag.as_deref() != Some(etag)
        || metadata.content_range.as_deref().and_then(content_range) != Some((start, end, length))
        || metadata
            .content_length
            .is_some_and(|n| n != end - start + 1)
    {
        return Err("The media representation or byte range changed; reload it".into());
    }
    Ok(())
}
impl Read for RangeSource {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if self.cancel.cancelled() {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "Media source cancelled",
            ));
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| io::Error::other("Media source state failed"))?;
        if let Some(error) = &state.failed {
            return Err(io::Error::other(error.clone()));
        }
        if output.is_empty() {
            return Ok(0);
        }
        let result = state.read(output, self.length, &self.cancel);
        if let Err(error) = &result {
            state.failed = Some(error.to_string());
        }
        result
    }
}
impl State {
    fn read(
        &mut self,
        output: &mut [u8],
        length: Option<u64>,
        cancel: &Cancel,
    ) -> io::Result<usize> {
        match &mut self.mode {
            Mode::Ranges {
                transport,
                etag,
                cache,
            } => {
                let length = length.unwrap();
                if self.position >= length {
                    return Ok(0);
                }
                let start = self.position / BLOCK * BLOCK;
                let index = cache.iter().position(|(offset, _)| *offset == start);
                let block = match index {
                    Some(index) => cache.remove(index).unwrap(),
                    None => {
                        let end = start.saturating_add(BLOCK - 1).min(length - 1);
                        let mut request = transport.request(end - start + 1);
                        request.range = Some(format!("bytes={start}-{end}"));
                        request.if_range = Some(etag.clone());
                        let mut bytes = Vec::with_capacity((end - start + 1) as usize);
                        let metadata = transport
                            .transfer(
                                &request,
                                cancel,
                                |metadata| validate_range(metadata, start, end, length, etag),
                                &mut bytes,
                            )
                            .map_err(io::Error::other)?;
                        if metadata.received != end - start + 1
                            || bytes.len() as u64 != end - start + 1
                        {
                            return Err(io::Error::other("The media range body was truncated"));
                        }
                        (start, bytes)
                    }
                };
                if cancel.cancelled() {
                    return Err(io::Error::new(
                        io::ErrorKind::ConnectionAborted,
                        "Media source cancelled",
                    ));
                }
                let offset = (self.position - start) as usize;
                let count = output.len().min(block.1.len() - offset);
                output[..count].copy_from_slice(&block.1[offset..offset + count]);
                self.position += count as u64;
                cache.push_back(block);
                while cache.len() > CACHE_BLOCKS {
                    cache.pop_front();
                }
                Ok(count)
            }
            Mode::Sequential {
                receiver,
                chunk,
                offset,
                ended,
            } => {
                while *offset >= chunk.len() && !*ended {
                    if cancel.cancelled() {
                        return Err(io::Error::new(
                            io::ErrorKind::ConnectionAborted,
                            "Media source cancelled",
                        ));
                    }
                    match receiver.recv_timeout(Duration::from_millis(50)) {
                        Ok(Packet::Data(bytes)) => {
                            *chunk = bytes;
                            *offset = 0;
                        }
                        Ok(Packet::Error(error)) => return Err(io::Error::other(error)),
                        Ok(Packet::End(received)) => {
                            if received != self.position || length.is_some_and(|n| n != received) {
                                return Err(io::Error::other(
                                    "The sequential media body was truncated",
                                ));
                            }
                            *ended = true;
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Err(_) => {
                            return Err(io::Error::other(
                                "The media stream closed without completing",
                            ));
                        }
                    }
                }
                if *ended {
                    return Ok(0);
                }
                let count = output.len().min(chunk.len() - *offset);
                output[..count].copy_from_slice(&chunk[*offset..*offset + count]);
                *offset += count;
                self.position += count as u64;
                Ok(count)
            }
        }
    }
}
impl Seek for RangeSource {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        if self.cancel.cancelled() {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "Media source cancelled",
            ));
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| io::Error::other("Media source state failed"))?;
        if let Some(error) = &state.failed {
            return Err(io::Error::other(error.clone()));
        }
        let target = match position {
            SeekFrom::Start(value) => value as i128,
            SeekFrom::Current(value) => state.position as i128 + value as i128,
            SeekFrom::End(value) => {
                self.length.ok_or_else(|| {
                    io::Error::new(io::ErrorKind::Unsupported, "Media length is unknown")
                })? as i128
                    + value as i128
            }
        };
        if target < 0 || target > u64::MAX as i128 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid media position",
            ));
        }
        let target = target as u64;
        if !self.seekable && target != state.position {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "This media response does not support seeking",
            ));
        }
        state.position = target;
        Ok(target)
    }
}
struct Pipe {
    send: mpsc::SyncSender<Packet>,
    cancel: Cancel,
}
fn send_packet(
    send: &mpsc::SyncSender<Packet>,
    mut value: Packet,
    cancel: &Cancel,
) -> io::Result<()> {
    loop {
        if cancel.cancelled() {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "Media source cancelled",
            ));
        }
        match send.try_send(value) {
            Ok(()) => return Ok(()),
            Err(mpsc::TrySendError::Full(returned)) => {
                value = returned;
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "Media source closed",
                ));
            }
        }
    }
}
impl Write for Pipe {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let count = bytes.len().min(PIPE_CHUNK);
        send_packet(
            &self.send,
            Packet::Data(bytes[..count].to_vec()),
            &self.cancel,
        )?;
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
#[cfg(test)]
mod tests;
